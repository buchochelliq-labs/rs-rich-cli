//! Inspectors: `rs_rich.ext.source_view` (source with line numbers and
//! search highlights), `.hex` (a hex dump), `.unicode_inspect` (graphemes,
//! code points and widths) and `.env_inspect` (environment variables and
//! `PATH`-like lists). Plus `.derive`: records as field grids and tables,
//! the runtime behind Rust's `#[derive(Rich)]`.

use std::path::Path;
use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString, PyType};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich_ext::derive::{self as derive, Field as CoreField, Presentation, RichRecord};
use rich_ext::env_inspect::{
    self as env, EnvView as CoreEnvView, PathKind, PathView as CorePathView, OS_PATH_SEPARATOR,
};
use rich_ext::hex::{self, ByteClass, HexView as CoreHexView};
use rich_ext::source_view::SourceView as CoreSourceView;
use rich_ext::unicode_inspect::{self as unicode, UnicodeView as CoreUnicodeView};

use super::common;
use crate::renderable::{self, AsRenderable};

// ---------------------------------------------------------------------------
// Source

/// `SourceView(code, language, *, line_numbers=True, start_line=1,
/// search=None, theme=None, tab_size=4)`: highlighted source with line
/// numbers, case-insensitive search matches marked.
#[pyclass(name = "SourceView", module = "rs_rich.ext.source_view", frozen)]
pub(crate) struct SourceView {
    inner: CoreSourceView,
}

impl AsRenderable for SourceView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl SourceView {
    #[new]
    #[pyo3(signature = (code, language, *, line_numbers=true, start_line=1, search=None, theme=None, tab_size=4))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        code: String,
        language: String,
        line_numbers: bool,
        start_line: usize,
        search: Option<String>,
        theme: Option<String>,
        tab_size: usize,
    ) -> Self {
        let mut inner = CoreSourceView::new(code, language)
            .line_numbers(line_numbers)
            .start_line(start_line)
            .tab_size(tab_size);
        if let Some(pattern) = search {
            inner = inner.search(pattern);
        }
        if let Some(theme) = theme {
            inner = inner.theme(theme);
        }
        SourceView { inner }
    }

    /// `(line_number, match_count)` for each line with a search match.
    fn matches(&self) -> Vec<(usize, usize)> {
        self.inner.matches()
    }
}

// ---------------------------------------------------------------------------
// Hex

fn bytes_arg(value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(text) = value.cast::<PyString>() {
        return Ok(text.to_cow()?.as_bytes().to_vec());
    }
    value.extract::<Vec<u8>>()
}

/// A search needle: `bytes`, or text with `\xNN` escapes (`hex` syntax).
fn needle(value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(bytes) = value.cast::<PyBytes>() {
        return Ok(bytes.as_bytes().to_vec());
    }
    let text: String = value.extract()?;
    hex::parse_needle(&text).map_err(PyValueError::new_err)
}

/// `HexView(data, *, offset=0, bytes_per_line=None, group=8,
/// highlight=None, ascii_panel=True, collapse=True)`: a hex dump with an
/// ASCII panel, byte classes coloured, repeated lines collapsed.
#[pyclass(name = "HexView", module = "rs_rich.ext.hex", frozen)]
pub(crate) struct HexView {
    inner: CoreHexView,
}

impl AsRenderable for HexView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl HexView {
    #[new]
    #[pyo3(signature = (data, *, offset=0, bytes_per_line=None, group=8, highlight=None, ascii_panel=true, collapse=true))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        data: &Bound<'_, PyAny>,
        offset: u64,
        bytes_per_line: Option<usize>,
        group: usize,
        highlight: Option<&Bound<'_, PyAny>>,
        ascii_panel: bool,
        collapse: bool,
    ) -> PyResult<Self> {
        let mut inner = CoreHexView::new(bytes_arg(data)?)
            .offset(offset)
            .bytes_per_line(bytes_per_line)
            .group(group)
            .ascii_panel(ascii_panel)
            .collapse(collapse);
        if let Some(value) = highlight {
            inner = inner.highlight(&needle(value)?);
        }
        Ok(HexView { inner })
    }

    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.bytes())
    }

    /// Hex digits in the offset column (at least 8).
    #[getter]
    fn offset_digits(&self) -> usize {
        self.inner.offset_digits()
    }

    /// The cells a line of `n` bytes takes.
    fn line_width(&self, n: usize) -> usize {
        self.inner.line_width(n)
    }

    /// Bytes per line at a console `width` (the fixed value, if set).
    fn resolved_bytes_per_line(&self, width: usize) -> usize {
        self.inner.resolved_bytes_per_line(width)
    }
}

/// `find_all(haystack, needle)`: every offset of `needle` in `haystack`.
#[pyfunction]
fn find_all(haystack: &[u8], needle: &Bound<'_, PyAny>) -> PyResult<Vec<usize>> {
    Ok(hex::find_all(haystack, &self::needle(needle)?))
}

/// `byte_class(byte)`: `null`, `printable`, `whitespace`, `control` or `high`.
#[pyfunction]
fn byte_class(byte: u8) -> &'static str {
    match ByteClass::of(byte) {
        ByteClass::Null => "null",
        ByteClass::Printable => "printable",
        ByteClass::Whitespace => "whitespace",
        ByteClass::Control => "control",
        ByteClass::High => "high",
    }
}

// ---------------------------------------------------------------------------
// Unicode

/// One grapheme cluster (or an invalid byte run).
#[pyclass(
    name = "GraphemeCluster",
    module = "rs_rich.ext.unicode_inspect",
    frozen
)]
pub(crate) struct GraphemeCluster {
    inner: unicode::Cluster,
}

#[pymethods]
impl GraphemeCluster {
    /// The byte offset in the input.
    #[getter]
    fn offset(&self) -> usize {
        self.inner.offset
    }
    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.bytes)
    }
    /// The text, or `None` for invalid bytes.
    #[getter]
    fn text(&self) -> Option<String> {
        self.inner.text.clone()
    }
    /// Terminal cells.
    #[getter]
    fn width(&self) -> usize {
        self.inner.width
    }
    /// `control`, `whitespace`, `combining`, `zero-width`, `variation
    /// selector`, `emoji`, `wide`, `ascii`, `other`, `invalid` or `bidi`.
    #[getter]
    fn kind(&self) -> &'static str {
        self.inner.kind.label()
    }
    /// `U+0065 U+0301`.
    #[getter]
    fn code_points(&self) -> String {
        self.inner.code_points()
    }
    /// `65 cc 81`.
    #[getter]
    fn hex(&self) -> String {
        self.inner.hex()
    }
    /// A Rust-style escape (`\u{301}`).
    #[getter]
    fn escape(&self) -> String {
        self.inner.escape()
    }
    /// How it shows in the view (control pictures for controls).
    #[pyo3(signature = (ascii=false))]
    fn display(&self, ascii: bool) -> String {
        self.inner.display(ascii)
    }
    fn __repr__(&self) -> String {
        format!(
            "<GraphemeCluster {} {}>",
            self.inner.code_points(),
            self.inner.kind
        )
    }
}

/// `UnicodeView(text, *, limit=None)`: a table of the graphemes of a `str`
/// (or `bytes`, invalid UTF-8 shown as such) with code points, widths and
/// kinds, then a summary.
#[pyclass(name = "UnicodeView", module = "rs_rich.ext.unicode_inspect", frozen)]
pub(crate) struct UnicodeView {
    inner: CoreUnicodeView,
}

impl AsRenderable for UnicodeView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl UnicodeView {
    #[new]
    #[pyo3(signature = (text, *, limit=None))]
    fn new(text: &Bound<'_, PyAny>, limit: Option<usize>) -> PyResult<Self> {
        let mut inner = match text.cast::<PyBytes>() {
            Ok(bytes) => CoreUnicodeView::from_bytes(bytes.as_bytes()),
            Err(_) => CoreUnicodeView::from_str(&text.extract::<String>()?),
        };
        if let Some(n) = limit {
            inner = inner.limit(n);
        }
        Ok(UnicodeView { inner })
    }

    fn clusters(&self) -> Vec<GraphemeCluster> {
        self.inner
            .clusters()
            .iter()
            .map(|c| GraphemeCluster { inner: c.clone() })
            .collect()
    }

    /// `{"bytes", "code_points", "graphemes", "cells", "invalid"}` counts.
    fn summary(&self) -> std::collections::BTreeMap<&'static str, usize> {
        let s = self.inner.summary();
        std::collections::BTreeMap::from([
            ("bytes", s.bytes),
            ("code_points", s.code_points),
            ("graphemes", s.graphemes),
            ("cells", s.cells),
            ("invalid", s.invalid),
        ])
    }

    /// The summary line (`12 bytes, 10 code points, ...`).
    fn summary_text(&self) -> String {
        self.inner.summary().to_string()
    }
}

/// `classify_cluster(cluster, width)`: the kind of one grapheme cluster.
#[pyfunction]
fn classify_cluster(cluster: &str, width: usize) -> &'static str {
    unicode::classify(cluster, width).label()
}

/// `control_picture(char, ascii=False)`: the visible stand-in for a
/// control character (`␛` or `<ESC>`), or `None`.
#[pyfunction]
#[pyo3(signature = (char, ascii=false))]
fn control_picture(char: char, ascii: bool) -> Option<String> {
    unicode::control_picture(char, ascii)
}

// ---------------------------------------------------------------------------
// Environment

fn separator_arg(value: Option<&str>) -> PyResult<char> {
    match value {
        None => Ok(OS_PATH_SEPARATOR),
        Some(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(c),
                _ => Err(PyValueError::new_err("separator must be one character")),
            }
        }
    }
}

/// `EnvView(vars=None, *, filter=None, redact=True, separator=None)`:
/// environment variables as a table (default: the process's), secrets
/// masked, `PATH`-like values split one entry per line. `filter` is a
/// case-insensitive substring or a glob.
#[pyclass(name = "EnvView", module = "rs_rich.ext.env_inspect", frozen)]
pub(crate) struct EnvView {
    inner: CoreEnvView,
}

impl AsRenderable for EnvView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl EnvView {
    #[new]
    #[pyo3(signature = (vars=None, *, filter=None, redact=true, separator=None))]
    fn new(
        vars: Option<&Bound<'_, PyAny>>,
        filter: Option<String>,
        redact: bool,
        separator: Option<&str>,
    ) -> PyResult<Self> {
        let mut inner = match vars {
            Some(vars) => CoreEnvView::new(
                common::pairs(vars)?
                    .into_iter()
                    .map(|(k, v)| Ok((k, v.str()?.to_string())))
                    .collect::<PyResult<Vec<_>>>()?,
            ),
            None => CoreEnvView::from_process(),
        };
        inner = inner.redact(redact).separator(separator_arg(separator)?);
        if let Some(pattern) = filter {
            inner = inner.filter(pattern);
        }
        Ok(EnvView { inner })
    }

    /// Whether a variable's value is masked.
    fn is_redacted(&self, name: &str) -> bool {
        self.inner.is_redacted(name)
    }

    /// The shown `(name, value)` pairs (values unmasked), sorted.
    fn vars(&self) -> Vec<(String, String)> {
        self.inner
            .vars()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }
}

/// `PathView(name, value=None, *, separator=None, case_insensitive=None,
/// probe=None)`: a `PATH`-like variable's entries, each checked: missing,
/// not a directory, duplicate or empty. `probe(path)` replaces the file
/// system check (return `"directory"`, `"file"` or `"missing"`).
#[pyclass(name = "PathView", module = "rs_rich.ext.env_inspect", frozen)]
pub(crate) struct PathView {
    name: String,
    value: String,
    separator: char,
    case_insensitive: Option<bool>,
    probe: Option<Arc<Py<PyAny>>>,
}

impl PathView {
    fn build(&self) -> CorePathView {
        let mut view = CorePathView::new(self.name.clone(), self.value.clone(), self.separator);
        if let Some(on) = self.case_insensitive {
            view = view.case_insensitive(on);
        }
        if let Some(probe) = &self.probe {
            let probe = probe.clone();
            view = view.probe(move |path: &Path| {
                Python::attach(|py| {
                    let result = probe
                        .bind(py)
                        .call1((path.to_string_lossy().into_owned(),))
                        .and_then(|kind| kind.extract::<String>());
                    match result {
                        Ok(kind) if kind == "directory" => PathKind::Directory,
                        Ok(kind) if kind == "file" || kind == "not_a_directory" => {
                            PathKind::NotADirectory
                        }
                        Ok(_) => PathKind::Missing,
                        Err(error) => {
                            error.write_unraisable(py, Some(probe.bind(py)));
                            PathKind::Missing
                        }
                    }
                })
            });
        }
        view
    }
}

struct BuiltPathView(CorePathView);

impl Renderable for BuiltPathView {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.0.rich_render(console, options)
    }
}

impl AsRenderable for PathView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(BuiltPathView(self.build())))
    }
}

#[pymethods]
impl PathView {
    #[new]
    #[pyo3(signature = (name, value=None, *, separator=None, case_insensitive=None, probe=None))]
    fn new(
        name: String,
        value: Option<String>,
        separator: Option<&str>,
        case_insensitive: Option<bool>,
        probe: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let value = match value {
            Some(value) => value,
            None => std::env::var(&name).unwrap_or_default(),
        };
        Ok(PathView {
            name,
            value,
            separator: separator_arg(separator)?,
            case_insensitive,
            probe: probe.map(Arc::new),
        })
    }

    #[getter]
    fn name(&self) -> String {
        self.name.clone()
    }

    /// `(index, entry, status)` per entry; status `ok`, `missing`, `not a
    /// directory`, `duplicate of #N` or `empty`.
    fn entries(&self) -> Vec<(usize, String, String)> {
        self.build()
            .entries()
            .into_iter()
            .map(|e| (e.index, e.entry, e.status.label()))
            .collect()
    }

    /// The entries with a problem.
    fn problems(&self) -> Vec<(usize, String, String)> {
        self.build()
            .entries()
            .into_iter()
            .filter(|e| e.status.is_problem())
            .map(|e| (e.index, e.entry, e.status.label()))
            .collect()
    }
}

/// `is_secret_name(name)`: whether an environment variable name looks
/// secret (`API_TOKEN`, `DB_PASSWORD`).
#[pyfunction]
fn is_secret_name(name: &str) -> bool {
    env::is_secret_name(name)
}

/// `redact_value(value)`: a secret's mask (its length, not its content).
#[pyfunction]
fn redact_value(value: &str) -> String {
    env::redact_value(value)
}

/// `name_matches(pattern, name)`: case-insensitive substring or glob match.
#[pyfunction]
fn name_matches(pattern: &str, name: &str) -> bool {
    env::name_matches(pattern, name)
}

/// `is_path_like(name, value, separator=None)`: whether a variable holds a
/// list of paths.
#[pyfunction]
#[pyo3(signature = (name, value, separator=None))]
fn is_path_like(name: &str, value: &str, separator: Option<&str>) -> PyResult<bool> {
    Ok(env::is_path_like(name, value, separator_arg(separator)?))
}

// ---------------------------------------------------------------------------
// Records (the runtime behind `#[derive(Rich)]`)

/// `RecordField(label, value, *, style=None, justify="left",
/// highlight=True)`: one labelled value of a `Record` (`value` is shown as
/// its `str`, highlighted like a repr).
#[pyclass(
    name = "RecordField",
    module = "rs_rich.ext.derive",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct RecordField {
    inner: CoreField,
}

#[pymethods]
impl RecordField {
    #[new]
    #[pyo3(signature = (label, value, *, style=None, justify="left", highlight=true))]
    fn new(
        label: String,
        value: &Bound<'_, PyAny>,
        style: Option<&Bound<'_, PyAny>>,
        justify: &str,
        highlight: bool,
    ) -> PyResult<Self> {
        let value = match value.cast::<PyString>() {
            Ok(s) => s.to_cow()?.into_owned(),
            Err(_) => value.str()?.to_string(),
        };
        let mut inner = CoreField::new(label, value);
        inner.style = common::style(style)?;
        inner.justify = crate::convert::justify(Some(justify))?;
        inner.highlight = highlight;
        Ok(RecordField { inner })
    }

    #[getter]
    fn label(&self) -> String {
        self.inner.label.clone()
    }
    #[getter]
    fn value(&self) -> String {
        self.inner.value.clone()
    }
}

#[derive(Clone)]
struct CoreRecord {
    title: Option<String>,
    fields: Vec<CoreField>,
    presentation: Presentation,
}

impl RichRecord for CoreRecord {
    fn rich_record(&self) -> (Option<String>, Vec<CoreField>) {
        (self.title.clone(), self.fields.clone())
    }

    fn rich_presentation(&self) -> Presentation {
        self.presentation
    }
}

impl Renderable for CoreRecord {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        derive::render(self, console, options)
    }
}

/// `Record(fields, *, title=None, presentation="fields")`: labelled
/// values as a grid (`fields`), a titled panel (`panel`) or a one-row table
/// (`table`). `Record.from_object(obj)` reads a dataclass's fields (or an
/// object's `__dict__`); `records_table(records)` lays many out as rows.
#[pyclass(name = "Record", module = "rs_rich.ext.derive", frozen)]
pub(crate) struct Record {
    inner: CoreRecord,
}

impl AsRenderable for Record {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

fn presentation(name: &str) -> PyResult<Presentation> {
    match name {
        "fields" => Ok(Presentation::Fields),
        "panel" => Ok(Presentation::Panel),
        "table" => Ok(Presentation::Table),
        other => Err(PyValueError::new_err(format!(
            "invalid presentation {other:?}; expected fields, panel or table"
        ))),
    }
}

fn field_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreField> {
    if let Ok(field) = value.extract::<PyRef<'_, RecordField>>() {
        return Ok(field.inner.clone());
    }
    let (label, v): (String, Bound<'_, PyAny>) = value.extract()?;
    Ok(RecordField::new(label, &v, None, "left", true)?.inner)
}

#[pymethods]
impl Record {
    #[new]
    #[pyo3(signature = (fields, *, title=None, presentation="fields"))]
    fn new(fields: &Bound<'_, PyAny>, title: Option<String>, presentation: &str) -> PyResult<Self> {
        let fields = if let Ok(map) = fields.cast::<pyo3::types::PyDict>() {
            map.iter()
                .map(|(k, v)| {
                    Ok(RecordField::new(k.str()?.to_string(), &v, None, "left", true)?.inner)
                })
                .collect::<PyResult<Vec<_>>>()?
        } else {
            fields
                .try_iter()?
                .map(|f| field_arg(&f?))
                .collect::<PyResult<Vec<_>>>()?
        };
        Ok(Record {
            inner: CoreRecord {
                title,
                fields,
                presentation: self::presentation(presentation)?,
            },
        })
    }

    /// A record of a dataclass instance (its fields) or any object (its
    /// public `__dict__` entries), titled with its class name. Values show
    /// as their `repr`.
    #[classmethod]
    #[pyo3(signature = (obj, *, title=None, presentation="fields"))]
    fn from_object(
        _cls: &Bound<'_, PyType>,
        obj: &Bound<'_, PyAny>,
        title: Option<String>,
        presentation: &str,
    ) -> PyResult<Self> {
        let py = obj.py();
        let dataclasses = py.import("dataclasses")?;
        let mut fields = Vec::new();
        if dataclasses
            .call_method1("is_dataclass", (obj,))?
            .is_truthy()?
        {
            for field in dataclasses.call_method1("fields", (obj,))?.try_iter()? {
                let name: String = field?.getattr("name")?.extract()?;
                let value = obj.getattr(name.as_str())?.repr()?;
                fields.push(CoreField::new(name, value.to_string()));
            }
        } else if let Some(dict) = obj.getattr_opt("__dict__")? {
            for (key, value) in dict.cast::<pyo3::types::PyDict>()?.iter() {
                let key: String = key.extract()?;
                if key.starts_with('_') {
                    continue;
                }
                fields.push(CoreField::new(key, value.repr()?.to_string()));
            }
        }
        let title = match title {
            Some(title) => Some(title),
            None => Some(obj.get_type().name()?.to_string()),
        };
        Ok(Record {
            inner: CoreRecord {
                title,
                fields,
                presentation: self::presentation(presentation)?,
            },
        })
    }

    #[getter]
    fn title(&self) -> Option<String> {
        self.inner.title.clone()
    }

    #[getter]
    fn fields(&self) -> Vec<RecordField> {
        self.inner
            .fields
            .iter()
            .map(|f| RecordField { inner: f.clone() })
            .collect()
    }
}

/// Many records as one table, one column per field label.
#[pyclass(name = "RecordsTable", module = "rs_rich.ext.derive", frozen)]
pub(crate) struct RecordsTable {
    records: Vec<CoreRecord>,
}

struct BuiltRecords(Vec<CoreRecord>);

impl Renderable for BuiltRecords {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        derive::table(self.0.iter()).rich_render(console, options)
    }
}

impl AsRenderable for RecordsTable {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(BuiltRecords(self.records.clone())))
    }
}

/// `records_table(records)`: `Record`s (or dataclass instances) as rows of
/// one table.
#[pyfunction]
fn records_table(py: Python<'_>, records: &Bound<'_, PyAny>) -> PyResult<RecordsTable> {
    let record_type = py.get_type::<Record>();
    let mut out = Vec::new();
    for item in records.try_iter()? {
        let item = item?;
        match item.extract::<PyRef<'_, Record>>() {
            Ok(record) => out.push(record.inner.clone()),
            Err(_) => out.push(Record::from_object(&record_type, &item, None, "fields")?.inner),
        }
    }
    Ok(RecordsTable { records: out })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<SourceView>(m)?;
    renderable::add_renderable_class::<HexView>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(find_all, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(byte_class, m)?)?;
    m.add("MAX_BYTES_PER_LINE", hex::MAX_BYTES_PER_LINE)?;
    m.add_class::<GraphemeCluster>()?;
    renderable::add_renderable_class::<UnicodeView>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(classify_cluster, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(control_picture, m)?)?;
    renderable::add_renderable_class::<EnvView>(m)?;
    renderable::add_renderable_class::<PathView>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(is_secret_name, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(redact_value, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(name_matches, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(is_path_like, m)?)?;
    m.add("OS_PATH_SEPARATOR", OS_PATH_SEPARATOR.to_string())?;
    m.add_class::<RecordField>()?;
    renderable::add_renderable_class::<Record>(m)?;
    renderable::add_renderable_class::<RecordsTable>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(records_table, m)?)?;
    Ok(())
}
