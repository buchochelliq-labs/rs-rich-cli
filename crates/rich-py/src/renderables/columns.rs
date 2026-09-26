//! `rich.columns.Columns`. Port of upstream `rich/columns.py`.
//!
//! The Python class keeps what it was given; printing builds core's
//! `Columns` (width, padding, `column_first`, `right_to_left`, `align`,
//! `title`, `expand`, `equal`).

use pyo3::exceptions::PyZeroDivisionError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::align::HorizontalAlign;
use rich::protocol::Renderable;
use rich::table::Cell;
use rich::Columns as CoreColumns;

use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::text::Text;

use super::align::{horizontal, Horizontal};
use super::padding::unpack;
use super::{text_markup, unmeasured};

/// `rich.columns.Columns`: renderables in neat columns.
#[pyclass(name = "Columns", module = "rs_rich.columns")]
pub(crate) struct Columns {
    renderables: Py<PyList>,
    #[pyo3(get, set)]
    width: Option<usize>,
    padding: Py<PyAny>,
    #[pyo3(get, set)]
    expand: bool,
    #[pyo3(get, set)]
    equal: bool,
    #[pyo3(get, set)]
    column_first: bool,
    #[pyo3(get, set)]
    right_to_left: bool,
    align: Option<String>,
    #[pyo3(get, set)]
    title: Option<Py<PyAny>>,
}

impl AsRenderable for Columns {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let mut cells = Vec::new();
        for item in self.renderables.bind(py).iter() {
            if let Ok(markup) = item.cast::<PyString>() {
                // `console.render_str(renderable)`, with the print's `markup`.
                let markup = renderable::render_str_markup(markup.to_cow()?.into_owned())?;
                cells.push(Cell::Markup(markup));
            } else if let Ok(text) = item.extract::<PyRef<'_, Text>>() {
                cells.push(Cell::Text(text.inner.clone()));
            } else {
                // Checked now, so a non-renderable raises from `print`. The
                // grid renders its cells with `highlight=False`.
                renderable::to_renderable(&item, Some(false))?;
                cells.push(Cell::Renderable(PyRenderable::shared(
                    item.unbind(),
                    Some(false),
                )));
            }
        }
        let padding = unpack(self.padding.bind(py))?;
        // Rich divides the width by `width + max(left, right)` padding.
        let step = match self.width {
            Some(width) if !cells.is_empty() => {
                Some(width.saturating_add(padding.1.max(padding.3)))
            }
            _ => None,
        };
        let mut columns = CoreColumns::from_cells(cells)
            .padding(padding)
            .expand(self.expand)
            .equal(self.equal)
            .column_first(self.column_first)
            .right_to_left(self.right_to_left);
        if let Some(width) = self.width {
            columns = columns.width(width);
        }
        if let Some(align) = self.align.as_deref() {
            columns = columns.align(match horizontal(align)? {
                Horizontal::Left => HorizontalAlign::Left,
                Horizontal::Center => HorizontalAlign::Center,
                Horizontal::Right => HorizontalAlign::Right,
            });
        }
        if let Some(title) = &self.title {
            let title = title.bind(py);
            columns = columns.title(if let Ok(text) = title.extract::<PyRef<'_, Text>>() {
                text_markup(&text.inner)
            } else {
                title.str()?.to_cow()?.into_owned()
            });
        }
        Ok(Box::new(Unmeasured { columns, step }))
    }
}

/// Upstream's `Columns` has no `__rich_measure__`: it takes any width.
/// `step` is a fixed column's width with its padding, when there is one.
struct Unmeasured {
    columns: CoreColumns,
    step: Option<usize>,
}

impl Renderable for Unmeasured {
    fn rich_render(
        &self,
        console: &rich::console::Console,
        options: &rich::console::ConsoleOptions,
    ) -> Vec<rich::segment::Segment> {
        // Rich's `max_width // (width + padding)` columns, then `item_count %
        // column_count`: no room for one column divides by zero.
        if let Some(step) = self.step {
            let message = match step {
                0 => Some("integer division or modulo by zero"),
                step if options.max_width / step == 0 => Some("integer modulo by zero"),
                _ => None,
            };
            if let Some(message) = message {
                Python::attach(|py| {
                    renderable::report_error(py, PyZeroDivisionError::new_err(message))
                });
                return Vec::new();
            }
        }
        self.columns.rich_render(console, options)
    }

    fn measure(
        &self,
        _console: &rich::console::Console,
        options: &rich::console::ConsoleOptions,
    ) -> rich::measure::Measurement {
        unmeasured(options)
    }
}

#[pymethods]
impl Columns {
    #[new]
    #[pyo3(signature = (
        renderables=None, padding=None, *, width=None, expand=false, equal=false,
        column_first=false, right_to_left=false, align=None, title=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        renderables: Option<&Bound<'_, PyAny>>,
        padding: Option<Bound<'_, PyAny>>,
        width: Option<usize>,
        expand: bool,
        equal: bool,
        column_first: bool,
        right_to_left: bool,
        align: Option<String>,
        title: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let list = PyList::empty(py);
        if let Some(renderables) = renderables.filter(|r| !r.is_none()) {
            for item in renderables.try_iter()? {
                list.append(item?)?;
            }
        }
        let padding = match padding {
            Some(padding) => padding,
            None => PyTuple::new(py, [0, 1])?.into_any(),
        };
        unpack(&padding)?;
        if let Some(align) = &align {
            horizontal(align)?;
        }
        Ok(Columns {
            renderables: list.unbind(),
            width,
            padding: padding.unbind(),
            expand,
            equal,
            column_first,
            right_to_left,
            align,
            title: title.filter(|title| !title.is_none(py)),
        })
    }

    /// Add a renderable to the columns.
    fn add_renderable(&self, py: Python<'_>, renderable: Py<PyAny>) -> PyResult<()> {
        self.renderables.bind(py).append(renderable)
    }

    #[getter]
    fn renderables(&self, py: Python<'_>) -> Py<PyList> {
        self.renderables.clone_ref(py)
    }

    #[setter]
    fn set_renderables(&mut self, py: Python<'_>, renderables: &Bound<'_, PyAny>) -> PyResult<()> {
        let list = PyList::empty(py);
        for item in renderables.try_iter()? {
            list.append(item?)?;
        }
        self.renderables = list.unbind();
        Ok(())
    }

    #[getter]
    fn padding(&self, py: Python<'_>) -> Py<PyAny> {
        self.padding.clone_ref(py)
    }

    #[setter]
    fn set_padding(&mut self, padding: Bound<'_, PyAny>) -> PyResult<()> {
        unpack(&padding)?;
        self.padding = padding.unbind();
        Ok(())
    }

    #[getter]
    fn align(&self) -> Option<String> {
        self.align.clone()
    }

    #[setter]
    fn set_align(&mut self, align: Option<String>) -> PyResult<()> {
        if let Some(align) = &align {
            horizontal(align)?;
        }
        self.align = align;
        Ok(())
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderables)?;
        visit.call(&self.padding)?;
        if let Some(title) = &self.title {
            visit.call(title)?;
        }
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Columns>(m)
}
