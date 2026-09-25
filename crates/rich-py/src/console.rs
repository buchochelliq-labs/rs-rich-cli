//! `rich.console`: the `Console`, its `Capture` and `ThemeContext`.
//!
//! Owner: the foundation. Areas that add Console methods Rich has
//! (`status`, `pager`, `screen`, `print_exception`) implement them in their
//! own module; the methods here only forward to them.
//!
//! # Threads, locks and Python code
//!
//! A `Console` is usable from any thread. Its settings, theme stack, record
//! and capture buffers live in `state`, a mutex that is only ever held
//! around pure Rust work, never while Python code runs, so a
//! `__rich_console__` may call back into the console without deadlocking.
//!
//! A print therefore works on a *snapshot*: it copies the settings and the
//! top theme out of `state`, builds a core console from them, and renders
//! with no lock held (Python protocol objects are called during that render,
//! on this thread, with the GIL). Only the rendered segments go back under
//! the lock, to be recorded or captured. `printer` then names the thread
//! writing to `file` (0: none), so prints from different threads never
//! interleave, and a print from inside `file.write` raises instead of
//! waiting for itself.

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use pyo3::exceptions::{
    PyIndexError, PyKeyError, PyNotImplementedError, PyRuntimeError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple, PyType};
use pyo3::{PyTraverseError, PyVisit};

use rich::align::{Align, HorizontalAlign};
use rich::color::ColorSystem;
use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::log_render::LogRender;
use rich::protocol::{Highlighter, Renderable};
use rich::segment::Segment as CoreSegment;
use rich::theme::Theme as CoreTheme;
use rich::{Control, Overflow, Rule, Style as CoreStyle, StyleType, Text as CoreText};

use crate::convert;
use crate::errors::{CaptureError, MissingStyle, ThemeStackError};
use crate::limits::MAX_CONSOLE_WIDTH;
use crate::protocol::{self, ConsoleOptions, Measurement, OptionsBase};
use crate::renderable::{self, Ambient, PyRenderable};
use crate::segment;
use crate::style::{style_type, Style};
use crate::terminal_theme::TerminalTheme;
use crate::text::Text;
use crate::theme::Theme;

/// Everything a print reads from the console, copied out of `state`.
#[derive(Clone)]
struct Settings {
    color_system: Option<ColorSystem>,
    force_terminal: Option<bool>,
    is_terminal: bool,
    no_color: bool,
    width: usize,
    height: usize,
    emoji: bool,
    highlight: bool,
    markup: bool,
    safe_box: bool,
    legacy_windows: bool,
    soft_wrap: bool,
    tab_size: usize,
    quiet: bool,
    stderr: bool,
    record: bool,
    force_interactive: Option<bool>,
    style: Option<StyleType>,
    log_time: bool,
    emoji_variant: Option<rich::emoji::EmojiVariant>,
}

/// Output held back by `with console:`, `capture()` or `begin_capture()`
/// on one thread (Rich's thread-local buffer).
struct ThreadBuffer {
    thread: u64,
    depth: usize,
    segments: Vec<CoreSegment>,
}

struct ConsoleState {
    settings: Settings,
    /// Rich's theme stack: the console's theme first, pushed themes above.
    themes: Vec<CoreTheme>,
    recorded: Vec<CoreSegment>,
    buffers: Vec<ThreadBuffer>,
    log_render: LogRender,
    file: Option<Py<PyAny>>,
    get_datetime: Option<Py<PyAny>>,
    get_time: Option<Py<PyAny>>,
    log_time_format: Option<Py<PyAny>>,
    is_alt_screen: bool,
    /// `Console(highlighter=...)`; `None` is Rich's `ReprHighlighter`.
    highlighter: Option<Py<PyAny>>,
    /// Rich's `_render_hooks`: objects with `process_renderables`.
    render_hooks: Vec<Py<PyAny>>,
    /// Rich's `_live_stack`: the running `Live` displays.
    live_stack: Vec<Py<PyAny>>,
}

/// `rich.console.Console`: renders through core `rich` and writes the result
/// to `file` (default: `sys.stdout`, or `sys.stderr` with `stderr=True`, at
/// print time).
#[pyclass(name = "Console", module = "rs_rich.console", frozen)]
pub(crate) struct Console {
    state: Mutex<ConsoleState>,
    printer: Mutex<u64>,
    printed: Condvar,
}

/// A number for the current thread, never 0 (0 means "nobody").
fn thread_number() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    thread_local! {
        static NUMBER: u64 = NEXT.fetch_add(1, Ordering::Relaxed);
    }
    NUMBER.with(|number| *number)
}

/// A lock whose holder panicked still guards consistent data here: every
/// critical section is a single core call.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Held while a print writes; frees the console when dropped.
struct Printing<'a> {
    console: &'a Console,
}

impl Drop for Printing<'_> {
    fn drop(&mut self) {
        *lock(&self.console.printer) = 0;
        self.console.printed.notify_all();
    }
}

fn color_system_name(system: Option<ColorSystem>) -> Option<&'static str> {
    system.map(|system| match system {
        ColorSystem::Standard => "standard",
        ColorSystem::EightBit => "256",
        ColorSystem::Truecolor => "truecolor",
        ColorSystem::Windows => "windows",
    })
}

fn check_width(width: usize) -> PyResult<usize> {
    if width > MAX_CONSOLE_WIDTH {
        return Err(PyValueError::new_err(format!(
            "width must be at most {MAX_CONSOLE_WIDTH}, got {width}"
        )));
    }
    Ok(width)
}

fn is_dumb_term() -> bool {
    std::env::var("TERM")
        .map(|term| matches!(term.to_lowercase().as_str(), "dumb" | "unknown"))
        .unwrap_or(false)
}

/// A print's copy of the console: settings, the top theme and the file.
pub(crate) struct Snapshot {
    settings: Settings,
    theme: CoreTheme,
    file: Option<Py<PyAny>>,
    extensions: Option<std::sync::Arc<crate::plugins::Installed>>,
    highlighter: Option<Py<PyAny>>,
    hooks: Vec<Py<PyAny>>,
    /// Rich's `options.ascii_only`: the file's encoding is not UTF.
    ascii_only: bool,
}

impl Snapshot {
    /// The core console a print renders with. `emoji` and `highlight` are
    /// the print's own (its arguments, else the console's).
    fn core(&self, emoji: bool, highlight: bool) -> CoreConsole {
        let s = &self.settings;
        let mut console = CoreConsole::builder()
            .force_terminal(s.is_terminal)
            .color_system(s.color_system)
            .width(s.width)
            .height(s.height)
            .no_color(s.no_color)
            .emoji(emoji)
            .highlight(highlight)
            .safe_box(s.safe_box)
            .legacy_windows(s.legacy_windows)
            .tab_size(s.tab_size)
            .emoji_variant(s.emoji_variant)
            .ascii_only(self.ascii_only)
            .theme(self.theme.clone())
            .build();
        if let Some(extensions) = &self.extensions {
            extensions.apply(&mut console);
        }
        console
    }

    fn default_core(&self) -> CoreConsole {
        self.core(self.settings.emoji, self.settings.highlight)
    }

    fn target<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        target(py, self.file.as_ref(), self.settings.stderr)
    }

    fn encoding(&self, py: Python<'_>) -> PyResult<String> {
        let file = self.target(py)?;
        let encoding = match file.getattr_opt("encoding")? {
            Some(encoding) if encoding.is_truthy()? => encoding.str()?.to_string(),
            _ => "utf-8".to_string(),
        };
        Ok(encoding.to_lowercase())
    }

    fn base(&self, py: Python<'_>) -> PyResult<OptionsBase> {
        Ok(OptionsBase {
            size: (self.settings.width, self.settings.height),
            legacy_windows: self.settings.legacy_windows,
            is_terminal: self.settings.is_terminal,
            encoding: self.encoding(py)?,
            max_height: self.settings.height,
            highlight: None,
            markup: None,
        })
    }

    fn ambient(&self, console: &Bound<'_, Console>, base: OptionsBase) -> Ambient {
        let py = console.py();
        Ambient {
            console: console.clone().into_any().unbind(),
            base,
            emoji: self.settings.emoji,
            markup: self.settings.markup,
            highlight: self.settings.highlight,
            highlighter: self.highlighter.as_ref().map(|h| h.clone_ref(py)),
            emoji_variant: self.settings.emoji_variant,
        }
    }
}

fn target<'py>(
    py: Python<'py>,
    file: Option<&Py<PyAny>>,
    stderr: bool,
) -> PyResult<Bound<'py, PyAny>> {
    match file {
        Some(file) => Ok(file.bind(py).clone()),
        None => py
            .import("sys")?
            .getattr(if stderr { "stderr" } else { "stdout" }),
    }
}

/// Something `print` renders, after Rich's `_collect_renderables`.
enum Item {
    /// A core renderable (one of the bindings' classes, `Align`).
    Core(Box<dyn Renderable>),
    /// Strings and `Text`s joined with `sep`, ending with `end`.
    Joined { text: CoreText, end: String },
    /// A Python object with `__rich_console__`.
    Object(Py<PyAny>),
    /// Segments ready to print (`NewLine`).
    Raw(Vec<CoreSegment>),
    /// A core renderable whose last line is not ended, as Rich prints a
    /// renderable that yields one segment (`Emoji`): no newline follows it.
    Inline(Box<dyn Renderable>),
}

/// Joined text and its `end`, as one core renderable for `Align`: Rich's joined `Text` renders its `end` after the last line,
/// inside whatever wraps it.
struct TextWithEnd {
    text: CoreText,
    end: String,
}

impl Renderable for TextWithEnd {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let mut segments = self.text.rich_render(console, options);
        if !self.end.is_empty() {
            segments.push(CoreSegment::new(self.end.clone(), None));
        }
        renderable::unterminated(segments)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> rich::measure::Measurement {
        self.text.measure(console, options)
    }
}

/// Raw segments as a core renderable, for `Align`.
struct RawSegments(Vec<CoreSegment>);

impl Renderable for RawSegments {
    fn rich_render(&self, _console: &CoreConsole, _options: &CoreOptions) -> Vec<CoreSegment> {
        renderable::unterminated(self.0.clone())
    }
}

/// A renderable in `Console.log`'s message cell. Core's log table takes
/// `Send + Sync` cells; this one is built, rendered and dropped by one call
/// on one thread, holding the GIL, and never leaves it.
struct ThreadBound(Box<dyn Renderable>);

// SAFETY: see above: the table owning a `ThreadBound` is local to
// `Console.log` and is never moved to or shared with another thread.
unsafe impl Send for ThreadBound {}
// SAFETY: as for `Send`.
unsafe impl Sync for ThreadBound {}

impl Renderable for ThreadBound {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.0.rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> rich::measure::Measurement {
        self.0.measure(console, options)
    }
}

impl Item {
    fn into_box(self) -> Box<dyn Renderable> {
        match self {
            Item::Core(renderable) => renderable,
            Item::Joined { text, end } => Box::new(TextWithEnd { text, end }),
            Item::Object(object) => Box::new(PyRenderable::new(object)),
            Item::Raw(segments) => Box::new(RawSegments(segments)),
            Item::Inline(renderable) => renderable,
        }
    }

    /// Render in upstream's convention (lines end with a newline).
    fn render(
        &self,
        py: Python<'_>,
        console: &CoreConsole,
        options: &CoreOptions,
    ) -> PyResult<Vec<CoreSegment>> {
        if options.max_width < 1 {
            return Ok(Vec::new());
        }
        match self {
            Item::Core(renderable) => {
                // What core's print does before rendering (its
                // `render_segments_with`, without the crop).
                let mut options = options.clone();
                if options.justify == rich::Justify::Default && renderable.fit_to_measurement() {
                    let measurement = renderable.measure(console, &options);
                    options.max_width = measurement.maximum.min(options.max_width).max(1);
                }
                let segments = renderable.rich_render(console, &options);
                renderable::check_pending()?;
                Ok(renderable::terminated(segments))
            }
            Item::Joined { text, end } => {
                let mut segments = text.rich_render(console, options);
                if !end.is_empty() {
                    segments.push(CoreSegment::new(end.clone(), None));
                }
                Ok(segments)
            }
            Item::Object(object) => Ok(segment::split_newlines(renderable::render_object(
                object.bind(py),
                console,
                options,
            )?)),
            Item::Raw(segments) => Ok(segments.clone()),
            Item::Inline(renderable) => {
                let segments = renderable.rich_render(console, options);
                renderable::check_pending()?;
                Ok(segments)
            }
        }
    }
}

/// An [`Item`] a render hook sees: Rich passes hooks the renderables a
/// print collected, and prints the list they return.
#[pyclass(name = "_PrintItem", module = "rs_rich.console", unsendable)]
pub(crate) struct PrintItem {
    item: Rc<Item>,
}

/// An item as a core renderable (what the hook's list holds when printed
/// or put in a container).
struct ItemRef(Rc<Item>);

impl Renderable for ItemRef {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        match &*self.0 {
            Item::Core(renderable) | Item::Inline(renderable) => {
                renderable.rich_render(console, options)
            }
            Item::Joined { text, end } => {
                let mut segments = text.rich_render(console, options);
                if !end.is_empty() {
                    segments.push(CoreSegment::new(end.clone(), None));
                }
                renderable::unterminated(segments)
            }
            Item::Object(object) => Python::attach(|py| {
                PyRenderable::new(object.clone_ref(py)).rich_render(console, options)
            }),
            Item::Raw(segments) => renderable::unterminated(segments.clone()),
        }
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> rich::measure::Measurement {
        match &*self.0 {
            Item::Core(renderable) | Item::Inline(renderable) => {
                renderable.measure(console, options)
            }
            Item::Joined { text, .. } => text.measure(console, options),
            Item::Object(object) => Python::attach(|py| {
                PyRenderable::new(object.clone_ref(py)).measure(console, options)
            }),
            Item::Raw(_) => rich::measure::Measurement::new(0, options.max_width),
        }
    }
}

impl renderable::AsRenderable for PrintItem {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(ItemRef(Rc::clone(&self.item))))
    }
}

/// Rich's `for hook in self._render_hooks: renderables =
/// hook.process_renderables(renderables)`. What a hook adds is collected
/// as `print` collects its objects.
fn apply_hooks(
    py: Python<'_>,
    hooks: &[Py<PyAny>],
    items: Vec<Item>,
    switches: &Switches,
) -> PyResult<Vec<Rc<Item>>> {
    let mut items: Vec<Rc<Item>> = items.into_iter().map(Rc::new).collect();
    for hook in hooks {
        let list = PyList::empty(py);
        for item in &items {
            list.append(Bound::new(
                py,
                PrintItem {
                    item: Rc::clone(item),
                },
            )?)?;
        }
        let returned = hook.bind(py).call_method1("process_renderables", (list,))?;
        let mut next = Vec::new();
        for object in returned.try_iter()? {
            let object = object?;
            if let Ok(item) = object.cast::<PrintItem>() {
                next.push(Rc::clone(&item.borrow().item));
            } else {
                let collected = collect(
                    std::slice::from_ref(&object),
                    " ",
                    "\n",
                    None,
                    switches,
                    false,
                )?;
                next.extend(collected.into_iter().map(Rc::new));
            }
        }
        items = next;
    }
    Ok(items)
}

/// How many lines Python's `str.splitlines` finds in the segments' text.
fn line_count(segments: &[CoreSegment]) -> usize {
    let mut lines = 0;
    let mut open = false;
    let text: String = segments.iter().map(|s| s.text.as_str()).collect();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            lines += 1;
            open = false;
        } else {
            open = true;
        }
    }
    lines + usize::from(open)
}

/// `print`'s arguments, after defaults.
struct PrintArgs<'a> {
    sep: &'a str,
    end: &'a str,
    style: Option<StyleType>,
    justify: Option<String>,
    overflow: Option<Overflow>,
    no_wrap: Option<bool>,
    emoji: Option<bool>,
    markup: Option<bool>,
    highlight: Option<bool>,
    width: Option<usize>,
    height: Option<usize>,
    crop: bool,
    soft_wrap: Option<bool>,
    new_line_start: bool,
}

impl Default for PrintArgs<'_> {
    fn default() -> Self {
        PrintArgs {
            sep: " ",
            end: "\n",
            style: None,
            justify: None,
            overflow: None,
            no_wrap: None,
            emoji: None,
            markup: None,
            highlight: None,
            width: None,
            height: None,
            crop: true,
            soft_wrap: None,
            new_line_start: false,
        }
    }
}

/// The effective `emoji`, `markup` and `highlight` of a call.
struct Switches {
    emoji: bool,
    markup: bool,
    highlight: bool,
    extensions: Option<std::sync::Arc<crate::plugins::Installed>>,
    highlighter: Option<Py<PyAny>>,
    emoji_variant: Option<rich::emoji::EmojiVariant>,
}

/// Rich's `_collect_renderables`: strings and `Text`s (and anything printed
/// as its `str`) join into one `Text` with `sep`; each other renderable
/// stands alone. `justify` left, center or right aligns each with `Align`.
fn collect(
    objects: &[Bound<'_, PyAny>],
    sep: &str,
    end: &str,
    justify: Option<&str>,
    switches: &Switches,
    align: bool,
) -> PyResult<Vec<Item>> {
    let alignment = match justify {
        Some("left") if align => Some(HorizontalAlign::Left),
        Some("center") if align => Some(HorizontalAlign::Center),
        Some("right") if align => Some(HorizontalAlign::Right),
        _ => None,
    };
    let aligned = |item: Item| match alignment {
        None => item,
        Some(HorizontalAlign::Left) => Item::Core(Box::new(Align::left(item.into_box()))),
        Some(HorizontalAlign::Center) => Item::Core(Box::new(Align::center(item.into_box()))),
        Some(HorizontalAlign::Right) => Item::Core(Box::new(Align::right(item.into_box()))),
    };
    let text_justify = convert::justify(justify)?;
    let mut items = Vec::new();
    let mut texts: Vec<CoreText> = Vec::new();
    let flush = |texts: &mut Vec<CoreText>, items: &mut Vec<Item>| {
        if !texts.is_empty() {
            let separator = CoreText::new(sep).justify(text_justify);
            items.push(aligned(Item::Joined {
                text: separator.join(texts),
                end: end.to_string(),
            }));
            texts.clear();
        }
    };
    for object in objects {
        let object = renderable::rich_cast(object)?;
        if let Ok(string) = object.cast::<PyString>() {
            let extra = switches
                .extensions
                .as_ref()
                .map(|extensions| extensions.highlighters())
                .unwrap_or_default();
            texts.push(renderable::render_str_with(
                string.to_cow()?.as_ref(),
                switches.emoji,
                switches.markup,
                switches.highlight,
                &extra,
                switches.highlighter.as_ref().map(|h| h.bind(object.py())),
                switches.emoji_variant,
            )?);
        } else if let Ok(text) = object.extract::<PyRef<'_, Text>>() {
            texts.push(text.inner.clone());
        } else if renderable::ends_inline(&object) {
            flush(&mut texts, &mut items);
            let item = Item::Inline(renderable::to_renderable(&object, None)?);
            items.push(aligned(item));
        } else if renderable::is_registered(&object) {
            flush(&mut texts, &mut items);
            let item = Item::Core(renderable::to_renderable(&object, None)?);
            items.push(aligned(item));
        } else if !object.is_instance_of::<PyType>() && object.hasattr("__rich_console__")? {
            flush(&mut texts, &mut items);
            items.push(aligned(Item::Object(object.unbind())));
        } else if renderable::is_expandable(&object)? {
            flush(&mut texts, &mut items);
            let item = Item::Core(crate::code::pretty_for_print(&object, switches.highlight)?);
            items.push(aligned(item));
        } else {
            let mut text = CoreText::new(object.str()?.to_cow()?.as_ref());
            if switches.highlight {
                match &switches.highlighter {
                    Some(highlighter) => {
                        text = crate::code::highlight_with(highlighter.bind(object.py()), text)?
                    }
                    None => rich::ReprHighlighter::new().highlight(&mut text),
                }
            }
            texts.push(text);
        }
    }
    flush(&mut texts, &mut items);
    Ok(items)
}

impl Console {
    fn state(&self) -> MutexGuard<'_, ConsoleState> {
        lock(&self.state)
    }

    pub(crate) fn snapshot(&self, py: Python<'_>) -> Snapshot {
        let state = self.state();
        let mut snapshot = Snapshot {
            settings: state.settings.clone(),
            theme: state
                .themes
                .last()
                .cloned()
                .unwrap_or_else(CoreTheme::default_theme),
            file: state.file.as_ref().map(|file| file.clone_ref(py)),
            extensions: crate::plugins::installed(py, self),
            highlighter: state.highlighter.as_ref().map(|h| h.clone_ref(py)),
            hooks: state.render_hooks.iter().map(|h| h.clone_ref(py)).collect(),
            ascii_only: false,
        };
        drop(state);
        // The file's encoding is read with no lock held (it is Python code).
        snapshot.ascii_only = snapshot
            .encoding(py)
            .is_ok_and(|encoding| !encoding.starts_with("utf"));
        snapshot
    }

    /// Start writing: wait (without the GIL) for another thread's write to
    /// finish, or fail if this thread is already writing on this console.
    fn start_printing(&self, py: Python<'_>) -> PyResult<Printing<'_>> {
        let me = thread_number();
        loop {
            {
                let mut printer = lock(&self.printer);
                if *printer == me {
                    return Err(PyRuntimeError::new_err(
                        "Console is already printing (print called from inside its own file.write)",
                    ));
                }
                if *printer == 0 {
                    *printer = me;
                    return Ok(Printing { console: self });
                }
            }
            // The writing thread needs the GIL to finish its write.
            py.detach(|| {
                let printer = lock(&self.printer);
                drop(
                    self.printed
                        .wait_while(printer, |printer| *printer != 0)
                        .unwrap_or_else(|poisoned| poisoned.into_inner()),
                );
            });
        }
    }

    /// Output rendered segments: into this thread's buffer while one is
    /// open, else (unless quiet) record them and write them to the file,
    /// then flush it, as upstream does after every print.
    fn emit(
        &self,
        py: Python<'_>,
        snapshot: &Snapshot,
        segments: Vec<CoreSegment>,
    ) -> PyResult<()> {
        let me = thread_number();
        {
            let mut state = self.state();
            if let Some(buffer) = state.buffers.iter_mut().find(|b| b.thread == me) {
                buffer.segments.extend(segments);
                return Ok(());
            }
            if state.settings.quiet {
                return Ok(());
            }
        }
        self.write(py, snapshot, segments)
    }

    fn write(
        &self,
        py: Python<'_>,
        snapshot: &Snapshot,
        segments: Vec<CoreSegment>,
    ) -> PyResult<()> {
        let output = snapshot.default_core().segments_to_string(&segments);
        let _printing = self.start_printing(py)?;
        {
            let mut state = self.state();
            if state.settings.record {
                state.recorded.extend(segments);
            }
        }
        let file = snapshot.target(py)?;
        file.call_method1("write", (output,))?;
        file.call_method0("flush")?;
        Ok(())
    }

    /// The print pipeline: collect, render (no lock held), style, crop, emit.
    fn print_objects(
        slf: &Bound<'_, Console>,
        objects: &[Bound<'_, PyAny>],
        args: PrintArgs<'_>,
    ) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let snapshot = this.snapshot(py);
        let settings = &snapshot.settings;
        let switches = Switches {
            emoji: args.emoji.unwrap_or(settings.emoji),
            markup: args.markup.unwrap_or(settings.markup),
            highlight: args.highlight.unwrap_or(settings.highlight),
            extensions: snapshot.extensions.clone(),
            highlighter: snapshot.highlighter.as_ref().map(|h| h.clone_ref(py)),
            emoji_variant: snapshot.settings.emoji_variant,
        };
        let (mut no_wrap, mut overflow, mut crop) = (args.no_wrap, args.overflow, args.crop);
        if args.soft_wrap.unwrap_or(settings.soft_wrap) {
            no_wrap = no_wrap.or(Some(true));
            overflow = overflow.or(Some(Overflow::Ignore));
            crop = false;
        }
        let justify = convert::justify(args.justify.as_deref())?;
        let core = snapshot.core(switches.emoji, switches.highlight);
        let mut base = snapshot.base(py)?;
        base.highlight = args.highlight;
        base.markup = args.markup;
        let ambient = snapshot.ambient(slf, base);
        let segments = renderable::scope(ambient, || {
            let empty;
            let objects = if objects.is_empty() && args.end != "\n" {
                empty = [PyString::new(py, "").into_any()];
                &empty[..]
            } else {
                objects
            };
            let items = if objects.is_empty() {
                vec![Item::Raw(vec![CoreSegment::line()])]
            } else {
                collect(
                    objects,
                    args.sep,
                    args.end,
                    args.justify.as_deref(),
                    &switches,
                    true,
                )?
            };
            let items = apply_hooks(py, &snapshot.hooks, items, &switches)?;
            // `Console(style=...)`: Rich wraps each renderable in `Styled`,
            // which styles what it renders; applied to the segments here.
            let console_style = settings
                .style
                .as_ref()
                .map(|style| resolve_style(&core, style))
                .transpose()?;
            let mut options = core.options();
            options.justify = justify;
            options.overflow = overflow;
            options.no_wrap = no_wrap;
            if let Some(width) = args.width {
                let width = width.min(core.width());
                options.min_width = width;
                options.max_width = width;
            }
            options.height = args.height;
            let style = args
                .style
                .as_ref()
                .map(|style| resolve_style(&core, style))
                .transpose()?;
            let mut segments = Vec::new();
            for item in &items {
                let mut rendered = item.render(py, &core, &options)?;
                if let Some(console_style) = &console_style {
                    rendered = CoreSegment::apply_style(&rendered, console_style);
                }
                match &style {
                    Some(style) => segments.extend(CoreSegment::apply_style(&rendered, style)),
                    None => segments.extend(rendered),
                }
            }
            if args.new_line_start && line_count(&segments) > 1 {
                segments.insert(0, CoreSegment::line());
            }
            if crop {
                segments = CoreSegment::crop_lines(&segments, core.width());
            }
            Ok(segments)
        })?;
        this.emit(py, &snapshot, segments)
    }

    /// Write control codes (unless on a dumb terminal), as `Console.control`.
    fn control(&self, py: Python<'_>, controls: &[Control]) -> PyResult<()> {
        let snapshot = self.snapshot(py);
        if snapshot.settings.is_terminal && is_dumb_term() {
            return Ok(());
        }
        let segments = controls
            .iter()
            .map(|control| CoreSegment::control(control.as_str()))
            .collect();
        self.emit(py, &snapshot, segments)
    }

    fn require_record(&self) -> PyResult<()> {
        if !self.state().settings.record {
            return Err(PyRuntimeError::new_err(
                "To export console contents set record=True in the constructor or instance",
            ));
        }
        Ok(())
    }

    /// The recorded segments, cleared if asked.
    fn recorded(&self, clear: bool) -> PyResult<Vec<CoreSegment>> {
        self.require_record()?;
        let mut state = self.state();
        Ok(if clear {
            std::mem::take(&mut state.recorded)
        } else {
            state.recorded.clone()
        })
    }

    fn enter_buffer(&self) {
        let me = thread_number();
        let mut state = self.state();
        match state.buffers.iter_mut().find(|b| b.thread == me) {
            Some(buffer) => buffer.depth += 1,
            None => state.buffers.push(ThreadBuffer {
                thread: me,
                depth: 1,
                segments: Vec::new(),
            }),
        }
    }

    /// Leave a buffer level; at the outermost, return what it held.
    fn exit_buffer(&self) -> Option<Vec<CoreSegment>> {
        let me = thread_number();
        let mut state = self.state();
        let index = state.buffers.iter().position(|b| b.thread == me)?;
        state.buffers[index].depth -= 1;
        if state.buffers[index].depth == 0 {
            Some(state.buffers.remove(index).segments)
        } else {
            None
        }
    }

    fn ambient_for(
        slf: &Bound<'_, Console>,
        snapshot: &Snapshot,
        options: Option<&ConsoleOptions>,
    ) -> PyResult<(Ambient, CoreOptions)> {
        let py = slf.py();
        let (base, core_options) = match options {
            Some(options) => (options.base(), options.to_core()?),
            None => (snapshot.base(py)?, snapshot.default_core().options()),
        };
        Ok((snapshot.ambient(slf, base), core_options))
    }

    fn write_file(py: Python<'_>, path: &Bound<'_, PyAny>, content: &str) -> PyResult<()> {
        let kwargs = PyDict::new(py);
        kwargs.set_item("encoding", "utf-8")?;
        let file = py
            .import("builtins")?
            .getattr("open")?
            .call((path, "w"), Some(&kwargs))?;
        let written = file.call_method1("write", (content,));
        file.call_method0("close")?;
        written.map(|_| ())
    }
}

/// `print_json`'s `indent`: `None`, an `int` or a `str`; 2 when not given.
enum JsonIndent {
    Two,
    Value(Py<PyAny>),
}

impl<'a, 'py> FromPyObject<'a, 'py> for JsonIndent {
    type Error = PyErr;

    fn extract(value: pyo3::Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        Ok(JsonIndent::Value(value.to_owned().unbind()))
    }
}

/// Resolve a style name against the console's theme (Rich's `get_style`).
fn resolve_style(console: &CoreConsole, style: &StyleType) -> PyResult<CoreStyle> {
    console.get_style(style).map_err(|error| {
        let name = match style {
            StyleType::Name(name) => name.clone(),
            StyleType::Style(style) => style.definition(),
        };
        let quoted = name.replace('\\', "\\\\").replace('\'', "\\'");
        MissingStyle::new_err(format!("Failed to get style '{quoted}'; {error}"))
    })
}

fn terminal_theme(
    theme: Option<PyRef<'_, TerminalTheme>>,
    default: rich::terminal_theme::TerminalTheme,
) -> rich::terminal_theme::TerminalTheme {
    theme.map_or(default, |theme| theme.inner.clone())
}

#[pymethods]
impl Console {
    #[new]
    #[pyo3(signature = (
        *, color_system=Some("auto".to_string()), force_terminal=None, force_jupyter=None,
        force_interactive=None, soft_wrap=false, theme=None, stderr=false, file=None,
        quiet=false, width=None, height=None, style=None, no_color=None, tab_size=8,
        record=false, markup=true, emoji=true, emoji_variant=None, highlight=true,
        log_time=true, log_path=true, log_time_format=None, highlighter=None,
        legacy_windows=None, safe_box=true, get_datetime=None, get_time=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        color_system: Option<String>,
        force_terminal: Option<bool>,
        force_jupyter: Option<bool>,
        force_interactive: Option<bool>,
        soft_wrap: bool,
        theme: Option<PyRef<'_, Theme>>,
        stderr: bool,
        file: Option<Py<PyAny>>,
        quiet: bool,
        width: Option<usize>,
        height: Option<usize>,
        style: Option<&Bound<'_, PyAny>>,
        no_color: Option<bool>,
        tab_size: usize,
        record: bool,
        markup: bool,
        emoji: bool,
        emoji_variant: Option<String>,
        highlight: bool,
        log_time: bool,
        log_path: bool,
        log_time_format: Option<Py<PyAny>>,
        highlighter: Option<Py<PyAny>>,
        legacy_windows: Option<bool>,
        safe_box: bool,
        get_datetime: Option<Py<PyAny>>,
        get_time: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        if let Some(width) = width {
            check_width(width)?;
        }
        if force_jupyter == Some(true) {
            return Err(PyNotImplementedError::new_err(
                "rs_rich does not render for Jupyter (force_jupyter=True)",
            ));
        }
        // Rich looks the variant up when it replaces codes, and an unknown
        // one adds nothing.
        let emoji_variant = emoji_variant
            .as_deref()
            .and_then(rich::emoji::EmojiVariant::parse);
        // Upstream asks the file, not the process's stdout, whether it is a
        // terminal; `force_terminal` overrides it.
        let is_terminal = match force_terminal {
            Some(value) => value,
            None => target(py, file.as_ref(), stderr)?
                .call_method0("isatty")
                .and_then(|v| v.extract::<bool>())
                .unwrap_or(false),
        };
        let mut builder = CoreConsole::builder()
            .force_terminal(is_terminal)
            .highlight(highlight)
            .emoji(emoji)
            .safe_box(safe_box);
        builder = match color_system.as_deref() {
            Some("auto") => builder,
            None => builder.color_system(None),
            Some("standard") => builder.color_system(Some(ColorSystem::Standard)),
            Some("256") => builder.color_system(Some(ColorSystem::EightBit)),
            Some("truecolor") => builder.color_system(Some(ColorSystem::Truecolor)),
            Some("windows") => builder.color_system(Some(ColorSystem::Windows)),
            Some(other) => {
                return Err(PyValueError::new_err(format!(
                    "{other:?} is not a valid color system; expected auto, standard, 256, truecolor, windows or None"
                )))
            }
        };
        if let Some(no_color) = no_color {
            builder = builder.no_color(no_color);
        }
        // Read through `os.environ`, as Rich does, so Python-side changes
        // to the environment count.
        let environ = py.import("os")?.getattr("environ")?;
        let env_size = |name: &str| -> PyResult<Option<usize>> {
            let value: Option<String> = environ.call_method1("get", (name,))?.extract()?;
            Ok(value
                .and_then(|v| v.trim().parse().ok())
                .filter(|v: &usize| *v > 0))
        };
        // A file that is not a terminal is `COLUMNS` wide (or 80) and `LINES`
        // tall (or 25), whatever size the process's own terminal is. On a
        // terminal, Rich asks the standard streams, then the environment.
        let (terminal_width, terminal_height) =
            if is_terminal && (width.is_none() || height.is_none()) {
                let os = py.import("os")?;
                let mut size = (None, None);
                for fd in 0..3 {
                    if let Ok(found) = os.call_method1("get_terminal_size", (fd,)) {
                        size = (
                            found.get_item(0)?.extract().ok(),
                            found.get_item(1)?.extract().ok(),
                        );
                        break;
                    }
                }
                size
            } else {
                (None, None)
            };
        let width = match width {
            Some(width) => width,
            None => env_size("COLUMNS")?
                .filter(|w| *w <= MAX_CONSOLE_WIDTH)
                .or(terminal_width.filter(|w: &usize| *w > 0))
                .unwrap_or(80)
                .min(MAX_CONSOLE_WIDTH),
        };
        builder = builder.width(width);
        let height = match height {
            Some(height) => height,
            None => env_size("LINES")?
                .or(terminal_height.filter(|h: &usize| *h > 0))
                .unwrap_or(25),
        };
        builder = builder.height(height);
        let detected = builder.build();
        let settings = Settings {
            color_system: detected.color_system(),
            force_terminal,
            is_terminal,
            no_color: detected.no_color(),
            width: detected.width().min(MAX_CONSOLE_WIDTH),
            height: detected.height(),
            emoji,
            highlight,
            markup,
            safe_box,
            legacy_windows: legacy_windows.unwrap_or(false),
            soft_wrap,
            tab_size,
            quiet,
            stderr,
            record,
            force_interactive,
            style: style_type(style)?,
            log_time,
            emoji_variant,
        };
        Ok(Console {
            state: Mutex::new(ConsoleState {
                settings,
                themes: vec![
                    theme.map_or_else(CoreTheme::default_theme, |theme| theme.inner.clone())
                ],
                recorded: Vec::new(),
                buffers: Vec::new(),
                log_render: LogRender::new().show_time(log_time).show_path(log_path),
                file,
                get_datetime,
                get_time,
                log_time_format,
                is_alt_screen: false,
                highlighter: highlighter.filter(|h| !h.is_none(py)),
                render_hooks: Vec::new(),
                live_stack: Vec::new(),
            }),
            printer: Mutex::new(0),
            printed: Condvar::new(),
        })
    }

    // `file` (or a callable) can refer back to the console.
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        // Never block in the collector: a console being printed to is alive.
        if let Ok(state) = self.state.try_lock() {
            for object in [
                &state.file,
                &state.get_datetime,
                &state.get_time,
                &state.log_time_format,
                &state.highlighter,
            ]
            .into_iter()
            .flatten()
            .chain(state.render_hooks.iter())
            .chain(state.live_stack.iter())
            {
                visit.call(object)?;
            }
        }
        Ok(())
    }

    fn __clear__(&self) {
        let mut state = self.state();
        state.file = None;
        state.get_datetime = None;
        state.get_time = None;
        state.log_time_format = None;
        state.highlighter = None;
        state.render_hooks.clear();
        state.live_stack.clear();
    }

    fn __repr__(&self) -> String {
        let state = self.state();
        let system = match state.settings.color_system {
            None => "None",
            Some(ColorSystem::Standard) => "ColorSystem.STANDARD",
            Some(ColorSystem::EightBit) => "ColorSystem.EIGHT_BIT",
            Some(ColorSystem::Truecolor) => "ColorSystem.TRUECOLOR",
            Some(ColorSystem::Windows) => "ColorSystem.WINDOWS",
        };
        format!("<console width={} {system}>", state.settings.width)
    }

    /// `with console:` holds output back until the block ends.
    fn __enter__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf.get().enter_buffer();
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, PyTuple>) -> PyResult<()> {
        if let Some(segments) = self.exit_buffer() {
            let snapshot = self.snapshot(py);
            if !snapshot.settings.quiet {
                self.write(py, &snapshot, segments)?;
            }
        }
        Ok(())
    }

    // --- properties -------------------------------------------------------

    /// The file output goes to: the one given, else `sys.stdout` (or
    /// `sys.stderr` with `stderr=True`), looked up each time.
    #[getter]
    fn file<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.snapshot(py).target(py)
    }

    #[setter]
    fn set_file(&self, py: Python<'_>, file: Option<Py<PyAny>>) -> PyResult<()> {
        let force = self.state().settings.force_terminal;
        let is_terminal = match force {
            Some(value) => value,
            None => {
                let stderr = self.state().settings.stderr;
                target(py, file.as_ref(), stderr)?
                    .call_method0("isatty")
                    .and_then(|v| v.extract::<bool>())
                    .unwrap_or(false)
            }
        };
        let mut state = self.state();
        state.file = file;
        state.settings.is_terminal = is_terminal;
        Ok(())
    }

    #[getter]
    fn width(&self) -> usize {
        self.state().settings.width
    }

    #[setter]
    fn set_width(&self, width: usize) -> PyResult<()> {
        self.state().settings.width = check_width(width)?;
        Ok(())
    }

    #[getter]
    fn height(&self) -> usize {
        self.state().settings.height
    }

    #[setter]
    fn set_height(&self, height: usize) {
        self.state().settings.height = height;
    }

    /// `ConsoleDimensions(width, height)`.
    #[getter]
    fn size(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let settings = &self.state().settings;
        protocol::dimensions(py, (settings.width, settings.height))
    }

    #[setter]
    fn set_size(&self, size: (usize, usize)) -> PyResult<()> {
        let width = check_width(size.0)?;
        let mut state = self.state();
        state.settings.width = width;
        state.settings.height = size.1;
        Ok(())
    }

    #[getter]
    fn is_terminal(&self) -> bool {
        self.state().settings.is_terminal
    }

    #[getter]
    fn is_dumb_terminal(&self) -> bool {
        self.state().settings.is_terminal && is_dumb_term()
    }

    #[getter]
    fn is_interactive(&self) -> bool {
        let settings = &self.state().settings;
        settings
            .force_interactive
            .unwrap_or(settings.is_terminal && !is_dumb_term())
    }

    #[setter]
    fn set_is_interactive(&self, value: bool) {
        self.state().settings.force_interactive = Some(value);
    }

    #[getter]
    fn color_system(&self) -> Option<&'static str> {
        color_system_name(self.state().settings.color_system)
    }

    /// The file's encoding, lower case (`"utf-8"` when it has none).
    #[getter]
    fn encoding(&self, py: Python<'_>) -> PyResult<String> {
        self.snapshot(py).encoding(py)
    }

    #[getter]
    fn no_color(&self) -> bool {
        self.state().settings.no_color
    }

    #[getter]
    fn legacy_windows(&self) -> bool {
        self.state().settings.legacy_windows
    }

    #[getter]
    fn safe_box(&self) -> bool {
        self.state().settings.safe_box
    }

    #[getter]
    fn tab_size(&self) -> usize {
        self.state().settings.tab_size
    }

    #[getter]
    fn quiet(&self) -> bool {
        self.state().settings.quiet
    }

    #[setter]
    fn set_quiet(&self, quiet: bool) {
        self.state().settings.quiet = quiet;
    }

    #[getter]
    fn soft_wrap(&self) -> bool {
        self.state().settings.soft_wrap
    }

    #[setter]
    fn set_soft_wrap(&self, soft_wrap: bool) {
        self.state().settings.soft_wrap = soft_wrap;
    }

    #[getter]
    fn record(&self) -> bool {
        self.state().settings.record
    }

    #[setter]
    fn set_record(&self, record: bool) {
        self.state().settings.record = record;
    }

    #[getter]
    fn stderr(&self) -> bool {
        self.state().settings.stderr
    }

    #[getter]
    fn is_alt_screen(&self) -> bool {
        self.state().is_alt_screen
    }

    /// The clock animations read: the one given, else `time.monotonic`.
    #[getter(get_time)]
    fn get_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.state().get_time {
            Some(clock) => Ok(clock.bind(py).clone()),
            None => py.import("time")?.getattr("monotonic"),
        }
    }

    /// What `log` stamps records with: the callable given, else
    /// `datetime.now`.
    #[getter(get_datetime)]
    fn get_datetime<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.state().get_datetime {
            Some(clock) => Ok(clock.bind(py).clone()),
            None => py.import("datetime")?.getattr("datetime")?.getattr("now"),
        }
    }

    /// The highlighter printed strings go through (Rich's `ReprHighlighter`
    /// unless one was given).
    #[getter]
    fn highlighter(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.state().highlighter {
            Some(highlighter) => Ok(highlighter.clone_ref(py)),
            None => Ok(py
                .import("rs_rich.highlighter")?
                .getattr("ReprHighlighter")?
                .call0()?
                .unbind()),
        }
    }

    #[setter]
    fn set_highlighter(&self, py: Python<'_>, highlighter: Option<Py<PyAny>>) {
        self.state().highlighter = highlighter.filter(|h| !h.is_none(py));
    }

    /// The running `Live` displays, outermost first (Rich's `_live_stack`).
    #[getter]
    fn _live_stack<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let lives: Vec<Py<PyAny>> = self
            .state()
            .live_stack
            .iter()
            .map(|live| live.clone_ref(py))
            .collect();
        PyList::new(py, lives)
    }

    /// Push a `Live` display; `True` when it is the only one (Rich's
    /// `set_live`).
    fn set_live(&self, live: Py<PyAny>) -> bool {
        let mut state = self.state();
        state.live_stack.push(live);
        state.live_stack.len() == 1
    }

    /// Pop the top `Live` display.
    fn clear_live(&self) -> PyResult<()> {
        let removed = self.state().live_stack.pop();
        match removed {
            Some(_) => Ok(()),
            None => Err(PyIndexError::new_err("pop from empty list")),
        }
    }

    /// Add a render hook: an object whose `process_renderables(renderables)`
    /// returns what each print prints instead (a `Live` display is one).
    fn push_render_hook(&self, hook: Py<PyAny>) {
        self.state().render_hooks.push(hook);
    }

    /// Remove the render hook added last.
    fn pop_render_hook(&self) -> PyResult<()> {
        let removed = self.state().render_hooks.pop();
        match removed {
            Some(_) => Ok(()),
            None => Err(PyIndexError::new_err("pop from empty list")),
        }
    }

    /// Set the terminal window title; `False` when not a terminal.
    fn set_window_title(&self, py: Python<'_>, title: &str) -> PyResult<bool> {
        if !self.state().settings.is_terminal {
            return Ok(false);
        }
        let snapshot = self.snapshot(py);
        let segment = CoreSegment::control(format!("\x1b]0;{title}\x07"));
        self.emit(py, &snapshot, vec![segment])?;
        Ok(true)
    }

    /// The default render options.
    #[getter]
    fn options(&self, py: Python<'_>) -> PyResult<ConsoleOptions> {
        let snapshot = self.snapshot(py);
        let options = snapshot.default_core().options();
        Ok(ConsoleOptions::from_core(&options, &snapshot.base(py)?))
    }

    // --- printing ---------------------------------------------------------

    /// Print objects, as `rich.console.Console.print`.
    #[pyo3(signature = (
        *objects, sep=" ", end="\n", style=None, justify=None, overflow=None, no_wrap=None,
        emoji=None, markup=None, highlight=None, width=None, height=None, crop=true,
        soft_wrap=None, new_line_start=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn print(
        slf: &Bound<'_, Self>,
        objects: &Bound<'_, PyTuple>,
        sep: &str,
        end: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<String>,
        overflow: Option<&str>,
        no_wrap: Option<bool>,
        emoji: Option<bool>,
        markup: Option<bool>,
        highlight: Option<bool>,
        width: Option<usize>,
        height: Option<usize>,
        crop: bool,
        soft_wrap: Option<bool>,
        new_line_start: bool,
    ) -> PyResult<()> {
        let objects: Vec<_> = objects.iter().collect();
        let args = PrintArgs {
            sep,
            end,
            style: style_type(style)?,
            justify,
            overflow: overflow.map(convert::overflow).transpose()?,
            no_wrap,
            emoji,
            markup,
            highlight,
            width,
            height,
            crop,
            soft_wrap,
            new_line_start,
        };
        Console::print_objects(slf, &objects, args)
    }

    /// Write objects as their `str`, with no markup, emoji, wrapping or
    /// cropping (`rich.console.Console.out`).
    #[pyo3(signature = (*objects, sep=" ", end="\n", style=None, highlight=None))]
    fn out(
        slf: &Bound<'_, Self>,
        objects: &Bound<'_, PyTuple>,
        sep: &str,
        end: &str,
        style: Option<&Bound<'_, PyAny>>,
        highlight: Option<bool>,
    ) -> PyResult<()> {
        let mut parts = Vec::new();
        for object in objects.iter() {
            parts.push(object.str()?.to_string());
        }
        let raw = PyString::new(slf.py(), &parts.join(sep)).into_any();
        let args = PrintArgs {
            end,
            style: style_type(style)?,
            highlight,
            emoji: Some(false),
            markup: Some(false),
            no_wrap: Some(true),
            overflow: Some(Overflow::Ignore),
            crop: false,
            ..PrintArgs::default()
        };
        Console::print_objects(slf, &[raw], args)
    }

    /// Log objects with the time and the caller's file and line, as
    /// `rich.console.Console.log`.
    #[pyo3(signature = (
        *objects, sep=" ", end="\n", style=None, justify=None, emoji=None, markup=None,
        highlight=None, log_locals=false, _stack_offset=1
    ))]
    #[allow(clippy::too_many_arguments)]
    fn log(
        slf: &Bound<'_, Self>,
        objects: &Bound<'_, PyTuple>,
        sep: &str,
        end: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
        emoji: Option<bool>,
        markup: Option<bool>,
        highlight: Option<bool>,
        log_locals: bool,
        _stack_offset: usize,
    ) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let snapshot = this.snapshot(py);
        let settings = &snapshot.settings;
        let switches = Switches {
            emoji: emoji.unwrap_or(settings.emoji),
            markup: markup.unwrap_or(settings.markup),
            highlight: highlight.unwrap_or(settings.highlight),
            extensions: snapshot.extensions.clone(),
            highlighter: snapshot.highlighter.as_ref().map(|h| h.clone_ref(py)),
            emoji_variant: snapshot.settings.emoji_variant,
        };
        let style = style_type(style)?;
        let text_justify = convert::justify(justify)?;
        // The caller's frame: a native method adds none of its own.
        let frame = py
            .import("sys")?
            .call_method1("_getframe", (_stack_offset.saturating_sub(1),))?;
        let filename: String = frame.getattr("f_code")?.getattr("co_filename")?.extract()?;
        let line_no: u32 = frame.getattr("f_lineno")?.extract()?;
        let os = py.import("os")?;
        let separator: String = os.getattr("sep")?.extract()?;
        let path = filename
            .rsplit_once(separator.as_str())
            .map_or(filename.as_str(), |(_, name)| name)
            .to_string();
        let link_path: Option<String> = if filename.starts_with('<') {
            None
        } else {
            Some(
                os.getattr("path")?
                    .call_method1("abspath", (&filename,))?
                    .extract()?,
            )
        };
        let time = if settings.log_time {
            let (get_datetime, format) = {
                let state = this.state();
                (
                    state.get_datetime.as_ref().map(|f| f.clone_ref(py)),
                    state.log_time_format.as_ref().map(|f| f.clone_ref(py)),
                )
            };
            let now = match get_datetime {
                Some(clock) => clock.bind(py).call0()?,
                None => py
                    .import("datetime")?
                    .getattr("datetime")?
                    .call_method0("now")?,
            };
            Some(match format {
                Some(format) if format.bind(py).is_callable() => {
                    let shown = format.bind(py).call1((now,))?;
                    match shown.extract::<PyRef<'_, Text>>() {
                        Ok(text) => text.inner.clone(),
                        Err(_) => CoreText::new(shown.str()?.to_cow()?.as_ref()),
                    }
                }
                Some(format) => CoreText::new(
                    now.call_method1("strftime", (format,))?
                        .str()?
                        .to_cow()?
                        .as_ref(),
                ),
                None => CoreText::new(
                    now.call_method1("strftime", ("[%X]",))?
                        .str()?
                        .to_cow()?
                        .as_ref(),
                ),
            })
        } else {
            None
        };
        // Rich's `_caller_frame_info` locals, without dunder names.
        let locals = if log_locals {
            let scope = PyDict::new(py);
            let frame_locals = frame.getattr("f_locals")?;
            for key in frame_locals.try_iter()? {
                let key = key?;
                let name: String = key.str()?.extract()?;
                if !name.starts_with("__") {
                    scope.set_item(&key, frame_locals.get_item(&key)?)?;
                }
            }
            Some(scope)
        } else {
            None
        };
        let core = snapshot.core(switches.emoji, switches.highlight);
        let ambient = snapshot.ambient(slf, snapshot.base(py)?);
        let segments = renderable::scope(ambient, || {
            let objects: Vec<_> = objects.iter().collect();
            // A line of text goes to core's `LogRender` as a `Text`, justified;
            // anything else is upstream's list of renderables, where `justify`
            // aligns each with `Align`.
            let simple = style.is_none()
                && locals.is_none()
                && objects
                    .iter()
                    .all(|o| o.is_instance_of::<PyString>() || o.cast::<Text>().is_ok());
            let items = if simple {
                collect(&objects, sep, end, None, &switches, false)?
            } else {
                collect(&objects, sep, end, justify, &switches, true)?
            };
            let style = style
                .as_ref()
                .map(|style| resolve_style(&core, style))
                .transpose()?;
            let table = match (items.as_slice(), &locals) {
                // One line of text: core's `LogRender` with a `Text` message.
                ([] | [Item::Joined { .. }], None) if simple => {
                    let mut message = match items.into_iter().next() {
                        Some(Item::Joined { text, .. }) => text,
                        _ => CoreText::new(""),
                    };
                    message.set_justify(text_justify);
                    let state = this.state();
                    state.log_render.render(
                        &core,
                        message,
                        time,
                        CoreText::new(""),
                        Some(&path),
                        Some(line_no),
                        link_path.as_deref(),
                    )
                }
                // Anything else: upstream's `Renderables` message cell.
                _ => {
                    let mut message: Vec<Arc<dyn Renderable + Send + Sync>> = Vec::new();
                    for item in items {
                        let item = match item {
                            Item::Joined { text, .. } => Item::Core(Box::new(text)),
                            item => item,
                        };
                        // Upstream wraps each renderable in `Styled`.
                        let renderable: Box<dyn Renderable> = match (item, &style) {
                            (item, Some(style)) => {
                                Box::new(rich::styled::Styled::new(item.into_box(), style.clone()))
                            }
                            (item, None) => item.into_box(),
                        };
                        message.push(Arc::new(ThreadBound(renderable)));
                    }
                    if let Some(scope) = &locals {
                        message.push(crate::code::render_scope(
                            scope,
                            Some("[i]locals".to_string()),
                        )?);
                    }
                    let state = this.state();
                    state.log_render.render_renderables(
                        &core,
                        message,
                        time,
                        CoreText::new(""),
                        Some(&path),
                        Some(line_no),
                        link_path.as_deref(),
                    )
                }
            };
            let items = apply_hooks(
                py,
                &snapshot.hooks,
                vec![Item::Core(Box::new(table))],
                &switches,
            )?;
            let mut segments = Vec::new();
            for item in &items {
                segments.extend(item.render(py, &core, &core.options())?);
            }
            Ok(CoreSegment::crop_lines(&segments, core.width()))
        })?;
        this.emit(py, &snapshot, segments)
    }

    /// Draw a horizontal rule, with an optional (markup) title.
    #[pyo3(signature = (title="", *, characters="─", style=None, align="center"))]
    fn rule(
        slf: &Bound<'_, Self>,
        title: &str,
        characters: &str,
        style: Option<&Bound<'_, PyAny>>,
        align: &str,
    ) -> PyResult<()> {
        let py = slf.py();
        let mut rule = if title.is_empty() {
            Rule::line()
        } else {
            Rule::new(title)
        }
        .characters(characters)
        .align(convert::align(align)?);
        if let Some(style) = style_type(style)? {
            let core = slf.get().snapshot(py).default_core();
            rule = rule.style(resolve_style(&core, &style)?);
        }
        Console::print_item(slf, Item::Core(Box::new(rule)), PrintArgs::default())
    }

    /// Write `count` blank lines.
    #[pyo3(signature = (count=1))]
    fn line(slf: &Bound<'_, Self>, count: usize) -> PyResult<()> {
        let newlines = vec![CoreSegment::new("\n".repeat(count), None)];
        Console::print_item(slf, Item::Raw(newlines), PrintArgs::default())
    }

    /// Clear the screen (on a terminal), and move the cursor home.
    #[pyo3(signature = (home=true))]
    fn clear(&self, py: Python<'_>, home: bool) -> PyResult<()> {
        if home {
            self.control(py, &[Control::clear(), Control::home()])
        } else {
            self.control(py, &[Control::clear()])
        }
    }

    /// Ring the terminal bell.
    fn bell(&self, py: Python<'_>) -> PyResult<()> {
        self.control(py, &[Control::bell()])
    }

    /// Show or hide the cursor; `False` when not a terminal.
    #[pyo3(signature = (show=true))]
    fn show_cursor(&self, py: Python<'_>, show: bool) -> PyResult<bool> {
        if !self.state().settings.is_terminal {
            return Ok(false);
        }
        self.control(py, &[Control::show_cursor(show)])?;
        Ok(true)
    }

    /// Enter or leave the alternate screen; `False` when not a terminal.
    #[pyo3(signature = (enable=true))]
    fn set_alt_screen(&self, py: Python<'_>, enable: bool) -> PyResult<bool> {
        let (is_terminal, legacy) = {
            let settings = &self.state().settings;
            (settings.is_terminal, settings.legacy_windows)
        };
        let mut changed = false;
        if is_terminal && !legacy {
            // Rich's `Control.alt_screen(True)` also moves the cursor home;
            // core's leaves that out.
            if enable {
                let codes = [rich::ControlType::EnableAltScreen, rich::ControlType::Home];
                self.control(py, &[Control::new(&codes)])?;
            } else {
                self.control(py, &[Control::alt_screen(false)])?;
            }
            changed = true;
            self.state().is_alt_screen = enable;
        }
        Ok(changed)
    }

    /// Show `prompt` (markup) and read a line: `input()`, `getpass` with
    /// `password=True`, or `stream.readline()`.
    #[pyo3(signature = (prompt=None, *, markup=true, emoji=true, password=false, stream=None))]
    fn input(
        slf: &Bound<'_, Self>,
        prompt: Option<&Bound<'_, PyAny>>,
        markup: bool,
        emoji: bool,
        password: bool,
        stream: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        if let Some(prompt) = prompt.filter(|p| p.is_truthy().unwrap_or(false)) {
            let args = PrintArgs {
                end: "",
                markup: Some(markup),
                emoji: Some(emoji),
                ..PrintArgs::default()
            };
            Console::print_objects(slf, std::slice::from_ref(prompt), args)?;
        }
        let result = if password {
            let kwargs = PyDict::new(py);
            kwargs.set_item("stream", stream)?;
            py.import("getpass")?
                .getattr("getpass")?
                .call(("",), Some(&kwargs))?
        } else if let Some(stream) = stream {
            stream.call_method0("readline")?
        } else {
            py.import("builtins")?.getattr("input")?.call0()?
        };
        Ok(result.unbind())
    }

    /// Pretty-print JSON (a string, or `data` to encode), as
    /// `rich.console.Console.print_json`.
    #[pyo3(signature = (
        json=None, *, data=None, indent=JsonIndent::Two, highlight=true, skip_keys=false,
        ensure_ascii=false, check_circular=true, allow_nan=true, default=None, sort_keys=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn print_json(
        slf: &Bound<'_, Self>,
        json: Option<&Bound<'_, PyAny>>,
        data: Option<&Bound<'_, PyAny>>,
        indent: JsonIndent,
        highlight: bool,
        skip_keys: bool,
        ensure_ascii: bool,
        check_circular: bool,
        allow_nan: bool,
        default: Option<&Bound<'_, PyAny>>,
        sort_keys: bool,
    ) -> PyResult<()> {
        let py = slf.py();
        let data = match json {
            None => data.map_or_else(|| py.None().into_bound(py), |d| d.clone()),
            Some(json) => {
                if !json.is_instance_of::<PyString>() {
                    return Err(PyTypeError::new_err(format!(
                        "json must be str. Did you mean print_json(data={}) ?",
                        json.repr()?
                    )));
                }
                py.import("json")?.call_method1("loads", (json,))?
            }
        };
        let indent = match indent {
            JsonIndent::Two => 2i64.into_pyobject(py)?.into_any(),
            JsonIndent::Value(value) => value.into_bound(py),
        };
        let text = crate::code::json_text(
            &data,
            &indent,
            highlight,
            skip_keys,
            ensure_ascii,
            check_circular,
            allow_nan,
            default,
            sort_keys,
        )?;
        // Upstream prints `JSON(...)`, whose `__rich__` is this `Text`.
        let item = Item::Joined {
            text,
            end: "\n".to_string(),
        };
        Console::print_item(
            slf,
            item,
            PrintArgs {
                soft_wrap: Some(true),
                ..PrintArgs::default()
            },
        )
    }

    // --- rendering --------------------------------------------------------

    /// Measure a renderable: `Measurement(minimum, maximum)`.
    #[pyo3(signature = (renderable, *, options=None))]
    fn measure(
        slf: &Bound<'_, Self>,
        renderable: &Bound<'_, PyAny>,
        options: Option<PyRef<'_, ConsoleOptions>>,
    ) -> PyResult<Measurement> {
        let snapshot = slf.get().snapshot(slf.py());
        let (ambient, core_options) = Console::ambient_for(slf, &snapshot, options.as_deref())?;
        let core = snapshot.default_core();
        let measurement = renderable::scope(ambient, || {
            renderable::measure_object(renderable, &core, &core_options)
        })?;
        Ok(Measurement::from_core(measurement))
    }

    /// Render a renderable to a list of `Segment`s (every line ends with a
    /// newline segment), as `rich.console.Console.render`.
    #[pyo3(signature = (renderable, options=None))]
    fn render<'py>(
        slf: &Bound<'py, Self>,
        renderable: &Bound<'py, PyAny>,
        options: Option<PyRef<'py, ConsoleOptions>>,
    ) -> PyResult<Bound<'py, PyList>> {
        let snapshot = slf.get().snapshot(slf.py());
        let (ambient, core_options) = Console::ambient_for(slf, &snapshot, options.as_deref())?;
        let core = snapshot.default_core();
        let segments = renderable::scope(ambient, || {
            renderable::render_object(renderable, &core, &core_options)
        })?;
        segment::to_python(slf.py(), &segments)
    }

    /// Render to lines of `Segment`s, padded to the width unless `pad` is
    /// false, as `rich.console.Console.render_lines`.
    #[pyo3(signature = (renderable, options=None, *, style=None, pad=true, new_lines=false))]
    fn render_lines<'py>(
        slf: &Bound<'py, Self>,
        renderable: &Bound<'py, PyAny>,
        options: Option<PyRef<'py, ConsoleOptions>>,
        style: Option<&Bound<'py, PyAny>>,
        pad: bool,
        new_lines: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = slf.py();
        let snapshot = slf.get().snapshot(py);
        let (ambient, core_options) = Console::ambient_for(slf, &snapshot, options.as_deref())?;
        let core = snapshot.default_core();
        let style = style_type(style)?
            .map(|style| resolve_style(&core, &style))
            .transpose()?;
        let object = PyRenderable::new(renderable.clone().unbind());
        let lines = renderable::scope(ambient, || {
            let lines = core.render_lines_styled(&object, &core_options, style.as_ref(), pad);
            renderable::check_pending()?;
            Ok(lines)
        })?;
        let result = PyList::empty(py);
        for mut line in lines {
            if new_lines {
                line.push(CoreSegment::line());
            }
            result.append(segment::to_python(py, &line)?)?;
        }
        Ok(result)
    }

    /// Convert a string to a `Text` (markup, emoji, highlighting), as
    /// `rich.console.Console.render_str`.
    #[pyo3(signature = (
        text, *, style=None, justify=None, overflow=None, emoji=None, markup=None, highlight=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn render_str(
        &self,
        py: Python<'_>,
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
        overflow: Option<&str>,
        emoji: Option<bool>,
        markup: Option<bool>,
        highlight: Option<bool>,
    ) -> PyResult<Text> {
        let (settings, highlighter) = {
            let state = self.state();
            (
                state.settings.clone(),
                state.highlighter.as_ref().map(|h| h.clone_ref(py)),
            )
        };
        let emoji = emoji.unwrap_or(settings.emoji);
        let markup = markup.unwrap_or(settings.markup);
        let highlight = highlight.unwrap_or(settings.highlight);
        let render = |highlight: bool| {
            renderable::render_str_with(
                text,
                emoji,
                markup,
                highlight,
                &[],
                highlighter.as_ref().map(|h| h.bind(py)),
                settings.emoji_variant,
            )
        };
        if highlight {
            // As in Rich, highlighting builds a new Text: style, justify and
            // overflow do not survive it.
            return Ok(Text::from_core(render(true)?));
        }
        let mut inner = render(false)?;
        if let Some(style) = style_type(style)? {
            inner.set_base_style(style);
        }
        inner.set_justify(convert::justify(justify)?);
        inner.set_overflow(overflow.map(convert::overflow).transpose()?);
        Ok(Text::from_core(inner))
    }

    /// A style by theme name or definition (a `Style` passes through).
    #[pyo3(signature = (name, *, default=None))]
    fn get_style(
        &self,
        py: Python<'_>,
        name: &Bound<'_, PyAny>,
        default: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Style> {
        if let Ok(style) = name.extract::<PyRef<'_, Style>>() {
            return Ok(style.clone());
        }
        let name: String = name.extract()?;
        let core = self.snapshot(py).default_core();
        match resolve_style(&core, &StyleType::Name(name)) {
            Ok(style) => Ok(Style::from_core(style)),
            Err(error) => match default {
                Some(default) => self.get_style(py, default, None),
                None => Err(error),
            },
        }
    }

    // --- themes -----------------------------------------------------------

    /// Push a theme; with `inherit` its styles override the current ones.
    #[pyo3(signature = (theme, *, inherit=true))]
    fn push_theme(&self, theme: PyRef<'_, Theme>, inherit: bool) {
        let mut state = self.state();
        let top = if inherit {
            let mut merged = state
                .themes
                .last()
                .cloned()
                .unwrap_or_else(CoreTheme::default_theme);
            merged.extend_from(&theme.inner);
            merged
        } else {
            theme.inner.clone()
        };
        state.themes.push(top);
    }

    /// Pop the theme pushed last; the console's own theme cannot be popped.
    fn pop_theme(&self) -> PyResult<()> {
        let mut state = self.state();
        if state.themes.len() <= 1 {
            return Err(ThemeStackError::new_err("Unable to pop base theme"));
        }
        state.themes.pop();
        Ok(())
    }

    /// A context manager that uses `theme` inside its block.
    #[pyo3(signature = (theme, *, inherit=true))]
    fn use_theme(slf: &Bound<'_, Self>, theme: Py<Theme>, inherit: bool) -> ThemeContext {
        ThemeContext {
            console: slf.clone().unbind(),
            theme,
            inherit,
        }
    }

    // --- capture and export -------------------------------------------------

    /// A context manager capturing output: `with console.capture() as c:`,
    /// then `c.get()`.
    fn capture(slf: &Bound<'_, Self>) -> Capture {
        Capture {
            console: slf.clone().unbind(),
            result: Mutex::new(None),
        }
    }

    /// Start capturing this thread's output.
    fn begin_capture(&self) {
        self.enter_buffer();
    }

    /// Stop capturing and return what was printed, with its ANSI styles.
    fn end_capture(&self, py: Python<'_>) -> String {
        let me = thread_number();
        let segments = {
            let mut state = self.state();
            state
                .buffers
                .iter_mut()
                .find(|b| b.thread == me)
                .map(|buffer| std::mem::take(&mut buffer.segments))
                .unwrap_or_default()
        };
        let output = self
            .snapshot(py)
            .default_core()
            .segments_to_string(&segments);
        self.exit_buffer();
        output
    }

    /// The recorded output as plain text (or with ANSI styles), as
    /// `Console(record=True).export_text()`.
    #[pyo3(signature = (*, clear=true, styles=false))]
    fn export_text(&self, clear: bool, styles: bool) -> PyResult<String> {
        let segments = self.recorded(clear)?;
        Ok(if styles {
            segments
                .iter()
                .map(|segment| match &segment.style {
                    Some(style) => style.render(&segment.text, Some(ColorSystem::Truecolor)),
                    None => segment.text.clone(),
                })
                .collect()
        } else {
            segments
                .iter()
                .filter(|segment| !segment.control)
                .map(|segment| segment.text.as_str())
                .collect()
        })
    }

    #[pyo3(signature = (path, *, clear=true, styles=false))]
    fn save_text(
        &self,
        py: Python<'_>,
        path: &Bound<'_, PyAny>,
        clear: bool,
        styles: bool,
    ) -> PyResult<()> {
        let text = self.export_text(clear, styles)?;
        Console::write_file(py, path, &text)
    }

    /// The recorded output as an HTML document.
    #[pyo3(signature = (*, theme=None, clear=true, code_format=None, inline_styles=false))]
    fn export_html(
        &self,
        theme: Option<PyRef<'_, TerminalTheme>>,
        clear: bool,
        code_format: Option<&str>,
        inline_styles: bool,
    ) -> PyResult<String> {
        let theme = terminal_theme(theme, rich::terminal_theme::DEFAULT_TERMINAL_THEME);
        let segments = self.recorded(clear)?;
        rich::export::export_html_with(&segments, &theme, code_format, inline_styles)
            .map_err(|error| PyKeyError::new_err(error.to_string()))
    }

    #[pyo3(signature = (path, *, theme=None, clear=true, code_format=None, inline_styles=false))]
    fn save_html(
        &self,
        py: Python<'_>,
        path: &Bound<'_, PyAny>,
        theme: Option<PyRef<'_, TerminalTheme>>,
        clear: bool,
        code_format: Option<&str>,
        inline_styles: bool,
    ) -> PyResult<()> {
        let html = self.export_html(theme, clear, code_format, inline_styles)?;
        Console::write_file(py, path, &html)
    }

    /// The recorded output as an SVG image of a terminal window.
    #[pyo3(signature = (
        *, title="Rich", theme=None, clear=true, code_format=None, font_aspect_ratio=0.61,
        unique_id=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn export_svg(
        &self,
        py: Python<'_>,
        title: &str,
        theme: Option<PyRef<'_, TerminalTheme>>,
        clear: bool,
        code_format: Option<&str>,
        font_aspect_ratio: f64,
        unique_id: Option<String>,
    ) -> PyResult<String> {
        let theme = terminal_theme(theme, rich::terminal_theme::SVG_EXPORT_THEME);
        let width = self.state().settings.width;
        let segments = self.recorded(clear)?;
        let unique_id = match unique_id {
            Some(id) => id,
            None => {
                // Rich hashes the segments' Python reprs, which the port
                // cannot reproduce; this is as stable, but different.
                let mut content = String::new();
                for segment in segments.iter().filter(|s| !s.control) {
                    content.push_str(&format!("{:?}", segment));
                }
                content.push_str(title);
                let checksum: u64 = py
                    .import("zlib")?
                    .call_method1(
                        "adler32",
                        (pyo3::types::PyBytes::new(py, content.as_bytes()),),
                    )?
                    .extract()?;
                format!("terminal-{checksum}")
            }
        };
        rich::svg::export_svg_with(
            &segments,
            &theme,
            title,
            &unique_id,
            width,
            code_format.unwrap_or(rich::svg::CONSOLE_SVG_FORMAT),
            font_aspect_ratio,
        )
        .map_err(|error| PyKeyError::new_err(error.to_string()))
    }

    #[pyo3(signature = (
        path, *, title="Rich", theme=None, clear=true, code_format=None, font_aspect_ratio=0.61,
        unique_id=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn save_svg(
        &self,
        py: Python<'_>,
        path: &Bound<'_, PyAny>,
        title: &str,
        theme: Option<PyRef<'_, TerminalTheme>>,
        clear: bool,
        code_format: Option<&str>,
        font_aspect_ratio: f64,
        unique_id: Option<String>,
    ) -> PyResult<()> {
        let svg = self.export_svg(
            py,
            title,
            theme,
            clear,
            code_format,
            font_aspect_ratio,
            unique_id,
        )?;
        Console::write_file(py, path, &svg)
    }

    // --- methods other areas implement --------------------------------------

    /// `Console.status(...)`: see the live area (`live.rs`).
    #[pyo3(signature = (*args, **kwargs))]
    fn status(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        crate::live::console_status(slf, args, kwargs)
    }

    /// `Console.pager(...)`: see the live area (`live.rs`).
    #[pyo3(signature = (*args, **kwargs))]
    fn pager(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        crate::live::console_pager(slf, args, kwargs)
    }

    /// `Console.screen(...)`: see the live area (`live.rs`).
    #[pyo3(signature = (*args, **kwargs))]
    fn screen(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        crate::live::console_screen(slf, args, kwargs)
    }

    /// `Console.print_exception(...)`: see the code area (`code.rs`).
    #[pyo3(signature = (*args, **kwargs))]
    fn print_exception(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        crate::code::console_print_exception(slf, args, kwargs)
    }
}

impl Console {
    /// Print one core renderable (what `Console.print(renderable)` does).
    /// For areas: `Console::print_core(console, Box::new(...))`.
    #[allow(dead_code)] // for the areas' Console methods (`print_exception`, ...)
    pub(crate) fn print_core(
        slf: &Bound<'_, Console>,
        renderable: Box<dyn Renderable>,
    ) -> PyResult<()> {
        Console::print_item(slf, Item::Core(renderable), PrintArgs::default())
    }

    /// Print one item with `print`'s console style, soft wrap and crop.
    fn print_item(slf: &Bound<'_, Console>, item: Item, args: PrintArgs<'_>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let snapshot = this.snapshot(py);
        let settings = &snapshot.settings;
        let core = snapshot.default_core();
        let (mut no_wrap, mut overflow, mut crop) = (args.no_wrap, args.overflow, args.crop);
        if args.soft_wrap.unwrap_or(settings.soft_wrap) {
            no_wrap = no_wrap.or(Some(true));
            overflow = overflow.or(Some(Overflow::Ignore));
            crop = false;
        }
        let ambient = snapshot.ambient(slf, snapshot.base(py)?);
        let switches = Switches {
            emoji: settings.emoji,
            markup: settings.markup,
            highlight: settings.highlight,
            extensions: snapshot.extensions.clone(),
            highlighter: snapshot.highlighter.as_ref().map(|h| h.clone_ref(py)),
            emoji_variant: snapshot.settings.emoji_variant,
        };
        let segments = renderable::scope(ambient, || {
            let items = apply_hooks(py, &snapshot.hooks, vec![item], &switches)?;
            let mut options = core.options();
            options.overflow = overflow;
            options.no_wrap = no_wrap;
            let console_style = settings
                .style
                .as_ref()
                .map(|style| resolve_style(&core, style))
                .transpose()?;
            let mut segments = Vec::new();
            for item in &items {
                let mut rendered = item.render(py, &core, &options)?;
                if let Some(style) = &console_style {
                    rendered = CoreSegment::apply_style(&rendered, style);
                }
                segments.extend(rendered);
            }
            Ok(if crop {
                CoreSegment::crop_lines(&segments, core.width())
            } else {
                segments
            })
        })?;
        this.emit(py, &snapshot, segments)
    }
}

/// `rich.console.Capture`: what `Console.capture()` returns.
#[pyclass(name = "Capture", module = "rs_rich.console", frozen)]
pub(crate) struct Capture {
    console: Py<Console>,
    result: Mutex<Option<String>>,
}

#[pymethods]
impl Capture {
    fn __enter__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf.get().console.get().begin_capture();
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, PyTuple>) {
        let output = self.console.get().end_capture(py);
        *lock(&self.result) = Some(output);
    }

    /// The captured output, once the `with` block has ended.
    fn get(&self) -> PyResult<String> {
        lock(&self.result).clone().ok_or_else(|| {
            CaptureError::new_err("Capture result is not available until context manager exits.")
        })
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.console)
    }
}

/// `rich.console.ThemeContext`: what `Console.use_theme()` returns.
#[pyclass(name = "ThemeContext", module = "rs_rich.console", frozen)]
pub(crate) struct ThemeContext {
    console: Py<Console>,
    theme: Py<Theme>,
    /// Kept for Rich's signature; Rich's context always inherits.
    #[allow(dead_code)]
    inherit: bool,
}

#[pymethods]
impl ThemeContext {
    fn __enter__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        let py = slf.py();
        let this = slf.get();
        this.console
            .get()
            .push_theme(this.theme.bind(py).borrow(), true);
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, _args: &Bound<'_, PyTuple>) -> PyResult<()> {
        self.console.get().pop_theme()
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.console)?;
        visit.call(&self.theme)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<PrintItem>(m)?;
    m.add_class::<Console>()?;
    m.add_class::<Capture>()?;
    m.add_class::<ThemeContext>()?;
    Ok(())
}
