//! `rich.columns.Columns`. Port of upstream `rich/columns.py`.
//!
//! Core's `Columns` has no `width`, `column_first`, `right_to_left`,
//! `align`, `title` or custom padding, so the layout is ported here; the
//! columns are laid out, as upstream does, in core's box-less
//! `Table::grid`.

use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::{PyList, PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::table::{Cell, Table as CoreTable};
use rich::Text as CoreText;

use crate::errors::MarkupError;
use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::text::Text;

use super::align::{horizontal, AlignRender, Horizontal};
use super::constrain::ConstrainRender;
use super::padding::{unpack, Pad};
use super::{text_markup, unmeasured};

type Shared = Arc<dyn Renderable + Send + Sync>;

enum Item {
    /// A `str`, which upstream turns into `console.render_str(item)`.
    Markup(String),
    Shared(Shared),
}

struct ColumnsRender {
    items: Vec<Item>,
    padding: Pad,
    width: Option<usize>,
    expand: bool,
    equal: bool,
    column_first: bool,
    right_to_left: bool,
    align: Option<Horizontal>,
    title: Option<String>,
}

/// `iter_renderables(column_count)`: item indices in the order the grid
/// fills, with `None` for the blanks that complete the last row.
fn order(item_count: usize, column_count: usize, column_first: bool) -> Vec<Option<usize>> {
    let mut sequence: Vec<Option<usize>> = Vec::with_capacity(item_count + column_count);
    if column_first {
        let mut column_lengths = vec![item_count / column_count; column_count];
        for length in column_lengths.iter_mut().take(item_count % column_count) {
            *length += 1;
        }
        let row_count = item_count.div_ceil(column_count);
        let mut cells = vec![vec![None; column_count]; row_count];
        let (mut row, mut col) = (0, 0);
        for index in 0..item_count {
            cells[row][col] = Some(index);
            column_lengths[col] -= 1;
            if column_lengths[col] > 0 {
                row += 1;
            } else {
                col += 1;
                row = 0;
            }
        }
        for index in cells.into_iter().flatten() {
            match index {
                Some(index) => sequence.push(Some(index)),
                None => break,
            }
        }
    } else {
        sequence.extend((0..item_count).map(Some));
    }
    if !item_count.is_multiple_of(column_count) {
        sequence.extend(std::iter::repeat_n(
            None,
            column_count - item_count % column_count,
        ));
    }
    sequence
}

impl Renderable for ColumnsRender {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let items: Vec<Shared> = self
            .items
            .iter()
            .map(|item| match item {
                Item::Markup(markup) => Arc::new(console.render_str(markup, None)) as Shared,
                Item::Shared(shared) => shared.clone(),
            })
            .collect();
        if items.is_empty() {
            return Vec::new();
        }
        let (top, right, bottom, left) = self.padding;
        let width_padding = left.max(right);
        let max_width = options.max_width;
        let mut widths: Vec<usize> = items
            .iter()
            .map(|item| CoreMeasurement::get(console, options, item.as_ref()).maximum)
            .collect();
        if self.equal {
            let widest = widths.iter().copied().max().unwrap_or(0);
            widths = vec![widest; widths.len()];
        }

        let mut table = CoreTable::grid()
            .padding(top, right, bottom, left)
            .collapse_padding(true)
            .pad_edge(false)
            .expand(self.expand);
        if let Some(title) = &self.title {
            table = table.title(title.clone());
        }
        let mut column_count = items.len();
        if let Some(width) = self.width {
            column_count = (max_width / (width + width_padding)).max(1);
            for _ in 0..column_count {
                table.add_column("");
                table.column_width(width);
            }
        } else {
            while column_count > 1 {
                let mut column_widths: Vec<usize> = Vec::new();
                let mut column_no = 0;
                let mut fits = true;
                for index in order(items.len(), column_count, self.column_first) {
                    let width = index.map_or(0, |index| widths[index]);
                    if column_no == column_widths.len() {
                        column_widths.push(width);
                    } else {
                        column_widths[column_no] = column_widths[column_no].max(width);
                    }
                    let total = column_widths.iter().sum::<usize>()
                        + width_padding * (column_widths.len() - 1);
                    if total > max_width {
                        column_count = column_widths.len() - 1;
                        fits = false;
                        break;
                    }
                    column_no = (column_no + 1) % column_count;
                }
                if fits {
                    break;
                }
            }
            column_count = column_count.max(1);
            for _ in 0..column_count {
                table.add_column("");
            }
        }

        let cells: Vec<Cell> = order(items.len(), column_count, self.column_first)
            .into_iter()
            .map(|index| {
                let Some(index) = index else {
                    return Cell::Markup(String::new());
                };
                let mut item = items[index].clone();
                if self.equal {
                    item = Arc::new(ConstrainRender {
                        child: item,
                        width: Some(widths[0]),
                    });
                }
                if let Some(align) = self.align {
                    item = Arc::new(AlignRender::new(item, align));
                }
                Cell::Renderable(item)
            })
            .collect();
        for row in cells.chunks(column_count) {
            let mut row = row.to_vec();
            if self.right_to_left {
                row.reverse();
            }
            table.add_row_cells(row);
        }
        table.rich_render(console, options)
    }

    fn measure(&self, _console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        unmeasured(options)
    }
}

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
        let mut items = Vec::new();
        for item in self.renderables.bind(py).iter() {
            if let Ok(markup) = item.cast::<PyString>() {
                let markup = markup.to_cow()?.into_owned();
                CoreText::from_markup(&markup).map_err(|e| MarkupError::new_err(e.to_string()))?;
                items.push(Item::Markup(markup));
            } else if let Ok(text) = item.extract::<PyRef<'_, Text>>() {
                items.push(Item::Shared(Arc::new(text.inner.clone())));
            } else {
                // Checked now, so a non-renderable raises from `print`. The
                // grid renders its cells with `highlight=False`.
                renderable::to_renderable(&item, Some(false))?;
                items.push(Item::Shared(PyRenderable::shared(
                    item.unbind(),
                    Some(false),
                )));
            }
        }
        let title = match &self.title {
            None => None,
            Some(title) => {
                let title = title.bind(py);
                if let Ok(text) = title.extract::<PyRef<'_, Text>>() {
                    Some(text_markup(&text.inner))
                } else {
                    Some(title.str()?.to_cow()?.into_owned())
                }
            }
        };
        Ok(Box::new(ColumnsRender {
            items,
            padding: unpack(self.padding.bind(py))?,
            width: self.width,
            expand: self.expand,
            equal: self.equal,
            column_first: self.column_first,
            right_to_left: self.right_to_left,
            align: self.align.as_deref().map(horizontal).transpose()?,
            title,
        }))
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
