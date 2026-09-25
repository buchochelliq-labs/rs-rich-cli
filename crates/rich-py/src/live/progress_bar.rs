//! `rich.progress_bar.ProgressBar`: the bar `BarColumn` draws, rendered by
//! core's `ProgressBar`.
//!
//! A protocol object (`__rich_console__`, `__rich_measure__`) like
//! upstream's, so a print of a bar ends where the bar does.

use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};
use pyo3::{PyTraverseError, PyVisit};

use rich::ProgressBar as CoreBar;

use super::util::{self, Arg};
use crate::protocol::Measurement;
use crate::segment::Segment;
use crate::style::style_type;

struct BarState {
    total: Py<PyAny>,
    completed: Py<PyAny>,
    width: Option<usize>,
    pulse: bool,
    style: Py<PyAny>,
    complete_style: Py<PyAny>,
    finished_style: Py<PyAny>,
    pulse_style: Py<PyAny>,
    animation_time: Option<f64>,
}

/// `rich.progress_bar.ProgressBar(total=100.0, completed=0, width=None,
/// pulse=False, style="bar.back", complete_style="bar.complete",
/// finished_style="bar.finished", pulse_style="bar.pulse", animation_time=None)`.
#[pyclass(name = "ProgressBar", module = "rs_rich.progress_bar", frozen)]
pub(crate) struct ProgressBar {
    state: Mutex<BarState>,
}

fn number(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    value.extract::<f64>()
}

impl ProgressBar {
    fn st(&self) -> MutexGuard<'_, BarState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build(
        py: Python<'_>,
        total: Option<Py<PyAny>>,
        completed: Py<PyAny>,
        width: Option<usize>,
        pulse: bool,
        style: Py<PyAny>,
        complete_style: Py<PyAny>,
        finished_style: Py<PyAny>,
        pulse_style: Py<PyAny>,
        animation_time: Option<f64>,
    ) -> ProgressBar {
        ProgressBar {
            state: Mutex::new(BarState {
                total: total.unwrap_or_else(|| py.None()),
                completed,
                width,
                pulse,
                style,
                complete_style,
                finished_style,
                pulse_style,
                animation_time,
            }),
        }
    }

    /// The core bar for the current values.
    pub(crate) fn core(&self, py: Python<'_>) -> PyResult<CoreBar> {
        let state = self.st();
        let total = state.total.bind(py);
        let completed = number(state.completed.bind(py))?;
        let mut bar = if total.is_none() {
            CoreBar::indeterminate()
        } else {
            CoreBar::new(number(total)?, completed)
        };
        if let Some(width) = state.width.filter(|w| *w > 0) {
            bar = bar.width(width);
        }
        bar = bar.pulse(state.pulse || total.is_none());
        if let Some(time) = state.animation_time {
            bar = bar.animation_time(time);
        }
        let style_of = |value: &Py<PyAny>, default: &str| -> PyResult<rich::StyleType> {
            Ok(style_type(Some(value.bind(py)))?.unwrap_or_else(|| default.into()))
        };
        Ok(bar
            .style(style_of(&state.style, "bar.back")?)
            .complete_style(style_of(&state.complete_style, "bar.complete")?)
            .finished_style(style_of(&state.finished_style, "bar.finished")?)
            .pulse_style(style_of(&state.pulse_style, "bar.pulse")?))
    }
}

fn default_style(py: Python<'_>, name: &str) -> Py<PyAny> {
    PyString::new(py, name).into_any().unbind()
}

#[pymethods]
impl ProgressBar {
    #[new]
    #[pyo3(signature = (
        total=Arg::Missing, completed=None,
        width=None, pulse=false, style=None, complete_style=None, finished_style=None,
        pulse_style=None, animation_time=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        total: Arg,
        completed: Option<Py<PyAny>>,
        width: Option<usize>,
        pulse: bool,
        style: Option<Py<PyAny>>,
        complete_style: Option<Py<PyAny>>,
        finished_style: Option<Py<PyAny>>,
        pulse_style: Option<Py<PyAny>>,
        animation_time: Option<f64>,
    ) -> ProgressBar {
        let total = total.or_else(|| 100.0f64.into_pyobject(py).unwrap().into_any().unbind());
        ProgressBar::build(
            py,
            Some(total),
            completed.unwrap_or_else(|| 0i64.into_pyobject(py).unwrap().into_any().unbind()),
            width,
            pulse,
            style.unwrap_or_else(|| default_style(py, "bar.back")),
            complete_style.unwrap_or_else(|| default_style(py, "bar.complete")),
            finished_style.unwrap_or_else(|| default_style(py, "bar.finished")),
            pulse_style.unwrap_or_else(|| default_style(py, "bar.pulse")),
            animation_time,
        )
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let state = self.st();
        Ok(format!(
            "<Bar {} of {}>",
            state.completed.bind(py).repr()?,
            state.total.bind(py).repr()?
        ))
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
    fn width(&self) -> Option<usize> {
        self.st().width
    }

    #[setter]
    fn set_width(&self, value: Option<usize>) {
        self.st().width = value;
    }

    #[getter]
    fn pulse(&self) -> bool {
        self.st().pulse
    }

    #[setter]
    fn set_pulse(&self, value: bool) {
        self.st().pulse = value;
    }

    #[getter]
    fn animation_time(&self) -> Option<f64> {
        self.st().animation_time
    }

    #[setter]
    fn set_animation_time(&self, value: Option<f64>) {
        self.st().animation_time = value;
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().style.clone_ref(py)
    }

    #[setter]
    fn set_style(&self, value: Py<PyAny>) {
        self.st().style = value;
    }

    #[getter]
    fn complete_style(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().complete_style.clone_ref(py)
    }

    #[setter]
    fn set_complete_style(&self, value: Py<PyAny>) {
        self.st().complete_style = value;
    }

    #[getter]
    fn finished_style(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().finished_style.clone_ref(py)
    }

    #[setter]
    fn set_finished_style(&self, value: Py<PyAny>) {
        self.st().finished_style = value;
    }

    #[getter]
    fn pulse_style(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().pulse_style.clone_ref(py)
    }

    #[setter]
    fn set_pulse_style(&self, value: Py<PyAny>) {
        self.st().pulse_style = value;
    }

    /// The percentage completed, or `None` without a total.
    #[getter]
    fn percentage_completed(&self, py: Python<'_>) -> PyResult<Option<f64>> {
        let state = self.st();
        let total = state.total.bind(py);
        if total.is_none() {
            return Ok(None);
        }
        let completed = number(state.completed.bind(py))? / number(total)? * 100.0;
        Ok(Some(completed.clamp(0.0, 100.0)))
    }

    /// Set `completed` (and `total`, unless `None`).
    #[pyo3(signature = (completed, total=None))]
    fn update(&self, py: Python<'_>, completed: Py<PyAny>, total: Option<Py<PyAny>>) {
        let mut state = self.st();
        state.completed = completed;
        if let Some(total) = total.filter(|t| !t.is_none(py)) {
            state.total = total;
        }
    }

    fn __rich_console__<'py>(
        &self,
        console: &Bound<'py, PyAny>,
        options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = console.py();
        let bar = self.core(py)?;
        let segments = util::render_core(console, options, Arc::new(bar))?;
        PyList::new(
            py,
            segments.iter().map(|segment| Segment::from_core(py, segment)),
        )
    }

    fn __rich_measure__(&self, _console: &Bound<'_, PyAny>, options: &Bound<'_, PyAny>) -> PyResult<Measurement> {
        let max_width: usize = options.getattr("max_width")?.extract()?;
        let width = self.st().width;
        Ok(Measurement::from_core(match width {
            Some(width) => rich::measure::Measurement::new(width, width),
            None => rich::measure::Measurement::new(4, max_width),
        }))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(state) = self.state.try_lock() {
            for object in [
                &state.total,
                &state.completed,
                &state.style,
                &state.complete_style,
                &state.finished_style,
                &state.pulse_style,
            ] {
                visit.call(object)?;
            }
        }
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ProgressBar>()
}
