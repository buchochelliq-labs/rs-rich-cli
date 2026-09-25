//! `rich.spinner.Spinner` and the `SPINNERS` table. Port of upstream
//! `rich/spinner.py`.
//!
//! Core's `Spinner` takes only markup text and keeps its table private, so
//! the animation is ported here over the vendored table
//! (`spinner_data.rs`). A spinner renders the frame for its console's
//! `get_time()`, read from the Python console so `Console(get_time=...)`
//! drives it as in Rich.

use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::exceptions::{PyKeyError, PyZeroDivisionError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use pyo3::{PyTraverseError, PyVisit};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::table::{Cell, Table as CoreTable};
use rich::Text as CoreText;

use crate::errors::MarkupError;
use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::style::style_type;
use crate::text::Text;

use super::spinner_data::{spinner_data, NAMES};

/// What `Spinner.render(time)` returns: the frame, or the frame beside the
/// spinner's renderable text.
enum Frame {
    Text(CoreText),
    Grid(CoreText, Py<PyAny>),
}

impl Frame {
    fn renderable(self) -> Box<dyn Renderable> {
        match self {
            Frame::Text(text) => Box::new(text),
            Frame::Grid(frame, text) => Box::new(grid(frame, text)),
        }
    }
}

/// `Table.grid(padding=1)` with one row: the frame and the renderable.
fn grid(frame: CoreText, text: Py<PyAny>) -> CoreTable {
    let mut table = CoreTable::grid().padding(1, 1, 1, 1);
    table.add_column("");
    table.add_column("");
    table.add_row_cells(vec![
        Cell::Text(frame),
        Cell::Renderable(PyRenderable::shared(text, Some(false))),
    ]);
    table
}

/// A spinner's state, shared between the Python object and its renders
/// (a render must advance the object's own clock, as Rich's does).
struct State {
    text: Py<PyAny>,
    frames: Vec<String>,
    interval: f64,
    start_time: Option<f64>,
    style: Option<Py<PyAny>>,
    speed: f64,
    frame_no_offset: f64,
    update_speed: f64,
}

type Shared = Arc<Mutex<State>>;

fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `Text.from_markup(text) if isinstance(text, str) else text`.
fn text_arg(text: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    match text.cast::<PyString>() {
        Ok(markup) => {
            let inner = CoreText::from_markup(markup.to_cow()?.as_ref())
                .map_err(|e| MarkupError::new_err(e.to_string()))?;
            Ok(Py::new(text.py(), Text { inner })?.into_any())
        }
        Err(_) => Ok(text.clone().unbind()),
    }
}

/// `Spinner.render(time)`.
fn frame(py: Python<'_>, state: &Shared, time: f64) -> PyResult<Frame> {
    // Update the clock under the lock; touch Python objects after it.
    let (frame, style, text) = {
        let mut state = lock(state);
        let start = *state.start_time.get_or_insert(time);
        let frame_no =
            (time - start) * state.speed / (state.interval / 1000.0) + state.frame_no_offset;
        let count = state.frames.len() as i64;
        if count == 0 {
            return Err(PyZeroDivisionError::new_err("integer modulo by zero"));
        }
        let index = (frame_no.trunc() as i64).rem_euclid(count) as usize;
        if state.update_speed != 0.0 {
            state.frame_no_offset = frame_no;
            state.start_time = Some(time);
            state.speed = state.update_speed;
            state.update_speed = 0.0;
        }
        (
            state.frames[index].clone(),
            state.style.as_ref().map(|style| style.clone_ref(py)),
            state.text.clone_ref(py),
        )
    };
    let style = match &style {
        Some(style) => style_type(Some(style.bind(py)))?,
        None => None,
    };
    let frame = match style {
        Some(style) => CoreText::styled(frame, style),
        None => CoreText::new(frame),
    };
    let text = text.bind(py);
    if !text.is_truthy()? {
        return Ok(Frame::Text(frame));
    }
    if let Ok(text) = text.extract::<PyRef<'_, Text>>() {
        // `Text.assemble(frame, " ", text)`.
        let mut assembled = CoreText::new("").append_text(&frame);
        assembled.append(" ", None);
        return Ok(Frame::Text(assembled.append_text(&text.inner)));
    }
    Ok(Frame::Grid(frame, text.clone().unbind()))
}

/// A spinner inside a render: it animates from the console's clock.
struct SpinnerRender {
    state: Shared,
}

impl SpinnerRender {
    fn at(
        &self,
        py: Python<'_>,
        console: &CoreConsole,
        time: Option<f64>,
    ) -> PyResult<Box<dyn Renderable>> {
        let time = match time {
            Some(time) => time,
            // `console.get_time()`, from the Python console when there is one.
            None => {
                let clock = match renderable::ambient() {
                    Ok(ambient) => ambient.console.bind(py).getattr_opt("get_time")?,
                    Err(_) => None,
                };
                match clock {
                    Some(clock) => clock.call0()?.extract()?,
                    None => console.get_time(),
                }
            }
        };
        Ok(frame(py, &self.state, time)?.renderable())
    }
}

impl Renderable for SpinnerRender {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        Python::attach(|py| match self.at(py, console, None) {
            Ok(frame) => frame.rich_render(console, options),
            Err(error) => {
                error.write_unraisable(py, None);
                Vec::new()
            }
        })
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        Python::attach(|py| match self.at(py, console, Some(0.0)) {
            Ok(frame) => CoreMeasurement::get(console, options, frame.as_ref()),
            Err(error) => {
                error.write_unraisable(py, None);
                CoreMeasurement::new(0, 0)
            }
        })
    }
}

/// `rich.spinner.Spinner`: an animation frame for a point in time.
#[pyclass(name = "Spinner", module = "rs_rich.spinner", frozen)]
pub(crate) struct Spinner {
    #[pyo3(get)]
    name: String,
    state: Shared,
}

impl AsRenderable for Spinner {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(SpinnerRender {
            state: self.state.clone(),
        }))
    }
}

#[pymethods]
impl Spinner {
    #[new]
    #[pyo3(signature = (name, text=None, *, style=None, speed=1.0))]
    fn new(
        py: Python<'_>,
        name: &str,
        text: Option<&Bound<'_, PyAny>>,
        style: Option<Py<PyAny>>,
        speed: f64,
    ) -> PyResult<Self> {
        let Some((interval, frames)) = spinner_data(name) else {
            return Err(PyKeyError::new_err(format!(
                "no spinner called {}",
                PyString::new(py, name).repr()?
            )));
        };
        let empty = PyString::new(py, "").into_any();
        let style = style.filter(|style| !style.is_none(py));
        if let Some(style) = &style {
            style_type(Some(style.bind(py)))?;
        }
        Ok(Spinner {
            name: name.to_string(),
            state: Arc::new(Mutex::new(State {
                text: text_arg(text.unwrap_or(&empty))?,
                frames: frames.iter().map(|frame| frame.to_string()).collect(),
                interval,
                start_time: None,
                style,
                speed,
                frame_no_offset: 0.0,
                update_speed: 0.0,
            })),
        })
    }

    /// The frame (and text) to show at `time` seconds: a `Text`, or a
    /// renderable grid when the text is another renderable.
    fn render(&self, py: Python<'_>, time: f64) -> PyResult<Py<PyAny>> {
        Ok(match frame(py, &self.state, time)? {
            Frame::Text(inner) => Py::new(py, Text { inner })?.into_any(),
            Frame::Grid(frame, text) => Py::new(py, SpinnerGrid { frame, text })?.into_any(),
        })
    }

    /// Change the text, style or speed of a running spinner.
    #[pyo3(signature = (*, text=None, style=None, speed=None))]
    fn update(
        &self,
        py: Python<'_>,
        text: Option<&Bound<'_, PyAny>>,
        style: Option<Py<PyAny>>,
        speed: Option<f64>,
    ) -> PyResult<()> {
        let text = match text {
            Some(text) if text.is_truthy()? => Some(text_arg(text)?),
            _ => None,
        };
        let style = match style {
            Some(style) if style.bind(py).is_truthy()? => {
                style_type(Some(style.bind(py)))?;
                Some(style)
            }
            _ => None,
        };
        // The replaced objects are dropped after the lock is released: a
        // drop can run Python code, which may come back to this spinner.
        let _replaced = {
            let mut state = lock(&self.state);
            if let Some(speed) = speed.filter(|speed| *speed != 0.0) {
                state.update_speed = speed;
            }
            (
                text.map(|text| std::mem::replace(&mut state.text, text)),
                style.map(|style| state.style.replace(style)),
            )
        };
        Ok(())
    }

    #[getter]
    fn text(&self, py: Python<'_>) -> Py<PyAny> {
        lock(&self.state).text.clone_ref(py)
    }

    #[setter]
    fn set_text(&self, text: Py<PyAny>) {
        let _replaced = std::mem::replace(&mut lock(&self.state).text, text);
    }

    #[getter]
    fn frames(&self) -> Vec<String> {
        lock(&self.state).frames.clone()
    }

    #[setter]
    fn set_frames(&self, frames: Vec<String>) {
        lock(&self.state).frames = frames;
    }

    #[getter]
    fn interval(&self) -> f64 {
        lock(&self.state).interval
    }

    #[setter]
    fn set_interval(&self, interval: f64) {
        lock(&self.state).interval = interval;
    }

    #[getter]
    fn start_time(&self) -> Option<f64> {
        lock(&self.state).start_time
    }

    #[setter]
    fn set_start_time(&self, start_time: Option<f64>) {
        lock(&self.state).start_time = start_time;
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        lock(&self.state)
            .style
            .as_ref()
            .map(|style| style.clone_ref(py))
    }

    #[setter]
    fn set_style(&self, py: Python<'_>, style: Option<Py<PyAny>>) -> PyResult<()> {
        let style = style.filter(|style| !style.is_none(py));
        if let Some(style) = &style {
            style_type(Some(style.bind(py)))?;
        }
        let _replaced = std::mem::replace(&mut lock(&self.state).style, style);
        Ok(())
    }

    #[getter]
    fn speed(&self) -> f64 {
        lock(&self.state).speed
    }

    #[setter]
    fn set_speed(&self, speed: f64) {
        lock(&self.state).speed = speed;
    }

    #[getter]
    fn frame_no_offset(&self) -> f64 {
        lock(&self.state).frame_no_offset
    }

    #[setter]
    fn set_frame_no_offset(&self, offset: f64) {
        lock(&self.state).frame_no_offset = offset;
    }

    #[getter(_update_speed)]
    fn update_speed(&self) -> f64 {
        lock(&self.state).update_speed
    }

    #[setter(_update_speed)]
    fn set_update_speed(&self, speed: f64) {
        lock(&self.state).update_speed = speed;
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        // A spinner being updated is skipped rather than waited for.
        let Ok(state) = self.state.try_lock() else {
            return Ok(());
        };
        visit.call(&state.text)?;
        if let Some(style) = &state.style {
            visit.call(style)?;
        }
        Ok(())
    }
}

/// What `Spinner.render` returns for a spinner whose text is a renderable
/// (upstream: a `Table.grid(padding=1)` of the frame and the text).
#[pyclass(name = "_SpinnerGrid", module = "rs_rich.spinner")]
struct SpinnerGrid {
    frame: CoreText,
    text: Py<PyAny>,
}

impl AsRenderable for SpinnerGrid {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(grid(self.frame.clone(), self.text.clone_ref(py))))
    }
}

/// `rich._spinners.SPINNERS`: `{name: {"interval": ms, "frames": [...]}}`.
fn spinners(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    let table = PyDict::new(py);
    for name in NAMES {
        let (interval, frames) = spinner_data(name).expect("every name has data");
        let entry = PyDict::new(py);
        if interval.fract() == 0.0 {
            entry.set_item("interval", interval as i64)?;
        } else {
            entry.set_item("interval", interval)?;
        }
        entry.set_item("frames", PyList::new(py, frames.iter())?)?;
        table.set_item(*name, entry)?;
    }
    Ok(table)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Spinner>(m)?;
    renderable::add_renderable_class::<SpinnerGrid>(m)?;
    m.add("SPINNERS", spinners(m.py())?)?;
    Ok(())
}
