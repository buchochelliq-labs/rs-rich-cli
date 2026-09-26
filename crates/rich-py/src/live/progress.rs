//! `rich.progress`: `Task`, the columns, `Progress`, `track`, `wrap_file`
//! and `open`.
//!
//! Task values stay the Python objects they were given (an `int` total
//! prints as `200`, not `200.0`) and are combined with Python's own
//! arithmetic, as upstream's are. Columns are Python-subclassable: a
//! `Progress` calls each column (`column(task)`, with upstream's refresh
//! cache), which calls its `render(task)`. The tasks grid is a core
//! `Table.grid`; the built-in columns' cells are built here in Rust.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::exceptions::{PyAssertionError, PyKeyError, PyNotImplementedError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyCFunction, PyDict, PyList, PyString, PyTuple, PyType};
use pyo3::{PyTraverseError, PyVisit};

use rich::table::{Cell, ColumnOptions};
use rich::{Spinner as CoreSpinner, Table as CoreTable, Text as CoreText};

use super::live_display::{self, Live};
use super::progress_bar::ProgressBar;
use super::util::{self, hold, Arg};
use crate::convert;
use crate::errors::NotRenderableError;
use crate::renderable::{self, PyRenderable};
use crate::style::{style_type, Style};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn float(py: Python<'_>, value: f64) -> Py<PyAny> {
    value
        .into_pyobject(py)
        .expect("a float converts")
        .into_any()
        .unbind()
}

fn int(py: Python<'_>, value: i64) -> Py<PyAny> {
    value
        .into_pyobject(py)
        .expect("an int converts")
        .into_any()
        .unbind()
}

fn text_obj(py: Python<'_>, plain: &str, style: &str) -> PyResult<Py<PyAny>> {
    util::new_text(py, CoreText::styled(plain, style))
}

/// Python's `int(value)`, unbounded as Rich's arithmetic on it.
fn py_int<'py>(value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    value
        .py()
        .import("builtins")?
        .getattr("int")?
        .call1((value,))
}

/// Python's `format(value, spec)`.
fn format_obj(value: &Bound<'_, PyAny>, spec: &str) -> PyResult<String> {
    value
        .py()
        .import("builtins")?
        .getattr("format")?
        .call1((value, spec))?
        .extract()
}

/// `rich.filesize.decimal(size)`, on a Python `int` of any size.
fn filesize_decimal(size: &Bound<'_, PyAny>) -> PyResult<String> {
    const SUFFIXES: [&str; 8] = ["kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let py = size.py();
    if size.eq(1)? {
        return Ok("1 byte".to_string());
    }
    if size.lt(1000)? {
        return Ok(format!("{} bytes", format_obj(size, ",")?));
    }
    let base = 1000i64.into_pyobject(py)?.into_any();
    let mut unit = base.clone();
    let mut suffix = SUFFIXES[0];
    for candidate in SUFFIXES {
        unit = unit.mul(&base)?;
        suffix = candidate;
        if size.lt(&unit)? {
            break;
        }
    }
    let value = base.mul(size)?.div(&unit)?;
    Ok(format!("{} {suffix}", format_obj(&value, ",.1f")?))
}

/// `rich.filesize.pick_unit_and_suffix(size, suffixes, base)`, on a Python
/// `int` of any size.
fn pick_unit_and_suffix<'py>(
    size: &Bound<'py, PyAny>,
    suffixes: &[&'static str],
    base: i64,
) -> PyResult<(Bound<'py, PyAny>, &'static str)> {
    let py = size.py();
    let base = base.into_pyobject(py)?.into_any();
    let mut unit = 1i64.into_pyobject(py)?.into_any();
    let mut suffix = suffixes[0];
    for (index, candidate) in suffixes.iter().enumerate() {
        if index > 0 {
            unit = unit.mul(&base)?;
        }
        suffix = candidate;
        if size.lt(unit.mul(&base)?)? {
            break;
        }
    }
    Ok((unit, suffix))
}

// ---------------------------------------------------------------------------
// Task

struct TaskState {
    id: Py<PyAny>,
    description: Py<PyAny>,
    total: Py<PyAny>,
    completed: Py<PyAny>,
    get_time: Py<PyAny>,
    finished_time: Py<PyAny>,
    visible: Py<PyAny>,
    fields: Py<PyAny>,
    start_time: Py<PyAny>,
    stop_time: Py<PyAny>,
    finished_speed: Py<PyAny>,
    /// `(timestamp, completed)` samples, at most 1000 (upstream's deque).
    samples: VecDeque<(Py<PyAny>, Py<PyAny>)>,
    lock: Py<PyAny>,
}

/// `rich.progress.Task`: one task of a `Progress`.
#[pyclass(name = "Task", module = "rs_rich.progress", frozen)]
pub(crate) struct Task {
    state: Mutex<TaskState>,
}

macro_rules! task_field {
    ($self:ident, $py:ident, $field:ident) => {
        lock(&$self.state).$field.clone_ref($py).into_bound($py)
    };
}

impl Task {
    fn st(&self) -> MutexGuard<'_, TaskState> {
        lock(&self.state)
    }

    fn now<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        task_field!(self, py, get_time).call0()
    }

    fn elapsed_value<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let (start, stop) = {
            let state = self.st();
            (
                state.start_time.clone_ref(py).into_bound(py),
                state.stop_time.clone_ref(py).into_bound(py),
            )
        };
        if start.is_none() {
            return Ok(py.None().into_bound(py));
        }
        if !stop.is_none() {
            return stop.sub(&start);
        }
        self.now(py)?.sub(&start)
    }

    fn is_finished(&self, py: Python<'_>) -> bool {
        !self.st().finished_time.is_none(py)
    }

    fn speed_value<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let none = py.None().into_bound(py);
        let samples: Vec<(Py<PyAny>, Py<PyAny>)> = {
            let state = self.st();
            if state.start_time.is_none(py) {
                return Ok(none);
            }
            state
                .samples
                .iter()
                .map(|(t, c)| (t.clone_ref(py), c.clone_ref(py)))
                .collect()
        };
        let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
            return Ok(none);
        };
        let total_time = last.0.bind(py).sub(first.0.bind(py))?;
        if total_time.eq(0)? {
            return Ok(none);
        }
        let mut total_completed = int(py, 0).into_bound(py);
        for (_, completed) in samples.iter().skip(1) {
            total_completed = total_completed.add(completed.bind(py))?;
        }
        total_completed.div(&total_time)
    }

    fn remaining_value<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let (total, completed) = {
            let state = self.st();
            (
                state.total.clone_ref(py).into_bound(py),
                state.completed.clone_ref(py).into_bound(py),
            )
        };
        if total.is_none() {
            return Ok(total);
        }
        total.sub(completed)
    }

    fn time_remaining_value<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        if self.is_finished(py) {
            return Ok(float(py, 0.0).into_bound(py));
        }
        let speed = self.speed_value(py)?;
        if speed.is_none() || !speed.is_truthy()? {
            return Ok(py.None().into_bound(py));
        }
        let remaining = self.remaining_value(py)?;
        if remaining.is_none() {
            return Ok(remaining);
        }
        py.import("math")?
            .call_method1("ceil", (remaining.div(speed)?,))
    }

    fn reset_progress(&self, py: Python<'_>) {
        let mut state = self.st();
        state.samples.clear();
        state.finished_time = py.None();
        state.finished_speed = py.None();
    }

    /// Drop samples older than `old`, keep at most 1000, then add one.
    fn push_sample(
        &self,
        py: Python<'_>,
        old: &Bound<'_, PyAny>,
        sample: Option<(Py<PyAny>, Py<PyAny>)>,
        cap_first: bool,
    ) -> PyResult<()> {
        loop {
            let front = self.st().samples.front().map(|(t, _)| t.clone_ref(py));
            match front {
                Some(time) if time.bind(py).lt(old)? => {
                    self.st().samples.pop_front();
                }
                _ => break,
            }
        }
        let mut state = self.st();
        if cap_first {
            while state.samples.len() > 1000 {
                state.samples.pop_front();
            }
        }
        if let Some(sample) = sample {
            state.samples.push_back(sample);
            while state.samples.len() > 1000 {
                state.samples.pop_front();
            }
        }
        Ok(())
    }
}

#[pymethods]
impl Task {
    #[new]
    #[pyo3(signature = (
        id, description, total, completed, _get_time, finished_time=None, visible=None,
        fields=None, finished_speed=None, _lock=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        id: Py<PyAny>,
        description: Py<PyAny>,
        total: Py<PyAny>,
        completed: Py<PyAny>,
        _get_time: Py<PyAny>,
        finished_time: Option<Py<PyAny>>,
        visible: Option<Py<PyAny>>,
        fields: Option<Py<PyAny>>,
        finished_speed: Option<Py<PyAny>>,
        _lock: Option<Py<PyAny>>,
    ) -> PyResult<Task> {
        Ok(Task {
            state: Mutex::new(TaskState {
                id,
                description,
                total,
                completed,
                get_time: _get_time,
                finished_time: finished_time.unwrap_or_else(|| py.None()),
                visible: visible.unwrap_or_else(|| {
                    pyo3::types::PyBool::new(py, true)
                        .to_owned()
                        .into_any()
                        .unbind()
                }),
                fields: match fields {
                    Some(fields) => fields,
                    None => PyDict::new(py).into_any().unbind(),
                },
                start_time: py.None(),
                stop_time: py.None(),
                finished_speed: finished_speed.unwrap_or_else(|| py.None()),
                samples: VecDeque::new(),
                lock: match _lock {
                    Some(lock) => lock,
                    None => util::rlock(py)?,
                },
            }),
        })
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let state = self.st();
        let r = |v: &Py<PyAny>| -> PyResult<String> { Ok(v.bind(py).repr()?.to_string()) };
        Ok(format!(
            "Task(id={}, description={}, total={}, completed={}, _get_time={}, finished_time={}, \
             visible={}, fields={}, finished_speed={}, _lock={})",
            r(&state.id)?,
            r(&state.description)?,
            r(&state.total)?,
            r(&state.completed)?,
            r(&state.get_time)?,
            r(&state.finished_time)?,
            r(&state.visible)?,
            r(&state.fields)?,
            r(&state.finished_speed)?,
            r(&state.lock)?,
        ))
    }

    #[getter]
    fn id(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().id.clone_ref(py)
    }

    #[setter]
    fn set_id(&self, value: Py<PyAny>) {
        self.st().id = value;
    }

    #[getter]
    fn description(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().description.clone_ref(py)
    }

    #[setter]
    fn set_description(&self, value: Py<PyAny>) {
        self.st().description = value;
    }

    #[getter]
    fn total(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().total.clone_ref(py)
    }

    #[setter]
    fn set_total(&self, value: Py<PyAny>) {
        self.st().total = value;
    }

    #[getter]
    fn completed(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().completed.clone_ref(py)
    }

    #[setter]
    fn set_completed(&self, value: Py<PyAny>) {
        self.st().completed = value;
    }

    #[getter]
    fn finished_time(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().finished_time.clone_ref(py)
    }

    #[setter]
    fn set_finished_time(&self, value: Py<PyAny>) {
        self.st().finished_time = value;
    }

    #[getter]
    fn visible(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().visible.clone_ref(py)
    }

    #[setter]
    fn set_visible(&self, value: Py<PyAny>) {
        self.st().visible = value;
    }

    #[getter]
    fn fields(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().fields.clone_ref(py)
    }

    #[setter]
    fn set_fields(&self, value: Py<PyAny>) {
        self.st().fields = value;
    }

    #[getter]
    fn start_time(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().start_time.clone_ref(py)
    }

    #[setter]
    fn set_start_time(&self, value: Py<PyAny>) {
        self.st().start_time = value;
    }

    #[getter]
    fn stop_time(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().stop_time.clone_ref(py)
    }

    #[setter]
    fn set_stop_time(&self, value: Py<PyAny>) {
        self.st().stop_time = value;
    }

    #[getter]
    fn finished_speed(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().finished_speed.clone_ref(py)
    }

    #[setter]
    fn set_finished_speed(&self, value: Py<PyAny>) {
        self.st().finished_speed = value;
    }

    #[getter]
    fn _get_time(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().get_time.clone_ref(py)
    }

    #[getter]
    fn _lock(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().lock.clone_ref(py)
    }

    /// The speed samples, as `(timestamp, completed)` tuples.
    #[getter]
    fn _progress<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let state = self.st();
        PyList::new(
            py,
            state
                .samples
                .iter()
                .map(|(t, c)| (t.clone_ref(py), c.clone_ref(py))),
        )
    }

    /// The current time, in seconds.
    fn get_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.now(py)
    }

    #[getter]
    fn started(&self, py: Python<'_>) -> bool {
        !self.st().start_time.is_none(py)
    }

    #[getter]
    fn remaining<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.remaining_value(py)
    }

    #[getter]
    fn elapsed<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.elapsed_value(py)
    }

    #[getter]
    fn finished(&self, py: Python<'_>) -> bool {
        self.is_finished(py)
    }

    #[getter]
    fn percentage(&self, py: Python<'_>) -> PyResult<f64> {
        let (total, completed) = {
            let state = self.st();
            (
                state.total.clone_ref(py).into_bound(py),
                state.completed.clone_ref(py).into_bound(py),
            )
        };
        if !total.is_truthy()? {
            return Ok(0.0);
        }
        let percentage: f64 = completed.div(total)?.mul(100.0)?.extract()?;
        // `min(100.0, max(0.0, percentage))`: Python's min and max keep their
        // first argument unless the other compares strictly past it, so -0.0
        // and NaN become 0.0 (`f64::clamp` keeps both).
        let percentage = if percentage > 0.0 { percentage } else { 0.0 };
        Ok(if percentage < 100.0 {
            percentage
        } else {
            100.0
        })
    }

    #[getter]
    fn speed<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.speed_value(py)
    }

    #[getter]
    fn time_remaining<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.time_remaining_value(py)
    }

    fn _reset(&self, py: Python<'_>) {
        self.reset_progress(py);
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(state) = self.state.try_lock() {
            for object in [
                &state.id,
                &state.description,
                &state.total,
                &state.completed,
                &state.get_time,
                &state.finished_time,
                &state.visible,
                &state.fields,
                &state.start_time,
                &state.stop_time,
                &state.finished_speed,
                &state.lock,
            ] {
                visit.call(object)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Columns

/// `rich.table.Column()` when the bindings have one, else `None`.
fn default_column(py: Python<'_>, no_wrap: bool) -> PyResult<Py<PyAny>> {
    let native = py.import("rs_rich._native")?;
    if let Some(column) = native.getattr_opt("Column")? {
        let kwargs = PyDict::new(py);
        if no_wrap {
            kwargs.set_item("no_wrap", true)?;
        }
        if let Ok(made) = column.call((), Some(&kwargs)) {
            return Ok(made.unbind());
        }
    }
    Ok(py.None())
}

/// `rich.progress.ProgressColumn`: the base of a column. Subclass it and
/// implement `render(task)`; set `max_refresh` to reuse a render for that
/// many seconds while the task has completed nothing.
#[pyclass(
    name = "ProgressColumn",
    module = "rs_rich.progress",
    subclass,
    dict,
    frozen
)]
pub(crate) struct ProgressColumn {
    table_column: Mutex<Option<Py<PyAny>>>,
    /// Upstream's text columns default to `Column(no_wrap=True)`.
    no_wrap: Mutex<bool>,
    cache: Mutex<HashMap<i64, Cached>>,
}

/// A column's last render for a task: `(timestamp, renderable)`.
type Cached = (Py<PyAny>, Py<PyAny>);

impl ProgressColumn {
    fn blank() -> ProgressColumn {
        ProgressColumn {
            table_column: Mutex::new(None),
            no_wrap: Mutex::new(false),
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn init(&self, py: Python<'_>, table_column: Option<Py<PyAny>>, no_wrap: bool) {
        *lock(&self.table_column) = table_column.filter(|c| !c.is_none(py));
        *lock(&self.no_wrap) = no_wrap;
    }
}

#[pymethods]
impl ProgressColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> ProgressColumn {
        ProgressColumn::blank()
    }

    #[pyo3(signature = (table_column=None))]
    fn __init__(&self, py: Python<'_>, table_column: Option<Py<PyAny>>) {
        self.init(py, table_column, false);
    }

    #[classattr]
    fn max_refresh(py: Python<'_>) -> Py<PyAny> {
        py.None()
    }

    /// The table column the tasks grid uses for this column.
    fn get_table_column(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if let Some(column) = lock(&self.table_column).as_ref() {
            return Ok(column.clone_ref(py));
        }
        default_column(py, *lock(&self.no_wrap))
    }

    /// The renderable for `task`: `render(task)`, reused within `max_refresh`
    /// seconds while the task has completed nothing.
    fn __call__(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let current_time = task.call_method0("get_time")?;
        let max_refresh = slf.getattr("max_refresh")?;
        let id: i64 = task.getattr("id")?.extract()?;
        if !max_refresh.is_none() && !task.getattr("completed")?.is_truthy()? {
            let cached = lock(&slf.get().cache)
                .get(&id)
                .map(|(t, r)| (t.clone_ref(py), r.clone_ref(py)));
            if let Some((timestamp, renderable)) = cached {
                if timestamp.bind(py).add(&max_refresh)?.gt(&current_time)? {
                    return Ok(renderable);
                }
            }
        }
        let renderable = slf.call_method1("render", (task,))?.unbind();
        lock(&slf.get().cache).insert(id, (current_time.unbind(), renderable.clone_ref(py)));
        Ok(renderable)
    }

    /// Return a renderable for `task` (implemented by subclasses).
    fn render(&self, _task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        Err(PyNotImplementedError::new_err(
            "ProgressColumn.render is abstract: subclass ProgressColumn and implement render(task)",
        ))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(column) = self.table_column.try_lock() {
            if let Some(column) = column.as_ref() {
                visit.call(column)?;
            }
        }
        if let Ok(cache) = self.cache.try_lock() {
            for (time, renderable) in cache.values() {
                visit.call(time)?;
                visit.call(renderable)?;
            }
        }
        Ok(())
    }

    fn __clear__(&self) {
        if let Ok(mut cache) = self.cache.try_lock() {
            cache.clear();
        }
        if let Ok(mut column) = self.table_column.try_lock() {
            *column = None;
        }
    }
}

fn base_init() -> PyClassInitializer<ProgressColumn> {
    PyClassInitializer::from(ProgressColumn::blank())
}

/// Set the base part of a column from a subclass's `__init__`.
fn init_base(
    slf: &Bound<'_, PyAny>,
    table_column: Option<Py<PyAny>>,
    no_wrap: bool,
) -> PyResult<()> {
    let base = slf.cast::<ProgressColumn>()?;
    base.get().init(slf.py(), table_column, no_wrap);
    Ok(())
}

/// `rich.progress.RenderableColumn(renderable="", *, table_column=None)`.
#[pyclass(name = "RenderableColumn", module = "rs_rich.progress", extends = ProgressColumn, subclass, frozen)]
pub(crate) struct RenderableColumn;

#[pymethods]
impl RenderableColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<RenderableColumn> {
        base_init().add_subclass(RenderableColumn)
    }

    #[pyo3(signature = (renderable=None, *, table_column=None))]
    fn __init__(
        slf: &Bound<'_, Self>,
        renderable: Option<Py<PyAny>>,
        table_column: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let py = slf.py();
        let renderable = renderable.unwrap_or_else(|| PyString::new(py, "").into_any().unbind());
        slf.setattr("renderable", renderable)?;
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, _task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        Ok(slf.getattr("renderable")?.unbind())
    }
}

/// `rich.progress.SpinnerColumn(spinner_name="dots", style="progress.spinner",
/// speed=1.0, finished_text=" ", table_column=None)`.
#[pyclass(name = "SpinnerColumn", module = "rs_rich.progress", extends = ProgressColumn, subclass, frozen)]
pub(crate) struct SpinnerColumn {
    spinner: Mutex<CoreSpinner>,
}

fn make_spinner(name: &str, style: Option<&Bound<'_, PyAny>>, speed: f64) -> PyResult<CoreSpinner> {
    // Rich's `Spinner(name)` raises `KeyError` for an unknown name.
    if rich::spinner::spinner_frames(name).is_none() {
        return Err(pyo3::exceptions::PyKeyError::new_err(format!(
            "no spinner called {}",
            Python::attach(|py| pyo3::types::PyString::new(py, name)
                .repr()
                .map(|r| r.to_string()))?
        )));
    }
    let mut spinner = CoreSpinner::new(name).speed(speed);
    if let Some(style) = style_type(style)? {
        spinner = spinner.style(style);
    }
    Ok(spinner)
}

#[pymethods]
impl SpinnerColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<SpinnerColumn> {
        base_init().add_subclass(SpinnerColumn {
            spinner: Mutex::new(CoreSpinner::new("dots")),
        })
    }

    #[pyo3(signature = (
        spinner_name="dots", style=Arg::Missing, speed=1.0, finished_text=None, table_column=None
    ))]
    fn __init__(
        slf: &Bound<'_, Self>,
        spinner_name: &str,
        style: Arg,
        speed: f64,
        finished_text: Option<Bound<'_, PyAny>>,
        table_column: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let py = slf.py();
        let style = style.or_else(|| PyString::new(py, "progress.spinner").into_any().unbind());
        *lock(&slf.get().spinner) = make_spinner(spinner_name, Some(style.bind(py)), speed)?;
        let finished_text = match finished_text {
            None => util::new_text(py, CoreText::new(" "))?,
            Some(text) if util::is_str(&text) => {
                let text = util::to_str(&text)?;
                util::new_text(py, renderable::render_str(&text, true, true, false)?)?
            }
            Some(text) => text.unbind(),
        };
        slf.setattr("finished_text", finished_text)?;
        init_base(slf.as_any(), table_column, false)
    }

    /// Replace the spinner.
    #[pyo3(signature = (spinner_name, spinner_style=Arg::Missing, speed=1.0))]
    fn set_spinner(
        &self,
        py: Python<'_>,
        spinner_name: &str,
        spinner_style: Arg,
        speed: f64,
    ) -> PyResult<()> {
        let style =
            spinner_style.or_else(|| PyString::new(py, "progress.spinner").into_any().unbind());
        *lock(&self.spinner) = make_spinner(spinner_name, Some(style.bind(py)), speed)?;
        Ok(())
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        if task.getattr("finished")?.is_truthy()? {
            return Ok(slf.getattr("finished_text")?.unbind());
        }
        let time: f64 = task.call_method0("get_time")?.extract()?;
        let frame = lock(&slf.get().spinner).render(time);
        util::new_text(py, frame)
    }
}

/// `text_format.format(task=task)` as markup (or plain) text with a style
/// and justification, then the highlighter: upstream's `TextColumn.render`.
fn format_text(
    slf: &Bound<'_, PyAny>,
    text_format: &Bound<'_, PyAny>,
    task: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let py = slf.py();
    let kwargs = PyDict::new(py);
    kwargs.set_item("task", task)?;
    let formatted = util::to_str(&text_format.call_method("format", (), Some(&kwargs))?)?;
    let markup = slf.getattr("markup")?.is_truthy()?;
    let mut text = if markup {
        renderable::render_str(&formatted, true, true, false)?
    } else {
        CoreText::new(formatted)
    };
    if let Some(style) = style_type(Some(&slf.getattr("style")?))? {
        text.set_base_style(style);
    }
    let justify = slf.getattr("justify")?;
    let justify: Option<String> = if justify.is_none() {
        None
    } else {
        Some(justify.extract()?)
    };
    text.set_justify(convert::justify(justify.as_deref())?);
    let text = util::new_text(py, text)?;
    let highlighter = slf.getattr("highlighter")?;
    if highlighter.is_truthy()? {
        highlighter.call_method1("highlight", (text.bind(py),))?;
    }
    Ok(text)
}

/// `rich.progress.TextColumn(text_format, style="none", justify="left",
/// markup=True, highlighter=None, table_column=None)`.
#[pyclass(name = "TextColumn", module = "rs_rich.progress", extends = ProgressColumn, subclass, frozen)]
pub(crate) struct TextColumn;

#[pymethods]
impl TextColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<TextColumn> {
        base_init().add_subclass(TextColumn)
    }

    #[pyo3(signature = (
        text_format, style=None, justify="left", markup=true, highlighter=None, table_column=None
    ))]
    fn __init__(
        slf: &Bound<'_, Self>,
        text_format: Py<PyAny>,
        style: Option<Py<PyAny>>,
        justify: &str,
        markup: bool,
        highlighter: Option<Py<PyAny>>,
        table_column: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let py = slf.py();
        slf.setattr("text_format", text_format)?;
        slf.setattr("justify", justify)?;
        slf.setattr(
            "style",
            style.unwrap_or_else(|| PyString::new(py, "none").into_any().unbind()),
        )?;
        slf.setattr("markup", markup)?;
        slf.setattr("highlighter", highlighter)?;
        init_base(slf.as_any(), table_column, true)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        format_text(slf.as_any(), &slf.getattr("text_format")?, task)
    }
}

/// `rich.progress.BarColumn(bar_width=40, style="bar.back",
/// complete_style="bar.complete", finished_style="bar.finished",
/// pulse_style="bar.pulse", table_column=None)`.
#[pyclass(name = "BarColumn", module = "rs_rich.progress", extends = ProgressColumn, subclass, frozen)]
pub(crate) struct BarColumn;

#[pymethods]
impl BarColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<BarColumn> {
        base_init().add_subclass(BarColumn)
    }

    #[pyo3(signature = (
        bar_width=Arg::Missing, style=None, complete_style=None, finished_style=None,
        pulse_style=None, table_column=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn __init__(
        slf: &Bound<'_, Self>,
        bar_width: Arg,
        style: Option<Py<PyAny>>,
        complete_style: Option<Py<PyAny>>,
        finished_style: Option<Py<PyAny>>,
        pulse_style: Option<Py<PyAny>>,
        table_column: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let py = slf.py();
        let name = |value: Option<Py<PyAny>>, default: &str| {
            value.unwrap_or_else(|| PyString::new(py, default).into_any().unbind())
        };
        slf.setattr("bar_width", bar_width.or_else(|| int(py, 40)))?;
        slf.setattr("style", name(style, "bar.back"))?;
        slf.setattr("complete_style", name(complete_style, "bar.complete"))?;
        slf.setattr("finished_style", name(finished_style, "bar.finished"))?;
        slf.setattr("pulse_style", name(pulse_style, "bar.pulse"))?;
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<ProgressBar>> {
        let py = slf.py();
        let builtins = py.import("builtins")?;
        let max = builtins.getattr("max")?;
        let total = task.getattr("total")?;
        let total = if total.is_none() {
            None
        } else {
            Some(max.call1((0, total))?.unbind())
        };
        let completed = max.call1((0, task.getattr("completed")?))?.unbind();
        let bar_width = slf.getattr("bar_width")?;
        let width = if bar_width.is_none() {
            None
        } else {
            Some(bar_width.extract::<i64>()?.max(1) as usize)
        };
        let pulse = !task.getattr("started")?.is_truthy()?;
        let time: f64 = task.call_method0("get_time")?.extract()?;
        Py::new(
            py,
            ProgressBar::build(
                py,
                total,
                completed,
                width,
                pulse,
                slf.getattr("style")?.unbind(),
                slf.getattr("complete_style")?.unbind(),
                slf.getattr("finished_style")?.unbind(),
                slf.getattr("pulse_style")?.unbind(),
                Some(time),
            ),
        )
    }
}

macro_rules! simple_column {
    ($rust:ident, $name:literal, $doc:literal) => {
        #[doc = $doc]
        #[pyclass(name = $name, module = "rs_rich.progress", extends = ProgressColumn, subclass, frozen)]
        pub(crate) struct $rust;
    };
}

simple_column!(
    TimeElapsedColumn,
    "TimeElapsedColumn",
    "`rich.progress.TimeElapsedColumn`: the elapsed time, `H:MM:SS`."
);
simple_column!(
    FileSizeColumn,
    "FileSizeColumn",
    "`rich.progress.FileSizeColumn`: the completed size in decimal units."
);
simple_column!(
    TotalFileSizeColumn,
    "TotalFileSizeColumn",
    "`rich.progress.TotalFileSizeColumn`: the total size in decimal units."
);
simple_column!(
    TransferSpeedColumn,
    "TransferSpeedColumn",
    "`rich.progress.TransferSpeedColumn`: the transfer speed, e.g. `1.2 MB/s`."
);

/// `task.finished_speed or task.speed`.
fn finished_speed_or_speed<'py>(task: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let finished = task.getattr("finished_speed")?;
    if finished.is_truthy()? {
        return Ok(finished);
    }
    task.getattr("speed")
}

#[pymethods]
impl TimeElapsedColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<TimeElapsedColumn> {
        base_init().add_subclass(TimeElapsedColumn)
    }

    #[pyo3(signature = (table_column=None))]
    fn __init__(slf: &Bound<'_, Self>, table_column: Option<Py<PyAny>>) -> PyResult<()> {
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let elapsed = if task.getattr("finished")?.is_truthy()? {
            task.getattr("finished_time")?
        } else {
            task.getattr("elapsed")?
        };
        if elapsed.is_none() {
            return text_obj(py, "-:--:--", "progress.elapsed");
        }
        // `str(timedelta(seconds=max(0, int(elapsed))))`
        let seconds = py_int(&elapsed)?;
        let seconds = if seconds.lt(0)? {
            0i64.into_pyobject(py)?.into_any()
        } else {
            seconds
        };
        let kwargs = PyDict::new(py);
        kwargs.set_item("seconds", seconds)?;
        let delta = py
            .import("datetime")?
            .getattr("timedelta")?
            .call((), Some(&kwargs))?;
        text_obj(py, &delta.str()?.to_cow()?, "progress.elapsed")
    }
}

#[pymethods]
impl FileSizeColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<FileSizeColumn> {
        base_init().add_subclass(FileSizeColumn)
    }

    #[pyo3(signature = (table_column=None))]
    fn __init__(slf: &Bound<'_, Self>, table_column: Option<Py<PyAny>>) -> PyResult<()> {
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let size = filesize_decimal(&py_int(&task.getattr("completed")?)?)?;
        text_obj(slf.py(), &size, "progress.filesize")
    }
}

#[pymethods]
impl TotalFileSizeColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<TotalFileSizeColumn> {
        base_init().add_subclass(TotalFileSizeColumn)
    }

    #[pyo3(signature = (table_column=None))]
    fn __init__(slf: &Bound<'_, Self>, table_column: Option<Py<PyAny>>) -> PyResult<()> {
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let total = task.getattr("total")?;
        let size = if total.is_none() {
            String::new()
        } else {
            filesize_decimal(&py_int(&total)?)?
        };
        text_obj(slf.py(), &size, "progress.filesize.total")
    }
}

#[pymethods]
impl TransferSpeedColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<TransferSpeedColumn> {
        base_init().add_subclass(TransferSpeedColumn)
    }

    #[pyo3(signature = (table_column=None))]
    fn __init__(slf: &Bound<'_, Self>, table_column: Option<Py<PyAny>>) -> PyResult<()> {
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let speed = finished_speed_or_speed(task)?;
        if speed.is_none() {
            return text_obj(slf.py(), "?", "progress.data.speed");
        }
        let size = filesize_decimal(&py_int(&speed)?)?;
        text_obj(slf.py(), &format!("{size}/s"), "progress.data.speed")
    }
}

/// `rich.progress.MofNCompleteColumn(separator="/", table_column=None)`.
#[pyclass(name = "MofNCompleteColumn", module = "rs_rich.progress", extends = ProgressColumn, subclass, frozen)]
pub(crate) struct MofNCompleteColumn;

#[pymethods]
impl MofNCompleteColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<MofNCompleteColumn> {
        base_init().add_subclass(MofNCompleteColumn)
    }

    #[pyo3(signature = (separator="/", table_column=None))]
    fn __init__(
        slf: &Bound<'_, Self>,
        separator: &str,
        table_column: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        slf.setattr("separator", separator)?;
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let completed = py_int(&task.getattr("completed")?)?;
        let total = task.getattr("total")?;
        let total = if total.is_none() {
            "?".to_string()
        } else {
            py_int(&total)?.str()?.to_cow()?.into_owned()
        };
        let width = total.chars().count();
        let completed = format_obj(&completed, &format!("{width}d"))?;
        let separator = util::to_str(&slf.getattr("separator")?)?;
        text_obj(
            slf.py(),
            &format!("{completed}{separator}{total}"),
            "progress.download",
        )
    }
}

/// `rich.progress.DownloadColumn(binary_units=False, table_column=None)`.
#[pyclass(name = "DownloadColumn", module = "rs_rich.progress", extends = ProgressColumn, subclass, frozen)]
pub(crate) struct DownloadColumn;

#[pymethods]
impl DownloadColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<DownloadColumn> {
        base_init().add_subclass(DownloadColumn)
    }

    #[pyo3(signature = (binary_units=false, table_column=None))]
    fn __init__(
        slf: &Bound<'_, Self>,
        binary_units: bool,
        table_column: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        slf.setattr("binary_units", binary_units)?;
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        const DECIMAL: &[&str] = &["bytes", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
        const BINARY: &[&str] = &[
            "bytes", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB",
        ];
        let py = slf.py();
        let completed = py_int(&task.getattr("completed")?)?;
        let total = task.getattr("total")?;
        let total = if total.is_none() {
            None
        } else {
            Some(py_int(&total)?)
        };
        let base_size = total.as_ref().unwrap_or(&completed);
        let (unit, suffix) = if slf.getattr("binary_units")?.is_truthy()? {
            pick_unit_and_suffix(base_size, BINARY, 1024)?
        } else {
            pick_unit_and_suffix(base_size, DECIMAL, 1000)?
        };
        let spec = if unit.eq(1)? { ",.0f" } else { ",.1f" };
        let completed_str = format_obj(&completed.div(&unit)?, spec)?;
        let total_str = match total {
            Some(total) => format_obj(&total.div(&unit)?, spec)?,
            None => "?".to_string(),
        };
        text_obj(
            py,
            &format!("{completed_str}/{total_str} {suffix}"),
            "progress.download",
        )
    }
}

/// `rich.progress.TimeRemainingColumn(compact=False,
/// elapsed_when_finished=False, table_column=None)`.
#[pyclass(name = "TimeRemainingColumn", module = "rs_rich.progress", extends = ProgressColumn, subclass, frozen)]
pub(crate) struct TimeRemainingColumn;

#[pymethods]
impl TimeRemainingColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<TimeRemainingColumn> {
        base_init().add_subclass(TimeRemainingColumn)
    }

    #[classattr]
    fn max_refresh() -> f64 {
        0.5
    }

    #[pyo3(signature = (compact=false, elapsed_when_finished=false, table_column=None))]
    fn __init__(
        slf: &Bound<'_, Self>,
        compact: bool,
        elapsed_when_finished: bool,
        table_column: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        slf.setattr("compact", compact)?;
        slf.setattr("elapsed_when_finished", elapsed_when_finished)?;
        init_base(slf.as_any(), table_column, false)
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let (task_time, style) = if slf.getattr("elapsed_when_finished")?.is_truthy()?
            && task.getattr("finished")?.is_truthy()?
        {
            (task.getattr("finished_time")?, "progress.elapsed")
        } else {
            (task.getattr("time_remaining")?, "progress.remaining")
        };
        if task.getattr("total")?.is_none() {
            return text_obj(py, "", style);
        }
        let compact = slf.getattr("compact")?.is_truthy()?;
        if task_time.is_none() {
            return text_obj(py, if compact { "--:--" } else { "-:--:--" }, style);
        }
        let (minutes, seconds) = py_int(&task_time)?
            .divmod(60)?
            .extract::<(Bound<'_, PyAny>, Bound<'_, PyAny>)>()?;
        let (hours, minutes) = minutes
            .divmod(60)?
            .extract::<(Bound<'_, PyAny>, Bound<'_, PyAny>)>()?;
        let (minutes, seconds) = (format_obj(&minutes, "02d")?, format_obj(&seconds, "02d")?);
        let formatted = if compact && !hours.is_truthy()? {
            format!("{minutes}:{seconds}")
        } else {
            format!("{}:{minutes}:{seconds}", hours.str()?)
        };
        text_obj(py, &formatted, style)
    }
}

/// `rich.progress.TaskProgressColumn(text_format="[progress.percentage]{task.percentage:>3.0f}%",
/// text_format_no_percentage="", style="none", justify="left", markup=True,
/// highlighter=None, table_column=None, show_speed=False)`.
#[pyclass(name = "TaskProgressColumn", module = "rs_rich.progress", extends = TextColumn, subclass, frozen)]
pub(crate) struct TaskProgressColumn;

#[pymethods]
impl TaskProgressColumn {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<TaskProgressColumn> {
        base_init()
            .add_subclass(TextColumn)
            .add_subclass(TaskProgressColumn)
    }

    #[pyo3(signature = (
        text_format="[progress.percentage]{task.percentage:>3.0f}%", text_format_no_percentage="",
        style=None, justify="left", markup=true, highlighter=None, table_column=None,
        show_speed=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn __init__(
        slf: &Bound<'_, Self>,
        text_format: &str,
        text_format_no_percentage: &str,
        style: Option<Py<PyAny>>,
        justify: &str,
        markup: bool,
        highlighter: Option<Py<PyAny>>,
        table_column: Option<Py<PyAny>>,
        show_speed: bool,
    ) -> PyResult<()> {
        let py = slf.py();
        slf.setattr("text_format_no_percentage", text_format_no_percentage)?;
        slf.setattr("show_speed", show_speed)?;
        slf.setattr("text_format", text_format)?;
        slf.setattr("justify", justify)?;
        slf.setattr(
            "style",
            style.unwrap_or_else(|| PyString::new(py, "none").into_any().unbind()),
        )?;
        slf.setattr("markup", markup)?;
        slf.setattr("highlighter", highlighter)?;
        init_base(slf.as_any(), table_column, true)
    }

    /// The speed in iterations per second, e.g. `2.5×10³ it/s`.
    #[classmethod]
    fn render_speed(
        _cls: &Bound<'_, PyType>,
        py: Python<'_>,
        speed: Option<f64>,
    ) -> PyResult<Py<PyAny>> {
        let Some(speed) = speed else {
            return text_obj(py, "", "progress.percentage");
        };
        let speed = speed.into_pyobject(py)?.into_any();
        let (unit, suffix) = pick_unit_and_suffix(
            &py_int(&speed)?,
            &["", "×10³", "×10⁶", "×10⁹", "×10¹²"],
            1000,
        )?;
        let data_speed = speed.div(&unit)?;
        text_obj(
            py,
            &format!("{}{suffix} it/s", format_obj(&data_speed, ".1f")?),
            "progress.percentage",
        )
    }

    fn render(slf: &Bound<'_, Self>, task: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let total_none = task.getattr("total")?.is_none();
        if total_none && slf.getattr("show_speed")?.is_truthy()? {
            let speed = finished_speed_or_speed(task)?;
            let speed: Option<f64> = if speed.is_none() {
                None
            } else {
                Some(speed.extract()?)
            };
            return slf
                .call_method1("render_speed", (speed,))
                .map(Bound::unbind);
        }
        let text_format = if total_none {
            slf.getattr("text_format_no_percentage")?
        } else {
            slf.getattr("text_format")?
        };
        format_text(slf.as_any(), &text_format, task)
    }
}

// ---------------------------------------------------------------------------
// The tasks grid

/// A Rich `Column` (or anything with its attributes) as core column options.
fn column_options(
    console: &Bound<'_, PyAny>,
    column: &Bound<'_, PyAny>,
    no_wrap: bool,
) -> PyResult<ColumnOptions> {
    let mut options = ColumnOptions {
        no_wrap,
        ..ColumnOptions::default()
    };
    if column.is_none() {
        return Ok(options);
    }
    let get = |name: &str| -> PyResult<Option<Bound<'_, PyAny>>> {
        Ok(column.getattr_opt(name)?.filter(|value| !value.is_none()))
    };
    if let Some(justify) = get("justify")? {
        options.justify = convert::justify(Some(&justify.extract::<String>()?))?;
    }
    if let Some(overflow) = get("overflow")? {
        options.overflow = convert::overflow(&overflow.extract::<String>()?)?;
    }
    options.width = get("width")?.map(|v| v.extract()).transpose()?;
    options.min_width = get("min_width")?.map(|v| v.extract()).transpose()?;
    options.max_width = get("max_width")?.map(|v| v.extract()).transpose()?;
    options.ratio = get("ratio")?.map(|v| v.extract()).transpose()?;
    if let Some(value) = get("no_wrap")? {
        options.no_wrap = value.is_truthy()?;
    }
    if let Some(style) = get("style")? {
        if style.is_truthy()? {
            let resolved = console.call_method1("get_style", (style,))?;
            options.style = resolved.extract::<PyRef<'_, Style>>()?.inner.clone();
        }
    }
    Ok(options)
}

/// A column's output as a grid cell, as `Table.add_row` takes it.
pub(crate) fn cell(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Cell> {
    if value.is_none() {
        return Ok(Cell::Markup(String::new()));
    }
    if let Some(text) = util::core_text(value) {
        return Ok(Cell::Text(text));
    }
    if util::is_str(value) {
        return Ok(Cell::Markup(util::to_str(value)?));
    }
    if let Ok(bar) = value.extract::<PyRef<'_, ProgressBar>>() {
        return Ok(Cell::Renderable(Arc::new(bar.core(py)?)));
    }
    if renderable::is_renderable(value)? {
        return Ok(Cell::Renderable(PyRenderable::shared(
            value.clone().unbind(),
            Some(false),
        )));
    }
    Err(NotRenderableError::new_err(format!(
        "unable to render {}; a string or other renderable object is required",
        value.get_type().name()?
    )))
}

// ---------------------------------------------------------------------------
// Progress

struct ProgressState {
    columns: Py<PyAny>,
    speed_estimate_period: f64,
    disable: bool,
    expand: bool,
    tasks: Vec<(i64, Py<Task>)>,
    task_index: i64,
    live: Option<Py<Live>>,
    /// The console, until the live display (which holds it) exists.
    console: Py<PyAny>,
    get_time: Option<Py<PyAny>>,
    lock: Py<PyAny>,
}

/// `rich.progress.Progress(*columns, console=None, auto_refresh=True,
/// refresh_per_second=10, speed_estimate_period=30.0, transient=False,
/// redirect_stdout=True, redirect_stderr=True, get_time=None, disable=False,
/// expand=False)`.
#[pyclass(name = "Progress", module = "rs_rich.progress", frozen, subclass)]
pub(crate) struct Progress {
    state: Mutex<Option<ProgressState>>,
}

impl Progress {
    fn with<R>(&self, f: impl FnOnce(&mut ProgressState) -> R) -> PyResult<R> {
        let mut state = lock(&self.state);
        match state.as_mut() {
            Some(state) => Ok(f(state)),
            None => Err(pyo3::exceptions::PyRuntimeError::new_err(
                "Progress.__init__ was not called",
            )),
        }
    }

    fn lock_obj(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.with(|state| state.lock.clone_ref(py))
    }

    fn live_obj<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, Live>> {
        self.with(|state| state.live.as_ref().map(|live| live.clone_ref(py)))?
            .map(|live| live.into_bound(py))
            .ok_or_else(|| {
                pyo3::exceptions::PyRuntimeError::new_err("Progress has no live display")
            })
    }

    fn console_obj<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let (live, console) = self.with(|state| {
            (
                state.live.as_ref().map(|live| live.clone_ref(py)),
                state.console.clone_ref(py),
            )
        })?;
        Ok(match live {
            Some(live) => live.get().console_of(py),
            None => console.into_bound(py),
        })
    }

    fn now<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        slf.getattr("get_time")?.call0()
    }

    fn task(&self, py: Python<'_>, task_id: &Bound<'_, PyAny>) -> PyResult<Py<Task>> {
        let id: Option<i64> = task_id.extract().ok();
        let found = self.with(|state| {
            state
                .tasks
                .iter()
                .find(|(key, _)| Some(*key) == id)
                .map(|(_, task)| task.clone_ref(py))
        })?;
        found.ok_or_else(|| PyKeyError::new_err(task_id.clone().unbind()))
    }

    fn task_list(&self, py: Python<'_>) -> PyResult<Vec<Py<Task>>> {
        self.with(|state| state.tasks.iter().map(|(_, t)| t.clone_ref(py)).collect())
    }
}

#[pymethods]
impl Progress {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Progress {
        Progress {
            state: Mutex::new(None),
        }
    }

    #[pyo3(signature = (
        *columns, console=None, auto_refresh=true, refresh_per_second=10.0,
        speed_estimate_period=30.0, transient=false, redirect_stdout=true, redirect_stderr=true,
        get_time=None, disable=false, expand=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn __init__(
        slf: &Bound<'_, Self>,
        columns: &Bound<'_, PyTuple>,
        console: Option<Bound<'_, PyAny>>,
        auto_refresh: bool,
        refresh_per_second: f64,
        speed_estimate_period: f64,
        transient: bool,
        redirect_stdout: bool,
        redirect_stderr: bool,
        get_time: Option<Py<PyAny>>,
        disable: bool,
        expand: bool,
    ) -> PyResult<()> {
        let py = slf.py();
        if refresh_per_second <= 0.0 {
            return Err(PyAssertionError::new_err("refresh_per_second must be > 0"));
        }
        let columns = if columns.is_empty() {
            slf.get_type().call_method0("get_default_columns")?.unbind()
        } else {
            columns.clone().into_any().unbind()
        };
        let console = util::console_or_global(py, console)?;
        *lock(&slf.get().state) = Some(ProgressState {
            console: console.clone().unbind(),
            columns,
            speed_estimate_period,
            disable,
            expand,
            tasks: Vec::new(),
            task_index: 0,
            live: None,
            get_time: None,
            lock: util::rlock(py)?,
        });
        let kwargs = PyDict::new(py);
        kwargs.set_item("console", &console)?;
        kwargs.set_item("auto_refresh", auto_refresh)?;
        kwargs.set_item("refresh_per_second", refresh_per_second)?;
        kwargs.set_item("transient", transient)?;
        kwargs.set_item("redirect_stdout", redirect_stdout)?;
        kwargs.set_item("redirect_stderr", redirect_stderr)?;
        kwargs.set_item("get_renderable", slf.getattr("get_renderable")?)?;
        let live = py
            .get_type::<Live>()
            .call((), Some(&kwargs))?
            .cast_into::<Live>()?
            .unbind();
        let get_time = match get_time.filter(|g| !g.is_none(py)) {
            Some(get_time) => get_time,
            None => util::console_clock(&console)?.unbind(),
        };
        slf.get().with(|state| {
            state.live = Some(live);
            state.get_time = Some(get_time);
        })
    }

    /// The default columns: description, bar, percentage and time remaining.
    #[classmethod]
    fn get_default_columns<'py>(cls: &Bound<'py, PyType>) -> PyResult<Bound<'py, PyTuple>> {
        let py = cls.py();
        PyTuple::new(
            py,
            [
                py.get_type::<TextColumn>()
                    .call1(("[progress.description]{task.description}",))?,
                py.get_type::<BarColumn>().call0()?,
                py.get_type::<TaskProgressColumn>().call0()?,
                py.get_type::<TimeRemainingColumn>().call0()?,
            ],
        )
    }

    #[getter]
    fn columns(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.with(|state| state.columns.clone_ref(py))
    }

    #[setter]
    fn set_columns(&self, columns: Py<PyAny>) -> PyResult<()> {
        self.with(|state| state.columns = columns)
    }

    #[getter]
    fn speed_estimate_period(&self) -> PyResult<f64> {
        self.with(|state| state.speed_estimate_period)
    }

    #[setter]
    fn set_speed_estimate_period(&self, value: f64) -> PyResult<()> {
        self.with(|state| state.speed_estimate_period = value)
    }

    #[getter]
    fn disable(&self) -> PyResult<bool> {
        self.with(|state| state.disable)
    }

    #[setter]
    fn set_disable(&self, value: bool) -> PyResult<()> {
        self.with(|state| state.disable = value)
    }

    #[getter]
    fn expand(&self) -> PyResult<bool> {
        self.with(|state| state.expand)
    }

    #[setter]
    fn set_expand(&self, value: bool) -> PyResult<()> {
        self.with(|state| state.expand = value)
    }

    #[getter]
    fn live<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, Live>> {
        self.live_obj(py)
    }

    #[getter(get_time)]
    fn time_source(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.with(|state| {
            state
                .get_time
                .as_ref()
                .map_or_else(|| py.None(), |g| g.clone_ref(py))
        })
    }

    #[setter(get_time)]
    fn set_time_source(&self, value: Py<PyAny>) -> PyResult<()> {
        self.with(|state| state.get_time = Some(value))
    }

    #[getter]
    fn _lock(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.lock_obj(py)
    }

    #[getter]
    fn console<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.console_obj(py)
    }

    /// `console.print`.
    #[getter]
    fn print<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.console_obj(py)?.getattr("print")
    }

    /// `console.log`.
    #[getter]
    fn log<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.console_obj(py)?.getattr("log")
    }

    #[getter]
    fn tasks<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let _held = hold(self.lock_obj(py)?.bind(py))?;
        PyList::new(py, self.task_list(py)?)
    }

    #[getter]
    fn task_ids<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let _held = hold(self.lock_obj(py)?.bind(py))?;
        let ids = self.with(|state| state.tasks.iter().map(|(id, _)| *id).collect::<Vec<_>>())?;
        PyList::new(py, ids)
    }

    /// Whether every task has finished.
    #[getter]
    fn finished(&self, py: Python<'_>) -> PyResult<bool> {
        let _held = hold(self.lock_obj(py)?.bind(py))?;
        Ok(self
            .task_list(py)?
            .iter()
            .all(|task| task.get().is_finished(py)))
    }

    fn start(slf: &Bound<'_, Self>) -> PyResult<()> {
        if !slf.get().with(|state| state.disable)? {
            let kwargs = PyDict::new(slf.py());
            kwargs.set_item("refresh", true)?;
            slf.get()
                .live_obj(slf.py())?
                .call_method("start", (), Some(&kwargs))?;
        }
        Ok(())
    }

    fn stop(slf: &Bound<'_, Self>) -> PyResult<()> {
        let py = slf.py();
        if !slf.get().with(|state| state.disable)? {
            slf.get().live_obj(py)?.call_method0("stop")?;
            let console = slf.get().console_obj(py)?;
            if !util::flag(&console, "is_interactive")? {
                console.call_method0("print")?;
            }
        }
        Ok(())
    }

    fn __enter__(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        slf.call_method0("start")?;
        Ok(slf)
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(slf: &Bound<'_, Self>, _args: &Bound<'_, PyTuple>) -> PyResult<()> {
        slf.call_method0("stop")?;
        Ok(())
    }

    /// Track progress over `sequence`, yielding its values.
    #[pyo3(signature = (
        sequence, total=None, completed=None, task_id=None, description=None,
        update_period=0.1
    ))]
    fn track(
        slf: &Bound<'_, Self>,
        sequence: Py<PyAny>,
        total: Option<Py<PyAny>>,
        completed: Option<Py<PyAny>>,
        task_id: Option<Py<PyAny>>,
        description: Option<Py<PyAny>>,
        update_period: f64,
    ) -> PyResult<Track> {
        let py = slf.py();
        let description = described(py, description, "Working...");
        Ok(Track::new(
            slf.clone().into_any().unbind(),
            sequence,
            total.filter(|t| !t.is_none(py)),
            completed.unwrap_or_else(|| int(py, 0)),
            task_id.filter(|t| !t.is_none(py)),
            description,
            update_period,
            None,
        ))
    }

    /// Track reading from a binary file.
    #[pyo3(signature = (file, total=None, *, task_id=None, description=None))]
    fn wrap_file(
        slf: &Bound<'_, Self>,
        file: Py<PyAny>,
        total: Option<Py<PyAny>>,
        task_id: Option<Py<PyAny>>,
        description: Option<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let description = described(py, description, "Reading...");
        let total = total.filter(|t| !t.is_none(py));
        let task_id = task_id.filter(|t| !t.is_none(py));
        let total_bytes = match (&total, &task_id) {
            (Some(total), _) => Some(total.clone_ref(py)),
            (None, Some(task_id)) => {
                let _held = hold(slf.get().lock_obj(py)?.bind(py))?;
                let task = slf.get().task(py, task_id.bind(py))?;
                let total = task.get().total(py);
                (!total.is_none(py)).then_some(total)
            }
            (None, None) => None,
        };
        let Some(total_bytes) = total_bytes else {
            return Err(PyValueError::new_err(
                "unable to get the total number of bytes, please specify 'total'",
            ));
        };
        let task_id = add_or_update(slf, task_id, description, total_bytes)?;
        let reader = glue(py)?.getattr("_Reader")?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("close_handle", false)?;
        Ok(reader.call((file, slf, task_id), Some(&kwargs))?.unbind())
    }

    /// Open a file for reading, tracking the progress.
    #[pyo3(signature = (
        file, mode="r", buffering=-1, encoding=None, errors=None, newline=None, *, total=None,
        task_id=None, description=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn open(
        slf: &Bound<'_, Self>,
        file: Py<PyAny>,
        mode: &str,
        buffering: i64,
        encoding: Option<Py<PyAny>>,
        errors: Option<Py<PyAny>>,
        newline: Option<Py<PyAny>>,
        total: Option<Py<PyAny>>,
        task_id: Option<Py<PyAny>>,
        description: Option<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let description = described(py, description, "Reading...");
        let mut sorted: Vec<char> = mode.chars().collect();
        sorted.sort_unstable();
        let normalized: String = sorted.into_iter().collect();
        if !matches!(normalized.as_str(), "br" | "rt" | "r") {
            return Err(PyValueError::new_err(format!(
                "invalid mode {}",
                PyString::new(py, mode).repr()?
            )));
        }
        let line_buffering = buffering == 1;
        let mut buffering = buffering;
        if normalized == "br" && buffering == 1 {
            py.import("warnings")?.call_method1(
                "warn",
                (
                    "line buffering (buffering=1) isn't supported in binary mode, the default buffer size will be used",
                    py.get_type::<pyo3::exceptions::PyRuntimeWarning>(),
                ),
            )?;
            buffering = -1;
        } else if normalized == "rt" || normalized == "r" {
            if buffering == 0 {
                return Err(PyValueError::new_err("can't have unbuffered text I/O"));
            } else if buffering == 1 {
                buffering = -1;
            }
        }
        let total = match total.filter(|t| !t.is_none(py)) {
            Some(total) => total,
            None => py
                .import("os")?
                .call_method1("stat", (file.bind(py),))?
                .getattr("st_size")?
                .unbind(),
        };
        let task_id = add_or_update(slf, task_id.filter(|t| !t.is_none(py)), description, total)?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("buffering", buffering)?;
        let handle = py
            .import("io")?
            .getattr("open")?
            .call((file, "rb"), Some(&kwargs))?;
        let reader_kwargs = PyDict::new(py);
        reader_kwargs.set_item("close_handle", true)?;
        let reader = glue(py)?
            .getattr("_Reader")?
            .call((handle, slf, task_id), Some(&reader_kwargs))?;
        if mode == "r" || mode == "rt" {
            let kwargs = PyDict::new(py);
            kwargs.set_item("encoding", encoding)?;
            kwargs.set_item("errors", errors)?;
            kwargs.set_item("newline", newline)?;
            kwargs.set_item("line_buffering", line_buffering)?;
            return Ok(py
                .import("io")?
                .getattr("TextIOWrapper")?
                .call((reader,), Some(&kwargs))?
                .unbind());
        }
        Ok(reader.unbind())
    }

    /// Start a task's clock (for a task added with `start=False`).
    fn start_task(slf: &Bound<'_, Self>, task_id: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = slf.py();
        let _held = hold(slf.get().lock_obj(py)?.bind(py))?;
        let task = slf.get().task(py, task_id)?;
        if task.get().st().start_time.is_none(py) {
            let now = Progress::now(slf)?.unbind();
            task.get().st().start_time = now;
        }
        Ok(())
    }

    /// Stop a task's clock, freezing its elapsed time.
    fn stop_task(slf: &Bound<'_, Self>, task_id: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = slf.py();
        let _held = hold(slf.get().lock_obj(py)?.bind(py))?;
        let task = slf.get().task(py, task_id)?;
        let now = Progress::now(slf)?.unbind();
        let mut state = task.get().st();
        if state.start_time.is_none(py) {
            state.start_time = now.clone_ref(py);
        }
        state.stop_time = now;
        Ok(())
    }

    /// Update a task's values (and custom fields).
    #[pyo3(signature = (
        task_id, *, total=None, completed=None, advance=None, description=None, visible=None,
        refresh=false, **fields
    ))]
    #[allow(clippy::too_many_arguments)]
    fn update(
        slf: &Bound<'_, Self>,
        task_id: &Bound<'_, PyAny>,
        total: Option<Py<PyAny>>,
        completed: Option<Py<PyAny>>,
        advance: Option<Py<PyAny>>,
        description: Option<Py<PyAny>>,
        visible: Option<Py<PyAny>>,
        refresh: bool,
        fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let py = slf.py();
        {
            let _held = hold(slf.get().lock_obj(py)?.bind(py))?;
            let task = slf.get().task(py, task_id)?;
            let task = task.get();
            let completed_start = task.completed(py).into_bound(py);
            if let Some(total) = total.filter(|t| !t.is_none(py)) {
                if total.bind(py).ne(task.total(py))? {
                    task.st().total = total;
                    task.reset_progress(py);
                }
            }
            if let Some(advance) = advance.filter(|a| !a.is_none(py)) {
                let sum = task.completed(py).into_bound(py).add(advance)?.unbind();
                task.st().completed = sum;
            }
            if let Some(completed) = completed.filter(|c| !c.is_none(py)) {
                task.st().completed = completed;
            }
            if let Some(description) = description.filter(|d| !d.is_none(py)) {
                task.st().description = description;
            }
            if let Some(visible) = visible.filter(|v| !v.is_none(py)) {
                task.st().visible = visible;
            }
            if let Some(fields) = fields {
                task.fields(py).bind(py).call_method1("update", (fields,))?;
            }
            let update_completed = task.completed(py).into_bound(py).sub(&completed_start)?;
            let current_time = Progress::now(slf)?;
            let period = slf.get().with(|state| state.speed_estimate_period)?;
            let old_sample_time = current_time.sub(period)?;
            let sample = if update_completed.gt(0)? {
                Some((current_time.unbind(), update_completed.unbind()))
            } else {
                None
            };
            task.push_sample(py, &old_sample_time, sample, false)?;
            let total = task.total(py).into_bound(py);
            if !total.is_none()
                && task.completed(py).into_bound(py).ge(&total)?
                && !task.is_finished(py)
            {
                let elapsed = task.elapsed_value(py)?.unbind();
                task.st().finished_time = elapsed;
            }
        }
        if refresh {
            slf.call_method0("refresh")?;
        }
        Ok(())
    }

    /// Reset a task: nothing completed and (by default) its clock restarted.
    #[pyo3(signature = (
        task_id, *, start=true, total=None, completed=None, visible=None, description=None,
        **fields
    ))]
    #[allow(clippy::too_many_arguments)]
    fn reset(
        slf: &Bound<'_, Self>,
        task_id: &Bound<'_, PyAny>,
        start: bool,
        total: Option<Py<PyAny>>,
        completed: Option<Py<PyAny>>,
        visible: Option<Py<PyAny>>,
        description: Option<Py<PyAny>>,
        fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let py = slf.py();
        let current_time = Progress::now(slf)?.unbind();
        {
            let _held = hold(slf.get().lock_obj(py)?.bind(py))?;
            let task = slf.get().task(py, task_id)?;
            let task = task.get();
            task.reset_progress(py);
            let mut state = task.st();
            state.start_time = if start { current_time } else { py.None() };
            if let Some(total) = total.filter(|t| !t.is_none(py)) {
                state.total = total;
            }
            state.completed = completed.unwrap_or_else(|| int(py, 0));
            if let Some(visible) = visible.filter(|v| !v.is_none(py)) {
                state.visible = visible;
            }
            if let Some(fields) = fields.filter(|f| !f.is_empty()) {
                state.fields = fields.clone().into_any().unbind();
            }
            if let Some(description) = description.filter(|d| !d.is_none(py)) {
                state.description = description;
            }
            state.finished_time = py.None();
        }
        slf.call_method0("refresh")?;
        Ok(())
    }

    /// Advance a task by `advance` steps.
    #[pyo3(signature = (task_id, advance=None))]
    fn advance(
        slf: &Bound<'_, Self>,
        task_id: &Bound<'_, PyAny>,
        advance: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let py = slf.py();
        let advance = advance.unwrap_or_else(|| int(py, 1));
        let current_time = Progress::now(slf)?;
        let _held = hold(slf.get().lock_obj(py)?.bind(py))?;
        let task = slf.get().task(py, task_id)?;
        let task = task.get();
        let completed_start = task.completed(py).into_bound(py);
        let completed = completed_start.add(advance)?;
        task.st().completed = completed.clone().unbind();
        let update_completed = completed.sub(&completed_start)?;
        let period = slf.get().with(|state| state.speed_estimate_period)?;
        let old_sample_time = current_time.sub(period)?;
        task.push_sample(
            py,
            &old_sample_time,
            Some((current_time.unbind(), update_completed.unbind())),
            true,
        )?;
        let total = task.total(py).into_bound(py);
        if !total.is_none()
            && task.completed(py).into_bound(py).ge(&total)?
            && !task.is_finished(py)
        {
            let elapsed = task.elapsed_value(py)?.unbind();
            let speed = task.speed_value(py)?.unbind();
            let mut state = task.st();
            state.finished_time = elapsed;
            state.finished_speed = speed;
        }
        Ok(())
    }

    /// Draw the display now (while it runs).
    fn refresh(slf: &Bound<'_, Self>) -> PyResult<()> {
        let py = slf.py();
        if slf.get().with(|state| state.disable)? {
            return Ok(());
        }
        let live = slf.get().live_obj(py)?;
        if live.get().is_started_now() {
            live.call_method0("refresh")?;
        }
        Ok(())
    }

    /// What the display shows: `Group(*self.get_renderables())`.
    fn get_renderable(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let children: Vec<Bound<'_, PyAny>> = slf
            .call_method0("get_renderables")?
            .try_iter()?
            .collect::<PyResult<_>>()?;
        util::group(py, children)
    }

    /// The renderables the display shows (the tasks grid).
    fn get_renderables<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        let py = slf.py();
        let tasks = slf.getattr("tasks")?;
        let table = slf.call_method1("make_tasks_table", (tasks,))?;
        PyList::new(py, [table])?
            .into_any()
            .try_iter()
            .map(Bound::into_any)
    }

    /// The grid of `tasks`: one row per visible task, one column per column.
    fn make_tasks_table(slf: &Bound<'_, Self>, tasks: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let (columns, expand) = slf
            .get()
            .with(|state| (state.columns.clone_ref(py), state.expand))?;
        let console = slf.get().console_obj(py)?;
        let columns: Vec<Bound<'_, PyAny>> =
            columns.bind(py).try_iter()?.collect::<PyResult<_>>()?;
        let mut table = CoreTable::grid().padding(0, 1, 0, 1).expand(expand);
        for column in &columns {
            let options = if util::is_str(column) {
                column_options(&console, &py.None().into_bound(py), true)?
            } else {
                let no_wrap = column
                    .cast::<ProgressColumn>()
                    .map(|base| *lock(&base.get().no_wrap))
                    .unwrap_or(false);
                let table_column = column.call_method0("get_table_column")?;
                column_options(&console, &table_column, no_wrap)?
            };
            table.add_column_with(CoreText::new(""), options);
        }
        for task in tasks.try_iter()? {
            let task = task?;
            if !task.getattr("visible")?.is_truthy()? {
                continue;
            }
            let mut cells = Vec::new();
            for column in &columns {
                let value = if util::is_str(column) {
                    let kwargs = PyDict::new(py);
                    kwargs.set_item("task", &task)?;
                    column.call_method("format", (), Some(&kwargs))?
                } else {
                    column.call1((&task,))?
                };
                cells.push(cell(py, &value)?);
            }
            table.add_row_cells(cells);
        }
        util::core_renderable(py, Arc::new(table))
    }

    fn __rich__(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let _held = hold(slf.get().lock_obj(py)?.bind(py))?;
        Ok(slf.call_method0("get_renderable")?.unbind())
    }

    /// Add a task and return its id.
    #[pyo3(signature = (
        description, start=true, total=Arg::Missing, completed=None, visible=true, **fields
    ))]
    fn add_task(
        slf: &Bound<'_, Self>,
        description: Py<PyAny>,
        start: bool,
        total: Arg,
        completed: Option<Py<PyAny>>,
        visible: bool,
        fields: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<i64> {
        let py = slf.py();
        let new_index = {
            let lock_obj = slf.get().lock_obj(py)?;
            let _held = hold(lock_obj.bind(py))?;
            let index = slf.get().with(|state| state.task_index)?;
            let fields = match fields {
                Some(fields) => fields.copy()?.into_any().unbind(),
                None => PyDict::new(py).into_any().unbind(),
            };
            let task = Task {
                state: Mutex::new(TaskState {
                    id: int(py, index),
                    description,
                    total: total.or_else(|| float(py, 100.0)),
                    completed: completed.unwrap_or_else(|| int(py, 0)),
                    get_time: slf.getattr("get_time")?.unbind(),
                    finished_time: py.None(),
                    visible: pyo3::types::PyBool::new(py, visible)
                        .to_owned()
                        .into_any()
                        .unbind(),
                    fields,
                    start_time: py.None(),
                    stop_time: py.None(),
                    finished_speed: py.None(),
                    samples: VecDeque::new(),
                    lock: lock_obj.clone_ref(py),
                }),
            };
            let task = Py::new(py, task)?;
            slf.get().with(|state| state.tasks.push((index, task)))?;
            if start {
                slf.call_method1("start_task", (index,))?;
            }
            slf.get().with(|state| state.task_index = index + 1)?;
            index
        };
        slf.call_method0("refresh")?;
        Ok(new_index)
    }

    /// Remove a task.
    fn remove_task(slf: &Bound<'_, Self>, task_id: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = slf.py();
        let _held = hold(slf.get().lock_obj(py)?.bind(py))?;
        let id: Option<i64> = task_id.extract().ok();
        let removed = slf.get().with(|state| {
            let index = state.tasks.iter().position(|(key, _)| Some(*key) == id);
            index.map(|index| state.tasks.remove(index))
        })?;
        match removed {
            Some(_) => Ok(()),
            None => Err(PyKeyError::new_err(task_id.clone().unbind())),
        }
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(state) = self.state.try_lock() {
            if let Some(state) = state.as_ref() {
                visit.call(&state.columns)?;
                visit.call(&state.console)?;
                visit.call(&state.lock)?;
                for (_, task) in &state.tasks {
                    visit.call(task)?;
                }
                if let Some(live) = &state.live {
                    visit.call(live)?;
                }
                if let Some(get_time) = &state.get_time {
                    visit.call(get_time)?;
                }
            }
        }
        Ok(())
    }

    fn __clear__(&self) {
        if let Ok(mut state) = self.state.try_lock() {
            if let Some(state) = state.as_mut() {
                state.live = None;
                state.get_time = None;
                state.tasks.clear();
            }
        }
    }
}

/// `add_task(description, total=total)` or `update(task_id, total=total)`.
fn add_or_update(
    slf: &Bound<'_, Progress>,
    task_id: Option<Py<PyAny>>,
    description: Py<PyAny>,
    total: Py<PyAny>,
) -> PyResult<Py<PyAny>> {
    let py = slf.py();
    let kwargs = PyDict::new(py);
    kwargs.set_item("total", total)?;
    match task_id {
        None => Ok(slf
            .call_method("add_task", (description,), Some(&kwargs))?
            .unbind()),
        Some(task_id) => {
            slf.call_method("update", (task_id.bind(py),), Some(&kwargs))?;
            Ok(task_id)
        }
    }
}

/// A `description=` argument, with its default.
fn described(py: Python<'_>, description: Option<Py<PyAny>>, default: &str) -> Py<PyAny> {
    description.unwrap_or_else(|| PyString::new(py, default).into_any().unbind())
}

/// The live area's Python glue module (`_Reader`, `_ReadContext`, ...).
pub(crate) fn glue(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    super::glue_module(py)
}

// ---------------------------------------------------------------------------
// track()

struct TrackState {
    progress: Py<PyAny>,
    sequence: Option<Py<PyAny>>,
    iterator: Option<Py<PyAny>>,
    total: Option<Py<PyAny>>,
    completed: Py<PyAny>,
    task_id: Option<Py<PyAny>>,
    description: Py<PyAny>,
    update_period: f64,
    /// Module-level `track()`: the progress to start and stop around it.
    owned: bool,
    started: bool,
    yielded: bool,
    finished: bool,
    auto: bool,
    /// The update thread's id in the exit registry.
    thread: Option<u64>,
    counter: Arc<AtomicI64>,
}

/// The iterator `Progress.track()` and `track()` return (upstream's
/// generators): values of the sequence, advancing the task as they go.
#[pyclass(name = "_Track", module = "rs_rich.progress", frozen)]
pub(crate) struct Track {
    state: Mutex<TrackState>,
}

impl Track {
    #[allow(clippy::too_many_arguments)]
    fn new(
        progress: Py<PyAny>,
        sequence: Py<PyAny>,
        total: Option<Py<PyAny>>,
        completed: Py<PyAny>,
        task_id: Option<Py<PyAny>>,
        description: Py<PyAny>,
        update_period: f64,
        owned: Option<bool>,
    ) -> Track {
        Track {
            state: Mutex::new(TrackState {
                progress,
                sequence: Some(sequence),
                iterator: None,
                total,
                completed,
                task_id,
                description,
                update_period,
                owned: owned.unwrap_or(false),
                started: false,
                yielded: false,
                finished: false,
                auto: false,
                thread: None,
                counter: Arc::new(AtomicI64::new(0)),
            }),
        }
    }

    fn st(&self) -> MutexGuard<'_, TrackState> {
        lock(&self.state)
    }

    /// The generator's body up to its loop: the task and the update thread.
    fn begin(&self, py: Python<'_>) -> PyResult<()> {
        let (progress, sequence, total, completed, task_id, description, owned) = {
            let state = self.st();
            (
                state.progress.clone_ref(py),
                state.sequence.as_ref().map(|s| s.clone_ref(py)),
                state.total.as_ref().map(|t| t.clone_ref(py)),
                state.completed.clone_ref(py),
                state.task_id.as_ref().map(|t| t.clone_ref(py)),
                state.description.clone_ref(py),
                state.owned,
            )
        };
        let progress = progress.bind(py);
        if owned {
            progress.call_method0("start")?;
        }
        let sequence = sequence.expect("a track has its sequence until it begins");
        let total = match total {
            Some(total) => Some(total),
            None => {
                let hint: f64 = py
                    .import("operator")?
                    .call_method1("length_hint", (sequence.bind(py),))?
                    .extract()?;
                (hint != 0.0).then(|| float(py, hint))
            }
        };
        let kwargs = PyDict::new(py);
        kwargs.set_item("total", total)?;
        kwargs.set_item("completed", completed)?;
        let task_id = match task_id {
            None => progress
                .call_method("add_task", (description,), Some(&kwargs))?
                .unbind(),
            Some(task_id) => {
                progress.call_method("update", (task_id.bind(py),), Some(&kwargs))?;
                task_id
            }
        };
        let auto = progress
            .getattr("live")?
            .getattr("auto_refresh")?
            .is_truthy()?;
        let iterator = sequence.bind(py).try_iter()?.into_any().unbind();
        let thread = if auto {
            Some(self.spawn(py, progress, &task_id)?)
        } else {
            None
        };
        let mut state = self.st();
        state.sequence = None;
        state.task_id = Some(task_id);
        state.iterator = Some(iterator);
        state.auto = auto;
        state.thread = thread;
        Ok(())
    }

    /// Upstream's `_TrackThread`: advance the task from the counter every
    /// `update_period` seconds, then set the final count.
    fn spawn(
        &self,
        py: Python<'_>,
        progress: &Bound<'_, PyAny>,
        task_id: &Py<PyAny>,
    ) -> PyResult<u64> {
        let done = py.import("threading")?.call_method0("Event")?.unbind();
        let (counter, period) = {
            let state = self.st();
            (state.counter.clone(), state.update_period)
        };
        let progress = progress.clone().unbind();
        let task_id = task_id.clone_ref(py);
        let event = done.clone_ref(py);
        let target = PyCFunction::new_closure(
            py,
            None,
            None,
            move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
                let py = args.py();
                let progress = progress.bind(py);
                let task_id = task_id.bind(py);
                let mut last = 0i64;
                loop {
                    if event
                        .bind(py)
                        .call_method1("wait", (period,))?
                        .is_truthy()?
                    {
                        break;
                    }
                    if !progress
                        .getattr("live")?
                        .getattr("is_started")?
                        .is_truthy()?
                    {
                        break;
                    }
                    let completed = counter.load(Ordering::SeqCst);
                    if completed != last {
                        progress.call_method1("advance", (task_id, completed - last))?;
                        last = completed;
                    }
                }
                let kwargs = PyDict::new(py);
                kwargs.set_item("completed", counter.load(Ordering::SeqCst))?;
                kwargs.set_item("refresh", true)?;
                progress.call_method("update", (task_id,), Some(&kwargs))?;
                Ok(())
            },
        )?;
        live_display::start_worker(py, target, &done)
    }

    /// Leave the loop: stop the update thread (which sets the final count),
    /// then stop an owned progress.
    fn finish(&self, py: Python<'_>) -> PyResult<()> {
        let (thread, progress, owned, started) = {
            let mut state = self.st();
            if state.finished {
                return Ok(());
            }
            state.finished = true;
            (
                state.thread.take(),
                state.progress.clone_ref(py),
                state.owned,
                state.started,
            )
        };
        let mut result = Ok(());
        if let Some(id) = thread {
            result = live_display::stop_worker(py, id);
        }
        if owned && started {
            let stopped = progress.bind(py).call_method0("stop").map(|_| ());
            result = result.and(stopped);
        }
        result
    }
}

#[pymethods]
impl Track {
    fn __iter__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }

    fn __next__(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let (started, finished) = {
            let state = self.st();
            (state.started, state.finished)
        };
        if finished {
            return Ok(None);
        }
        if !started {
            self.st().started = true;
            if let Err(error) = self.begin(py) {
                let _ = self.finish(py);
                return Err(error);
            }
        }
        let (yielded, auto, progress, task_id, iterator, counter) = {
            let state = self.st();
            (
                state.yielded,
                state.auto,
                state.progress.clone_ref(py),
                state.task_id.as_ref().map(|t| t.clone_ref(py)),
                state.iterator.as_ref().map(|i| i.clone_ref(py)),
                state.counter.clone(),
            )
        };
        if yielded {
            if auto {
                counter.fetch_add(1, Ordering::SeqCst);
            } else if let Some(task_id) = &task_id {
                let progress = progress.bind(py);
                progress.call_method1("advance", (task_id.bind(py), 1))?;
                progress.call_method0("refresh")?;
            }
        }
        let Some(iterator) = iterator else {
            return Ok(None);
        };
        let next = py
            .import("builtins")?
            .getattr("next")?
            .call1((iterator.bind(py), Sentinel::get(py)?));
        match next {
            Ok(value) if value.is(Sentinel::get(py)?) => {
                self.finish(py)?;
                Ok(None)
            }
            Ok(value) => {
                self.st().yielded = true;
                Ok(Some(value.unbind()))
            }
            Err(error) => {
                let _ = self.finish(py);
                Err(error)
            }
        }
    }

    /// Stop early, as closing upstream's generator does.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let started = self.st().started;
        if started {
            self.finish(py)
        } else {
            self.st().finished = true;
            Ok(())
        }
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(state) = self.state.try_lock() {
            visit.call(&state.progress)?;
            for object in [
                &state.sequence,
                &state.iterator,
                &state.total,
                &state.task_id,
            ]
            .into_iter()
            .flatten()
            {
                visit.call(object)?;
            }
        }
        Ok(())
    }
}

impl Drop for Track {
    fn drop(&mut self) {
        let pending = {
            let state = lock(&self.state);
            state.started && !state.finished
        };
        if pending {
            Python::attach(|py| {
                if let Err(error) = self.finish(py) {
                    error.write_unraisable(py, None);
                }
            });
        }
    }
}

/// A unique object marking the end of an iterator.
struct Sentinel;

impl Sentinel {
    fn get(py: Python<'_>) -> PyResult<&Bound<'_, PyAny>> {
        static SENTINEL: pyo3::sync::PyOnceLock<Py<PyAny>> = pyo3::sync::PyOnceLock::new();
        SENTINEL
            .get_or_try_init(py, || {
                Ok::<_, PyErr>(py.import("builtins")?.getattr("object")?.call0()?.unbind())
            })
            .map(|sentinel| sentinel.bind(py))
    }
}

/// The columns of module-level `track()`, `wrap_file()` and `open()`.
#[allow(clippy::too_many_arguments)]
fn helper_columns<'py>(
    py: Python<'py>,
    description: &Bound<'py, PyAny>,
    style: Option<Py<PyAny>>,
    complete_style: Option<Py<PyAny>>,
    finished_style: Option<Py<PyAny>>,
    pulse_style: Option<Py<PyAny>>,
    tail: Vec<Bound<'py, PyAny>>,
) -> PyResult<Vec<Bound<'py, PyAny>>> {
    let mut columns = Vec::new();
    if description.is_truthy()? {
        columns.push(
            py.get_type::<TextColumn>()
                .call1(("[progress.description]{task.description}",))?,
        );
    }
    let kwargs = PyDict::new(py);
    if let Some(style) = style {
        kwargs.set_item("style", style)?;
    }
    if let Some(style) = complete_style {
        kwargs.set_item("complete_style", style)?;
    }
    if let Some(style) = finished_style {
        kwargs.set_item("finished_style", style)?;
    }
    if let Some(style) = pulse_style {
        kwargs.set_item("pulse_style", style)?;
    }
    columns.push(py.get_type::<BarColumn>().call((), Some(&kwargs))?);
    columns.extend(tail);
    Ok(columns)
}

#[allow(clippy::too_many_arguments)]
fn helper_progress<'py>(
    py: Python<'py>,
    columns: Vec<Bound<'py, PyAny>>,
    auto_refresh: bool,
    console: Option<Bound<'py, PyAny>>,
    transient: bool,
    get_time: Option<Py<PyAny>>,
    refresh_per_second: f64,
    disable: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let kwargs = PyDict::new(py);
    kwargs.set_item("auto_refresh", auto_refresh)?;
    kwargs.set_item("console", console)?;
    kwargs.set_item("transient", transient)?;
    kwargs.set_item("get_time", get_time)?;
    kwargs.set_item(
        "refresh_per_second",
        if refresh_per_second == 0.0 {
            10.0
        } else {
            refresh_per_second
        },
    )?;
    kwargs.set_item("disable", disable)?;
    py.get_type::<Progress>()
        .call(PyTuple::new(py, columns)?, Some(&kwargs))
}

/// `rich.progress.track`: iterate over `sequence` with a progress display.
#[pyfunction]
#[pyo3(signature = (
    sequence, description=None, total=None, completed=None, auto_refresh=true,
    console=None, transient=false, get_time=None, refresh_per_second=10.0, style=None,
    complete_style=None, finished_style=None, pulse_style=None, update_period=0.1,
    disable=false, show_speed=true
))]
#[allow(clippy::too_many_arguments)]
fn track(
    py: Python<'_>,
    sequence: Py<PyAny>,
    description: Option<Py<PyAny>>,
    total: Option<Py<PyAny>>,
    completed: Option<Py<PyAny>>,
    auto_refresh: bool,
    console: Option<Bound<'_, PyAny>>,
    transient: bool,
    get_time: Option<Py<PyAny>>,
    refresh_per_second: f64,
    style: Option<Py<PyAny>>,
    complete_style: Option<Py<PyAny>>,
    finished_style: Option<Py<PyAny>>,
    pulse_style: Option<Py<PyAny>>,
    update_period: f64,
    disable: bool,
    show_speed: bool,
) -> PyResult<Track> {
    let description = described(py, description, "Working...").into_bound(py);
    let task_progress = {
        let kwargs = PyDict::new(py);
        kwargs.set_item("show_speed", show_speed)?;
        py.get_type::<TaskProgressColumn>()
            .call((), Some(&kwargs))?
    };
    let remaining = {
        let kwargs = PyDict::new(py);
        kwargs.set_item("elapsed_when_finished", true)?;
        py.get_type::<TimeRemainingColumn>()
            .call((), Some(&kwargs))?
    };
    let columns = helper_columns(
        py,
        &description,
        style,
        complete_style,
        finished_style,
        pulse_style,
        vec![task_progress, remaining],
    )?;
    let progress = helper_progress(
        py,
        columns,
        auto_refresh,
        console,
        transient,
        get_time,
        refresh_per_second,
        disable,
    )?;
    Ok(Track::new(
        progress.unbind(),
        sequence,
        total.filter(|t| !t.is_none(py)),
        completed.unwrap_or_else(|| int(py, 0)),
        None,
        description.unbind(),
        update_period,
        Some(true),
    ))
}

/// `rich.progress.wrap_file`: read bytes from a file while tracking progress.
#[pyfunction]
#[pyo3(signature = (
    file, total, *, description=None, auto_refresh=true, console=None,
    transient=false, get_time=None, refresh_per_second=10.0, style=None, complete_style=None,
    finished_style=None, pulse_style=None, disable=false
))]
#[allow(clippy::too_many_arguments)]
fn wrap_file(
    py: Python<'_>,
    file: Py<PyAny>,
    total: Py<PyAny>,
    description: Option<Py<PyAny>>,
    auto_refresh: bool,
    console: Option<Bound<'_, PyAny>>,
    transient: bool,
    get_time: Option<Py<PyAny>>,
    refresh_per_second: f64,
    style: Option<Py<PyAny>>,
    complete_style: Option<Py<PyAny>>,
    finished_style: Option<Py<PyAny>>,
    pulse_style: Option<Py<PyAny>>,
    disable: bool,
) -> PyResult<Py<PyAny>> {
    let description = described(py, description, "Reading...").into_bound(py);
    let tail = vec![
        py.get_type::<DownloadColumn>().call0()?,
        py.get_type::<TimeRemainingColumn>().call0()?,
    ];
    let columns = helper_columns(
        py,
        &description,
        style,
        complete_style,
        finished_style,
        pulse_style,
        tail,
    )?;
    let progress = helper_progress(
        py,
        columns,
        auto_refresh,
        console,
        transient,
        get_time,
        refresh_per_second,
        disable,
    )?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("total", total)?;
    kwargs.set_item("description", &description)?;
    let reader = progress.call_method("wrap_file", (file,), Some(&kwargs))?;
    Ok(glue(py)?
        .getattr("_ReadContext")?
        .call1((progress, reader))?
        .unbind())
}

/// `rich.progress.open`: open a file for reading while tracking progress.
#[pyfunction]
#[pyo3(signature = (
    file, mode="r", buffering=-1, encoding=None, errors=None, newline=None, *, total=None,
    description=None, auto_refresh=true, console=None, transient=false, get_time=None,
    refresh_per_second=10.0, style=None, complete_style=None, finished_style=None,
    pulse_style=None, disable=false
))]
#[allow(clippy::too_many_arguments)]
fn open(
    py: Python<'_>,
    file: Py<PyAny>,
    mode: &str,
    buffering: i64,
    encoding: Option<Py<PyAny>>,
    errors: Option<Py<PyAny>>,
    newline: Option<Py<PyAny>>,
    total: Option<Py<PyAny>>,
    description: Option<Py<PyAny>>,
    auto_refresh: bool,
    console: Option<Bound<'_, PyAny>>,
    transient: bool,
    get_time: Option<Py<PyAny>>,
    refresh_per_second: f64,
    style: Option<Py<PyAny>>,
    complete_style: Option<Py<PyAny>>,
    finished_style: Option<Py<PyAny>>,
    pulse_style: Option<Py<PyAny>>,
    disable: bool,
) -> PyResult<Py<PyAny>> {
    let description = described(py, description, "Reading...").into_bound(py);
    let tail = vec![
        py.get_type::<DownloadColumn>().call0()?,
        py.get_type::<TimeRemainingColumn>().call0()?,
    ];
    let columns = helper_columns(
        py,
        &description,
        style,
        complete_style,
        finished_style,
        pulse_style,
        tail,
    )?;
    let progress = helper_progress(
        py,
        columns,
        auto_refresh,
        console,
        transient,
        get_time,
        refresh_per_second,
        disable,
    )?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("mode", mode)?;
    kwargs.set_item("buffering", buffering)?;
    kwargs.set_item("encoding", encoding)?;
    kwargs.set_item("errors", errors)?;
    kwargs.set_item("newline", newline)?;
    kwargs.set_item("total", total)?;
    kwargs.set_item("description", &description)?;
    let reader = progress.call_method("open", (file,), Some(&kwargs))?;
    Ok(glue(py)?
        .getattr("_ReadContext")?
        .call1((progress, reader))?
        .unbind())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add_class::<Task>()?;
    m.add_class::<ProgressColumn>()?;
    m.add_class::<RenderableColumn>()?;
    m.add_class::<SpinnerColumn>()?;
    m.add_class::<TextColumn>()?;
    m.add_class::<BarColumn>()?;
    m.add_class::<TimeElapsedColumn>()?;
    m.add_class::<TaskProgressColumn>()?;
    m.add_class::<TimeRemainingColumn>()?;
    m.add_class::<FileSizeColumn>()?;
    m.add_class::<TotalFileSizeColumn>()?;
    m.add_class::<MofNCompleteColumn>()?;
    m.add_class::<DownloadColumn>()?;
    m.add_class::<TransferSpeedColumn>()?;
    m.add_class::<Progress>()?;
    m.add_class::<Track>()?;
    m.add_function(pyo3::wrap_pyfunction!(track, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(wrap_file, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(open, m)?)?;
    // `TaskID = NewType("TaskID", int)`.
    let task_id = py
        .import("typing")?
        .getattr("NewType")?
        .call1(("TaskID", py.get_type::<pyo3::types::PyInt>()))?;
    task_id.setattr("__module__", "rs_rich.progress")?;
    m.add("TaskID", task_id)?;
    Ok(())
}
