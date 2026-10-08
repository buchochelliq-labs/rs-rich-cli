//! `rs_rich.tui`: intuiTUIve, the TUI framework (`rs-rich-intuituive`),
//! with browser serving (`rs-rich-web`) and the terminal pane and web view
//! (`rs-rich-embed`) (0.0.18 workstream 8).
//!
//! Owner: the tui area. It mirrors the Rust API name for name, so one guide
//! serves both: `text(lambda: f"{count.get()}").on_key("+", lambda cx:
//! count.update(lambda c: c + 1))`. A node, a signal and an app are
//! handles on the Rust objects, not configurations built per run as the
//! interact area's components are: an app keeps its tree for as long as it
//! runs.
//!
//! # Threads
//!
//! intuiTUIve's runtime is per thread (signals live on the thread that
//! made them), so every class that holds a node, a signal or an app is
//! `unsendable`: PyO3 raises if another thread touches it. The app runs on
//! the Python thread that called `run()` (or a `Driver` method), with the
//! GIL released while it waits for the terminal; every Python callback
//! takes the GIL back for as long as it runs ([`Callback`]). That is what
//! lets `spawn`'s work (a thread of its own) and other Python threads run
//! while the app waits. A served app (`serve`) is built and run on the
//! session's own thread, as rs-rich-web does in Rust.
//!
//! # Exceptions
//!
//! A Python exception raised by any callback (a handler, a node's closure,
//! a memo, a watch, a widget's method, a task's work) stops the app: no
//! more Python code of that app runs, the app quits at its next turn, and
//! `run()` (or the `Driver` method that was running) raises the exception.
//! In a served app, which no Python call is waiting on, the exception is
//! reported as unraisable (`sys.unraisablehook`) and that session ends.
//! A `resource`'s fetch is the exception to the rule: its exception is the
//! resource's `Load` failing, as a Rust fetch's `Err` is.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyString, PyTuple};

use rich_intuituive as intuituive;
use rich_intuituive::interact::Key;
use rich_intuituive::{Proxy, Size};

mod app;
mod embed;
mod node;
mod reactive;
mod web;
mod widget;
mod widgets;

pub(crate) use node::PyNode;

// ---------------------------------------------------------------------------
// The app a callback belongs to

/// What every callback of one app shares: the first exception raised, and
/// the way to stop the app. `Send + Sync`, so tasks and proxies carry it.
pub(crate) struct Shared {
    error: Mutex<Option<PyErr>>,
    failed: AtomicBool,
    /// The app's proxy, set once the app is built.
    proxy: OnceLock<Proxy>,
    /// A served session: exceptions go to `sys.unraisablehook`.
    unraisable: AtomicBool,
    /// The app finished: its signals are gone.
    closed: AtomicBool,
}

pub(crate) type Handle = Arc<Shared>;

impl Shared {
    pub(crate) fn new() -> Handle {
        Arc::new(Shared {
            error: Mutex::new(None),
            failed: AtomicBool::new(false),
            proxy: OnceLock::new(),
            unraisable: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        })
    }

    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }

    pub(crate) fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn set_proxy(&self, proxy: Proxy) {
        let _ = self.proxy.set(proxy);
    }

    pub(crate) fn set_unraisable(&self) {
        self.unraisable.store(true, Ordering::SeqCst);
    }

    /// Keep `error` (the first one wins) and stop the app at its next turn.
    pub(crate) fn fail(&self, py: Python<'_>, error: PyErr) {
        if self.unraisable.load(Ordering::SeqCst) {
            error.write_unraisable(py, None);
        } else {
            let mut slot = self.error.lock().unwrap_or_else(|p| p.into_inner());
            if slot.is_none() {
                *slot = Some(error);
            }
        }
        if !self.failed.swap(true, Ordering::SeqCst) {
            if let Some(proxy) = self.proxy.get() {
                proxy.run_with(|cx| cx.quit());
            }
        }
    }

    /// Stop the app at its next turn, for an exception kept elsewhere (the
    /// render scope's, which `run()` raises).
    pub(crate) fn stop(&self) {
        if !self.failed.swap(true, Ordering::SeqCst) {
            if let Some(proxy) = self.proxy.get() {
                proxy.run_with(|cx| cx.quit());
            }
        }
    }

    /// The exception a callback raised, taken.
    pub(crate) fn take_error(&self) -> Option<PyErr> {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    /// Raise what a callback raised, if anything did.
    pub(crate) fn check(&self) -> PyResult<()> {
        match self.take_error() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

thread_local! {
    /// The apps whose code is running on this thread, innermost last.
    static CURRENT: RefCell<Vec<Handle>> = const { RefCell::new(Vec::new()) };
    /// How deep in building an app or a screen this thread is: where
    /// `every` may be called.
    static BUILDING: Cell<usize> = const { Cell::new(0) };
}

/// The app whose code is running now, if any.
pub(crate) fn current() -> Option<Handle> {
    CURRENT.with(|current| current.borrow().last().cloned())
}

/// The app whose code is running now, or the `RuntimeError` that `what`
/// was called outside one.
pub(crate) fn require(what: &str) -> PyResult<Handle> {
    match current() {
        Some(app) if !app.closed() => Ok(app),
        Some(_) => Err(PyRuntimeError::new_err(format!(
            "{what} is called while its app runs; this app has finished"
        ))),
        None => Err(PyRuntimeError::new_err(format!(
            "{what} is called while an app is built or running: inside App's build \
             function, a node's function or an event handler"
        ))),
    }
}

/// `app`'s code is running on this thread until this is dropped.
pub(crate) struct Enter(());

impl Enter {
    pub(crate) fn new(app: &Handle) -> Enter {
        CURRENT.with(|current| current.borrow_mut().push(app.clone()));
        Enter(())
    }
}

impl Drop for Enter {
    fn drop(&mut self) {
        CURRENT.with(|current| current.borrow_mut().pop());
    }
}

/// The thread a handle's app runs on. Signals, memos, logs and resources
/// are plain handles (`Copy` in Rust), checked against it by hand: a use
/// from another thread raises a `RuntimeError`, and dropping one anywhere
/// is harmless (a served app's handles may be dropped on any thread).
pub(crate) struct Home(std::thread::ThreadId);

impl Home {
    pub(crate) fn here() -> Home {
        Home(std::thread::current().id())
    }

    pub(crate) fn check(&self, what: &str) -> PyResult<()> {
        if std::thread::current().id() == self.0 {
            return Ok(());
        }
        Err(PyRuntimeError::new_err(format!(
            "this {what} belongs to the thread its app runs on; from another thread, \
             change the app's state through a Proxy"
        )))
    }
}

/// An app or a screen is being built until this is dropped.
pub(crate) struct Building(());

impl Building {
    pub(crate) fn new() -> Building {
        BUILDING.with(|depth| depth.set(depth.get() + 1));
        Building(())
    }

    pub(crate) fn active() -> bool {
        BUILDING.with(|depth| depth.get() > 0)
    }
}

impl Drop for Building {
    fn drop(&mut self) {
        BUILDING.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// A Python callable that Rust calls back: on the app's thread, with the
/// GIL taken for the call, inside the app it was made for. An exception
/// stops the app ([`Shared::fail`]); after one, no more callbacks of that
/// app run, and each returns `None`.
pub(crate) struct Callback {
    func: Py<PyAny>,
    app: Option<Handle>,
}

impl Callback {
    /// A callback of the app being built or run now.
    pub(crate) fn new(func: Py<PyAny>) -> Callback {
        Callback {
            func,
            app: current(),
        }
    }

    /// A callback of `app`.
    pub(crate) fn of(func: Py<PyAny>, app: Option<Handle>) -> Callback {
        Callback { func, app }
    }

    /// Check `func` is callable, naming `what` when it is not.
    pub(crate) fn checked(func: &Bound<'_, PyAny>, what: &str) -> PyResult<Callback> {
        if !func.is_callable() {
            return Err(PyTypeError::new_err(format!(
                "{what} must be callable, got {}",
                func.get_type().name()?
            )));
        }
        Ok(Callback::new(func.clone().unbind()))
    }

    pub(crate) fn app(&self) -> Option<Handle> {
        self.app.clone()
    }

    pub(crate) fn func(&self) -> &Py<PyAny> {
        &self.func
    }

    /// The app to report to: the one it was made for, else the one running.
    fn owner(&self) -> Option<Handle> {
        self.app.clone().or_else(current)
    }

    /// Call with the arguments `args` builds and convert the result.
    pub(crate) fn call<R>(
        &self,
        args: impl for<'py> FnOnce(Python<'py>) -> PyResult<Bound<'py, PyTuple>>,
        convert: impl for<'py> FnOnce(&Bound<'py, PyAny>) -> PyResult<R>,
    ) -> Option<R> {
        invoke(self.owner().as_ref(), |py| {
            let value = self.func.bind(py).call1(args(py)?)?;
            convert(&value)
        })
    }

    /// Call with no arguments, for the returned object.
    pub(crate) fn call0(&self) -> Option<Py<PyAny>> {
        self.call(
            |py| Ok(PyTuple::empty(py)),
            |value| Ok(value.clone().unbind()),
        )
    }
}

/// Run Python code `f` for `app`, with the GIL: not at all once the app
/// has failed; an exception stops the app ([`Shared::fail`]).
pub(crate) fn invoke<R>(
    app: Option<&Handle>,
    f: impl FnOnce(Python<'_>) -> PyResult<R>,
) -> Option<R> {
    Python::attach(|py| {
        if app.is_some_and(|app| app.failed()) {
            return None;
        }
        let _enter = app.map(Enter::new);
        match f(py) {
            Ok(value) => Some(value),
            Err(error) => {
                fail(py, app, error);
                None
            }
        }
    })
}

/// Report `error`: to `app`, else (no app to stop) as unraisable.
pub(crate) fn fail(py: Python<'_>, app: Option<&Handle>, error: PyErr) {
    match app {
        Some(app) => app.fail(py, error),
        None => error.write_unraisable(py, None),
    }
}

/// Run `f` with the GIL released, though what it moves is not `Send`.
///
/// Sound because `detach` runs `f` on this thread, before it returns: the
/// values never reach another thread. Python objects are touched only
/// inside [`Python::attach`], as the callbacks do.
pub(crate) fn detached<T>(py: Python<'_>, f: impl FnOnce() -> T) -> T {
    struct AssertSend<F>(F);
    // SAFETY: see above; the closure runs on the calling thread.
    unsafe impl<F> Send for AssertSend<F> {}
    impl<F> AssertSend<F> {
        fn into_inner(self) -> F {
            self.0
        }
    }
    let f = AssertSend(f);
    let result = py.detach(move || AssertSend((f.into_inner())()));
    result.into_inner()
}

// ---------------------------------------------------------------------------
// Handles valid only during a call

/// A Rust value lent to Python for one call (a `Ctx`, a `Canvas`): Python
/// may keep the object, but using it after the call raises.
pub(crate) struct Lent<T: ?Sized> {
    ptr: NonNull<T>,
    live: Rc<Cell<bool>>,
    busy: Cell<bool>,
}

impl<T: ?Sized> Lent<T> {
    /// # Safety
    /// `value` must outlive every use, which [`Loan`] ensures by marking
    /// the handle dead before the borrow ends.
    pub(crate) unsafe fn new(value: NonNull<T>, live: Rc<Cell<bool>>) -> Lent<T> {
        Lent {
            ptr: value,
            live,
            busy: Cell::new(false),
        }
    }

    /// Use the value, unless the call it was lent for is over (or this is
    /// a call back from inside another use).
    pub(crate) fn with<R>(&self, what: &str, f: impl FnOnce(&mut T) -> R) -> PyResult<R> {
        if !self.live.get() {
            return Err(PyRuntimeError::new_err(format!(
                "this {what} was lent for one call, which is over"
            )));
        }
        if self.busy.replace(true) {
            return Err(PyRuntimeError::new_err(format!(
                "this {what} is in use by the call that called this"
            )));
        }
        struct Free<'a>(&'a Cell<bool>);
        impl Drop for Free<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _free = Free(&self.busy);
        // SAFETY: live, so the borrow it came from is still in force, and
        // not busy, so no other use of it is running.
        Ok(f(unsafe { &mut *self.ptr.as_ptr() }))
    }

    pub(crate) fn token(&self) -> Rc<Cell<bool>> {
        self.live.clone()
    }
}

/// A Rust value lent to Python for one call by shared reference (a
/// widget's `MeasureCx`).
pub(crate) struct LentRef<T: ?Sized> {
    ptr: NonNull<T>,
    live: Rc<Cell<bool>>,
}

impl<T: ?Sized> LentRef<T> {
    /// # Safety
    /// As for [`Lent::new`]: `value` outlives every use.
    pub(crate) unsafe fn new(value: NonNull<T>, live: Rc<Cell<bool>>) -> LentRef<T> {
        LentRef { ptr: value, live }
    }

    pub(crate) fn with<R>(&self, what: &str, f: impl FnOnce(&T) -> R) -> PyResult<R> {
        if !self.live.get() {
            return Err(PyRuntimeError::new_err(format!(
                "this {what} was lent for one call, which is over"
            )));
        }
        // SAFETY: live, so the shared borrow it came from is in force.
        Ok(f(unsafe { self.ptr.as_ref() }))
    }
}

/// The lifetime of a loan: dropping it ends every handle lent under it.
pub(crate) struct Loan {
    live: Rc<Cell<bool>>,
}

impl Loan {
    pub(crate) fn new() -> Loan {
        Loan {
            live: Rc::new(Cell::new(true)),
        }
    }

    pub(crate) fn token(&self) -> Rc<Cell<bool>> {
        self.live.clone()
    }
}

impl Drop for Loan {
    fn drop(&mut self) {
        self.live.set(false);
    }
}

// ---------------------------------------------------------------------------
// Arguments

/// Key names separated by spaces, checked: a `ValueError` for a name that
/// is not a key, or for none at all (Rust panics on both).
pub(crate) fn keys(names: &str) -> PyResult<Vec<Key>> {
    let keys = rich_intuituive::interact::keymap::try_keys(names)
        .map_err(|name| PyValueError::new_err(format!("unknown key {name:?}")))?;
    if keys.is_empty() {
        return Err(PyValueError::new_err(format!(
            "no keys in {names:?}: name them separated by spaces; the space key is \"space\""
        )));
    }
    Ok(keys)
}

/// `Size`: how a node is sized along its parent's axis. `Size.Fixed(3)`,
/// `Size.Percent(40)`, `Size.Flex(1)` and `Size.Auto`, as in Rust;
/// anywhere a size is taken, an `int` is `Fixed` and a `str` is a
/// stylesheet's size (`"3"`, `"40%"`, `"2fr"`, `"auto"`).
#[pyclass(name = "Size", module = "rs_rich.tui", frozen, skip_from_py_object)]
#[derive(Clone, Copy)]
pub(crate) struct PySize(pub(crate) Size);

#[pymethods]
#[allow(non_snake_case)]
impl PySize {
    #[staticmethod]
    fn Fixed(cells: u16) -> PySize {
        PySize(Size::Fixed(cells))
    }

    #[staticmethod]
    fn Percent(percent: u16) -> PySize {
        PySize(Size::Percent(percent))
    }

    #[staticmethod]
    fn Flex(weight: u16) -> PySize {
        PySize(Size::Flex(weight))
    }

    #[classattr]
    fn Auto() -> PySize {
        PySize(Size::Auto)
    }

    /// `Size.parse("40%")`: a size written as a stylesheet writes it.
    #[staticmethod]
    fn parse(text: &str) -> PyResult<PySize> {
        parse_size(text).map(PySize)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .cast::<PySize>()
            .is_ok_and(|other| other.get().0 == self.0)
    }

    fn __hash__(&self) -> u64 {
        match self.0 {
            Size::Fixed(n) => u64::from(n),
            Size::Percent(n) => 1 << 20 | u64::from(n),
            Size::Flex(n) => 2 << 20 | u64::from(n),
            Size::Auto => 3 << 20,
        }
    }

    fn __repr__(&self) -> String {
        size_repr(self.0)
    }
}

pub(crate) fn size_repr(size: Size) -> String {
    match size {
        Size::Fixed(n) => format!("Size.Fixed({n})"),
        Size::Percent(n) => format!("Size.Percent({n})"),
        Size::Flex(n) => format!("Size.Flex({n})"),
        Size::Auto => "Size.Auto".to_string(),
    }
}

fn parse_size(text: &str) -> PyResult<Size> {
    let text = text.trim();
    let number = |digits: &str| {
        digits.trim().parse::<u16>().map_err(|_| {
            PyValueError::new_err(format!(
                "invalid size {text:?}: expected cells (\"3\"), a percentage (\"40%\"), a share \
                 (\"2fr\") or \"auto\""
            ))
        })
    };
    if text.eq_ignore_ascii_case("auto") {
        Ok(Size::Auto)
    } else if let Some(n) = text.strip_suffix('%') {
        Ok(Size::Percent(number(n)?))
    } else if let Some(n) = text.strip_suffix("fr") {
        Ok(Size::Flex(number(n)?))
    } else {
        Ok(Size::Fixed(number(text)?))
    }
}

/// A size argument: a `Size`, an `int` (cells) or a `str`.
pub(crate) fn size_arg(value: &Bound<'_, PyAny>) -> PyResult<Size> {
    if let Ok(size) = value.cast::<PySize>() {
        return Ok(size.get().0);
    }
    if let Ok(text) = value.cast::<PyString>() {
        return parse_size(&text.to_cow()?);
    }
    if let Ok(cells) = value.extract::<u16>() {
        return Ok(Size::Fixed(cells));
    }
    Err(PyTypeError::new_err(format!(
        "a size is a Size, an int (cells) or a str (\"40%\", \"2fr\", \"auto\"), got {}",
        value.get_type().name()?
    )))
}

/// Sizes from an iterable.
pub(crate) fn sizes(value: &Bound<'_, PyAny>) -> PyResult<Vec<Size>> {
    if value.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err("sizes must be an iterable, not a str"));
    }
    value.try_iter()?.map(|item| size_arg(&item?)).collect()
}

/// Defines a Python enum mirroring a Rust one: the variants as in Rust
/// (`Easing.EaseOut`), and, wherever one is taken, its snake-case name as
/// a `str` too (`"ease_out"`).
macro_rules! py_enum {
    (
        $(#[$doc:meta])*
        $py:ident = $name:literal for $rust:ty {
            $($variant:ident = $text:literal => $value:expr),+ $(,)?
        }
        $arg:ident $({ $($extra:tt)* })?
    ) => {
        $(#[$doc])*
        #[pyclass(name = $name, module = "rs_rich.tui", eq, eq_int, from_py_object)]
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub(crate) enum $py {
            $($variant),+
        }

        impl $py {
            pub(crate) fn rust(self) -> $rust {
                match self {
                    $($py::$variant => $value),+
                }
            }

            #[allow(dead_code, unreachable_patterns)]
            pub(crate) fn from_rust(value: $rust) -> Option<$py> {
                $(if value == $value {
                    return Some($py::$variant);
                })+
                None
            }

            pub(crate) fn text(self) -> &'static str {
                match self {
                    $($py::$variant => $text),+
                }
            }
        }

        #[pymethods]
        impl $py {
            /// The snake-case name: what a `str` argument may say instead.
            #[getter]
            fn value(&self) -> &'static str {
                self.text()
            }

            fn __str__(&self) -> &'static str {
                self.text()
            }

            $($($extra)*)?
        }

        /// The enum from itself or its name.
        pub(crate) fn $arg(value: &Bound<'_, PyAny>) -> PyResult<$rust> {
            if let Ok(value) = value.extract::<$py>() {
                return Ok(value.rust());
            }
            let text: String = value.extract().map_err(|_| {
                PyTypeError::new_err(concat!("expected a ", $name, " or its name"))
            })?;
            let wanted = text.trim().to_ascii_lowercase().replace(['-', ' '], "_");
            $(if wanted == $text {
                return Ok($value);
            })+
            Err(PyValueError::new_err(format!(
                concat!("invalid ", $name, " {:?}; expected one of: {}"),
                text,
                [$($text),+].join(", ")
            )))
        }
    };
}

py_enum! {
    /// How an animation moves between its two values.
    Easing = "Easing" for intuituive::Easing {
        Linear = "linear" => intuituive::Easing::Linear,
        EaseIn = "ease_in" => intuituive::Easing::EaseIn,
        EaseOut = "ease_out" => intuituive::Easing::EaseOut,
        EaseInOut = "ease_in_out" => intuituive::Easing::EaseInOut,
    }
    easing_arg
}

py_enum! {
    /// Where a pop-up goes, next to its anchor.
    Placement = "Placement" for intuituive::Placement {
        Below = "below" => intuituive::Placement::Below,
        Above = "above" => intuituive::Placement::Above,
        Right = "right" => intuituive::Placement::Right,
        Left = "left" => intuituive::Placement::Left,
    }
    placement_arg
}

py_enum! {
    /// Which way children are laid out: `Horizontal` (side by side) or
    /// `Vertical`.
    Axis = "Axis" for intuituive::node::Axis {
        Horizontal = "horizontal" => intuituive::node::Axis::Horizontal,
        Vertical = "vertical" => intuituive::node::Axis::Vertical,
    }
    axis_arg
}

py_enum! {
    /// Which way a table's column is sorted.
    Order = "Order" for intuituive::widgets::Order {
        Ascending = "ascending" => intuituive::widgets::Order::Ascending,
        Descending = "descending" => intuituive::widgets::Order::Descending,
    }
    order_arg
}

py_enum! {
    /// What a node is, for assistive technology: ARIA's roles.
    Role = "Role" for intuituive::a11y::Role {
        Group = "group" => intuituive::a11y::Role::Group,
        Region = "region" => intuituive::a11y::Role::Region,
        Text = "text" => intuituive::a11y::Role::Text,
        Button = "button" => intuituive::a11y::Role::Button,
        List = "list" => intuituive::a11y::Role::List,
        ListItem = "listitem" => intuituive::a11y::Role::ListItem,
        Table = "table" => intuituive::a11y::Role::Table,
        Row = "row" => intuituive::a11y::Role::Row,
        Cell = "cell" => intuituive::a11y::Role::Cell,
        Tree = "tree" => intuituive::a11y::Role::Tree,
        TreeItem = "treeitem" => intuituive::a11y::Role::TreeItem,
        Tab = "tab" => intuituive::a11y::Role::Tab,
        TabList = "tablist" => intuituive::a11y::Role::TabList,
        Dialog = "dialog" => intuituive::a11y::Role::Dialog,
        Menu = "menu" => intuituive::a11y::Role::Menu,
        MenuBar = "menubar" => intuituive::a11y::Role::MenuBar,
        MenuItem = "menuitem" => intuituive::a11y::Role::MenuItem,
        TextBox = "textbox" => intuituive::a11y::Role::TextBox,
        Status = "status" => intuituive::a11y::Role::Status,
        Log = "log" => intuituive::a11y::Role::Log,
        Grid = "grid" => intuituive::a11y::Role::Grid,
        Separator = "separator" => intuituive::a11y::Role::Separator,
        CheckBox = "checkbox" => intuituive::a11y::Role::CheckBox,
        Switch = "switch" => intuituive::a11y::Role::Switch,
    }
    role_arg {
        /// What a screen reader says: `"button"`, `"list item"`, `"text box"`.
        fn spoken(&self) -> &'static str {
            self.rust().spoken()
        }

        /// Whether a widget of this role holds items, one selected.
        fn has_items(&self) -> bool {
            self.rust().has_items()
        }
    }
}

/// `Rect(x, y, width, height)`: a rectangle of cells; anywhere one is
/// taken, a 4-tuple is one too.
#[pyclass(name = "Rect", module = "rs_rich.tui", frozen, skip_from_py_object)]
#[derive(Clone, Copy)]
pub(crate) struct PyRect(pub(crate) intuituive::screen::Rect);

#[pymethods]
impl PyRect {
    #[new]
    fn new(x: u16, y: u16, width: u16, height: u16) -> PyRect {
        PyRect(intuituive::screen::Rect::new(x, y, width, height))
    }

    #[getter]
    fn x(&self) -> u16 {
        self.0.x
    }

    #[getter]
    fn y(&self) -> u16 {
        self.0.y
    }

    #[getter]
    fn width(&self) -> u16 {
        self.0.width
    }

    #[getter]
    fn height(&self) -> u16 {
        self.0.height
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let r = self.0;
        PyTuple::new(py, [r.x, r.y, r.width, r.height])?
            .as_any()
            .try_iter()
            .map(Bound::into_any)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        rect_arg(other).is_ok_and(|other| other == self.0)
    }

    fn __hash__(&self) -> u64 {
        let r = self.0;
        u64::from(r.x) << 48 | u64::from(r.y) << 32 | u64::from(r.width) << 16 | u64::from(r.height)
    }

    fn __repr__(&self) -> String {
        let r = self.0;
        format!("Rect({}, {}, {}, {})", r.x, r.y, r.width, r.height)
    }
}

/// A rectangle argument: a `Rect` or `(x, y, width, height)`.
pub(crate) fn rect_arg(value: &Bound<'_, PyAny>) -> PyResult<intuituive::screen::Rect> {
    if let Ok(rect) = value.cast::<PyRect>() {
        return Ok(rect.get().0);
    }
    let (x, y, width, height): (u16, u16, u16, u16) = value.extract().map_err(|_| {
        PyTypeError::new_err("a rectangle is a Rect or a tuple (x, y, width, height)")
    })?;
    Ok(intuituive::screen::Rect::new(x, y, width, height))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("TuiSize", py.get_type::<PySize>()),
        ("TuiRect", py.get_type::<PyRect>()),
        ("TuiEasing", py.get_type::<Easing>()),
        ("TuiPlacement", py.get_type::<Placement>()),
        ("TuiAxis", py.get_type::<Axis>()),
        ("TuiOrder", py.get_type::<Order>()),
        ("TuiRole", py.get_type::<Role>()),
    ] {
        m.add(name, class)?;
    }
    reactive::register(m)?;
    node::register(m)?;
    widgets::register(m)?;
    widget::register(m)?;
    app::register(m)?;
    web::register(m)?;
    embed::register(m)
}
