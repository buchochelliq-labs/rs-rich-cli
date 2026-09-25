//! `rs_rich._native`: the Python bindings' only compiled module.
//!
//! Everything that renders is core `rich`. This crate converts Python values
//! into core types and back, and nothing more: a Python `Table` stores what
//! it was given and builds a core `Table` when printed, so nested objects can
//! still change until then, as they can in Python `rich`.

use pyo3::create_exception;
use pyo3::exceptions::{
    PyException, PyNotImplementedError, PyRuntimeError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyFloat, PyInt, PyString, PyTuple};

use rich::align::HorizontalAlign;
use rich::color::ColorSystem;
use rich::console::{Console as CoreConsole, ConsoleOptions};
use rich::measure::Measurement;
use rich::protocol::Renderable;
use rich::r#box::Box as CoreBox;
use rich::{Cell, Justify, Overflow, Panel as CorePanel, Rule, Segment, Style as CoreStyle};
use rich::{StyleType, Table as CoreTable, Text as CoreText};

// Rich's hierarchy: both derive from `ConsoleError(Exception)`.
create_exception!(_native, ConsoleError, PyException);
create_exception!(_native, MarkupError, ConsoleError);
create_exception!(_native, StyleSyntaxError, ConsoleError);

// ---------------------------------------------------------------------------
// Style

/// `rich.style.Style`: attributes, colours and a link.
#[pyclass(name = "Style", module = "rs_rich.style", frozen, skip_from_py_object)]
#[derive(Clone)]
struct Style {
    definition: String,
    inner: CoreStyle,
}

fn parse_style(definition: &str) -> PyResult<CoreStyle> {
    CoreStyle::parse(definition).map_err(|e| StyleSyntaxError::new_err(e.to_string()))
}

#[pymethods]
impl Style {
    #[new]
    #[pyo3(signature = (
        *, color=None, bgcolor=None, bold=None, dim=None, italic=None, underline=None,
        blink=None, reverse=None, conceal=None, strike=None, link=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        color: Option<&str>,
        bgcolor: Option<&str>,
        bold: Option<bool>,
        dim: Option<bool>,
        italic: Option<bool>,
        underline: Option<bool>,
        blink: Option<bool>,
        reverse: Option<bool>,
        conceal: Option<bool>,
        strike: Option<bool>,
        link: Option<&str>,
    ) -> PyResult<Self> {
        // Rich's own style grammar: `bold not italic red on blue link URL`.
        let mut words: Vec<String> = Vec::new();
        for (name, value) in [
            ("bold", bold),
            ("dim", dim),
            ("italic", italic),
            ("underline", underline),
            ("blink", blink),
            ("reverse", reverse),
            ("conceal", conceal),
            ("strike", strike),
        ] {
            match value {
                Some(true) => words.push(name.into()),
                Some(false) => words.push(format!("not {name}")),
                None => {}
            }
        }
        if let Some(color) = color {
            words.push(color.into());
        }
        if let Some(bgcolor) = bgcolor {
            words.push(format!("on {bgcolor}"));
        }
        if let Some(link) = link {
            words.push(format!("link {link}"));
        }
        let definition = words.join(" ");
        let inner = parse_style(&definition)?;
        Ok(Style { definition, inner })
    }

    /// `Style.parse("bold red on white")`.
    #[staticmethod]
    fn parse(definition: &str) -> PyResult<Self> {
        Ok(Style {
            inner: parse_style(definition)?,
            definition: definition.to_string(),
        })
    }

    /// Combine: `other`'s attributes and colours win where it sets them.
    fn __add__(&self, other: &Style) -> Style {
        Style {
            definition: format!("{} {}", self.definition, other.definition)
                .trim()
                .to_string(),
            inner: self.inner.combine(&other.inner),
        }
    }

    fn __eq__(&self, other: &Style) -> bool {
        self.inner == other.inner
    }

    fn __str__(&self) -> String {
        if self.definition.is_empty() {
            "none".into()
        } else {
            self.definition.clone()
        }
    }

    fn __repr__(&self) -> String {
        format!("Style.parse({:?})", self.__str__())
    }
}

/// A `style=` argument: a string (a theme name or a definition) or a `Style`.
fn style_type(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<StyleType>> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(None);
    };
    if let Ok(style) = value.extract::<PyRef<'_, Style>>() {
        return Ok(Some(StyleType::Style(style.inner.clone())));
    }
    if let Ok(name) = value.extract::<String>() {
        return Ok(Some(StyleType::Name(name)));
    }
    Err(PyTypeError::new_err("style must be a str or a Style"))
}

/// A `style=` argument that core wants resolved now (table columns, borders).
fn resolved_style(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreStyle>> {
    Ok(match style_type(value)? {
        Some(StyleType::Style(style)) => Some(style),
        Some(StyleType::Name(name)) => Some(parse_style(&name)?),
        None => None,
    })
}

// ---------------------------------------------------------------------------
// Text

fn justify(value: Option<&str>) -> PyResult<Justify> {
    Ok(match value {
        None | Some("default") => Justify::Default,
        Some("left") => Justify::Left,
        Some("center") => Justify::Center,
        Some("right") => Justify::Right,
        Some("full") => Justify::Full,
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "invalid justify {other:?}; expected left, center, right, full or default"
            )))
        }
    })
}

fn overflow(value: &str) -> PyResult<Overflow> {
    Ok(match value {
        "fold" => Overflow::Fold,
        "crop" => Overflow::Crop,
        "ellipsis" => Overflow::Ellipsis,
        "ignore" => Overflow::Ignore,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid overflow {other:?}; expected fold, crop, ellipsis or ignore"
            )))
        }
    })
}

fn align(value: &str) -> PyResult<HorizontalAlign> {
    Ok(match value {
        "left" => HorizontalAlign::Left,
        "center" => HorizontalAlign::Center,
        "right" => HorizontalAlign::Right,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid align {other:?}; expected left, center or right"
            )))
        }
    })
}

/// `rich.text.Text`: a string with styled spans.
#[pyclass(name = "Text", module = "rs_rich.text", skip_from_py_object)]
#[derive(Clone)]
struct Text {
    inner: CoreText,
}

impl Text {
    /// The byte offset of character `index` (negative counts from the end,
    /// as in Python), clamped to the text.
    fn byte_offset(&self, index: isize) -> usize {
        let plain = self.inner.plain();
        let chars = plain.chars().count() as isize;
        let index = if index < 0 { chars + index } else { index }.clamp(0, chars) as usize;
        plain
            .char_indices()
            .nth(index)
            .map_or(plain.len(), |(offset, _)| offset)
    }
}

#[pymethods]
impl Text {
    #[new]
    #[pyo3(signature = (text="", style=None, *, justify=None, overflow=None, no_wrap=None))]
    fn new(
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
        overflow: Option<&str>,
        no_wrap: Option<bool>,
    ) -> PyResult<Self> {
        let mut inner = match style_type(style)? {
            Some(style) => CoreText::styled(text, style),
            None => CoreText::new(text),
        };
        inner.set_justify(self::justify(justify)?);
        if let Some(value) = overflow {
            inner.set_overflow(Some(self::overflow(value)?));
        }
        inner.set_no_wrap(no_wrap);
        Ok(Text { inner })
    }

    /// `Text.from_markup("[bold]hi[/]")`.
    #[classmethod]
    #[pyo3(signature = (text, *, style=None, justify=None))]
    fn from_markup(
        _cls: &Bound<'_, pyo3::types::PyType>,
        text: &str,
        style: Option<&Bound<'_, PyAny>>,
        justify: Option<&str>,
    ) -> PyResult<Self> {
        let mut inner =
            CoreText::from_markup(text).map_err(|e| MarkupError::new_err(e.to_string()))?;
        if let Some(style) = style_type(style)? {
            inner.set_base_style(style);
        }
        inner.set_justify(self::justify(justify)?);
        Ok(Text { inner })
    }

    #[getter]
    fn plain(&self) -> String {
        self.inner.plain().to_string()
    }

    /// Append a string (with an optional style) or another `Text`.
    #[pyo3(signature = (text, style=None))]
    fn append<'py>(
        mut slf: PyRefMut<'py, Self>,
        text: &Bound<'py, PyAny>,
        style: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        if let Ok(other) = text.extract::<PyRef<'_, Text>>() {
            if style_type(style)?.is_some() {
                return Err(PyValueError::new_err(
                    "style must not be set when appending a Text instance",
                ));
            }
            let joined = slf.inner.clone().append_text(&other.inner);
            slf.inner = joined;
        } else if let Ok(string) = text.extract::<String>() {
            let style = style_type(style)?;
            slf.inner.append(&string, style);
        } else {
            return Err(PyTypeError::new_err(
                "Only str or Text can be appended to Text",
            ));
        }
        Ok(slf)
    }

    /// Style characters `start..end` (Python indices; negative from the end).
    #[pyo3(signature = (style, start=0, end=None))]
    fn stylize(
        &mut self,
        style: &Bound<'_, PyAny>,
        start: isize,
        end: Option<isize>,
    ) -> PyResult<()> {
        let Some(style) = style_type(Some(style))? else {
            return Ok(());
        };
        let start = self.byte_offset(start);
        let end = end.map_or(self.inner.plain().len(), |end| self.byte_offset(end));
        self.inner.stylize(style, start, end);
        Ok(())
    }

    fn __len__(&self) -> usize {
        self.inner.plain().chars().count()
    }

    fn __str__(&self) -> String {
        self.plain()
    }

    fn __repr__(&self) -> String {
        format!("<text {:?}>", self.inner.plain())
    }
}

// ---------------------------------------------------------------------------
// Boxes

/// A box style for tables and panels (`rs_rich.box.ROUNDED`, …).
#[pyclass(name = "Box", module = "rs_rich.box", frozen)]
struct PyBox {
    name: &'static str,
    inner: CoreBox,
}

#[pymethods]
impl PyBox {
    fn __repr__(&self) -> String {
        format!("box.{}", self.name)
    }
}

const BOXES: &[(&str, CoreBox)] = {
    use rich::r#box::*;
    &[
        ("ASCII", ASCII),
        ("ASCII2", ASCII2),
        ("ASCII_DOUBLE_HEAD", ASCII_DOUBLE_HEAD),
        ("SQUARE", SQUARE),
        ("SQUARE_DOUBLE_HEAD", SQUARE_DOUBLE_HEAD),
        ("MINIMAL", MINIMAL),
        ("MINIMAL_HEAVY_HEAD", MINIMAL_HEAVY_HEAD),
        ("MINIMAL_DOUBLE_HEAD", MINIMAL_DOUBLE_HEAD),
        ("SIMPLE", SIMPLE),
        ("SIMPLE_HEAD", SIMPLE_HEAD),
        ("SIMPLE_HEAVY", SIMPLE_HEAVY),
        ("HORIZONTALS", HORIZONTALS),
        ("ROUNDED", ROUNDED),
        ("HEAVY", HEAVY),
        ("HEAVY_EDGE", HEAVY_EDGE),
        ("HEAVY_HEAD", HEAVY_HEAD),
        ("DOUBLE", DOUBLE),
        ("DOUBLE_EDGE", DOUBLE_EDGE),
        ("MARKDOWN", MARKDOWN),
    ]
};

/// A `box=` argument: not given, an explicit `None` (no box), or a box.
/// PyO3 maps both a missing argument and `None` to `Option::None`, so this
/// type tells them apart.
enum BoxArg {
    Default,
    NoBox,
    Box(CoreBox),
}

impl<'a, 'py> FromPyObject<'a, 'py> for BoxArg {
    type Error = PyErr;

    fn extract(value: pyo3::Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        if value.is_none() {
            return Ok(BoxArg::NoBox);
        }
        value
            .extract::<PyRef<'_, PyBox>>()
            .map(|b| BoxArg::Box(b.inner))
            .map_err(|_| PyTypeError::new_err("box must be one of rs_rich.box's constants or None"))
    }
}

impl BoxArg {
    fn or(self, default: CoreBox) -> Option<CoreBox> {
        match self {
            BoxArg::Default => Some(default),
            BoxArg::NoBox => None,
            BoxArg::Box(inner) => Some(inner),
        }
    }
}

// ---------------------------------------------------------------------------
// Renderables

/// A string renderable: console markup, emoji and highlighting apply when
/// it renders, as upstream's `Console.render_str` does for `str` children.
/// `highlight` overrides the console's, as a container's options do upstream
/// (`Panel` renders its child with `highlight=False`).
struct MarkupStr {
    markup: String,
    highlight: Option<bool>,
}

impl Renderable for MarkupStr {
    fn rich_render(&self, console: &CoreConsole, options: &ConsoleOptions) -> Vec<Segment> {
        console
            .render_str(&self.markup, self.highlight)
            .rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &ConsoleOptions) -> Measurement {
        console
            .render_str(&self.markup, self.highlight)
            .measure(console, options)
    }
}

/// Build the core renderable for a Python object. A `str` renders with
/// `highlight` (`None`: the console's setting).
fn renderable(value: &Bound<'_, PyAny>, highlight: Option<bool>) -> PyResult<Box<dyn Renderable>> {
    if value.is_instance_of::<PyString>() {
        return Ok(Box::new(MarkupStr {
            markup: value.extract()?,
            highlight,
        }));
    }
    if let Ok(text) = value.extract::<PyRef<'_, Text>>() {
        return Ok(Box::new(text.inner.clone()));
    }
    if let Ok(table) = value.extract::<PyRef<'_, Table>>() {
        return Ok(Box::new(table.build(value.py())?));
    }
    if let Ok(panel) = value.extract::<PyRef<'_, Panel>>() {
        return Ok(Box::new(panel.build(value.py())?));
    }
    Err(PyNotImplementedError::new_err(format!(
        "rs_rich cannot render {} yet: this first slice supports str, Text, Table and Panel",
        value.get_type().name()?
    )))
}

// ---------------------------------------------------------------------------
// Table

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
}

/// `rich.table.Table`: columns and rows of `str` (markup) or `Text` cells.
#[pyclass(name = "Table", module = "rs_rich.table", unsendable)]
struct Table {
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
    fn build(&self, _py: Python<'_>) -> PyResult<CoreTable> {
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
                })
                .collect();
            table.add_row_cells(cells);
        }
        Ok(table)
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
        self.columns.push(ColumnSpec {
            header: header.to_string(),
            justify: self::justify(Some(justify))?,
            style: resolved_style(style)?,
            header_style: resolved_style(header_style)?,
            overflow: self::overflow(overflow)?,
            width,
            min_width,
            max_width,
            ratio,
            no_wrap,
        });
        Ok(())
    }

    /// Add a row of `str` (console markup) or `Text` cells.
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
            } else {
                return Err(PyNotImplementedError::new_err(
                    "rs_rich table cells are str or Text in this first slice",
                ));
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
}

// ---------------------------------------------------------------------------
// Panel

/// `rich.panel.Panel`: a border around a `str`, `Text`, `Table` or `Panel`.
#[pyclass(name = "Panel", module = "rs_rich.panel", unsendable)]
struct Panel {
    renderable: Py<PyAny>,
    box_set: CoreBox,
    title: Option<String>,
    title_align: HorizontalAlign,
    subtitle: Option<String>,
    subtitle_align: HorizontalAlign,
    expand: bool,
    border_style: Option<CoreStyle>,
    width: Option<usize>,
    padding: (usize, usize, usize, usize),
}

impl Panel {
    fn build(&self, py: Python<'_>) -> PyResult<CorePanel> {
        // Upstream renders a panel's child with `highlight=False`.
        let mut panel = CorePanel::new(renderable(self.renderable.bind(py), Some(false))?)
            .box_set(self.box_set)
            .expand(self.expand)
            .title_align(self.title_align)
            .subtitle_align(self.subtitle_align)
            .padding(self.padding);
        if let Some(title) = &self.title {
            panel = panel.title(title.clone());
        }
        if let Some(subtitle) = &self.subtitle {
            panel = panel.subtitle(subtitle.clone());
        }
        if let Some(style) = &self.border_style {
            panel = panel.border_style(style.clone());
        }
        if let Some(width) = self.width {
            panel = panel.width(width);
        }
        Ok(panel)
    }
}

/// Rich's padding: 1, 2 or 4 integers.
fn padding(value: &Bound<'_, PyAny>) -> PyResult<(usize, usize, usize, usize)> {
    if let Ok(all) = value.extract::<usize>() {
        return Ok((all, all, all, all));
    }
    if let Ok((vertical, horizontal)) = value.extract::<(usize, usize)>() {
        return Ok((vertical, horizontal, vertical, horizontal));
    }
    if let Ok(four) = value.extract::<(usize, usize, usize, usize)>() {
        return Ok(four);
    }
    Err(PyValueError::new_err("padding must be 1, 2 or 4 integers"))
}

#[pymethods]
impl Panel {
    #[new]
    #[pyo3(signature = (
        renderable, r#box=BoxArg::Default, *, title=None, title_align="center", subtitle=None,
        subtitle_align="center", expand=true, border_style=None, width=None, padding=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        renderable: Py<PyAny>,
        r#box: BoxArg,
        title: Option<String>,
        title_align: &str,
        subtitle: Option<String>,
        subtitle_align: &str,
        expand: bool,
        border_style: Option<&Bound<'_, PyAny>>,
        width: Option<usize>,
        padding: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(Panel {
            renderable,
            box_set: r#box
                .or(rich::r#box::ROUNDED)
                .ok_or_else(|| PyValueError::new_err("a Panel needs a box"))?,
            title,
            title_align: align(title_align)?,
            subtitle,
            subtitle_align: align(subtitle_align)?,
            expand,
            border_style: resolved_style(border_style)?,
            width,
            padding: match padding {
                Some(value) => self::padding(value)?,
                None => (0, 1, 0, 1),
            },
        })
    }

    /// `Panel.fit(...)`: a panel that fits its content (`expand=False`).
    #[classmethod]
    #[pyo3(signature = (
        renderable, r#box=BoxArg::Default, *, title=None, title_align="center", subtitle=None,
        subtitle_align="center", border_style=None, width=None, padding=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn fit(
        _cls: &Bound<'_, pyo3::types::PyType>,
        renderable: Py<PyAny>,
        r#box: BoxArg,
        title: Option<String>,
        title_align: &str,
        subtitle: Option<String>,
        subtitle_align: &str,
        border_style: Option<&Bound<'_, PyAny>>,
        width: Option<usize>,
        padding: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Panel::new(
            renderable,
            r#box,
            title,
            title_align,
            subtitle,
            subtitle_align,
            false,
            border_style,
            width,
            padding,
        )
    }
}

// ---------------------------------------------------------------------------
// Console

/// `rich.console.Console`: renders through core `rich` and writes the result
/// to `file` (default: `sys.stdout` at print time).
#[pyclass(name = "Console", module = "rs_rich.console", unsendable)]
struct Console {
    console: CoreConsole,
    file: Option<Py<PyAny>>,
    record: bool,
    recorded: Vec<Segment>,
}

fn color_system_name(system: Option<ColorSystem>) -> Option<&'static str> {
    system.map(|system| match system {
        ColorSystem::Standard => "standard",
        ColorSystem::EightBit => "256",
        ColorSystem::Truecolor => "truecolor",
        ColorSystem::Windows => "windows",
    })
}

impl Console {
    fn target<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.file {
            Some(file) => Ok(file.bind(py).clone()),
            None => py.import("sys")?.getattr("stdout"),
        }
    }

    /// Render with core, record if asked, and write to the target file.
    fn emit(&mut self, py: Python<'_>, render: impl FnOnce(&CoreConsole)) -> PyResult<()> {
        let segments = self.console.record_output(render);
        let output = self.console.segments_to_string(&segments);
        if self.record {
            self.recorded.extend(segments);
        }
        self.target(py)?.call_method1("write", (output,))?;
        Ok(())
    }
}

#[pymethods]
impl Console {
    #[new]
    #[pyo3(signature = (
        *, file=None, width=None, height=None, color_system=Some("auto".to_string()),
        force_terminal=None, no_color=None, record=false, highlight=true, emoji=true,
        safe_box=true
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        file: Option<Py<PyAny>>,
        width: Option<usize>,
        height: Option<usize>,
        color_system: Option<String>,
        force_terminal: Option<bool>,
        no_color: Option<bool>,
        record: bool,
        highlight: bool,
        emoji: bool,
        safe_box: bool,
    ) -> PyResult<Self> {
        // Upstream asks the file, not the process's stdout, whether it is a
        // terminal; `force_terminal` overrides it.
        let target = match &file {
            Some(file) => file.bind(py).clone(),
            None => py.import("sys")?.getattr("stdout")?,
        };
        let is_terminal = match force_terminal {
            Some(value) => value,
            None => target
                .call_method0("isatty")
                .and_then(|v| v.extract::<bool>())
                .unwrap_or(false),
        };
        let mut builder = CoreConsole::builder()
            .force_terminal(is_terminal)
            .highlight(highlight)
            .emoji(emoji)
            .safe_box(safe_box);
        builder = match color_system.as_deref() {
            Some("auto") => builder,
            None => builder.color_system(None),
            Some("standard") => builder.color_system(Some(ColorSystem::Standard)),
            Some("256") => builder.color_system(Some(ColorSystem::EightBit)),
            Some("truecolor") => builder.color_system(Some(ColorSystem::Truecolor)),
            Some("windows") => builder.color_system(Some(ColorSystem::Windows)),
            Some(other) => {
                return Err(PyValueError::new_err(format!(
                    "{other:?} is not a valid color system; expected auto, standard, 256, truecolor, windows or None"
                )))
            }
        };
        if let Some(no_color) = no_color {
            builder = builder.no_color(no_color);
        }
        // A file that is not a terminal is `COLUMNS` wide, or 80, whatever
        // size the process's own terminal is.
        let width = width.or_else(|| {
            (!is_terminal).then(|| {
                std::env::var("COLUMNS")
                    .ok()
                    .and_then(|v| v.trim().parse().ok())
                    .filter(|w: &usize| *w > 0)
                    .unwrap_or(80)
            })
        });
        if let Some(width) = width {
            builder = builder.width(width);
        }
        if let Some(height) = height {
            builder = builder.height(height);
        }
        Ok(Console {
            console: builder.build(),
            file,
            record,
            recorded: Vec::new(),
        })
    }

    /// The file output goes to: the one given, else `sys.stdout`.
    #[getter]
    fn file<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.target(py)
    }

    #[getter]
    fn width(&self) -> usize {
        self.console.width()
    }

    #[getter]
    fn height(&self) -> usize {
        self.console.height()
    }

    #[getter]
    fn is_terminal(&self) -> bool {
        self.console.is_terminal()
    }

    #[getter]
    fn color_system(&self) -> Option<&'static str> {
        color_system_name(self.console.color_system())
    }

    /// Print objects: `str` (console markup), `Text`, `Table` or `Panel`.
    /// Consecutive strings are joined with `sep`; each other object prints
    /// on its own lines.
    #[pyo3(signature = (*objects, sep=" ", end="\n", justify=None))]
    fn print(
        &mut self,
        py: Python<'_>,
        objects: &Bound<'_, PyTuple>,
        sep: &str,
        end: &str,
        justify: Option<&str>,
    ) -> PyResult<()> {
        if end != "\n" {
            return Err(PyNotImplementedError::new_err(
                "rs_rich prints with end=\"\\n\" only in this first slice",
            ));
        }
        let justify = self::justify(justify)?;
        if objects.is_empty() {
            return self.emit(py, |console| console.print_str(""));
        }
        let mut strings: Vec<String> = Vec::new();
        let flush = |this: &mut Self, strings: &mut Vec<String>| -> PyResult<()> {
            if strings.is_empty() {
                return Ok(());
            }
            let content = strings.join(sep);
            strings.clear();
            let mut result = Ok(());
            this.emit(py, |console| {
                result = if justify == Justify::Default {
                    console.try_print_str(&content)
                } else {
                    console.try_print_justified(&content, justify)
                };
            })?;
            result.map_err(|e| MarkupError::new_err(e.to_string()))
        };
        for object in objects.iter() {
            if object.is_instance_of::<PyString>() {
                strings.push(object.extract()?);
                continue;
            }
            // Upstream prints a number, bool or None as its `str`.
            if object.is_none()
                || object.is_instance_of::<PyBool>()
                || object.is_instance_of::<PyInt>()
                || object.is_instance_of::<PyFloat>()
            {
                strings.push(object.str()?.extract()?);
                continue;
            }
            if justify != Justify::Default {
                return Err(PyNotImplementedError::new_err(
                    "rs_rich justifies str objects only in this first slice",
                ));
            }
            flush(self, &mut strings)?;
            let renderable = renderable(&object, None)?;
            self.emit(py, |console| console.print(renderable.as_ref()))?;
        }
        flush(self, &mut strings)
    }

    /// Draw a horizontal rule, with an optional (markup) title.
    #[pyo3(signature = (title="", *, characters="─", style=None))]
    fn rule(
        &mut self,
        py: Python<'_>,
        title: &str,
        characters: &str,
        style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let mut rule = if title.is_empty() {
            Rule::line()
        } else {
            Rule::new(title)
        }
        .characters(characters);
        if let Some(style) = resolved_style(style)? {
            rule = rule.style(style);
        }
        self.emit(py, |console| console.print(&rule))
    }

    /// The recorded output as plain text (or with ANSI styles), as
    /// `Console(record=True).export_text()`.
    #[pyo3(signature = (*, clear=true, styles=false))]
    fn export_text(&mut self, clear: bool, styles: bool) -> PyResult<String> {
        if !self.record {
            return Err(PyRuntimeError::new_err(
                "To export console contents set record=True in the constructor or instance",
            ));
        }
        let text = if styles {
            self.console.segments_to_string(&self.recorded)
        } else {
            self.recorded
                .iter()
                .filter(|segment| !segment.control)
                .map(|segment| segment.text.as_str())
                .collect()
        };
        if clear {
            self.recorded.clear();
        }
        Ok(text)
    }
}

// ---------------------------------------------------------------------------

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<Console>()?;
    m.add_class::<Text>()?;
    m.add_class::<Style>()?;
    m.add_class::<Table>()?;
    m.add_class::<Panel>()?;
    m.add_class::<PyBox>()?;
    for (name, inner) in BOXES {
        m.add(
            *name,
            PyBox {
                name,
                inner: *inner,
            },
        )?;
    }
    m.add("ConsoleError", m.py().get_type::<ConsoleError>())?;
    m.add("MarkupError", m.py().get_type::<MarkupError>())?;
    m.add("StyleSyntaxError", m.py().get_type::<StyleSyntaxError>())?;
    m.add("escape", pyo3::wrap_pyfunction!(escape, m)?)?;
    Ok(())
}

/// `rich.markup.escape`: backslash-escape `[` so text is not read as markup.
#[pyfunction]
fn escape(markup: &str) -> String {
    rich::markup::escape(markup)
}
