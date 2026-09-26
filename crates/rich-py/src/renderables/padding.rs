//! `rich.padding`: `Padding`. Port of upstream `rich/padding.py`.
//!
//! Core's `Padding` always fills the width; upstream's `expand=False` (and
//! so `Padding.indent`) fits the content, so the renderer is ported here.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyInt, PyString, PyTuple, PyType};
use pyo3::{PyTraverseError, PyVisit};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;

use crate::limits::MAX_PADDING;
use crate::renderable::{self, AsRenderable};
use crate::style::style_type;

use super::{get_style, join_lines, render_lines, Child};

pub(crate) type Pad = (usize, usize, usize, usize);

pub(crate) struct PaddingRender<C> {
    pub(crate) child: C,
    pub(crate) pad: Pad,
    pub(crate) style: StyleType,
    pub(crate) expand: bool,
}

impl<C: Child> Renderable for PaddingRender<C> {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let (top, right, bottom, left) = self.pad;
        let style = get_style(console, &self.style);
        let width = if self.expand {
            options.max_width
        } else {
            (CoreMeasurement::get(console, options, self.child.get()).maximum + left + right)
                .min(options.max_width)
        };
        let mut render_options = options.update_width(width.saturating_sub(left + right));
        if let Some(height) = render_options.height {
            render_options.height = Some(height.saturating_sub(top + bottom));
        }
        let lines = render_lines(
            console,
            self.child.get(),
            &render_options,
            Some(&style),
            true,
        );
        let style = Some(style);
        let blank = || vec![CoreSegment::new(" ".repeat(width), style.clone())];
        let mut rows = Vec::with_capacity(top + lines.len() + bottom);
        rows.extend((0..top).map(|_| blank()));
        for line in lines {
            let mut row = Vec::with_capacity(line.len() + 2);
            if left > 0 {
                row.push(CoreSegment::new(" ".repeat(left), style.clone()));
            }
            row.extend(line);
            if right > 0 {
                row.push(CoreSegment::new(" ".repeat(right), style.clone()));
            }
            rows.push(row);
        }
        rows.extend((0..bottom).map(|_| blank()));
        join_lines(rows)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let (_, right, _, left) = self.pad;
        let max_width = options.max_width;
        let extra_width = left + right;
        if max_width < extra_width + 1 {
            return CoreMeasurement::new(max_width, max_width);
        }
        let child = CoreMeasurement::get(console, options, self.child.get());
        CoreMeasurement::new(child.minimum + extra_width, child.maximum + extra_width)
            .with_maximum(max_width)
    }
}

fn side(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    let number: i128 = if value.is_instance_of::<PyInt>() {
        value.extract().unwrap_or(i128::MAX)
    } else {
        value.extract()?
    };
    usize::try_from(number)
        .ok()
        .filter(|number| *number <= MAX_PADDING)
        .ok_or_else(|| {
            PyValueError::new_err(format!(
                "padding must be between 0 and {MAX_PADDING}, got {number}"
            ))
        })
}

/// `Padding.unpack`: 1, 2 or 4 integers, CSS style.
pub(crate) fn unpack(pad: &Bound<'_, PyAny>) -> PyResult<Pad> {
    if pad.is_instance_of::<PyInt>() {
        let all = side(pad)?;
        return Ok((all, all, all, all));
    }
    let items: Vec<Bound<'_, PyAny>> = pad.try_iter()?.collect::<PyResult<_>>()?;
    match items.as_slice() {
        [all] => {
            let all = side(all)?;
            Ok((all, all, all, all))
        }
        [vertical, horizontal] => {
            let (vertical, horizontal) = (side(vertical)?, side(horizontal)?);
            Ok((vertical, horizontal, vertical, horizontal))
        }
        [top, right, bottom, left] => Ok((side(top)?, side(right)?, side(bottom)?, side(left)?)),
        other => Err(PyValueError::new_err(format!(
            "1, 2 or 4 integers required for padding; {} given",
            other.len()
        ))),
    }
}

/// `rich.padding.Padding`: space around content.
#[pyclass(name = "Padding", module = "rs_rich.padding")]
pub(crate) struct Padding {
    #[pyo3(get, set)]
    renderable: Py<PyAny>,
    #[pyo3(get, set)]
    top: usize,
    #[pyo3(get, set)]
    right: usize,
    #[pyo3(get, set)]
    bottom: usize,
    #[pyo3(get, set)]
    left: usize,
    style: Py<PyAny>,
    #[pyo3(get, set)]
    expand: bool,
}

impl AsRenderable for Padding {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(PaddingRender {
            child: renderable::to_renderable(self.renderable.bind(py), None)?,
            pad: (self.top, self.right, self.bottom, self.left),
            style: style_type(Some(self.style.bind(py)))?.unwrap_or_default(),
            expand: self.expand,
        }))
    }
}

#[pymethods]
impl Padding {
    #[new]
    #[pyo3(signature = (renderable, pad=None, *, style=None, expand=true))]
    fn new(
        py: Python<'_>,
        renderable: Py<PyAny>,
        pad: Option<&Bound<'_, PyAny>>,
        style: Option<Py<PyAny>>,
        expand: bool,
    ) -> PyResult<Self> {
        let (top, right, bottom, left) = match pad {
            Some(pad) => unpack(pad)?,
            None => (0, 0, 0, 0),
        };
        let style = style.unwrap_or_else(|| PyString::new(py, "none").into_any().unbind());
        style_type(Some(style.bind(py)))?;
        Ok(Padding {
            renderable,
            top,
            right,
            bottom,
            left,
            style,
            expand,
        })
    }

    /// `Padding.indent(renderable, level)`: indent by `level` cells, fitting
    /// the content.
    #[classmethod]
    fn indent(
        _cls: &Bound<'_, PyType>,
        py: Python<'_>,
        renderable: Py<PyAny>,
        level: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        let zero = 0i64.into_pyobject(py)?.into_any();
        let pad = PyTuple::new(py, [zero.clone(), zero.clone(), zero, level.clone()])?;
        Padding::new(py, renderable, Some(pad.as_any()), None, false)
    }

    /// `Padding.unpack(pad)`: `(top, right, bottom, left)`.
    #[staticmethod]
    #[pyo3(name = "unpack")]
    fn py_unpack(pad: &Bound<'_, PyAny>) -> PyResult<Pad> {
        unpack(pad)
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Py<PyAny> {
        self.style.clone_ref(py)
    }

    #[setter]
    fn set_style(&mut self, style: Bound<'_, PyAny>) -> PyResult<()> {
        style_type(Some(&style))?;
        self.style = style.unbind();
        Ok(())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Padding({}, ({},{},{},{}))",
            self.renderable.bind(py).repr()?,
            self.top,
            self.right,
            self.bottom,
            self.left
        ))
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderable)?;
        visit.call(&self.style)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Padding>(m)
}
