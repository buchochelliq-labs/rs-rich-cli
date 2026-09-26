//! `rich.align`: `Align` and `VerticalCenter`. Port of upstream
//! `rich/align.py`.
//!
//! Core's `Align` is horizontal only; vertical alignment, `style`, `pad`,
//! `width` and `height` are ported here.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyString, PyType};
use pyo3::{PyTraverseError, PyVisit};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::{Style as CoreStyle, StyleType};

use crate::limits::{check_alloc, MAX_CONSOLE_HEIGHT, MAX_CONSOLE_WIDTH};
use crate::renderable::{self, AsRenderable};
use crate::style::style_type;

use super::{
    get_style, join_lines, render, render_lines, screen_height, shape, split_lines, Child,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Horizontal {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Vertical {
    Top,
    Middle,
    Bottom,
}

pub(crate) fn horizontal(align: &str) -> PyResult<Horizontal> {
    Ok(match align {
        "left" => Horizontal::Left,
        "center" => Horizontal::Center,
        "right" => Horizontal::Right,
        other => {
            return Err(PyValueError::new_err(format!(
            "invalid value for align, expected \"left\", \"center\", or \"right\" (not '{other}')"
        )))
        }
    })
}

fn vertical(value: Option<&str>) -> PyResult<Option<Vertical>> {
    Ok(match value {
        None => None,
        Some("top") => Some(Vertical::Top),
        Some("middle") => Some(Vertical::Middle),
        Some("bottom") => Some(Vertical::Bottom),
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "invalid value for vertical, expected \"top\", \"middle\", or \"bottom\" (not '{other}')"
            )))
        }
    })
}

pub(crate) struct AlignRender<C> {
    pub(crate) child: C,
    pub(crate) align: Horizontal,
    pub(crate) style: Option<StyleType>,
    pub(crate) vertical: Option<Vertical>,
    pub(crate) pad: bool,
    pub(crate) width: Option<usize>,
    pub(crate) height: Option<usize>,
}

impl<C> AlignRender<C> {
    pub(crate) fn new(child: C, align: Horizontal) -> Self {
        AlignRender {
            child,
            align,
            style: None,
            vertical: None,
            pad: true,
            width: None,
            height: None,
        }
    }
}

impl<C: Child> Renderable for AlignRender<C> {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let child = self.child.get();
        let measured = CoreMeasurement::get(console, options, child).maximum;
        let constrained = match self.width {
            Some(width) => measured.min(width),
            None => measured,
        };
        // `console.render(Constrain(renderable, width), options.update(height=None))`.
        let mut child_options = options.clone();
        child_options.height = None;
        let child_options = child_options.update_width(constrained.min(options.max_width));
        let rendered = render(console, child, &child_options);
        let lines = split_lines(rendered);
        let (width, height) = shape(&lines);
        let lines: Vec<Vec<CoreSegment>> = lines
            .iter()
            .map(|line| CoreSegment::adjust_line_length(line, width, None))
            .collect();
        let excess = options.max_width as isize - width as isize;
        let style: Option<CoreStyle> = self.style.as_ref().map(|s| get_style(console, s));

        let mut rows: Vec<Vec<CoreSegment>> = Vec::new();
        let body = |rows: &mut Vec<Vec<CoreSegment>>| {
            for line in &lines {
                let mut row = Vec::with_capacity(line.len() + 2);
                if excess <= 0 {
                    row.extend(line.iter().cloned());
                } else {
                    let excess = excess as usize;
                    match self.align {
                        Horizontal::Left => {
                            row.extend(line.iter().cloned());
                            if self.pad {
                                row.push(CoreSegment::new(" ".repeat(excess), style.clone()));
                            }
                        }
                        Horizontal::Center => {
                            let left = excess / 2;
                            if left > 0 {
                                row.push(CoreSegment::new(" ".repeat(left), style.clone()));
                            }
                            row.extend(line.iter().cloned());
                            if self.pad {
                                row.push(CoreSegment::new(
                                    " ".repeat(excess - left),
                                    style.clone(),
                                ));
                            }
                        }
                        Horizontal::Right => {
                            row.push(CoreSegment::new(" ".repeat(excess), style.clone()));
                            row.extend(line.iter().cloned());
                        }
                    }
                }
                rows.push(row);
            }
        };
        let blank_width = self.width.unwrap_or(options.max_width);
        let blank = |rows: &mut Vec<Vec<CoreSegment>>, count: isize| {
            for _ in 0..count.max(0) {
                rows.push(if self.pad {
                    vec![CoreSegment::new(" ".repeat(blank_width), style.clone())]
                } else {
                    Vec::new()
                });
            }
        };
        let vertical_height = self.height.or(options.height);
        match (self.vertical, vertical_height) {
            (Some(vertical), Some(total)) => {
                let spare = total as isize - height as isize;
                match vertical {
                    Vertical::Top => {
                        body(&mut rows);
                        blank(&mut rows, spare);
                    }
                    Vertical::Middle => {
                        let top = spare.div_euclid(2);
                        blank(&mut rows, top);
                        body(&mut rows);
                        blank(&mut rows, spare - top);
                    }
                    Vertical::Bottom => {
                        blank(&mut rows, spare);
                        body(&mut rows);
                    }
                }
            }
            _ => body(&mut rows),
        }
        let segments = join_lines(rows);
        match &self.style {
            Some(style) => CoreSegment::apply_style(&segments, &get_style(console, style)),
            None => segments,
        }
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        CoreMeasurement::get(console, options, self.child.get())
    }
}

/// `rich.align.Align`: align a renderable by adding spaces.
#[pyclass(name = "Align", module = "rs_rich.align")]
pub(crate) struct Align {
    #[pyo3(get, set)]
    renderable: Py<PyAny>,
    align: String,
    style: Option<Py<PyAny>>,
    vertical: Option<String>,
    #[pyo3(get, set)]
    pad: bool,
    #[pyo3(get, set)]
    width: Option<usize>,
    #[pyo3(get, set)]
    height: Option<usize>,
}

impl AsRenderable for Align {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        // Rich pads to `width` and `height` with blank cells and lines.
        if let Some(width) = self.width {
            check_alloc("width", width, MAX_CONSOLE_WIDTH)?;
        }
        if let Some(height) = self.height {
            check_alloc("height", height, MAX_CONSOLE_HEIGHT)?;
        }
        let child = renderable::to_renderable(self.renderable.bind(py), None)?;
        Ok(Box::new(AlignRender {
            child,
            align: horizontal(&self.align)?,
            style: style_type(self.style.as_ref().map(|s| s.bind(py)))?,
            vertical: vertical(self.vertical.as_deref())?,
            pad: self.pad,
            width: self.width,
            height: self.height,
        }))
    }
}

#[pymethods]
impl Align {
    #[new]
    #[pyo3(signature = (
        renderable, align="left", style=None, *, vertical=None, pad=true, width=None, height=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        renderable: Py<PyAny>,
        align: &str,
        style: Option<Py<PyAny>>,
        vertical: Option<String>,
        pad: bool,
        width: Option<usize>,
        height: Option<usize>,
    ) -> PyResult<Self> {
        horizontal(align)?;
        self::vertical(vertical.as_deref())?;
        let style = style.filter(|style| !style.is_none(py));
        style_type(style.as_ref().map(|s| s.bind(py)))?;
        Ok(Align {
            renderable,
            align: align.to_string(),
            style,
            vertical,
            pad,
            width,
            height,
        })
    }

    /// Align a renderable to the left.
    #[classmethod]
    #[pyo3(signature = (renderable, style=None, *, vertical=None, pad=true, width=None, height=None))]
    #[allow(clippy::too_many_arguments)]
    fn left(
        _cls: &Bound<'_, PyType>,
        py: Python<'_>,
        renderable: Py<PyAny>,
        style: Option<Py<PyAny>>,
        vertical: Option<String>,
        pad: bool,
        width: Option<usize>,
        height: Option<usize>,
    ) -> PyResult<Self> {
        Align::new(py, renderable, "left", style, vertical, pad, width, height)
    }

    /// Align a renderable to the center.
    #[classmethod]
    #[pyo3(signature = (renderable, style=None, *, vertical=None, pad=true, width=None, height=None))]
    #[allow(clippy::too_many_arguments)]
    fn center(
        _cls: &Bound<'_, PyType>,
        py: Python<'_>,
        renderable: Py<PyAny>,
        style: Option<Py<PyAny>>,
        vertical: Option<String>,
        pad: bool,
        width: Option<usize>,
        height: Option<usize>,
    ) -> PyResult<Self> {
        Align::new(
            py, renderable, "center", style, vertical, pad, width, height,
        )
    }

    /// Align a renderable to the right.
    #[classmethod]
    #[pyo3(signature = (renderable, style=None, *, vertical=None, pad=true, width=None, height=None))]
    #[allow(clippy::too_many_arguments)]
    fn right(
        _cls: &Bound<'_, PyType>,
        py: Python<'_>,
        renderable: Py<PyAny>,
        style: Option<Py<PyAny>>,
        vertical: Option<String>,
        pad: bool,
        width: Option<usize>,
        height: Option<usize>,
    ) -> PyResult<Self> {
        Align::new(py, renderable, "right", style, vertical, pad, width, height)
    }

    #[getter]
    fn align(&self) -> String {
        self.align.clone()
    }

    #[setter]
    fn set_align(&mut self, align: &str) -> PyResult<()> {
        horizontal(align)?;
        self.align = align.to_string();
        Ok(())
    }

    #[getter]
    fn vertical(&self) -> Option<String> {
        self.vertical.clone()
    }

    #[setter]
    fn set_vertical(&mut self, value: Option<String>) -> PyResult<()> {
        self::vertical(value.as_deref())?;
        self.vertical = value;
        Ok(())
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.style.as_ref().map(|s| s.clone_ref(py))
    }

    #[setter]
    fn set_style(&mut self, py: Python<'_>, style: Option<Py<PyAny>>) -> PyResult<()> {
        let style = style.filter(|style| !style.is_none(py));
        style_type(style.as_ref().map(|s| s.bind(py)))?;
        self.style = style;
        Ok(())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Align({}, {})",
            self.renderable.bind(py).repr()?,
            PyString::new(py, &self.align).repr()?
        ))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderable)?;
        if let Some(style) = &self.style {
            visit.call(style)?;
        }
        Ok(())
    }
}

struct VerticalCenterRender {
    child: Box<dyn Renderable>,
    style: Option<StyleType>,
}

impl Renderable for VerticalCenterRender {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let style = self.style.as_ref().map(|s| get_style(console, s));
        let mut child_options = options.clone();
        child_options.height = None;
        let lines = render_lines(console, self.child.as_ref(), &child_options, None, false);
        let (width, _) = shape(&lines);
        let height = options.height.unwrap_or_else(|| screen_height(console));
        let top = height.saturating_sub(lines.len()) / 2;
        let bottom = (height as isize - top as isize - lines.len() as isize).max(0) as usize;
        let blank = || vec![CoreSegment::new(" ".repeat(width), style.clone())];
        let mut rows = Vec::with_capacity(top + lines.len() + bottom);
        rows.extend((0..top).map(|_| blank()));
        rows.extend(lines);
        rows.extend((0..bottom).map(|_| blank()));
        join_lines(rows)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        CoreMeasurement::get(console, options, self.child.as_ref())
    }
}

/// `rich.align.VerticalCenter`: center a renderable vertically (deprecated
/// upstream in favour of `Align(vertical="middle")`).
#[pyclass(name = "VerticalCenter", module = "rs_rich.align")]
pub(crate) struct VerticalCenter {
    #[pyo3(get, set)]
    renderable: Py<PyAny>,
    style: Option<Py<PyAny>>,
}

impl AsRenderable for VerticalCenter {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(VerticalCenterRender {
            child: renderable::to_renderable(self.renderable.bind(py), None)?,
            style: style_type(self.style.as_ref().map(|s| s.bind(py)))?,
        }))
    }
}

#[pymethods]
impl VerticalCenter {
    #[new]
    #[pyo3(signature = (renderable, style=None))]
    fn new(py: Python<'_>, renderable: Py<PyAny>, style: Option<Py<PyAny>>) -> PyResult<Self> {
        let style = style.filter(|style| !style.is_none(py));
        style_type(style.as_ref().map(|s| s.bind(py)))?;
        Ok(VerticalCenter { renderable, style })
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.style.as_ref().map(|s| s.clone_ref(py))
    }

    #[setter]
    fn set_style(&mut self, py: Python<'_>, style: Option<Py<PyAny>>) -> PyResult<()> {
        let style = style.filter(|style| !style.is_none(py));
        style_type(style.as_ref().map(|s| s.bind(py)))?;
        self.style = style;
        Ok(())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "VerticalCenter({})",
            self.renderable.bind(py).repr()?
        ))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderable)?;
        if let Some(style) = &self.style {
            visit.call(style)?;
        }
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Align>(m)?;
    renderable::add_renderable_class::<VerticalCenter>(m)
}
