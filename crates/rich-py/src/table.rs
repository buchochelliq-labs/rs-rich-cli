//! `rich.table`: the `Table` class.
//!
//! Owner: the foundation (the static-renderables area may extend it: `grid`,
//! `Column`, more options).

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::protocol::Renderable;
use rich::r#box::Box as CoreBox;
use rich::{Cell, Justify, Overflow, Style as CoreStyle, StyleType};
use rich::{Table as CoreTable, Text as CoreText};

use crate::boxes::BoxArg;
use crate::convert;
use crate::errors::NotRenderableError;
use crate::limits::{check_size, MAX_COLUMN_RATIO, MAX_COLUMN_WIDTH, MAX_CONSOLE_HEIGHT};
use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::style::{py_style_type, resolved_style, style_type};
use crate::text::Text;

struct ColumnSpec {
    header: CellSpec,
    footer: CellSpec,
    footer_style: Option<CoreStyle>,
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

/// A row: its cells, and `add_row`'s `style` and `end_section`.
struct RowSpec {
    cells: Vec<CellSpec>,
    style: Option<StyleType>,
    end_section: bool,
}

/// A title or caption: console markup, or a `Text`.
enum Annotation {
    Markup(String),
    Text(CoreText),
}

fn annotation(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Annotation>> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(None);
    };
    if let Ok(text) = value.extract::<PyRef<'_, Text>>() {
        return Ok(Some(Annotation::Text(text.inner.clone())));
    }
    match value.extract::<String>() {
        Ok(markup) => Ok(Some(Annotation::Markup(markup))),
        Err(_) => Err(PyTypeError::new_err(
            "a title or caption must be a str or a Text",
        )),
    }
}

/// A title or caption as Rich holds it: the `str` or a copy of the `Text`.
fn annotation_value(py: Python<'_>, value: Option<&Annotation>) -> PyResult<Option<Py<PyAny>>> {
    Ok(match value {
        None => None,
        Some(Annotation::Markup(markup)) => Some(PyString::new(py, markup).into_any().unbind()),
        Some(Annotation::Text(text)) => {
            Some(Py::new(py, Text::from_core(text.clone()))?.into_any())
        }
    })
}

/// A style attribute's value: `None`, a `str` or a `Style`.
fn style_value(py: Python<'_>, value: Option<&StyleType>) -> PyResult<Option<Py<PyAny>>> {
    value.map(|style| py_style_type(py, style)).transpose()
}

/// A `style=` / `border_style=` argument, checked now as before (a name
/// must parse as a style definition); resolved when the table prints.
fn checked_style(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<StyleType>> {
    resolved_style(value)?;
    style_type(value)
}

/// A checked style, resolved.
fn resolve(value: &Option<StyleType>) -> PyResult<Option<CoreStyle>> {
    Ok(match value {
        Some(StyleType::Style(style)) => Some(style.clone()),
        Some(StyleType::Name(name)) => Some(crate::style::parse_style(name)?),
        None => None,
    })
}

/// `row_styles=`: any iterable of styles.
fn row_styles_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<StyleType>> {
    match value.filter(|v| !v.is_none()) {
        None => Ok(Vec::new()),
        Some(styles) => styles
            .try_iter()?
            .map(|style| {
                let style = style?;
                Ok(style_type(Some(&style))?.unwrap_or_default())
            })
            .collect(),
    }
}

/// A table `width` / `min_width`: at most the widest console.
fn check_width(name: &str, value: Option<usize>) -> PyResult<Option<usize>> {
    if let Some(value) = value.filter(|value| *value > MAX_COLUMN_WIDTH) {
        return Err(PyValueError::new_err(format!(
            "{name} must be at most {MAX_COLUMN_WIDTH}, got {value}"
        )));
    }
    Ok(value)
}

/// `leading`: blank lines between rows, each as wide as the table. Rich
/// builds them all, so a huge value is its `MemoryError`.
fn check_leading(value: usize) -> PyResult<usize> {
    check_size("leading", value, MAX_CONSOLE_HEIGHT)
}

/// A cell, header or footer: `str` (console markup), `Text`, `None` (empty)
/// or any other renderable.
fn cell_spec(cell: &Bound<'_, PyAny>) -> PyResult<CellSpec> {
    if let Ok(text) = cell.extract::<PyRef<'_, Text>>() {
        Ok(CellSpec::Text(text.inner.clone()))
    } else if cell.is_instance_of::<PyString>() {
        Ok(CellSpec::Markup(cell.extract()?))
    } else if cell.is_none() {
        Ok(CellSpec::Markup(String::new()))
    } else if renderable::is_renderable(cell)? {
        Ok(CellSpec::Object(cell.clone().unbind()))
    } else {
        Err(NotRenderableError::new_err(format!(
            "unable to render {}; a string or other renderable object is required",
            cell.get_type().name()?
        )))
    }
}

/// A `header_style=` / `footer_style=` argument: not given (Rich's
/// `"table.header"` / `"table.footer"`), or a style, where `None` means none
/// at all (`header_style or ""`). PyO3 maps a missing argument and `None`
/// alike, so this type tells them apart.
enum RowStyleArg {
    Default,
    Style(StyleType),
}

impl<'a, 'py> FromPyObject<'a, 'py> for RowStyleArg {
    type Error = PyErr;

    fn extract(value: pyo3::Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        let value = value.to_owned();
        Ok(RowStyleArg::Style(
            style_type(Some(&value))?.unwrap_or_else(|| StyleType::Name(String::new())),
        ))
    }
}

impl RowStyleArg {
    fn or(self, default: &str) -> StyleType {
        match self {
            RowStyleArg::Default => StyleType::Name(default.to_string()),
            RowStyleArg::Style(style) => style,
        }
    }
}

/// `rich.table.Table`: columns and rows of renderables.
#[pyclass(name = "Table", module = "rs_rich.table")]
pub(crate) struct Table {
    columns: Vec<ColumnSpec>,
    rows: Vec<RowSpec>,
    title: Option<Annotation>,
    caption: Option<Annotation>,
    width: Option<usize>,
    min_width: Option<usize>,
    #[pyo3(get, set)]
    show_footer: bool,
    leading: usize,
    row_styles: Vec<StyleType>,
    header_style: StyleType,
    footer_style: StyleType,
    title_style: Option<StyleType>,
    caption_style: Option<StyleType>,
    title_justify: Justify,
    caption_justify: Justify,
    #[pyo3(get, set)]
    safe_box: Option<bool>,
    box_set: Option<CoreBox>,
    #[pyo3(get, set)]
    show_header: bool,
    #[pyo3(get, set)]
    show_lines: bool,
    #[pyo3(get, set)]
    show_edge: bool,
    #[pyo3(get, set)]
    expand: bool,
    border_style: Option<StyleType>,
    padding: (usize, usize, usize, usize),
    #[pyo3(get, set)]
    collapse_padding: bool,
    #[pyo3(get, set)]
    pad_edge: bool,
    style: Option<StyleType>,
    #[pyo3(get, set)]
    highlight: bool,
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

impl CellSpec {
    /// The core cell, for a render: a `str` goes through Rich's
    /// `render_str` then, so it is checked (or taken literally) by the
    /// print's `markup`.
    fn to_cell(&self, py: Python<'_>, highlight: bool) -> PyResult<Cell> {
        Ok(match self {
            CellSpec::Markup(markup) => {
                Cell::Markup(renderable::render_str_markup(markup.clone())?)
            }
            CellSpec::Text(text) => Cell::Text(text.clone()),
            // Rich renders cells with the table's `highlight`.
            CellSpec::Object(object) => {
                Cell::Renderable(PyRenderable::shared(object.clone_ref(py), Some(highlight)))
            }
        })
    }
}

impl Table {
    fn build(&self, py: Python<'_>) -> PyResult<CoreTable> {
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
            .highlight(self.highlight)
            .width(self.width)
            .min_width(self.min_width)
            .show_footer(self.show_footer)
            .leading(self.leading)
            .row_styles(self.row_styles.clone())
            .header_style(self.header_style.clone())
            .footer_style(self.footer_style.clone())
            .title_justify(self.title_justify)
            .caption_justify(self.caption_justify)
            .safe_box(self.safe_box);
        if let Some(style) = &self.title_style {
            table = table.title_style(style.clone());
        }
        if let Some(style) = &self.caption_style {
            table = table.caption_style(style.clone());
        }
        if let Some(style) = resolve(&self.style)? {
            table = table.style(style);
        }
        if let Some(style) = resolve(&self.border_style)? {
            table = table.border_style(style);
        }
        match &self.title {
            Some(Annotation::Markup(title)) => {
                table = table.title(renderable::render_str_markup(title.clone())?)
            }
            Some(Annotation::Text(title)) => table = table.title_text(title.clone()),
            None => {}
        }
        match &self.caption {
            Some(Annotation::Markup(caption)) => {
                table = table.caption(renderable::render_str_markup(caption.clone())?)
            }
            Some(Annotation::Text(caption)) => table = table.caption_text(caption.clone()),
            None => {}
        }
        for column in &self.columns {
            table.add_column_cell(column.header.to_cell(py, self.highlight)?, column.justify);
            table.column_footer(column.footer.to_cell(py, self.highlight)?);
            if let Some(style) = &column.footer_style {
                table.column_footer_fill(style.clone());
            }
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
                .cells
                .iter()
                .map(|cell| cell.to_cell(py, self.highlight))
                .collect::<PyResult<Vec<_>>>()?;
            table.add_row_with(cells, row.style.clone(), row.end_section);
        }
        Ok(table)
    }
}

impl AsRenderable for Table {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.build(py)?))
    }
}

#[pymethods]
impl Table {
    #[new]
    #[pyo3(signature = (
        *headers, title=None, caption=None, width=None, min_width=None, r#box=BoxArg::Default,
        safe_box=None, padding=None, collapse_padding=false, pad_edge=true, expand=false,
        show_header=true, show_footer=false, show_edge=true, show_lines=false, leading=0,
        style=None, row_styles=None, header_style=RowStyleArg::Default,
        footer_style=RowStyleArg::Default, border_style=None,
        title_style=None, caption_style=None, title_justify="center", caption_justify="center",
        highlight=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        headers: &Bound<'_, PyTuple>,
        title: Option<&Bound<'_, PyAny>>,
        caption: Option<&Bound<'_, PyAny>>,
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
        header_style: RowStyleArg,
        footer_style: RowStyleArg,
        border_style: Option<&Bound<'_, PyAny>>,
        title_style: Option<&Bound<'_, PyAny>>,
        caption_style: Option<&Bound<'_, PyAny>>,
        title_justify: &str,
        caption_justify: &str,
        highlight: bool,
    ) -> PyResult<Self> {
        check_width("width", width)?;
        check_width("min_width", min_width)?;
        check_leading(leading)?;
        let row_styles = row_styles_arg(row_styles)?;
        let mut table = Table {
            columns: Vec::new(),
            rows: Vec::new(),
            title: annotation(title)?,
            caption: annotation(caption)?,
            width,
            min_width,
            show_footer,
            leading,
            row_styles,
            header_style: header_style.or("table.header"),
            footer_style: footer_style.or("table.footer"),
            title_style: style_type(title_style)?,
            caption_style: style_type(caption_style)?,
            title_justify: convert::justify(Some(title_justify))?,
            caption_justify: convert::justify(Some(caption_justify))?,
            safe_box,
            box_set: r#box.or(rich::r#box::HEAVY_HEAD),
            show_header,
            show_lines,
            show_edge,
            expand,
            border_style: checked_style(border_style)?,
            padding: match padding {
                Some(value) if !value.is_none() => convert::padding(value)?,
                _ => (0, 1, 0, 1),
            },
            collapse_padding,
            pad_edge,
            style: checked_style(style)?,
            highlight,
        };
        for header in headers.iter() {
            table.add_header(&header)?;
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
            width: None,
            min_width: None,
            show_footer: false,
            leading: 0,
            row_styles: Vec::new(),
            header_style: StyleType::Name("table.header".to_string()),
            footer_style: StyleType::Name("table.footer".to_string()),
            title_style: None,
            caption_style: None,
            title_justify: Justify::Center,
            caption_justify: Justify::Center,
            safe_box: None,
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
            table.add_header(&header)?;
        }
        Ok(table)
    }

    /// Add a column. A `str` header or footer is console markup; either
    /// may be any renderable.
    #[pyo3(signature = (
        header=None, footer=None, *, header_style=None, highlight=None, footer_style=None,
        style=None, justify="left", vertical="top", overflow="ellipsis", width=None,
        min_width=None, max_width=None, ratio=None, no_wrap=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn add_column(
        &mut self,
        header: Option<&Bound<'_, PyAny>>,
        footer: Option<&Bound<'_, PyAny>>,
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
        let spec = |value: Option<&Bound<'_, PyAny>>| match value {
            Some(value) => cell_spec(value),
            None => Ok(CellSpec::Markup(String::new())),
        };
        self.columns.push(ColumnSpec {
            header: spec(header)?,
            footer: spec(footer)?,
            footer_style: resolved_style(footer_style)?,
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
        let mut cells = Vec::new();
        for cell in renderables.iter() {
            cells.push(cell_spec(&cell)?);
        }
        // A row longer than the table adds columns, as Rich's does.
        while self.columns.len() < cells.len() {
            self.columns
                .push(ColumnSpec::new(CellSpec::Markup(String::new())));
            if let Some(column) = self.columns.last_mut() {
                column.highlight = Some(self.highlight);
            }
        }
        self.rows.push(RowSpec {
            cells,
            style: style_type(style)?,
            end_section,
        });
        Ok(())
    }

    /// `add_section()`: draw a line beneath the last row.
    fn add_section(&mut self) {
        if let Some(row) = self.rows.last_mut() {
            row.end_section = true;
        }
    }

    #[getter]
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    // Rich's attributes. The table is a specification converted when it
    // prints, so each setter only updates the specification.

    #[getter]
    fn title(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        annotation_value(py, self.title.as_ref())
    }

    #[setter]
    fn set_title(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.title = annotation(value)?;
        Ok(())
    }

    #[getter]
    fn caption(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        annotation_value(py, self.caption.as_ref())
    }

    #[setter]
    fn set_caption(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.caption = annotation(value)?;
        Ok(())
    }

    #[getter]
    fn width(&self) -> Option<usize> {
        self.width
    }

    #[setter]
    fn set_width(&mut self, value: Option<usize>) -> PyResult<()> {
        self.width = check_width("width", value)?;
        Ok(())
    }

    #[getter]
    fn min_width(&self) -> Option<usize> {
        self.min_width
    }

    #[setter]
    fn set_min_width(&mut self, value: Option<usize>) -> PyResult<()> {
        self.min_width = check_width("min_width", value)?;
        Ok(())
    }

    #[getter(r#box)]
    fn get_box(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.box_set.map(|b| crate::boxes::to_py(py, b)).transpose()
    }

    #[setter(r#box)]
    fn set_box(&mut self, value: BoxArg) {
        self.box_set = value.or(rich::r#box::HEAVY_HEAD);
    }

    #[getter]
    fn padding(&self) -> (usize, usize, usize, usize) {
        self.padding
    }

    #[setter]
    fn set_padding(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.padding = convert::padding(value)?;
        Ok(())
    }

    #[getter]
    fn leading(&self) -> usize {
        self.leading
    }

    #[setter]
    fn set_leading(&mut self, value: usize) -> PyResult<()> {
        self.leading = check_leading(value)?;
        Ok(())
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        // Rich's default is `"none"`.
        match &self.style {
            Some(style) => py_style_type(py, style),
            None => Ok(PyString::new(py, "none").into_any().unbind()),
        }
    }

    #[setter]
    fn set_style(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.style = checked_style(value)?;
        Ok(())
    }

    #[getter]
    fn border_style(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        style_value(py, self.border_style.as_ref())
    }

    #[setter]
    fn set_border_style(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.border_style = checked_style(value)?;
        Ok(())
    }

    #[getter]
    fn row_styles(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.row_styles
            .iter()
            .map(|style| py_style_type(py, style))
            .collect()
    }

    #[setter]
    fn set_row_styles(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.row_styles = row_styles_arg(value)?;
        Ok(())
    }

    #[getter]
    fn header_style(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        py_style_type(py, &self.header_style)
    }

    #[setter]
    fn set_header_style(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.header_style = style_type(value)?.unwrap_or_else(|| StyleType::Name(String::new()));
        Ok(())
    }

    #[getter]
    fn footer_style(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        py_style_type(py, &self.footer_style)
    }

    #[setter]
    fn set_footer_style(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.footer_style = style_type(value)?.unwrap_or_else(|| StyleType::Name(String::new()));
        Ok(())
    }

    #[getter]
    fn title_style(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        style_value(py, self.title_style.as_ref())
    }

    #[setter]
    fn set_title_style(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.title_style = style_type(value)?;
        Ok(())
    }

    #[getter]
    fn caption_style(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        style_value(py, self.caption_style.as_ref())
    }

    #[setter]
    fn set_caption_style(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.caption_style = style_type(value)?;
        Ok(())
    }

    #[getter]
    fn title_justify(&self) -> &'static str {
        convert::justify_name(self.title_justify).unwrap_or("default")
    }

    #[setter]
    fn set_title_justify(&mut self, value: &str) -> PyResult<()> {
        self.title_justify = convert::justify(Some(value))?;
        Ok(())
    }

    #[getter]
    fn caption_justify(&self) -> &'static str {
        convert::justify_name(self.caption_justify).unwrap_or("default")
    }

    #[setter]
    fn set_caption_justify(&mut self, value: &str) -> PyResult<()> {
        self.caption_justify = convert::justify(Some(value))?;
        Ok(())
    }

    // A cell can refer back to the table (`holder.table = table`).
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        let columns = self
            .columns
            .iter()
            .flat_map(|column| [&column.header, &column.footer]);
        for cell in self.rows.iter().flat_map(|row| &row.cells).chain(columns) {
            if let CellSpec::Object(object) = cell {
                visit.call(object)?;
            }
        }
        Ok(())
    }

    fn __clear__(&mut self) {
        let columns = self
            .columns
            .iter_mut()
            .flat_map(|column| [&mut column.header, &mut column.footer]);
        let rows = self.rows.iter_mut().flat_map(|row| row.cells.iter_mut());
        for cell in rows.chain(columns) {
            if matches!(cell, CellSpec::Object(_)) {
                *cell = CellSpec::Markup(String::new());
            }
        }
    }
}

impl ColumnSpec {
    /// A column with every default.
    fn new(header: CellSpec) -> Self {
        ColumnSpec {
            header,
            footer: CellSpec::Markup(String::new()),
            footer_style: None,
            justify: Justify::Left,
            style: None,
            header_style: None,
            overflow: Overflow::Ellipsis,
            width: None,
            min_width: None,
            max_width: None,
            ratio: None,
            no_wrap: false,
            vertical: rich::align::VerticalAlign::Top,
            highlight: None,
        }
    }
}

impl Table {
    /// `add_column(header)` with every default.
    fn add_header(&mut self, header: &Bound<'_, PyAny>) -> PyResult<()> {
        self.columns.push(ColumnSpec::new(cell_spec(header)?));
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Table>(m)
}
