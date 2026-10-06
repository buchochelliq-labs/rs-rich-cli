//! `rs_rich.data`: the `rs-rich-data` crate from Python (0.0.16 workstream
//! 7): rows read from CSV, TSV and JSON Lines, type inference, column
//! statistics, profiles, data-quality reports, SQL result sets and
//! virtualised tables. The schema views (the model, `SchemaTree`,
//! `SchemaDiff`, `SchemaTimeline`, ER diagrams), merge conflicts and the
//! record inspector are in [`views`].
//!
//! Owner: the data area. `Rows` is mutable (`push`, and `Inference.apply`
//! converts its cells); every other class is frozen and builds its
//! `rs-rich-data` value when printed. Class names carry a prefix where the
//! flat native module already has the plain one (`DataSourceError`, since
//! `DataError` is `rs_rich.ext.data`'s parse error).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use rich::protocol::Renderable;
use rich_data::csv::{CsvReader, Header};
use rich_data::infer::{
    ColumnInference as CoreColumnInference, Inference as CoreInference, InferredType, Inferrer,
};
use rich_data::profile::{Profile as CoreProfile, ProfileOptions, Profiler};
use rich_data::quality::{
    CheckResult as CoreCheck, FailingRows as CoreFailingRows, QualityReport as CoreReport, Status,
};
use rich_data::sql::{typed_columns, ResultSet as CoreResultSet};
use rich_data::stats::{ColumnStats as CoreColumnStats, Stats as CoreStats, StatsOptions};
use rich_data::{jsonl, DataError as CoreDataError, Row, RowSource, Rows as CoreRows};
use rich_ext::table::VirtualTable as CoreVirtualTable;

use crate::art::repr_str;
use crate::ext::common::{self, names};
use crate::ext::tables::{value, value_to_py, TableData};
use crate::renderable::{self, AsRenderable};

mod views;

create_exception!(_native, DataSourceError, PyException);

/// A Python `DataSourceError`, with `line` (1-based, or `None`) and `reason`.
fn source_error(error: &CoreDataError) -> PyErr {
    let exception = DataSourceError::new_err(error.to_string());
    Python::attach(|py| {
        let value = exception.value(py);
        let _ = value.setattr("line", error.line());
        let _ = value.setattr("reason", error.message().to_string());
    });
    exception
}

/// A JSON value as plain Python, through the `json` module.
pub(crate) fn json_to_py(py: Python<'_>, json: &serde_json::Value) -> PyResult<Py<PyAny>> {
    Ok(py
        .import("json")?
        .call_method1("loads", (json.to_string(),))?
        .unbind())
}

/// JSON text, `indent` spaces per level when given.
pub(crate) fn json_text(
    py: Python<'_>,
    json: &serde_json::Value,
    indent: Option<usize>,
) -> PyResult<String> {
    match indent {
        None => Ok(json.to_string()),
        Some(indent) => {
            let kwargs = PyDict::new(py);
            kwargs.set_item("indent", indent)?;
            kwargs.set_item("ensure_ascii", false)?;
            py.import("json")?
                .call_method("dumps", (json_to_py(py, json)?,), Some(&kwargs))?
                .extract()
        }
    }
}

// ---------------------------------------------------------------------------
// Reading rows

names!(inferred_type, inferred_type_name, InferredType, "type", {
    "null" => InferredType::Null,
    "boolean" => InferredType::Boolean,
    "integer" => InferredType::Integer,
    "float" => InferredType::Float,
    "date" => InferredType::Date,
    "timestamp" => InferredType::Timestamp,
    "text" => InferredType::Text,
});

/// How a file is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    /// CSV or TSV, sniffed; the delimiter to fall back to.
    Csv(Option<char>),
    JsonLines,
}

fn format_arg(name: &str) -> PyResult<Format> {
    match name {
        "csv" => Ok(Format::Csv(Some(','))),
        "tsv" => Ok(Format::Csv(Some('\t'))),
        "jsonl" | "ndjson" => Ok(Format::JsonLines),
        _ => Err(PyValueError::new_err(format!(
            "unknown format {name:?}: expected \"csv\", \"tsv\" or \"jsonl\""
        ))),
    }
}

/// The format by a path's extension, as `rich profile` reads it. Only the
/// file name is read as text (lossily); the path itself stays as given.
fn format_by_name(path: &Path) -> Option<Format> {
    let name = path.file_name()?.to_string_lossy();
    let extension = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("jsonl" | "ndjson") => Some(Format::JsonLines),
        Some("csv") => Some(Format::Csv(Some(','))),
        Some("tsv" | "tab") => Some(Format::Csv(Some('\t'))),
        _ => None,
    }
}

/// The format by the first character that is not whitespace or a BOM.
fn format_by_content(head: &[u8]) -> Format {
    let head = head.strip_prefix("\u{feff}".as_bytes()).unwrap_or(head);
    match head.iter().find(|b| !b.is_ascii_whitespace()) {
        Some(b'{' | b'[') => Format::JsonLines,
        _ => Format::Csv(None),
    }
}

/// `True`, `False`, `"sniff"` (Python's `csv.Sniffer`, as `rich --csv`), or
/// `None` / `"unless_numeric"`: a header unless every cell of the first row
/// is a number, as `rich profile` reads it (the sniffer alone says "no
/// header" when one cell is empty).
fn header_mode(header: Option<&Bound<'_, PyAny>>) -> PyResult<Header> {
    let Some(header) = header.filter(|h| !h.is_none()) else {
        return Ok(Header::UnlessNumeric);
    };
    if let Ok(name) = header.extract::<String>() {
        return match name.as_str() {
            "sniff" => Ok(Header::Sniff),
            "unless_numeric" => Ok(Header::UnlessNumeric),
            _ => Err(PyValueError::new_err(format!(
                "unknown header mode {name:?}: expected True, False, None, \"sniff\" or \
                 \"unless_numeric\""
            ))),
        };
    }
    Ok(if header.extract::<bool>()? {
        Header::Yes
    } else {
        Header::No
    })
}

fn csv_reader(
    delimiter: Option<char>,
    fallback: Option<char>,
    header: Header,
) -> PyResult<CsvReader> {
    let mut reader = CsvReader::new().header_mode(header);
    if let Some(delimiter) = delimiter {
        reader = reader.delimiter(delimiter);
    }
    if let Some(fallback) = fallback {
        reader = reader.fallback(fallback);
    }
    Ok(reader)
}

fn one_char(value: Option<&str>, what: &str) -> PyResult<Option<char>> {
    value
        .map(|text| {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(c),
                _ => Err(PyValueError::new_err(format!(
                    "{what} must be one character, got {text:?}"
                ))),
            }
        })
        .transpose()
}

/// Every row of `source` into a profiler, or into memory.
fn drain(source: &mut dyn RowSource, mut push: impl FnMut(Row)) -> Result<(), CoreDataError> {
    while let Some(row) = source.next_row() {
        push(row?);
    }
    Ok(())
}

/// Open `path` and run `f` over its rows as a source.
fn with_file_source<T>(
    path: &Path,
    format: Option<&str>,
    header: Header,
    f: impl FnOnce(&mut dyn RowSource) -> PyResult<T>,
) -> PyResult<T> {
    let file = File::open(path).map_err(|err| {
        pyo3::exceptions::PyOSError::new_err(format!("cannot read {}: {err}", path.display()))
    })?;
    let mut reader = BufReader::new(file);
    let format = match format {
        Some(name) => format_arg(name)?,
        None => match format_by_name(path) {
            Some(format) => format,
            None => format_by_content(reader.fill_buf()?),
        },
    };
    with_source(Box::new(reader), format, header, f)
}

fn with_text_source<T>(
    text: String,
    format: &str,
    header: Header,
    f: impl FnOnce(&mut dyn RowSource) -> PyResult<T>,
) -> PyResult<T> {
    let format = format_arg(format)?;
    with_source(
        Box::new(std::io::Cursor::new(text.into_bytes())),
        format,
        header,
        f,
    )
}

fn with_source<T>(
    reader: Box<dyn BufRead>,
    format: Format,
    header: Header,
    f: impl FnOnce(&mut dyn RowSource) -> PyResult<T>,
) -> PyResult<T> {
    match format {
        Format::JsonLines => {
            let mut source = jsonl::source(reader).map_err(|e| source_error(&e))?;
            f(&mut source)
        }
        Format::Csv(fallback) => {
            let mut source = csv_reader(None, Some(fallback.unwrap_or(',')), header)?
                .source(reader)
                .map_err(|e| source_error(&e))?;
            f(&mut source)
        }
    }
}

fn collect(source: &mut dyn RowSource) -> PyResult<CoreRows> {
    let mut rows = CoreRows::new(source.columns().to_vec());
    rows.set_schema(source.schema().cloned());
    drain(source, |row| {
        rows.push(row);
    })
    .map_err(|e| source_error(&e))?;
    Ok(rows)
}

/// Read CSV text into `Rows`. The delimiter is sniffed unless given.
/// `header` is `True`, `False`, `"sniff"` (as `rich --csv`), or `None`: the
/// first row is a header unless every cell in it is a number. Cells are
/// text until `infer` types them.
#[pyfunction]
#[pyo3(signature = (text, *, delimiter=None, header=None))]
fn read_csv(
    text: &str,
    delimiter: Option<&str>,
    header: Option<&Bound<'_, PyAny>>,
) -> PyResult<Rows> {
    let reader = csv_reader(
        one_char(delimiter, "delimiter")?,
        None,
        header_mode(header)?,
    )?;
    reader
        .read(text)
        .map(Rows::from)
        .map_err(|e| source_error(&e))
}

/// Read TSV text into `Rows` (`read_csv` with a tab).
#[pyfunction]
#[pyo3(signature = (text, *, header=None))]
fn read_tsv(text: &str, header: Option<&Bound<'_, PyAny>>) -> PyResult<Rows> {
    CsvReader::tsv()
        .header_mode(header_mode(header)?)
        .read(text)
        .map(Rows::from)
        .map_err(|e| source_error(&e))
}

/// Read JSON Lines (one object per line) into `Rows`: the columns are the
/// keys, in the order first seen; nested values are their JSON text.
#[pyfunction]
fn read_jsonl(text: &str) -> PyResult<Rows> {
    jsonl::read(text)
        .map(Rows::from)
        .map_err(|e| source_error(&e))
}

/// Read a CSV, TSV or JSON Lines file into `Rows`. `format` (`"csv"`,
/// `"tsv"` or `"jsonl"`) defaults to the extension, then to the first
/// character (`{` or `[` is JSON Lines).
#[pyfunction]
#[pyo3(signature = (path, *, format=None, header=None))]
fn read_file(
    py: Python<'_>,
    path: PathBuf,
    format: Option<&str>,
    header: Option<&Bound<'_, PyAny>>,
) -> PyResult<Rows> {
    let header = header_mode(header)?;
    let format = format.map(str::to_string);
    py.detach(move || with_file_source(&path, format.as_deref(), header, collect))
        .map(Rows::from)
}

// ---------------------------------------------------------------------------
// Rows

/// Rows held in memory (`rich_data::Rows`): column names, an optional
/// `Schema`, and rows of `None`, `int`, `float`, `str` or `Text` cells. It
/// renders as a table (numeric columns, by the schema, on the right).
#[pyclass(name = "Rows", module = "rs_rich.data", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Rows {
    pub(crate) inner: CoreRows,
}

impl From<CoreRows> for Rows {
    fn from(inner: CoreRows) -> Self {
        Rows { inner }
    }
}

impl AsRenderable for Rows {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.to_table_data()))
    }
}

fn python_row(values: &Bound<'_, PyAny>) -> PyResult<Row> {
    values.try_iter()?.map(|v| value(&v?)).collect()
}

#[pymethods]
impl Rows {
    /// `columns` are the names; `rows` iterables of cells (missing cells are
    /// `None`, extra cells dropped); `schema` a `Schema` with one field per
    /// column.
    #[new]
    #[pyo3(signature = (columns, rows=None, *, schema=None))]
    fn new(
        columns: &Bound<'_, PyAny>,
        rows: Option<&Bound<'_, PyAny>>,
        schema: Option<PyRef<'_, views::Schema>>,
    ) -> PyResult<Self> {
        let mut inner = CoreRows::new(common::strings(columns)?);
        if let Some(rows) = rows {
            for row in rows.try_iter()? {
                inner.push(python_row(&row?)?);
            }
        }
        inner.set_schema(schema.map(|s| s.model.clone()));
        Ok(Rows { inner })
    }

    /// Add a row. Returns the rows.
    fn push<'py>(
        mut slf: PyRefMut<'py, Self>,
        values: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let row = python_row(values)?;
        slf.inner.push(row);
        Ok(slf)
    }

    #[getter]
    fn columns(&self) -> Vec<String> {
        self.inner.columns().to_vec()
    }

    /// The rows, as Python values.
    #[getter]
    fn rows(&self, py: Python<'_>) -> PyResult<Vec<Vec<Py<PyAny>>>> {
        self.inner
            .rows()
            .iter()
            .map(|row| row.iter().map(|v| value_to_py(py, v)).collect())
            .collect()
    }

    /// The schema, if any (set by `Inference.apply`).
    #[getter]
    fn get_schema(&self) -> Option<views::Schema> {
        self.inner.schema().cloned().map(views::Schema::from_model)
    }

    #[setter]
    fn set_schema(&mut self, schema: Option<PyRef<'_, views::Schema>>) {
        self.inner.set_schema(schema.map(|s| s.model.clone()));
    }

    /// Every cell of the column `name`, top to bottom.
    fn column(&self, py: Python<'_>, name: &str) -> PyResult<Vec<Py<PyAny>>> {
        let index = self
            .inner
            .column_index(name)
            .ok_or_else(|| PyKeyError::new_err(name.to_string()))?;
        self.inner
            .column(index)
            .map(|v| value_to_py(py, v))
            .collect()
    }

    /// Infer each column's type (see `infer`).
    #[pyo3(signature = (*, null_tokens=None, overrides=None))]
    fn infer(
        &self,
        null_tokens: Option<&Bound<'_, PyAny>>,
        overrides: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Inference> {
        infer_rows(&self.inner, null_tokens, overrides)
    }

    /// A `TableData` of these rows (`rs_rich.ext.table`): sort, group,
    /// total and style it there.
    fn to_table_data(&self) -> TableData {
        TableData {
            inner: self.inner.to_table_data(),
        }
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Rows columns={} rows={}>",
            self.inner.columns().len(),
            self.inner.len()
        )
    }
}

// ---------------------------------------------------------------------------
// Inference

/// One column's inferred type and the counts behind it.
#[pyclass(name = "ColumnInference", module = "rs_rich.data", frozen)]
pub(crate) struct ColumnInference {
    #[pyo3(get)]
    name: String,
    /// `"null"`, `"boolean"`, `"integer"`, `"float"`, `"date"`,
    /// `"timestamp"` or `"text"`.
    #[pyo3(get)]
    r#type: &'static str,
    #[pyo3(get)]
    overridden: bool,
    #[pyo3(get)]
    summary: String,
    cells: usize,
    nulls: usize,
    booleans: usize,
    integers: usize,
    floats: usize,
    dates: usize,
    timestamps: usize,
}

impl From<&CoreColumnInference> for ColumnInference {
    fn from(c: &CoreColumnInference) -> Self {
        let e = c.evidence();
        ColumnInference {
            name: c.name().to_string(),
            r#type: inferred_type_name(c.data_type()),
            overridden: c.is_overridden(),
            summary: c.summary(),
            cells: e.cells,
            nulls: e.nulls,
            booleans: e.booleans,
            integers: e.integers,
            floats: e.floats,
            dates: e.dates,
            timestamps: e.timestamps,
        }
    }
}

#[pymethods]
impl ColumnInference {
    /// The counts: `cells`, `nulls`, and the values that parse as each type.
    #[getter]
    fn evidence<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("cells", self.cells)?;
        d.set_item("nulls", self.nulls)?;
        d.set_item("booleans", self.booleans)?;
        d.set_item("integers", self.integers)?;
        d.set_item("floats", self.floats)?;
        d.set_item("dates", self.dates)?;
        d.set_item("timestamps", self.timestamps)?;
        Ok(d)
    }

    fn __repr__(&self) -> String {
        format!(
            "ColumnInference(name={}, type={})",
            repr_str(&self.name),
            repr_str(self.r#type)
        )
    }
}

/// Every column's inferred type (`rich_data::infer::Inference`). It renders
/// as a table of column, type, parsed values and nulls.
#[pyclass(name = "Inference", module = "rs_rich.data", frozen)]
pub(crate) struct Inference {
    inner: CoreInference,
}

impl AsRenderable for Inference {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl Inference {
    #[getter]
    fn columns(&self) -> Vec<ColumnInference> {
        self.inner
            .columns()
            .iter()
            .map(ColumnInference::from)
            .collect()
    }

    /// The schema these types describe.
    #[getter]
    fn schema(&self) -> views::Schema {
        views::Schema::from_model(self.inner.schema())
    }

    /// Convert `rows`' cells to their columns' types (null tokens to `None`,
    /// integers to `int`, floats to `float`) and set its schema.
    fn apply(&self, mut rows: PyRefMut<'_, Rows>) {
        self.inner.apply(&mut rows.inner);
    }

    fn __repr__(&self) -> String {
        format!("<Inference columns={}>", self.inner.columns().len())
    }
}

fn infer_rows(
    rows: &CoreRows,
    null_tokens: Option<&Bound<'_, PyAny>>,
    overrides: Option<&Bound<'_, PyDict>>,
) -> PyResult<Inference> {
    let mut inferrer = Inferrer::new();
    if let Some(tokens) = null_tokens {
        inferrer = inferrer.null_tokens(common::strings(tokens)?);
    }
    if let Some(overrides) = overrides {
        for (column, kind) in overrides.iter() {
            inferrer = inferrer.override_type(
                column.extract::<String>()?,
                inferred_type(&kind.extract::<String>()?)?,
            );
        }
    }
    Ok(Inference {
        inner: inferrer.infer(rows),
    })
}

/// Infer each column's type (`rich_data::infer`): integers, floats,
/// booleans, dates, timestamps, nulls or text, with the evidence.
/// `null_tokens` replaces the cells that count as null (default `""`,
/// `null`, `NULL`, `NA`, `N/A`); `overrides` maps column names to types.
#[pyfunction]
#[pyo3(signature = (rows, *, null_tokens=None, overrides=None))]
fn infer(
    rows: PyRef<'_, Rows>,
    null_tokens: Option<&Bound<'_, PyAny>>,
    overrides: Option<&Bound<'_, PyDict>>,
) -> PyResult<Inference> {
    infer_rows(&rows.inner, null_tokens, overrides)
}

// ---------------------------------------------------------------------------
// Statistics

/// One column's statistics.
#[pyclass(name = "ColumnStats", module = "rs_rich.data", frozen)]
pub(crate) struct ColumnStats {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    count: usize,
    #[pyo3(get)]
    nulls: usize,
    #[pyo3(get)]
    distinct: usize,
    #[pyo3(get)]
    numeric: bool,
    #[pyo3(get)]
    min: Option<String>,
    #[pyo3(get)]
    max: Option<String>,
    #[pyo3(get)]
    mean: Option<f64>,
    #[pyo3(get)]
    median: Option<f64>,
    /// `(q, value)` pairs.
    #[pyo3(get)]
    quantiles: Vec<(f64, f64)>,
    /// `(value, count)` pairs, most common first.
    #[pyo3(get)]
    top: Vec<(String, usize)>,
    #[pyo3(get)]
    summary: String,
}

impl From<&CoreColumnStats> for ColumnStats {
    fn from(c: &CoreColumnStats) -> Self {
        ColumnStats {
            name: c.name.clone(),
            count: c.count,
            nulls: c.nulls,
            distinct: c.distinct,
            numeric: c.numeric,
            min: c.min.clone(),
            max: c.max.clone(),
            mean: c.mean,
            median: c.median,
            quantiles: c.quantiles.clone(),
            top: c.top.clone(),
            summary: c.summary(),
        }
    }
}

#[pymethods]
impl ColumnStats {
    fn __repr__(&self) -> String {
        format!("ColumnStats({})", repr_str(&self.summary))
    }
}

/// Per-column statistics of `rows` (`rich_data::stats::Stats`): count,
/// nulls, distinct values, min, max, mean, median, `quantiles` and the
/// `top` most common values. It renders as a table.
#[pyclass(name = "Stats", module = "rs_rich.data", frozen)]
pub(crate) struct Stats {
    inner: CoreStats,
}

impl AsRenderable for Stats {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.to_table()))
    }
}

#[pymethods]
impl Stats {
    #[new]
    #[pyo3(signature = (rows, *, quantiles=None, top=3))]
    fn new(rows: PyRef<'_, Rows>, quantiles: Option<Vec<f64>>, top: usize) -> PyResult<Self> {
        let mut options = StatsOptions {
            top,
            ..StatsOptions::default()
        };
        if let Some(quantiles) = quantiles {
            if let Some(q) = quantiles.iter().find(|q| !(0.0..=1.0).contains(*q)) {
                return Err(PyValueError::new_err(format!(
                    "a quantile must be between 0 and 1, got {q}"
                )));
            }
            options.quantiles = quantiles;
        }
        Ok(Stats {
            inner: CoreStats::with_options(&rows.inner, &options),
        })
    }

    #[getter]
    fn columns(&self) -> Vec<ColumnStats> {
        self.inner.columns().iter().map(ColumnStats::from).collect()
    }

    /// Each column's statistics in one line, for a heading.
    fn headers(&self) -> Vec<String> {
        self.inner.headers()
    }

    fn __repr__(&self) -> String {
        format!("<Stats columns={}>", self.inner.columns().len())
    }
}

// ---------------------------------------------------------------------------
// Profiles

/// The most histogram bins a profile takes: far past a readable histogram.
const MAX_BINS: usize = 1000;

fn profile_options(
    sample: usize,
    top: usize,
    bins: usize,
    buckets: usize,
    columns: Option<&Bound<'_, PyAny>>,
    nulls: Option<&Bound<'_, PyAny>>,
) -> PyResult<ProfileOptions> {
    if sample == 0 || buckets == 0 {
        return Err(PyValueError::new_err(
            "sample and buckets must be at least 1",
        ));
    }
    // Every numeric column's histogram allocates its bins up front.
    if bins > MAX_BINS {
        return Err(PyValueError::new_err(format!(
            "bins must be at most {MAX_BINS}, got {bins}"
        )));
    }
    let mut options = ProfileOptions {
        sample,
        top,
        bins,
        buckets,
        ..ProfileOptions::default()
    };
    if let Some(columns) = columns.filter(|c| !c.is_none()) {
        options.columns = Some(common::strings(columns)?);
    }
    if let Some(nulls) = nulls.filter(|n| !n.is_none()) {
        options.nulls = common::strings(nulls)?;
    }
    Ok(options)
}

fn profile_source(source: &mut dyn RowSource, options: ProfileOptions) -> PyResult<CoreProfile> {
    let mut profiler = Profiler::new(source.columns(), options).map_err(|e| source_error(&e))?;
    drain(source, |row| profiler.push(row)).map_err(|e| source_error(&e))?;
    Ok(profiler.finish())
}

/// A bounded profile of rows (`rich_data::profile::Profile`): each column's
/// type, nulls, distinct values, statistics and a histogram or its top
/// values, and a missing-value map. At most `sample` rows (a fixed-seed
/// reservoir) are kept; the row count and nulls count every row. `bins`
/// is at most 1000. It renders as a heading, a table, the distributions
/// and the map.
#[pyclass(name = "Profile", module = "rs_rich.data", frozen)]
pub(crate) struct Profile {
    inner: CoreProfile,
}

impl AsRenderable for Profile {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl Profile {
    /// Profile `rows` in memory.
    #[new]
    #[pyo3(signature = (
        rows, *, name=None, sample=rich_data::profile::DEFAULT_SAMPLE,
        top=rich_data::profile::DEFAULT_TOP, bins=rich_data::profile::DEFAULT_BINS,
        buckets=rich_data::profile::DEFAULT_BUCKETS, columns=None, nulls=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        rows: PyRef<'_, Rows>,
        name: Option<String>,
        sample: usize,
        top: usize,
        bins: usize,
        buckets: usize,
        columns: Option<&Bound<'_, PyAny>>,
        nulls: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let options = profile_options(sample, top, bins, buckets, columns, nulls)?;
        let mut source = rows.inner.clone().into_source();
        let profile = profile_source(&mut source, options)?;
        Ok(Profile {
            inner: match name {
                Some(name) => profile.with_name(name),
                None => profile,
            },
        })
    }

    /// Profile a CSV, TSV or JSON Lines file, streaming it (`format` as
    /// `read_file`; a CSV's first row is its header unless every cell is a
    /// number, as `rich profile` reads it). Named after the path.
    #[staticmethod]
    #[pyo3(signature = (
        path, *, format=None, sample=rich_data::profile::DEFAULT_SAMPLE,
        top=rich_data::profile::DEFAULT_TOP, bins=rich_data::profile::DEFAULT_BINS,
        buckets=rich_data::profile::DEFAULT_BUCKETS, columns=None, nulls=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_path(
        py: Python<'_>,
        path: PathBuf,
        format: Option<String>,
        sample: usize,
        top: usize,
        bins: usize,
        buckets: usize,
        columns: Option<&Bound<'_, PyAny>>,
        nulls: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let options = profile_options(sample, top, bins, buckets, columns, nulls)?;
        let inner = py.detach(|| {
            with_file_source(&path, format.as_deref(), Header::UnlessNumeric, |source| {
                profile_source(source, options)
            })
        })?;
        Ok(Profile {
            // The name is for the heading only; the file was opened by its
            // own path, whatever bytes it holds.
            inner: inner.with_name(path.to_string_lossy()),
        })
    }

    /// Profile CSV, TSV or JSON Lines text (`format` `"csv"`, `"tsv"` or
    /// `"jsonl"`).
    #[staticmethod]
    #[pyo3(signature = (
        text, *, format="csv", name=None, sample=rich_data::profile::DEFAULT_SAMPLE,
        top=rich_data::profile::DEFAULT_TOP, bins=rich_data::profile::DEFAULT_BINS,
        buckets=rich_data::profile::DEFAULT_BUCKETS, columns=None, nulls=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn from_text(
        text: String,
        format: &str,
        name: Option<String>,
        sample: usize,
        top: usize,
        bins: usize,
        buckets: usize,
        columns: Option<&Bound<'_, PyAny>>,
        nulls: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let options = profile_options(sample, top, bins, buckets, columns, nulls)?;
        let profile = with_text_source(text, format, Header::UnlessNumeric, |source| {
            profile_source(source, options)
        })?;
        Ok(Profile {
            inner: match name {
                Some(name) => profile.with_name(name),
                None => profile,
            },
        })
    }

    #[getter]
    fn name(&self) -> Option<String> {
        self.inner.name.clone()
    }

    /// Every row read.
    #[getter]
    fn rows(&self) -> u64 {
        self.inner.rows()
    }

    /// The rows the types, statistics and distributions describe.
    #[getter]
    fn sample_size(&self) -> usize {
        self.inner.sample_size()
    }

    /// Whether the sample is fewer rows than were read.
    #[getter]
    fn sampled(&self) -> bool {
        self.inner.sampled()
    }

    /// One `dict` per column (as `to_dict()["columns"]`).
    #[getter]
    fn columns(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(py, &self.inner.to_json()["columns"])
    }

    /// The column called `name`, as a `dict`, or `None`.
    fn column(&self, py: Python<'_>, name: &str) -> PyResult<Option<Py<PyAny>>> {
        let json = self.inner.to_json();
        let found = json["columns"]
            .as_array()
            .and_then(|columns| columns.iter().find(|c| c["name"] == name));
        found.map(|c| json_to_py(py, c)).transpose()
    }

    #[getter]
    fn notes(&self) -> Vec<String> {
        self.inner.notes().to_vec()
    }

    /// The heading: the name, the rows (and the sample) and the columns.
    #[getter]
    fn heading(&self) -> String {
        self.inner.heading()
    }

    /// The profile as plain Python: `rows`, `sample`, `sampled`, `columns`,
    /// `missing`, and `name` and `notes` when set.
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        json_to_py(py, &self.inner.to_json())
    }

    /// The profile as JSON text (`rich profile --json`'s shape).
    #[pyo3(signature = (*, indent=None))]
    fn to_json(&self, py: Python<'_>, indent: Option<usize>) -> PyResult<String> {
        json_text(py, &self.inner.to_json(), indent)
    }

    fn __repr__(&self) -> String {
        format!(
            "<Profile rows={} columns={}>",
            self.inner.rows(),
            self.inner.columns().len()
        )
    }
}

// ---------------------------------------------------------------------------
// Data quality

names!(status, status_name, Status, "status", {
    "pass" => Status::Pass,
    "warn" => Status::Warn,
    "fail" => Status::Fail,
    "error" => Status::Error,
});

/// One check's result (`rich_data::quality::CheckResult`). `status` is
/// `"pass"`, `"warn"`, `"fail"` or `"error"`; `failing_rows` a sample of
/// the rows it failed on, as `(columns, rows)` or `(columns, rows, total)`
/// with every cell text.
#[pyclass(name = "CheckResult", module = "rs_rich.data", frozen)]
pub(crate) struct CheckResult {
    inner: CoreCheck,
}

#[pymethods]
impl CheckResult {
    #[new]
    #[pyo3(signature = (
        check, status, *, column=None, observed=None, expected=None, message=None,
        failing_rows=None
    ))]
    fn new(
        check: String,
        status: &str,
        column: Option<String>,
        observed: Option<String>,
        expected: Option<String>,
        message: Option<String>,
        failing_rows: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreCheck::new(check, self::status(status)?);
        inner.column = column;
        inner.observed = observed;
        inner.expected = expected;
        inner.message = message;
        if let Some(rows) = failing_rows.filter(|r| !r.is_none()) {
            let (columns, rows, total) = match rows.extract::<(Vec<String>, Vec<Vec<String>>)>() {
                Ok((columns, rows)) => (columns, rows, None),
                Err(_) => rows.extract::<(Vec<String>, Vec<Vec<String>>, Option<u64>)>()?,
            };
            inner.failing_rows = Some(CoreFailingRows {
                columns,
                rows,
                total,
            });
        }
        Ok(CheckResult { inner })
    }

    #[getter]
    fn check(&self) -> &str {
        &self.inner.check
    }

    #[getter]
    fn status(&self) -> &'static str {
        status_name(self.inner.status)
    }

    #[getter]
    fn column(&self) -> Option<String> {
        self.inner.column.clone()
    }

    #[getter]
    fn observed(&self) -> Option<String> {
        self.inner.observed.clone()
    }

    #[getter]
    fn expected(&self) -> Option<String> {
        self.inner.expected.clone()
    }

    #[getter]
    fn message(&self) -> Option<String> {
        self.inner.message.clone()
    }

    /// `(columns, rows, total)`, or `None`.
    #[getter]
    #[allow(clippy::type_complexity)]
    fn failing_rows(&self) -> Option<(Vec<String>, Vec<Vec<String>>, Option<u64>)> {
        self.inner
            .failing_rows
            .as_ref()
            .map(|f| (f.columns.clone(), f.rows.clone(), f.total))
    }

    /// `check` on `column`, as a report heads it.
    #[getter]
    fn title(&self) -> String {
        self.inner.title()
    }

    fn __repr__(&self) -> String {
        format!(
            "CheckResult({}, {})",
            repr_str(&self.inner.title()),
            repr_str(status_name(self.inner.status))
        )
    }
}

fn check_error(error: CoreDataError) -> PyErr {
    PyKeyError::new_err(error.message().to_string())
}

/// `not_null`: every cell of `column` has a value. Raises `KeyError` for a
/// column `rows` does not have.
#[pyfunction]
fn check_not_null(rows: PyRef<'_, Rows>, column: &str) -> PyResult<CheckResult> {
    rich_data::quality::not_null(&rows.inner, column)
        .map(|inner| CheckResult { inner })
        .map_err(check_error)
}

/// `unique`: no two non-null cells of `column` hold the same value.
#[pyfunction]
fn check_unique(rows: PyRef<'_, Rows>, column: &str) -> PyResult<CheckResult> {
    rich_data::quality::unique(&rows.inner, column)
        .map(|inner| CheckResult { inner })
        .map_err(check_error)
}

/// Check results as a report (`rich_data::quality::QualityReport`):
/// failures first, a table, each failure's rows, and a summary. Status is
/// a word and a colour, never a colour alone.
#[pyclass(name = "QualityReport", module = "rs_rich.data", frozen)]
pub(crate) struct QualityReport {
    inner: CoreReport,
}

impl AsRenderable for QualityReport {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl QualityReport {
    /// `show_rows`: the most failing rows shown per check (default 5).
    #[new]
    #[pyo3(signature = (results, *, show_rows=rich_data::quality::FAILING_ROWS))]
    fn new(results: &Bound<'_, PyAny>, show_rows: usize) -> PyResult<Self> {
        let results = results
            .try_iter()?
            .map(|r| Ok(r?.extract::<PyRef<'_, CheckResult>>()?.inner.clone()))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(QualityReport {
            inner: CoreReport::new(results).show_rows(show_rows),
        })
    }

    /// A report from JSON: `{"results": [...]}` (as `to_json` writes) or a
    /// list of results, each with `check`, `status` and the optional
    /// `column`, `observed`, `expected`, `message` and `failing_rows`
    /// (`columns`, `rows`, `total`).
    #[staticmethod]
    #[pyo3(signature = (text, *, show_rows=rich_data::quality::FAILING_ROWS))]
    fn from_json(text: &str, show_rows: usize) -> PyResult<Self> {
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let results = match json {
            serde_json::Value::Object(mut object) => object
                .remove("results")
                .ok_or_else(|| PyValueError::new_err("a report needs \"results\""))?,
            other => other,
        };
        let results: Vec<CoreCheck> =
            serde_json::from_value(results).map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(QualityReport {
            inner: CoreReport::new(results).show_rows(show_rows),
        })
    }

    #[getter]
    fn results(&self) -> Vec<CheckResult> {
        self.inner
            .results()
            .iter()
            .map(|r| CheckResult { inner: r.clone() })
            .collect()
    }

    /// `{"passed": n, "warned": n, "failed": n, "errored": n}`.
    #[getter]
    fn totals<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let t = self.inner.totals();
        let d = PyDict::new(py);
        d.set_item("passed", t.passed)?;
        d.set_item("warned", t.warned)?;
        d.set_item("failed", t.failed)?;
        d.set_item("errored", t.errored)?;
        Ok(d)
    }

    /// Whether nothing failed or errored (warnings pass).
    #[getter]
    fn ok(&self) -> bool {
        self.inner.is_success()
    }

    /// `3 passed, 1 warned, 2 failed`, with errors when any.
    #[getter]
    fn summary(&self) -> String {
        self.inner.summary()
    }

    /// `results`, `totals` and `ok` as JSON text.
    #[pyo3(signature = (*, indent=None))]
    fn to_json(&self, py: Python<'_>, indent: Option<usize>) -> PyResult<String> {
        json_text(py, &self.inner.to_json(), indent)
    }

    fn __len__(&self) -> usize {
        self.inner.results().len()
    }

    fn __repr__(&self) -> String {
        format!("<QualityReport {}>", self.inner.summary())
    }
}

// ---------------------------------------------------------------------------
// Tables over rows

/// A SQL-shaped query result (`rich_data::sql::ResultSet`): columns typed
/// by the rows' schema (numbers right, text left), `NULL` marked apart from
/// empty text, `limit` rows from `offset`, and a footer with the row count
/// and, when given, how long it took (`elapsed`, in seconds).
#[pyclass(name = "ResultSet", module = "rs_rich.data", frozen)]
pub(crate) struct ResultSet {
    rows: CoreRows,
    offset: usize,
    limit: usize,
    elapsed: Option<std::time::Duration>,
    null_marker: String,
    title: Option<String>,
}

impl ResultSet {
    fn core(&self) -> CoreResultSet {
        let mut set = CoreResultSet::new(self.rows.clone())
            .offset(self.offset)
            .limit(self.limit)
            .null_marker(self.null_marker.clone());
        if let Some(elapsed) = self.elapsed {
            set = set.elapsed(elapsed);
        }
        if let Some(title) = &self.title {
            set = set.title(title.clone());
        }
        set
    }
}

impl AsRenderable for ResultSet {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.core()))
    }
}

#[pymethods]
impl ResultSet {
    #[new]
    #[pyo3(signature = (
        rows, *, offset=0, limit=rich_data::sql::DEFAULT_LIMIT, elapsed=None,
        null_marker="NULL", title=None
    ))]
    fn new(
        rows: PyRef<'_, Rows>,
        offset: usize,
        limit: usize,
        elapsed: Option<&Bound<'_, PyAny>>,
        null_marker: &str,
        title: Option<String>,
    ) -> PyResult<Self> {
        Ok(ResultSet {
            rows: rows.inner.clone(),
            offset,
            limit,
            elapsed: common::opt_seconds(elapsed)?,
            null_marker: null_marker.to_string(),
            title,
        })
    }

    #[getter]
    fn columns(&self) -> Vec<String> {
        self.rows.columns().to_vec()
    }

    #[getter]
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table shows only part of the rows.
    #[getter]
    fn windowed(&self) -> bool {
        self.core().is_windowed()
    }

    /// `(3 rows)`, `(1 row, 12ms)`.
    #[pyo3(signature = (*, ascii=false))]
    fn footer(&self, ascii: bool) -> String {
        self.core().footer(ascii)
    }

    fn __repr__(&self) -> String {
        format!(
            "<ResultSet columns={} rows={}>",
            self.rows.columns().len(),
            self.rows.len()
        )
    }
}

/// One window of rows as a table (`rich_ext::table::VirtualTable`): only
/// `height` rows from `offset` are laid out, column widths come from a
/// sample of the first rows (`fit_window` widens them to the rows shown),
/// and a position line (`rows 41-60 of 1,000`) says where the window is.
/// `scroll_by` moves it.
#[pyclass(name = "VirtualTable", module = "rs_rich.data")]
pub(crate) struct VirtualTable {
    /// Shared with every table `core` builds, so a call copies no rows.
    rows: Arc<CoreRows>,
    offset: usize,
    height: usize,
    row_numbers: bool,
    null_marker: Option<String>,
    show_position: bool,
    wrap: bool,
    max_column_width: usize,
    fit_window: bool,
    title: Option<String>,
}

impl VirtualTable {
    fn core(&self) -> CoreVirtualTable<Arc<CoreRows>> {
        let sample = &self.rows.rows()[..self.rows.len().min(100)];
        let columns = typed_columns(self.rows.columns(), self.rows.schema(), sample);
        let mut table = CoreVirtualTable::new(columns, Arc::clone(&self.rows))
            .offset(self.offset)
            .height(self.height)
            .row_numbers(self.row_numbers)
            .show_position(self.show_position)
            .wrap(self.wrap)
            .max_column_width(self.max_column_width)
            .fit_window(self.fit_window);
        if let Some(marker) = &self.null_marker {
            table = table.null_marker(marker.clone());
        }
        if let Some(title) = &self.title {
            table = table.title(title.clone());
        }
        table
    }
}

impl AsRenderable for VirtualTable {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.core()))
    }
}

#[pymethods]
impl VirtualTable {
    #[new]
    #[pyo3(signature = (
        rows, *, offset=0, height=20, row_numbers=false, null_marker=None,
        show_position=true, wrap=false,
        max_column_width=rich_ext::table::virtualized::DEFAULT_MAX_COLUMN_WIDTH,
        fit_window=false, title=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        rows: PyRef<'_, Rows>,
        offset: usize,
        height: usize,
        row_numbers: bool,
        null_marker: Option<String>,
        show_position: bool,
        wrap: bool,
        max_column_width: usize,
        fit_window: bool,
        title: Option<String>,
    ) -> Self {
        VirtualTable {
            rows: Arc::new(rows.inner.clone()),
            offset,
            height: height.min(rich_ext::table::virtualized::MAX_HEIGHT),
            row_numbers,
            null_marker,
            show_position,
            wrap,
            max_column_width,
            fit_window,
            title,
        }
    }

    /// The first row shown (0-based).
    #[getter]
    fn get_offset(&self) -> usize {
        self.offset
    }

    #[setter]
    fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    /// The rows shown at most.
    #[getter]
    fn get_height(&self) -> usize {
        self.height
    }

    #[setter]
    fn set_height(&mut self, height: usize) {
        self.height = height.min(rich_ext::table::virtualized::MAX_HEIGHT);
    }

    #[getter]
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The largest offset that still fills the window.
    #[getter]
    fn max_offset(&self) -> usize {
        self.rows.len().saturating_sub(self.height)
    }

    /// Move the window by `rows` (negative moves up), within the rows.
    fn scroll_by(&mut self, rows: isize) {
        let mut table = self.core();
        table.scroll_by(rows);
        self.offset = table.window_offset();
    }

    /// `(start, end)`: the 0-based rows the window shows, end exclusive.
    fn window(&self) -> (usize, usize) {
        let range = self.core().page().range();
        (range.start, range.end)
    }

    /// The position line: `rows 41-60 of 1,000`.
    #[pyo3(signature = (*, ascii=false))]
    fn position(&self, ascii: bool) -> String {
        self.core().page().position(ascii)
    }

    fn __repr__(&self) -> String {
        format!(
            "<VirtualTable rows={} offset={} height={}>",
            self.rows.len(),
            self.offset,
            self.height
        )
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("DataSourceError", py.get_type::<DataSourceError>())?;
    m.add_function(pyo3::wrap_pyfunction!(read_csv, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(read_tsv, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(read_jsonl, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(read_file, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(infer, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(check_not_null, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(check_unique, m)?)?;
    renderable::add_renderable_class::<Rows>(m)?;
    renderable::add_renderable_class::<Inference>(m)?;
    m.add_class::<ColumnInference>()?;
    renderable::add_renderable_class::<Stats>(m)?;
    m.add_class::<ColumnStats>()?;
    renderable::add_renderable_class::<Profile>(m)?;
    m.add_class::<CheckResult>()?;
    renderable::add_renderable_class::<QualityReport>(m)?;
    renderable::add_renderable_class::<ResultSet>(m)?;
    renderable::add_renderable_class::<VirtualTable>(m)?;
    views::register(m)?;
    Ok(())
}
