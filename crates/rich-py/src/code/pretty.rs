//! `rich.pretty`: `Pretty`, `Node`, `traverse`, `pretty_repr`, `pprint` and
//! `install`.
//!
//! Upstream builds a tree of `Node`s by walking a Python object (containers,
//! dataclasses, attrs classes, named tuples and `__rich_repr__`), then lays
//! it out, expanding a container onto several lines only when it does not
//! fit. Core's `Pretty` formats a Rust value's `Debug` output instead, and
//! cannot see Python objects, so both halves live here: the walk is the
//! binding's reflection of Python values, the layout is a line-for-line port
//! of upstream's `Node.render` and `_Line`.

use std::collections::HashSet;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyBytes, PyDict, PyList, PySlice, PyString, PyTuple, PyType};

use rich::cells::cell_len;
use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;
use rich::{AnsiDecoder, Text as CoreText};

use super::highlighter::Highlight;
use crate::convert;
use crate::renderable::{self, AsRenderable, PyRenderable};

// ---------------------------------------------------------------------------
// Node and its layout (upstream `Node` and `_Line`)

/// A node in a repr tree: an atom (`value_repr`) or a container.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) key_repr: String,
    pub(crate) value_repr: String,
    pub(crate) open_brace: String,
    pub(crate) close_brace: String,
    pub(crate) empty: String,
    pub(crate) last: bool,
    pub(crate) is_tuple: bool,
    pub(crate) is_namedtuple: bool,
    pub(crate) children: Option<Vec<Node>>,
    pub(crate) key_separator: String,
    pub(crate) separator: String,
}

impl Default for Node {
    fn default() -> Self {
        Node {
            key_repr: String::new(),
            value_repr: String::new(),
            open_brace: String::new(),
            close_brace: String::new(),
            empty: String::new(),
            last: false,
            is_tuple: false,
            is_namedtuple: false,
            children: None,
            key_separator: ": ".to_string(),
            separator: ", ".to_string(),
        }
    }
}

impl Node {
    fn value(value_repr: impl Into<String>) -> Node {
        Node {
            value_repr: value_repr.into(),
            ..Node::default()
        }
    }

    /// `Node.iter_tokens`, as a visitor that stops when `f` returns false.
    /// Returns false when stopped.
    fn tokens<'a>(&'a self, f: &mut dyn FnMut(&'a str) -> bool) -> bool {
        if !self.key_repr.is_empty() && !(f(&self.key_repr) && f(&self.key_separator)) {
            return false;
        }
        if !self.value_repr.is_empty() {
            return f(&self.value_repr);
        }
        let Some(children) = &self.children else {
            return true;
        };
        if children.is_empty() {
            return f(&self.empty);
        }
        if !f(&self.open_brace) {
            return false;
        }
        if self.is_tuple && !self.is_namedtuple && children.len() == 1 {
            if !(children[0].tokens(f) && f(",")) {
                return false;
            }
        } else {
            for child in children {
                if !child.tokens(f) {
                    return false;
                }
                if !child.last && !f(&self.separator) {
                    return false;
                }
            }
        }
        f(&self.close_brace)
    }

    pub(crate) fn token_list(&self) -> Vec<String> {
        let mut tokens = Vec::new();
        self.tokens(&mut |token| {
            tokens.push(token.to_string());
            true
        });
        tokens
    }

    /// `Node.check_length`: whether the node fits in `max_length` cells
    /// after `start_length`.
    pub(crate) fn check_length(&self, start_length: isize, max_length: isize) -> bool {
        let mut total = start_length;
        self.tokens(&mut |token| {
            total += cell_len(token) as isize;
            total <= max_length
        })
    }

    /// `str(node)`: the one-line repr.
    pub(crate) fn to_repr(&self) -> String {
        let mut repr = String::new();
        self.tokens(&mut |token| {
            repr.push_str(token);
            true
        });
        repr
    }

    /// `Node.render`: the repr, expanded onto new lines to fit `max_width`.
    pub(crate) fn render(&self, max_width: isize, indent_size: usize, expand_all: bool) -> String {
        let mut lines = vec![Line {
            node: Some(self),
            ..Line::default()
        }];
        let mut line_no = 0;
        while line_no < lines.len() {
            let line = &lines[line_no];
            if line.expandable() && (expand_all || !line.check_length(max_width)) {
                let expanded = line.expand(indent_size);
                lines.splice(line_no..line_no + 1, expanded);
            }
            line_no += 1;
        }
        lines
            .iter()
            .map(Line::to_repr)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Upstream's `_Line`: one line of repr output.
#[derive(Default)]
struct Line<'a> {
    node: Option<&'a Node>,
    text: String,
    suffix: String,
    whitespace: String,
    last: bool,
}

impl<'a> Line<'a> {
    fn expandable(&self) -> bool {
        self.node
            .and_then(|node| node.children.as_ref())
            .is_some_and(|children| !children.is_empty())
    }

    fn check_length(&self, max_length: isize) -> bool {
        let start = self.whitespace.len() + cell_len(&self.text) + cell_len(&self.suffix);
        self.node
            .expect("an expandable line has a node")
            .check_length(start as isize, max_length)
    }

    fn expand(&self, indent_size: usize) -> Vec<Line<'a>> {
        let node = self.node.expect("an expandable line has a node");
        let children = node.children.as_deref().unwrap_or_default();
        let mut lines = Vec::with_capacity(children.len() + 2);
        let text = if node.key_repr.is_empty() {
            node.open_brace.clone()
        } else {
            format!("{}{}{}", node.key_repr, node.key_separator, node.open_brace)
        };
        lines.push(Line {
            text,
            whitespace: self.whitespace.clone(),
            ..Line::default()
        });
        let child_whitespace = format!("{}{}", self.whitespace, " ".repeat(indent_size));
        let tuple_of_one = node.is_tuple && children.len() == 1;
        let last_index = children.len().saturating_sub(1);
        for (index, child) in children.iter().enumerate() {
            let separator = if tuple_of_one { "," } else { &node.separator };
            lines.push(Line {
                node: Some(child),
                whitespace: child_whitespace.clone(),
                suffix: separator.to_string(),
                last: index == last_index && !tuple_of_one,
                ..Line::default()
            });
        }
        lines.push(Line {
            text: node.close_brace.clone(),
            whitespace: self.whitespace.clone(),
            suffix: self.suffix.clone(),
            last: self.last,
            ..Line::default()
        });
        lines
    }

    fn to_repr(&self) -> String {
        let node = self.node.map(Node::to_repr).unwrap_or_default();
        if self.last {
            format!("{}{}{}", self.whitespace, self.text, node)
        } else {
            format!(
                "{}{}{}{}",
                self.whitespace,
                self.text,
                node,
                self.suffix.trim_end()
            )
        }
    }
}

// ---------------------------------------------------------------------------
// traverse: a Python object to a Node tree

/// Limits for [`traverse`].
#[derive(Clone, Copy, Default)]
pub(crate) struct Limits {
    pub(crate) max_length: Option<usize>,
    pub(crate) max_string: Option<usize>,
    pub(crate) max_depth: Option<usize>,
}

/// Python helpers the walk needs, looked up once.
struct Helpers {
    containers: Vec<Py<PyType>>,
    mappings: Py<PyTuple>,
    dataclass_files: Vec<Py<PyAny>>,
    namedtuple_file: Py<PyAny>,
    attr: Option<Py<PyModule>>,
}

fn helpers(py: Python<'_>) -> PyResult<&Helpers> {
    static HELPERS: PyOnceLock<Helpers> = PyOnceLock::new();
    HELPERS.get_or_try_init(py, || {
        let builtins = py.import("builtins")?;
        let collections = py.import("collections")?;
        let os = py.import("os")?;
        let mapping_proxy = py.import("types")?.getattr("MappingProxyType")?;
        // Upstream's `_BRACES` order: the first class an object is an
        // instance of picks its braces.
        let containers = [
            os.getattr("_Environ")?,
            py.import("array")?.getattr("array")?,
            collections.getattr("defaultdict")?,
            collections.getattr("Counter")?,
            collections.getattr("deque")?,
            builtins.getattr("dict")?,
            collections.getattr("UserDict")?,
            builtins.getattr("frozenset")?,
            builtins.getattr("list")?,
            collections.getattr("UserList")?,
            builtins.getattr("set")?,
            builtins.getattr("tuple")?,
            mapping_proxy.clone(),
        ]
        .into_iter()
        .map(|class| Ok(class.cast_into::<PyType>()?.unbind()))
        .collect::<PyResult<Vec<_>>>()?;
        let mappings = PyTuple::new(
            py,
            [
                builtins.getattr("dict")?,
                os.getattr("_Environ")?,
                mapping_proxy,
                collections.getattr("UserDict")?,
            ],
        )?
        .unbind();
        let dataclass_files = vec![
            py.import("dataclasses")?.getattr("__file__")?.unbind(),
            py.import("reprlib")?.getattr("__file__")?.unbind(),
        ];
        let dummy =
            collections.call_method1("namedtuple", ("_dummy_namedtuple", PyList::empty(py)))?;
        let namedtuple_file = py
            .import("inspect")?
            .call_method1("getfile", (dummy.getattr("__repr__")?,))?
            .unbind();
        let attr = match py.import("attr") {
            Ok(module) if module.hasattr("ib")? => Some(module.unbind()),
            _ => None,
        };
        Ok(Helpers {
            containers,
            mappings,
            dataclass_files,
            namedtuple_file,
            attr,
        })
    })
}

fn safe_isinstance(object: &Bound<'_, PyAny>, class: &Bound<'_, PyAny>) -> bool {
    object.is_instance(class).unwrap_or(false)
}

fn is_class(object: &Bound<'_, PyAny>) -> bool {
    object.is_instance_of::<PyType>()
}

/// `_is_namedtuple`: a tuple with a tuple `_fields`.
fn is_namedtuple(object: &Bound<'_, PyAny>) -> bool {
    let fields = match object.getattr_opt("_fields") {
        Ok(Some(fields)) => fields,
        _ => return false,
    };
    object.is_instance_of::<PyTuple>() && fields.is_instance_of::<PyTuple>()
}

/// One attrs field: its name, its value (or the error getting it), and
/// its own repr function.
type AttrItem<'py> = (
    String,
    Result<Bound<'py, PyAny>, PyErr>,
    Option<Bound<'py, PyAny>>,
);

struct Walker<'a, 'py> {
    py: Python<'py>,
    limits: Limits,
    helpers: &'a Helpers,
    visited: HashSet<usize>,
}

impl<'py> Walker<'_, 'py> {
    /// `to_repr`: the repr, with long strings cut at `max_string`, and a
    /// failing `__repr__` reported rather than raised.
    fn to_repr(&self, object: &Bound<'py, PyAny>) -> PyResult<String> {
        if let Some(max_string) = self.limits.max_string {
            if object.is_instance_of::<PyString>() || object.is_instance_of::<PyBytes>() {
                let length = object.len()?;
                if length > max_string {
                    let head = object.get_item(PySlice::new(self.py, 0, max_string as isize, 1))?;
                    return Ok(format!("{}+{}", head.repr()?, length - max_string));
                }
            }
        }
        match object.repr() {
            Ok(repr) => Ok(repr.to_string()),
            Err(error) => {
                let message = error.value(self.py).str()?;
                Ok(format!("<repr-error {}>", message.repr()?))
            }
        }
    }

    fn is_attr_object(&self, object: &Bound<'py, PyAny>) -> bool {
        let Some(attr) = &self.helpers.attr else {
            return false;
        };
        attr.bind(self.py)
            .call_method1("has", (object.get_type(),))
            .and_then(|has| has.is_truthy())
            .unwrap_or(false)
    }

    fn is_dataclass_repr(&self, object: &Bound<'py, PyAny>) -> bool {
        let file = (|| {
            object
                .getattr("__repr__")?
                .getattr("__code__")?
                .getattr("co_filename")
        })();
        match file {
            Ok(file) => self
                .helpers
                .dataclass_files
                .iter()
                .any(|known| file.eq(known.bind(self.py)).unwrap_or(false)),
            Err(_) => false,
        }
    }

    fn has_default_namedtuple_repr(&self, object: &Bound<'py, PyAny>) -> PyResult<bool> {
        let file = object
            .getattr("__repr__")
            .and_then(|repr| self.py.import("inspect")?.call_method1("getfile", (repr,)));
        Ok(match file {
            Ok(file) => file.eq(self.helpers.namedtuple_file.bind(self.py))?,
            Err(error)
                if error.is_instance_of::<pyo3::exceptions::PyOSError>(self.py)
                    || error.is_instance_of::<PyTypeError>(self.py) =>
            {
                false
            }
            Err(error) => return Err(error),
        })
    }

    /// Mark `children` so the last one knows it is last (upstream's
    /// `loop_last`).
    fn keyed(&mut self, key: String, child: &Bound<'py, PyAny>, depth: usize) -> PyResult<Node> {
        let mut node = self.walk(child, false, depth + 1)?;
        node.key_repr = key;
        node.key_separator = "=".to_string();
        Ok(node)
    }

    fn walk(&mut self, object: &Bound<'py, PyAny>, root: bool, depth: usize) -> PyResult<Node> {
        let py = self.py;
        let id = object.as_ptr() as usize;
        if self.visited.contains(&id) {
            return Ok(Node::value("..."));
        }
        let reached_max_depth = self.limits.max_depth.is_some_and(|max| depth >= max);
        let fake_attributes = object
            .hasattr("awehoi234_wdfjwljet234_234wdfoijsdfmmnxpi492")
            .unwrap_or(false);
        let class_name =
            || -> PyResult<String> { object.getattr("__class__")?.getattr("__name__")?.extract() };

        let mut rich_repr_result = None;
        if !fake_attributes {
            if let Ok(true) = object.hasattr("__rich_repr__") {
                if !is_class(object) {
                    if let Ok(result) = object.call_method0("__rich_repr__") {
                        if !result.is_none() {
                            rich_repr_result = Some(result);
                        }
                    }
                }
            }
        }

        let mut node = if let Some(result) = rich_repr_result {
            self.visited.insert(id);
            let angular = object
                .getattr("__rich_repr__")?
                .getattr_opt("angular")?
                .map(|a| a.is_truthy())
                .transpose()?
                .unwrap_or(false);
            // `iter_rich_args`.
            let mut args: Vec<(Option<String>, Bound<'py, PyAny>)> = Vec::new();
            for arg in result.try_iter()? {
                let arg = arg?;
                if let Ok(tuple) = arg.cast::<PyTuple>() {
                    match tuple.len() {
                        3 => {
                            let (key, child, default) =
                                (tuple.get_item(0)?, tuple.get_item(1)?, tuple.get_item(2)?);
                            if default.eq(&child)? {
                                continue;
                            }
                            args.push((Some(key.extract()?), child));
                        }
                        2 => args.push((Some(tuple.get_item(0)?.extract()?), tuple.get_item(1)?)),
                        1 => args.push((None, tuple.get_item(0)?)),
                        _ => {}
                    }
                } else {
                    args.push((None, arg));
                }
            }
            let class_name = class_name()?;
            let node = if args.is_empty() {
                Node {
                    value_repr: if angular {
                        format!("<{class_name}>")
                    } else {
                        format!("{class_name}()")
                    },
                    children: Some(Vec::new()),
                    last: root,
                    ..Node::default()
                }
            } else if reached_max_depth {
                Node::value(if angular {
                    format!("<{class_name}...>")
                } else {
                    format!("{class_name}(...)")
                })
            } else {
                let last_index = args.len() - 1;
                let mut children = Vec::with_capacity(args.len());
                for (index, (key, child)) in args.iter().enumerate() {
                    let mut child_node = match key {
                        Some(key) => self.keyed(key.clone(), child, depth)?,
                        None => self.walk(child, false, depth + 1)?,
                    };
                    child_node.last = index == last_index;
                    children.push(child_node);
                }
                if angular {
                    Node {
                        open_brace: format!("<{class_name} "),
                        close_brace: ">".to_string(),
                        children: Some(children),
                        last: root,
                        separator: " ".to_string(),
                        ..Node::default()
                    }
                } else {
                    Node {
                        open_brace: format!("{class_name}("),
                        close_brace: ")".to_string(),
                        children: Some(children),
                        last: root,
                        ..Node::default()
                    }
                }
            };
            self.visited.remove(&id);
            node
        } else if self.is_attr_object(object) && !fake_attributes {
            self.visited.insert(id);
            let attr = self
                .helpers
                .attr
                .as_ref()
                .expect("attrs is installed")
                .bind(py);
            let fields = attr.call_method1("fields", (object.get_type(),))?;
            let node = if fields.len()? == 0 {
                Node {
                    value_repr: format!("{}()", class_name()?),
                    children: Some(Vec::new()),
                    last: root,
                    ..Node::default()
                }
            } else if reached_max_depth {
                Node::value(format!("{}(...)", class_name()?))
            } else {
                // `iter_attrs`.
                let mut items: Vec<AttrItem<'py>> = Vec::new();
                for field in fields.try_iter()? {
                    let field = field?;
                    let repr = field.getattr("repr")?;
                    if !repr.is_truthy()? {
                        continue;
                    }
                    let name: String = field.getattr("name")?.extract()?;
                    let value = object.getattr(name.as_str());
                    let callable = if value.is_ok() && repr.is_callable() {
                        Some(repr)
                    } else {
                        None
                    };
                    items.push((name, value, callable));
                }
                let last_index = items.len().saturating_sub(1);
                let mut children = Vec::with_capacity(items.len());
                for (index, (name, value, callable)) in items.into_iter().enumerate() {
                    let value = value
                        .unwrap_or_else(|error| error.into_value(py).into_bound(py).into_any());
                    let mut child_node = match callable {
                        Some(callable) => {
                            Node::value(callable.call1((&value,))?.str()?.to_string())
                        }
                        None => self.walk(&value, false, depth + 1)?,
                    };
                    child_node.last = index == last_index;
                    child_node.key_repr = name;
                    child_node.key_separator = "=".to_string();
                    children.push(child_node);
                }
                Node {
                    open_brace: format!("{}(", class_name()?),
                    close_brace: ")".to_string(),
                    children: Some(children),
                    last: root,
                    ..Node::default()
                }
            };
            self.visited.remove(&id);
            node
        } else if py
            .import("dataclasses")?
            .call_method1("is_dataclass", (object,))?
            .is_truthy()?
            && !is_class(object)
            && !fake_attributes
            && self.is_dataclass_repr(object)
        {
            self.visited.insert(id);
            let class_name = class_name()?;
            let node = if reached_max_depth {
                Node::value(format!("{class_name}(...)"))
            } else {
                let mut fields = Vec::new();
                for field in py
                    .import("dataclasses")?
                    .call_method1("fields", (object,))?
                    .try_iter()?
                {
                    let field = field?;
                    let name: String = field.getattr("name")?.extract()?;
                    if field.getattr("repr")?.is_truthy()? && object.hasattr(name.as_str())? {
                        fields.push(name);
                    }
                }
                let last_index = fields.len().saturating_sub(1);
                let mut children = Vec::with_capacity(fields.len());
                for (index, name) in fields.into_iter().enumerate() {
                    let value = object.getattr(name.as_str())?;
                    let mut child_node = self.keyed(name, &value, depth)?;
                    child_node.last = index == last_index;
                    children.push(child_node);
                }
                Node {
                    open_brace: format!("{class_name}("),
                    close_brace: ")".to_string(),
                    children: Some(children),
                    last: root,
                    empty: format!("{class_name}()"),
                    ..Node::default()
                }
            };
            self.visited.remove(&id);
            node
        } else if is_namedtuple(object) && self.has_default_namedtuple_repr(object)? {
            self.visited.insert(id);
            let class_name = class_name()?;
            let node = if reached_max_depth {
                Node::value(format!("{class_name}(...)"))
            } else {
                let items = object.call_method0("_asdict")?.call_method0("items")?;
                let items: Vec<(String, Bound<'py, PyAny>)> = items
                    .try_iter()?
                    .map(|item| item?.extract())
                    .collect::<PyResult<_>>()?;
                let last_index = items.len().saturating_sub(1);
                let mut children = Vec::with_capacity(items.len());
                for (index, (key, value)) in items.into_iter().enumerate() {
                    let mut child_node = self.keyed(key, &value, depth)?;
                    child_node.last = index == last_index;
                    children.push(child_node);
                }
                Node {
                    open_brace: format!("{class_name}("),
                    close_brace: ")".to_string(),
                    children: Some(children),
                    empty: format!("{class_name}()"),
                    ..Node::default()
                }
            };
            self.visited.remove(&id);
            node
        } else if let Some(container) = self
            .helpers
            .containers
            .iter()
            .map(|class| class.bind(py))
            .find(|class| safe_isinstance(object, class.as_any()))
        {
            self.visited.insert(id);
            let (open_brace, close_brace, empty) = braces(object, container)?;
            let node = if reached_max_depth {
                Node::value(format!("{open_brace}...{close_brace}"))
            } else if !container
                .getattr("__repr__")?
                .eq(object.get_type().getattr("__repr__")?)?
            {
                Node {
                    value_repr: self.to_repr(object)?,
                    last: root,
                    ..Node::default()
                }
            } else if object.is_truthy()? {
                let num_items = object.len()?;
                let last_item_index = num_items as isize - 1;
                let mut children = Vec::new();
                let is_mapping = safe_isinstance(object, self.helpers.mappings.bind(py).as_any());
                let max_length = self.limits.max_length;
                if is_mapping {
                    let items = object.call_method0("items")?;
                    for (index, item) in items.try_iter()?.enumerate() {
                        if max_length.is_some_and(|max| index >= max) {
                            break;
                        }
                        let (key, child): (Bound<'py, PyAny>, Bound<'py, PyAny>) =
                            item?.extract()?;
                        let mut child_node = self.walk(&child, false, depth + 1)?;
                        child_node.key_repr = self.to_repr(&key)?;
                        child_node.last = index as isize == last_item_index;
                        children.push(child_node);
                    }
                } else {
                    for (index, child) in object.try_iter()?.enumerate() {
                        if max_length.is_some_and(|max| index >= max) {
                            break;
                        }
                        let mut child_node = self.walk(&child?, false, depth + 1)?;
                        child_node.last = index as isize == last_item_index;
                        children.push(child_node);
                    }
                }
                if let Some(max_length) = max_length {
                    if num_items > max_length {
                        children.push(Node {
                            value_repr: format!("... +{}", num_items - max_length),
                            last: true,
                            ..Node::default()
                        });
                    }
                }
                Node {
                    open_brace,
                    close_brace,
                    children: Some(children),
                    last: root,
                    ..Node::default()
                }
            } else {
                Node {
                    empty,
                    children: Some(Vec::new()),
                    last: root,
                    ..Node::default()
                }
            };
            self.visited.remove(&id);
            node
        } else {
            Node {
                value_repr: self.to_repr(object)?,
                last: root,
                ..Node::default()
            }
        };
        node.is_tuple = object.get_type().is(py.get_type::<PyTuple>());
        node.is_namedtuple = is_namedtuple(object);
        Ok(node)
    }
}

/// Upstream's `_BRACES[type](obj)`.
fn braces(
    object: &Bound<'_, PyAny>,
    container: &Bound<'_, PyType>,
) -> PyResult<(String, String, String)> {
    let name: String = container.getattr("__qualname__")?.extract()?;
    let module: String = container.getattr("__module__")?.extract()?;
    let simple = |open: &str, close: &str, empty: &str| {
        (open.to_string(), close.to_string(), empty.to_string())
    };
    Ok(match (module.as_str(), name.as_str()) {
        ("os", "_Environ") => simple("environ({", "})", "environ({})"),
        ("array", "array") => {
            let typecode = object.getattr("typecode")?.repr()?;
            (
                format!("array({typecode}, ["),
                "])".to_string(),
                format!("array({typecode})"),
            )
        }
        ("collections", "defaultdict") => {
            let factory = object.getattr("default_factory")?.repr()?;
            (
                format!("defaultdict({factory}, {{"),
                "})".to_string(),
                format!("defaultdict({factory}, {{}})"),
            )
        }
        ("collections", "Counter") => simple("Counter({", "})", "Counter()"),
        ("collections", "deque") => {
            let maxlen = object.getattr("maxlen")?;
            if maxlen.is_none() {
                simple("deque([", "])", "deque()")
            } else {
                let maxlen = maxlen.repr()?;
                (
                    "deque([".to_string(),
                    format!("], maxlen={maxlen})"),
                    format!("deque(maxlen={maxlen})"),
                )
            }
        }
        (_, "frozenset") => simple("frozenset({", "})", "frozenset()"),
        (_, "list") | ("collections", "UserList") => simple("[", "]", "[]"),
        (_, "set") => simple("{", "}", "set()"),
        (_, "tuple") => simple("(", ")", "()"),
        (_, "mappingproxy") => simple("mappingproxy({", "})", "mappingproxy({})"),
        _ => simple("{", "}", "{}"),
    })
}

/// `rich.pretty.traverse`.
pub(crate) fn traverse(object: &Bound<'_, PyAny>, limits: Limits) -> PyResult<Node> {
    let py = object.py();
    let helpers = helpers(py)?;
    let mut walker = Walker {
        py,
        limits,
        helpers,
        visited: HashSet::new(),
    };
    let _nesting = renderable::Nesting::enter()?;
    walker.walk(object, true, 0)
}

// ---------------------------------------------------------------------------
// Text helpers upstream keeps on `Text`

/// `Text.from_ansi(s, style=style)`: decode escape codes, one line at a time,
/// and join the lines with a `"\n"` in the base style.
pub(crate) fn from_ansi(content: &str, style: &str) -> CoreText {
    let lines = AnsiDecoder::new().decode(content);
    let joiner = CoreText::styled("\n", StyleType::Name(style.to_string()));
    joiner.join(&lines)
}

/// `Text.with_indent_guides(indent_size, style=style)`.
pub(crate) fn with_indent_guides(
    text: &CoreText,
    indent_size: usize,
    style: StyleType,
) -> CoreText {
    let indent_size = indent_size.max(1);
    let mut text = text.clone();
    text.expand_tabs(8);
    let indent_line = format!("│{}", " ".repeat(indent_size - 1));
    let styled = |plain: &str| CoreText::styled(plain, style.clone());
    let mut new_lines: Vec<CoreText> = Vec::new();
    let mut blank_lines = 0;
    for line in text.split("\n", false, true) {
        let plain = line.plain();
        let indent = plain.len() - plain.trim_start_matches(' ').len();
        if indent == plain.len() {
            blank_lines += 1;
            continue;
        }
        let (full, remaining) = (indent / indent_size, indent % indent_size);
        let new_indent = format!("{}{}", indent_line.repeat(full), " ".repeat(remaining));
        // Same characters, more bytes: shift the spans past the indent.
        let offsets = super::highlighter::char_offsets(&new_indent);
        let shift = new_indent.len() - indent;
        let remap = |offset: usize| {
            if offset <= indent {
                offsets[offset]
            } else {
                offset + shift
            }
        };
        let mut replaced = line.blank_copy();
        replaced.append(&format!("{new_indent}{}", &plain[indent..]), None);
        for span in line.spans() {
            replaced.stylize(span.style.clone(), remap(span.start), remap(span.end));
        }
        replaced.stylize(style.clone(), 0, new_indent.len());
        for _ in 0..blank_lines {
            new_lines.push(styled(&new_indent));
        }
        blank_lines = 0;
        new_lines.push(replaced);
    }
    for _ in 0..blank_lines {
        new_lines.push(styled(""));
    }
    let mut joiner = text.blank_copy();
    joiner.append("\n", None);
    joiner.join(&new_lines)
}

// ---------------------------------------------------------------------------
// Pretty

/// What a `Pretty` lays out, whether or not its highlighter is Python code.
pub(crate) struct Layout {
    pub(crate) node: Node,
    pub(crate) type_repr: String,
    pub(crate) indent_size: usize,
    pub(crate) justify: rich::Justify,
    pub(crate) overflow: Option<rich::Overflow>,
    pub(crate) no_wrap: Option<bool>,
    pub(crate) indent_guides: bool,
    pub(crate) expand_all: bool,
    pub(crate) margin: usize,
    pub(crate) insert_line: bool,
}

impl Layout {
    /// The text `__rich_console__` yields (after an empty line when
    /// `insert_line` applies, the returned flag).
    fn text(
        &self,
        py: Python<'_>,
        highlight: &Highlight,
        options: &CoreOptions,
        ascii_only: bool,
    ) -> PyResult<(bool, CoreText)> {
        let width = options.max_width as isize - self.margin as isize;
        let pretty = self.node.render(width, self.indent_size, self.expand_all);
        let mut text = from_ansi(&pretty, "pretty");
        text.set_justify(if self.justify == rich::Justify::Default {
            options.justify
        } else {
            self.justify
        });
        text.set_overflow(self.overflow.or(options.overflow));
        text.set_no_wrap(self.no_wrap.or(options.no_wrap));
        let mut text = if text.plain().is_empty() {
            CoreText::styled(
                format!("{}.__repr__ returned empty string", self.type_repr),
                StyleType::Name("dim italic".to_string()),
            )
        } else {
            highlight.apply(py, text)?
        };
        if self.indent_guides && !ascii_only {
            text = with_indent_guides(
                &text,
                self.indent_size,
                StyleType::Name("repr.indent".to_string()),
            );
        }
        Ok((self.insert_line && text.plain().contains('\n'), text))
    }

    fn measure(&self, options: &CoreOptions) -> CoreMeasurement {
        let pretty = self.node.render(
            options.max_width as isize,
            self.indent_size,
            self.expand_all,
        );
        let width = pretty.lines().map(cell_len).max().unwrap_or(0);
        CoreMeasurement::new(width, width)
    }
}

impl Layout {
    /// `Pretty(obj)`'s defaults, for a traversed `node`.
    pub(crate) fn new(node: Node, type_repr: String) -> Layout {
        Layout {
            node,
            type_repr,
            indent_size: 4,
            justify: rich::Justify::Default,
            overflow: None,
            no_wrap: Some(false),
            indent_guides: false,
            expand_all: false,
            margin: 0,
            insert_line: false,
        }
    }
}

/// A `Pretty` with the repr highlighter, shareable as a table cell. `map`
/// renames the highlighter's styles (see `layout::restyle`).
pub(crate) fn shared_repr(
    layout: Layout,
    map: &'static [(&'static str, &'static str)],
) -> std::sync::Arc<dyn Renderable + Send + Sync> {
    std::sync::Arc::new(NativePretty {
        layout,
        highlight: Highlight::Repr,
        map,
    })
}

/// A `Pretty` whose highlighter is Rust's: rendering never calls Python.
struct NativePretty {
    layout: Layout,
    highlight: Highlight,
    map: &'static [(&'static str, &'static str)],
}

impl Renderable for NativePretty {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        // The highlight is Repr or Null, which never fails or calls Python.
        let rendered = Python::attach(|py| {
            self.layout
                .text(py, &self.highlight, options, console.ascii_only())
        });
        let Ok((blank, mut text)) = rendered else {
            return Vec::new();
        };
        if !self.map.is_empty() {
            text = super::layout::restyle(&text, self.map);
        }
        let mut segments = Vec::new();
        if blank {
            segments.push(CoreSegment::line());
        }
        segments.extend(text.rich_render(console, options));
        segments
    }

    fn measure(&self, _console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        self.layout.measure(options)
    }
}

/// A `Pretty` whose highlighter is Python code: rendered through Rich's
/// protocol, so an exception it raises reaches the caller.
#[pyclass(module = "rs_rich.pretty", frozen)]
struct PrettyLayout {
    layout: Layout,
    highlight: Highlight,
}

#[pymethods]
impl PrettyLayout {
    fn __rich_console__(
        &self,
        py: Python<'_>,
        console: &Bound<'_, PyAny>,
        options: PyRef<'_, crate::protocol::ConsoleOptions>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let core = options.to_core()?;
        let ascii_only = !options.base().encoding.to_lowercase().starts_with("utf");
        let _ = console;
        let (blank, text) = self.layout.text(py, &self.highlight, &core, ascii_only)?;
        let mut items = Vec::new();
        if blank {
            items.push(PyString::new(py, "").into_any().unbind());
        }
        items.push(super::highlighter::new_text(py, text)?.into_any().unbind());
        Ok(items)
    }

    fn __rich_measure__(
        &self,
        _console: &Bound<'_, PyAny>,
        options: PyRef<'_, crate::protocol::ConsoleOptions>,
    ) -> PyResult<crate::protocol::Measurement> {
        let core = options.to_core()?;
        Ok(crate::protocol::Measurement::from_core(
            self.layout.measure(&core),
        ))
    }
}

/// The renderable for a layout: native unless the highlighter is Python.
pub(crate) fn renderable_for(
    py: Python<'_>,
    layout: Layout,
    highlight: Highlight,
) -> PyResult<Box<dyn Renderable>> {
    if highlight.is_python() {
        let object = Py::new(py, PrettyLayout { layout, highlight })?;
        Ok(Box::new(PyRenderable::new(object.into_any())))
    } else {
        Ok(Box::new(NativePretty {
            layout,
            highlight,
            map: &[],
        }))
    }
}

/// `rich.pretty.Pretty`: a renderable that pretty-prints any object.
#[pyclass(name = "Pretty", module = "rs_rich.pretty")]
pub(crate) struct Pretty {
    object: Py<PyAny>,
    highlighter: Option<Py<PyAny>>,
    #[pyo3(get, set)]
    indent_size: usize,
    #[pyo3(get, set)]
    justify: Option<String>,
    #[pyo3(get, set)]
    overflow: Option<String>,
    #[pyo3(get, set)]
    no_wrap: Option<bool>,
    #[pyo3(get, set)]
    indent_guides: bool,
    #[pyo3(get, set)]
    max_length: Option<usize>,
    #[pyo3(get, set)]
    max_string: Option<usize>,
    #[pyo3(get, set)]
    max_depth: Option<usize>,
    #[pyo3(get, set)]
    expand_all: bool,
    #[pyo3(get, set)]
    margin: usize,
    #[pyo3(get, set)]
    insert_line: bool,
}

impl Pretty {
    pub(crate) fn limits(&self) -> Limits {
        Limits {
            max_length: self.max_length,
            max_string: self.max_string,
            max_depth: self.max_depth,
        }
    }
}

/// The `repr` of an object's type (for the empty-repr message).
pub(crate) fn type_repr(object: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(object.get_type().repr()?.to_string())
}

impl AsRenderable for Pretty {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let object = self.object.bind(py);
        let layout = Layout {
            node: traverse(object, self.limits())?,
            type_repr: type_repr(object)?,
            indent_size: self.indent_size,
            justify: convert::justify(self.justify.as_deref())?,
            overflow: self
                .overflow
                .as_deref()
                .map(convert::overflow)
                .transpose()?,
            no_wrap: self.no_wrap,
            indent_guides: self.indent_guides,
            expand_all: self.expand_all,
            margin: self.margin,
            insert_line: self.insert_line,
        };
        let highlight = Highlight::from_arg(self.highlighter.as_ref().map(|h| h.bind(py)))?;
        renderable_for(py, layout, highlight)
    }
}

#[pymethods]
impl Pretty {
    #[new]
    #[pyo3(signature = (
        _object, highlighter=None, *, indent_size=4, justify=None, overflow=None, no_wrap=Some(false),
        indent_guides=false, max_length=None, max_string=None, max_depth=None, expand_all=false,
        margin=0, insert_line=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        _object: Py<PyAny>,
        highlighter: Option<Py<PyAny>>,
        indent_size: usize,
        justify: Option<String>,
        overflow: Option<String>,
        no_wrap: Option<bool>,
        indent_guides: bool,
        max_length: Option<usize>,
        max_string: Option<usize>,
        max_depth: Option<usize>,
        expand_all: bool,
        margin: usize,
        insert_line: bool,
    ) -> PyResult<Self> {
        convert::justify(justify.as_deref())?;
        overflow.as_deref().map(convert::overflow).transpose()?;
        Ok(Pretty {
            object: _object,
            highlighter,
            indent_size,
            justify,
            overflow,
            no_wrap,
            indent_guides,
            max_length,
            max_string,
            max_depth,
            expand_all,
            margin,
            insert_line,
        })
    }

    #[getter]
    fn _object(&self, py: Python<'_>) -> Py<PyAny> {
        self.object.clone_ref(py)
    }

    #[getter]
    fn highlighter(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.highlighter {
            Some(highlighter) => Ok(highlighter.clone_ref(py)),
            None => Ok(py
                .get_type::<super::highlighter::ReprHighlighter>()
                .call0()?
                .unbind()),
        }
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.object)?;
        if let Some(highlighter) = &self.highlighter {
            visit.call(highlighter)?;
        }
        Ok(())
    }
}

/// `rich.pretty.Node`: one node of the tree `traverse` builds.
#[pyclass(name = "Node", module = "rs_rich.pretty", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct PyNode {
    pub(crate) inner: Node,
}

#[pymethods]
impl PyNode {
    #[new]
    #[pyo3(signature = (
        key_repr=String::new(), value_repr=String::new(), open_brace=String::new(),
        close_brace=String::new(), empty=String::new(), last=false, is_tuple=false,
        is_namedtuple=false, children=None, key_separator=": ".to_string(),
        separator=", ".to_string()
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        key_repr: String,
        value_repr: String,
        open_brace: String,
        close_brace: String,
        empty: String,
        last: bool,
        is_tuple: bool,
        is_namedtuple: bool,
        children: Option<Vec<PyRef<'_, PyNode>>>,
        key_separator: String,
        separator: String,
    ) -> Self {
        PyNode {
            inner: Node {
                key_repr,
                value_repr,
                open_brace,
                close_brace,
                empty,
                last,
                is_tuple,
                is_namedtuple,
                children: children.map(|c| c.iter().map(|n| n.inner.clone()).collect()),
                key_separator,
                separator,
            },
        }
    }

    #[getter]
    fn key_repr(&self) -> &str {
        &self.inner.key_repr
    }
    #[setter]
    fn set_key_repr(&mut self, value: String) {
        self.inner.key_repr = value;
    }
    #[getter]
    fn value_repr(&self) -> &str {
        &self.inner.value_repr
    }
    #[setter]
    fn set_value_repr(&mut self, value: String) {
        self.inner.value_repr = value;
    }
    #[getter]
    fn open_brace(&self) -> &str {
        &self.inner.open_brace
    }
    #[setter]
    fn set_open_brace(&mut self, value: String) {
        self.inner.open_brace = value;
    }
    #[getter]
    fn close_brace(&self) -> &str {
        &self.inner.close_brace
    }
    #[setter]
    fn set_close_brace(&mut self, value: String) {
        self.inner.close_brace = value;
    }
    #[getter]
    fn empty(&self) -> &str {
        &self.inner.empty
    }
    #[setter]
    fn set_empty(&mut self, value: String) {
        self.inner.empty = value;
    }
    #[getter]
    fn last(&self) -> bool {
        self.inner.last
    }
    #[setter]
    fn set_last(&mut self, value: bool) {
        self.inner.last = value;
    }
    #[getter]
    fn is_tuple(&self) -> bool {
        self.inner.is_tuple
    }
    #[setter]
    fn set_is_tuple(&mut self, value: bool) {
        self.inner.is_tuple = value;
    }
    #[getter]
    fn is_namedtuple(&self) -> bool {
        self.inner.is_namedtuple
    }
    #[setter]
    fn set_is_namedtuple(&mut self, value: bool) {
        self.inner.is_namedtuple = value;
    }
    #[getter]
    fn key_separator(&self) -> &str {
        &self.inner.key_separator
    }
    #[setter]
    fn set_key_separator(&mut self, value: String) {
        self.inner.key_separator = value;
    }
    #[getter]
    fn separator(&self) -> &str {
        &self.inner.separator
    }
    #[setter]
    fn set_separator(&mut self, value: String) {
        self.inner.separator = value;
    }

    /// The children, as copies (assign a list to change them).
    #[getter]
    fn children(&self) -> Option<Vec<PyNode>> {
        self.inner.children.as_ref().map(|children| {
            children
                .iter()
                .map(|child| PyNode {
                    inner: child.clone(),
                })
                .collect()
        })
    }
    #[setter]
    fn set_children(&mut self, value: Option<Vec<PyRef<'_, PyNode>>>) {
        self.inner.children = value.map(|c| c.iter().map(|n| n.inner.clone()).collect());
    }

    fn iter_tokens<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        PyList::new(py, self.inner.token_list())?
            .try_iter()
            .map(Bound::into_any)
    }

    fn check_length(&self, start_length: isize, max_length: isize) -> bool {
        self.inner.check_length(start_length, max_length)
    }

    #[pyo3(signature = (max_width=80, indent_size=4, expand_all=false))]
    fn render(&self, max_width: isize, indent_size: usize, expand_all: bool) -> String {
        self.inner.render(max_width, indent_size, expand_all)
    }

    fn __str__(&self) -> String {
        self.inner.to_repr()
    }

    fn __repr__(&self) -> String {
        let node = &self.inner;
        format!(
            "Node(key_repr={:?}, value_repr={:?}, open_brace={:?}, close_brace={:?}, empty={:?}, \
             last={}, is_tuple={}, is_namedtuple={}, children=..., key_separator={:?}, separator={:?})",
            node.key_repr,
            node.value_repr,
            node.open_brace,
            node.close_brace,
            node.empty,
            py_bool(node.last),
            py_bool(node.is_tuple),
            py_bool(node.is_namedtuple),
            node.key_separator,
            node.separator
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, PyNode>>()
            .is_ok_and(|other| format!("{:?}", other.inner) == format!("{:?}", self.inner))
    }
}

fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

/// `rich.pretty.traverse(_object, max_length=None, max_string=None, max_depth=None)`.
#[pyfunction(name = "traverse")]
#[pyo3(signature = (_object, max_length=None, max_string=None, max_depth=None))]
fn traverse_py(
    _object: &Bound<'_, PyAny>,
    max_length: Option<usize>,
    max_string: Option<usize>,
    max_depth: Option<usize>,
) -> PyResult<PyNode> {
    Ok(PyNode {
        inner: traverse(
            _object,
            Limits {
                max_length,
                max_string,
                max_depth,
            },
        )?,
    })
}

/// `rich.pretty.pretty_repr`: a repr expanded onto lines to fit `max_width`.
#[pyfunction]
#[pyo3(signature = (
    _object, *, max_width=80, indent_size=4, max_length=None, max_string=None, max_depth=None,
    expand_all=false
))]
fn pretty_repr(
    _object: &Bound<'_, PyAny>,
    max_width: isize,
    indent_size: usize,
    max_length: Option<usize>,
    max_string: Option<usize>,
    max_depth: Option<usize>,
    expand_all: bool,
) -> PyResult<String> {
    let node = match _object.extract::<PyRef<'_, PyNode>>() {
        Ok(node) => node.inner.clone(),
        Err(_) => traverse(
            _object,
            Limits {
                max_length,
                max_string,
                max_depth,
            },
        )?,
    };
    Ok(node.render(max_width, indent_size, expand_all))
}

/// The console an optional `console=` argument names (`rs_rich.get_console()`
/// when it is `None`).
pub(crate) fn console_or_global<'py>(
    py: Python<'py>,
    console: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    match console.filter(|c| !c.is_none()) {
        Some(console) => Ok(console.clone()),
        None => py.import("rs_rich")?.call_method0("get_console"),
    }
}

/// `rich.pretty.pprint`: pretty-print an object, with indent guides.
#[pyfunction]
#[pyo3(signature = (
    _object, *, console=None, indent_guides=true, max_length=None, max_string=None,
    max_depth=None, expand_all=false
))]
#[allow(clippy::too_many_arguments)]
fn pprint(
    py: Python<'_>,
    _object: Py<PyAny>,
    console: Option<&Bound<'_, PyAny>>,
    indent_guides: bool,
    max_length: Option<usize>,
    max_string: Option<usize>,
    max_depth: Option<usize>,
    expand_all: bool,
) -> PyResult<()> {
    let console = console_or_global(py, console)?;
    let pretty = Pretty {
        object: _object,
        highlighter: None,
        indent_size: 4,
        justify: None,
        overflow: Some("ignore".to_string()),
        no_wrap: Some(false),
        indent_guides,
        max_length,
        max_string,
        max_depth,
        expand_all,
        margin: 0,
        insert_line: false,
    };
    let kwargs = PyDict::new(py);
    kwargs.set_item("soft_wrap", true)?;
    console.call_method("print", (Py::new(py, pretty)?,), Some(&kwargs))?;
    Ok(())
}

/// `isinstance(value, RichRenderable)`: one of the bindings' renderables, or
/// an object with `__rich__` or `__rich_console__`.
fn is_rich_renderable(value: &Bound<'_, PyAny>) -> bool {
    renderable::is_registered(value)
        || value.hasattr("__rich__").unwrap_or(false)
        || value.hasattr("__rich_console__").unwrap_or(false)
}

/// `rich.pretty.install`: pretty-print values in the REPL (`sys.displayhook`).
#[pyfunction]
#[pyo3(signature = (
    console=None, overflow="ignore".to_string(), crop=false, indent_guides=false, max_length=None,
    max_string=None, max_depth=None, expand_all=false
))]
#[allow(clippy::too_many_arguments)]
fn pretty_install(
    py: Python<'_>,
    console: Option<&Bound<'_, PyAny>>,
    overflow: Option<String>,
    crop: bool,
    indent_guides: bool,
    max_length: Option<usize>,
    max_string: Option<usize>,
    max_depth: Option<usize>,
    expand_all: bool,
) -> PyResult<()> {
    overflow.as_deref().map(convert::overflow).transpose()?;
    let console = console_or_global(py, console)?.unbind();
    let hook = pyo3::types::PyCFunction::new_closure(
        py,
        Some(c"display_hook"),
        Some(c"Replacement sys.displayhook which prettifies objects with rs_rich."),
        move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
            let py = args.py();
            let value = args.get_item(0)?;
            if value.is_none() {
                return Ok(());
            }
            let builtins = py.import("builtins")?;
            builtins.setattr("_", py.None())?;
            let printed: Py<PyAny> = if is_rich_renderable(&value) {
                value.clone().unbind()
            } else {
                Py::new(
                    py,
                    Pretty {
                        object: value.clone().unbind(),
                        highlighter: None,
                        indent_size: 4,
                        justify: None,
                        overflow: overflow.clone(),
                        no_wrap: Some(false),
                        indent_guides,
                        max_length,
                        max_string,
                        max_depth,
                        expand_all,
                        margin: 0,
                        insert_line: false,
                    },
                )?
                .into_any()
            };
            let kwargs = PyDict::new(py);
            kwargs.set_item("crop", crop)?;
            console
                .bind(py)
                .call_method("print", (printed,), Some(&kwargs))?;
            builtins.setattr("_", value)?;
            Ok(())
        },
    )?;
    py.import("sys")?.setattr("displayhook", hook)?;
    Ok(())
}

/// What `Console.print` renders for an expandable object:
/// `Pretty(obj, highlighter=...)` with the repr or the null highlighter.
pub(crate) fn for_print(
    value: &Bound<'_, PyAny>,
    highlight: bool,
) -> PyResult<Box<dyn Renderable>> {
    let py = value.py();
    let layout = Layout {
        node: traverse(value, Limits::default())?,
        type_repr: type_repr(value)?,
        indent_size: 4,
        justify: rich::Justify::Default,
        overflow: None,
        no_wrap: Some(false),
        indent_guides: false,
        expand_all: false,
        margin: 0,
        insert_line: false,
    };
    let highlight = if highlight {
        Highlight::Repr
    } else {
        Highlight::Null
    };
    renderable_for(py, layout, highlight)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Pretty>(m)?;
    m.add_class::<PyNode>()?;
    m.add_function(wrap_pyfunction!(traverse_py, m)?)?;
    m.add_function(wrap_pyfunction!(pretty_repr, m)?)?;
    m.add_function(wrap_pyfunction!(pprint, m)?)?;
    m.add_function(wrap_pyfunction!(pretty_install, m)?)?;
    Ok(())
}
