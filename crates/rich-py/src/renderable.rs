//! The renderable bridge: any Python object to a core renderable.
//!
//! Owner: the foundation. Every area uses it; no area needs to edit it.
//!
//! # What renders
//!
//! Rich renders a `str` (console markup), any object with
//! `__rich_console__(console, options)`, and any object whose `__rich__()`
//! returns one of those. The bindings add their own classes: each pyclass
//! that is a renderable implements [`AsRenderable`] and is registered with
//! [`add_renderable_class`], and from then on it renders wherever a
//! renderable is accepted: `Console.print`, table cells, panel children,
//! and anything an area builds with [`to_renderable`].
//!
//! # Making a pyclass renderable (the pattern for area modules)
//!
//! ```ignore
//! #[pyclass(name = "Rule", module = "rs_rich.rule")]
//! pub(crate) struct Rule { /* what the constructor was given */ }
//!
//! impl AsRenderable for Rule {
//!     fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
//!         Ok(Box::new(rich::Rule::new(&self.title)))
//!     }
//! }
//!
//! pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
//!     renderable::add_renderable_class::<Rule>(m)
//! }
//! ```
//!
//! `to_renderable` runs when the object is printed (not when it is built),
//! so an object can still change until then, as it can in Rich. A container
//! whose children are Python objects converts each child with
//! [`to_renderable`] (or wraps it in a [`PyRenderable`] when core needs a
//! `Send + Sync` child, as `Table` cells do). Conversion checks the nesting
//! depth ([`MAX_NESTING`]), so deep chains raise `RecursionError` instead of
//! overflowing the native stack.
//!
//! # Python code during a render
//!
//! Core's `Renderable` trait cannot fail, and a `__rich_console__` can. So a
//! render that may reach Python code runs inside [`scope`]: the first Python
//! exception raised under it is kept, the rest of the render is skipped
//! (no more Python code runs), and `scope` returns the exception once core
//! is done. `scope` also holds the [`Ambient`] a Python callback needs: the
//! Python `Console` to pass as `console`, and the `ConsoleOptions` fields
//! core does not track (`highlight`, `markup`, `encoding`, ...).
//!
//! Rendering happens with the GIL held, on the calling thread, and without
//! any console lock held (see `console.rs`), so Python code may call back
//! into the console (`console.width`, `console.render_lines`, even
//! `console.print`) without deadlocking. A render on another thread (a live
//! refresh) must enter its own `scope`.
//!
//! # Line endings
//!
//! Upstream's `Console.render` ends every line with `"\n"`; core's
//! renderables leave the last one off and `Console.print` adds it. The bridge
//! works in upstream's convention for Python objects ([`render_object`]) and
//! converts at the boundary ([`terminated`], [`unterminated`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use pyo3::exceptions::{PyRecursionError, PyRuntimeError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyString, PyTuple, PyType};
use pyo3::PyClass;

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::{Highlighter, Renderable};
use rich::segment::Segment as CoreSegment;
use rich::Text as CoreText;

use crate::errors::{MarkupError, NotRenderableError};
use crate::limits::MAX_NESTING;
use crate::protocol::{ConsoleOptions, Measurement, OptionsBase};
use crate::segment::Segment;

// ---------------------------------------------------------------------------
// The registry of renderable pyclasses

/// A pyclass that renders. See the module docs for the pattern.
pub(crate) trait AsRenderable: PyClass {
    /// Build the core renderable for this object, now.
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>>;
}

type Converter = for<'py> fn(&Bound<'py, PyAny>) -> PyResult<Box<dyn Renderable>>;

struct Entry {
    class: Py<PyType>,
    convert: Converter,
}

static REGISTRY: RwLock<Vec<Entry>> = RwLock::new(Vec::new());

fn convert<T: AsRenderable>(value: &Bound<'_, PyAny>) -> PyResult<Box<dyn Renderable>> {
    let object = value.extract::<PyRef<'_, T>>().map_err(PyErr::from)?;
    object.to_renderable(value.py())
}

/// Register `T` as a renderable class (without adding it to the module).
pub(crate) fn register_renderable<T: AsRenderable>(py: Python<'_>) {
    let class = py.get_type::<T>().unbind();
    let mut registry = REGISTRY
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !registry.iter().any(|entry| entry.class.is(&class)) {
        registry.push(Entry {
            class,
            convert: convert::<T>,
        });
    }
}

/// Add `T` to the module and register it as a renderable class.
pub(crate) fn add_renderable_class<T: AsRenderable>(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<T>()?;
    register_renderable::<T>(m.py());
    Ok(())
}

/// Whether a registered renderable leaves its last line open, as Rich's
/// `Emoji` (one segment) and a `Rule` with an `end` that is not a newline do:
/// nothing ends the line after it when it is printed or rendered.
pub(crate) fn ends_inline(value: &Bound<'_, PyAny>) -> bool {
    value.is_instance_of::<crate::color::emoji::Emoji>()
        || crate::renderables::rule_ends_inline(value)
}

/// The converter for a registered class's instance (or a subclass's).
fn lookup(value: &Bound<'_, PyAny>) -> Option<Converter> {
    let registry = REGISTRY
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let class = value.get_type();
    registry
        .iter()
        .find(|entry| class.is(entry.class.bind(value.py())))
        .or_else(|| {
            registry.iter().find(|entry| {
                value
                    .is_instance(entry.class.bind(value.py()))
                    .unwrap_or(false)
            })
        })
        .map(|entry| entry.convert)
}

/// Whether `value` is one of the bindings' own renderable classes.
pub(crate) fn is_registered(value: &Bound<'_, PyAny>) -> bool {
    lookup(value).is_some()
}

// ---------------------------------------------------------------------------
// Render scope: the ambient console, pending errors and nesting depth

/// What a Python callback needs that core's render call does not carry.
pub(crate) struct Ambient {
    /// The Python `Console` passed to `__rich_console__` and friends.
    pub(crate) console: Py<PyAny>,
    /// The `ConsoleOptions` fields core does not track.
    pub(crate) base: OptionsBase,
    /// The console's defaults, for strings rendered where `base` says `None`.
    pub(crate) emoji: bool,
    pub(crate) markup: bool,
    pub(crate) highlight: bool,
    /// The console's `highlighter` (`None`: Rich's `ReprHighlighter`, with
    /// any installed extension highlighters, which core applies).
    pub(crate) highlighter: Option<Py<PyAny>>,
    /// The console's default emoji variant.
    pub(crate) emoji_variant: Option<rich::emoji::EmojiVariant>,
}

thread_local! {
    static AMBIENT: RefCell<Vec<Rc<Ambient>>> = const { RefCell::new(Vec::new()) };
    static PENDING: RefCell<Option<PyErr>> = const { RefCell::new(None) };
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Pops the scope's ambient and restores the outer scope's pending error,
/// also when a panic unwinds through [`scope`].
struct ScopeGuard {
    saved: Option<PyErr>,
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        AMBIENT.with(|ambient| ambient.borrow_mut().pop());
        let saved = self.saved.take();
        PENDING.with(|pending| *pending.borrow_mut() = saved);
    }
}

/// Run a render that may call Python code. The first exception raised
/// inside core's render (by a `__rich_console__`, say) is returned once `f`
/// is done; after it, no more Python code runs in this render. Scopes nest:
/// an inner one (a `console.print` inside `__rich_console__`) keeps its own
/// errors.
pub(crate) fn scope<T>(ambient: Ambient, f: impl FnOnce() -> PyResult<T>) -> PyResult<T> {
    let saved = PENDING.with(|pending| pending.borrow_mut().take());
    AMBIENT.with(|stack| stack.borrow_mut().push(Rc::new(ambient)));
    let guard = ScopeGuard { saved };
    let result = f();
    let pending = PENDING.with(|pending| pending.borrow_mut().take());
    drop(guard);
    match pending {
        Some(error) => Err(error),
        None => result,
    }
}

/// The innermost scope's ambient.
pub(crate) fn ambient() -> PyResult<Rc<Ambient>> {
    AMBIENT
        .with(|stack| stack.borrow().last().cloned())
        .ok_or_else(|| {
            PyRuntimeError::new_err("rs_rich rendered a Python object outside a console")
        })
}

fn in_scope() -> bool {
    AMBIENT.with(|stack| !stack.borrow().is_empty())
}

fn has_pending() -> bool {
    PENDING.with(|pending| pending.borrow().is_some())
}

/// Report an error raised while rendering, from any area: the enclosing
/// [`scope`] raises it from the print (the first one wins).
pub(crate) fn report_error(py: Python<'_>, error: PyErr) {
    set_pending(py, error);
}

/// Keep `error` for the enclosing scope (the first one wins). Outside any
/// scope there is nobody to return it to, so it is reported as unraisable.
fn set_pending(py: Python<'_>, error: PyErr) {
    if !in_scope() {
        error.write_unraisable(py, None);
        return;
    }
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        if pending.is_none() {
            *pending = Some(error);
        }
    });
}

/// Return an error kept by a render that just finished inside core.
pub(crate) fn check_pending() -> PyResult<()> {
    match PENDING.with(|pending| pending.borrow_mut().take()) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// One level of render nesting; dropping it leaves the level.
pub(crate) struct Nesting(());

impl Nesting {
    /// Enter a level, or raise `RecursionError` past [`MAX_NESTING`].
    pub(crate) fn enter() -> PyResult<Nesting> {
        DEPTH.with(|depth| {
            if depth.get() >= MAX_NESTING {
                return Err(PyRecursionError::new_err(format!(
                    "maximum recursion depth exceeded: rs_rich renders at most {MAX_NESTING} \
                     nested renderables"
                )));
            }
            depth.set(depth.get() + 1);
            Ok(Nesting(()))
        })
    }
}

impl Drop for Nesting {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

// ---------------------------------------------------------------------------
// Conversions

const GIBBERISH: &str = "aihwerij235234ljsdnp34ksodfipwoe234234jlskjdf";

/// `rich.protocol.rich_cast`: follow `__rich__` until it returns something
/// without one (or a type already seen, which ends a loop).
pub(crate) fn rich_cast<'py>(value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let mut value = value.clone();
    let mut visited: Vec<Bound<'py, PyType>> = Vec::new();
    while !value.is_instance_of::<PyType>() {
        let Some(method) = value.getattr_opt("__rich__")? else {
            break;
        };
        // An object claiming every attribute (a mock) renders as its repr.
        if value.hasattr(GIBBERISH)? {
            return Ok(value.repr()?.into_any());
        }
        value = method.call0()?;
        let class = value.get_type();
        if visited.iter().any(|seen| seen.is(&class)) {
            break;
        }
        visited.push(class);
    }
    Ok(value)
}

/// `rich.protocol.is_renderable`: a `str`, or an object with `__rich__` or
/// `__rich_console__`, or one of the bindings' classes.
pub(crate) fn is_renderable(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(value.is_instance_of::<PyString>()
        || is_registered(value)
        || value.hasattr("__rich__")?
        || value.hasattr("__rich_console__")?)
}

/// Whether `print` pretty-prints `value` (`rich.pretty.is_expandable`):
/// a container, a dataclass, an attrs object or one with `__rich_repr__`.
pub(crate) fn is_expandable(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    static CONTAINERS: PyOnceLock<Py<PyTuple>> = PyOnceLock::new();
    let py = value.py();
    if value.is_instance_of::<PyType>() {
        return Ok(false);
    }
    let containers = CONTAINERS.get_or_try_init(py, || {
        let builtins = py.import("builtins")?;
        let collections = py.import("collections")?;
        let classes = [
            py.import("os")?.getattr("_Environ")?,
            py.import("array")?.getattr("array")?,
            collections.getattr("defaultdict")?,
            collections.getattr("Counter")?,
            collections.getattr("deque")?,
            builtins.getattr("dict")?,
            collections.getattr("UserDict")?,
            builtins.getattr("frozenset")?,
            builtins.getattr("list")?,
            collections.getattr("UserList")?,
            builtins.getattr("set")?,
            builtins.getattr("tuple")?,
            py.import("types")?.getattr("MappingProxyType")?,
        ];
        Ok::<_, PyErr>(PyTuple::new(py, classes)?.unbind())
    })?;
    if value.is_instance(containers.bind(py).as_any())? {
        return Ok(true);
    }
    if py
        .import("dataclasses")?
        .call_method1("is_dataclass", (value,))?
        .is_truthy()?
    {
        return Ok(true);
    }
    if value.hasattr("__rich_repr__")? {
        return Ok(true);
    }
    value.get_type().hasattr("__attrs_attrs__")
}

/// `Console.render_str` without a console: emoji codes, console markup
/// (raising `MarkupError`, as Rich does) and the repr highlighter. As in
/// Rich, highlighting builds a fresh `Text` and copies only the spans.
pub(crate) fn render_str(
    content: &str,
    emoji: bool,
    markup: bool,
    highlight: bool,
) -> PyResult<CoreText> {
    render_str_with(content, emoji, markup, highlight, &[], None, None)
}

/// [`render_str`] with a console's installed extension highlighters, which
/// run before `ReprHighlighter`, as core's `Console::decorate_with_repr` does,
/// its `highlighter` (`None`: `ReprHighlighter`), which replaces
/// `ReprHighlighter` as `Console(highlighter=...)` does in Rich, and its
/// default emoji variant (which, as upstream's `markup.render`, applies only
/// to markup without tags).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_str_with(
    content: &str,
    emoji: bool,
    markup: bool,
    highlight: bool,
    extra: &[Box<dyn rich::Highlighter + Send>],
    highlighter: Option<&Bound<'_, PyAny>>,
    variant: Option<rich::emoji::EmojiVariant>,
) -> PyResult<CoreText> {
    let content = if !emoji {
        content.to_string()
    } else if markup && content.contains('[') {
        rich::emoji::replace(content)
    } else {
        rich::emoji::replace_with_variant(content, variant)
    };
    let text = if markup {
        CoreText::from_markup(&content).map_err(|e| MarkupError::new_err(e.to_string()))?
    } else {
        CoreText::new(content)
    };
    if !highlight {
        return Ok(text);
    }
    let mut highlighted = CoreText::new(text.plain());
    for highlighter in extra {
        highlighter.highlight(&mut highlighted);
    }
    match highlighter {
        Some(highlighter) => highlighted = crate::code::highlight_with(highlighter, highlighted)?,
        None => rich::ReprHighlighter::new().highlight(&mut highlighted),
    }
    for span in text.spans() {
        highlighted.stylize(span.style.clone(), span.start, span.end);
    }
    Ok(highlighted)
}

/// A `str` child of a container: markup, emoji and highlighting apply when
/// it renders, as upstream's `Console.render_str` does for `str` children.
/// `highlight` overrides the console's, as a container's options do upstream
/// (`Panel` renders its child with `highlight=False`).
struct MarkupStr {
    markup: String,
    highlight: Option<bool>,
}

impl MarkupStr {
    /// The text: core's `render_str`, or, under a console with its own
    /// `highlighter`, the markup highlighted by it.
    fn text(&self, console: &CoreConsole) -> CoreText {
        let custom = Python::attach(|py| {
            let ambient = AMBIENT.with(|stack| stack.borrow().last().cloned())?;
            let highlighter = ambient.highlighter.as_ref()?;
            let highlight = self
                .highlight
                .or(ambient.base.highlight)
                .unwrap_or_else(|| console.highlight());
            let result = render_str_with(
                &self.markup,
                console.emoji(),
                ambient.base.markup.unwrap_or(ambient.markup),
                highlight,
                &[],
                Some(highlighter.bind(py)),
                ambient.emoji_variant,
            );
            match result {
                Ok(text) => Some(text),
                Err(error) => {
                    set_pending(py, error);
                    Some(CoreText::new(""))
                }
            }
        });
        custom.unwrap_or_else(|| console.render_str(&self.markup, self.highlight))
    }
}

impl Renderable for MarkupStr {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.text(console).rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        self.text(console).measure(console, options)
    }
}

/// Build the core renderable for a Python object that a container holds.
///
/// A `str` renders as markup with `highlight` (`None`: the console's). One
/// of the bindings' classes converts now. Any other renderable (Rich's
/// protocol) becomes a [`PyRenderable`] that calls back into Python when
/// it renders, with `options.highlight` set to `highlight` when that is
/// `Some`. Pass what the Rich class passes to its children: `Some(false)`
/// for `Panel` and `Table`, `None` for containers that leave it alone.
/// Anything else is Rich's `NotRenderableError`.
pub(crate) fn to_renderable(
    value: &Bound<'_, PyAny>,
    highlight: Option<bool>,
) -> PyResult<Box<dyn Renderable>> {
    if let Ok(string) = value.cast::<PyString>() {
        let markup = string.to_cow()?.into_owned();
        // Rich raises on bad markup; core's lenient path would print it.
        CoreText::from_markup(&markup).map_err(|e| MarkupError::new_err(e.to_string()))?;
        return Ok(Box::new(MarkupStr { markup, highlight }));
    }
    let cast = rich_cast(value)?;
    if let Some(convert) = lookup(&cast) {
        let _nesting = Nesting::enter()?;
        return convert(&cast);
    }
    // An object whose `__rich__` returns a `str` stays lazy: Rich measures
    // it before casting, so it takes the full width.
    let renders = cast.is_instance_of::<PyString>()
        || (!cast.is_instance_of::<PyType>() && cast.hasattr("__rich_console__")?);
    if renders {
        return Ok(Box::new(PyRenderable::with_highlight(
            value.clone().unbind(),
            highlight,
        )));
    }
    Err(not_renderable(&cast)?)
}

pub(crate) fn not_renderable(value: &Bound<'_, PyAny>) -> PyResult<PyErr> {
    Ok(NotRenderableError::new_err(format!(
        "Unable to render {}; A str, Segment or object with __rich_console__ method is required",
        value.repr()?
    )))
}

// ---------------------------------------------------------------------------
// Rendering Python objects

/// Core's convention from upstream's: drop the newline that ends the last
/// line. A render that was only a newline is one blank line, which core
/// writes as a lone empty segment.
pub(crate) fn unterminated(segments: Vec<CoreSegment>) -> Vec<CoreSegment> {
    let mut segments = crate::segment::split_newlines(segments);
    if segments.is_empty() {
        return segments;
    }
    if let Some(last) = segments.last_mut() {
        if !last.control && last.text.ends_with('\n') {
            last.text.pop();
            if last.text.is_empty() {
                segments.pop();
            }
        }
    }
    if segments.is_empty() {
        segments.push(CoreSegment::new("", None));
    }
    segments
}

/// Upstream's convention from core's: a render that produced anything ends
/// its last line with a newline.
pub(crate) fn terminated(mut segments: Vec<CoreSegment>) -> Vec<CoreSegment> {
    if !segments.is_empty() {
        segments.push(CoreSegment::line());
    }
    segments
}

/// `Console.render`: render any renderable Python object to segments, in
/// upstream's convention (every line ends with a newline). Call inside a
/// [`scope`].
pub(crate) fn render_object(
    value: &Bound<'_, PyAny>,
    console: &CoreConsole,
    options: &CoreOptions,
) -> PyResult<Vec<CoreSegment>> {
    if options.max_width < 1 {
        return Ok(Vec::new());
    }
    let _nesting = Nesting::enter()?;
    let py = value.py();
    let value = rich_cast(value)?;
    if let Some(convert) = lookup(&value) {
        let renderable = convert(&value)?;
        let segments = renderable.rich_render(console, options);
        check_pending()?;
        if ends_inline(&value) {
            let mut segments = segments;
            while segments
                .last()
                .is_some_and(|last| last.text.is_empty() && !last.control)
            {
                segments.pop();
            }
            return Ok(segments);
        }
        return Ok(terminated(segments));
    }
    if !value.is_instance_of::<PyType>() {
        if let Some(method) = value.getattr_opt("__rich_console__")? {
            let ambient = ambient()?;
            let python_options = ConsoleOptions::from_core(options, &ambient.base);
            let result = method.call1((ambient.console.bind(py), python_options))?;
            let items = result.try_iter().map_err(|_| {
                NotRenderableError::new_err(format!(
                    "object {} is not renderable",
                    result.repr().map(|r| r.to_string()).unwrap_or_default()
                ))
            })?;
            let mut child_options = options.clone();
            child_options.height = None;
            let mut segments = Vec::new();
            for item in items {
                let item = item?;
                if let Ok(segment) = item.extract::<PyRef<'_, Segment>>() {
                    segments.push(segment.to_core());
                } else {
                    segments.extend(render_object(&item, console, &child_options)?);
                }
            }
            return Ok(segments);
        }
    }
    if let Ok(string) = value.cast::<PyString>() {
        let ambient = ambient()?;
        let text = render_str_with(
            string.to_cow()?.as_ref(),
            ambient.emoji,
            ambient.base.markup.unwrap_or(ambient.markup),
            ambient.base.highlight.unwrap_or(ambient.highlight),
            &[],
            ambient.highlighter.as_ref().map(|h| h.bind(py)),
            ambient.emoji_variant,
        )?;
        return Ok(terminated(text.rich_render(console, options)));
    }
    Err(not_renderable(&value)?)
}

/// `Measurement.get` for any renderable Python object. Call inside a
/// [`scope`].
pub(crate) fn measure_object(
    value: &Bound<'_, PyAny>,
    console: &CoreConsole,
    options: &CoreOptions,
) -> PyResult<CoreMeasurement> {
    let max_width = options.max_width;
    if max_width < 1 {
        return Ok(CoreMeasurement::new(0, 0));
    }
    let _nesting = Nesting::enter()?;
    let py = value.py();
    if let Ok(string) = value.cast::<PyString>() {
        let ambient = ambient()?;
        let markup = ambient.base.markup.unwrap_or(ambient.markup);
        let text = render_str(string.to_cow()?.as_ref(), ambient.emoji, markup, false)?;
        return Ok(CoreMeasurement::get(console, options, &text));
    }
    let value = rich_cast(value)?;
    if let Some(convert) = lookup(&value) {
        let renderable = convert(&value)?;
        let measurement = CoreMeasurement::get(console, options, renderable.as_ref());
        check_pending()?;
        return Ok(measurement);
    }
    if !is_renderable(&value)? {
        return Err(NotRenderableError::new_err(format!(
            "Unable to get render width for {}; a str, Segment, or object with __rich_console__ \
             method is required",
            value.repr()?
        )));
    }
    let method = if value.is_instance_of::<PyType>() {
        None
    } else {
        value.getattr_opt("__rich_measure__")?
    };
    let Some(method) = method else {
        return Ok(CoreMeasurement::new(0, max_width));
    };
    let ambient = ambient()?;
    let python_options = ConsoleOptions::from_core(options, &ambient.base);
    let measured =
        Measurement::extract_any(&method.call1((ambient.console.bind(py), python_options))?)?;
    let normalized = measured.to_core().normalize().with_maximum(max_width);
    if normalized.maximum < 1 {
        return Ok(CoreMeasurement::new(0, 0));
    }
    Ok(normalized.normalize())
}

/// A Python object inside a core renderable: it converts (or calls its
/// `__rich_console__`) each time core renders or measures it. It is `Send +
/// Sync`, so it can be a `Table` cell. Errors go to the enclosing [`scope`].
pub(crate) struct PyRenderable {
    object: Py<PyAny>,
    highlight: Option<bool>,
}

impl PyRenderable {
    pub(crate) fn new(object: Py<PyAny>) -> PyRenderable {
        PyRenderable {
            object,
            highlight: None,
        }
    }

    /// Render with `options.highlight` set to `highlight`, as a Rich
    /// container that passes `highlight=` to its children does (`Panel` and
    /// `Table` pass `False`). Core's options have no `highlight` to carry it.
    pub(crate) fn with_highlight(object: Py<PyAny>, highlight: Option<bool>) -> PyRenderable {
        PyRenderable { object, highlight }
    }

    /// As a shared core renderable, for `Cell::Renderable` and the like.
    pub(crate) fn shared(
        object: Py<PyAny>,
        highlight: Option<bool>,
    ) -> Arc<dyn Renderable + Send + Sync> {
        Arc::new(PyRenderable::with_highlight(object, highlight))
    }

    /// Run `f` under this object's highlight override, if it has one.
    fn overridden<T>(&self, py: Python<'_>, f: impl FnOnce() -> PyResult<T>) -> PyResult<T> {
        let Some(highlight) = self.highlight else {
            return f();
        };
        let outer = ambient()?;
        let mut base = outer.base.clone();
        base.highlight = Some(highlight);
        let inner = Ambient {
            console: outer.console.clone_ref(py),
            base,
            emoji: outer.emoji,
            markup: outer.markup,
            highlight: outer.highlight,
            highlighter: outer.highlighter.as_ref().map(|h| h.clone_ref(py)),
            emoji_variant: outer.emoji_variant,
        };
        struct Pop;
        impl Drop for Pop {
            fn drop(&mut self) {
                AMBIENT.with(|stack| stack.borrow_mut().pop());
            }
        }
        AMBIENT.with(|stack| stack.borrow_mut().push(Rc::new(inner)));
        let _pop = Pop;
        f()
    }
}

impl Renderable for PyRenderable {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        Python::attach(|py| {
            if has_pending() {
                return Vec::new();
            }
            let rendered =
                self.overridden(py, || render_object(self.object.bind(py), console, options));
            match rendered {
                Ok(segments) => unterminated(segments),
                Err(error) => {
                    set_pending(py, error);
                    Vec::new()
                }
            }
        })
    }

    /// Upstream's `getattr(renderable, "vertical", None)`: a table cell
    /// holding an `Align(vertical=...)` (or any object with a `vertical`)
    /// aligns by it.
    fn vertical(&self) -> Option<rich::align::VerticalAlign> {
        Python::attach(|py| {
            let value = self.object.bind(py).getattr_opt("vertical").ok()??;
            match value.extract::<String>().ok()?.as_str() {
                "top" => Some(rich::align::VerticalAlign::Top),
                "middle" => Some(rich::align::VerticalAlign::Middle),
                "bottom" => Some(rich::align::VerticalAlign::Bottom),
                _ => None,
            }
        })
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        Python::attach(|py| {
            if has_pending() {
                return CoreMeasurement::new(0, 0);
            }
            let measured = self.overridden(py, || {
                measure_object(self.object.bind(py), console, options)
            });
            match measured {
                Ok(measurement) => measurement,
                Err(error) => {
                    set_pending(py, error);
                    CoreMeasurement::new(0, 0)
                }
            }
        })
    }
}
