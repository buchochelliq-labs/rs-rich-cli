//! `rich.table`: the `Table` class.
//!
//! Owner: the foundation (the static-renderables area may extend it: `grid`,
//! `Column`, more options).

use pyo3::exceptions::{PyNotImplementedError, PyValueError};
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
    vertical: rich::align::VerticalAlign,
    highlight: Option<bool>,
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
    padding: (usize, usize, usize, usize),
    collapse_padding: bool,
    pad_edge: bool,
    style: Option<CoreStyle>,
    highlight: bool,
}

/// Refuse a Rich option core's `Table` has no counterpart for, unless it
/// has its default.
fn unsupported(name: &str, is_default: bool) -> PyResult<()> {
    if is_default {
        return Ok(());
    }
    Err(PyNotImplementedError::new_err(format!(
        "rs_rich's Table does not support {name} yet: core's Table has no such option"
    )))
}

fn vertical(value: &str) -> PyResult<rich::align::VerticalAlign> {
    match value {
        "top" => Ok(rich::align::VerticalAlign::Top),
        "middle" => Ok(rich::align::VerticalAlign::Middle),
        "bottom" => Ok(rich::align::VerticalAlign::Bottom),
        other => Err(PyValueError::new_err(format!(
            "invalid vertical {other:?}; expected top, middle or bottom"
        ))),
    }
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
            .expand(self.expand)
            .padding(
                self.padding.0,
                self.padding.1,
                self.padding.2,
                self.padding.3,
            )
            .collapse_padding(self.collapse_padding)
            .pad_edge(self.pad_edge)
            .highlight(self.highlight);
        if let Some(style) = &self.style {
            table = table.style(style.clone());
        }
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
            table.column_vertical(column.vertical);
            if let Some(highlight) = column.highlight {
                table.column_highlight(highlight);
            }
        }
        for row in &self.rows {
            let cells = row
                .iter()
                .map(|cell| match cell {
                    CellSpec::Markup(markup) => Cell::Markup(markup.clone()),
                    CellSpec::Text(text) => Cell::Text(text.clone()),
                    CellSpec::Object(object) => {
                        // Rich renders cells with the table's `highlight`.
                        Cell::Renderable(PyRenderable::shared(
                            object.clone_ref(py),
                            Some(self.highlight),
                        ))
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
        *headers, title=None, caption=None, width=None, min_width=None, r#box=BoxArg::Default,
        safe_box=None, padding=None, collapse_padding=false, pad_edge=true, expand=false,
        show_header=true, show_footer=false, show_edge=true, show_lines=false, leading=0,
        style=None, row_styles=None, header_style=None, footer_style=None, border_style=None,
        title_style=None, caption_style=None, title_justify="center", caption_justify="center",
        highlight=false
    ))]
    #[allow(clippy::too_many_arguments, unused_variables)]
    fn new(
        headers: &Bound<'_, PyTuple>,
        title: Option<String>,
        caption: Option<String>,
        width: Option<usize>,
        min_width: Option<usize>,
        r#box: BoxArg,
        safe_box: Option<bool>,
        padding: Option<&Bound<'_, PyAny>>,
        collapse_padding: bool,
        pad_edge: bool,
        expand: bool,
        show_header: bool,
        show_footer: bool,
        show_edge: bool,
        show_lines: bool,
        leading: usize,
        style: Option<&Bound<'_, PyAny>>,
        row_styles: Option<&Bound<'_, PyAny>>,
        header_style: Option<&Bound<'_, PyAny>>,
        footer_style: Option<&Bound<'_, PyAny>>,
        border_style: Option<&Bound<'_, PyAny>>,
        title_style: Option<&Bound<'_, PyAny>>,
        caption_style: Option<&Bound<'_, PyAny>>,
        title_justify: &str,
        caption_justify: &str,
        highlight: bool,
    ) -> PyResult<Self> {
        let is_default_style = |value: Option<&Bound<'_, PyAny>>, default: &str| {
            value.is_none_or(|v| v.is_none() || v.extract::<String>().is_ok_and(|s| s == default))
        };
        unsupported("width", width.is_none())?;
        unsupported("min_width", min_width.is_none())?;
        unsupported("show_footer", !show_footer)?;
        unsupported("leading", leading == 0)?;
        unsupported(
            "row_styles",
            row_styles.is_none_or(|v| v.is_none() || !v.is_truthy().unwrap_or(true)),
        )?;
        unsupported(
            "header_style",
            is_default_style(header_style, "table.header"),
        )?;
        unsupported("title_style", is_default_style(title_style, "table.title"))?;
        unsupported(
            "caption_style",
            is_default_style(caption_style, "table.caption"),
        )?;
        unsupported("title_justify", title_justify == "center")?;
        unsupported("caption_justify", caption_justify == "center")?;
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
            padding: match padding {
                Some(value) if !value.is_none() => convert::padding(value)?,
                _ => (0, 1, 0, 1),
            },
            collapse_padding,
            pad_edge,
            style: resolved_style(style)?,
            highlight,
        };
        for header in headers.iter() {
            table.add_header(&header.extract::<String>()?)?;
        }
        Ok(table)
    }

    /// `Table.grid(*headers, padding=0, collapse_padding=True, pad_edge=False,
    /// expand=False)`: a table with no borders or header, for layout.
    #[classmethod]
    #[pyo3(signature = (*headers, padding=None, collapse_padding=true, pad_edge=false, expand=false))]
    fn grid(
        _cls: &Bound<'_, pyo3::types::PyType>,
        headers: &Bound<'_, PyTuple>,
        padding: Option<&Bound<'_, PyAny>>,
        collapse_padding: bool,
        pad_edge: bool,
        expand: bool,
    ) -> PyResult<Self> {
        let mut table = Table {
            columns: Vec::new(),
            rows: Vec::new(),
            title: None,
            caption: None,
            box_set: None,
            show_header: false,
            show_lines: false,
            show_edge: false,
            expand,
            border_style: None,
            padding: match padding {
                Some(value) if !value.is_none() => convert::padding(value)?,
                _ => (0, 0, 0, 0),
            },
            collapse_padding,
            pad_edge,
            style: None,
            highlight: false,
        };
        for header in headers.iter() {
            table.add_header(&header.extract::<String>()?)?;
        }
        Ok(table)
    }

    /// Add a column. The header is console markup.
    #[pyo3(signature = (
        header="", footer="", *, header_style=None, highlight=None, footer_style=None,
        style=None, justify="left", vertical="top", overflow="ellipsis", width=None,
        min_width=None, max_width=None, ratio=None, no_wrap=false
    ))]
    #[allow(clippy::too_many_arguments, unused_variables)]
    fn add_column(
        &mut self,
        header: &str,
        footer: &str,
        header_style: Option<&Bound<'_, PyAny>>,
        highlight: Option<bool>,
        footer_style: Option<&Bound<'_, PyAny>>,
        style: Option<&Bound<'_, PyAny>>,
        justify: &str,
        vertical: &str,
        overflow: &str,
        width: Option<usize>,
        min_width: Option<usize>,
        max_width: Option<usize>,
        ratio: Option<usize>,
        no_wrap: bool,
    ) -> PyResult<()> {
        unsupported("footer", footer.is_empty())?;
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
            vertical: self::vertical(vertical)?,
            highlight,
        });
        Ok(())
    }

    /// Add a row of renderables: `str` (console markup), `Text`, `None`
    /// (an empty cell) or any other renderable.
    #[pyo3(signature = (*renderables, style=None, end_section=false))]
    fn add_row(
        &mut self,
        renderables: &Bound<'_, PyTuple>,
        style: Option<&Bound<'_, PyAny>>,
        end_section: bool,
    ) -> PyResult<()> {
        unsupported("a row style", style.is_none_or(|s| s.is_none()))?;
        unsupported("sections", !end_section)?;
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

    /// Rich's `add_section()`: core's `Table` has no sections.
    fn add_section(&self) -> PyResult<()> {
        unsupported("sections", false)
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

impl Table {
    /// `add_column(header)` with every default.
    fn add_header(&mut self, header: &str) -> PyResult<()> {
        self.add_column(
            header, "", None, None, None, None, "left", "top", "ellipsis", None, None, None, None,
            false,
        )
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Table>(m)
}
