//! `rs_rich.ext.table`: `TableData` (typed rows with a stable multi-column
//! sort, grouping, subtotals and totals) and `StreamingTable` (keyed rows
//! that re-render only what changed), plus the `Sort` and `Group`
//! transforms.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use pyo3::exceptions::{PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::table::ColumnOptions;
use rich_ext::table::transform::{Group as CoreGroupTransform, Sort as CoreSortTransform};
use rich_ext::table::{
    Aggregate as CoreAggregate, Column as CoreColumn, GroupBy as CoreGroupBy, SortKey as CoreKey,
    StreamingTable as CoreStream, TableData as CoreData, Value, Window,
};
use rich_ext::transform::Transform;

use super::common::{self, TransformError};
use crate::boxes::BoxArg;
use crate::renderable::{self, AsRenderable};
use crate::text::Text;

/// A Python cell value: `None`, `int`, `float`, `str` or `Text` (anything
/// else becomes its `str`).
fn value(v: &Bound<'_, PyAny>) -> PyResult<Value> {
    if v.is_none() {
        return Ok(Value::Null);
    }
    if v.is_instance_of::<PyBool>() {
        return Ok(Value::Str(v.str()?.to_string()));
    }
    if v.is_instance_of::<PyInt>() {
        if let Ok(n) = v.extract::<i64>() {
            return Ok(Value::Int(n));
        }
        return Ok(Value::Str(v.str()?.to_string()));
    }
    if let Ok(f) = v.cast::<PyFloat>() {
        return Ok(Value::Float(f.value()));
    }
    if let Ok(s) = v.cast::<PyString>() {
        return Ok(Value::Str(s.to_cow()?.into_owned()));
    }
    if let Ok(t) = v.extract::<PyRef<'_, Text>>() {
        return Ok(Value::Text(t.inner.clone()));
    }
    Ok(Value::Str(v.str()?.to_string()))
}

fn value_to_py(py: Python<'_>, v: &Value) -> PyResult<Py<PyAny>> {
    Ok(match v {
        Value::Null => py.None(),
        Value::Int(n) => n.into_pyobject(py)?.into_any().unbind(),
        Value::Float(f) => f.into_pyobject(py)?.into_any().unbind(),
        Value::Str(s) => s.into_pyobject(py)?.into_any().unbind(),
        Value::Text(t) => common::py_text(py, t.clone())?.into_any(),
    })
}

fn row(values: &Bound<'_, PyAny>) -> PyResult<Vec<Value>> {
    values.try_iter()?.map(|v| value(&v?)).collect()
}

fn rows_to_py(py: Python<'_>, rows: &[Vec<Value>]) -> PyResult<Vec<Vec<Py<PyAny>>>> {
    rows.iter()
        .map(|r| r.iter().map(|v| value_to_py(py, v)).collect())
        .collect()
}

// ---------------------------------------------------------------------------
// Columns, sort keys and aggregates

/// `DataColumn(header, *, justify="left", style=None, width=None,
/// min_width=None, max_width=None, ratio=None, no_wrap=False,
/// overflow="ellipsis", format=None)`: a column of a `TableData` or
/// `StreamingTable`. `format(value)` returns the cell (`str` or `Text`).
#[pyclass(
    name = "DataColumn",
    module = "rs_rich.ext.table",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct DataColumn {
    inner: CoreColumn,
}

#[pymethods]
impl DataColumn {
    #[new]
    #[pyo3(signature = (
        header, *, justify="left", style=None, width=None, min_width=None, max_width=None,
        ratio=None, no_wrap=false, overflow="ellipsis", format=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        header: String,
        justify: &str,
        style: Option<&Bound<'_, PyAny>>,
        width: Option<usize>,
        min_width: Option<usize>,
        max_width: Option<usize>,
        ratio: Option<usize>,
        no_wrap: bool,
        overflow: &str,
        format: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let options = ColumnOptions {
            justify: crate::convert::justify(Some(justify))?,
            width,
            min_width,
            max_width,
            ratio,
            no_wrap,
            overflow: crate::convert::overflow(overflow)?,
            style: common::style(style)?.unwrap_or_default(),
        };
        let mut inner = CoreColumn::new(header).options(options);
        if let Some(format) = format {
            let format = Arc::new(format);
            inner = inner.format(move |v: &Value| {
                Python::attach(|py| {
                    let result = value_to_py(py, v)
                        .and_then(|arg| format.bind(py).call1((arg,)))
                        .and_then(|out| common::markup_or_text(&out));
                    result.unwrap_or_else(|error| {
                        error.write_unraisable(py, Some(format.bind(py)));
                        v.to_text()
                    })
                })
            });
        }
        Ok(DataColumn { inner })
    }

    #[getter]
    fn header(&self) -> String {
        self.inner.header().to_string()
    }

    /// The cell a value becomes in this column.
    fn cell(&self, py: Python<'_>, v: &Bound<'_, PyAny>) -> PyResult<Py<Text>> {
        common::py_text(py, self.inner.cell(&value(v)?))
    }
}

fn columns(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreColumn>> {
    value
        .try_iter()?
        .map(|c| {
            let c = c?;
            if let Ok(header) = c.extract::<String>() {
                return Ok(CoreColumn::new(header));
            }
            Ok(c.extract::<PyRef<'_, DataColumn>>()?.inner.clone())
        })
        .collect()
}

/// `SortKey(column, *, descending=False, lexical=False)`: natural order
/// (numbers by value, `file10` after `file9`) unless `lexical`.
#[pyclass(
    name = "SortKey",
    module = "rs_rich.ext.table",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct SortKey {
    inner: CoreKey,
}

#[pymethods]
impl SortKey {
    #[new]
    #[pyo3(signature = (column, *, descending=false, lexical=false))]
    fn new(column: usize, descending: bool, lexical: bool) -> Self {
        let mut inner = if descending {
            CoreKey::desc(column)
        } else {
            CoreKey::asc(column)
        };
        if lexical {
            inner = inner.lexical();
        }
        SortKey { inner }
    }

    #[getter]
    fn column(&self) -> usize {
        self.inner.column
    }

    #[getter]
    fn descending(&self) -> bool {
        self.inner.order == rich_ext::table::Order::Descending
    }

    #[getter]
    fn lexical(&self) -> bool {
        self.inner.compare == rich_ext::table::Compare::Lexical
    }
}

/// Sort keys from `SortKey`s or column numbers (ascending).
fn sort_keys(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreKey>> {
    if let Ok(column) = value.extract::<usize>() {
        return Ok(vec![CoreKey::asc(column)]);
    }
    if let Ok(key) = value.extract::<PyRef<'_, SortKey>>() {
        return Ok(vec![key.inner]);
    }
    value
        .try_iter()?
        .map(|k| {
            let k = k?;
            if let Ok(column) = k.extract::<usize>() {
                return Ok(CoreKey::asc(column));
            }
            Ok(k.extract::<PyRef<'_, SortKey>>()?.inner)
        })
        .collect()
}

/// `Aggregate(kind, column)`: `count`, `sum`, `min`, `max` or `mean` of a
/// column, or `Aggregate.custom(column, function)` with
/// `function(values) -> value`.
#[pyclass(
    name = "Aggregate",
    module = "rs_rich.ext.table",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Aggregate {
    inner: CoreAggregate,
}

#[pymethods]
impl Aggregate {
    #[new]
    fn new(kind: &str, column: usize) -> PyResult<Self> {
        let inner = match kind {
            "count" => CoreAggregate::count(column),
            "sum" => CoreAggregate::sum(column),
            "min" => CoreAggregate::min(column),
            "max" => CoreAggregate::max(column),
            "mean" => CoreAggregate::mean(column),
            other => {
                return Err(PyValueError::new_err(format!(
                    "invalid aggregate {other:?}; expected count, sum, min, max or mean"
                )))
            }
        };
        Ok(Aggregate { inner })
    }

    /// An aggregate computed by `function(values)` (a list of cell values).
    #[staticmethod]
    fn custom(column: usize, function: Py<PyAny>) -> Self {
        let function = Arc::new(function);
        Aggregate {
            inner: CoreAggregate::custom(column, move |values: &[&Value]| {
                Python::attach(|py| {
                    let result = values
                        .iter()
                        .map(|v| value_to_py(py, v))
                        .collect::<PyResult<Vec<_>>>()
                        .and_then(|args| function.bind(py).call1((PyList::new(py, args)?,)))
                        .and_then(|out| value(&out));
                    result.unwrap_or_else(|error| {
                        error.write_unraisable(py, Some(function.bind(py)));
                        Value::Null
                    })
                })
            }),
        }
    }

    #[getter]
    fn column(&self) -> usize {
        self.inner.column()
    }

    /// The aggregate over some values.
    fn compute(&self, py: Python<'_>, values: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let values = row(values)?;
        let refs: Vec<&Value> = values.iter().collect();
        value_to_py(py, &self.inner.compute(&refs))
    }
}

fn aggregates(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreAggregate>> {
    value
        .try_iter()?
        .map(|a| Ok(a?.extract::<PyRef<'_, Aggregate>>()?.inner.clone()))
        .collect()
}

/// `GroupBy(column, *, aggregates=(), label="subtotal")`: rows grouped by a
/// column's value, each group with a subtotal row.
#[pyclass(
    name = "GroupBy",
    module = "rs_rich.ext.table",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct GroupBy {
    inner: CoreGroupBy,
}

#[pymethods]
impl GroupBy {
    #[new]
    #[pyo3(signature = (column, *, aggregates=None, label=None))]
    fn new(
        column: usize,
        aggregates: Option<&Bound<'_, PyAny>>,
        label: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreGroupBy::new(column);
        if let Some(items) = aggregates {
            for aggregate in self::aggregates(items)? {
                inner = inner.aggregate(aggregate);
            }
        }
        if let Some(label) = label {
            inner = inner.label(label);
        }
        Ok(GroupBy { inner })
    }

    #[getter]
    fn column(&self) -> usize {
        self.inner.column()
    }

    #[getter]
    fn label(&self) -> String {
        self.inner.summary_label().to_string()
    }
}

// ---------------------------------------------------------------------------
// TableData

/// Frame settings (title, box, edges) shared by both table kinds.
#[derive(Clone, Default)]
struct FrameArgs {
    title: Option<String>,
    caption: Option<String>,
    box_set: Option<Option<rich::r#box::Box>>,
    show_edge: Option<bool>,
    expand: Option<bool>,
    border_style: Option<rich::Style>,
}

macro_rules! apply_frame {
    ($target:expr, $frame:expr) => {{
        let frame = $frame;
        let mut target = $target;
        if let Some(title) = &frame.title {
            target = target.title(title.clone());
        }
        if let Some(caption) = &frame.caption {
            target = target.caption(caption.clone());
        }
        match frame.box_set {
            Some(Some(b)) => target = target.box_set(b),
            Some(None) => target = target.without_box(),
            None => {}
        }
        if let Some(show) = frame.show_edge {
            target = target.show_edge(show);
        }
        if let Some(expand) = frame.expand {
            target = target.expand(expand);
        }
        if let Some(style) = &frame.border_style {
            target = target.border_style(style.clone());
        }
        target
    }};
}

fn frame_args(
    title: Option<String>,
    caption: Option<String>,
    r#box: BoxArg,
    show_edge: Option<bool>,
    expand: Option<bool>,
    border_style: Option<&Bound<'_, PyAny>>,
) -> PyResult<FrameArgs> {
    Ok(FrameArgs {
        title,
        caption,
        box_set: match r#box {
            BoxArg::Default => None,
            BoxArg::NoBox => Some(None),
            BoxArg::Box(b) => Some(Some(b)),
        },
        show_edge,
        expand,
        border_style: common::style(border_style)?,
    })
}

/// `TableData(columns, rows=(), *, sort=None, group_by=None, totals=None,
/// totals_label="total", title=None, caption=None, box=HEAVY_HEAD,
/// show_edge=True, expand=False, border_style=None)`: typed rows rendered
/// as a table, sorted stably (headers show `▲`/`▼`), optionally grouped
/// with subtotals and a totals row. `columns` are `DataColumn`s or header
/// strings.
#[pyclass(name = "TableData", module = "rs_rich.ext.table", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct TableData {
    pub(crate) inner: CoreData,
}

impl AsRenderable for TableData {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl TableData {
    #[new]
    #[pyo3(signature = (
        columns, rows=None, *, sort=None, group_by=None, totals=None, totals_label="total",
        title=None, caption=None, r#box=BoxArg::Default, show_edge=None, expand=None,
        border_style=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        columns: &Bound<'_, PyAny>,
        rows: Option<&Bound<'_, PyAny>>,
        sort: Option<&Bound<'_, PyAny>>,
        group_by: Option<PyRef<'_, GroupBy>>,
        totals: Option<&Bound<'_, PyAny>>,
        totals_label: &str,
        title: Option<String>,
        caption: Option<String>,
        r#box: BoxArg,
        show_edge: Option<bool>,
        expand: Option<bool>,
        border_style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let frame = frame_args(title, caption, r#box, show_edge, expand, border_style)?;
        let mut inner = apply_frame!(CoreData::new(self::columns(columns)?), &frame);
        if let Some(rows) = rows {
            for r in rows.try_iter()? {
                inner.push(row(&r?)?);
            }
        }
        if let Some(sort) = sort {
            inner = inner.sort_by(sort_keys(sort)?);
        }
        if let Some(group) = group_by {
            inner = inner.group_by(group.inner.clone());
        }
        if let Some(totals) = totals {
            inner = inner.totals(totals_label, aggregates(totals)?);
        }
        Ok(TableData { inner })
    }

    /// Add a row (missing cells are empty). Returns the table.
    fn push<'py>(
        mut slf: PyRefMut<'py, Self>,
        values: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let values = row(values)?;
        slf.inner.push(values);
        Ok(slf)
    }

    /// Add rows. Returns the table.
    fn extend<'py>(
        mut slf: PyRefMut<'py, Self>,
        rows: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        for r in rows.try_iter()? {
            let values = row(&r?)?;
            slf.inner.push(values);
        }
        Ok(slf)
    }

    /// Replace the sort keys (`SortKey`s or column numbers).
    fn sort_by<'py>(
        mut slf: PyRefMut<'py, Self>,
        keys: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let keys = sort_keys(keys)?;
        slf.inner.set_sort(keys);
        Ok(slf)
    }

    #[getter]
    fn headers(&self) -> Vec<String> {
        self.inner
            .columns()
            .iter()
            .map(|c| c.header().to_string())
            .collect()
    }

    /// The rows as given (values, not cells).
    #[getter]
    fn rows(&self, py: Python<'_>) -> PyResult<Vec<Vec<Py<PyAny>>>> {
        rows_to_py(py, self.inner.rows())
    }

    /// Row indices in display order.
    fn order(&self) -> Vec<usize> {
        self.inner.order()
    }

    fn __len__(&self) -> usize {
        self.inner.rows().len()
    }
}

fn data_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreData> {
    Ok(value.extract::<PyRef<'_, TableData>>()?.inner.clone())
}

/// `TableSort(keys)` (`Sort` in `rs_rich.ext.table`): a transform setting a `TableData`'s sort.
#[pyclass(name = "TableSort", module = "rs_rich.ext.table", frozen)]
pub(crate) struct Sort {
    keys: Vec<CoreKey>,
}

#[pymethods]
impl Sort {
    #[new]
    fn new(keys: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Sort {
            keys: sort_keys(keys)?,
        })
    }

    fn apply(&self, data: &Bound<'_, PyAny>) -> PyResult<TableData> {
        CoreSortTransform(self.keys.clone())
            .apply(data_arg(data)?)
            .map(|inner| TableData { inner })
            .map_err(|e| TransformError::new_err(e.message().to_string()))
    }

    fn __call__(&self, data: &Bound<'_, PyAny>) -> PyResult<TableData> {
        self.apply(data)
    }
}

/// `TableGroup(group_by)` (`Group` in `rs_rich.ext.table`): a transform setting a `TableData`'s grouping.
#[pyclass(name = "TableGroup", module = "rs_rich.ext.table", frozen)]
pub(crate) struct Group {
    group: CoreGroupBy,
}

#[pymethods]
impl Group {
    #[new]
    fn new(group_by: PyRef<'_, GroupBy>) -> Self {
        Group {
            group: group_by.inner.clone(),
        }
    }

    fn apply(&self, data: &Bound<'_, PyAny>) -> PyResult<TableData> {
        CoreGroupTransform(self.group.clone())
            .apply(data_arg(data)?)
            .map(|inner| TableData { inner })
            .map_err(|e| TransformError::new_err(e.message().to_string()))
    }

    fn __call__(&self, data: &Bound<'_, PyAny>) -> PyResult<TableData> {
        self.apply(data)
    }
}

// ---------------------------------------------------------------------------
// StreamingTable

/// `StreamingTable(columns, *, window="all", capacity=None, sort=None,
/// title=None, ...)`: rows by key, upserted as data arrives; renders only
/// rows that changed, and keeps the newest `capacity` rows. `window` is
/// `"all"`, `("head", n)` or `("tail", n)`. Keys are any hashable values.
#[pyclass(name = "StreamingTable", module = "rs_rich.ext.table")]
pub(crate) struct StreamingTable {
    pub(crate) inner: Arc<Mutex<CoreStream<u64>>>,
    ids: Py<PyDict>,
    keys: HashMap<u64, Py<PyAny>>,
    next: u64,
}

fn window(value: Option<&Bound<'_, PyAny>>) -> PyResult<Window> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(Window::All);
    };
    if let Ok(name) = value.extract::<String>() {
        return match name.as_str() {
            "all" => Ok(Window::All),
            other => Err(PyValueError::new_err(format!("invalid window {other:?}"))),
        };
    }
    let (kind, rows): (String, usize) = value.extract()?;
    match kind.as_str() {
        "head" => Ok(Window::Head(rows)),
        "tail" => Ok(Window::Tail(rows)),
        other => Err(PyValueError::new_err(format!(
            "invalid window {other:?}; expected \"all\", (\"head\", n) or (\"tail\", n)"
        ))),
    }
}

impl StreamingTable {
    fn id(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<Option<u64>> {
        self.ids
            .bind(py)
            .get_item(key)?
            .map(|id| id.extract())
            .transpose()
    }

    fn table(&self) -> MutexGuard<'_, CoreStream<u64>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A streaming table shared with its renders: the table keeps its render
/// cache between frames, so a render uses the table itself, not a copy.
struct SharedStream(Arc<Mutex<CoreStream<u64>>>);

impl Renderable for SharedStream {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let table = self.0.lock().unwrap_or_else(|e| e.into_inner());
        table.rich_render(console, options)
    }
}

impl AsRenderable for StreamingTable {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(SharedStream(self.inner.clone())))
    }
}

#[pymethods]
impl StreamingTable {
    #[new]
    #[pyo3(signature = (
        columns, *, window=None, capacity=None, sort=None, title=None, caption=None,
        r#box=BoxArg::Default, show_edge=None, expand=None, border_style=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        columns: &Bound<'_, PyAny>,
        window: Option<&Bound<'_, PyAny>>,
        capacity: Option<usize>,
        sort: Option<&Bound<'_, PyAny>>,
        title: Option<String>,
        caption: Option<String>,
        r#box: BoxArg,
        show_edge: Option<bool>,
        expand: Option<bool>,
        border_style: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let frame = frame_args(title, caption, r#box, show_edge, expand, border_style)?;
        let mut inner = apply_frame!(CoreStream::new(self::columns(columns)?), &frame)
            .window(self::window(window)?);
        if let Some(rows) = capacity {
            inner = inner.capacity(rows);
        }
        if let Some(sort) = sort {
            inner = inner.sort_by(sort_keys(sort)?);
        }
        Ok(StreamingTable {
            inner: Arc::new(Mutex::new(inner)),
            ids: PyDict::new(py).unbind(),
            keys: HashMap::new(),
            next: 0,
        })
    }

    /// Insert or replace the row for `key`; `True` if anything changed.
    fn upsert(
        &mut self,
        py: Python<'_>,
        key: &Bound<'_, PyAny>,
        values: &Bound<'_, PyAny>,
    ) -> PyResult<bool> {
        let values = row(values)?;
        let id = match self.id(py, key)? {
            Some(id) => id,
            None => {
                let id = self.next;
                self.next += 1;
                self.ids.bind(py).set_item(key, id)?;
                self.keys.insert(id, key.clone().unbind());
                id
            }
        };
        let changed = self.table().upsert(id, values);
        self.forget_evicted(py)?;
        Ok(changed)
    }

    /// Set one cell of `key`'s row; `False` when there is no such row.
    fn update_cell(
        &mut self,
        py: Python<'_>,
        key: &Bound<'_, PyAny>,
        column: usize,
        v: &Bound<'_, PyAny>,
    ) -> PyResult<bool> {
        let Some(id) = self.id(py, key)? else {
            return Ok(false);
        };
        Ok(self.table().update_cell(&id, column, value(v)?))
    }

    /// Remove `key`'s row; returns its values, or `None`.
    fn remove(
        &mut self,
        py: Python<'_>,
        key: &Bound<'_, PyAny>,
    ) -> PyResult<Option<Vec<Py<PyAny>>>> {
        let Some(id) = self.id(py, key)? else {
            return Ok(None);
        };
        self.ids.bind(py).del_item(key)?;
        self.keys.remove(&id);
        self.table()
            .remove(&id)
            .map(|values| values.iter().map(|v| value_to_py(py, v)).collect())
            .transpose()
    }

    fn clear(&mut self, py: Python<'_>) {
        self.table().clear();
        self.ids.bind(py).clear();
        self.keys.clear();
    }

    /// `key`'s values, or `None`.
    fn get(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<Option<Vec<Py<PyAny>>>> {
        let Some(id) = self.id(py, key)? else {
            return Ok(None);
        };
        self.table()
            .get(&id)
            .map(|values| values.iter().map(|v| value_to_py(py, v)).collect())
            .transpose()
    }

    fn __contains__(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self
            .id(py, key)?
            .is_some_and(|id| self.table().contains_key(&id)))
    }

    fn __len__(&self) -> usize {
        self.table().len()
    }

    fn __getitem__(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
        self.get(py, key)?
            .ok_or_else(|| PyKeyError::new_err(key.clone().unbind()))
    }

    /// `(key, values)` in insertion order.
    fn rows(&self, py: Python<'_>) -> PyResult<Vec<(Py<PyAny>, Vec<Py<PyAny>>)>> {
        self.table()
            .rows()
            .map(|(id, values)| {
                let key = self
                    .keys
                    .get(id)
                    .map(|k| k.clone_ref(py))
                    .unwrap_or_else(|| py.None());
                let values = values
                    .iter()
                    .map(|v| value_to_py(py, v))
                    .collect::<PyResult<_>>()?;
                Ok((key, values))
            })
            .collect()
    }

    /// How many rows the capacity pushed out.
    #[getter]
    fn evicted(&self) -> u64 {
        self.table().evicted()
    }

    #[setter]
    fn set_window(&mut self, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        self.table().set_window(window(value)?);
        Ok(())
    }

    /// Replace the sort keys.
    fn sort_by(&mut self, keys: &Bound<'_, PyAny>) -> PyResult<()> {
        self.table().set_sort(sort_keys(keys)?);
        Ok(())
    }

    /// Render statistics: `frames`, `rows_prepared`, `rows_rendered`,
    /// `relayouts`.
    fn stats(&self) -> HashMap<&'static str, u64> {
        let s = self.table().stats();
        HashMap::from([
            ("frames", s.frames),
            ("rows_prepared", s.rows_prepared),
            ("rows_rendered", s.rows_rendered),
            ("relayouts", s.relayouts),
        ])
    }

    fn reset_stats(&self) {
        self.table().reset_stats();
    }

    /// Drop the render cache (after a theme change, say).
    fn invalidate(&self) {
        self.table().invalidate();
    }

    /// The rows as a `TableData`, in display order.
    fn to_data(&self) -> TableData {
        TableData {
            inner: self.table().to_data(),
        }
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.ids)?;
        for key in self.keys.values() {
            visit.call(key)?;
        }
        Ok(())
    }
}

impl StreamingTable {
    /// Forget the keys of rows the capacity evicted.
    fn forget_evicted(&mut self, py: Python<'_>) -> PyResult<()> {
        let gone: Vec<u64> = self
            .keys
            .keys()
            .copied()
            .filter(|id| !self.table().contains_key(id))
            .collect();
        for id in gone {
            if let Some(key) = self.keys.remove(&id) {
                self.ids.bind(py).del_item(key.bind(py))?;
            }
        }
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<DataColumn>()?;
    m.add_class::<SortKey>()?;
    m.add_class::<Aggregate>()?;
    m.add_class::<GroupBy>()?;
    renderable::add_renderable_class::<TableData>(m)?;
    m.add_class::<Sort>()?;
    m.add_class::<Group>()?;
    renderable::add_renderable_class::<StreamingTable>(m)?;
    m.add("TABLE_STYLES", rich_ext::table::STYLES.to_vec())?;
    Ok(())
}
