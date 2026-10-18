//! `LogView` (#724) for `rs_rich.interact`: the view behind `rich log -i`,
//! given its records up front, built into a fresh `rich_interact::LogView`
//! per run like the other component classes.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use rich_ext::log_stream::{Parsers, Record as CoreRecord, TimeMode};
use rich_interact::LogView as CoreLogView;

use super::compose::{self, Node};
use super::{execute, record, Build, Mode, Record};
use crate::ext::common;
use crate::ext::log_stream::LogRecord;

/// One record: a `LogRecord`, a `dict` of fields, or a line read by the
/// default parsers (kept as text when none reads it).
fn record_from(value: &Bound<'_, PyAny>, parsers: &Parsers) -> PyResult<CoreRecord> {
    if let Ok(record) = value.extract::<PyRef<'_, LogRecord>>() {
        return Ok(record.inner.clone());
    }
    if value.is_instance_of::<PyString>() {
        let line: String = value.extract()?;
        return Ok(parsers
            .parse(&line)
            .unwrap_or_else(|| CoreRecord::text(line)));
    }
    if value.is_instance_of::<PyDict>() {
        let mut record = CoreRecord::new("json");
        for (key, value) in common::pairs(value)? {
            record.set(key, common::event_value(&value)?);
        }
        return Ok(record);
    }
    Err(PyTypeError::new_err(format!(
        "expected a LogRecord, a dict of fields or a line, got {}",
        value.get_type().name()?
    )))
}

/// `LogView(records=(), *, collapse=True, time="absolute", fields=False,
/// filter=None, limit=200000)`: a scrolling view of log records, newest at
/// the bottom. `/` filters (words match text, `FIELD=VALUE` and the other
/// `rich log --where` forms match fields), Enter shows a record's fields
/// and the lines that followed it, Space pauses, `q` closes. Records are
/// `LogRecord`s, dicts of fields, or lines. Without a terminal the rows are
/// written out. The answer is `None`.
#[pyclass(name = "LogView", module = "rs_rich.interact", frozen)]
pub(crate) struct LogView {
    records: Vec<CoreRecord>,
    /// Runs of identical records fold into one row with a count.
    #[pyo3(get)]
    collapse: bool,
    time: TimeMode,
    /// Every record's fields show, as if Enter had been pressed on each.
    #[pyo3(get)]
    fields: bool,
    #[pyo3(get)]
    filter: Option<String>,
    /// The most records kept; the oldest drop past it.
    #[pyo3(get)]
    limit: usize,
}

struct LogViewBuild {
    records: Vec<CoreRecord>,
    collapse: bool,
    time: TimeMode,
    fields: bool,
    filter: Option<String>,
    limit: usize,
}

impl LogView {
    fn prepare(&self) -> LogViewBuild {
        LogViewBuild {
            records: self.records.clone(),
            collapse: self.collapse,
            time: self.time,
            fields: self.fields,
            filter: self.filter.clone(),
            limit: self.limit,
        }
    }
}

impl Build for LogViewBuild {
    type C = CoreLogView;

    fn build(self) -> CoreLogView {
        let view = CoreLogView::new()
            .limit(self.limit)
            .records(self.records)
            .collapse(self.collapse)
            .time(self.time)
            .expand_all(self.fields);
        match self.filter {
            Some(filter) => view.filter(filter),
            None => view,
        }
    }
}

#[pymethods]
impl LogView {
    #[new]
    #[pyo3(signature = (
        records=None, *, collapse=true, time="absolute", fields=false, filter=None,
        limit=CoreLogView::LIMIT
    ))]
    fn new(
        records: Option<&Bound<'_, PyAny>>,
        collapse: bool,
        time: &str,
        fields: bool,
        filter: Option<String>,
        limit: usize,
    ) -> PyResult<Self> {
        let time: TimeMode = time.parse().map_err(PyValueError::new_err)?;
        let parsers = Parsers::new();
        let mut kept = Vec::new();
        if let Some(records) = records {
            for value in records.try_iter()? {
                kept.push(record_from(&value?, &parsers)?);
            }
        }
        Ok(LogView {
            records: kept,
            collapse,
            time,
            fields,
            filter,
            limit: limit.max(1),
        })
    }

    /// How times show: `"absolute"`, `"relative"` or `"none"`.
    #[getter]
    fn time(&self) -> &'static str {
        self.time.name()
    }

    /// The records, as `LogRecord`s.
    #[getter]
    fn records(&self) -> Vec<LogRecord> {
        self.records
            .iter()
            .map(|inner| LogRecord {
                inner: inner.clone(),
            })
            .collect()
    }

    fn __len__(&self) -> usize {
        self.records.len()
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

// ---------------------------------------------------------------------------
// Dispatch

/// Run `component` if it is a `LogView`; otherwise give `mode` back for the
/// next kind to try.
pub(crate) fn drive(
    py: Python<'_>,
    component: &Bound<'_, PyAny>,
    mode: Mode,
) -> PyResult<Result<Record, Mode>> {
    if let Ok(view) = component.cast::<LogView>() {
        let ran = execute(py, view.get().prepare(), mode)?;
        return record(py, ran, |()| Ok(py.None())).map(Ok);
    }
    Ok(Err(mode))
}

/// The node for a `LogView` as a child of a container (see
/// `compose::leaf`), if `component` is one.
pub(super) fn leaf(component: &Bound<'_, PyAny>) -> PyResult<Option<Node>> {
    if let Ok(view) = component.cast::<LogView>() {
        return Ok(Some(compose::leaf(view.get().prepare(), |py, ()| {
            Ok(py.None())
        })));
    }
    Ok(None)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("InteractLogView", m.py().get_type::<LogView>())
}
