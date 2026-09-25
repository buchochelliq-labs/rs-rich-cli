//! `rich.constrain.Constrain` and `rich.styled.Styled`. Ports of upstream
//! `rich/constrain.py` and `rich/styled.py`.
//!
//! Both are small enough that the renderers are written here, generic over
//! the child, so `Columns` can use them around its shared cells; a `Styled`
//! style may be a theme name, resolved when it renders.

use pyo3::prelude::*;
use pyo3::{PyTraverseError, PyVisit};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;

use crate::renderable::{self, AsRenderable};
use crate::style::style_type;

use super::{get_style, render, Child};

pub(crate) struct ConstrainRender<C> {
    pub(crate) child: C,
    pub(crate) width: Option<usize>,
}

impl<C: Child> Renderable for ConstrainRender<C> {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        match self.width {
            None => render(console, self.child.get(), options),
            Some(width) => render(
                console,
                self.child.get(),
                &options.update_width(width.min(options.max_width)),
            ),
        }
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let options = match self.width {
            Some(width) => options.update_width(width),
            None => options.clone(),
        };
        CoreMeasurement::get(console, &options, self.child.get())
    }
}

pub(crate) struct StyledRender<C> {
    pub(crate) child: C,
    pub(crate) style: StyleType,
}

impl<C: Child> Renderable for StyledRender<C> {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let style = get_style(console, &self.style);
        let segments = render(console, self.child.get(), options);
        if style.is_null() {
            return segments;
        }
        CoreSegment::apply_style(&segments, &style)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        CoreMeasurement::get(console, options, self.child.get())
    }
}

/// `rich.constrain.Constrain`: render within at most `width` cells.
#[pyclass(name = "Constrain", module = "rs_rich.constrain")]
pub(crate) struct Constrain {
    #[pyo3(get, set)]
    renderable: Py<PyAny>,
    #[pyo3(get, set)]
    width: Option<usize>,
}

impl AsRenderable for Constrain {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(ConstrainRender {
            child: renderable::to_renderable(self.renderable.bind(py), None)?,
            width: self.width,
        }))
    }
}

#[pymethods]
impl Constrain {
    #[new]
    #[pyo3(signature = (renderable, width=Some(80)))]
    fn new(renderable: Py<PyAny>, width: Option<usize>) -> Self {
        Constrain { renderable, width }
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderable)
    }
}

/// `rich.styled.Styled`: apply a style across a whole renderable.
#[pyclass(name = "Styled", module = "rs_rich.styled")]
pub(crate) struct Styled {
    #[pyo3(get, set)]
    renderable: Py<PyAny>,
    style: Py<PyAny>,
}

impl AsRenderable for Styled {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(StyledRender {
            child: renderable::to_renderable(self.renderable.bind(py), None)?,
            style: style_type(Some(self.style.bind(py)))?.unwrap_or_default(),
        }))
    }
}

#[pymethods]
impl Styled {
    #[new]
    fn new(renderable: Py<PyAny>, style: Bound<'_, PyAny>) -> PyResult<Self> {
        style_type(Some(&style))?;
        Ok(Styled {
            renderable,
            style: style.unbind(),
        })
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

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderable)?;
        visit.call(&self.style)
    }
}

/// The child of a `Styled` or `Constrain`.
pub(crate) fn end_child<'py>(value: &Bound<'py, PyAny>) -> Option<Bound<'py, PyAny>> {
    let py = value.py();
    if let Ok(styled) = value.cast::<Styled>() {
        return Some(styled.borrow().renderable.bind(py).clone());
    }
    if let Ok(constrain) = value.cast::<Constrain>() {
        return Some(constrain.borrow().renderable.bind(py).clone());
    }
    None
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Constrain>(m)?;
    renderable::add_renderable_class::<Styled>(m)
}
