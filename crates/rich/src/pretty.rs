//! Port of upstream rich/pretty.py.
//!
//! Upstream pretty-prints a *Python* object: `traverse` walks it into a tree
//! of [`Node`]s, and [`Node::render`] lays the tree out to fit a width,
//! expanding containers onto their own lines only where they do not fit.
//! Rust has no runtime reflection, so the object is described by a
//! [`PrettyValue`] (Python's `None`, `bool`, `int`, `float`, `str`, `bytes`,
//! `list`, `tuple`, `set`, `frozenset`, `dict`, and objects with a
//! dataclass, namedtuple or `__rich_repr__` repr). From there, `traverse`,
//! `Node`, `_Line` and [`pretty_repr`] follow upstream line for line, and
//! [`Pretty::from_value`] renders byte for byte what upstream's `Pretty`
//! renders for the same object (golden `parity_docs.tsv`).
//!
//! [`Pretty::new`] and [`Pretty::compact`] are the older Rust-native form: they
//! format any value with its [`Debug`] implementation (`{:#?}` / `{:?}`) and
//! colour that with the [`ReprHighlighter`]. Debug spellings differ from
//! Python's repr (`true`, `"a"`), so that form is not parity (DIVERGENCES
//! §19).

use std::fmt::Debug;

use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions, Justify, Overflow};
use crate::highlighter::ReprHighlighter;
use crate::measure::Measurement;
use crate::protocol::{Highlighter, Renderable};
use crate::pyformat::float_repr;
use crate::segment::Segment;
use crate::text::Text;

/// A Python value, as `traverse` sees it. Mirrors the types upstream's
/// `pretty.py` knows how to expand (`_BRACES`, dataclasses, namedtuples and
/// `__rich_repr__`); anything else is an atom with its own repr
/// ([`PrettyValue::Repr`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PrettyValue {
    /// `None`.
    None,
    /// `True` / `False`.
    Bool(bool),
    /// An `int`. Larger integers can be given as [`PrettyValue::Repr`].
    Int(i128),
    /// A `float`, written as Python's `repr` writes it.
    Float(f64),
    /// A `str`, written as Python's `repr` writes it (and cut by `max_string`).
    Str(String),
    /// A `bytes`, written as Python's `repr` writes it (and cut by `max_string`).
    Bytes(Vec<u8>),
    /// Any other atom: the text is its repr, used as it is.
    Repr(String),
    /// A `list`.
    List(Vec<PrettyValue>),
    /// A `tuple` (a tuple of one is written `(x,)`).
    Tuple(Vec<PrettyValue>),
    /// A `set`, its items in iteration order.
    Set(Vec<PrettyValue>),
    /// A `frozenset`, its items in iteration order.
    FrozenSet(Vec<PrettyValue>),
    /// A `dict`, its items in insertion order.
    Dict(Vec<(PrettyValue, PrettyValue)>),
    /// An object with a structured repr: a dataclass, a namedtuple or a
    /// `__rich_repr__`.
    Object(PrettyObject),
}

/// Which of upstream's structured reprs an object has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectRepr {
    /// `__rich_repr__`: `Name(a, b=1)`, or `<Name a b=1>` when `angular`.
    RichRepr { angular: bool },
    /// A dataclass with its generated repr: `Name(a=1)`.
    Dataclass,
    /// A namedtuple with its default repr: `Name(a=1)`.
    NamedTuple,
}

/// An object with a structured repr: a class name and its fields, each
/// positional (`None` key) or named.
#[derive(Clone, Debug, PartialEq)]
pub struct PrettyObject {
    pub name: String,
    pub fields: Vec<(Option<String>, PrettyValue)>,
    pub kind: ObjectRepr,
}

impl PrettyObject {
    /// An object with the given class name, structured repr and fields.
    pub fn new(
        name: impl Into<String>,
        kind: ObjectRepr,
        fields: Vec<(Option<String>, PrettyValue)>,
    ) -> Self {
        PrettyObject {
            name: name.into(),
            fields,
            kind,
        }
    }
}

impl From<&serde_json::Value> for PrettyValue {
    /// The value `json.loads` makes of a JSON document: `null` is `None`,
    /// objects are `dict`s in document order, arrays are `list`s, integers
    /// are `int`s and other numbers `float`s.
    fn from(value: &serde_json::Value) -> Self {
        use serde_json::Value;
        match value {
            Value::Null => PrettyValue::None,
            Value::Bool(b) => PrettyValue::Bool(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    PrettyValue::Int(i128::from(i))
                } else if let Some(u) = n.as_u64() {
                    PrettyValue::Int(i128::from(u))
                } else {
                    PrettyValue::Float(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            Value::String(s) => PrettyValue::Str(s.clone()),
            Value::Array(items) => PrettyValue::List(items.iter().map(Into::into).collect()),
            Value::Object(map) => PrettyValue::Dict(
                map.iter()
                    .map(|(k, v)| (PrettyValue::Str(k.clone()), v.into()))
                    .collect(),
            ),
        }
    }
}

/// Python's `str.isprintable` for one character, as `repr` uses it.
///
/// Covers the control (Cc), format (Cf), separator (Zs other than the space,
/// Zl, Zp) and private-use (Co) categories. Unassigned code points (Cn)
/// count as printable here; Python escapes those too.
fn is_printable(ch: char) -> bool {
    let c = ch as u32;
    !matches!(c,
        0x00..=0x1F | 0x7F..=0xA0 | 0xAD
        | 0x600..=0x605 | 0x61C | 0x6DD | 0x70F | 0x890..=0x891 | 0x8E2
        | 0x1680 | 0x180E
        | 0x2000..=0x200F | 0x2028..=0x202F | 0x205F..=0x2064 | 0x2066..=0x206F
        | 0x3000 | 0xE000..=0xF8FF | 0xFEFF | 0xFFF9..=0xFFFB
        | 0x110BD | 0x110CD | 0x13430..=0x1343F | 0x1BCA0..=0x1BCA3
        | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F
        | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

/// Python's `repr()` of a `str`.
pub fn str_repr(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch if !is_printable(ch) => {
                let c = ch as u32;
                if c < 0x100 {
                    out.push_str(&format!("\\x{c:02x}"));
                } else if c < 0x10000 {
                    out.push_str(&format!("\\u{c:04x}"));
                } else {
                    out.push_str(&format!("\\U{c:08x}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// Python's `repr()` of a `bytes`.
pub fn bytes_repr(value: &[u8]) -> String {
    let quote = if value.contains(&b'\'') && !value.contains(&b'"') {
        b'"'
    } else {
        b'\''
    };
    let mut out = String::with_capacity(value.len() + 3);
    out.push('b');
    out.push(quote as char);
    for &byte in value {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b if b == quote => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7E => out.push(byte as char),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push(quote as char);
    out
}

/// A node in a repr tree. May be atomic or a container. Port of
/// `rich.pretty.Node`.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub key_repr: String,
    pub value_repr: String,
    pub open_brace: String,
    pub close_brace: String,
    pub empty: String,
    pub last: bool,
    pub is_tuple: bool,
    pub is_namedtuple: bool,
    pub children: Option<Vec<Node>>,
    pub key_separator: String,
    pub separator: String,
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
    fn atom(value_repr: impl Into<String>) -> Node {
        Node {
            value_repr: value_repr.into(),
            ..Node::default()
        }
    }

    /// Visit the tokens of this node in order; stops early, returning
    /// `false`, when `visit` does. Port of `Node.iter_tokens`.
    fn visit_tokens(&self, visit: &mut dyn FnMut(&str) -> bool) -> bool {
        if !self.key_repr.is_empty() && !(visit(&self.key_repr) && visit(&self.key_separator)) {
            return false;
        }
        if !self.value_repr.is_empty() {
            return visit(&self.value_repr);
        }
        if let Some(children) = &self.children {
            if children.is_empty() {
                return visit(&self.empty);
            }
            if !visit(&self.open_brace) {
                return false;
            }
            if self.is_tuple && !self.is_namedtuple && children.len() == 1 {
                if !(children[0].visit_tokens(visit) && visit(",")) {
                    return false;
                }
            } else {
                for child in children {
                    if !child.visit_tokens(visit) {
                        return false;
                    }
                    if !child.last && !visit(&self.separator) {
                        return false;
                    }
                }
            }
            return visit(&self.close_brace);
        }
        true
    }

    /// Whether the node fits in `max_length` cells after `start_length`.
    /// Port of `Node.check_length`.
    pub fn check_length(&self, start_length: usize, max_length: usize) -> bool {
        let mut total_length = start_length;
        self.visit_tokens(&mut |token| {
            total_length += cell_len(token);
            total_length <= max_length
        })
    }

    /// Render the node to a pretty repr, expanding containers that do not
    /// fit in `max_width` (or all of them with `expand_all`). Port of
    /// `Node.render`.
    pub fn render(&self, max_width: usize, indent_size: usize, expand_all: bool) -> String {
        let mut lines = vec![Line {
            node: Some(self),
            ..Line::default()
        }];
        let mut line_no = 0;
        while line_no < lines.len() {
            let line = &lines[line_no];
            if line.expandable() && (expand_all || !line.check_length(max_width)) {
                let expanded = line.expand(indent_size);
                lines.splice(line_no..=line_no, expanded);
            }
            line_no += 1;
        }
        lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = Ok(());
        self.visit_tokens(&mut |token| {
            result = f.write_str(token);
            result.is_ok()
        });
        result
    }
}

/// A line in repr output. Port of `rich.pretty._Line`.
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

    fn check_length(&self, max_length: usize) -> bool {
        let start_length = self.whitespace.len() + cell_len(&self.text) + cell_len(&self.suffix);
        self.node
            .expect("an expandable line has a node")
            .check_length(start_length, max_length)
    }

    fn expand(&self, indent_size: usize) -> Vec<Line<'a>> {
        let node = self.node.expect("an expandable line has a node");
        let children = node.children.as_ref().expect("expandable");
        let whitespace = self.whitespace.clone();
        let text = if node.key_repr.is_empty() {
            node.open_brace.clone()
        } else {
            format!("{}{}{}", node.key_repr, node.key_separator, node.open_brace)
        };
        let mut lines = vec![Line {
            text,
            whitespace: whitespace.clone(),
            ..Line::default()
        }];
        let child_whitespace = format!("{whitespace}{}", " ".repeat(indent_size));
        let tuple_of_one = node.is_tuple && children.len() == 1;
        let count = children.len();
        for (index, child) in children.iter().enumerate() {
            let last = index + 1 == count;
            lines.push(Line {
                node: Some(child),
                whitespace: child_whitespace.clone(),
                suffix: if tuple_of_one {
                    ",".to_string()
                } else {
                    node.separator.clone()
                },
                last: last && !tuple_of_one,
                ..Line::default()
            });
        }
        lines.push(Line {
            text: node.close_brace.clone(),
            whitespace,
            suffix: self.suffix.clone(),
            last: self.last,
            ..Line::default()
        });
        lines
    }
}

impl std::fmt::Display for Line<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.whitespace, self.text)?;
        if let Some(node) = self.node {
            write!(f, "{node}")?;
        }
        if !self.last {
            f.write_str(self.suffix.trim_end())?;
        }
        Ok(())
    }
}

/// Limits for [`traverse`] and [`pretty_repr`]; `None` disables each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReprLimits {
    /// Maximum length of containers before abbreviating (`... +N`).
    pub max_length: Option<usize>,
    /// Maximum length of strings and bytes before truncating (`'ab'+3`).
    pub max_string: Option<usize>,
    /// Maximum depth of nested data structures (`[...]`).
    pub max_depth: Option<usize>,
}

/// Python's `repr()` of a value (no limits, on one line).
pub fn py_repr(value: &PrettyValue) -> String {
    traverse(value, ReprLimits::default()).to_string()
}

/// `to_repr`: the repr of an atom, cut to `max_string` for `str` and `bytes`.
fn to_repr(value: &PrettyValue, max_string: Option<usize>) -> String {
    match value {
        PrettyValue::Str(s) => match max_string {
            Some(max) if s.chars().count() > max => {
                let truncated = s.chars().count() - max;
                let head: String = s.chars().take(max).collect();
                format!("{}+{truncated}", str_repr(&head))
            }
            _ => str_repr(s),
        },
        PrettyValue::Bytes(b) => match max_string {
            Some(max) if b.len() > max => {
                format!("{}+{}", bytes_repr(&b[..max]), b.len() - max)
            }
            _ => bytes_repr(b),
        },
        PrettyValue::None => "None".to_string(),
        PrettyValue::Bool(true) => "True".to_string(),
        PrettyValue::Bool(false) => "False".to_string(),
        PrettyValue::Int(i) => i.to_string(),
        PrettyValue::Float(f) => float_repr(*f),
        PrettyValue::Repr(r) => r.clone(),
        other => py_repr(other),
    }
}

/// Walk a value into a tree of [`Node`]s. Port of `rich.pretty.traverse`.
pub fn traverse(value: &PrettyValue, limits: ReprLimits) -> Node {
    traverse_node(value, true, 0, limits)
}

fn traverse_node(value: &PrettyValue, root: bool, depth: usize, limits: ReprLimits) -> Node {
    let reached_max_depth = limits.max_depth.is_some_and(|max| depth >= max);
    let mut node = match value {
        PrettyValue::Object(object) => traverse_object(object, root, depth, limits),
        PrettyValue::List(items)
        | PrettyValue::Tuple(items)
        | PrettyValue::Set(items)
        | PrettyValue::FrozenSet(items) => {
            let (open_brace, close_brace, empty) = match value {
                PrettyValue::List(_) => ("[", "]", "[]"),
                PrettyValue::Tuple(_) => ("(", ")", "()"),
                PrettyValue::Set(_) => ("{", "}", "set()"),
                _ => ("frozenset({", "})", "frozenset()"),
            };
            if reached_max_depth {
                Node::atom(format!("{open_brace}...{close_brace}"))
            } else if items.is_empty() {
                Node {
                    empty: empty.to_string(),
                    children: Some(Vec::new()),
                    last: root,
                    ..Node::default()
                }
            } else {
                let last_item_index = items.len() - 1;
                let shown = limits.max_length.unwrap_or(usize::MAX);
                let mut children: Vec<Node> = items
                    .iter()
                    .take(shown)
                    .enumerate()
                    .map(|(index, child)| {
                        let mut child_node = traverse_node(child, false, depth + 1, limits);
                        child_node.last = index == last_item_index;
                        child_node
                    })
                    .collect();
                push_more(&mut children, items.len(), limits.max_length);
                Node {
                    open_brace: open_brace.to_string(),
                    close_brace: close_brace.to_string(),
                    children: Some(children),
                    last: root,
                    ..Node::default()
                }
            }
        }
        PrettyValue::Dict(items) => {
            if reached_max_depth {
                Node::atom("{...}")
            } else if items.is_empty() {
                Node {
                    empty: "{}".to_string(),
                    children: Some(Vec::new()),
                    last: root,
                    ..Node::default()
                }
            } else {
                let last_item_index = items.len() - 1;
                let shown = limits.max_length.unwrap_or(usize::MAX);
                let mut children: Vec<Node> = items
                    .iter()
                    .take(shown)
                    .enumerate()
                    .map(|(index, (key, child))| {
                        let mut child_node = traverse_node(child, false, depth + 1, limits);
                        child_node.key_repr = to_repr(key, limits.max_string);
                        child_node.last = index == last_item_index;
                        child_node
                    })
                    .collect();
                push_more(&mut children, items.len(), limits.max_length);
                Node {
                    open_brace: "{".to_string(),
                    close_brace: "}".to_string(),
                    children: Some(children),
                    last: root,
                    ..Node::default()
                }
            }
        }
        atom => Node {
            value_repr: to_repr(atom, limits.max_string),
            last: root,
            ..Node::default()
        },
    };
    node.is_tuple = matches!(value, PrettyValue::Tuple(_));
    node.is_namedtuple = matches!(
        value,
        PrettyValue::Object(PrettyObject {
            kind: ObjectRepr::NamedTuple,
            ..
        })
    );
    node
}

/// The `... +N` node after a container cut to `max_length`.
fn push_more(children: &mut Vec<Node>, num_items: usize, max_length: Option<usize>) {
    if let Some(max_length) = max_length {
        if num_items > max_length {
            children.push(Node {
                value_repr: format!("... +{}", num_items - max_length),
                last: true,
                ..Node::default()
            });
        }
    }
}

fn traverse_object(object: &PrettyObject, root: bool, depth: usize, limits: ReprLimits) -> Node {
    let reached_max_depth = limits.max_depth.is_some_and(|max| depth >= max);
    let name = &object.name;
    let children = || -> Vec<Node> {
        let count = object.fields.len();
        object
            .fields
            .iter()
            .enumerate()
            .map(|(index, (key, child))| {
                let mut child_node = traverse_node(child, false, depth + 1, limits);
                child_node.last = index + 1 == count;
                if let Some(key) = key {
                    child_node.key_repr = key.clone();
                    child_node.key_separator = "=".to_string();
                }
                child_node
            })
            .collect()
    };
    match object.kind {
        ObjectRepr::RichRepr { angular } => {
            if object.fields.is_empty() {
                Node {
                    value_repr: if angular {
                        format!("<{name}>")
                    } else {
                        format!("{name}()")
                    },
                    children: Some(Vec::new()),
                    last: root,
                    ..Node::default()
                }
            } else if reached_max_depth {
                Node::atom(if angular {
                    format!("<{name}...>")
                } else {
                    format!("{name}(...)")
                })
            } else if angular {
                Node {
                    open_brace: format!("<{name} "),
                    close_brace: ">".to_string(),
                    children: Some(children()),
                    last: root,
                    separator: " ".to_string(),
                    ..Node::default()
                }
            } else {
                Node {
                    open_brace: format!("{name}("),
                    close_brace: ")".to_string(),
                    children: Some(children()),
                    last: root,
                    ..Node::default()
                }
            }
        }
        ObjectRepr::Dataclass | ObjectRepr::NamedTuple => {
            if reached_max_depth {
                Node::atom(format!("{name}(...)"))
            } else {
                Node {
                    open_brace: format!("{name}("),
                    close_brace: ")".to_string(),
                    children: Some(children()),
                    // Upstream's namedtuple node leaves `last` unset.
                    last: root && object.kind == ObjectRepr::Dataclass,
                    empty: format!("{name}()"),
                    ..Node::default()
                }
            }
        }
    }
}

/// A repr of `value`, expanded onto several lines to fit `max_width`. Port
/// of `rich.pretty.pretty_repr`.
pub fn pretty_repr(
    value: &PrettyValue,
    max_width: usize,
    indent_size: usize,
    limits: ReprLimits,
    expand_all: bool,
) -> String {
    traverse(value, limits).render(max_width, indent_size, expand_all)
}

enum Source {
    /// A Rust value's `Debug` output (the Rust-native form).
    Debug(Text),
    /// A Python value, laid out as upstream does.
    Value(PrettyValue),
}

/// A renderable that pretty prints a value. Mirrors `rich.pretty.Pretty`.
pub struct Pretty {
    source: Source,
    highlighter: Box<dyn Highlighter + Send + Sync>,
    indent_size: usize,
    justify: Option<Justify>,
    overflow: Option<Overflow>,
    no_wrap: Option<bool>,
    indent_guides: bool,
    limits: ReprLimits,
    expand_all: bool,
    margin: usize,
    insert_line: bool,
}

impl Pretty {
    /// Pretty-print `value` (`{:#?}`, multi-line and indented) and highlight
    /// it. Not upstream's layout; see [`Pretty::from_value`].
    pub fn new(value: &impl Debug) -> Self {
        Pretty::from_debug_string(format!("{value:#?}"))
    }

    /// Format `value` compactly on one line (`{:?}`) and highlight it.
    pub fn compact(value: &impl Debug) -> Self {
        Pretty::from_debug_string(format!("{value:?}"))
    }

    fn from_debug_string(rendered: String) -> Self {
        let mut text = Text::new(rendered);
        ReprHighlighter::new().highlight(&mut text);
        Pretty::with_source(Source::Debug(text))
    }

    /// Pretty-print a Python value as upstream's `Pretty(obj)` does.
    pub fn from_value(value: PrettyValue) -> Self {
        Pretty::with_source(Source::Value(value))
    }

    fn with_source(source: Source) -> Self {
        Pretty {
            source,
            highlighter: Box::new(ReprHighlighter::new()),
            indent_size: 4,
            justify: None,
            overflow: None,
            no_wrap: Some(false),
            indent_guides: false,
            limits: ReprLimits::default(),
            expand_all: false,
            margin: 0,
            insert_line: false,
        }
    }

    /// The highlighter applied to the repr (upstream `highlighter`, default
    /// [`ReprHighlighter`]).
    pub fn highlighter(mut self, highlighter: impl Highlighter + Send + Sync + 'static) -> Self {
        self.highlighter = Box::new(highlighter);
        self
    }

    /// Spaces per indent level (upstream `indent_size`, default 4).
    pub fn indent_size(mut self, indent_size: usize) -> Self {
        self.indent_size = indent_size;
        self
    }

    /// Justify method, or `None` for the console's (upstream `justify`).
    pub fn justify(mut self, justify: Option<Justify>) -> Self {
        self.justify = justify;
        self
    }

    /// Overflow method, or `None` for the console's (upstream `overflow`).
    pub fn overflow(mut self, overflow: Option<Overflow>) -> Self {
        self.overflow = overflow;
        self
    }

    /// Disable wrapping, or `None` for the console's (upstream `no_wrap`,
    /// default `False`).
    pub fn no_wrap(mut self, no_wrap: Option<bool>) -> Self {
        self.no_wrap = no_wrap;
        self
    }

    /// Draw indent guides (upstream `indent_guides`).
    pub fn indent_guides(mut self, indent_guides: bool) -> Self {
        self.indent_guides = indent_guides;
        self
    }

    /// Abbreviate containers longer than this (upstream `max_length`).
    pub fn max_length(mut self, max_length: Option<usize>) -> Self {
        self.limits.max_length = max_length;
        self
    }

    /// Truncate strings longer than this (upstream `max_string`).
    pub fn max_string(mut self, max_string: Option<usize>) -> Self {
        self.limits.max_string = max_string;
        self
    }

    /// Stop at this depth of nesting (upstream `max_depth`).
    pub fn max_depth(mut self, max_depth: Option<usize>) -> Self {
        self.limits.max_depth = max_depth;
        self
    }

    /// Expand every container (upstream `expand_all`).
    pub fn expand_all(mut self, expand_all: bool) -> Self {
        self.expand_all = expand_all;
        self
    }

    /// Cells taken off the width, so containers expand sooner (upstream
    /// `margin`).
    pub fn margin(mut self, margin: usize) -> Self {
        self.margin = margin;
        self
    }

    /// Start with a blank line when the output has several lines (upstream
    /// `insert_line`).
    pub fn insert_line(mut self, insert_line: bool) -> Self {
        self.insert_line = insert_line;
        self
    }

    fn repr_at(&self, value: &PrettyValue, max_width: usize) -> String {
        pretty_repr(
            value,
            max_width,
            self.indent_size,
            self.limits,
            self.expand_all,
        )
    }
}

impl Renderable for Pretty {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let value = match &self.source {
            Source::Debug(text) => return text.rich_render(console, options),
            Source::Value(value) => value,
        };
        let pretty_str = self.repr_at(value, options.max_width.saturating_sub(self.margin));
        let mut pretty_text = Text::from_ansi(&pretty_str, "pretty");
        pretty_text.set_justify(self.justify.unwrap_or(options.justify));
        pretty_text.set_overflow(self.overflow.or(options.overflow));
        pretty_text.set_no_wrap(Some(self.no_wrap.or(options.no_wrap).unwrap_or(false)));
        if pretty_text.is_empty() {
            // Upstream names the object's type here, which a `PrettyValue`
            // does not carry.
            pretty_text = Text::styled("__repr__ returned empty string", "dim italic");
        } else {
            self.highlighter.highlight(&mut pretty_text);
        }
        if self.indent_guides && !options.ascii_only() {
            pretty_text =
                pretty_text.with_indent_guides(Some(self.indent_size), "│", "repr.indent");
        }
        let mut segments = Vec::new();
        if self.insert_line && pretty_text.plain().contains('\n') {
            segments.push(Segment::line());
        }
        segments.extend(pretty_text.rich_render(console, options));
        segments
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let value = match &self.source {
            Source::Debug(text) => return text.measure(console, options),
            Source::Value(value) => value,
        };
        let pretty_str = self.repr_at(value, options.max_width);
        let text_width = pretty_str.lines().map(cell_len).max().unwrap_or(0);
        Measurement {
            minimum: text_width,
            maximum: text_width,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(pretty: &Pretty) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .build()
            .render_to_string(pretty)
    }

    #[test]
    fn highlights_numbers_in_debug_output() {
        // Numbers get the repr.number style (bold cyan → 1;36).
        let out = render(&Pretty::compact(&vec![1, 2, 3]));
        assert!(out.contains("\x1b[1;36m1\x1b[0m"), "got {out:?}");
        assert!(out.contains('3'));
    }

    #[test]
    fn highlights_string_in_debug_output() {
        // A quoted string gets the repr.str style (green → 32).
        let out = render(&Pretty::compact(&"hello"));
        assert!(out.contains("\x1b[32m\"hello\"\x1b[0m"), "got {out:?}");
    }

    #[test]
    fn pretty_multiline_preserves_structure() {
        // `{:#?}` on a nested collection spans multiple indented lines.
        let out = render(&Pretty::new(&vec![vec![1, 2], vec![3, 4]]));
        assert!(out.contains('\n'), "expected pretty multi-line layout");
        assert!(out.contains("\x1b[1;36m4\x1b[0m"), "numbers highlighted");
    }

    #[test]
    fn python_reprs() {
        use PrettyValue as V;
        assert_eq!(str_repr("it's"), "\"it's\"");
        assert_eq!(
            str_repr("a'\"b\n\x01é\u{2028}"),
            "'a\\'\"b\\n\\x01é\\u2028'"
        );
        assert_eq!(bytes_repr(b"a\x00'"), "b\"a\\x00'\"");
        let value = V::List(vec![
            V::Tuple(vec![V::Int(1)]),
            V::Set(vec![]),
            V::Dict(vec![(V::Str("k".into()), V::Float(1e16))]),
            V::None,
            V::Bool(true),
        ]);
        assert_eq!(py_repr(&value), "[(1,), set(), {'k': 1e+16}, None, True]");
    }

    #[test]
    fn expands_only_what_does_not_fit() {
        use PrettyValue as V;
        let value = V::Dict(vec![
            (V::Str("a".into()), V::List((0..3).map(V::Int).collect())),
            (V::Str("b".into()), V::Tuple(vec![V::Str("x".repeat(30))])),
        ]);
        let out = pretty_repr(&value, 20, 4, ReprLimits::default(), false);
        assert_eq!(
            out,
            "{\n    'a': [0, 1, 2],\n    'b': (\n        'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx',\n    )\n}"
        );
    }
}
