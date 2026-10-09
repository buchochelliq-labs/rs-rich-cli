//! Widgets written in Python: subclass `Widget` and make a node of it with
//! `widget(w)`, as a Rust app implements the `Widget` trait.
//!
//! The Rust side ([`PyWidget`]) implements the trait by calling the Python
//! object's methods. What it hands them (`DrawCx`, `Canvas`, `EventCx`,
//! `MeasureCx`) is lent for the one call ([`Lent`]): a widget that keeps
//! one and uses it later gets a `RuntimeError`, never a dangling pointer.
//! Methods the subclass does not override keep the trait's defaults
//! without calling Python. `name()`, `children()`, `focusable()`,
//! `retained()`, `viewport()` and `previews_keys()` are read once, when the
//! node is made.

use std::collections::HashSet;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Mutex;

use pyo3::exceptions::{PyNotImplementedError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString, PyTuple};

use rich::Style as CoreStyle;
use rich_intuituive as intuituive;
use rich_intuituive::node::Axis as CoreAxis;
use rich_intuituive::screen::Rect;
use rich_intuituive::widget::{
    Canvas, DrawCx, EventCx, MeasureCx, ScrollCx, Used, Widget as CoreWidget, WidgetEvent,
};
use rich_intuituive::Node;

use super::app::{PyAccessState, PyCtx, PyTheme};
use super::node::{segment_lines, take_nodes, PyMouse};
use super::{
    axis_arg, current, invoke, rect_arg, role_arg, Axis, Handle, Lent, LentRef, Loan, PyRect,
};
use crate::ext::common::style;
use crate::renderable;

// ---------------------------------------------------------------------------
// The base class

/// `Widget`: subclass it to write a widget. Override `draw(cx, canvas)`
/// (required) and any of `event(cx, event)` (return `True` when it used
/// the event), `measure(cx, axis, width, height)`, `layout(cx, rect)`,
/// `scroll(cx)`, `caret()`, `role()`, `cursor()`, `access_state()`,
/// `describe()`; and, read once when `widget(w)` makes its node, `name()`,
/// `children()` (the nodes it holds), `focusable()`, `retained()`,
/// `viewport()` and `previews_keys()`.
#[pyclass(name = "Widget", module = "rs_rich.tui", subclass, unsendable)]
pub(crate) struct PyWidgetBase;

#[pymethods]
impl PyWidgetBase {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        PyWidgetBase
    }

    /// What the inspector (and a stylesheet's kind selector) calls it.
    fn name(&self) -> &'static str {
        "widget"
    }

    /// More for the inspector to show after the name.
    fn describe(&self) -> Option<String> {
        None
    }

    /// The nodes it holds, shown in Tab order (read once).
    fn children(&self) -> Vec<Py<PyAny>> {
        Vec::new()
    }

    /// Cells along `axis` it needs in `width` x `height` (0: as tall as it
    /// likes). Default: all it is given.
    fn measure(
        &self,
        cx: &Bound<'_, PyAny>,
        axis: &Bound<'_, PyAny>,
        width: u16,
        height: u16,
    ) -> PyResult<u16> {
        let _ = cx;
        Ok(match axis_arg(axis)? {
            CoreAxis::Horizontal => width,
            CoreAxis::Vertical => height,
        })
    }

    /// Where each child goes, given its own `rect`. Default: nowhere.
    fn layout(&self, cx: &Bound<'_, PyAny>, rect: &Bound<'_, PyAny>) -> Vec<PyRect> {
        let _ = (cx, rect);
        Vec::new()
    }

    /// Draw itself (not its children) on `canvas`.
    fn draw(
        slf: &Bound<'_, Self>,
        cx: &Bound<'_, PyAny>,
        canvas: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let _ = (cx, canvas);
        Err(PyNotImplementedError::new_err(format!(
            "{} must define draw(cx, canvas)",
            slf.get_type().name()?
        )))
    }

    fn retained(&self) -> bool {
        false
    }

    fn viewport(&self) -> bool {
        false
    }

    /// For a viewport: `(window, (x, y))`, where the window is in its own
    /// cells and `(x, y)` the content's point at its top left.
    fn scroll(&self, cx: &Bound<'_, PyAny>) -> (PyRect, (u16, u16)) {
        let _ = cx;
        (PyRect(Rect::default()), (0, 0))
    }

    /// Handle an event; `True` when it used it.
    fn event(&self, cx: &Bound<'_, PyAny>, event: &Bound<'_, PyAny>) -> bool {
        let _ = (cx, event);
        false
    }

    fn focusable(&self) -> bool {
        false
    }

    fn previews_keys(&self) -> bool {
        false
    }

    /// Where the text caret goes while it has the focus.
    fn caret(&self) -> Option<(u16, u16)> {
        None
    }

    fn role(&self) -> super::Role {
        super::Role::Group
    }

    /// Where its selected item is, in its own cells.
    fn cursor(&self) -> Option<PyRect> {
        None
    }

    fn access_state(&self) -> PyAccessState {
        PyAccessState(intuituive::a11y::AccessState::default())
    }
}

// ---------------------------------------------------------------------------
// What a widget is handed

/// `DrawCx`: what a widget sees while it draws.
#[pyclass(name = "DrawCx", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyDrawCx {
    cx: Rc<Lent<DrawCx<'static>>>,
}

impl PyDrawCx {
    fn with<R>(&self, f: impl FnOnce(&mut DrawCx<'static>) -> R) -> PyResult<R> {
        self.cx.with("DrawCx", f)
    }
}

#[pymethods]
impl PyDrawCx {
    /// This widget's node id.
    #[getter]
    fn id(&self) -> PyResult<u64> {
        self.with(|cx| cx.id())
    }

    /// Whether everything must be drawn, on a clear canvas.
    fn repaint(&self) -> PyResult<bool> {
        self.with(|cx| cx.repaint())
    }

    /// Whether it has the focus (asking redraws it when that changes).
    fn focused(&self) -> PyResult<bool> {
        self.with(|cx| cx.focused())
    }

    /// Whether the focus is on it or inside it.
    fn focus_within(&self) -> PyResult<bool> {
        self.with(|cx| cx.focus_within())
    }

    /// Whether the pointer is over it (asking redraws it when that changes).
    fn hovered(&self) -> PyResult<bool> {
        self.with(|cx| cx.hovered())
    }

    /// Turn on pointer movement events, without redrawing on them.
    fn report_movement(&self) -> PyResult<()> {
        self.with(|cx| cx.report_movement())
    }

    /// Where the pointer is over it, `(x, y)` in its own cells.
    fn pointer(&self) -> PyResult<Option<(u16, u16)>> {
        self.with(|cx| cx.pointer())
    }

    /// A theme style by name (`"accent"`), or `fallback` parsed.
    #[pyo3(signature = (name, fallback=""))]
    fn style(&self, name: &str, fallback: &str) -> PyResult<crate::style::Style> {
        self.with(|cx| crate::style::Style::from_core(cx.style(name, fallback)))
    }

    /// The app's theme.
    fn theme(&self) -> PyResult<PyTheme> {
        self.with(|cx| PyTheme(cx.theme().clone()))
    }

    /// Whether the frame is drawn as text for assistive technology.
    fn text_mode(&self) -> bool {
        intuituive::a11y::text_mode()
    }
}

/// `Canvas`: the widget's rectangle of the screen, in its own cells.
/// Writes outside it are cut.
#[pyclass(name = "Canvas", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyCanvas {
    canvas: Lent<Canvas<'static>>,
    cx: Rc<Lent<DrawCx<'static>>>,
}

impl PyCanvas {
    fn with<R>(&self, f: impl FnOnce(&mut Canvas<'static>) -> R) -> PyResult<R> {
        self.canvas.with("Canvas", f)
    }
}

fn opt_style(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreStyle>> {
    style(value.filter(|v| !v.is_none()))
}

fn lines_arg(value: &Bound<'_, PyAny>) -> PyResult<Vec<Vec<rich::Segment>>> {
    segment_lines(value)?
        .ok_or_else(|| PyTypeError::new_err("lines are a list of lists of Segment"))
}

#[pymethods]
impl PyCanvas {
    #[getter]
    fn width(&self) -> PyResult<u16> {
        self.with(|c| c.width())
    }

    #[getter]
    fn height(&self) -> PyResult<u16> {
        self.with(|c| c.height())
    }

    /// Write rendered `lines` (lists of `Segment`s) over the whole canvas.
    fn lines(&self, lines: &Bound<'_, PyAny>) -> PyResult<()> {
        let lines = lines_arg(lines)?;
        self.with(|c| c.lines(&lines))
    }

    /// Write `lines` into the part at `x`, `y`, `width` x `height`.
    fn lines_at(
        &self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        lines: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let lines = lines_arg(lines)?;
        self.with(|c| c.lines_at(x, y, width, height, &lines))
    }

    /// Write `text` from `x`, `y` in `style`.
    #[pyo3(signature = (x, y, text, style=None))]
    fn print(&self, x: u16, y: u16, text: &str, style: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let style = opt_style(style)?;
        self.with(|c| c.print(x, y, text, style.as_ref()))
    }

    /// Write console `markup` on one row from `x`, `y`, at most `width`
    /// cells, with `style` under it; the cells it took.
    #[pyo3(signature = (x, y, width, markup, style=None))]
    fn markup(
        &self,
        x: u16,
        y: u16,
        width: u16,
        markup: &str,
        style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<u16> {
        let style = opt_style(style)?;
        self.cx.with("DrawCx", |cx| {
            self.with(|c| c.markup(cx.console(), x, y, width, markup, style.as_ref()))
        })?
    }

    /// Render any renderable into the part at `x`, `y`, `width` x
    /// `height`.
    fn render(
        &self,
        py: Python<'_>,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        renderable: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        super::node::render_scope(py, width.into(), height.into(), || {
            let renderable = renderable::to_renderable(renderable, None)?;
            self.cx.with("DrawCx", |cx| {
                self.with(|c| c.render(cx.console(), x, y, width, height, &*renderable))
            })?
        })
    }

    /// Set the cell at `x`, `y` to `char`.
    #[pyo3(signature = (x, y, char, style=None))]
    fn set(&self, x: u16, y: u16, char: char, style: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let style = opt_style(style)?;
        self.with(|c| c.set(x, y, char, style.as_ref()))
    }

    /// Fill a part with spaces in `style`.
    #[pyo3(signature = (x, y, width, height, style=None))]
    fn fill(
        &self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let style = opt_style(style)?;
        self.with(|c| c.fill(x, y, width, height, style.as_ref()))
    }

    /// Clear a part.
    fn clear(&self, x: u16, y: u16, width: u16, height: u16) -> PyResult<()> {
        self.with(|c| c.clear(x, y, width, height))
    }

    /// Lay `style` over a part's cells, keeping their text.
    fn restyle(
        &self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        style: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let style = crate::ext::common::required_style(style)?;
        self.with(|c| c.restyle(x, y, width, height, &style))
    }

    /// Move everything up `rows` rows.
    fn scroll_up(&self, rows: u16) -> PyResult<()> {
        self.with(|c| c.scroll_up(rows))
    }

    /// A rounded border on the edges, with `title` in the top one.
    #[pyo3(signature = (title="", style=None, title_style=None))]
    fn border(
        &self,
        title: &str,
        style: Option<&Bound<'_, PyAny>>,
        title_style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let style = opt_style(style)?.unwrap_or_default();
        let title_style = opt_style(title_style)?.unwrap_or_default();
        self.with(|c| c.border(title, &style, &title_style))
    }
}

/// `EventCx`: what a widget can do while it handles an event: `app()` is
/// the `Ctx` (quit, open a screen, move the focus), `redraw()` draws it
/// again, `capture_mouse()` keeps the mouse during a drag.
#[pyclass(name = "EventCx", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyEventCx {
    cx: Lent<EventCx<'static>>,
}

impl PyEventCx {
    fn with<R>(&self, f: impl FnOnce(&mut EventCx<'static>) -> R) -> PyResult<R> {
        self.cx.with("EventCx", f)
    }
}

#[pymethods]
impl PyEventCx {
    /// The `Ctx`, for as long as this event.
    fn app<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyCtx>> {
        let ctx = self.with(|cx| NonNull::from(cx.app()))?;
        PyCtx::lend_ptr(py, ctx, self.cx.token())
    }

    /// Draw again: its own state changed.
    fn redraw(&self) -> PyResult<()> {
        self.with(|cx| cx.redraw())
    }

    fn capture_mouse(&self) -> PyResult<()> {
        self.with(|cx| cx.capture_mouse())
    }

    fn release_mouse(&self) -> PyResult<()> {
        self.with(|cx| cx.release_mouse())
    }

    /// Its width and height when it last drew.
    fn size(&self) -> PyResult<(u16, u16)> {
        self.with(|cx| cx.size())
    }

    /// Where it is on the screen.
    fn rect(&self) -> PyResult<PyRect> {
        self.with(|cx| PyRect(cx.rect()))
    }

    fn focused(&self) -> PyResult<bool> {
        self.with(|cx| cx.focused())
    }
}

/// `MeasureCx`: what a widget sees while it measures or lays out; children
/// are named by their index in `children()`.
#[pyclass(name = "MeasureCx", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyMeasureCx {
    cx: LentRef<MeasureCx<'static>>,
    children: LentRef<[Node]>,
}

impl PyMeasureCx {
    fn child<R>(&self, index: usize, f: impl FnOnce(&MeasureCx, &Node) -> R) -> PyResult<R> {
        self.cx.with("MeasureCx", |cx| {
            self.children.with("MeasureCx", |children| {
                children
                    .get(index)
                    .map(|child| f(cx, child))
                    .ok_or_else(|| {
                        pyo3::exceptions::PyIndexError::new_err(format!(
                            "no child {index}: the widget holds {}",
                            children.len()
                        ))
                    })
            })?
        })?
    }
}

#[pymethods]
impl PyMeasureCx {
    /// How many cells along `axis` child `index`'s content needs.
    fn measure(
        &self,
        index: usize,
        axis: &Bound<'_, PyAny>,
        width: u16,
        height: u16,
    ) -> PyResult<u16> {
        let axis = axis_arg(axis)?;
        self.child(index, |cx, child| cx.measure(child, axis, width, height))
    }

    /// The cells along `axis` child `index` takes as a column or row sizes
    /// it.
    fn extent(
        &self,
        index: usize,
        axis: &Bound<'_, PyAny>,
        width: u16,
        height: u16,
    ) -> PyResult<u16> {
        let axis = axis_arg(axis)?;
        self.child(index, |cx, child| cx.extent(child, axis, width, height))
    }

    /// Lay every child out along `axis` in `rect`, `gap` apart, as a column
    /// or row does.
    fn stack(
        &self,
        axis: &Bound<'_, PyAny>,
        gap: u16,
        rect: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<PyRect>> {
        let axis = axis_arg(axis)?;
        let rect = rect_arg(rect)?;
        self.cx.with("MeasureCx", |cx| {
            self.children.with("MeasureCx", |children| {
                cx.stack(axis, gap, children, rect)
                    .into_iter()
                    .map(PyRect)
                    .collect()
            })
        })?
    }
}

/// `ScrollCx`: what a viewport sees when it decides where to scroll: the
/// `content`'s `(width, height)`, and the `focused` node inside, as `(id,
/// Rect)`, if any.
#[pyclass(name = "ScrollCx", module = "rs_rich.tui", frozen)]
pub(crate) struct PyScrollCx {
    #[pyo3(get)]
    content: (u16, u16),
    #[pyo3(get)]
    focused: Option<(u64, PyRect)>,
}

/// `WidgetEvent`: what a widget's `event` gets. `kind` is `"key"`,
/// `"key_up"`, `"preview"` (with `key`, a key name), `"mouse"` (with
/// `mouse`), `"paste"` (with `text`), `"focus"` or `"hover"` (with
/// `value`, whether it came or left) or `"resize"` (with `width` and
/// `height`).
#[pyclass(name = "WidgetEvent", module = "rs_rich.tui", frozen)]
pub(crate) struct PyWidgetEvent {
    #[pyo3(get)]
    kind: &'static str,
    #[pyo3(get)]
    key: Option<String>,
    #[pyo3(get)]
    mouse: Option<Py<PyMouse>>,
    #[pyo3(get)]
    text: Option<String>,
    #[pyo3(get)]
    value: Option<bool>,
    #[pyo3(get)]
    width: Option<u16>,
    #[pyo3(get)]
    height: Option<u16>,
}

#[pymethods]
impl PyWidgetEvent {
    fn __repr__(&self) -> String {
        match (&self.key, &self.text, self.value) {
            (Some(key), _, _) => format!("WidgetEvent({}, {key:?})", self.kind),
            (_, Some(text), _) => format!("WidgetEvent({}, {text:?})", self.kind),
            (_, _, Some(value)) => format!("WidgetEvent({}, {value})", self.kind),
            _ => format!("WidgetEvent({})", self.kind),
        }
    }
}

fn widget_event(py: Python<'_>, event: &WidgetEvent) -> PyResult<PyWidgetEvent> {
    let mut out = PyWidgetEvent {
        kind: "unknown",
        key: None,
        mouse: None,
        text: None,
        value: None,
        width: None,
        height: None,
    };
    match event {
        WidgetEvent::Key(key) => {
            out.kind = "key";
            out.key = Some(key.to_string());
        }
        WidgetEvent::KeyUp(key) => {
            out.kind = "key_up";
            out.key = Some(key.to_string());
        }
        WidgetEvent::Preview(key) => {
            out.kind = "preview";
            out.key = Some(key.to_string());
        }
        WidgetEvent::Mouse(mouse) => {
            out.kind = "mouse";
            out.mouse = Some(Py::new(py, PyMouse(*mouse))?);
        }
        WidgetEvent::Paste(text) => {
            out.kind = "paste";
            out.text = Some(text.clone());
        }
        WidgetEvent::Focus(on) => {
            out.kind = "focus";
            out.value = Some(*on);
        }
        WidgetEvent::Hover(on) => {
            out.kind = "hover";
            out.value = Some(*on);
        }
        WidgetEvent::Resize { width, height } => {
            out.kind = "resize";
            out.width = Some(*width);
            out.height = Some(*height);
        }
        _ => {}
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// The bridge

/// Names given to widgets, kept for the program's life (the trait returns
/// `&'static str`); each distinct name once.
fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut names = NAMES.lock().unwrap_or_else(|p| p.into_inner());
    let names = names.get_or_insert_with(HashSet::new);
    if let Some(found) = names.get(name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// Which optional methods the subclass overrides.
#[derive(Default)]
struct Overrides {
    describe: bool,
    measure: bool,
    layout: bool,
    scroll: bool,
    event: bool,
    caret: bool,
    role: bool,
    cursor: bool,
    access_state: bool,
}

/// A Python widget, as a Rust one.
struct PyWidget {
    object: Py<PyAny>,
    name: &'static str,
    children: Vec<Node>,
    overrides: Overrides,
    focusable: bool,
    retained: bool,
    viewport: bool,
    previews_keys: bool,
    app: Option<Handle>,
}

impl PyWidget {
    fn call<R>(
        &self,
        method: &str,
        args: impl for<'py> FnOnce(Python<'py>) -> PyResult<Bound<'py, PyTuple>>,
        convert: impl for<'py> FnOnce(&Bound<'py, PyAny>) -> PyResult<R>,
    ) -> Option<R> {
        invoke(self.app.as_ref(), |py| {
            let value = self.object.bind(py).call_method1(method, args(py)?)?;
            convert(&value)
        })
    }

    /// Lend the measure context and the children to Python for one call.
    fn measure_cx<'py>(
        &self,
        py: Python<'py>,
        cx: &MeasureCx,
        loan: &Loan,
    ) -> PyResult<Bound<'py, PyMeasureCx>> {
        let cx: NonNull<MeasureCx<'static>> = NonNull::from(cx).cast();
        let children = NonNull::from(self.children.as_slice());
        // SAFETY: the loan ends before `cx` and `self.children` are
        // borrowed differently: every caller drops it first.
        let (cx, children) = unsafe {
            (
                LentRef::new(cx, loan.token()),
                LentRef::new(children, loan.token()),
            )
        };
        Bound::new(py, PyMeasureCx { cx, children })
    }
}

impl CoreWidget for PyWidget {
    fn name(&self) -> &'static str {
        self.name
    }

    fn describe(&self) -> Option<String> {
        if !self.overrides.describe {
            return None;
        }
        self.call("describe", |py| Ok(PyTuple::empty(py)), |v| v.extract())
            .flatten()
    }

    fn measure(&mut self, cx: &MeasureCx, axis: CoreAxis, width: u16, height: u16) -> u16 {
        let fallback = match axis {
            CoreAxis::Horizontal => width,
            CoreAxis::Vertical => height,
        };
        if !self.overrides.measure {
            return fallback;
        }
        let loan = Loan::new();
        let axis = Axis::from_rust(axis).expect("every axis");
        self.call(
            "measure",
            |py| {
                let cx = self.measure_cx(py, cx, &loan)?;
                PyTuple::new(
                    py,
                    [
                        cx.into_any(),
                        Bound::new(py, axis)?.into_any(),
                        width.into_pyobject(py)?.into_any(),
                        height.into_pyobject(py)?.into_any(),
                    ],
                )
            },
            |v| v.extract::<u16>(),
        )
        .unwrap_or(fallback)
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        if !self.overrides.layout {
            return Vec::new();
        }
        let loan = Loan::new();
        self.call(
            "layout",
            |py| {
                let cx = self.measure_cx(py, cx, &loan)?;
                PyTuple::new(
                    py,
                    [cx.into_any(), Bound::new(py, PyRect(rect))?.into_any()],
                )
            },
            |rects| {
                rects
                    .try_iter()?
                    .map(|r| {
                        let r = r?;
                        if r.is_none() {
                            Ok(Rect::default())
                        } else {
                            rect_arg(&r)
                        }
                    })
                    .collect()
            },
        )
        .unwrap_or_default()
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let loan = Loan::new();
        let cx: NonNull<DrawCx<'static>> = NonNull::from(cx).cast();
        let canvas: NonNull<Canvas<'static>> = NonNull::from(canvas).cast();
        self.call(
            "draw",
            |py| {
                // SAFETY: `loan` is dropped below, before `cx` and `canvas`
                // go back to the caller.
                let (cx, canvas) = unsafe {
                    (
                        Rc::new(Lent::new(cx, loan.token())),
                        Lent::new(canvas, loan.token()),
                    )
                };
                let draw_cx = Bound::new(py, PyDrawCx { cx: cx.clone() })?;
                let canvas = Bound::new(py, PyCanvas { canvas, cx })?;
                PyTuple::new(py, [draw_cx.into_any(), canvas.into_any()])
            },
            |_| Ok(()),
        );
        drop(loan);
    }

    fn retained(&self) -> bool {
        self.retained
    }

    fn viewport(&self) -> bool {
        self.viewport
    }

    fn scroll(&mut self, cx: &ScrollCx) -> (Rect, (u16, u16)) {
        if !self.overrides.scroll {
            return (Rect::default(), (0, 0));
        }
        let (content, focused) = (cx.content(), cx.focused());
        self.call(
            "scroll",
            |py| {
                let cx = Bound::new(
                    py,
                    PyScrollCx {
                        content,
                        focused: focused.map(|(id, rect)| (id, PyRect(rect))),
                    },
                )?;
                PyTuple::new(py, [cx])
            },
            |value| {
                let (window, at): (Bound<'_, PyAny>, (u16, u16)) = value.extract()?;
                Ok((rect_arg(&window)?, at))
            },
        )
        .unwrap_or((Rect::default(), (0, 0)))
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if !self.overrides.event {
            return Used::No;
        }
        let loan = Loan::new();
        let cx: NonNull<EventCx<'static>> = NonNull::from(cx).cast();
        let used = self
            .call(
                "event",
                |py| {
                    // SAFETY: `loan` is dropped below, before `cx` goes back.
                    let cx = unsafe { Lent::new(cx, loan.token()) };
                    let cx = Bound::new(py, PyEventCx { cx })?;
                    let event = Bound::new(py, widget_event(py, event)?)?;
                    PyTuple::new(py, [cx.into_any(), event.into_any()])
                },
                |used| used.is_truthy(),
            )
            .unwrap_or(true);
        drop(loan);
        if used {
            Used::Yes
        } else {
            Used::No
        }
    }

    fn focusable(&self) -> bool {
        self.focusable
    }

    fn previews_keys(&self) -> bool {
        self.previews_keys
    }

    fn caret(&self) -> Option<(u16, u16)> {
        if !self.overrides.caret {
            return None;
        }
        self.call("caret", |py| Ok(PyTuple::empty(py)), |v| v.extract())
            .flatten()
    }

    fn role(&self) -> intuituive::a11y::Role {
        if !self.overrides.role {
            return intuituive::a11y::Role::Group;
        }
        self.call("role", |py| Ok(PyTuple::empty(py)), role_arg)
            .unwrap_or_default()
    }

    fn cursor(&self) -> Option<Rect> {
        if !self.overrides.cursor {
            return None;
        }
        self.call(
            "cursor",
            |py| Ok(PyTuple::empty(py)),
            |v| {
                if v.is_none() {
                    Ok(None)
                } else {
                    rect_arg(v).map(Some)
                }
            },
        )
        .flatten()
    }

    fn access_state(&self) -> intuituive::a11y::AccessState {
        if !self.overrides.access_state {
            return intuituive::a11y::AccessState::default();
        }
        self.call(
            "access_state",
            |py| Ok(PyTuple::empty(py)),
            |v| {
                if v.is_none() {
                    return Ok(intuituive::a11y::AccessState::default());
                }
                Ok(v.cast::<PyAccessState>()?.get().0)
            },
        )
        .unwrap_or_default()
    }
}

/// The node `widget(w)` makes from a `Widget` subclass instance.
pub(crate) fn node_of(object: &Bound<'_, PyAny>) -> PyResult<Node> {
    let py = object.py();
    if !object.is_instance_of::<PyWidgetBase>() {
        return Err(PyTypeError::new_err(format!(
            "widget() takes a Widget subclass instance, got {}",
            object.get_type().name()?
        )));
    }
    let class = object.get_type();
    let base = py.get_type::<PyWidgetBase>();
    let overrides =
        |name: &str| -> PyResult<bool> { Ok(!class.getattr(name)?.is(&base.getattr(name)?)) };
    let flag = |name: &str| -> PyResult<bool> { object.call_method0(name)?.is_truthy() };
    let name: String = {
        let value = object.call_method0("name")?;
        value
            .cast::<PyString>()
            .map_err(|_| PyTypeError::new_err("a widget's name() returns a str"))?
            .to_cow()?
            .into_owned()
    };
    let children = take_nodes(&object.call_method0("children")?)?;
    let widget = PyWidget {
        object: object.clone().unbind(),
        name: intern(&name),
        children,
        overrides: Overrides {
            describe: overrides("describe")?,
            measure: overrides("measure")?,
            layout: overrides("layout")?,
            scroll: overrides("scroll")?,
            event: overrides("event")?,
            caret: overrides("caret")?,
            role: overrides("role")?,
            cursor: overrides("cursor")?,
            access_state: overrides("access_state")?,
        },
        focusable: flag("focusable")?,
        retained: flag("retained")?,
        viewport: flag("viewport")?,
        previews_keys: flag("previews_keys")?,
        app: current(),
    };
    Ok(intuituive::widget::widget(widget))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("TuiWidget", py.get_type::<PyWidgetBase>()),
        ("TuiDrawCx", py.get_type::<PyDrawCx>()),
        ("TuiCanvas", py.get_type::<PyCanvas>()),
        ("TuiEventCx", py.get_type::<PyEventCx>()),
        ("TuiMeasureCx", py.get_type::<PyMeasureCx>()),
        ("TuiScrollCx", py.get_type::<PyScrollCx>()),
        ("TuiWidgetEvent", py.get_type::<PyWidgetEvent>()),
    ] {
        m.add(name, class)?;
    }
    Ok(())
}
