//! `rs_rich.ext.data`: one document tree for JSON, YAML, TOML, XML, INI and
//! dotenv, with the explorer, table, flat, search, diff, redaction and
//! config views, JSONPath selection and the document transforms.
//!
//! Paths are the display form `servers[0].name` (a `str`); a Python value
//! converts to a document with [`DataNode::from_python`].

use std::borrow::Cow;
use std::str::FromStr;

use pyo3::exceptions::{PyIndexError, PyKeyError, PyRecursionError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple, PyType};

use rich::protocol::Renderable;
use rich::Justify;
use rich_ext::data::transform::{
    Document as CoreDocument, Filter as CoreFilter, Highlight as CoreHighlight,
    Redact as CoreRedact, Select as CoreSelect,
};
use rich_ext::data::{
    self as core, ChangeKind, ConfigFileView as CoreConfigView, DataError as CoreDataError,
    DiffView as CoreDiffView, Explorer as CoreExplorer, FlatView as CoreFlatView, Format,
    MatchKind, Node, Path, Redaction as CoreRedaction, SearchQuery as CoreQuery,
    SearchResults as CoreResults, Selectors, TableView as CoreTableView, Value, View,
};
use rich_ext::transform::Transform;

use super::common::{self, names, DataError, SelectError, TransformError};
use super::diagnostic::Diagnostic;
use crate::renderable::{self, AsRenderable, Nesting};

names!(format, format_name, Format, "format", {
    "json" => Format::Json,
    "yaml" => Format::Yaml,
    "toml" => Format::Toml,
    "xml" => Format::Xml,
    "ini" => Format::Ini,
    "dotenv" => Format::Dotenv,
});

names!(view, view_name, View, "view", {
    "tree" => View::Tree,
    "table" => View::Table,
});

fn path(value: &str) -> PyResult<Path> {
    Path::from_str(value).map_err(|e| PyValueError::new_err(format!("invalid path {value:?}: {e}")))
}

/// A Python `DataError` for a parse failure, with `format`, `line`,
/// `column` and, when the source is known, a `diagnostic` underlining it.
fn data_error(py: Python<'_>, error: &CoreDataError, source: Option<(&str, &str)>) -> PyErr {
    let err = DataError::new_err(error.to_string());
    let value = err.value(py);
    let _ = value.setattr("format", format_name(error.format));
    let _ = value.setattr("reason", error.message.clone());
    let _ = value.setattr("line", error.position.map(|p| p.line));
    let _ = value.setattr("column", error.position.map(|p| p.column));
    if let Some((content, name)) = source {
        let diagnostic = Diagnostic::from_core(
            error.to_diagnostic(content, name),
            rich_ext::event::EventView::Expanded,
        );
        if let Ok(diagnostic) = Py::new(py, diagnostic) {
            let _ = value.setattr("diagnostic", diagnostic);
        }
    } else {
        let _ = value.setattr("diagnostic", py.None());
    }
    err
}

// ---------------------------------------------------------------------------
// Nodes

/// A node of a parsed document: a value (scalar, sequence or map) and where
/// it came from (position, YAML anchor or alias, comment, XML kind).
#[pyclass(
    name = "DataNode",
    module = "rs_rich.ext.data",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct DataNode {
    pub(crate) inner: Node,
}

pub(crate) fn node_arg(value: &Bound<'_, PyAny>) -> PyResult<Node> {
    if let Ok(node) = value.extract::<PyRef<'_, DataNode>>() {
        return Ok(node.inner.clone());
    }
    if let Ok(document) = value.extract::<PyRef<'_, Document>>() {
        return Ok(document.inner.node.clone());
    }
    python_node(value)
}

/// The deepest a document may nest: the limit the parsers enforce
/// (`rich_ext::data`'s `MAX_DEPTH`).
const MAX_DOCUMENT_DEPTH: usize = 512;

/// How many levels `node` nests (a scalar is one), counted without
/// recursion.
fn node_depth(node: &Node) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(node, 1usize)];
    while let Some((node, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        match &node.value {
            Value::Seq(items) => stack.extend(items.iter().map(|n| (n, depth + 1))),
            Value::Map(entries) => stack.extend(entries.iter().map(|(_, n)| (n, depth + 1))),
            _ => {}
        }
    }
    deepest
}

/// A Python value as a document node: `None`, `bool`, `int`, `float`, `str`,
/// `dict` (keys as `str`), and `list`/`tuple` (or any other iterable but a
/// string); dates and anything else become their `str`.
fn python_node(value: &Bound<'_, PyAny>) -> PyResult<Node> {
    let _nesting = Nesting::enter()?;
    if let Ok(node) = value.extract::<PyRef<'_, DataNode>>() {
        // An embedded node brings its own depth: wrapping one again and
        // again would otherwise nest without bound (and overflow the stack
        // cloning or dropping it).
        let depth = node_depth(&node.inner) + renderable::nesting_depth();
        if depth > MAX_DOCUMENT_DEPTH {
            return Err(PyRecursionError::new_err(format!(
                "maximum recursion depth exceeded: a document nests at most \
                 {MAX_DOCUMENT_DEPTH} levels"
            )));
        }
        return Ok(node.inner.clone());
    }
    let node = if value.is_none() {
        Value::Null
    } else if let Ok(flag) = value.cast::<PyBool>() {
        Value::Bool(flag.is_true())
    } else if value.is_instance_of::<PyInt>() {
        if let Ok(v) = value.extract::<i64>() {
            Value::Int(v)
        } else if let Ok(v) = value.extract::<u64>() {
            Value::UInt(v)
        } else {
            Value::String(value.str()?.to_string())
        }
    } else if let Ok(v) = value.cast::<PyFloat>() {
        Value::Float(v.value())
    } else if let Ok(v) = value.cast::<PyString>() {
        Value::String(v.to_cow()?.into_owned())
    } else if let Ok(map) = value.cast::<PyDict>() {
        let mut entries = Vec::with_capacity(map.len());
        for (key, item) in map.iter() {
            let key = match key.cast::<PyString>() {
                Ok(key) => key.to_cow()?.into_owned(),
                Err(_) => key.str()?.to_string(),
            };
            entries.push((key, python_node(&item)?));
        }
        Value::Map(entries)
    } else if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
        let items: PyResult<Vec<_>> = value.try_iter()?.map(|item| python_node(&item?)).collect();
        Value::Seq(items?)
    } else if value.hasattr("isoformat")? {
        Value::DateTime(value.call_method0("isoformat")?.extract()?)
    } else if let Some(fields) = dataclass_fields(value)? {
        fields
    } else if let Ok(iter) = value.try_iter() {
        let items: PyResult<Vec<_>> = iter.map(|item| python_node(&item?)).collect();
        Value::Seq(items?)
    } else {
        Value::String(value.str()?.to_string())
    };
    Ok(Node::new(node))
}

/// A dataclass instance as a map of its fields.
fn dataclass_fields(value: &Bound<'_, PyAny>) -> PyResult<Option<Value>> {
    let py = value.py();
    let dataclasses = py.import("dataclasses")?;
    if value.is_instance_of::<PyType>()
        || !dataclasses
            .call_method1("is_dataclass", (value,))?
            .is_truthy()?
    {
        return Ok(None);
    }
    let mut entries = Vec::new();
    for field in dataclasses.call_method1("fields", (value,))?.try_iter()? {
        let name: String = field?.getattr("name")?.extract()?;
        let item = value.getattr(name.as_str())?;
        entries.push((name, python_node(&item)?));
    }
    Ok(Some(Value::Map(entries)))
}

fn node_to_python(py: Python<'_>, node: &Node) -> PyResult<Py<PyAny>> {
    let _nesting = Nesting::enter()?;
    Ok(match &node.value {
        Value::Null => py.None(),
        Value::Bool(v) => PyBool::new(py, *v).to_owned().into_any().unbind(),
        Value::Int(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::UInt(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::Float(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::String(v) | Value::DateTime(v) => v.into_pyobject(py)?.into_any().unbind(),
        Value::Seq(items) => {
            let items: PyResult<Vec<_>> = items.iter().map(|n| node_to_python(py, n)).collect();
            PyList::new(py, items?)?.into_any().unbind()
        }
        Value::Map(entries) => {
            let dict = PyDict::new(py);
            for (key, item) in entries {
                dict.set_item(key, node_to_python(py, item)?)?;
            }
            dict.into_any().unbind()
        }
    })
}

fn wrap(node: Node) -> DataNode {
    DataNode { inner: node }
}

impl AsRenderable for DataNode {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(CoreExplorer::new(self.inner.clone())))
    }
}

#[pymethods]
impl DataNode {
    /// A document from a Python value (`dict`, `list`, scalars, dataclasses).
    #[staticmethod]
    fn from_python(value: &Bound<'_, PyAny>) -> PyResult<DataNode> {
        Ok(wrap(python_node(value)?))
    }

    /// The value as plain Python (`dict`, `list`, scalars; dates as `str`).
    fn to_python(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        node_to_python(py, &self.inner)
    }

    /// The value as JSON text (non-finite floats become `null`); `indent`
    /// pretty-prints with that many spaces per level.
    #[pyo3(signature = (*, indent=None))]
    fn to_json(&self, indent: Option<usize>) -> String {
        let json = self.inner.to_json();
        match indent {
            None => json.to_string(),
            Some(width) => format!("{json:#}")
                .lines()
                .map(|line| {
                    let body = line.trim_start_matches(' ');
                    let depth = (line.len() - body.len()) / 2;
                    format!("{}{body}", " ".repeat(depth * width))
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// `null`, `bool`, `int`, `float`, `str`, `datetime`, `seq` or `map`.
    #[getter]
    fn type_name(&self) -> &'static str {
        self.inner.type_name()
    }

    #[getter]
    fn is_container(&self) -> bool {
        self.inner.is_container()
    }

    /// The scalar value (`None` for a container; use `to_python`).
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if self.inner.is_container() {
            return Ok(py.None());
        }
        node_to_python(py, &self.inner)
    }

    /// `(line, column)` in the source, 1-based, when known.
    #[getter]
    fn position(&self) -> Option<(usize, usize)> {
        self.inner.meta.position.map(|p| (p.line, p.column))
    }

    /// The YAML anchor defined on this node.
    #[getter]
    fn anchor(&self) -> Option<String> {
        self.inner.meta.anchor.clone()
    }

    /// For a YAML alias, the anchor it refers to.
    #[getter]
    fn alias(&self) -> Option<String> {
        self.inner.meta.alias.clone()
    }

    /// The comment attached to this entry (INI and dotenv).
    #[getter]
    fn comment(&self) -> Option<String> {
        self.inner.meta.comment.clone()
    }

    /// For XML: `element`, `attribute` or `text`.
    #[getter]
    fn xml_kind(&self) -> Option<&'static str> {
        self.inner.meta.xml.map(|kind| match kind {
            core::XmlKind::Element => "element",
            core::XmlKind::Attribute => "attribute",
            core::XmlKind::Text => "text",
        })
    }

    /// The keys of a map (empty otherwise).
    fn keys(&self) -> Vec<String> {
        match &self.inner.value {
            Value::Map(entries) => entries.iter().map(|(k, _)| k.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// The value of `key` in a map, or `default`.
    #[pyo3(signature = (key, default=None))]
    fn get(&self, key: &str, default: Option<Py<PyAny>>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.inner.get(key) {
            Some(node) => Ok(Py::new(py, wrap(node.clone()))?.into_any()),
            None => Ok(default.unwrap_or_else(|| py.None())),
        }
    }

    /// The node at a path (`servers[0].name`), or `None`.
    fn at(&self, path: &str) -> PyResult<Option<DataNode>> {
        Ok(self.inner.at(&self::path(path)?).cloned().map(wrap))
    }

    /// Every node with its path, parents before children.
    fn walk(&self) -> Vec<(String, DataNode)> {
        let mut out = Vec::new();
        self.inner
            .walk(|path, node| out.push((path.to_string(), wrap(node.clone()))));
        out
    }

    /// A copy with secrets masked by a `Redaction`.
    fn redacted(&self, redaction: PyRef<'_, Redaction>) -> DataNode {
        wrap(self.inner.redacted(&redaction.inner))
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn __getitem__(&self, key: &Bound<'_, PyAny>) -> PyResult<DataNode> {
        if let Ok(name) = key.extract::<String>() {
            return self
                .inner
                .get(&name)
                .cloned()
                .map(wrap)
                .ok_or_else(|| PyKeyError::new_err(name));
        }
        let index: isize = key.extract()?;
        let len = self.inner.len() as isize;
        let index = if index < 0 { index + len } else { index };
        usize::try_from(index)
            .ok()
            .and_then(|i| self.inner.index(i))
            .cloned()
            .map(wrap)
            .ok_or_else(|| PyIndexError::new_err("data node index out of range"))
    }

    fn __contains__(&self, key: &str) -> bool {
        self.inner.get(key).is_some()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, DataNode>>()
            .is_ok_and(|o| o.inner.to_json() == self.inner.to_json())
    }

    fn __repr__(&self) -> String {
        let json = self.inner.to_json().to_string();
        let json = if json.chars().count() > 60 {
            format!("{}…", json.chars().take(60).collect::<String>())
        } else {
            json
        };
        format!("<DataNode {} {json}>", self.inner.type_name())
    }
}

/// `parse(content, format=None, *, name=None)`: parse JSON, YAML, TOML,
/// XML, INI or dotenv into a `DataNode`. Without `format` it is detected
/// from `name` (a file name) and the content.
#[pyfunction]
#[pyo3(signature = (content, format=None, *, name=None))]
fn parse_data(
    py: Python<'_>,
    content: &str,
    format: Option<&str>,
    name: Option<&str>,
) -> PyResult<DataNode> {
    let format = match format {
        Some(format) => self::format(format)?,
        None => Format::detect(content, name)
            .ok_or_else(|| DataError::new_err("cannot detect the format; pass format="))?,
    };
    core::parse(format, content)
        .map(wrap)
        .map_err(|e| data_error(py, &e, Some((content, name.unwrap_or("<input>")))))
}

/// `detect_format(content, name=None)`: the format of `content` (a file name
/// helps), or `None`.
#[pyfunction]
#[pyo3(signature = (content, name=None))]
fn detect_format(content: &str, name: Option<&str>) -> Option<&'static str> {
    Format::detect(content, name).map(format_name)
}

/// `format_for(file_name)`: the format a file name implies, or `None`.
#[pyfunction]
fn format_for(file_name: &str) -> Option<&'static str> {
    Format::from_file_name(file_name).map(format_name)
}

// ---------------------------------------------------------------------------
// Views

fn highlights(value: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<(Path, rich::Style)>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    common::pairs(value)?
        .into_iter()
        .map(|(p, style)| Ok((path(&p)?, common::required_style(&style)?)))
        .collect()
}

/// `Explorer(data, *, max_depth=None, ...)`: a width-aware tree (or table)
/// of a document, folding deep containers and cutting long strings.
#[pyclass(name = "Explorer", module = "rs_rich.ext.data")]
pub(crate) struct Explorer {
    node: Node,
    #[pyo3(get, set)]
    max_depth: Option<usize>,
    #[pyo3(get, set)]
    max_length: Option<usize>,
    #[pyo3(get, set)]
    max_string: Option<usize>,
    #[pyo3(get, set)]
    show_paths: bool,
    #[pyo3(get, set)]
    show_types: bool,
    folded: Vec<Path>,
    highlighted: Vec<(Path, rich::Style)>,
    #[pyo3(get, set)]
    root_label: Option<String>,
    view: View,
}

impl Explorer {
    fn build(&self) -> CoreExplorer<'static> {
        let mut explorer = CoreExplorer::new(self.node.clone())
            .show_paths(self.show_paths)
            .show_types(self.show_types)
            .view(self.view);
        if let Some(depth) = self.max_depth {
            explorer = explorer.max_depth(depth);
        }
        if let Some(length) = self.max_length {
            explorer = explorer.max_length(length);
        }
        if let Some(length) = self.max_string {
            explorer = explorer.max_string(length);
        }
        for path in &self.folded {
            explorer = explorer.fold(path.clone());
        }
        for (path, style) in &self.highlighted {
            explorer = explorer.highlight(path.clone(), style.clone());
        }
        if let Some(label) = &self.root_label {
            explorer = explorer.root_label(label.clone());
        }
        explorer
    }
}

impl AsRenderable for Explorer {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.build()))
    }
}

#[pymethods]
impl Explorer {
    #[new]
    #[pyo3(signature = (
        data, *, max_depth=None, max_length=None, max_string=None, show_paths=false,
        show_types=false, fold=None, highlight=None, root_label=None, view="tree"
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        data: &Bound<'_, PyAny>,
        max_depth: Option<usize>,
        max_length: Option<usize>,
        max_string: Option<usize>,
        show_paths: bool,
        show_types: bool,
        fold: Option<&Bound<'_, PyAny>>,
        highlight: Option<&Bound<'_, PyAny>>,
        root_label: Option<String>,
        view: &str,
    ) -> PyResult<Self> {
        let folded = match fold {
            Some(paths) => common::strings(paths)?
                .iter()
                .map(|p| path(p))
                .collect::<PyResult<_>>()?,
            None => Vec::new(),
        };
        Ok(Explorer {
            node: node_arg(data)?,
            max_depth,
            max_length,
            max_string,
            show_paths,
            show_types,
            folded,
            highlighted: highlights(highlight)?,
            root_label,
            view: self::view(view)?,
        })
    }

    #[getter]
    fn node(&self) -> DataNode {
        wrap(self.node.clone())
    }

    #[getter]
    fn get_view(&self) -> &'static str {
        view_name(self.view)
    }

    #[setter]
    fn set_view(&mut self, value: &str) -> PyResult<()> {
        self.view = view(value)?;
        Ok(())
    }

    /// Fold the container at `path`. Returns the explorer.
    fn fold<'py>(mut slf: PyRefMut<'py, Self>, at: &str) -> PyResult<PyRefMut<'py, Self>> {
        let at = path(at)?;
        slf.folded.push(at);
        Ok(slf)
    }

    /// Style the tree line at `path`. Returns the explorer.
    fn highlight<'py>(
        mut slf: PyRefMut<'py, Self>,
        at: &str,
        style: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let entry = (path(at)?, common::required_style(style)?);
        slf.highlighted.push(entry);
        Ok(slf)
    }
}

fn justify(name: &str) -> PyResult<Justify> {
    crate::convert::justify(Some(name))
}

/// `TableView(data, *, columns=None, headers=None, justify=None, title=None,
/// max_rows=None, max_string=None)`: records (a list of maps) as a table.
#[pyclass(name = "TableView", module = "rs_rich.ext.data")]
pub(crate) struct TableView {
    node: Node,
    columns: Option<Vec<String>>,
    headers: Vec<(String, String)>,
    justify: Vec<(String, Justify)>,
    title: Option<String>,
    max_rows: Option<usize>,
    max_string: Option<usize>,
}

impl AsRenderable for TableView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let mut view = CoreTableView::new(self.node.clone());
        if let Some(columns) = &self.columns {
            view = view.columns(columns.clone());
        }
        for (column, header) in &self.headers {
            view = view.header(column.clone(), header.clone());
        }
        for (column, justify) in &self.justify {
            view = view.justify(column.clone(), *justify);
        }
        if let Some(title) = &self.title {
            view = view.title(title.clone());
        }
        if let Some(rows) = self.max_rows {
            view = view.max_rows(rows);
        }
        if let Some(length) = self.max_string {
            view = view.max_string(length);
        }
        Ok(Box::new(view))
    }
}

#[pymethods]
impl TableView {
    #[new]
    #[pyo3(signature = (data, *, columns=None, headers=None, justify=None, title=None, max_rows=None, max_string=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        data: &Bound<'_, PyAny>,
        columns: Option<&Bound<'_, PyAny>>,
        headers: Option<&Bound<'_, PyAny>>,
        justify: Option<&Bound<'_, PyAny>>,
        title: Option<String>,
        max_rows: Option<usize>,
        max_string: Option<usize>,
    ) -> PyResult<Self> {
        let headers = match headers {
            Some(value) => common::pairs(value)?
                .into_iter()
                .map(|(k, v)| Ok((k, v.extract::<String>()?)))
                .collect::<PyResult<_>>()?,
            None => Vec::new(),
        };
        let justify = match justify {
            Some(value) => common::pairs(value)?
                .into_iter()
                .map(|(k, v)| Ok((k, self::justify(&v.extract::<String>()?)?)))
                .collect::<PyResult<_>>()?,
            None => Vec::new(),
        };
        Ok(TableView {
            node: node_arg(data)?,
            columns: columns.map(common::strings).transpose()?,
            headers,
            justify,
            title,
            max_rows,
            max_string,
        })
    }
}

/// `FlatView(data, *, show_types=False, max_string=None)`: leaves as a
/// `path | value` table.
#[pyclass(name = "FlatView", module = "rs_rich.ext.data", frozen)]
pub(crate) struct FlatView {
    node: Node,
    show_types: bool,
    max_string: Option<usize>,
}

impl AsRenderable for FlatView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let mut view = CoreFlatView::new(self.node.clone()).show_types(self.show_types);
        if let Some(length) = self.max_string {
            view = view.max_string(length);
        }
        Ok(Box::new(view))
    }
}

#[pymethods]
impl FlatView {
    #[new]
    #[pyo3(signature = (data, *, show_types=false, max_string=None))]
    fn new(data: &Bound<'_, PyAny>, show_types: bool, max_string: Option<usize>) -> PyResult<Self> {
        Ok(FlatView {
            node: node_arg(data)?,
            show_types,
            max_string,
        })
    }
}

/// `flatten(data)`: the leaves as `(path, DataNode)` pairs.
#[pyfunction]
fn flatten(data: &Bound<'_, PyAny>) -> PyResult<Vec<(String, DataNode)>> {
    Ok(core::flatten(&node_arg(data)?)
        .into_iter()
        .map(|(path, node)| (path.to_string(), wrap(node)))
        .collect())
}

/// `unflatten(leaves)`: rebuild a document from `(path, value)` pairs.
#[pyfunction]
fn unflatten(leaves: &Bound<'_, PyAny>) -> PyResult<DataNode> {
    let mut pairs = Vec::new();
    for item in leaves.try_iter()? {
        let (p, value): (String, Bound<'_, PyAny>) = item?.extract()?;
        pairs.push((path(&p)?, node_arg(&value)?));
    }
    core::unflatten(pairs)
        .map(wrap)
        .map_err(|e| PyValueError::new_err(format!("{e:?}")))
}

// ---------------------------------------------------------------------------
// Search and selection

/// `SearchQuery(*, key=None, path=None, value=None, text=None,
/// case_insensitive=False)`: criteria that must all match.
#[pyclass(
    name = "SearchQuery",
    module = "rs_rich.ext.data",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct SearchQuery {
    inner: CoreQuery,
}

#[pymethods]
impl SearchQuery {
    #[new]
    #[pyo3(signature = (*, key=None, path=None, value=None, text=None, case_insensitive=false))]
    fn new(
        key: Option<String>,
        path: Option<String>,
        value: Option<String>,
        text: Option<String>,
        case_insensitive: bool,
    ) -> PyResult<Self> {
        let mut query = CoreQuery::default();
        if key.is_none() && path.is_none() && value.is_none() && text.is_none() {
            return Err(PyValueError::new_err(
                "a SearchQuery needs key=, path=, value= or text=",
            ));
        }
        if let Some(pattern) = key {
            query = query.and_key(pattern);
        }
        if let Some(pattern) = path {
            query = query.and_path(pattern);
        }
        if let Some(pattern) = value {
            query = query.and_value(pattern);
        }
        if let Some(pattern) = text {
            query = query.and_text(pattern);
        }
        Ok(SearchQuery {
            inner: query.case_insensitive(case_insensitive),
        })
    }
}

fn query_arg(
    query: Option<&Bound<'_, PyAny>>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<CoreQuery> {
    if let Some(query) = query {
        if let Ok(query) = query.extract::<PyRef<'_, SearchQuery>>() {
            return Ok(query.inner.clone());
        }
        if let Ok(text) = query.extract::<String>() {
            return Ok(CoreQuery::text(text));
        }
        return Err(PyTypeError::new_err("query must be a SearchQuery or a str"));
    }
    let get = |name: &str| -> PyResult<Option<String>> {
        match kwargs {
            Some(kwargs) => kwargs.get_item(name)?.map(|v| v.extract()).transpose(),
            None => Ok(None),
        }
    };
    let case = match kwargs {
        Some(kwargs) => kwargs
            .get_item("case_insensitive")?
            .map(|v| v.extract())
            .transpose()?
            .unwrap_or(false),
        None => false,
    };
    Ok(SearchQuery::new(get("key")?, get("path")?, get("value")?, get("text")?, case)?.inner)
}

/// One search hit.
#[pyclass(name = "SearchMatch", module = "rs_rich.ext.data", frozen)]
pub(crate) struct SearchMatch {
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    node: DataNode,
    #[pyo3(get)]
    matched_on: &'static str,
}

#[pymethods]
impl SearchMatch {
    fn __repr__(&self) -> String {
        format!(
            "SearchMatch(path={:?}, matched_on={:?})",
            self.path, self.matched_on
        )
    }
}

/// `search(data, query=None, *, key=, path=, value=, text=,
/// case_insensitive=)`: every node that matches, in document order.
#[pyfunction]
#[pyo3(signature = (data, query=None, **kwargs))]
fn search(
    data: &Bound<'_, PyAny>,
    query: Option<&Bound<'_, PyAny>>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<SearchMatch>> {
    let node = node_arg(data)?;
    let query = query_arg(query, kwargs)?;
    Ok(core::search(&node, &query)
        .into_iter()
        .map(|hit| SearchMatch {
            path: hit.path.to_string(),
            node: wrap(hit.node.clone()),
            matched_on: match hit.matched_on {
                MatchKind::Key => "key",
                MatchKind::Path => "path",
                MatchKind::Value => "value",
            },
        })
        .collect())
}

/// `SearchResults(data, query=None, *, context=0, max_string=80, key=, ...)`:
/// the hits with their paths and matches highlighted.
#[pyclass(name = "SearchResults", module = "rs_rich.ext.data", frozen)]
pub(crate) struct SearchResults {
    node: Node,
    query: CoreQuery,
    context: usize,
    max_string: usize,
}

struct OwnedResults {
    node: Node,
    query: CoreQuery,
    context: usize,
    max_string: usize,
}

impl OwnedResults {
    fn view(&self) -> CoreResults<'_> {
        CoreResults::new(&self.node, &self.query)
            .context(self.context)
            .max_string(self.max_string)
    }
}

impl Renderable for OwnedResults {
    fn rich_render(
        &self,
        console: &rich::Console,
        options: &rich::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        self.view().rich_render(console, options)
    }

    fn measure(
        &self,
        console: &rich::Console,
        options: &rich::ConsoleOptions,
    ) -> rich::measure::Measurement {
        self.view().measure(console, options)
    }
}

impl AsRenderable for SearchResults {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(OwnedResults {
            node: self.node.clone(),
            query: self.query.clone(),
            context: self.context,
            max_string: self.max_string,
        }))
    }
}

#[pymethods]
impl SearchResults {
    #[new]
    #[pyo3(signature = (data, query=None, *, context=0, max_string=80, **kwargs))]
    fn new(
        data: &Bound<'_, PyAny>,
        query: Option<&Bound<'_, PyAny>>,
        context: usize,
        max_string: usize,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        Ok(SearchResults {
            node: node_arg(data)?,
            query: query_arg(query, kwargs)?,
            context,
            max_string,
        })
    }

    fn __len__(&self) -> usize {
        core::search(&self.node, &self.query).len()
    }
}

fn select_error(error: core::SelectError) -> PyErr {
    let err = SelectError::new_err(error.to_string());
    Python::attach(|py| {
        let _ = err.value(py).setattr("column", error.column);
    });
    err
}

/// `select(data, expression, *, backend="jsonpath")`: the `(path, node)`
/// pairs a JSONPath expression selects.
#[pyfunction]
#[pyo3(signature = (data, expression, *, backend="jsonpath"))]
fn select(
    data: &Bound<'_, PyAny>,
    expression: &str,
    backend: &str,
) -> PyResult<Vec<(String, DataNode)>> {
    let node = node_arg(data)?;
    let selector = Selectors::default()
        .compile(backend, expression)
        .map_err(select_error)?;
    Ok(selector
        .select(&node)
        .map_err(select_error)?
        .into_iter()
        .map(|(path, node)| (path.to_string(), wrap(node.clone())))
        .collect())
}

/// `selector_backends()`: the selection languages available (`["jsonpath"]`).
#[pyfunction]
fn selector_backends() -> Vec<String> {
    Selectors::default()
        .names()
        .into_iter()
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Diff, redaction and config files

/// One leaf-level difference between two documents.
#[pyclass(name = "DataChange", module = "rs_rich.ext.data", frozen)]
pub(crate) struct DataChange {
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    kind: &'static str,
    #[pyo3(get)]
    old: Option<DataNode>,
    #[pyo3(get)]
    new: Option<DataNode>,
}

#[pymethods]
impl DataChange {
    fn __repr__(&self) -> String {
        format!("DataChange(path={:?}, kind={:?})", self.path, self.kind)
    }
}

/// `diff_data(old, new)`: the differences leaf by leaf, in document order.
#[pyfunction]
fn diff_data(old: &Bound<'_, PyAny>, new: &Bound<'_, PyAny>) -> PyResult<Vec<DataChange>> {
    Ok(core::diff(&node_arg(old)?, &node_arg(new)?)
        .into_iter()
        .map(|change| DataChange {
            path: change.path.to_string(),
            kind: match change.kind {
                ChangeKind::Added => "added",
                ChangeKind::Removed => "removed",
                ChangeKind::Changed => "changed",
            },
            old: change.old.map(wrap),
            new: change.new.map(wrap),
        })
        .collect())
}

/// `DataDiffView(old, new)`: the leaf differences as `+`/`-`/`~` lines.
#[pyclass(name = "DataDiffView", module = "rs_rich.ext.data", frozen)]
pub(crate) struct DataDiffView {
    old: Node,
    new: Node,
}

impl AsRenderable for DataDiffView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(CoreDiffView::new(&self.old, &self.new)))
    }
}

#[pymethods]
impl DataDiffView {
    #[new]
    fn new(old: &Bound<'_, PyAny>, new: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(DataDiffView {
            old: node_arg(old)?,
            new: node_arg(new)?,
        })
    }

    fn __len__(&self) -> usize {
        CoreDiffView::new(&self.old, &self.new).changes().len()
    }
}

/// `Redaction(patterns=(), *, secrets=False, mask="********")`: masks
/// string and number leaves whose key matches a pattern (substring, or a
/// glob with `*`/`?`; case-insensitive, `-` and `_` alike).
#[pyclass(
    name = "Redaction",
    module = "rs_rich.ext.data",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Redaction {
    pub(crate) inner: CoreRedaction,
}

#[pymethods]
impl Redaction {
    #[new]
    #[pyo3(signature = (patterns=None, *, secrets=false, mask=None))]
    fn new(
        patterns: Option<&Bound<'_, PyAny>>,
        secrets: bool,
        mask: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = if secrets {
            CoreRedaction::secrets()
        } else {
            CoreRedaction::new()
        };
        if let Some(patterns) = patterns {
            inner = inner.patterns(common::strings(patterns)?);
        }
        if let Some(mask) = mask {
            inner = inner.mask(mask);
        }
        Ok(Redaction { inner })
    }

    /// Common secret key names (`password`, `token`, `api_key`, ...).
    #[staticmethod]
    fn secrets() -> Self {
        Redaction {
            inner: CoreRedaction::secrets(),
        }
    }

    /// Whether `key` would be masked.
    fn matches_key(&self, key: &str) -> bool {
        self.inner.matches_key(key)
    }
}

/// `ConfigFileView(data, *, redaction=None, title=None)`: INI and dotenv
/// files as `section | key | value | comment`.
#[pyclass(name = "ConfigFileView", module = "rs_rich.ext.data", frozen)]
pub(crate) struct ConfigFileView {
    node: Node,
    redaction: Option<CoreRedaction>,
    title: Option<String>,
}

impl AsRenderable for ConfigFileView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let mut view = CoreConfigView::new(Cow::Owned(self.node.clone()));
        if let Some(redaction) = &self.redaction {
            view = view.redactor(redaction.clone());
        }
        if let Some(title) = &self.title {
            view = view.title(title.clone());
        }
        Ok(Box::new(view))
    }
}

#[pymethods]
impl ConfigFileView {
    #[new]
    #[pyo3(signature = (data, *, redaction=None, title=None))]
    fn new(
        data: &Bound<'_, PyAny>,
        redaction: Option<PyRef<'_, Redaction>>,
        title: Option<String>,
    ) -> PyResult<Self> {
        Ok(ConfigFileView {
            node: node_arg(data)?,
            redaction: redaction.map(|r| r.inner.clone()),
            title,
        })
    }
}

// ---------------------------------------------------------------------------
// Serde-style helpers for Python values

/// `data_json(value)`: a Python value as highlighted JSON (core's `JSON`).
#[pyclass(name = "DataJson", module = "rs_rich.ext.data", frozen)]
pub(crate) struct DataJson {
    text: String,
}

impl AsRenderable for DataJson {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let json = rich::Json::new(&self.text).map_err(|e| DataError::new_err(e.to_string()))?;
        Ok(Box::new(json))
    }
}

#[pymethods]
impl DataJson {
    #[new]
    fn new(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let node = python_node(value)?;
        Ok(DataJson {
            text: node.to_json().to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Document transforms

/// `Document(data, *, label=None)`: a data tree on its way to being shown,
/// with the label and highlights transforms decided. Renders as its
/// explorer.
#[pyclass(name = "Document", module = "rs_rich.ext.data", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Document {
    pub(crate) inner: CoreDocument,
}

impl AsRenderable for Document {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let mut explorer = CoreExplorer::new(self.inner.node.clone());
        if let Some(label) = &self.inner.label {
            explorer = explorer.root_label(label.clone());
        }
        for (path, style) in &self.inner.highlights {
            explorer = explorer.highlight(path.clone(), style.clone());
        }
        Ok(Box::new(explorer))
    }
}

#[pymethods]
impl Document {
    #[new]
    #[pyo3(signature = (data, *, label=None))]
    fn new(data: &Bound<'_, PyAny>, label: Option<String>) -> PyResult<Self> {
        let mut inner = CoreDocument::new(node_arg(data)?);
        if let Some(label) = label {
            inner = inner.label(label);
        }
        Ok(Document { inner })
    }

    #[getter]
    fn node(&self) -> DataNode {
        wrap(self.inner.node.clone())
    }

    #[getter]
    fn label(&self) -> Option<String> {
        self.inner.label.clone()
    }

    /// `(path, style)` for each highlighted line.
    #[getter]
    fn highlights(&self) -> Vec<(String, String)> {
        self.inner
            .highlights
            .iter()
            .map(|(path, style)| (path.to_string(), style.definition()))
            .collect()
    }

    /// An `Explorer` over the document, with its label and highlights.
    fn explorer(&self) -> PyResult<Explorer> {
        Ok(Explorer {
            node: self.inner.node.clone(),
            max_depth: None,
            max_length: None,
            max_string: None,
            show_paths: false,
            show_types: false,
            folded: Vec::new(),
            highlighted: self.inner.highlights.clone(),
            root_label: self.inner.label.clone(),
            view: View::Tree,
        })
    }
}

fn document_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreDocument> {
    if let Ok(document) = value.extract::<PyRef<'_, Document>>() {
        return Ok(document.inner.clone());
    }
    Ok(CoreDocument::new(node_arg(value)?))
}

fn transform_error(error: rich_ext::transform::TransformError) -> PyErr {
    TransformError::new_err(error.message().to_string())
}

/// Apply a `rich-ext` document transform to a Python document argument.
fn apply_document(
    transform: &dyn Transform<CoreDocument>,
    document: &Bound<'_, PyAny>,
) -> PyResult<Document> {
    let document = document_arg(document)?;
    transform
        .apply(document)
        .map(|inner| Document { inner })
        .map_err(transform_error)
}

/// `Select(expression)`: narrow a document to what a JSONPath selects.
#[pyclass(name = "Select", module = "rs_rich.ext.data", frozen)]
pub(crate) struct Select {
    inner: CoreSelect,
    #[pyo3(get)]
    expression: String,
}

#[pymethods]
impl Select {
    #[new]
    fn new(expression: String) -> PyResult<Self> {
        Ok(Select {
            inner: CoreSelect::new(&expression).map_err(select_error)?,
            expression,
        })
    }

    fn apply(&self, document: &Bound<'_, PyAny>) -> PyResult<Document> {
        apply_document(&self.inner, document)
    }

    fn __call__(&self, document: &Bound<'_, PyAny>) -> PyResult<Document> {
        self.apply(document)
    }
}

/// `Filter(expression)`: keep what a JSONPath selects and the containers
/// above it.
#[pyclass(name = "Filter", module = "rs_rich.ext.data", frozen)]
pub(crate) struct Filter {
    inner: CoreFilter,
    #[pyo3(get)]
    expression: String,
}

#[pymethods]
impl Filter {
    #[new]
    fn new(expression: String) -> PyResult<Self> {
        Ok(Filter {
            inner: CoreFilter::new(&expression).map_err(select_error)?,
            expression,
        })
    }

    fn apply(&self, document: &Bound<'_, PyAny>) -> PyResult<Document> {
        apply_document(&self.inner, document)
    }

    fn __call__(&self, document: &Bound<'_, PyAny>) -> PyResult<Document> {
        self.apply(document)
    }
}

/// `Highlight(expression, style="reverse")`: style the tree lines of what a
/// JSONPath selects.
#[pyclass(name = "Highlight", module = "rs_rich.ext.data", frozen)]
pub(crate) struct Highlight {
    inner: CoreHighlight,
    #[pyo3(get)]
    expression: String,
}

#[pymethods]
impl Highlight {
    #[new]
    #[pyo3(signature = (expression, style=None))]
    fn new(expression: String, style: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let style = match style {
            Some(style) => common::required_style(style)?,
            None => crate::style::parse_style("reverse")?,
        };
        Ok(Highlight {
            inner: CoreHighlight::new(&expression, style).map_err(select_error)?,
            expression,
        })
    }

    fn apply(&self, document: &Bound<'_, PyAny>) -> PyResult<Document> {
        apply_document(&self.inner, document)
    }

    fn __call__(&self, document: &Bound<'_, PyAny>) -> PyResult<Document> {
        self.apply(document)
    }
}

/// `Redact(redaction=None)`: mask values (default: `Redaction.secrets()`).
#[pyclass(name = "Redact", module = "rs_rich.ext.data", frozen)]
pub(crate) struct Redact {
    inner: CoreRedaction,
}

#[pymethods]
impl Redact {
    #[new]
    #[pyo3(signature = (redaction=None))]
    fn new(redaction: Option<PyRef<'_, Redaction>>) -> Self {
        Redact {
            inner: redaction.map_or_else(CoreRedaction::secrets, |r| r.inner.clone()),
        }
    }

    fn apply(&self, document: &Bound<'_, PyAny>) -> PyResult<Document> {
        apply_document(&CoreRedact(self.inner.clone()), document)
    }

    fn __call__(&self, document: &Bound<'_, PyAny>) -> PyResult<Document> {
        self.apply(document)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<DataNode>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(parse_data, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(detect_format, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_for, m)?)?;
    renderable::add_renderable_class::<Explorer>(m)?;
    renderable::add_renderable_class::<TableView>(m)?;
    renderable::add_renderable_class::<FlatView>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(flatten, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(unflatten, m)?)?;
    m.add_class::<SearchQuery>()?;
    m.add_class::<SearchMatch>()?;
    m.add_function(pyo3::wrap_pyfunction!(search, m)?)?;
    renderable::add_renderable_class::<SearchResults>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(select, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(selector_backends, m)?)?;
    m.add_class::<DataChange>()?;
    m.add_function(pyo3::wrap_pyfunction!(diff_data, m)?)?;
    renderable::add_renderable_class::<DataDiffView>(m)?;
    m.add_class::<Redaction>()?;
    renderable::add_renderable_class::<ConfigFileView>(m)?;
    renderable::add_renderable_class::<DataJson>(m)?;
    renderable::add_renderable_class::<Document>(m)?;
    m.add_class::<Select>()?;
    m.add_class::<Filter>()?;
    m.add_class::<Highlight>()?;
    m.add_class::<Redact>()?;
    m.add(
        "DATA_FORMATS",
        Format::ALL
            .iter()
            .map(|f| format_name(*f))
            .collect::<Vec<_>>(),
    )?;
    m.add("SECRET_KEYS", core::SECRET_KEYS.to_vec())?;
    Ok(())
}
