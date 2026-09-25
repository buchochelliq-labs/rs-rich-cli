//! `rich.inspect` and `rich._inspect.Inspect`: a report on any Python
//! object. The object is examined here (its attributes, signatures and
//! docstrings are Python's to give); the report is core renderables
//! (`Panel`, `Table`, `Text`) and this area's `Pretty`.

use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use rich::protocol::Renderable;
use rich::style::StyleType;
use rich::table::{Cell, ColumnOptions, Table};
use rich::{Justify, Text as CoreText};

use super::highlighter::Highlight;
use super::layout::{to_markup, Blank, NamedPanel, Stack};
use super::pretty::{shared_repr, traverse, type_repr, Layout, Limits};
use crate::renderable::{self, AsRenderable};

fn named(style: &str) -> Option<StyleType> {
    Some(StyleType::Name(style.to_string()))
}

/// Upstream's `escape_control_codes`: BEL, BS, VT, FF and CR as escapes.
fn escape_control_codes(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\x07' => escaped.push_str("\\a"),
            '\x08' => escaped.push_str("\\b"),
            '\x0b' => escaped.push_str("\\v"),
            '\x0c' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Python's `str.strip("_").lower()`.
fn sort_name(key: &str) -> String {
    key.trim_matches('_').to_lowercase()
}

fn repr_text(py: Python<'_>, text: &str) -> PyResult<CoreText> {
    Highlight::Repr.apply(py, CoreText::new(text))
}

/// The options of an `Inspect`.
#[derive(Clone)]
struct Options {
    help: bool,
    methods: bool,
    docs: bool,
    private: bool,
    dunder: bool,
    sort: bool,
    value: bool,
}

struct Inspector<'py> {
    py: Python<'py>,
    inspect: Bound<'py, PyModule>,
    options: Options,
}

impl<'py> Inspector<'py> {
    fn is_class(&self, object: &Bound<'py, PyAny>) -> PyResult<bool> {
        self.inspect.call_method1("isclass", (object,))?.is_truthy()
    }

    fn is_module(&self, object: &Bound<'py, PyAny>) -> PyResult<bool> {
        self.inspect
            .call_method1("ismodule", (object,))?
            .is_truthy()
    }

    /// Upstream's `_get_formatted_doc`.
    fn formatted_doc(&self, object: &Bound<'py, PyAny>) -> PyResult<Option<String>> {
        let docs = self.inspect.call_method1("getdoc", (object,))?;
        if docs.is_none() {
            return Ok(None);
        }
        let docs: String = self.inspect.call_method1("cleandoc", (docs,))?.extract()?;
        let mut docs = docs.trim().to_string();
        if !self.options.help {
            if let Some(end) = docs.find("\n\n") {
                docs.truncate(end);
            }
        }
        Ok(Some(escape_control_codes(&docs)))
    }

    /// Upstream's `_get_signature`.
    fn signature(&self, name: &str, object: &Bound<'py, PyAny>) -> PyResult<Option<CoreText>> {
        let py = self.py;
        let signature = match self.inspect.call_method1("signature", (object,)) {
            Ok(signature) => format!("{}:", signature.str()?),
            Err(error) if error.is_instance_of::<pyo3::exceptions::PyValueError>(py) => {
                "(...)".to_string()
            }
            Err(error) if error.is_instance_of::<pyo3::exceptions::PyTypeError>(py) => {
                return Ok(None)
            }
            Err(error) => return Err(error),
        };
        let signature_text = repr_text(py, &signature)?;
        let mut qualname = name.to_string();
        if qualname.is_empty() {
            let attr = object.getattr_opt("__qualname__")?;
            match attr.as_ref().filter(|a| a.is_instance_of::<PyString>()) {
                Some(value) => qualname = value.extract()?,
                None => {
                    if let Some(value) = object
                        .getattr_opt("__name__")?
                        .filter(|a| a.is_instance_of::<PyString>())
                    {
                        qualname = value.extract()?;
                    }
                }
            }
        }
        let prefix = if self.is_class(object)? {
            "class"
        } else if self
            .inspect
            .call_method1("iscoroutinefunction", (object,))?
            .is_truthy()?
        {
            "async def"
        } else {
            "def"
        };
        let mut text = CoreText::new("");
        text.append(
            &format!("{prefix} "),
            named(&format!("inspect.{}", prefix.replace(' ', "_"))),
        );
        text.append(&qualname, named("inspect.callable"));
        Ok(Some(text.append_text(&signature_text)))
    }

    fn pretty(&self, value: &Bound<'py, PyAny>, limits: Limits, guides: bool) -> PyResult<Cell> {
        let mut layout = Layout::new(traverse(value, limits)?, type_repr(value)?);
        layout.indent_guides = guides;
        Ok(Cell::Renderable(shared_repr(layout, &[])))
    }

    /// Upstream's `_render`.
    fn render(
        &self,
        object: &Bound<'py, PyAny>,
    ) -> PyResult<Vec<Box<dyn Renderable + Send + Sync>>> {
        let py = self.py;
        let mut parts: Vec<Box<dyn Renderable + Send + Sync>> = Vec::new();
        let mut keys: Vec<String> = py
            .import("builtins")?
            .call_method1("dir", (object,))?
            .extract()?;
        let total = keys.len();
        if !self.options.dunder {
            keys.retain(|key| !key.starts_with("__"));
        }
        if !self.options.private {
            keys.retain(|key| !key.starts_with('_'));
        }
        let not_shown = total - keys.len();
        let mut items: Vec<(String, Result<Bound<'py, PyAny>, PyErr>, bool)> = keys
            .into_iter()
            .map(|key| {
                let value = object.getattr(key.as_str());
                let callable = value.as_ref().is_ok_and(|v| v.is_callable());
                (key, value, callable)
            })
            .collect();
        if self.options.sort {
            items.sort_by_cached_key(|(key, value, callable)| {
                // Upstream sorts an attribute that raised as not callable.
                (value.is_ok() && *callable, sort_name(key))
            });
        }

        let is_class = self.is_class(object)?;
        let is_module = self.is_module(object)?;
        if object.is_callable() {
            if let Some(signature) = self.signature("", object)? {
                parts.push(Box::new(signature));
                parts.push(Box::new(Blank));
            }
        }
        if self.options.docs {
            if let Some(doc) = self.formatted_doc(object)? {
                let doc = Highlight::Repr.apply(
                    py,
                    CoreText::styled(doc, StyleType::Name("inspect.help".to_string())),
                )?;
                parts.push(Box::new(doc));
                parts.push(Box::new(Blank));
            }
        }
        if self.options.value && !(is_class || object.is_callable() || is_module) {
            let limits = Limits {
                max_length: Some(10),
                max_string: Some(60),
                max_depth: None,
            };
            let mut layout = Layout::new(traverse(object, limits)?, type_repr(object)?);
            layout.indent_guides = true;
            parts.push(Box::new(NamedPanel {
                child: shared_repr(layout, &[]),
                border_style: StyleType::Name("inspect.value.border".to_string()),
                title: None,
                expand: true,
                width: None,
                padding: (0, 1, 0, 1),
            }));
            parts.push(Box::new(Blank));
        }

        let mut table = Table::grid().padding(0, 1, 0, 1).expand(false);
        table.add_column_with(
            CoreText::new(""),
            ColumnOptions {
                justify: Justify::Right,
                ..ColumnOptions::default()
            },
        );
        table.add_column_with(CoreText::new(""), ColumnOptions::default());
        let mut rows = 0;
        for (key, value, callable) in items {
            let mut key_text = CoreText::new("");
            key_text.append(
                &key,
                named(if key.starts_with("__") {
                    "inspect.attr.dunder"
                } else {
                    "inspect.attr"
                }),
            );
            key_text.append(" =", named("inspect.equals"));
            let value = match value {
                Ok(value) => value,
                Err(error) => {
                    let length = key_text.plain().len();
                    key_text.stylize(StyleType::Name("inspect.error".to_string()), 0, length);
                    let repr = error.value(py).repr()?.to_string();
                    table.add_row_cells(vec![
                        Cell::Text(key_text),
                        Cell::Text(repr_text(py, &repr)?),
                    ]);
                    rows += 1;
                    continue;
                }
            };
            let cell = if callable {
                if !self.options.methods {
                    continue;
                }
                match self.signature(&key, &value)? {
                    None => self.pretty(&value, Limits::default(), false)?,
                    Some(mut signature) => {
                        if self.options.docs {
                            if let Some(docs) = self.formatted_doc(&value)? {
                                signature
                                    .append(if docs.contains('\n') { "\n" } else { " " }, None);
                                let mut doc = repr_text(py, &docs)?;
                                let length = doc.plain().len();
                                doc.stylize(StyleType::Name("inspect.doc".to_string()), 0, length);
                                signature = signature.append_text(&doc);
                            }
                        }
                        Cell::Text(signature)
                    }
                }
            } else {
                self.pretty(&value, Limits::default(), false)?
            };
            table.add_row_cells(vec![Cell::Text(key_text), cell]);
            rows += 1;
        }
        if rows > 0 {
            parts.push(Box::new(table));
        } else if not_shown > 0 {
            let text = CoreText::from_markup(&format!(
                "[b cyan]{not_shown}[/][i] attribute(s) not shown.[/i] Run \
                 [b][magenta]inspect[/]([not b]inspect[/])[/b] for options."
            ))
            .map_err(|e| crate::errors::MarkupError::new_err(e.to_string()))?;
            parts.push(Box::new(text));
        }
        Ok(parts)
    }
}

/// `rich._inspect.Inspect`: a renderable report on an object.
#[pyclass(name = "Inspect", module = "rs_rich._inspect")]
pub(crate) struct Inspect {
    object: Py<PyAny>,
    title: Option<Py<PyAny>>,
    options: Options,
}

impl Inspect {
    fn title_markup(&self, py: Python<'_>, inspector: &Inspector<'_>) -> PyResult<String> {
        if let Some(title) = self.title.as_ref().map(|t| t.bind(py)) {
            if let Ok(text) = title.extract::<PyRef<'_, crate::text::Text>>() {
                return Ok(to_markup(&text.inner));
            }
            return Ok(title.str()?.to_string());
        }
        // Upstream's `_make_title`.
        let object = self.object.bind(py);
        let title = if inspector.is_class(object)?
            || object.is_callable()
            || inspector.is_module(object)?
        {
            object.str()?.to_string()
        } else {
            object.get_type().str()?.to_string()
        };
        Ok(to_markup(&repr_text(py, &title)?))
    }
}

impl AsRenderable for Inspect {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let inspector = Inspector {
            py,
            inspect: py.import("inspect")?,
            options: self.options.clone(),
        };
        let parts = inspector.render(self.object.bind(py))?;
        let group: Arc<dyn Renderable + Send + Sync> = Arc::new(Stack { children: parts });
        Ok(Box::new(NamedPanel {
            child: group,
            border_style: StyleType::Name("scope.border".to_string()),
            title: Some(self.title_markup(py, &inspector)?),
            expand: false,
            width: None,
            padding: (0, 1, 0, 1),
        }))
    }
}

#[pymethods]
impl Inspect {
    #[new]
    #[pyo3(signature = (
        obj, *, title=None, help=false, methods=false, docs=true, private=false, dunder=false,
        sort=true, all=true, value=true
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        obj: Py<PyAny>,
        title: Option<Py<PyAny>>,
        help: bool,
        methods: bool,
        docs: bool,
        private: bool,
        dunder: bool,
        sort: bool,
        all: bool,
        value: bool,
    ) -> Self {
        let (methods, private, dunder) = if all {
            (true, true, true)
        } else {
            (methods, private, dunder)
        };
        Inspect {
            object: obj,
            title,
            options: Options {
                help,
                methods,
                docs: docs || help,
                private: private || dunder,
                dunder,
                sort,
                value,
            },
        }
    }

    #[getter]
    fn obj(&self, py: Python<'_>) -> Py<PyAny> {
        self.object.clone_ref(py)
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.object)?;
        if let Some(title) = &self.title {
            visit.call(title)?;
        }
        Ok(())
    }
}

/// `rich.inspect`: print a report on any object (the package's `inspect`
/// calls this with its console).
#[pyfunction]
#[pyo3(signature = (
    obj, *, console=None, title=None, help=false, methods=false, docs=true, private=false,
    dunder=false, sort=true, all=false, value=true
))]
#[allow(clippy::too_many_arguments)]
fn inspect(
    py: Python<'_>,
    obj: Py<PyAny>,
    console: Option<&Bound<'_, PyAny>>,
    title: Option<Py<PyAny>>,
    help: bool,
    methods: bool,
    docs: bool,
    private: bool,
    dunder: bool,
    sort: bool,
    all: bool,
    value: bool,
) -> PyResult<()> {
    let console = super::pretty::console_or_global(py, console)?;
    // `inspect(inspect)` shows its own help and methods.
    let is_inspect = py
        .import("rs_rich")?
        .getattr("inspect")
        .map(|f| f.is(obj.bind(py)))
        .unwrap_or(false);
    let report = Inspect::new(
        obj,
        title,
        is_inspect || help,
        is_inspect || methods,
        is_inspect || docs,
        private,
        dunder,
        sort,
        all,
        value,
    );
    let kwargs = PyDict::new(py);
    console.call_method("print", (Py::new(py, report)?,), Some(&kwargs))?;
    Ok(())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Inspect>(m)?;
    m.add_function(wrap_pyfunction!(inspect, m)?)?;
    Ok(())
}
