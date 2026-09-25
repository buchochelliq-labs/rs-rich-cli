//! `rich.table`: the `Table` class.
//!
//! Owner: the foundation (the static-renderables area may extend it: `grid`,
//! `Column`, more options).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::protocol::Renderable;
use rich::r#box::Box as CoreBox;
use rich::{Cell, Justify, Overflow, Style as CoreStyle};
use rich::{Table as CoreTable, Text as CoreText};

use crate::boxes::BoxArg;
use crate::convert;
use crate::errors::NotRenderableError;
use crate::limits::{MAX_COLUMN_RATIO, MAX_COLUMN_WIDTH};
use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::style::resolved_style;
use crate::text::Text;

struct ColumnSpec {
    header: String,
    justify: Justify,
    style: Option<CoreStyle>,
    header_style: Option<CoreStyle>,
    overflow: Overflow,
    width: Option<usize>,
    min_width: Option<usize>,
    max_width: Option<usize>,
    ratio: Option<usize>,
    no_wrap: bool,
}

enum CellSpec {
    Markup(String),
    Text(CoreText),
    /// Any other renderable, rendered through the bridge when printed.
    Object(Py<PyAny>),
}

/// `rich.table.Table`: columns and rows of renderables.
#[pyclass(name = "Table", module = "rs_rich.table")]
pub(crate) struct Table {
    columns: Vec<ColumnSpec>,
    rows: Vec<Vec<CellSpec>>,
    title: Option<String>,
    caption: Option<String>,
    box_set: Option<CoreBox>,
    show_header: bool,
    show_lines: bool,
    show_edge: bool,
    expand: bool,
    border_style: Option<CoreStyle>,
}

impl Table {
    fn build(&self, py: Python<'_>) -> CoreTable {
        let mut table = CoreTable::new();
        table = match self.box_set {
            Some(box_set) => table.box_set(box_set),
            None => table.without_box(),
        };
        table = table
            .show_header(self.show_header)
            .show_lines(self.show_lines)
            .show_edge(self.show_edge)
            .expand(self.expand);
        if let Some(style) = &self.border_style {
            table = table.border_style(style.clone());
        }
        if let Some(title) = &self.title {
            table = table.title(title.clone());
        }
        if let Some(caption) = &self.caption {
            table = table.caption(caption.clone());
        }
        for column in &self.columns {
            table.add_column_justify(column.header.clone(), column.justify);
            if let Some(style) = &column.style {
                table.column_style(style.clone());
            }
            // Rich's `header_style` styles the whole header cell.
            if let Some(style) = &column.header_style {
                table.column_header_fill(style.clone());
            }
            table.column_overflow(column.overflow);
            if let Some(width) = column.width {
                table.column_width(width);
            }
            if let Some(width) = column.min_width {
                table.column_min_width(width);
            }
            if let Some(width) = column.max_width {
                table.column_max_width(width);
            }
            if let Some(ratio) = column.ratio {
                table.column_ratio(ratio);
            }
            if column.no_wrap {
                table.column_no_wrap();
            }
        }
        for row in &self.rows {
            let cells = row
                .iter()
                .map(|cell| match cell {
                    CellSpec::Markup(markup) => Cell::Markup(markup.clone()),
                    CellSpec::Text(text) => Cell::Text(text.clone()),
                    CellSpec::Object(object) => {
                        // Rich's `Table(highlight=False)` renders cells unhighlighted.
                        Cell::Renderable(PyRenderable::shared(object.clone_ref(py), Some(false)))
                    }
                })
                .collect();
            table.add_row_cells(cells);
        }
        table
    }
}

impl AsRenderable for Table {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.build(py)))
    }
}

#[pymethods]
impl Table {
    #[new]
    #[pyo3(signature = (
        *headers, title=None, caption=None, r#box=BoxArg::Default, show_header=true,
        show_lines=false, show_edge=true, expand=false, border_style=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        headers: &Bound<'_, PyTuple>,
        title: Option<String>,
        caption: Option<String>,
        r#box: BoxArg,
        show_header: bool,
        show_lines: bool,
        show_edge: bool,
        expand: bool,
        border_style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut table = Table {
            columns: Vec::new(),
            rows: Vec::new(),
            title,
            caption,
            box_set: r#box.or(rich::r#box::HEAVY_HEAD),
            show_header,
            show_lines,
            show_edge,
            expand,
            border_style: resolved_style(border_style)?,
        };
        for header in headers.iter() {
            table.add_column(
                &header.extract::<String>()?,
                None,
                None,
                "left",
                "ellipsis",
                None,
                None,
                None,
                None,
                false,
            )?;
        }
        Ok(table)
    }

    /// Add a column. The header is console markup.
    #[pyo3(signature = (
        header="", *, style=None, header_style=None, justify="left", overflow="ellipsis",
        width=None, min_width=None, max_width=None, ratio=None, no_wrap=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_column(
        &mut self,
        header: &str,
        style: Option<&Bound<'_, PyAny>>,
        header_style: Option<&Bound<'_, PyAny>>,
        justify: &str,
        overflow: &str,
        width: Option<usize>,
        min_width: Option<usize>,
        max_width: Option<usize>,
        ratio: Option<usize>,
        no_wrap: bool,
    ) -> PyResult<()> {
        // Larger values than these make core overflow or take minutes, and
        // no terminal is that wide anyway.
        for (name, value, limit) in [
            ("width", width, MAX_COLUMN_WIDTH),
            ("min_width", min_width, MAX_COLUMN_WIDTH),
            ("max_width", max_width, MAX_COLUMN_WIDTH),
            ("ratio", ratio, MAX_COLUMN_RATIO),
        ] {
            if let Some(value) = value.filter(|value| *value > limit) {
                return Err(PyValueError::new_err(format!(
                    "{name} must be at most {limit}, got {value}"
                )));
            }
        }
        self.columns.push(ColumnSpec {
            header: header.to_string(),
            justify: convert::justify(Some(justify))?,
            style: resolved_style(style)?,
            header_style: resolved_style(header_style)?,
            overflow: convert::overflow(overflow)?,
            width,
            min_width,
            max_width,
            ratio,
            no_wrap,
        });
        Ok(())
    }

    /// Add a row of renderables: `str` (console markup), `Text`, `None`
    /// (an empty cell) or any other renderable.
    #[pyo3(signature = (*renderables))]
    fn add_row(&mut self, renderables: &Bound<'_, PyTuple>) -> PyResult<()> {
        let mut row = Vec::new();
        for cell in renderables.iter() {
            if let Ok(text) = cell.extract::<PyRef<'_, Text>>() {
                row.push(CellSpec::Text(text.inner.clone()));
            } else if cell.is_instance_of::<PyString>() {
                row.push(CellSpec::Markup(cell.extract()?));
            } else if cell.is_none() {
                row.push(CellSpec::Markup(String::new()));
            } else if renderable::is_renderable(&cell)? {
                row.push(CellSpec::Object(cell.unbind()));
            } else {
                return Err(NotRenderableError::new_err(format!(
                    "unable to render {}; a string or other renderable object is required",
                    cell.get_type().name()?
                )));
            }
        }
        if row.len() > self.columns.len() {
            return Err(PyValueError::new_err(format!(
                "too many values in row ({} > {} columns)",
                row.len(),
                self.columns.len()
            )));
        }
        self.rows.push(row);
        Ok(())
    }

    #[getter]
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    // A cell can refer back to the table (`holder.table = table`).
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for row in &self.rows {
            for cell in row {
                if let CellSpec::Object(object) = cell {
                    visit.call(object)?;
                }
            }
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        for row in &mut self.rows {
            for cell in row.iter_mut() {
                if matches!(cell, CellSpec::Object(_)) {
                    *cell = CellSpec::Markup(String::new());
                }
            }
        }
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Table>(m)
}
