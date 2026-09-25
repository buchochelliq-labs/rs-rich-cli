//! `rich.status.Status` (and `Console.status`): a spinner and a message,
//! animated in a transient `Live`.
//!
//! The spinner is the live area's own renderable over core's `Spinner`
//! (frames, speed changes), assembled as upstream's `Spinner.render` does:
//! `Text.assemble(frame, " ", text)` for text, a `Table.grid(padding=1)` of
//! the frame and any other renderable.

use std::sync::{Mutex, MutexGuard};

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::protocol::Renderable;
use rich::table::{Cell, ColumnOptions};
use rich::{Spinner as CoreSpinner, StyleType, Table as CoreTable, Text as CoreText};

use super::live_display::Live;
use super::util;
use crate::console::Console;
use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::style::style_type;

struct SpinnerState {
    spinner: CoreSpinner,
    text: Option<Py<PyAny>>,
    style: Option<StyleType>,
}

/// The spinner a `Status` shows (upstream's `rich.spinner.Spinner` with a
/// text). Renders the frame for the console's clock (`console.get_time()`).
#[pyclass(name = "_StatusSpinner", module = "rs_rich.status", frozen)]
pub(crate) struct StatusSpinner {
    state: Mutex<SpinnerState>,
}

/// `Text.from_markup(text)` for a `str`, else the object.
fn spinner_text(py: Python<'_>, text: Option<Bound<'_, PyAny>>) -> PyResult<Option<Py<PyAny>>> {
    Ok(match text {
        Some(text) if util::is_str(&text) => {
            // `Text.from_markup(text)`: emoji codes and markup.
            let markup = util::to_str(&text)?;
            let parsed = renderable::render_str(&markup, true, true, false)?;
            Some(util::new_text(py, parsed)?)
        }
        Some(text) if !text.is_none() => Some(text.unbind()),
        _ => None,
    })
}

impl StatusSpinner {
    fn st(&self) -> MutexGuard<'_, SpinnerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn create(
        py: Python<'_>,
        name: &str,
        text: Option<Bound<'_, PyAny>>,
        style: Option<&Bound<'_, PyAny>>,
        speed: f64,
    ) -> PyResult<Py<StatusSpinner>> {
        // Rich's `Spinner(name)` raises `KeyError` for an unknown name.
        if rich::spinner::spinner_frames(name).is_none() {
            return Err(pyo3::exceptions::PyKeyError::new_err(format!(
                "no spinner called {}",
                pyo3::types::PyString::new(py, name).repr()?
            )));
        }
        Py::new(
            py,
            StatusSpinner {
                state: Mutex::new(SpinnerState {
                    spinner: CoreSpinner::new(name).speed(speed),
                    text: spinner_text(py, text)?,
                    style: style_type(style)?,
                }),
            },
        )
    }

    /// `Spinner.update(text=..., style=..., speed=...)`.
    fn update_with(
        &self,
        py: Python<'_>,
        text: Option<Bound<'_, PyAny>>,
        style: Option<&Bound<'_, PyAny>>,
        speed: Option<f64>,
    ) -> PyResult<()> {
        let text = match text {
            Some(text) if text.is_truthy()? => spinner_text(py, Some(text))?,
            _ => None,
        };
        let style = match style {
            Some(style) if style.is_truthy()? => style_type(Some(style))?,
            _ => None,
        };
        let mut state = self.st();
        if let Some(text) = text {
            state.text = Some(text);
        }
        if let Some(style) = style {
            state.style = Some(style);
        }
        if let Some(speed) = speed.filter(|s| *s != 0.0) {
            state.spinner.update(None, None, Some(speed));
        }
        Ok(())
    }

    /// `Spinner.render(time)` as a core renderable.
    fn render_at(&self, py: Python<'_>, time: f64) -> PyResult<Box<dyn Renderable>> {
        let (frame, text, style) = {
            let state = self.st();
            (
                state.spinner.render(time).plain().to_string(),
                state.text.as_ref().map(|t| t.clone_ref(py)),
                state.style.clone(),
            )
        };
        let styled_frame = || {
            let mut frame_text = CoreText::new("");
            frame_text.append(&frame, style.clone());
            frame_text
        };
        let Some(text) = text else {
            return Ok(Box::new(styled_frame()));
        };
        let text = text.bind(py);
        if !text.is_truthy()? {
            return Ok(Box::new(styled_frame()));
        }
        if let Some(core) = util::core_text(text) {
            let mut assembled = styled_frame();
            assembled.append(" ", None);
            return Ok(Box::new(assembled.append_text(&core)));
        }
        let mut table = CoreTable::grid().padding(1, 1, 1, 1);
        table.add_column_with(CoreText::new(""), ColumnOptions::default());
        table.add_column_with(CoreText::new(""), ColumnOptions::default());
        table.add_row_cells(vec![
            Cell::Text(styled_frame()),
            Cell::Renderable(PyRenderable::shared(text.clone().unbind(), None)),
        ]);
        Ok(Box::new(table))
    }
}

impl AsRenderable for StatusSpinner {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let console = renderable::ambient()?.console.clone_ref(py);
        let time: f64 = util::console_clock(console.bind(py))?.call0()?.extract()?;
        self.render_at(py, time)
    }
}

#[pymethods]
impl StatusSpinner {
    #[getter]
    fn text(&self, py: Python<'_>) -> Py<PyAny> {
        self.st()
            .text
            .as_ref()
            .map_or_else(|| py.None(), |t| t.clone_ref(py))
    }

    /// The frame (and text) at `time`.
    fn render(&self, py: Python<'_>, time: f64) -> PyResult<Py<PyAny>> {
        // Text is the common case; anything else is shown through a grid.
        let (frame, text, style) = {
            let state = self.st();
            (
                state.spinner.render(time).plain().to_string(),
                state.text.as_ref().map(|t| t.clone_ref(py)),
                state.style.clone(),
            )
        };
        let mut frame_text = CoreText::new("");
        frame_text.append(&frame, style);
        match text.as_ref().and_then(|t| util::core_text(t.bind(py))) {
            Some(core) if !core.plain().is_empty() => {
                frame_text.append(" ", None);
                util::new_text(py, frame_text.append_text(&core))
            }
            _ => util::new_text(py, frame_text),
        }
    }

    #[pyo3(signature = (*, text=None, style=None, speed=None))]
    fn update(
        &self,
        py: Python<'_>,
        text: Option<Bound<'_, PyAny>>,
        style: Option<Bound<'_, PyAny>>,
        speed: Option<f64>,
    ) -> PyResult<()> {
        self.update_with(py, text, style.as_ref(), speed)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(state) = self.state.try_lock() {
            if let Some(text) = &state.text {
                visit.call(text)?;
            }
        }
        Ok(())
    }
}

struct StatusState {
    status: Py<PyAny>,
    spinner_style: Py<PyAny>,
    speed: f64,
    spinner: Py<StatusSpinner>,
}

/// `rich.status.Status(status, *, console=None, spinner="dots",
/// spinner_style="status.spinner", speed=1.0, refresh_per_second=12.5)`.
#[pyclass(name = "Status", module = "rs_rich.status", frozen)]
pub(crate) struct Status {
    state: Mutex<StatusState>,
    live: Py<Live>,
}

impl Status {
    fn st(&self) -> MutexGuard<'_, StatusState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[pymethods]
impl Status {
    #[new]
    #[pyo3(signature = (
        status, *, console=None, spinner="dots", spinner_style=None, speed=1.0,
        refresh_per_second=12.5
    ))]
    fn new(
        py: Python<'_>,
        status: Bound<'_, PyAny>,
        console: Option<Bound<'_, PyAny>>,
        spinner: &str,
        spinner_style: Option<Bound<'_, PyAny>>,
        speed: f64,
        refresh_per_second: f64,
    ) -> PyResult<Status> {
        let spinner_style = spinner_style
            .filter(|s| !s.is_none())
            .unwrap_or_else(|| pyo3::types::PyString::new(py, "status.spinner").into_any());
        let spinner_obj = StatusSpinner::create(
            py,
            spinner,
            Some(status.clone()),
            Some(&spinner_style),
            speed,
        )?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("console", console)?;
        kwargs.set_item("refresh_per_second", refresh_per_second)?;
        kwargs.set_item("transient", true)?;
        let live = py
            .get_type::<Live>()
            .call((spinner_obj.clone_ref(py),), Some(&kwargs))?
            .cast_into::<Live>()?
            .unbind();
        Ok(Status {
            state: Mutex::new(StatusState {
                status: status.unbind(),
                spinner_style: spinner_style.unbind(),
                speed,
                spinner: spinner_obj,
            }),
            live,
        })
    }

    /// The spinner shown.
    #[getter]
    fn renderable(&self, py: Python<'_>) -> Py<StatusSpinner> {
        self.st().spinner.clone_ref(py)
    }

    #[getter]
    fn console(&self, py: Python<'_>) -> Py<PyAny> {
        self.live.get().console_of(py).unbind()
    }

    #[getter]
    fn _live(&self, py: Python<'_>) -> Py<Live> {
        self.live.clone_ref(py)
    }

    #[getter]
    fn status(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().status.clone_ref(py)
    }

    #[getter]
    fn spinner_style(&self, py: Python<'_>) -> Py<PyAny> {
        self.st().spinner_style.clone_ref(py)
    }

    #[getter]
    fn speed(&self) -> f64 {
        self.st().speed
    }

    /// Change the message, spinner, its style or its speed.
    #[pyo3(signature = (status=None, *, spinner=None, spinner_style=None, speed=None))]
    fn update(
        &self,
        py: Python<'_>,
        status: Option<Bound<'_, PyAny>>,
        spinner: Option<&str>,
        spinner_style: Option<Bound<'_, PyAny>>,
        speed: Option<f64>,
    ) -> PyResult<()> {
        let (status, style, speed) = {
            let mut state = self.st();
            if let Some(status) = status.filter(|s| !s.is_none()) {
                state.status = status.unbind();
            }
            if let Some(style) = spinner_style.filter(|s| !s.is_none()) {
                state.spinner_style = style.unbind();
            }
            if let Some(speed) = speed {
                state.speed = speed;
            }
            (
                state.status.clone_ref(py),
                state.spinner_style.clone_ref(py),
                state.speed,
            )
        };
        if let Some(name) = spinner {
            let new = StatusSpinner::create(
                py,
                name,
                Some(status.bind(py).clone()),
                Some(style.bind(py)),
                speed,
            )?;
            self.st().spinner = new.clone_ref(py);
            let kwargs = PyDict::new(py);
            kwargs.set_item("refresh", true)?;
            self.live
                .bind(py)
                .call_method("update", (new,), Some(&kwargs))?;
        } else {
            let spinner = self.st().spinner.clone_ref(py);
            spinner.get().update_with(
                py,
                Some(status.bind(py).clone()),
                Some(style.bind(py)),
                Some(speed),
            )?;
        }
        Ok(())
    }

    fn start(&self, py: Python<'_>) -> PyResult<()> {
        self.live.bind(py).call_method0("start")?;
        Ok(())
    }

    fn stop(&self, py: Python<'_>) -> PyResult<()> {
        self.live.bind(py).call_method0("stop")?;
        Ok(())
    }

    fn __rich__(&self, py: Python<'_>) -> Py<StatusSpinner> {
        self.renderable(py)
    }

    fn __enter__(slf: Bound<'_, Self>) -> PyResult<Bound<'_, Self>> {
        slf.get().start(slf.py())?;
        Ok(slf)
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, PyTuple>) -> PyResult<()> {
        self.stop(py)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.live)?;
        if let Ok(state) = self.state.try_lock() {
            visit.call(&state.status)?;
            visit.call(&state.spinner_style)?;
            visit.call(&state.spinner)?;
        }
        Ok(())
    }
}

/// `Console.status(status, *, spinner="dots", spinner_style="status.spinner",
/// speed=1.0, refresh_per_second=12.5)`.
pub(crate) fn console_status(
    console: &Bound<'_, Console>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let py = console.py();
    let call_kwargs = PyDict::new(py);
    if let Some(kwargs) = kwargs {
        call_kwargs.update(kwargs.as_mapping())?;
    }
    call_kwargs.set_item("console", console)?;
    Ok(py
        .get_type::<Status>()
        .call(args, Some(&call_kwargs))?
        .unbind())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<StatusSpinner>(m)?;
    m.add_class::<Status>()?;
    Ok(())
}
