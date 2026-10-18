//! `rs_rich.ext.log_stream`: log records read from JSON Lines or text logs,
//! filtered, folded and grouped (`rich_ext::log_stream`, #724).
//!
//! Names that other areas already use in the flat native module carry a
//! `Log` prefix there (`LogRecord`, `LogFilter`, ...); the Python module
//! also exports them under the Rust names.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};

use rich::protocol::Renderable;
use rich_ext::log_stream::{
    format_offset, parse_timestamp, Assembled, Assembler, Collapser as CoreCollapser,
    Filter as CoreFilter, Group, GroupBy, GroupHeader, LevelFilter as CoreLevelFilter, Parsers,
    Predicate as CorePredicate, Record as CoreRecord, Repeat, Summary as CoreSummary,
};

use super::common;
use super::diagnostic::{severity, severity_name, StructuredEvent};
use super::widgets::Redactor;
use crate::renderable::{self, AsRenderable};

fn value_error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

/// The parsers `name` asks for: `auto` (every one), `text` (JSON only),
/// or one by its name or alias, with JSON.
fn parsers(name: &str) -> PyResult<Parsers> {
    match name {
        "auto" => Ok(Parsers::new()),
        "text" => Ok(Parsers::new().only(&["json"])),
        name if Parsers::new().contains(name) => Ok(Parsers::new().only(&["json", name])),
        other => Err(PyValueError::new_err(format!(
            "unknown parser {other:?}; expected auto, json, logfmt, level, timestamp, access or \
             text"
        ))),
    }
}

fn group_by(name: &str) -> PyResult<GroupBy> {
    name.parse().map_err(value_error)
}

// ---------------------------------------------------------------------------
// Records

/// `LogRecord(fields=None, *, format="json", continuation=())`: one record
/// of a log, its fields under the names the input gave them (a `dict` or
/// `(key, value)` pairs), and the lines that followed it that no parser
/// read. Printed, it renders as its `StructuredEvent`.
#[pyclass(
    name = "LogRecord",
    module = "rs_rich.ext.log_stream",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct LogRecord {
    pub(crate) inner: CoreRecord,
}

fn record(inner: CoreRecord) -> LogRecord {
    LogRecord { inner }
}

#[pymethods]
impl LogRecord {
    #[new]
    #[pyo3(signature = (fields=None, *, format="json", continuation=None))]
    fn new(
        fields: Option<&Bound<'_, PyAny>>,
        format: &str,
        continuation: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreRecord::new(format.to_string());
        if let Some(fields) = fields {
            for (key, value) in common::pairs(fields)? {
                inner.set(key, common::event_value(&value)?);
            }
        }
        if let Some(lines) = continuation {
            inner.continuation = common::strings(lines)?;
        }
        Ok(record(inner))
    }

    /// A line no parser read: its `message` is the line.
    #[staticmethod]
    fn text(line: String) -> Self {
        record(CoreRecord::text(line))
    }

    /// The fields, in order.
    #[getter]
    fn fields<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (key, value) in &self.inner.fields {
            dict.set_item(key, common::event_value_to_py(py, value)?)?;
        }
        Ok(dict)
    }

    /// The parser that read it (`"json"`, `"logfmt"`, ..., `"text"`).
    #[getter]
    fn format(&self) -> String {
        self.inner.format.to_string()
    }

    #[getter]
    fn continuation(&self) -> Vec<String> {
        self.inner.continuation.clone()
    }

    #[getter]
    fn message(&self) -> Option<String> {
        self.inner.message().map(|message| message.into_owned())
    }

    /// The level as written.
    #[getter]
    fn level(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.inner
            .level()
            .map(|value| common::event_value_to_py(py, value))
            .transpose()
    }

    /// The severity the level names (`"warn"`, ...), or `None`.
    #[getter]
    fn severity(&self) -> Option<&'static str> {
        self.inner.severity().map(severity_name)
    }

    /// The time as written.
    #[getter]
    fn time(&self) -> Option<String> {
        self.inner.time().map(|time| time.into_owned())
    }

    /// The time in microseconds since the Unix epoch, when it parses.
    #[getter]
    fn instant(&self) -> Option<i64> {
        self.inner.instant()
    }

    /// The field named exactly `key`.
    fn get(&self, py: Python<'_>, key: &str) -> PyResult<Option<Py<PyAny>>> {
        self.inner
            .get(key)
            .map(|value| common::event_value_to_py(py, value))
            .transpose()
    }

    /// The value at `path`: a key, an alias (`message` finds `msg`), or
    /// dotted keys and list indices (`http.status`, `tags.0`).
    fn lookup(&self, py: Python<'_>, path: &str) -> PyResult<Option<Py<PyAny>>> {
        self.inner
            .lookup(path)
            .map(|value| common::event_value_to_py(py, value))
            .transpose()
    }

    /// The record as a `StructuredEvent`.
    fn to_event(&self) -> StructuredEvent {
        StructuredEvent {
            inner: self.inner.to_event(),
        }
    }

    /// Whether `other` is the same record but for its time.
    fn same_as(&self, other: PyRef<'_, LogRecord>) -> bool {
        self.inner.same_as(&other.inner)
    }

    /// A copy with secrets masked by `redactor`.
    fn redacted(&self, redactor: PyRef<'_, Redactor>) -> Self {
        let mut inner = self.inner.clone();
        inner.redact(&redactor.inner);
        record(inner)
    }

    /// A copy with terminal controls shown as symbols.
    fn sanitized(&self) -> Self {
        let mut inner = self.inner.clone();
        inner.sanitize();
        record(inner)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .cast::<LogRecord>()
            .is_ok_and(|other| other.get().inner == self.inner)
    }

    fn __repr__(&self) -> String {
        let keys: Vec<&str> = self.inner.fields.iter().map(|(k, _)| k.as_str()).collect();
        format!(
            "LogRecord(format={:?}, fields={keys:?}, continuation={})",
            self.inner.format,
            self.inner.continuation.len()
        )
    }
}

impl AsRenderable for LogRecord {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.to_event()))
    }
}

fn record_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreRecord> {
    Ok(value.extract::<PyRef<'_, LogRecord>>()?.inner.clone())
}

// ---------------------------------------------------------------------------
// Parsing

/// The record `line` holds, read by `parser` (`"auto"` tries each), or
/// `None` when no parser reads it.
#[pyfunction]
#[pyo3(signature = (line, parser="auto"))]
fn parse_log_line(line: &str, parser: &str) -> PyResult<Option<LogRecord>> {
    Ok(parsers(parser)?.parse(line).map(record))
}

/// The built-in parsers' names, in the order `"auto"` tries them.
#[pyfunction]
fn log_parser_names() -> Vec<String> {
    Parsers::new()
        .names()
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// `LogAssembler(parser="auto", *, continuation_limit=1000)`: lines in,
/// records out. A line no parser reads joins the record before it (a stack
/// trace), or is a text record. `push` gives the record a line completed
/// (or, after `flush`, a line that belongs under the last one, as a `str`);
/// `flush` gives the record being read.
#[pyclass(name = "LogAssembler", module = "rs_rich.ext.log_stream")]
pub(crate) struct LogAssembler {
    inner: Assembler,
}

#[pymethods]
impl LogAssembler {
    #[new]
    #[pyo3(signature = (parser="auto", *, continuation_limit=1000))]
    fn new(parser: &str, continuation_limit: usize) -> PyResult<Self> {
        Ok(LogAssembler {
            inner: Assembler::new(parsers(parser)?).continuation_limit(continuation_limit),
        })
    }

    fn push(&mut self, py: Python<'_>, line: &str) -> PyResult<Py<PyAny>> {
        Ok(match self.inner.push(line) {
            Some(Assembled::Record(done)) => Py::new(py, record(done))?.into_any(),
            Some(Assembled::Continuation(line)) => PyString::new(py, &line).into_any().unbind(),
            None => py.None(),
        })
    }

    fn flush(&mut self) -> Option<LogRecord> {
        self.inner.flush().map(record)
    }
}

// ---------------------------------------------------------------------------
// Filters

/// `LevelFilter(spec)`: `"error"` keeps errors, `"warn+"` warnings and
/// above, `"info,error+"` both.
#[pyclass(name = "LevelFilter", module = "rs_rich.ext.log_stream", frozen)]
pub(crate) struct LevelFilter {
    inner: CoreLevelFilter,
}

#[pymethods]
impl LevelFilter {
    #[new]
    fn new(spec: &str) -> PyResult<Self> {
        Ok(LevelFilter {
            inner: spec.parse().map_err(value_error)?,
        })
    }

    /// Whether a record at `severity` (a name, or `None`) is kept.
    #[pyo3(signature = (severity=None))]
    fn allows(&self, severity: Option<&str>) -> PyResult<bool> {
        let severity = severity.map(self::severity).transpose()?;
        Ok(self.inner.allows(severity))
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!("LevelFilter({:?})", self.inner.to_string())
    }
}

/// `LogPredicate(expression)`: one test on a field, as `rich log --where`
/// takes it: `service=api`, `latency_ms>500`, `msg~timeout`, `trace_id`,
/// `!trace_id`.
#[pyclass(name = "LogPredicate", module = "rs_rich.ext.log_stream", frozen)]
pub(crate) struct LogPredicate {
    inner: CorePredicate,
}

#[pymethods]
impl LogPredicate {
    #[new]
    fn new(expression: &str) -> PyResult<Self> {
        Ok(LogPredicate {
            inner: CorePredicate::parse(expression).map_err(value_error)?,
        })
    }

    #[getter]
    fn path(&self) -> String {
        self.inner.path().to_string()
    }

    /// The operator as written (`""` for a bare field, `"!"` for a missing
    /// one).
    #[getter]
    fn op(&self) -> &'static str {
        self.inner.op().symbol()
    }

    #[getter]
    fn value(&self) -> String {
        self.inner.value().to_string()
    }

    fn matches(&self, record: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.inner.matches(&record_arg(record)?))
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!("LogPredicate({:?})", self.inner.to_string())
    }
}

/// `LogFilter(*, level=None, predicates=())`: a level filter and
/// predicates, all of which a record must pass.
#[pyclass(name = "LogFilter", module = "rs_rich.ext.log_stream", frozen)]
pub(crate) struct LogFilter {
    inner: CoreFilter,
}

#[pymethods]
impl LogFilter {
    #[new]
    #[pyo3(signature = (*, level=None, predicates=None))]
    fn new(level: Option<&str>, predicates: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let mut inner = CoreFilter::new();
        if let Some(level) = level {
            inner = inner.levels(level.parse().map_err(value_error)?);
        }
        if let Some(predicates) = predicates {
            for expression in common::strings(predicates)? {
                inner = inner.predicate(CorePredicate::parse(&expression).map_err(value_error)?);
            }
        }
        Ok(LogFilter { inner })
    }

    fn matches(&self, record: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.inner.matches(&record_arg(record)?))
    }
}

// ---------------------------------------------------------------------------
// Repeats

/// A run of identical records: how many, and the last one's time.
#[pyclass(name = "LogRepeat", module = "rs_rich.ext.log_stream", frozen)]
pub(crate) struct LogRepeat {
    #[pyo3(get)]
    count: usize,
    #[pyo3(get)]
    last_time: Option<String>,
    #[pyo3(get)]
    last_instant: Option<i64>,
}

#[pymethods]
impl LogRepeat {
    fn __repr__(&self) -> String {
        format!(
            "LogRepeat(count={}, last_time={:?})",
            self.count, self.last_time
        )
    }
}

fn repeat(repeat: Repeat) -> LogRepeat {
    LogRepeat {
        count: repeat.count,
        last_time: repeat.last_time,
        last_instant: repeat.last_instant,
    }
}

/// `Collapser()`: folds runs of records that are the same but for their
/// time. `push(record)` returns `(ended, show)`: the run the record ended
/// (a `LogRepeat`, or `None`) and whether to show the record.
#[pyclass(name = "Collapser", module = "rs_rich.ext.log_stream")]
pub(crate) struct Collapser {
    inner: CoreCollapser,
}

#[pymethods]
impl Collapser {
    #[new]
    fn new() -> Self {
        Collapser {
            inner: CoreCollapser::new(),
        }
    }

    fn push(&mut self, record: &Bound<'_, PyAny>) -> PyResult<(Option<LogRepeat>, bool)> {
        let push = self.inner.push(&record_arg(record)?);
        Ok((push.ended.map(repeat), push.show))
    }

    /// The run so far, when it has repeats.
    fn current(&self) -> Option<LogRepeat> {
        self.inner.current().map(repeat)
    }

    /// End the run: its repeats, if any.
    fn finish(&mut self) -> Option<LogRepeat> {
        self.inner.finish().map(repeat)
    }

    /// The records folded so far.
    #[getter]
    fn folded(&self) -> usize {
        self.inner.folded()
    }
}

// ---------------------------------------------------------------------------
// Groups and the summary

/// `record`'s trace, span or correlation id (`by`), or `None`.
#[pyfunction]
fn log_group_key(record: &Bound<'_, PyAny>, by: &str) -> PyResult<Option<String>> {
    Ok(group_by(by)?.key_of(&record_arg(record)?))
}

/// `LogGroupHeader(by, key, records=None)`: a group's header line, with
/// the records' count, errors, warnings and time span when `records` (the
/// group's `LogRecord`s) are given.
#[pyclass(name = "LogGroupHeader", module = "rs_rich.ext.log_stream", frozen)]
pub(crate) struct LogGroupHeader {
    inner: GroupHeader,
}

#[pymethods]
impl LogGroupHeader {
    #[new]
    #[pyo3(signature = (by, key, records=None))]
    fn new(by: &str, key: Option<String>, records: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let by = group_by(by)?;
        let inner = match records {
            Some(records) => {
                let records: PyResult<Vec<CoreRecord>> =
                    records.try_iter()?.map(|item| record_arg(&item?)).collect();
                Group {
                    key,
                    records: records?,
                }
                .header(by)
            }
            None => GroupHeader::streaming(by, key),
        };
        Ok(LogGroupHeader { inner })
    }
}

impl AsRenderable for LogGroupHeader {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

/// `LogSummary(by=None)`: what a stream held, for its end: `read(n)`
/// counts records read, `shown(record)` one kept. Printed, it shows the
/// records shown, the counts by level and, with `by`, the groups with
/// errors.
#[pyclass(name = "LogSummary", module = "rs_rich.ext.log_stream")]
pub(crate) struct LogSummary {
    inner: CoreSummary,
}

#[pymethods]
impl LogSummary {
    #[new]
    #[pyo3(signature = (by=None))]
    fn new(by: Option<&str>) -> PyResult<Self> {
        Ok(LogSummary {
            inner: CoreSummary::new(by.map(group_by).transpose()?),
        })
    }

    #[pyo3(signature = (records=1))]
    fn read(&mut self, records: usize) {
        self.inner.read(records);
    }

    fn shown(&mut self, record: &Bound<'_, PyAny>) -> PyResult<()> {
        self.inner.shown(&record_arg(record)?);
        Ok(())
    }

    fn folded(&mut self, repeats: usize) {
        self.inner.folded(repeats);
    }

    #[getter]
    fn records_read(&self) -> usize {
        self.inner.records_read()
    }

    #[getter]
    fn records_shown(&self) -> usize {
        self.inner.records_shown()
    }

    #[getter]
    fn repeats_folded(&self) -> usize {
        self.inner.repeats_folded()
    }

    /// The records shown at each level that has any, most severe first;
    /// `"none"` counts those without a level.
    #[getter]
    fn levels<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        use rich_ext::event::Severity;
        let dict = PyDict::new(py);
        let levels = self.inner.levels();
        for severity in [
            Severity::Fatal,
            Severity::Error,
            Severity::Warn,
            Severity::Info,
            Severity::Debug,
            Severity::Trace,
        ] {
            let count = levels.get(severity);
            if count > 0 {
                dict.set_item(severity_name(severity), count)?;
            }
        }
        if levels.without_level() > 0 {
            dict.set_item("none", levels.without_level())?;
        }
        Ok(dict)
    }

    /// The groups with errors and how many, as `(id, errors)` (the id
    /// `None` for the records without one).
    #[getter]
    fn failing<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        PyList::new(py, self.inner.failing().iter().cloned())
    }
}

impl AsRenderable for LogSummary {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

// ---------------------------------------------------------------------------
// Times

/// `text` as microseconds since the Unix epoch: RFC 3339 and the ISO 8601
/// forms logs use, the access log's time, or an epoch number.
#[pyfunction]
fn parse_log_timestamp(text: &str) -> Option<i64> {
    parse_timestamp(text)
}

/// A time difference in microseconds as a relative time shows it
/// (`+1.250s`, `+1m02.500s`).
#[pyfunction]
fn format_log_offset(micros: i64) -> String {
    format_offset(micros)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<LogRecord>(m)?;
    m.add_class::<LogAssembler>()?;
    m.add_class::<LevelFilter>()?;
    m.add_class::<LogPredicate>()?;
    m.add_class::<LogFilter>()?;
    m.add_class::<LogRepeat>()?;
    m.add_class::<Collapser>()?;
    renderable::add_renderable_class::<LogGroupHeader>(m)?;
    renderable::add_renderable_class::<LogSummary>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(parse_log_line, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(log_parser_names, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(log_group_key, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(parse_log_timestamp, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_log_offset, m)?)?;
    m.add("LOG_STYLES", rich_ext::log_stream::STYLES.to_vec())?;
    Ok(())
}
