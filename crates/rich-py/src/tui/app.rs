//! The app from Python: `App`, what handlers get (`Ctx`), `Theme`,
//! `Stylesheet`, the `Driver` for a loop of your own, headless runs
//! (`run`, `Run`), and the accessibility tree (`AccessNode`).

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::{PyOSError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use rich_intuituive as intuituive;
use rich_intuituive::interact::headless::{Headless, Script as CoreScript};
use rich_intuituive::interact::{BackendKind, Event, Key};
use rich_intuituive::{Anchor, Ctx};

use super::node::{mouse_from, screen_builder, take_node, PyNode};
use super::reactive::{call_with_ctx, signal_arg, PyProxy};
use super::{
    detached, easing_arg, keys, placement_arg, rect_arg, size_arg, Building, Callback, Enter,
    Handle, Lent, Loan, PyRect, Role, Shared,
};
use crate::ext::common::{scoped, seconds};

create_exception!(_native, TuiSheetError, PyValueError);

// ---------------------------------------------------------------------------
// What handlers get

/// `Ctx`: what a handler can do: quit, open and close screens, modals and
/// pop-ups, move the focus, show toasts, animate, copy, announce, switch
/// the theme. Lent for one call: keeping it and using it later raises.
#[pyclass(name = "Ctx", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyCtx {
    cx: Lent<Ctx>,
}

impl PyCtx {
    /// Lend `cx` to Python until `loan` is dropped.
    pub(crate) fn lend<'py>(
        py: Python<'py>,
        cx: &mut Ctx,
        loan: &Loan,
    ) -> PyResult<Bound<'py, PyCtx>> {
        // SAFETY: the loan ends (marking this dead) before the borrow of
        // `cx` does: every caller drops `loan` before returning.
        let cx = unsafe { Lent::new(NonNull::from(cx), loan.token()) };
        Bound::new(py, PyCtx { cx })
    }

    /// Lend the `Ctx` behind a widget's event context, for as long as that.
    pub(crate) fn lend_ptr<'py>(
        py: Python<'py>,
        cx: NonNull<Ctx>,
        token: std::rc::Rc<Cell<bool>>,
    ) -> PyResult<Bound<'py, PyCtx>> {
        // SAFETY: as above; `token` is the event context's own.
        let cx = unsafe { Lent::new(cx, token) };
        Bound::new(py, PyCtx { cx })
    }

    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut Ctx) -> R) -> PyResult<R> {
        self.cx.with("Ctx", f)
    }
}

/// A pop-up's anchor: a node's id (or the node), or a `Rect`.
pub(crate) fn anchor_arg(value: &Bound<'_, PyAny>) -> PyResult<Anchor> {
    if let Ok(node) = value.cast::<PyNode>() {
        return Ok(Anchor::Node(node.getattr("id")?.extract()?));
    }
    if let Ok(id) = value.extract::<u64>() {
        return Ok(Anchor::Node(id));
    }
    rect_arg(value)
        .map(Anchor::Rect)
        .map_err(|_| PyTypeError::new_err("an anchor is a node's id or a Rect"))
}

#[pymethods]
impl PyCtx {
    /// Open the screen `build()` returns over this one.
    fn push(&self, build: &Bound<'_, PyAny>) -> PyResult<()> {
        let build = screen_builder(Callback::checked(build, "the screen's build")?);
        self.with(|cx| cx.push(build))
    }

    /// Open a modal: `build()`'s node in a box `width` x `height` (sizes)
    /// over this screen.
    fn modal(
        &self,
        width: &Bound<'_, PyAny>,
        height: &Bound<'_, PyAny>,
        build: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let (width, height) = (size_arg(width)?, size_arg(height)?);
        let build = screen_builder(Callback::checked(build, "the modal's build")?);
        self.with(|cx| cx.modal(width, height, build))
    }

    /// Open a pop-up next to `anchor` (a node's id or a `Rect`).
    fn popup(
        &self,
        anchor: &Bound<'_, PyAny>,
        placement: &Bound<'_, PyAny>,
        width: &Bound<'_, PyAny>,
        height: &Bound<'_, PyAny>,
        build: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let anchor = anchor_arg(anchor)?;
        let placement = placement_arg(placement)?;
        let (width, height) = (size_arg(width)?, size_arg(height)?);
        let build = screen_builder(Callback::checked(build, "the pop-up's build")?);
        self.with(|cx| cx.popup(anchor, placement, width, height, build))
    }

    /// Open the command palette: the bindings made with `bind`.
    fn palette(&self) -> PyResult<()> {
        self.with(Ctx::palette)
    }

    /// Open the help: the same bindings, with their keys.
    fn help(&self) -> PyResult<()> {
        self.with(Ctx::help)
    }

    /// Put `text` on the clipboard.
    fn copy(&self, text: &str) -> PyResult<()> {
        self.with(|cx| cx.copy(text))
    }

    /// Show `markup` in a toast for three seconds.
    fn toast(&self, markup: &str) -> PyResult<()> {
        self.with(|cx| cx.toast(markup))
    }

    /// Show `markup` in a toast for `duration` seconds.
    fn toast_for(&self, markup: &str, duration: &Bound<'_, PyAny>) -> PyResult<()> {
        let duration = seconds(duration)?;
        self.with(|cx| cx.toast_for(markup, duration))
    }

    /// Say `text` to assistive technology, now if `urgent`.
    #[pyo3(signature = (text, urgent=false))]
    fn announce(&self, text: &str, urgent: bool) -> PyResult<()> {
        self.with(|cx| cx.announce(text, urgent))
    }

    /// Move the signal `value` (made a float one) to `to` over `duration`
    /// seconds, eased.
    #[pyo3(signature = (value, to, duration, easing=None))]
    fn animate(
        &self,
        value: &Bound<'_, PyAny>,
        to: f64,
        duration: &Bound<'_, PyAny>,
        easing: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let signal = signal_arg(value, "animate's value")?
            .borrow()
            .f64(value.py(), "an animation")?;
        let duration = seconds(duration)?;
        let easing = match easing {
            Some(easing) => easing_arg(easing)?,
            None => intuituive::Easing::default(),
        };
        self.with(|cx| cx.animate(signal, to, duration, easing))
    }

    /// Where the mouse event being handled happened, as `(column, row)`.
    fn pointer(&self) -> PyResult<Option<(u16, u16)>> {
        self.with(|cx| cx.pointer())
    }

    /// Close the top screen, modal or pop-up.
    fn pop(&self) -> PyResult<()> {
        self.with(Ctx::pop)
    }

    /// Replace the top screen with the one `build()` returns.
    fn replace(&self, build: &Bound<'_, PyAny>) -> PyResult<()> {
        let build = screen_builder(Callback::checked(build, "the screen's build")?);
        self.with(|cx| cx.replace(build))
    }

    /// Switch the app to `theme`.
    fn set_theme(&self, theme: &Bound<'_, PyTheme>) -> PyResult<()> {
        let theme = theme.get().0.clone();
        self.with(|cx| cx.set_theme(theme))
    }

    /// End the app after this event.
    fn quit(&self) -> PyResult<()> {
        self.with(Ctx::quit)
    }

    fn focus_next(&self) -> PyResult<()> {
        self.with(Ctx::focus_next)
    }

    fn focus_previous(&self) -> PyResult<()> {
        self.with(Ctx::focus_previous)
    }

    /// Focus the node with `id` (or the node).
    fn focus(&self, id: &Bound<'_, PyAny>) -> PyResult<()> {
        let Anchor::Node(id) = anchor_arg(id)? else {
            return Err(PyTypeError::new_err("focus takes a node's id"));
        };
        self.with(|cx| cx.focus(id))
    }

    /// A handle for other threads.
    fn proxy(&self) -> PyResult<PyProxy> {
        let proxy = self.with(|cx| cx.proxy())?;
        Ok(PyProxy::new(proxy, super::current()))
    }
}

// ---------------------------------------------------------------------------
// Themes

/// `Theme`: the styles of what the framework draws, and named styles for
/// markup. `Theme.dark()` (the default), `Theme.light()`, `Theme.mono()`;
/// `theme.style(name, style)` returns it with one more.
#[pyclass(name = "Theme", module = "rs_rich.tui", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct PyTheme(pub(crate) intuituive::Theme);

#[pymethods]
impl PyTheme {
    #[new]
    fn new() -> PyTheme {
        PyTheme(intuituive::Theme::default())
    }

    #[staticmethod]
    fn dark() -> PyTheme {
        PyTheme(intuituive::Theme::dark())
    }

    #[staticmethod]
    fn light() -> PyTheme {
        PyTheme(intuituive::Theme::light())
    }

    #[staticmethod]
    fn mono() -> PyTheme {
        PyTheme(intuituive::Theme::mono())
    }

    /// The theme with the named style `name` (a style or its definition).
    fn style(&self, name: &str, style: &Bound<'_, PyAny>) -> PyResult<PyTheme> {
        let style = crate::ext::common::required_style(style)?;
        Ok(PyTheme(self.0.clone().style(name, style)))
    }

    /// The theme with the styles of a theme file's text (`[styles]`).
    fn with_config(&self, config: &str) -> PyResult<PyTheme> {
        self.0
            .clone()
            .with_config(config)
            .map(PyTheme)
            .map_err(PyValueError::new_err)
    }

    /// A theme file, read once.
    #[staticmethod]
    fn load(path: std::path::PathBuf) -> PyResult<PyTheme> {
        intuituive::Theme::load(path)
            .map(PyTheme)
            .map_err(PyValueError::new_err)
    }

    #[getter]
    fn border(&self) -> crate::style::Style {
        crate::style::Style::from_core(self.0.border.clone())
    }

    #[getter]
    fn border_focused(&self) -> crate::style::Style {
        crate::style::Style::from_core(self.0.border_focused.clone())
    }

    #[getter]
    fn title(&self) -> crate::style::Style {
        crate::style::Style::from_core(self.0.title.clone())
    }

    /// The named styles, as `(name, Style)` pairs.
    #[getter]
    fn styles(&self) -> Vec<(String, crate::style::Style)> {
        self.0
            .styles
            .iter()
            .map(|(name, style)| (name.clone(), crate::style::Style::from_core(style.clone())))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Stylesheets

/// `Stylesheet.parse(css)`: check a stylesheet; it raises `SheetError`
/// (with `line`, `column` and `message`) where it stops parsing.
#[pyclass(name = "Stylesheet", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyStylesheet(intuituive::Stylesheet);

#[pymethods]
impl PyStylesheet {
    #[staticmethod]
    fn parse(css: &str) -> PyResult<PyStylesheet> {
        intuituive::Stylesheet::parse(css)
            .map(PyStylesheet)
            .map_err(|error| {
                let err = TuiSheetError::new_err(error.to_string());
                Python::attach(|py| {
                    let value = err.value(py);
                    let _ = value.setattr("line", error.line);
                    let _ = value.setattr("column", error.column);
                    let _ = value.setattr("message", error.message.clone());
                });
                err
            })
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Accessibility

/// `AccessState`: ARIA's states of a node: `expanded` and `checked`
/// (`None` when it cannot be), `selected`, `busy`, and `position`, the
/// selected item's place `(at, of)`.
#[pyclass(
    name = "AccessState",
    module = "rs_rich.tui",
    frozen,
    skip_from_py_object
)]
#[derive(Clone, Copy)]
pub(crate) struct PyAccessState(pub(crate) intuituive::a11y::AccessState);

#[pymethods]
impl PyAccessState {
    /// For a widget's `access_state()`.
    #[new]
    #[pyo3(signature = (*, expanded=None, checked=None, selected=false, busy=false, position=None))]
    fn new(
        expanded: Option<bool>,
        checked: Option<bool>,
        selected: bool,
        busy: bool,
        position: Option<(usize, usize)>,
    ) -> PyAccessState {
        PyAccessState(intuituive::a11y::AccessState {
            expanded,
            checked,
            selected,
            busy,
            position,
        })
    }

    /// The state of a widget of `count` items with item `index` selected.
    #[staticmethod]
    fn item(index: usize, count: usize) -> PyAccessState {
        PyAccessState(intuituive::a11y::AccessState::item(index, count))
    }

    #[getter]
    fn expanded(&self) -> Option<bool> {
        self.0.expanded
    }

    #[getter]
    fn checked(&self) -> Option<bool> {
        self.0.checked
    }

    #[getter]
    fn selected(&self) -> bool {
        self.0.selected
    }

    #[getter]
    fn busy(&self) -> bool {
        self.0.busy
    }

    #[getter]
    fn position(&self) -> Option<(usize, usize)> {
        self.0.position
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.0)
    }
}

/// `AccessNode`: one node of the accessibility tree: `depth`, `id`, `role`
/// (a `Role`), `name`, `value`, `focused`, `disabled`, `state` and `rect`.
/// `describe()` is the line linear mode writes for it.
#[pyclass(name = "AccessNode", module = "rs_rich.tui", frozen)]
pub(crate) struct PyAccessNode(intuituive::a11y::AccessNode);

#[pymethods]
impl PyAccessNode {
    #[getter]
    fn depth(&self) -> usize {
        self.0.depth
    }

    #[getter]
    fn id(&self) -> u64 {
        self.0.id
    }

    #[getter]
    fn role(&self) -> Role {
        Role::from_rust(self.0.role).unwrap_or(Role::Group)
    }

    #[getter]
    fn name(&self) -> &str {
        &self.0.name
    }

    #[getter]
    fn value(&self) -> Option<&str> {
        self.0.value.as_deref()
    }

    #[getter]
    fn focused(&self) -> bool {
        self.0.focused
    }

    #[getter]
    fn disabled(&self) -> bool {
        self.0.disabled
    }

    #[getter]
    fn state(&self) -> PyAccessState {
        PyAccessState(self.0.state)
    }

    #[getter]
    fn rect(&self) -> PyRect {
        PyRect(self.0.rect)
    }

    fn describe(&self) -> String {
        self.0.describe()
    }

    /// Its ARIA attributes, as `(name, value)` pairs.
    fn aria_attributes(&self) -> Vec<(&'static str, String)> {
        self.0.aria_attributes()
    }

    fn __repr__(&self) -> String {
        format!("<AccessNode {}>", self.0.describe())
    }
}

/// `Announcement`: something said to assistive technology: `text`, and
/// whether it is `urgent`.
#[pyclass(name = "Announcement", module = "rs_rich.tui", frozen)]
pub(crate) struct PyAnnouncement {
    #[pyo3(get)]
    text: String,
    #[pyo3(get)]
    urgent: bool,
}

#[pymethods]
impl PyAnnouncement {
    fn __repr__(&self) -> String {
        format!("Announcement({:?}, urgent={})", self.text, self.urgent)
    }
}

impl From<intuituive::a11y::Announcement> for PyAnnouncement {
    fn from(a: intuituive::a11y::Announcement) -> PyAnnouncement {
        PyAnnouncement {
            text: a.text,
            urgent: a.urgent,
        }
    }
}

/// `FrameStats`: the last frame's counts: nodes `drawn`, `bytes` written.
#[pyclass(name = "FrameStats", module = "rs_rich.tui", frozen)]
pub(crate) struct PyFrameStats {
    #[pyo3(get)]
    drawn: usize,
    #[pyo3(get)]
    bytes: usize,
}

#[pymethods]
impl PyFrameStats {
    fn __repr__(&self) -> String {
        format!("FrameStats(drawn={}, bytes={})", self.drawn, self.bytes)
    }
}

impl From<intuituive::FrameStats> for PyFrameStats {
    fn from(stats: intuituive::FrameStats) -> PyFrameStats {
        PyFrameStats {
            drawn: stats.drawn,
            bytes: stats.bytes,
        }
    }
}

// ---------------------------------------------------------------------------
// The app

/// `App(build)`: an intuiTUIve app. `build()` runs once, now: it makes the
/// app's signals and returns the root node. The builders (`theme`,
/// `stylesheet`, `inline`, `palette_key`, ...) return the app; `run()`
/// takes the terminal until a handler quits (or Ctrl+C).
#[pyclass(name = "App", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyApp {
    app: RefCell<Option<intuituive::App>>,
    shared: Handle,
}

fn spent() -> PyErr {
    PyRuntimeError::new_err("this app has run (an app runs once; build another)")
}

impl PyApp {
    /// The app, taken to run.
    pub(crate) fn take(&self) -> PyResult<(intuituive::App, Handle)> {
        let app = self.app.borrow_mut().take().ok_or_else(spent)?;
        Ok((app, self.shared.clone()))
    }

    fn map(&self, f: impl FnOnce(intuituive::App) -> intuituive::App) -> PyResult<()> {
        let app = self.app.borrow_mut().take().ok_or_else(spent)?;
        *self.app.borrow_mut() = Some(f(app));
        Ok(())
    }

    /// Build an app from `build`, for `app`.
    pub(crate) fn build(
        py: Python<'_>,
        build: &Bound<'_, PyAny>,
        shared: Handle,
    ) -> PyResult<PyApp> {
        let build = Callback::of(
            Callback::checked(build, "App's build")?
                .func()
                .clone_ref(py),
            Some(shared.clone()),
        );
        let app = {
            let _enter = Enter::new(&shared);
            intuituive::App::new(|| {
                let _building = Building::new();
                build
                    .call(|py| Ok(PyTuple::empty(py)), take_node)
                    .unwrap_or_else(|| intuituive::label(""))
            })
        };
        shared.set_proxy(app.proxy());
        shared.check()?;
        Ok(PyApp {
            app: RefCell::new(Some(app)),
            shared,
        })
    }
}

/// The terminal's size, for the render scope around a run.
fn terminal_size() -> (usize, usize) {
    let console = rich::Console::new();
    (console.width(), console.height())
}

/// Run `f` (which drives the app) in a render scope with the GIL released,
/// then raise what a callback raised, then what the scope kept.
fn drive<T>(
    py: Python<'_>,
    shared: &Handle,
    (width, height): (usize, usize),
    terminal: bool,
    f: impl FnOnce() -> T,
) -> PyResult<T> {
    let _enter = Enter::new(shared);
    let result = scoped(py, width, height, terminal, || Ok(detached(py, f)));
    shared.check()?;
    result
}

#[pymethods]
impl PyApp {
    #[new]
    fn new(py: Python<'_>, build: &Bound<'_, PyAny>) -> PyResult<PyApp> {
        PyApp::build(py, build, Shared::new())
    }

    /// Dock the inspector on the right (F12 shows and hides it).
    #[pyo3(signature = (on=true))]
    fn inspector(slf: Bound<'_, Self>, on: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.inspector(on))?;
        Ok(slf)
    }

    /// Load styles from a theme file, again whenever it changes.
    fn theme_file(slf: Bound<'_, Self>, path: std::path::PathBuf) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.theme_file(path))?;
        Ok(slf)
    }

    /// Style and lay out the nodes with a stylesheet (a CSS subset).
    fn stylesheet<'py>(slf: Bound<'py, Self>, css: &str) -> PyResult<Bound<'py, Self>> {
        slf.borrow().map(|a| a.stylesheet(css))?;
        Ok(slf)
    }

    /// A stylesheet from a file, read again whenever it changes.
    fn stylesheet_file(
        slf: Bound<'_, Self>,
        path: std::path::PathBuf,
    ) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.stylesheet_file(path))?;
        Ok(slf)
    }

    /// Draw for assistive technology: the cursor on the focus, boxes as
    /// blanks, no colour, `>` on selected items.
    #[pyo3(signature = (on=true))]
    fn accessible(slf: Bound<'_, Self>, on: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.accessible(on))?;
        Ok(slf)
    }

    /// Linear mode: no screen; lines of text, then the lines that change.
    #[pyo3(signature = (on=true))]
    fn linear(slf: Bound<'_, Self>, on: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.linear(on))?;
        Ok(slf)
    }

    /// Call `announcer(announcement)` for each announcement as it happens.
    fn announcer<'py>(
        slf: Bound<'py, Self>,
        announcer: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let announcer = Callback::of(
            Callback::checked(announcer, "the announcer")?
                .func()
                .clone_ref(slf.py()),
            Some(slf.borrow().shared.clone()),
        );
        slf.borrow().map(|a| {
            a.announcer(move |announcement: &intuituive::a11y::Announcement| {
                announcer.call(
                    |py| {
                        let announcement = Py::new(py, PyAnnouncement::from(announcement.clone()))?;
                        PyTuple::new(py, [announcement])
                    },
                    |_| Ok(()),
                );
            })
        })?;
        Ok(slf)
    }

    /// Call `tick(cx)` every `interval` seconds.
    fn every<'py>(
        slf: Bound<'py, Self>,
        interval: &Bound<'py, PyAny>,
        tick: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, Self>> {
        let interval = seconds(interval)?;
        let tick = Callback::of(
            Callback::checked(tick, "every's tick")?
                .func()
                .clone_ref(slf.py()),
            Some(slf.borrow().shared.clone()),
        );
        slf.borrow()
            .map(|a| a.every(interval, move |cx| call_with_ctx(&tick, None, cx)))?;
        Ok(slf)
    }

    /// Run inline, in `height` rows below the cursor.
    fn inline(slf: Bound<'_, Self>, height: u16) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.inline(height))?;
        Ok(slf)
    }

    /// Before each event, wait for the tasks in flight (for tests).
    #[pyo3(signature = (on=true))]
    fn wait_for_tasks(slf: Bound<'_, Self>, on: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.wait_for_tasks(on))?;
        Ok(slf)
    }

    /// Use `theme`.
    fn theme<'py>(
        slf: Bound<'py, Self>,
        theme: &Bound<'py, PyTheme>,
    ) -> PyResult<Bound<'py, Self>> {
        let theme = theme.get().0.clone();
        slf.borrow().map(|a| a.theme(theme))?;
        Ok(slf)
    }

    /// Whether to hand each frame's plain text to the backend.
    #[pyo3(signature = (on=true))]
    fn text_frames(slf: Bound<'_, Self>, on: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.text_frames(on))?;
        Ok(slf)
    }

    /// Open the command palette with `keys`.
    fn palette_key<'py>(slf: Bound<'py, Self>, keys: &str) -> PyResult<Bound<'py, Self>> {
        super::keys(keys)?;
        slf.borrow().map(|a| a.palette_key(keys))?;
        Ok(slf)
    }

    /// Open the help with `keys`.
    fn help_key<'py>(slf: Bound<'py, Self>, keys: &str) -> PyResult<Bound<'py, Self>> {
        super::keys(keys)?;
        slf.borrow().map(|a| a.help_key(keys))?;
        Ok(slf)
    }

    /// Read keys as a legacy terminal sends them (no kitty protocol).
    #[pyo3(signature = (on=true))]
    fn legacy_keys(slf: Bound<'_, Self>, on: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.legacy_keys(on))?;
        Ok(slf)
    }

    /// Synchronized output: `True` always, `False` never, `None` detected.
    fn synchronized_output(slf: Bound<'_, Self>, on: Option<bool>) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.synchronized_output(on))?;
        Ok(slf)
    }

    /// Whether a drag that no node uses selects and copies text.
    #[pyo3(signature = (on=true))]
    fn selectable(slf: Bound<'_, Self>, on: bool) -> PyResult<Bound<'_, Self>> {
        slf.borrow().map(|a| a.selectable(on))?;
        Ok(slf)
    }

    /// A handle other threads use to change the app's state.
    fn proxy(&self) -> PyResult<PyProxy> {
        let app = self.app.borrow();
        let app = app.as_ref().ok_or_else(spent)?;
        Ok(PyProxy::new(app.proxy(), Some(self.shared.clone())))
    }

    /// Counts from the last frame.
    fn stats(&self) -> PyResult<PyFrameStats> {
        let app = self.app.borrow();
        let app = app.as_ref().ok_or_else(spent)?;
        Ok(app.stats().into())
    }

    /// Run in the terminal until a handler quits (or Ctrl+C), on this
    /// thread, with the GIL released while it waits.
    fn run(&self, py: Python<'_>) -> PyResult<()> {
        self.run_with(py, "crossterm")
    }

    /// `run`, with the terminal driven by `backend`. This wheel has
    /// crossterm only.
    fn run_with(&self, py: Python<'_>, backend: &str) -> PyResult<()> {
        let backend = match backend {
            "crossterm" => BackendKind::Crossterm,
            "termion" | "termwiz" => {
                return Err(PyValueError::new_err(format!(
                    "the {backend} backend is not built into this wheel; use crossterm"
                )))
            }
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown backend {other:?}; expected crossterm"
                )))
            }
        };
        let (app, shared) = self.take()?;
        let result = drive(py, &shared, terminal_size(), true, move || {
            app.run_with(backend)
        });
        shared.close();
        result?.map_err(|error| PyOSError::new_err(error.to_string()))
    }

    /// Drive the app from a loop of your own, in a terminal `width` x
    /// `height`.
    fn driver(&self, width: u16, height: u16) -> PyResult<PyDriver> {
        let (app, shared) = self.take()?;
        let driver = {
            let _enter = Enter::new(&shared);
            app.driver(width, height)
        };
        Ok(PyDriver {
            driver: RefCell::new(Some(driver)),
            shared,
            size: (width.into(), height.into()),
        })
    }

    /// Run headless at `width` x `height`, pressing `keys` (key names, one
    /// per step), and return the last screen's rows.
    fn render_with(
        &self,
        py: Python<'_>,
        keys: Vec<String>,
        width: u16,
        height: u16,
    ) -> PyResult<Vec<String>> {
        let mut script = CoreScript::new();
        for names in &keys {
            super::keys(names)?;
            script = script.keys(names);
        }
        let ran = headless(py, self, script, width, height, false)?;
        Ok(ran.last_frame().lines().map(str::to_string).collect())
    }

    fn __repr__(&self) -> String {
        match &*self.app.borrow() {
            Some(_) => "<App>".to_string(),
            None => "<App (ran)>".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Headless runs

/// What a headless run (`run`) painted: every frame's plain text, the bytes
/// written, what was copied, and whether the app quit before the script
/// ran out (`finished`).
#[pyclass(name = "Run", module = "rs_rich.tui", frozen)]
pub(crate) struct PyRun {
    #[pyo3(get)]
    frames: Vec<String>,
    #[pyo3(get)]
    writes: Vec<String>,
    #[pyo3(get)]
    copies: Vec<String>,
    #[pyo3(get)]
    finished: bool,
}

impl PyRun {
    fn last_frame(&self) -> &str {
        self.frames.last().map_or("", String::as_str)
    }
}

#[pymethods]
impl PyRun {
    /// The last frame painted, as text.
    #[getter(last_frame)]
    fn last_frame_py(&self) -> &str {
        self.last_frame()
    }

    /// The last frame's rows.
    #[getter]
    fn screen(&self) -> Vec<String> {
        self.last_frame().lines().map(str::to_string).collect()
    }

    /// Everything written, as one string.
    #[getter]
    fn output(&self) -> String {
        self.writes.concat()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Run {} frames={}>",
            if self.finished {
                "finished"
            } else {
                "unfinished"
            },
            self.frames.len()
        )
    }
}

fn headless(
    py: Python<'_>,
    app: &PyApp,
    script: CoreScript,
    width: u16,
    height: u16,
    exact_keys: bool,
) -> PyResult<PyRun> {
    let (app, shared) = app.take()?;
    let size = (usize::from(width.max(1)), usize::from(height.max(1)));
    let result = drive(py, &shared, size, false, move || {
        let mut backend = Headless::new(script, width.max(1), height.max(1));
        backend.exact_keys = exact_keys;
        let record = backend.record();
        let ran = app.run_on(&mut backend);
        let record = record.borrow().clone();
        (ran, record)
    });
    shared.close();
    let (ran, record) = result?;
    let finished = match ran {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => false,
        Err(error) => return Err(PyOSError::new_err(error.to_string())),
    };
    Ok(PyRun {
        frames: record.frames,
        writes: record.writes,
        copies: record.copies,
        finished,
    })
}

/// `run(app, script=None, *, width=80, height=24, exact_keys=False)`: run
/// `app` headless with scripted input (a `str` of key names or a `Script`)
/// on a virtual clock, and return the `Run`. With `exact_keys`, keys
/// arrive as a terminal with the kitty keyboard protocol sends them.
#[pyfunction]
#[pyo3(signature = (app, script=None, *, width=80, height=24, exact_keys=false))]
fn tui_run(
    py: Python<'_>,
    app: &Bound<'_, PyApp>,
    script: Option<&Bound<'_, PyAny>>,
    width: u16,
    height: u16,
    exact_keys: bool,
) -> PyResult<PyRun> {
    let script = crate::interact::script_arg(script)?;
    headless(py, &app.borrow(), script, width, height, exact_keys)
}

// ---------------------------------------------------------------------------
// The driver

/// `Driver`: the app driven from a loop of your own (`App.driver(width,
/// height)`). Each turn: `update(now)` (seconds since the start), `render()`
/// for the bytes that show what changed, `timeout(now)` for how long to wait
/// for input, and `event`, `key`, `paste`, `mouse` or `resize` for what
/// arrived. `finish()` returns the bytes that leave the terminal as it was.
#[pyclass(name = "Driver", module = "rs_rich.tui", unsendable)]
pub(crate) struct PyDriver {
    driver: RefCell<Option<intuituive::Driver>>,
    shared: Handle,
    size: (usize, usize),
}

fn finished() -> PyErr {
    PyRuntimeError::new_err("this driver has finished")
}

fn time(now: f64) -> PyResult<Duration> {
    Duration::try_from_secs_f64(now)
        .map_err(|_| PyValueError::new_err("the time is seconds since the start, at least 0"))
}

impl PyDriver {
    /// Run `f` on the driver: in a render scope, with the GIL released, then
    /// raise what a callback raised.
    fn step<R>(&self, py: Python<'_>, f: impl FnOnce(&mut intuituive::Driver) -> R) -> PyResult<R> {
        let mut guard = self.driver.try_borrow_mut().map_err(|_| {
            PyRuntimeError::new_err("the driver is in use by the call that called this")
        })?;
        let driver = guard.as_mut().ok_or_else(finished)?;
        drive(py, &self.shared, self.size, false, move || f(driver))
    }

    fn read<R>(&self, f: impl FnOnce(&intuituive::Driver) -> R) -> PyResult<R> {
        let guard = self.driver.try_borrow().map_err(|_| {
            PyRuntimeError::new_err("the driver is in use by the call that called this")
        })?;
        guard.as_ref().map(f).ok_or_else(finished)
    }

    fn send(&self, py: Python<'_>, events: Vec<Event>) -> PyResult<()> {
        self.step(py, move |driver| {
            for event in events {
                if driver.is_done() {
                    break;
                }
                driver.event(event);
            }
        })
    }
}

impl Drop for PyDriver {
    fn drop(&mut self) {
        self.shared.close();
    }
}

#[pymethods]
impl PyDriver {
    /// Bring the app up to `now` seconds: timers, animations, results from
    /// other threads, and the watches they set off.
    fn update(&self, py: Python<'_>, now: f64) -> PyResult<()> {
        let now = time(now)?;
        self.step(py, move |driver| driver.update(now))
    }

    /// Whether anything changed since the last frame.
    fn needs_render(&self) -> PyResult<bool> {
        self.read(intuituive::Driver::needs_render)
    }

    /// Draw what changed: the bytes that show it, or `None`. In linear
    /// mode, the lines to write.
    fn render(&self, py: Python<'_>) -> PyResult<Option<String>> {
        self.step(py, intuituive::Driver::render)
    }

    /// How long (seconds) to wait for an event: until the next timer, a
    /// frame while something animates, at most 0.05.
    fn timeout(&self, now: f64) -> PyResult<f64> {
        let now = time(now)?;
        self.read(|driver| driver.timeout(now).as_secs_f64())
    }

    /// A key, by name (`"enter"`, `"ctrl+s"`, `"a"`).
    fn event(&self, py: Python<'_>, key: &str) -> PyResult<()> {
        self.key(py, key)
    }

    /// Keys, by name, separated by spaces, one event each.
    fn key(&self, py: Python<'_>, names: &str) -> PyResult<()> {
        let events = keys(names)?.into_iter().map(Event::Key).collect();
        self.send(py, events)
    }

    /// A key let go (as a terminal with the kitty protocol reports it).
    fn key_up(&self, py: Python<'_>, name: &str) -> PyResult<()> {
        let key = Key::parse(name)
            .ok_or_else(|| PyValueError::new_err(format!("unknown key {name:?}")))?;
        self.send(py, vec![Event::KeyUp(key)])
    }

    /// Each character of `text` as a key.
    fn text(&self, py: Python<'_>, text: &str) -> PyResult<()> {
        let events = text.chars().map(|c| Event::Key(Key::char(c))).collect();
        self.send(py, events)
    }

    /// Text pasted at once.
    fn paste(&self, py: Python<'_>, text: &str) -> PyResult<()> {
        self.send(py, vec![Event::Paste(text.to_string())])
    }

    /// A mouse event: `kind` is `"down"`, `"up"`, `"drag"`, `"moved"`,
    /// `"scroll_up"` or `"scroll_down"`, at `column`, `row` of the screen.
    #[pyo3(signature = (kind, column, row, button="left"))]
    fn mouse(
        &self,
        py: Python<'_>,
        kind: &str,
        column: u16,
        row: u16,
        button: &str,
    ) -> PyResult<()> {
        let mouse = mouse_from(kind, column, row, button)?;
        self.send(py, vec![Event::Mouse(mouse)])
    }

    /// A left click at `column`, `row`: the button down, then up.
    fn click(&self, py: Python<'_>, column: u16, row: u16) -> PyResult<()> {
        let down = mouse_from("down", column, row, "left")?;
        let up = mouse_from("up", column, row, "left")?;
        self.send(py, vec![Event::Mouse(down), Event::Mouse(up)])
    }

    /// The terminal is now `columns` x `rows`.
    fn resize(&self, py: Python<'_>, columns: u16, rows: u16) -> PyResult<()> {
        self.step(py, move |driver| driver.resize(columns, rows))
    }

    /// Whether the app quit.
    fn is_done(&self) -> PyResult<bool> {
        self.read(intuituive::Driver::is_done)
    }

    /// The frame as drawn, as plain rows.
    fn screen(&self) -> PyResult<Vec<String>> {
        self.read(|driver| driver.screen().plain())
    }

    /// The frame as drawn, as rows of `Segment`s.
    fn lines<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let lines = self.read(|driver| driver.screen().lines())?;
        let rows = lines
            .iter()
            .map(|line| {
                let segments = line
                    .iter()
                    .map(|segment| Py::new(py, crate::segment::Segment::from_core(py, segment)))
                    .collect::<PyResult<Vec<_>>>()?;
                PyList::new(py, segments)
            })
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, rows)
    }

    /// Counts from the last frame.
    fn stats(&self) -> PyResult<PyFrameStats> {
        self.read(|driver| driver.stats().into())
    }

    /// Whether text can reach the clipboard (selections are then copied).
    fn set_clipboard(&self, py: Python<'_>, on: bool) -> PyResult<()> {
        self.step(py, move |driver| driver.set_clipboard(on))
    }

    /// Text put on the clipboard since the last call.
    fn take_copies(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        self.step(py, intuituive::Driver::take_copies)
    }

    /// `text` is on the clipboard: a toast says so.
    fn copied(&self, py: Python<'_>, text: &str) -> PyResult<()> {
        self.step(py, move |driver| driver.copied(text))
    }

    /// The accessibility tree, in reading order.
    fn accessibility(&self) -> PyResult<Vec<PyAccessNode>> {
        self.read(|driver| {
            driver
                .accessibility()
                .into_iter()
                .map(PyAccessNode)
                .collect()
        })
    }

    /// The announcements since the last call.
    fn take_announcements(&self, py: Python<'_>) -> PyResult<Vec<PyAnnouncement>> {
        let taken = self.step(py, intuituive::Driver::take_announcements)?;
        Ok(taken.into_iter().map(PyAnnouncement::from).collect())
    }

    fn is_accessible(&self) -> PyResult<bool> {
        self.read(intuituive::Driver::is_accessible)
    }

    fn is_linear(&self) -> PyResult<bool> {
        self.read(intuituive::Driver::is_linear)
    }

    /// The bytes to write before giving the terminal away for a while.
    fn suspend(&self, py: Python<'_>) -> PyResult<String> {
        self.step(py, intuituive::Driver::suspend)
    }

    /// Inline: the terminal row the app's region starts on.
    fn set_origin(&self, py: Python<'_>, row: u16) -> PyResult<()> {
        self.step(py, move |driver| driver.set_origin(row))
    }

    /// Stop: the bytes that leave the terminal as it was.
    fn finish(&self, py: Python<'_>) -> PyResult<String> {
        let driver = self.driver.borrow_mut().take().ok_or_else(finished)?;
        let result = drive(py, &self.shared, self.size, false, move || driver.finish());
        self.shared.close();
        result
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("TuiCtx", py.get_type::<PyCtx>()),
        ("TuiTheme", py.get_type::<PyTheme>()),
        ("TuiStylesheet", py.get_type::<PyStylesheet>()),
        ("TuiAccessState", py.get_type::<PyAccessState>()),
        ("TuiAccessNode", py.get_type::<PyAccessNode>()),
        ("TuiAnnouncement", py.get_type::<PyAnnouncement>()),
        ("TuiFrameStats", py.get_type::<PyFrameStats>()),
        ("TuiApp", py.get_type::<PyApp>()),
        ("TuiRun", py.get_type::<PyRun>()),
        ("TuiDriver", py.get_type::<PyDriver>()),
    ] {
        m.add(name, class)?;
    }
    // Set on each raised one; a bare `SheetError(...)` has none.
    let error = py.get_type::<TuiSheetError>();
    error.setattr("line", py.None())?;
    error.setattr("column", py.None())?;
    error.setattr("message", py.None())?;
    m.add("TuiSheetError", error)?;
    m.add_function(wrap_pyfunction!(tui_run, m)?)?;
    Ok(())
}
