//! `rich.live_render.LiveRender`: a renderable that remembers the shape of
//! its last render, so a live display can move the cursor back over it.
//!
//! It is a protocol object (`__rich_console__`), as upstream's is, so a print
//! of it adds no newline after its last line.

use std::sync::{Mutex, MutexGuard};

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use pyo3::{PyTraverseError, PyVisit};

use rich::control::{Control as CoreControl, ControlType};
use rich::segment::Segment as CoreSegment;
use rich::{Justify, Overflow, Text as CoreText};

use super::util::{self, Control};
use crate::segment::Segment;

struct State {
    renderable: Py<PyAny>,
    style: Py<PyAny>,
    vertical_overflow: String,
    shape: Option<(usize, usize)>,
}

/// `rich.live_render.LiveRender(renderable, style="", vertical_overflow="ellipsis")`.
#[pyclass(name = "LiveRender", module = "rs_rich.live_render", frozen)]
pub(crate) struct LiveRender {
    state: Mutex<State>,
}

impl LiveRender {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn create(
        py: Python<'_>,
        renderable: Py<PyAny>,
        vertical_overflow: &str,
    ) -> PyResult<Py<LiveRender>> {
        Py::new(
            py,
            LiveRender {
                state: Mutex::new(State {
                    renderable,
                    style: PyString::new(py, "").into_any().unbind(),
                    vertical_overflow: vertical_overflow.to_string(),
                    shape: None,
                }),
            },
        )
    }

    pub(crate) fn set(&self, renderable: Py<PyAny>) {
        self.state().renderable = renderable;
    }

    pub(crate) fn set_overflow(&self, vertical_overflow: &str) {
        self.state().vertical_overflow = vertical_overflow.to_string();
    }

    pub(crate) fn height(&self) -> usize {
        self.state().shape.map_or(0, |(_, height)| height)
    }

    /// The codes of `position_cursor()`.
    pub(crate) fn position_codes(&self) -> String {
        match self.state().shape {
            Some((_, height)) => {
                let mut codes = vec![ControlType::CarriageReturn, ControlType::EraseInLine(2)];
                for _ in 0..height.saturating_sub(1) {
                    codes.push(ControlType::CursorUp(1));
                    codes.push(ControlType::EraseInLine(2));
                }
                CoreControl::new(&codes).as_str().to_string()
            }
            None => String::new(),
        }
    }

    /// The codes of `restore_cursor()`.
    pub(crate) fn restore_codes(&self) -> String {
        match self.state().shape {
            Some((_, height)) => {
                let mut codes = vec![ControlType::CarriageReturn];
                for _ in 0..height {
                    codes.push(ControlType::CursorUp(1));
                    codes.push(ControlType::EraseInLine(2));
                }
                CoreControl::new(&codes).as_str().to_string()
            }
            None => String::new(),
        }
    }
}

#[pymethods]
impl LiveRender {
    #[new]
    #[pyo3(signature = (renderable, style=None, vertical_overflow="ellipsis"))]
    fn new(
        py: Python<'_>,
        renderable: Py<PyAny>,
        style: Option<Py<PyAny>>,
        vertical_overflow: &str,
    ) -> LiveRender {
        LiveRender {
            state: Mutex::new(State {
                renderable,
                style: style.unwrap_or_else(|| PyString::new(py, "").into_any().unbind()),
                vertical_overflow: vertical_overflow.to_string(),
                shape: None,
            }),
        }
    }

    #[getter]
    fn renderable(&self, py: Python<'_>) -> Py<PyAny> {
        self.state().renderable.clone_ref(py)
    }

    #[setter(renderable)]
    fn set_renderable_attr(&self, renderable: Py<PyAny>) {
        self.set(renderable);
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Py<PyAny> {
        self.state().style.clone_ref(py)
    }

    #[setter]
    fn set_style(&self, style: Py<PyAny>) {
        self.state().style = style;
    }

    #[getter]
    fn vertical_overflow(&self) -> String {
        self.state().vertical_overflow.clone()
    }

    #[setter]
    fn set_vertical_overflow(&self, value: String) {
        self.state().vertical_overflow = value;
    }

    /// The number of lines in the last render (0 before any).
    #[getter]
    fn last_render_height(&self) -> usize {
        self.height()
    }

    fn set_renderable(&self, renderable: Py<PyAny>) {
        self.set(renderable);
    }

    /// Control codes to move the cursor to the start of the last render.
    fn position_cursor(&self) -> Control {
        Control {
            codes: self.position_codes(),
        }
    }

    /// Control codes to clear the last render and put the cursor back.
    fn restore_cursor(&self) -> Control {
        Control {
            codes: self.restore_codes(),
        }
    }

    fn __rich_console__<'py>(
        &self,
        console: &Bound<'py, PyAny>,
        options: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = console.py();
        let (renderable, style, overflow) = {
            let state = self.state();
            (
                state.renderable.clone_ref(py),
                state.style.clone_ref(py),
                state.vertical_overflow.clone(),
            )
        };
        let style = style.bind(py);
        let kwargs = PyDict::new(py);
        let empty = style
            .cast::<PyString>()
            .is_ok_and(|s| s.to_cow().is_ok_and(|s| s.is_empty()));
        if !style.is_none() && !empty {
            kwargs.set_item("style", console.call_method1("get_style", (style,))?)?;
        }
        kwargs.set_item("pad", false)?;
        let rendered = console.call_method("render_lines", (renderable, options), Some(&kwargs))?;
        let mut lines = util::core_lines(&rendered)?;
        let mut shape = util::get_shape(&lines);
        let screen_height: usize = options.getattr("size")?.get_item(1)?.extract()?;
        if shape.1 > screen_height {
            if overflow == "crop" {
                lines.truncate(screen_height);
                shape = util::get_shape(&lines);
            } else if overflow == "ellipsis" {
                // `lines[: height - 1]`: a height of 0 drops the last line.
                let keep = if screen_height == 0 {
                    lines.len().saturating_sub(1)
                } else {
                    screen_height - 1
                };
                lines.truncate(keep);
                let mut text = CoreText::styled("...", "live.ellipsis");
                text.set_overflow(Some(Overflow::Crop));
                text.set_justify(Justify::Center);
                let text = util::new_text(py, text)?;
                let segments = util::core_segments(&console.call_method1("render", (text,))?)?;
                let line: Vec<CoreSegment> = crate::renderable::unterminated(segments)
                    .into_iter()
                    .filter(|segment| segment.text != "\n")
                    .collect();
                lines.push(line);
                shape = util::get_shape(&lines);
            }
        }
        self.state().shape = Some(shape);
        let result = PyList::empty(py);
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.iter().enumerate() {
            for segment in line {
                result.append(Segment::from_core(py, segment))?;
            }
            if index != last {
                result.append(Segment::from_core(py, &CoreSegment::line()))?;
            }
        }
        Ok(result)
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Ok(state) = self.state.try_lock() {
            visit.call(&state.renderable)?;
            visit.call(&state.style)?;
        }
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LiveRender>()
}
