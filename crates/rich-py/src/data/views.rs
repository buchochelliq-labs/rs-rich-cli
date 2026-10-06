//! `rs_rich.data`'s views: the format-neutral schema model read from JSON
//! Schema or SQL DDL (`Schema`), drawn as a tree (`SchemaTree`), compared
//! (`SchemaDiff`), laid on a timeline (`SchemaTimeline`) and as an ER
//! diagram (`ErDiagram`, `rich_data::er`); a file's merge conflicts
//! (`ConflictView`, `rich_ext::diff`); and one record as a table of fields
//! (`RecordView`, `rich_ext::data`).

use std::borrow::Cow;
use std::str::FromStr;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use rich::protocol::Renderable;
use rich_data::er;
use rich_diagram::{Direction, ErDiagram as CoreErDiagram};
use rich_ext::data::{Path, RecordView as CoreRecordView};
use rich_ext::diff::{
    ConflictError as CoreConflictError, ConflictLayout, ConflictView as CoreConflictView,
};
use rich_ext::schema::sql::SqlError;
use rich_ext::schema::{
    self as core_schema, ChangeKind, Field as CoreField, Schema as CoreSchema,
    SchemaDiff as CoreSchemaDiff, SchemaTimeline as CoreTimeline, SchemaTree as CoreTree,
};

use crate::art::{repr_opt, repr_str};
use crate::ext::common::names;
use crate::ext::data::node_arg;
use crate::renderable::{self, AsRenderable};

create_exception!(_native, SchemaError, PyException);
create_exception!(_native, ConflictError, PyException);

fn sql_error(error: &SqlError) -> PyErr {
    let exception = SchemaError::new_err(error.to_string());
    Python::attach(|py| {
        let value = exception.value(py);
        let _ = value.setattr("line", error.line());
    });
    exception
}

fn conflict_error(error: &CoreConflictError) -> PyErr {
    let exception = ConflictError::new_err(error.to_string());
    Python::attach(|py| {
        let value = exception.value(py);
        let _ = value.setattr("line", error.line);
    });
    exception
}

/// A JSON Schema given as JSON text or as Python values (`dict`, `bool`).
fn json_schema(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    let text: String = if let Ok(text) = value.cast::<PyString>() {
        text.to_cow()?.into_owned()
    } else {
        value
            .py()
            .import("json")?
            .call_method1("dumps", (value,))?
            .extract()?
    };
    core_schema::parse(&text).map_err(|e| SchemaError::new_err(e.to_string()))
}

// ---------------------------------------------------------------------------
// The model

/// A field of a `Schema` (`rich_ext::schema::Field`).
#[pyclass(name = "SchemaField", module = "rs_rich.data", frozen)]
pub(crate) struct SchemaField {
    inner: CoreField,
}

#[pymethods]
impl SchemaField {
    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    /// The type as the source writes it (`VARCHAR(80)`), else the model's
    /// (`string`, `list<integer>`).
    #[getter]
    fn r#type(&self) -> String {
        self.inner.type_label()
    }

    #[getter]
    fn required(&self) -> bool {
        self.inner.is_required()
    }

    #[getter]
    fn nullable(&self) -> bool {
        self.inner.is_nullable()
    }

    #[getter]
    fn description(&self) -> Option<&str> {
        self.inner.description()
    }

    /// Each constraint as the tree writes it: `minLength=1`,
    /// `primary key`, `→ users.id`.
    #[getter]
    fn constraints(&self) -> Vec<String> {
        self.inner
            .constraints()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[getter]
    fn primary_key(&self) -> bool {
        self.inner.is_primary_key()
    }

    #[getter]
    fn unique(&self) -> bool {
        self.inner.is_unique()
    }

    /// The table and column a foreign key refers to (`users.id`), or `None`.
    #[getter]
    fn references(&self) -> Option<String> {
        self.inner.references().map(ToString::to_string)
    }

    /// The fields of a struct, a list's item, or a map's key and value.
    #[getter]
    fn children(&self) -> Vec<SchemaField> {
        self.inner
            .children()
            .map(|f| SchemaField { inner: f.clone() })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "SchemaField(name={}, type={}, required={})",
            repr_str(self.inner.name()),
            repr_str(&self.inner.type_label()),
            if self.inner.is_required() {
                "True"
            } else {
                "False"
            }
        )
    }
}

/// A schema in the format-neutral model (`rich_ext::schema::Schema`): its
/// fields, or its tables (SQL DDL), each with its columns. Read one with
/// `Schema.from_json_schema` or `Schema.from_sql`; `Inference.schema` gives
/// rows' schema. It renders as its `SchemaTree`.
#[pyclass(name = "Schema", module = "rs_rich.data", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Schema {
    pub(crate) model: CoreSchema,
    /// The JSON Schema it was read from, which the tree and diffs read
    /// directly (following `$ref`s).
    json: Option<serde_json::Value>,
    notes: Vec<String>,
}

impl Schema {
    pub(crate) fn from_model(model: CoreSchema) -> Self {
        Schema {
            model,
            json: None,
            notes: Vec::new(),
        }
    }

    fn tree(&self) -> CoreTree {
        match &self.json {
            Some(json) => CoreTree::new(json.clone()),
            None => CoreTree::from_model(self.model.clone()),
        }
    }
}

impl AsRenderable for Schema {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.tree()))
    }
}

/// A `Schema`, or a JSON Schema as a `dict` or JSON text.
fn schema_arg(value: &Bound<'_, PyAny>) -> PyResult<Schema> {
    if let Ok(schema) = value.extract::<PyRef<'_, Schema>>() {
        return Ok(schema.clone());
    }
    if value.cast::<PyString>().is_ok()
        || value.cast::<PyDict>().is_ok()
        || value.is_instance_of::<pyo3::types::PyBool>()
    {
        return Schema::from_json_value(json_schema(value)?);
    }
    Err(PyTypeError::new_err(format!(
        "expected a Schema, or a JSON Schema as a dict or JSON text, got {}",
        value.get_type().name()?
    )))
}

impl Schema {
    fn from_json_value(json: serde_json::Value) -> PyResult<Schema> {
        Ok(Schema {
            model: rich_ext::schema::json::to_model(&json),
            json: Some(json),
            notes: Vec::new(),
        })
    }
}

#[pymethods]
impl Schema {
    /// Read a JSON Schema (a `dict`, or JSON text). Raises `SchemaError`
    /// when it is not one.
    #[staticmethod]
    fn from_json_schema(schema: &Bound<'_, PyAny>) -> PyResult<Schema> {
        Schema::from_json_value(json_schema(schema)?)
    }

    /// Read SQL DDL: a table per `CREATE TABLE`, with column types, `NOT
    /// NULL`, keys, `UNIQUE`, `CHECK`, defaults and references. What the
    /// reader skipped is in `notes`; raises `SchemaError` (with `line`) for
    /// DDL it cannot read.
    #[staticmethod]
    fn from_sql(ddl: &str) -> PyResult<Schema> {
        let parsed = rich_ext::schema::sql::parse(ddl).map_err(|e| sql_error(&e))?;
        Ok(Schema {
            model: parsed.schema,
            json: None,
            notes: parsed.notes.iter().map(ToString::to_string).collect(),
        })
    }

    #[getter]
    fn name(&self) -> Option<&str> {
        self.model.name()
    }

    #[getter]
    fn description(&self) -> Option<&str> {
        self.model.description()
    }

    #[getter]
    fn fields(&self) -> Vec<SchemaField> {
        self.model
            .fields()
            .iter()
            .map(|f| SchemaField { inner: f.clone() })
            .collect()
    }

    /// The tables of a schema read from DDL (each a `Schema`).
    #[getter]
    fn tables(&self) -> Vec<Schema> {
        self.model
            .tables()
            .iter()
            .map(|t| Schema::from_model(t.clone()))
            .collect()
    }

    /// The table called `name`, or `None`.
    fn table(&self, name: &str) -> Option<Schema> {
        self.model
            .table(name)
            .map(|t| Schema::from_model(t.clone()))
    }

    /// The primary key's columns.
    #[getter]
    fn primary_key(&self) -> Vec<String> {
        self.model
            .primary_key()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// What the DDL reader skipped, as `line N: what`.
    #[getter]
    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn __len__(&self) -> usize {
        self.model.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Schema name={} fields={} tables={}>",
            repr_opt(self.model.name()),
            self.model.fields().len(),
            self.model.tables().len()
        )
    }
}

// ---------------------------------------------------------------------------
// Tree, diff and timeline

/// A schema drawn as a tree (`rich_ext::schema::SchemaTree`): each field
/// with its type, `(required)`, constraints and description. `schema` is a
/// `Schema`, or a JSON Schema as a `dict` or JSON text (its `$ref`s are
/// resolved and drawn in place).
#[pyclass(name = "SchemaTree", module = "rs_rich.data", frozen)]
pub(crate) struct SchemaTree {
    schema: Schema,
    title: Option<String>,
    max_depth: usize,
}

impl AsRenderable for SchemaTree {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let mut tree = self.schema.tree().max_depth(self.max_depth);
        if let Some(title) = &self.title {
            tree = tree.title(title.clone());
        }
        Ok(Box::new(tree))
    }
}

#[pymethods]
impl SchemaTree {
    #[new]
    #[pyo3(signature = (schema, *, title=None, max_depth=32))]
    fn new(schema: &Bound<'_, PyAny>, title: Option<String>, max_depth: usize) -> PyResult<Self> {
        Ok(SchemaTree {
            schema: schema_arg(schema)?,
            title,
            max_depth,
        })
    }

    #[getter]
    fn schema(&self) -> Schema {
        self.schema.clone()
    }

    fn __repr__(&self) -> String {
        format!("<SchemaTree {}>", repr_opt(self.schema.model.name()))
    }
}

/// One difference between two schemas: `kind` (`"added"`, `"removed"` or
/// `"changed"`), its `marker` (`+`, `-`, `~`), the `path`, the `detail` in
/// words, and whether it is `breaking`.
#[pyclass(name = "SchemaChange", module = "rs_rich.data", frozen)]
pub(crate) struct SchemaChange {
    #[pyo3(get)]
    kind: &'static str,
    #[pyo3(get)]
    marker: &'static str,
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    detail: String,
    #[pyo3(get)]
    breaking: bool,
}

#[pymethods]
impl SchemaChange {
    fn __repr__(&self) -> String {
        format!(
            "SchemaChange({} {} {}{})",
            self.marker,
            repr_str(&self.path),
            repr_str(&self.detail),
            if self.breaking { ", breaking" } else { "" }
        )
    }
}

fn changes(diff: &CoreSchemaDiff) -> Vec<SchemaChange> {
    diff.changes()
        .iter()
        .map(|c| SchemaChange {
            kind: match c.kind {
                ChangeKind::Added => "added",
                ChangeKind::Removed => "removed",
                ChangeKind::Changed => "changed",
            },
            marker: c.kind.marker(),
            path: c.path.clone(),
            detail: c.detail.clone(),
            breaking: c.breaking,
        })
        .collect()
}

fn diff_of(old: &Schema, new: &Schema) -> CoreSchemaDiff {
    match (&old.json, &new.json) {
        (Some(a), Some(b)) => CoreSchemaDiff::new(a, b),
        _ => CoreSchemaDiff::models(&old.model, &new.model),
    }
}

/// What changed between two schemas (`rich_ext::schema::SchemaDiff`):
/// fields added and removed, type changes, required, enum values,
/// constraints and keys, each marked `+`, `-` or `~`, and `breaking` where
/// data the old schema accepted may now be refused. Two JSON Schemas are
/// compared directly; anything else through the model, so DDL versions
/// diff the same way.
#[pyclass(name = "SchemaDiff", module = "rs_rich.data", frozen)]
pub(crate) struct SchemaDiff {
    inner: CoreSchemaDiff,
}

impl AsRenderable for SchemaDiff {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl SchemaDiff {
    #[new]
    #[pyo3(signature = (old, new, *, old_name="old", new_name="new"))]
    fn new(
        old: &Bound<'_, PyAny>,
        new: &Bound<'_, PyAny>,
        old_name: &str,
        new_name: &str,
    ) -> PyResult<Self> {
        let (old, new) = (schema_arg(old)?, schema_arg(new)?);
        Ok(SchemaDiff {
            inner: diff_of(&old, &new).names(old_name, new_name),
        })
    }

    #[getter]
    fn changes(&self) -> Vec<SchemaChange> {
        changes(&self.inner)
    }

    /// How many changes are breaking.
    #[getter]
    fn breaking(&self) -> usize {
        self.inner.breaking()
    }

    /// Whether the comparison stopped at its limit.
    #[getter]
    fn truncated(&self) -> bool {
        self.inner.is_truncated()
    }

    /// `3 changes from old to new, 1 breaking`.
    #[getter]
    fn summary(&self) -> String {
        self.inner.summary()
    }

    fn __len__(&self) -> usize {
        self.inner.changes().len()
    }

    fn __repr__(&self) -> String {
        format!("<SchemaDiff {}>", self.inner.summary())
    }
}

/// A series of schema versions on a timeline
/// (`rich_ext::schema::SchemaTimeline`): a row per field from the version
/// it appeared in, each version marked with what changed, and the changes
/// listed under it (`details`). `versions` are `(label, schema)` or
/// `(label, schema, at)` pairs, `at` placing the version on the scale.
#[pyclass(name = "SchemaTimeline", module = "rs_rich.data")]
pub(crate) struct SchemaTimeline {
    versions: Vec<(String, Schema, Option<f64>)>,
    details: bool,
}

impl SchemaTimeline {
    fn core(&self) -> CoreTimeline {
        let mut timeline = CoreTimeline::new().details(self.details);
        for (label, schema, at) in &self.versions {
            timeline = match &schema.json {
                Some(json) => timeline.push_json(label.clone(), json.clone()),
                None => timeline.push(label.clone(), schema.model.clone()),
            };
            if let Some(at) = at {
                timeline = timeline.at(*at);
            }
        }
        timeline
    }
}

impl AsRenderable for SchemaTimeline {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.core()))
    }
}

#[pymethods]
impl SchemaTimeline {
    #[new]
    #[pyo3(signature = (versions=None, *, details=true))]
    fn new(versions: Option<&Bound<'_, PyAny>>, details: bool) -> PyResult<Self> {
        let mut timeline = SchemaTimeline {
            versions: Vec::new(),
            details,
        };
        if let Some(versions) = versions {
            for version in versions.try_iter()? {
                let version = version?;
                let (label, schema, at) = match version.extract::<(String, Bound<'_, PyAny>)>() {
                    Ok((label, schema)) => (label, schema, None),
                    Err(_) => version.extract::<(String, Bound<'_, PyAny>, Option<f64>)>()?,
                };
                timeline.versions.push((label, schema_arg(&schema)?, at));
            }
        }
        Ok(timeline)
    }

    /// Add the next version. Returns the timeline.
    #[pyo3(signature = (label, schema, *, at=None))]
    fn push<'py>(
        mut slf: PyRefMut<'py, Self>,
        label: String,
        schema: &Bound<'py, PyAny>,
        at: Option<f64>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let schema = schema_arg(schema)?;
        slf.versions.push((label, schema, at));
        Ok(slf)
    }

    #[getter]
    fn labels(&self) -> Vec<String> {
        self.versions.iter().map(|(l, _, _)| l.clone()).collect()
    }

    /// What changed into each version after the first: `(label, SchemaDiff)`.
    fn changes(&self) -> Vec<(String, SchemaDiff)> {
        self.core()
            .changes()
            .into_iter()
            .map(|(label, inner)| (label.to_string(), SchemaDiff { inner }))
            .collect()
    }

    fn __len__(&self) -> usize {
        self.versions.len()
    }

    fn __repr__(&self) -> String {
        format!("<SchemaTimeline versions={}>", self.versions.len())
    }
}

// ---------------------------------------------------------------------------
// ER diagrams

/// An entity-relationship diagram of a schema (`rich_data::er`): a box per
/// table with its columns, types and keys (`PK`, `FK`, `UK`, `?` for
/// nullable), and an edge per foreign key with its cardinality.
/// `direction` is `"LR"` (the default), `"RL"`, `"TD"` or `"BT"`.
#[pyclass(name = "ErDiagram", module = "rs_rich.data", frozen)]
pub(crate) struct ErDiagram {
    inner: CoreErDiagram,
    notes: Vec<String>,
}

impl AsRenderable for ErDiagram {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

fn er_diagram(
    schema: &CoreSchema,
    notes: Vec<String>,
    direction: &str,
    ascii: Option<bool>,
) -> PyResult<ErDiagram> {
    let direction: Direction = crate::diagram::direction(direction)?;
    let mut inner = CoreErDiagram::new(er::model(schema)).direction(direction);
    if let Some(ascii) = ascii {
        inner = inner.ascii(ascii);
    }
    Ok(ErDiagram { inner, notes })
}

#[pymethods]
impl ErDiagram {
    /// `schema` is a `Schema` (or a JSON Schema): its tables, or the schema
    /// itself as one entity.
    #[new]
    #[pyo3(signature = (schema, *, direction="LR", ascii=None))]
    fn new(schema: &Bound<'_, PyAny>, direction: &str, ascii: Option<bool>) -> PyResult<Self> {
        let schema = schema_arg(schema)?;
        er_diagram(&schema.model, schema.notes.clone(), direction, ascii)
    }

    /// Read SQL DDL and draw it; raises `SchemaError` as `Schema.from_sql`.
    #[staticmethod]
    #[pyo3(signature = (ddl, *, direction="LR", ascii=None))]
    fn from_sql(ddl: &str, direction: &str, ascii: Option<bool>) -> PyResult<Self> {
        let schema = Schema::from_sql(ddl)?;
        er_diagram(&schema.model, schema.notes, direction, ascii)
    }

    /// The entity names, in order.
    #[getter]
    fn entities(&self) -> Vec<String> {
        self.inner
            .model()
            .entities
            .iter()
            .map(|e| e.name.clone())
            .collect()
    }

    /// `(from, to, cardinality)`: each foreign key from the referring table
    /// to the one it names, `cardinality` `"N:1"`, `"1:1"` or `None`.
    #[getter]
    fn relationships(&self) -> Vec<(String, String, Option<&'static str>)> {
        self.inner
            .model()
            .relationships
            .iter()
            .map(|r| {
                (
                    r.from.clone(),
                    r.to.clone(),
                    r.cardinality.map(|c| c.notation()),
                )
            })
            .collect()
    }

    /// What the DDL reader skipped.
    #[getter]
    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn __repr__(&self) -> String {
        let model = self.inner.model();
        format!(
            "<ErDiagram entities={} relationships={}>",
            model.entities.len(),
            model.relationships.len()
        )
    }
}

// ---------------------------------------------------------------------------
// Merge conflicts

names!(layout, layout_name, ConflictLayout, "layout", {
    "auto" => ConflictLayout::Auto,
    "side_by_side" => ConflictLayout::SideBySide,
    "stacked" => ConflictLayout::Stacked,
});

/// One conflict of a `ConflictView`: its `number`, the 1-based lines it
/// spans, and each side's text and marker label (`base` only with diff3
/// markers).
#[pyclass(name = "MergeConflict", module = "rs_rich.data", frozen)]
pub(crate) struct MergeConflict {
    #[pyo3(get)]
    number: usize,
    #[pyo3(get)]
    start_line: usize,
    #[pyo3(get)]
    end_line: usize,
    #[pyo3(get)]
    ours: String,
    #[pyo3(get)]
    ours_label: Option<String>,
    #[pyo3(get)]
    base: Option<String>,
    #[pyo3(get)]
    base_label: Option<String>,
    #[pyo3(get)]
    theirs: String,
    #[pyo3(get)]
    theirs_label: Option<String>,
}

#[pymethods]
impl MergeConflict {
    fn __repr__(&self) -> String {
        format!(
            "MergeConflict(number={}, lines={}-{}, ours={}, theirs={})",
            self.number,
            self.start_line,
            self.end_line,
            repr_opt(self.ours_label.as_deref()),
            repr_opt(self.theirs_label.as_deref())
        )
    }
}

/// A file's merge conflicts (`rich_ext::diff::ConflictView`): each
/// conflict numbered, ours, base (with diff3 markers) and theirs side by
/// side or stacked (`layout`: `"auto"`, `"side_by_side"`, `"stacked"`),
/// syntax highlighted by `language` or `path`, between `context` lines.
/// Raises `ConflictError` (with `line`) for markers out of order or a
/// conflict never closed.
#[pyclass(name = "ConflictView", module = "rs_rich.data", frozen)]
pub(crate) struct ConflictView {
    inner: CoreConflictView,
}

impl AsRenderable for ConflictView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl ConflictView {
    #[new]
    #[pyo3(signature = (
        text, *, path=None, language=None, layout="auto", context=3, line_numbers=true,
        wrap=true, base=true
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        text: &str,
        path: Option<String>,
        language: Option<String>,
        layout: &str,
        context: usize,
        line_numbers: bool,
        wrap: bool,
        base: bool,
    ) -> PyResult<Self> {
        let layout = self::layout(layout)?;
        let mut inner = CoreConflictView::parse(text)
            .map_err(|e| conflict_error(&e))?
            .layout(layout)
            .context(context)
            .line_numbers(line_numbers)
            .wrap(wrap)
            .base(base);
        if let Some(path) = path {
            inner = inner.path(path);
        }
        if let Some(language) = language {
            inner = inner.language(language);
        }
        Ok(ConflictView { inner })
    }

    #[getter]
    fn conflicts(&self) -> Vec<MergeConflict> {
        let file = self.inner.file();
        file.conflicts()
            .iter()
            .map(|c| MergeConflict {
                number: c.number,
                start_line: c.start_line(),
                end_line: c.end_line,
                ours: file.text(&c.ours),
                ours_label: c.ours.label.clone(),
                base: c.base.as_ref().map(|b| file.text(b)),
                base_label: c.base.as_ref().and_then(|b| b.label.clone()),
                theirs: file.text(&c.theirs),
                theirs_label: c.theirs.label.clone(),
            })
            .collect()
    }

    #[getter]
    fn has_conflicts(&self) -> bool {
        self.inner.file().has_conflicts()
    }

    fn __len__(&self) -> usize {
        self.inner.file().conflicts().len()
    }

    fn __repr__(&self) -> String {
        format!(
            "<ConflictView conflicts={}>",
            self.inner.file().conflicts().len()
        )
    }
}

// ---------------------------------------------------------------------------
// The record inspector

fn path(value: &str) -> PyResult<Path> {
    Path::from_str(value).map_err(|e| PyValueError::new_err(format!("invalid path {value:?}: {e}")))
}

/// One record as a table of fields (`rich_ext::data::RecordView`): `field |
/// type | value`, nested values as trees opened `depth` levels deep.
/// `expand` and `collapse` open or fold branches by path (`owner.oncall`,
/// `ports[2]`); `max_items` and `max_string` cut long containers and
/// strings (`None` keeps them whole). `record` is a Python value (`dict`,
/// dataclass, …) or an `rs_rich.ext.data` `DataNode` or `Document`.
#[pyclass(name = "RecordView", module = "rs_rich.data", frozen)]
pub(crate) struct RecordView {
    inner: CoreRecordView<'static>,
}

impl AsRenderable for RecordView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl RecordView {
    #[new]
    #[pyo3(signature = (
        record, *, depth=1, expand=None, collapse=None, max_items=Some(20),
        max_string=Some(200), show_types=true, title=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        record: &Bound<'_, PyAny>,
        depth: usize,
        expand: Option<Vec<String>>,
        collapse: Option<Vec<String>>,
        max_items: Option<usize>,
        max_string: Option<usize>,
        show_types: bool,
        title: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreRecordView::new(Cow::Owned(node_arg(record)?))
            .depth(depth)
            .max_items(max_items)
            .max_string(max_string)
            .show_types(show_types);
        for branch in expand.unwrap_or_default() {
            inner = inner.expand(path(&branch)?);
        }
        for branch in collapse.unwrap_or_default() {
            inner = inner.collapse(path(&branch)?);
        }
        if let Some(title) = title {
            inner = inner.title(title);
        }
        Ok(RecordView { inner })
    }

    /// The record's fields that hold a non-empty container: the branches
    /// there are to open.
    #[getter]
    fn branches(&self) -> Vec<String> {
        self.inner
            .branches()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// Whether the branch at `path` is open.
    fn is_open(&self, path: &str) -> PyResult<bool> {
        Ok(self.inner.is_open(&self::path(path)?))
    }

    fn __repr__(&self) -> String {
        format!("<RecordView branches={}>", self.inner.branches().len())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("SchemaError", py.get_type::<SchemaError>())?;
    m.add("ConflictError", py.get_type::<ConflictError>())?;
    renderable::add_renderable_class::<Schema>(m)?;
    m.add_class::<SchemaField>()?;
    renderable::add_renderable_class::<SchemaTree>(m)?;
    m.add_class::<SchemaChange>()?;
    renderable::add_renderable_class::<SchemaDiff>(m)?;
    renderable::add_renderable_class::<SchemaTimeline>(m)?;
    renderable::add_renderable_class::<ErDiagram>(m)?;
    m.add_class::<MergeConflict>()?;
    renderable::add_renderable_class::<ConflictView>(m)?;
    renderable::add_renderable_class::<RecordView>(m)?;
    Ok(())
}
