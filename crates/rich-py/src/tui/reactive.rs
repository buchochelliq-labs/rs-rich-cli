//! Reactive state from Python: `signal`, `memo`, `watch`, `every`, `Log`,
//! and background work (`spawn`, `spawn_async`, `resource`, `Proxy`).
//!
//! A Python signal holds any Python object. The widgets that need a typed
//! Rust signal (a list's selected row, a split's ratio, a calendar's day)
//! take a Python signal too: the first time one is given to such a widget
//! it becomes a signal of that type, keeping its value (an `int`, a
//! `float`, a `datetime.date`, ...), and anything that read it before is
//! told, so it reads the new one from then on.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyList, PySet, PyTuple};

use rich_embed::ExitStatus;
use rich_intuituive as intuituive;
use rich_intuituive::widgets::{Date, Order};
use rich_intuituive::{Load, Memo, Resource, Signal};

use super::app::PyCtx;
use std::sync::Mutex;

use super::{require, Building, Callback, Handle, Home, Loan};

// ---------------------------------------------------------------------------
// Python values in Rust's reactive types

/// A Python object as a signal's value: cloned as a new reference, equal
/// by Python's `==` (an exception there counts as not equal).
pub(crate) struct PyValue(pub(crate) Py<PyAny>);

impl PyValue {
    pub(crate) fn none() -> PyValue {
        Python::attach(|py| PyValue(py.None()))
    }

    pub(crate) fn object(&self, py: Python<'_>) -> Py<PyAny> {
        self.0.clone_ref(py)
    }
}

impl Clone for PyValue {
    fn clone(&self) -> PyValue {
        Python::attach(|py| PyValue(self.0.clone_ref(py)))
    }
}

impl PartialEq for PyValue {
    fn eq(&self, other: &PyValue) -> bool {
        Python::attach(|py| {
            let (a, b) = (self.0.bind(py), other.0.bind(py));
            a.is(b) || a.eq(b).unwrap_or(false)
        })
    }
}

impl Eq for PyValue {}

impl Hash for PyValue {
    /// Python's hash; an unhashable value (a list) hashes alike, so `==`
    /// decides.
    fn hash<H: Hasher>(&self, state: &mut H) {
        Python::attach(|py| self.0.bind(py).hash().unwrap_or(0)).hash(state);
    }
}

// ---------------------------------------------------------------------------
// Typed values

fn date_from(value: &Bound<'_, PyAny>) -> PyResult<Date> {
    let part = |name: &str| -> PyResult<i64> {
        value.getattr(name)?.extract().map_err(|_| {
            PyTypeError::new_err("a date is a datetime.date (with year, month and day)")
        })
    };
    let (year, month, day) = (part("year")?, part("month")?, part("day")?);
    let (Ok(year), Ok(month @ 1..=12), Ok(day)) = (
        i32::try_from(year),
        u32::try_from(month),
        u32::try_from(day),
    ) else {
        return Err(PyValueError::new_err("the date is out of range"));
    };
    if day == 0 || day > Date::days_in_month(year, month) {
        return Err(PyValueError::new_err(
            "the day is out of range for its month",
        ));
    }
    Ok(Date::new(year, month, day))
}

pub(crate) fn date_to(py: Python<'_>, date: Date) -> PyResult<Py<PyAny>> {
    // Beyond what `datetime.date` holds (years 1 to 9999), the calendar's
    // arithmetic can go; such a day comes back as a tuple.
    let made = py
        .import("datetime")?
        .getattr("date")?
        .call1((date.year, date.month, date.day));
    match made {
        Ok(date) => Ok(date.unbind()),
        Err(_) => Ok(PyTuple::new(
            py,
            [i64::from(date.year), date.month.into(), date.day.into()],
        )?
        .into_any()
        .unbind()),
    }
}

pub(crate) fn date_arg(value: &Bound<'_, PyAny>) -> PyResult<Date> {
    date_from(value)
}

fn path_from(value: &Bound<'_, PyAny>) -> PyResult<Vec<usize>> {
    value
        .extract::<Vec<usize>>()
        .map_err(|_| PyTypeError::new_err("a tree path is a list of indices (ints, at least 0)"))
}

fn paths_from(value: &Bound<'_, PyAny>) -> PyResult<HashSet<Vec<usize>>> {
    value
        .try_iter()
        .map_err(|_| PyTypeError::new_err("expanded paths are an iterable of tuples of indices"))?
        .map(|path| path_from(&path?))
        .collect()
}

fn paths_to(py: Python<'_>, paths: &HashSet<Vec<usize>>) -> PyResult<Py<PyAny>> {
    let mut sorted: Vec<&Vec<usize>> = paths.iter().collect();
    sorted.sort();
    let tuples = sorted
        .into_iter()
        .map(|path| PyTuple::new(py, path))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PySet::new(py, tuples)?.into_any().unbind())
}

fn sort_from(value: &Bound<'_, PyAny>) -> PyResult<Option<(usize, Order)>> {
    if value.is_none() {
        return Ok(None);
    }
    let (column, order): (usize, Bound<'_, PyAny>) = value
        .extract()
        .map_err(|_| PyTypeError::new_err("a sort is None or (column, Order)"))?;
    Ok(Some((column, super::order_arg(&order)?)))
}

fn sort_to(py: Python<'_>, sort: Option<(usize, Order)>) -> PyResult<Py<PyAny>> {
    Ok(match sort {
        None => py.None(),
        Some((column, order)) => {
            let order = super::Order::from_rust(order).expect("every order");
            (column, order).into_pyobject(py)?.into_any().unbind()
        }
    })
}

fn exit_to(py: Python<'_>, status: &Option<ExitStatus>) -> PyResult<Py<PyAny>> {
    Ok(match status {
        None => py.None(),
        Some(status) => Py::new(py, super::embed::PyExitStatus(status.clone()))?.into_any(),
    })
}

/// What a Python signal is now: a Python object, or a typed Rust signal a
/// widget needs.
#[derive(Clone, Copy)]
pub(crate) enum Slot {
    Any(Signal<PyValue>),
    Usize(Signal<usize>),
    U16(Signal<u16>),
    F64(Signal<f64>),
    Date(Signal<Date>),
    Path(Signal<Vec<usize>>),
    Paths(Signal<HashSet<Vec<usize>>>),
    Sort(Signal<Option<(usize, Order)>>),
    Key(Signal<Option<String>>),
    Str(Signal<String>),
    Bool(Signal<bool>),
    Exit(Signal<Option<ExitStatus>>),
}

impl Slot {
    fn kind(&self) -> &'static str {
        match self {
            Slot::Any(_) => "any value",
            Slot::Usize(_) => "an index (int)",
            Slot::U16(_) => "an offset (int)",
            Slot::F64(_) => "a float",
            Slot::Date(_) => "a date",
            Slot::Path(_) => "a tree path",
            Slot::Paths(_) => "a set of tree paths",
            Slot::Sort(_) => "a sort",
            Slot::Key(_) => "a key (str or None)",
            Slot::Str(_) => "a str",
            Slot::Bool(_) => "a bool",
            Slot::Exit(_) => "an exit status",
        }
    }
}

/// `Signal`: a reactive value. `get()` reads it (a node drawing, or a memo
/// computing, subscribes to it), `set(value)` writes it (nothing happens
/// when the value is equal), and `update(f)` sets it to `f(value)`, or,
/// when `f` returns `None`, keeps the value `f` changed in place (a list
/// appended to) and tells the readers. Made by `signal(value)`.
#[pyclass(name = "Signal", module = "rs_rich.tui", frozen)]
pub(crate) struct PySignal {
    slot: Mutex<Slot>,
    app: Option<Handle>,
    home: Home,
}

impl PySignal {
    fn current(&self) -> Slot {
        *self.slot.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn replace(&self, slot: Slot) {
        *self.slot.lock().unwrap_or_else(|p| p.into_inner()) = slot;
    }

    pub(crate) fn wrap(slot: Slot) -> PySignal {
        PySignal {
            slot: Mutex::new(slot),
            app: super::current(),
            home: Home::here(),
        }
    }

    fn alive(&self) -> PyResult<()> {
        self.home.check("signal")?;
        if self.app.as_ref().is_some_and(|app| app.closed()) {
            return Err(PyRuntimeError::new_err(
                "the app this signal belongs to has finished",
            ));
        }
        Ok(())
    }

    fn read(&self, py: Python<'_>, tracked: bool) -> PyResult<Py<PyAny>> {
        self.alive()?;
        macro_rules! read {
            ($signal:expr, |$v:ident| $convert:expr) => {{
                if tracked {
                    $signal.with(|$v| $convert)
                } else {
                    $signal.with_untracked(|$v| $convert)
                }
            }};
        }
        match self.current() {
            Slot::Any(s) => read!(s, |v| Ok(v.object(py))),
            Slot::Usize(s) => read!(s, |v| Ok(v.into_pyobject(py)?.into_any().unbind())),
            Slot::U16(s) => read!(s, |v| Ok(v.into_pyobject(py)?.into_any().unbind())),
            Slot::F64(s) => read!(s, |v| Ok(v.into_pyobject(py)?.into_any().unbind())),
            Slot::Date(s) => read!(s, |v| date_to(py, *v)),
            Slot::Path(s) => read!(s, |v| Ok(PyList::new(py, v)?.into_any().unbind())),
            Slot::Paths(s) => read!(s, |v| paths_to(py, v)),
            Slot::Sort(s) => read!(s, |v| sort_to(py, *v)),
            Slot::Key(s) => read!(s, |v| Ok(v.into_pyobject(py)?.into_any().unbind())),
            Slot::Str(s) => read!(s, |v| Ok(v.into_pyobject(py)?.into_any().unbind())),
            Slot::Bool(s) => read!(s, |v| Ok(v
                .into_pyobject(py)?
                .to_owned()
                .into_any()
                .unbind())),
            Slot::Exit(s) => read!(s, |v| exit_to(py, v)),
        }
    }

    fn write(&self, value: &Bound<'_, PyAny>, force: bool) -> PyResult<()> {
        self.alive()?;
        macro_rules! write {
            ($signal:expr, $value:expr) => {{
                let value = $value;
                if force {
                    $signal.update(|slot| *slot = value);
                } else {
                    $signal.set(value);
                }
            }};
        }
        let typed = |what: &str, error: PyErr| {
            PyTypeError::new_err(format!("this signal holds {what}: {error}"))
        };
        match self.current() {
            Slot::Any(s) => write!(s, PyValue(value.clone().unbind())),
            Slot::Usize(s) => write!(
                s,
                value.extract::<usize>().map_err(|e| typed("an index", e))?
            ),
            Slot::U16(s) => write!(
                s,
                value.extract::<u16>().map_err(|e| typed("an offset", e))?
            ),
            Slot::F64(s) => write!(s, value.extract::<f64>().map_err(|e| typed("a float", e))?),
            Slot::Date(s) => write!(s, date_from(value)?),
            Slot::Path(s) => write!(s, path_from(value)?),
            Slot::Paths(s) => write!(s, paths_from(value)?),
            Slot::Sort(s) => write!(s, sort_from(value)?),
            Slot::Key(s) => write!(
                s,
                value
                    .extract::<Option<String>>()
                    .map_err(|e| typed("a key", e))?
            ),
            Slot::Str(s) => write!(s, value.extract::<String>().map_err(|e| typed("a str", e))?),
            Slot::Bool(s) => write!(s, value.extract::<bool>().map_err(|e| typed("a bool", e))?),
            Slot::Exit(_) => {
                return Err(PyTypeError::new_err("a program's exit status is read only"))
            }
        }
        Ok(())
    }

    /// This signal as a typed one, converting it the first time.
    fn typed<T: 'static>(
        &self,
        py: Python<'_>,
        what: &str,
        from: fn(Slot) -> Option<Signal<T>>,
        make: fn(Signal<T>) -> Slot,
        convert: impl FnOnce(&Bound<'_, PyAny>) -> PyResult<T>,
    ) -> PyResult<Signal<T>> {
        self.alive()?;
        let slot = self.current();
        if let Some(signal) = from(slot) {
            return Ok(signal);
        }
        let Slot::Any(old) = slot else {
            return Err(PyTypeError::new_err(format!(
                "{what} needs its own signal; this one already holds {}",
                slot.kind()
            )));
        };
        require("a widget")?;
        let value = old.with_untracked(|v| v.object(py));
        let value = convert(value.bind(py))
            .map_err(|error| PyTypeError::new_err(format!("{what}: {error}")))?;
        let signal = intuituive::signal(value);
        self.replace(make(signal));
        // Whatever read the old value (a memo made before the widget) reads
        // again, from the new signal.
        old.update(|_| {});
        Ok(signal)
    }

    pub(crate) fn usize(&self, py: Python<'_>, what: &str) -> PyResult<Signal<usize>> {
        let from = |s| match s {
            Slot::Usize(s) => Some(s),
            _ => None,
        };
        self.typed(py, what, from, Slot::Usize, |v| {
            v.extract::<usize>()
                .map_err(|_| PyTypeError::new_err("its value must be an int, at least 0"))
        })
    }

    pub(crate) fn u16(&self, py: Python<'_>, what: &str) -> PyResult<Signal<u16>> {
        let from = |s| match s {
            Slot::U16(s) => Some(s),
            _ => None,
        };
        self.typed(py, what, from, Slot::U16, |v| {
            v.extract::<u16>()
                .map_err(|_| PyTypeError::new_err("its value must be an int from 0 to 65535"))
        })
    }

    pub(crate) fn f64(&self, py: Python<'_>, what: &str) -> PyResult<Signal<f64>> {
        let from = |s| match s {
            Slot::F64(s) => Some(s),
            _ => None,
        };
        self.typed(py, what, from, Slot::F64, |v| {
            v.extract::<f64>()
                .map_err(|_| PyTypeError::new_err("its value must be a float"))
        })
    }

    pub(crate) fn date(&self, py: Python<'_>, what: &str) -> PyResult<Signal<Date>> {
        let from = |s| match s {
            Slot::Date(s) => Some(s),
            _ => None,
        };
        self.typed(py, what, from, Slot::Date, date_from)
    }

    pub(crate) fn path(&self, py: Python<'_>, what: &str) -> PyResult<Signal<Vec<usize>>> {
        let from = |s| match s {
            Slot::Path(s) => Some(s),
            _ => None,
        };
        self.typed(py, what, from, Slot::Path, path_from)
    }

    pub(crate) fn paths(
        &self,
        py: Python<'_>,
        what: &str,
    ) -> PyResult<Signal<HashSet<Vec<usize>>>> {
        let from = |s| match s {
            Slot::Paths(s) => Some(s),
            _ => None,
        };
        self.typed(py, what, from, Slot::Paths, paths_from)
    }

    pub(crate) fn sort(
        &self,
        py: Python<'_>,
        what: &str,
    ) -> PyResult<Signal<Option<(usize, Order)>>> {
        let from = |s| match s {
            Slot::Sort(s) => Some(s),
            _ => None,
        };
        self.typed(py, what, from, Slot::Sort, sort_from)
    }

    pub(crate) fn key(&self, py: Python<'_>, what: &str) -> PyResult<Signal<Option<String>>> {
        let from = |s| match s {
            Slot::Key(s) => Some(s),
            _ => None,
        };
        self.typed(py, what, from, Slot::Key, |v| {
            v.extract::<Option<String>>()
                .map_err(|_| PyTypeError::new_err("its value must be a str or None"))
        })
    }
}

#[pymethods]
impl PySignal {
    /// The value; a node drawing (or a memo computing) subscribes to it.
    fn get(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.read(py, true)
    }

    /// The value, without subscribing (in a handler, say).
    fn get_untracked(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.read(py, false)
    }

    /// Replace the value; readers update only if it changed (`==`).
    fn set(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.write(value, false)
    }

    /// Set the value to `f(value)`; when `f` returns `None`, keep the value
    /// (which `f` changed in place) and tell the readers all the same.
    fn update(&self, py: Python<'_>, f: &Bound<'_, PyAny>) -> PyResult<()> {
        let value = self.read(py, false)?;
        let new = f.call1((value.bind(py),))?;
        if new.is_none() {
            self.write(value.bind(py), true)
        } else {
            self.write(&new, true)
        }
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        match self.read(py, false) {
            Ok(value) => format!(
                "Signal({})",
                value
                    .bind(py)
                    .repr()
                    .map_or_else(|_| "?".to_string(), |r| r.to_string())
            ),
            Err(_) => "Signal(<finished>)".to_string(),
        }
    }
}

/// A signal argument, checked.
pub(crate) fn signal_arg<'py>(
    value: &Bound<'py, PyAny>,
    what: &str,
) -> PyResult<Bound<'py, PySignal>> {
    value
        .cast::<PySignal>()
        .cloned()
        .map_err(|_| PyTypeError::new_err(format!("{what} must be a Signal (from signal(value))")))
}

/// `signal(value)`: a new signal holding `value`, in the app being built
/// or run.
#[pyfunction]
fn tui_signal(value: &Bound<'_, PyAny>) -> PyResult<PySignal> {
    let app = require("signal()")?;
    Ok(PySignal {
        slot: Mutex::new(Slot::Any(intuituive::signal(PyValue(
            value.clone().unbind(),
        )))),
        app: Some(app),
        home: Home::here(),
    })
}

/// `Memo`: a value derived from signals, recomputed when they change; its
/// readers update only when the result is different (`==`).
#[pyclass(name = "Memo", module = "rs_rich.tui", frozen)]
pub(crate) struct PyMemo {
    memo: Memo<PyValue>,
    app: Option<Handle>,
    home: Home,
}

impl PyMemo {
    fn alive(&self) -> PyResult<()> {
        self.home.check("memo")?;
        if self.app.as_ref().is_some_and(|app| app.closed()) {
            return Err(PyRuntimeError::new_err(
                "the app this memo belongs to has finished",
            ));
        }
        Ok(())
    }
}

#[pymethods]
impl PyMemo {
    fn get(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.alive()?;
        Ok(self.memo.with(|v| v.object(py)))
    }

    fn get_untracked(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.alive()?;
        Ok(self.memo.with_untracked(|v| v.object(py)))
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        match self.get_untracked(py) {
            Ok(value) => format!(
                "Memo({})",
                value
                    .bind(py)
                    .repr()
                    .map_or_else(|_| "?".to_string(), |r| r.to_string())
            ),
            Err(_) => "Memo(<finished>)".to_string(),
        }
    }
}

/// `memo(f)`: a memo computing `f()`. `f` only computes: writing a signal
/// inside it raises.
#[pyfunction]
fn tui_memo(f: &Bound<'_, PyAny>) -> PyResult<PyMemo> {
    let app = require("memo()")?;
    let f = Callback::checked(f, "memo's function")?;
    let memo = intuituive::memo(move || f.call0().map(PyValue).unwrap_or_else(PyValue::none));
    Ok(PyMemo {
        memo,
        app: Some(app),
        home: Home::here(),
    })
}

/// `watch(source, on_change)`: call `on_change(value, cx)` with the value
/// of `source()` whenever it changes (`==`), and once at the start,
/// between frames. A watch made while a screen is built stops with it.
#[pyfunction]
fn tui_watch(source: &Bound<'_, PyAny>, on_change: &Bound<'_, PyAny>) -> PyResult<()> {
    require("watch()")?;
    let source = Callback::checked(source, "watch's source")?;
    let on_change = Callback::checked(on_change, "watch's callback")?;
    intuituive::watch(
        move || source.call0().map(PyValue).unwrap_or_else(PyValue::none),
        move |value: PyValue, cx| call_with_ctx(&on_change, Some(value), cx),
    );
    Ok(())
}

/// Call `callback(value, cx)` (or `callback(cx)` without a value), lending
/// it `cx` for the call.
pub(crate) fn call_with_ctx(callback: &Callback, value: Option<PyValue>, cx: &mut intuituive::Ctx) {
    let loan = Loan::new();
    callback.call(
        |py| {
            let ctx = PyCtx::lend(py, cx, &loan)?;
            match &value {
                Some(value) => PyTuple::new(py, [value.object(py), ctx.into_any().unbind()]),
                None => PyTuple::new(py, [ctx]),
            }
        },
        |_| Ok(()),
    );
}

/// `every(interval, tick)`: call `tick(cx)` every `interval` seconds (a
/// float or a `timedelta`) while the app runs. Inside `App`'s build
/// function or a screen's; its timers stop when the screen closes.
#[pyfunction]
fn tui_every(interval: &Bound<'_, PyAny>, tick: &Bound<'_, PyAny>) -> PyResult<()> {
    require("every()")?;
    if !Building::active() {
        return Err(PyRuntimeError::new_err(
            "every() is called inside App's build function or a screen's",
        ));
    }
    let interval = crate::ext::common::seconds(interval)?;
    let tick = Callback::checked(tick, "every's tick")?;
    intuituive::every(interval, move |cx| call_with_ctx(&tick, None, cx));
    Ok(())
}

// ---------------------------------------------------------------------------
// Logs

/// `Log(capacity)`: a bounded log of console-markup lines. `push(line)`
/// adds one at the bottom, `clear()` empties it, and `view()` is a node
/// showing the latest lines, which on an append renders only the new ones.
#[pyclass(name = "Log", module = "rs_rich.tui", frozen)]
pub(crate) struct PyLog {
    log: intuituive::Log,
    app: Option<Handle>,
    home: Home,
}

impl PyLog {
    fn alive(&self) -> PyResult<()> {
        self.home.check("log")?;
        if self.app.as_ref().is_some_and(|app| app.closed()) {
            return Err(PyRuntimeError::new_err(
                "the app this log belongs to has finished",
            ));
        }
        Ok(())
    }
}

#[pymethods]
impl PyLog {
    #[new]
    #[pyo3(signature = (capacity=1000))]
    fn new(capacity: usize) -> PyResult<PyLog> {
        let app = require("Log()")?;
        Ok(PyLog {
            log: intuituive::Log::new(capacity),
            app: Some(app),
            home: Home::here(),
        })
    }

    fn push(&self, line: &str) -> PyResult<()> {
        self.alive()?;
        self.log.push(line);
        Ok(())
    }

    fn clear(&self) -> PyResult<()> {
        self.alive()?;
        self.log.clear();
        Ok(())
    }

    fn __len__(&self) -> PyResult<usize> {
        self.alive()?;
        Ok(self.log.len())
    }

    fn is_empty(&self) -> PyResult<bool> {
        self.alive()?;
        Ok(self.log.is_empty())
    }

    /// A node showing the latest lines, one per row.
    fn view(&self) -> PyResult<super::PyNode> {
        self.alive()?;
        Ok(super::PyNode::new(self.log.view()))
    }
}

// ---------------------------------------------------------------------------
// Background work

/// `Task`: background work started by `spawn`. `cancel()` drops its result
/// (`done` will not run); `is_finished()` says whether the work returned.
#[pyclass(name = "Task", module = "rs_rich.tui", frozen)]
pub(crate) struct PyTask(intuituive::Task);

#[pymethods]
impl PyTask {
    fn cancel(&self) {
        self.0.cancel();
    }

    fn is_finished(&self) -> bool {
        self.0.is_finished()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Task {}>",
            if self.0.is_finished() {
                "finished"
            } else {
                "running"
            }
        )
    }
}

/// Start `work()` on a thread of its own and call `done(result, cx)` with
/// what it returned, back on the app's thread. An exception from either
/// stops the app.
fn start(
    app: Handle,
    work: impl FnOnce(Python<'_>) -> PyResult<Py<PyAny>> + Send + 'static,
    done: Option<Py<PyAny>>,
) -> PyTask {
    let task = intuituive::spawn(
        move || Python::attach(work),
        move |result: PyResult<Py<PyAny>>, cx| match result {
            Ok(value) => {
                if let Some(done) = done {
                    let done = Callback::of(done, Some(app));
                    call_with_ctx(&done, Some(PyValue(value)), cx);
                }
            }
            Err(error) => Python::attach(|py| app.fail(py, error)),
        },
    );
    PyTask(task)
}

/// `spawn(work, done=None)`: run `work()` on a thread of its own (it takes
/// the GIL while it runs Python code, so the app keeps drawing), then
/// `done(result, cx)` on the app's thread, where it may write signals,
/// open a screen or quit.
#[pyfunction]
#[pyo3(signature = (work, done=None))]
fn tui_spawn(work: &Bound<'_, PyAny>, done: Option<&Bound<'_, PyAny>>) -> PyResult<PyTask> {
    let app = require("spawn()")?;
    let work = Callback::checked(work, "spawn's work")?
        .func()
        .clone_ref(work.py());
    let done = match done.filter(|d| !d.is_none()) {
        Some(done) => Some(
            Callback::checked(done, "spawn's done")?
                .func()
                .clone_ref(done.py()),
        ),
        None => None,
    };
    Ok(start(
        app,
        move |py| work.bind(py).call0().map(Bound::unbind),
        done,
    ))
}

/// `spawn_async(coroutine, done=None, *, loop=None)`: await `coroutine` (a
/// coroutine, or a function returning one) on an asyncio event loop, from
/// a thread of its own: `loop`'s, when given (a loop running on another
/// thread), else a new one for the coroutine (`asyncio.run`). Then
/// `done(result, cx)` on the app's thread.
#[pyfunction]
#[pyo3(signature = (coroutine, done=None, *, r#loop=None))]
fn tui_spawn_async(
    coroutine: &Bound<'_, PyAny>,
    done: Option<&Bound<'_, PyAny>>,
    r#loop: Option<&Bound<'_, PyAny>>,
) -> PyResult<PyTask> {
    let py = coroutine.py();
    let app = require("spawn_async()")?;
    let coroutine = coroutine.clone().unbind();
    let event_loop = r#loop.filter(|l| !l.is_none()).map(|l| l.clone().unbind());
    let done = match done.filter(|d| !d.is_none()) {
        Some(done) => Some(
            Callback::checked(done, "spawn_async's done")?
                .func()
                .clone_ref(py),
        ),
        None => None,
    };
    Ok(start(
        app,
        move |py| {
            let asyncio = py.import("asyncio")?;
            let mut awaitable = coroutine.into_bound(py);
            if awaitable.is_callable() && !awaitable.hasattr("__await__")? {
                awaitable = awaitable.call0()?;
            }
            match event_loop {
                Some(event_loop) => asyncio
                    .call_method1("run_coroutine_threadsafe", (awaitable, event_loop))?
                    .call_method0("result")
                    .map(Bound::unbind),
                None => {
                    if !asyncio
                        .call_method1("iscoroutine", (&awaitable,))?
                        .is_truthy()?
                    {
                        // `asyncio.run` takes only a coroutine: any other
                        // awaitable goes in one (`rs_rich.tui._awaited`).
                        awaitable = py
                            .import("rs_rich.tui")?
                            .getattr("_awaited")?
                            .call1((awaitable,))?;
                    }
                    asyncio.call_method1("run", (awaitable,)).map(Bound::unbind)
                }
            }
        },
        done,
    ))
}

/// `Load`: a resource's state: `loading`, else `value` (`ready`) or
/// `error` (`failed`).
#[pyclass(name = "Load", module = "rs_rich.tui", frozen)]
pub(crate) struct PyLoad {
    state: Load<PyValue>,
}

#[pymethods]
impl PyLoad {
    /// `"loading"`, `"ready"` or `"failed"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.state {
            Load::Loading => "loading",
            Load::Ready(_) => "ready",
            Load::Failed(_) => "failed",
        }
    }

    fn is_loading(&self) -> bool {
        self.state.is_loading()
    }

    /// The value, if it arrived.
    fn ready(&self, py: Python<'_>) -> Py<PyAny> {
        self.state
            .ready()
            .map_or_else(|| py.None(), |value| value.object(py))
    }

    /// The value, if it arrived (`None` otherwise).
    #[getter]
    fn value(&self, py: Python<'_>) -> Py<PyAny> {
        self.ready(py)
    }

    /// Why the fetch failed, if it did.
    #[getter]
    fn error(&self) -> Option<String> {
        match &self.state {
            Load::Failed(error) => Some(error.clone()),
            _ => None,
        }
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        match &self.state {
            Load::Loading => "Load.Loading".to_string(),
            Load::Ready(value) => format!(
                "Load.Ready({})",
                value
                    .0
                    .bind(py)
                    .repr()
                    .map_or_else(|_| "?".to_string(), |r| r.to_string())
            ),
            Load::Failed(error) => format!("Load.Failed({error:?})"),
        }
    }
}

/// `Resource`: a value fetched in the background. `get()` is its `Load`
/// (reading it subscribes the reader, which redraws when it arrives), and
/// `reload()` fetches again, dropping any older result still on its way.
#[pyclass(name = "Resource", module = "rs_rich.tui", frozen)]
pub(crate) struct PyResource {
    resource: Resource<PyValue>,
    app: Option<Handle>,
    home: Home,
}

impl PyResource {
    fn alive(&self) -> PyResult<()> {
        self.home.check("resource")?;
        if self.app.as_ref().is_some_and(|app| app.closed()) {
            return Err(PyRuntimeError::new_err(
                "the app this resource belongs to has finished",
            ));
        }
        Ok(())
    }
}

#[pymethods]
impl PyResource {
    fn get(&self) -> PyResult<PyLoad> {
        self.alive()?;
        Ok(PyLoad {
            state: self.resource.get(),
        })
    }

    fn reload(&self) -> PyResult<()> {
        self.alive()?;
        self.resource.reload();
        Ok(())
    }
}

/// `resource(fetch)`: call `fetch()` on a thread of its own, now and on
/// every `reload()`. An exception it raises is the resource failing
/// (`Load.error` is its message), not the app's.
#[pyfunction]
fn tui_resource(fetch: &Bound<'_, PyAny>) -> PyResult<PyResource> {
    let app = require("resource()")?;
    let fetch = Callback::checked(fetch, "resource's fetch")?
        .func()
        .clone_ref(fetch.py());
    let resource = intuituive::resource(move || {
        Python::attach(|py| {
            fetch
                .bind(py)
                .call0()
                .map(|value| PyValue(value.unbind()))
                .map_err(|error| {
                    let message = error
                        .value(py)
                        .str()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    if message.is_empty() {
                        error
                            .get_type(py)
                            .name()
                            .map(|n| n.to_string())
                            .unwrap_or_default()
                    } else {
                        message
                    }
                })
        })
    });
    Ok(PyResource {
        resource,
        app: Some(app),
        home: Home::here(),
    })
}

/// `Proxy`: a handle any thread uses to change the app's state: `run(f)`
/// calls `f()` on the app's thread, between frames, where it may write
/// signals; `run_with(f)` calls `f(cx)`. Each returns `False` once the app
/// has finished.
#[pyclass(name = "Proxy", module = "rs_rich.tui", frozen)]
pub(crate) struct PyProxy {
    proxy: intuituive::Proxy,
    app: Option<Handle>,
}

impl PyProxy {
    pub(crate) fn new(proxy: intuituive::Proxy, app: Option<Handle>) -> PyProxy {
        PyProxy { proxy, app }
    }
}

#[pymethods]
impl PyProxy {
    fn run(&self, f: &Bound<'_, PyAny>) -> PyResult<bool> {
        let f = Callback::checked(f, "the proxy's function")?
            .func()
            .clone_ref(f.py());
        let app = self.app.clone();
        Ok(self.proxy.run(move || {
            Callback::of(f, app).call0();
        }))
    }

    fn run_with(&self, f: &Bound<'_, PyAny>) -> PyResult<bool> {
        let f = Callback::checked(f, "the proxy's function")?
            .func()
            .clone_ref(f.py());
        let app = self.app.clone();
        Ok(self.proxy.run_with(move |cx| {
            call_with_ctx(&Callback::of(f, app), None, cx);
        }))
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    for (name, class) in [
        ("TuiSignal", py.get_type::<PySignal>()),
        ("TuiMemo", py.get_type::<PyMemo>()),
        ("TuiLog", py.get_type::<PyLog>()),
        ("TuiTask", py.get_type::<PyTask>()),
        ("TuiLoad", py.get_type::<PyLoad>()),
        ("TuiResource", py.get_type::<PyResource>()),
        ("TuiProxy", py.get_type::<PyProxy>()),
    ] {
        m.add(name, class)?;
    }
    m.add_function(wrap_pyfunction!(tui_signal, m)?)?;
    m.add_function(wrap_pyfunction!(tui_memo, m)?)?;
    m.add_function(wrap_pyfunction!(tui_watch, m)?)?;
    m.add_function(wrap_pyfunction!(tui_every, m)?)?;
    m.add_function(wrap_pyfunction!(tui_spawn, m)?)?;
    m.add_function(wrap_pyfunction!(tui_spawn_async, m)?)?;
    m.add_function(wrap_pyfunction!(tui_resource, m)?)?;
    Ok(())
}
