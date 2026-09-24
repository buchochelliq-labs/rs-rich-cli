//! Structured data: a format-neutral document tree, parsers that fill it, and
//! views that render it.
//!
//! Every format parses into the same [`Node`] tree — JSON, INI and dotenv with
//! the `data` feature; YAML, TOML and XML with their own features — so one set
//! of views covers them all:
//!
//! - [`Explorer`]: a width-aware tree (or table) with folding and limits
//! - [`TableView`]: records as a table ([`table`], [`print_table`])
//! - [`FlatView`], [`flatten`] / [`unflatten`]: `path = value` leaves
//! - [`SearchResults`], [`search`]: key / path / value search with highlights
//! - [`Selectors`]: pluggable selection expressions (JSONPath with `jsonpath`)
//! - [`DiffView`], [`diff`]: leaf-level differences
//! - [`Redaction`]: masking secrets before display
//! - [`ConfigFileView`]: INI and dotenv files as `section | key | value`
//!
//! Serde values join in through [`from_serialize`] and the zero-boilerplate
//! helpers [`print_json`], [`print_table`] and [`print_tree`].
//!
//! ```
//! use rich::Console;
//! use rich_ext::data::{parse, Explorer, Format};
//!
//! let node = parse(Format::Json, r#"{"name": "demo", "ports": [80, 443]}"#).unwrap();
//! let console = Console::builder().width(40).build();
//! let out = console.render_export(&Explorer::new(&node));
//! assert_eq!(
//!     out,
//!     "{…} 2 keys\n├── name: \"demo\"\n└── ports\n    ├── [0]: 80\n    └── [1]: 443\n"
//! );
//! ```
//!
//! Styles resolve through the console theme. Values reuse core's JSON names
//! (`json.key`, `json.str`, `json.number`, `json.bool_true`, `json.bool_false`,
//! `json.null`) so a JSON theme applies here too; this module's own names and
//! their fallbacks are listed in [`DATA_STYLES`].

use std::borrow::Cow;
use std::fmt;

use rich::{Console, Style};

use crate::event::theme_style;

mod config;
mod diff;
mod dotenv;
mod explorer;
mod flatten;
mod helpers;
mod ini;
mod json;
mod redact;
mod search;
pub mod select;
mod ser;
mod table;

#[cfg(feature = "toml")]
mod toml_doc;
#[cfg(feature = "xml")]
mod xml;
#[cfg(feature = "yaml")]
mod yaml;

pub use config::ConfigFileView;
pub use diff::{diff, Change, ChangeKind, DiffView};
pub use explorer::{Explorer, View};
pub use flatten::{flatten, unflatten, FlatView, UnflattenError};
pub use helpers::{
    json, print_json, print_json_to, print_table, print_table_to, print_tree, print_tree_to, table,
    tree,
};
pub use redact::SECRET_KEYS;
pub use redact::{Redaction, Redactor};
pub use search::{search, MatchKind, SearchMatch, SearchQuery, SearchResults};
#[cfg(feature = "jsonpath")]
pub use select::{JsonPath, JsonPathSelector};
pub use select::{SelectError, Selector, SelectorBackend, Selectors};
pub use table::{TableOptions, TableView};

/// How deeply YAML and XML documents may nest. Parsing is iterative, but
/// dropping, cloning and converting a [`Node`] recurse, so an adversarial
/// document must not build a tree deep enough to overflow the stack. (serde
/// JSON stops at 128 levels and the `toml` parser has its own limit.)
#[cfg(any(feature = "yaml", feature = "xml"))]
pub(crate) const MAX_DEPTH: usize = 512;

/// Style names this module uses beyond core's `json.*` names, with the
/// fallback each gets when the console theme does not define it.
pub const DATA_STYLES: &[(&str, &str)] = &[
    ("data.match", "bold reverse yellow"),
    ("data.anchor", "dim cyan"),
    ("data.alias", "dim cyan"),
    ("data.attribute", "not italic yellow"),
    ("data.index", "dim"),
    ("data.comment", "dim"),
    ("data.path", "dim"),
    ("data.type", "dim italic"),
    ("data.summary", "dim"),
    ("data.null", "dim"),
    ("data.datetime", "magenta"),
    ("data.section", "bold"),
    ("data.added", "green"),
    ("data.removed", "red"),
    ("data.changed", "yellow"),
];

/// Core's JSON style names with the defaults `rich::Json` uses.
const JSON_STYLES: &[(&str, &str)] = &[
    ("json.key", "bold blue"),
    ("json.str", "not bold not italic green"),
    ("json.number", "bold not italic cyan"),
    ("json.bool_true", "italic bright_green"),
    ("json.bool_false", "italic bright_red"),
    ("json.null", "italic magenta"),
];

/// The theme style for one of this module's names (or a `json.*` name).
pub(crate) fn style(console: &Console, key: &str) -> Style {
    let fallback = DATA_STYLES
        .iter()
        .chain(JSON_STYLES)
        .find(|(name, _)| *name == key)
        .map_or("", |(_, spec)| *spec);
    theme_style(console, key, fallback)
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// A place in the source text. Both fields are 1-based; the column counts
/// characters, not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Position { line, column }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// What an XML-derived node was in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum XmlKind {
    /// An element (`<a>…</a>`).
    Element,
    /// An attribute, stored under an `@name` key.
    Attribute,
    /// Text content of a mixed element, stored under `#text`.
    Text,
}

/// Where a node came from and what the source said about it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    /// The node's position (for a map entry, its key's).
    pub position: Option<Position>,
    /// The YAML anchor defined on this node (`&name`).
    pub anchor: Option<String>,
    /// For a YAML alias (`*name`), the anchor it refers to. The node holds a
    /// copy of the anchored value.
    pub alias: Option<String>,
    /// A comment attached to this entry (INI and dotenv).
    pub comment: Option<String>,
    /// For XML documents, what the node was.
    pub xml: Option<XmlKind>,
}

/// A value in the document tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    /// An unsigned integer above `i64::MAX`.
    UInt(u64),
    Float(f64),
    String(String),
    /// A TOML date, time or date-time, exactly as written.
    DateTime(String),
    Seq(Vec<Node>),
    /// Entries in insertion (document) order.
    Map(Vec<(String, Node)>),
}

/// A value plus its [`Meta`].
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub value: Value,
    pub meta: Meta,
}

impl From<Value> for Node {
    fn from(value: Value) -> Self {
        Node::new(value)
    }
}

impl<'a> From<Node> for Cow<'a, Node> {
    fn from(node: Node) -> Self {
        Cow::Owned(node)
    }
}

impl<'a> From<&'a Node> for Cow<'a, Node> {
    fn from(node: &'a Node) -> Self {
        Cow::Borrowed(node)
    }
}

impl Node {
    /// A node with no metadata.
    pub fn new(value: Value) -> Self {
        Node {
            value,
            meta: Meta::default(),
        }
    }

    /// A node with metadata.
    pub fn with_meta(value: Value, meta: Meta) -> Self {
        Node { value, meta }
    }

    /// Builder: set the source position.
    pub fn at_position(mut self, position: Option<Position>) -> Self {
        self.meta.position = position;
        self
    }

    /// Whether this is a sequence or a map.
    pub fn is_container(&self) -> bool {
        matches!(self.value, Value::Seq(_) | Value::Map(_))
    }

    /// The number of children of a container (0 for scalars).
    pub fn len(&self) -> usize {
        match &self.value {
            Value::Seq(items) => items.len(),
            Value::Map(entries) => entries.len(),
            _ => 0,
        }
    }

    /// Whether this is an empty container or a scalar.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The value of `key`, for a map.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match &self.value {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Item `index`, for a sequence.
    pub fn index(&self, index: usize) -> Option<&Node> {
        match &self.value {
            Value::Seq(items) => items.get(index),
            _ => None,
        }
    }

    /// The node at `path`.
    pub fn at(&self, path: &Path) -> Option<&Node> {
        path.segments()
            .iter()
            .try_fold(self, |node, segment| match segment {
                PathSegment::Key(key) => node.get(key),
                PathSegment::Index(index) => node.index(*index),
            })
    }

    /// The string, for a string or date-time.
    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::String(s) | Value::DateTime(s) => Some(s),
            _ => None,
        }
    }

    /// A short type name: `null`, `bool`, `int`, `float`, `str`, `datetime`,
    /// `seq` or `map`.
    pub fn type_name(&self) -> &'static str {
        match &self.value {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) | Value::UInt(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "str",
            Value::DateTime(_) => "datetime",
            Value::Seq(_) => "seq",
            Value::Map(_) => "map",
        }
    }

    /// The value as JSON. Non-finite floats become `null` and date-times
    /// strings; metadata is dropped.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match &self.value {
            Value::Null => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Int(i) => J::from(*i),
            Value::UInt(u) => J::from(*u),
            Value::Float(f) => serde_json::Number::from_f64(*f).map_or(J::Null, J::Number),
            Value::String(s) | Value::DateTime(s) => J::String(s.clone()),
            Value::Seq(items) => J::Array(items.iter().map(Node::to_json).collect()),
            Value::Map(entries) => J::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_json()))
                    .collect(),
            ),
        }
    }

    /// Visit every node, parents before children, with its path.
    pub fn walk<'a>(&'a self, mut visit: impl FnMut(&Path, &'a Node)) {
        // An explicit stack: parsers accept deeper documents than the call
        // stack would.
        let mut stack: Vec<(Path, &'a Node)> = vec![(Path::root(), self)];
        while let Some((path, node)) = stack.pop() {
            visit(&path, node);
            match &node.value {
                Value::Seq(items) => {
                    for (i, item) in items.iter().enumerate().rev() {
                        stack.push((path.child_index(i), item));
                    }
                }
                Value::Map(entries) => {
                    for (key, value) in entries.iter().rev() {
                        stack.push((path.child_key(key), value));
                    }
                }
                _ => {}
            }
        }
    }

    /// A copy with every string or number leaf that `redactor` masks
    /// replaced, keeping the structure.
    pub fn redacted(&self, redactor: &dyn Redactor) -> Node {
        redact::apply(self, redactor)
    }
}

/// Values compare equal ignoring metadata; floats by bit pattern so `NaN`
/// equals itself; `Int` and `UInt` by numeric value.
pub(crate) fn value_eq(a: &Node, b: &Node) -> bool {
    match (&a.value, &b.value) {
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits() || x == y,
        (Value::Int(x), Value::UInt(y)) | (Value::UInt(y), Value::Int(x)) => {
            u64::try_from(*x).is_ok_and(|x| x == *y)
        }
        (Value::Seq(x), Value::Seq(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| value_eq(x, y))
        }
        (Value::Map(x), Value::Map(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((kx, vx), (ky, vy))| kx == ky && value_eq(vx, vy))
        }
        (x, y) => x == y,
    }
}

impl From<&serde_json::Value> for Node {
    fn from(value: &serde_json::Value) -> Self {
        use serde_json::Value as J;
        Node::new(match value {
            J::Null => Value::Null,
            J::Bool(b) => Value::Bool(*b),
            J::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else if let Some(u) = n.as_u64() {
                    Value::UInt(u)
                } else {
                    Value::Float(n.as_f64().unwrap_or(f64::NAN))
                }
            }
            J::String(s) => Value::String(s.clone()),
            J::Array(items) => Value::Seq(items.iter().map(Node::from).collect()),
            J::Object(entries) => Value::Map(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), Node::from(v)))
                    .collect(),
            ),
        })
    }
}

impl From<serde_json::Value> for Node {
    fn from(value: serde_json::Value) -> Self {
        Node::from(&value)
    }
}

/// Convert any `Serialize` value into a [`Node`].
///
/// This is a direct serializer, so `u64` above `i64::MAX` stays exact, map
/// keys that are not strings are stringified (`1` → `"1"`), and field order
/// is kept. `i128`/`u128` values outside the 64-bit range become floats.
///
/// ```
/// use rich_ext::data::{from_serialize, Value};
/// use std::collections::BTreeMap;
///
/// let map = BTreeMap::from([(1, u64::MAX)]);
/// let node = from_serialize(&map).unwrap();
/// assert_eq!(node.get("1").unwrap().value, Value::UInt(u64::MAX));
/// ```
pub fn from_serialize<T: serde::Serialize + ?Sized>(value: &T) -> Result<Node, DataError> {
    value.serialize(ser::NodeSerializer)
}

// ---------------------------------------------------------------------------
// Scalars as text
// ---------------------------------------------------------------------------

/// A scalar's display text and the style name it takes. Strings are quoted
/// JSON-style when `quote` is set, so control characters never reach the
/// terminal raw.
pub(crate) fn scalar_text(value: &Value, quote: bool) -> (String, &'static str) {
    match value {
        Value::Null => ("null".into(), "json.null"),
        Value::Bool(true) => ("true".into(), "json.bool_true"),
        Value::Bool(false) => ("false".into(), "json.bool_false"),
        Value::Int(i) => (i.to_string(), "json.number"),
        Value::UInt(u) => (u.to_string(), "json.number"),
        Value::Float(f) => (rich::pyformat::float_repr(*f), "json.number"),
        Value::String(s) if quote => (quote_str(s), "json.str"),
        Value::String(s) => (escape_controls(s), "json.str"),
        Value::DateTime(s) => (s.clone(), "data.datetime"),
        Value::Seq(_) | Value::Map(_) => (String::new(), "data.summary"),
    }
}

/// `s` as a JSON string literal. serde_json escapes only U+0000–U+001F, so
/// DEL and the C1 controls (U+0080–U+009F, among them the one-character
/// CSI U+009B) are escaped here too: no control character reaches the
/// terminal raw.
pub(crate) fn quote_str(s: &str) -> String {
    let quoted = serde_json::to_string(s).unwrap_or_else(|_| format!("{s:?}"));
    if !quoted.chars().any(char::is_control) {
        return quoted;
    }
    let mut out = String::with_capacity(quoted.len() + 8);
    for c in quoted.chars() {
        if c.is_control() {
            out.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// `s` with newlines, tabs and other control characters escaped, for
/// unquoted single-line display.
pub(crate) fn escape_controls(s: &str) -> String {
    if !s.chars().any(char::is_control) {
        return s.to_string();
    }
    let quoted = quote_str(s);
    // Drop the quotes but keep the escapes; `\"` stays escaped, harmlessly.
    quoted[1..quoted.len() - 1].replace("\\\"", "\"")
}

/// A folded container's summary: `{…} 3 keys`, `[…] 1 item`, `{}`, `[]`.
pub(crate) fn summary(node: &Node) -> String {
    let plural = |n: usize, word: &str| {
        if n == 1 {
            format!("{n} {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    match &node.value {
        Value::Map(e) if e.is_empty() => "{}".into(),
        Value::Seq(i) if i.is_empty() => "[]".into(),
        Value::Map(e) => format!("{{…}} {}", plural(e.len(), "key")),
        Value::Seq(i) => format!("[…] {}", plural(i.len(), "item")),
        _ => String::new(),
    }
}

/// `text` cut to at most `max` characters, with an ellipsis when cut.
pub(crate) fn truncate_chars(text: &str, max: usize) -> Cow<'_, str> {
    match text.char_indices().nth(max) {
        Some((cut, _)) => Cow::Owned(format!("{}…", &text[..cut])),
        None => Cow::Borrowed(text),
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// One step of a [`Path`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

/// A route from the root to a node. Displays as `servers[0].name`, quoting
/// keys that are not identifiers: `a["weird key"]`. The root is the empty
/// path and displays as an empty string.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Path(Vec<PathSegment>);

impl Path {
    /// The empty path.
    pub fn root() -> Self {
        Path(Vec::new())
    }

    pub fn segments(&self) -> &[PathSegment] {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, segment: PathSegment) {
        self.0.push(segment);
    }

    /// This path plus `key`.
    pub fn child_key(&self, key: &str) -> Path {
        let mut path = self.clone();
        path.0.push(PathSegment::Key(key.to_string()));
        path
    }

    /// This path plus `[index]`.
    pub fn child_index(&self, index: usize) -> Path {
        let mut path = self.clone();
        path.0.push(PathSegment::Index(index));
        path
    }

    /// The path without its last segment.
    pub fn parent(&self) -> Option<Path> {
        (!self.0.is_empty()).then(|| Path(self.0[..self.0.len() - 1].to_vec()))
    }

    pub fn last(&self) -> Option<&PathSegment> {
        self.0.last()
    }

    /// The last key on the path (skipping trailing indexes): `tokens` for
    /// `auth.tokens[1]`.
    pub fn last_key(&self) -> Option<&str> {
        self.0.iter().rev().find_map(|s| match s {
            PathSegment::Key(k) => Some(k.as_str()),
            PathSegment::Index(_) => None,
        })
    }
}

impl From<Vec<PathSegment>> for Path {
    fn from(segments: Vec<PathSegment>) -> Self {
        Path(segments)
    }
}

pub(crate) fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// How one segment displays after `previous` segments.
pub(crate) fn segment_text(segment: &PathSegment, first: bool) -> String {
    match segment {
        PathSegment::Index(i) => format!("[{i}]"),
        PathSegment::Key(k) if is_identifier(k) && first => k.clone(),
        PathSegment::Key(k) if is_identifier(k) => format!(".{k}"),
        PathSegment::Key(k) => format!("[{}]", quote_str(k)),
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.0.iter().enumerate() {
            f.write_str(&segment_text(segment, i == 0))?;
        }
        Ok(())
    }
}

impl std::str::FromStr for Path {
    type Err = String;

    /// Parse the display form back: `a.b[0]["c d"]`. A leading `$` or `.` is
    /// accepted.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        let mut path = Path::root();
        if chars.first() == Some(&'$') {
            i = 1;
        }
        let mut first = true;
        while i < chars.len() {
            match chars[i] {
                '.' => {
                    i += 1;
                    let start = i;
                    while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
                        i += 1;
                    }
                    if start == i {
                        return Err(format!("empty key at column {}", start + 1));
                    }
                    path.push(PathSegment::Key(chars[start..i].iter().collect()));
                }
                '[' => {
                    i += 1;
                    if chars.get(i) == Some(&'"') {
                        let start = i;
                        i += 1;
                        while i < chars.len() && chars[i] != '"' {
                            if chars[i] == '\\' {
                                i += 1;
                            }
                            i += 1;
                        }
                        let literal: String =
                            chars[start..(i + 1).min(chars.len())].iter().collect();
                        let key: String = serde_json::from_str(&literal)
                            .map_err(|_| format!("bad quoted key at column {}", start + 1))?;
                        i += 1;
                        path.push(PathSegment::Key(key));
                    } else {
                        let start = i;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                        let digits: String = chars[start..i].iter().collect();
                        let index = digits
                            .parse()
                            .map_err(|_| format!("expected an index at column {}", start + 1))?;
                        path.push(PathSegment::Index(index));
                    }
                    if chars.get(i) != Some(&']') {
                        return Err(format!("expected `]` at column {}", i + 1));
                    }
                    i += 1;
                }
                _ if first => {
                    let start = i;
                    while i < chars.len() && chars[i] != '.' && chars[i] != '[' {
                        i += 1;
                    }
                    path.push(PathSegment::Key(chars[start..i].iter().collect()));
                }
                c => return Err(format!("unexpected `{c}` at column {}", i + 1)),
            }
            first = false;
        }
        Ok(path)
    }
}

// ---------------------------------------------------------------------------
// Formats, detection and errors
// ---------------------------------------------------------------------------

/// A source format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Json,
    Yaml,
    Toml,
    Xml,
    Ini,
    Dotenv,
}

impl Format {
    /// Every format, whether or not its feature is compiled in.
    pub const ALL: [Format; 6] = [
        Format::Json,
        Format::Yaml,
        Format::Toml,
        Format::Xml,
        Format::Ini,
        Format::Dotenv,
    ];

    /// The lowercase name: `json`, `yaml`, `toml`, `xml`, `ini`, `dotenv`.
    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
            Format::Xml => "xml",
            Format::Ini => "ini",
            Format::Dotenv => "dotenv",
        }
    }

    /// A format by name, case-insensitively; `yml` and `env` are accepted.
    pub fn from_name(name: &str) -> Option<Format> {
        match name.to_ascii_lowercase().as_str() {
            "json" => Some(Format::Json),
            "yaml" | "yml" => Some(Format::Yaml),
            "toml" => Some(Format::Toml),
            "xml" => Some(Format::Xml),
            "ini" => Some(Format::Ini),
            "dotenv" | "env" => Some(Format::Dotenv),
            _ => None,
        }
    }

    /// A format by file extension, with or without the dot: `json`, `yaml`,
    /// `yml`, `toml`, `xml`, `ini`, `cfg`, `env`.
    pub fn from_extension(extension: &str) -> Option<Format> {
        let extension = extension.strip_prefix('.').unwrap_or(extension);
        match extension.to_ascii_lowercase().as_str() {
            "json" => Some(Format::Json),
            "yaml" | "yml" => Some(Format::Yaml),
            "toml" => Some(Format::Toml),
            "xml" => Some(Format::Xml),
            "ini" | "cfg" => Some(Format::Ini),
            "env" => Some(Format::Dotenv),
            _ => None,
        }
    }

    /// A format by file name: its extension, or `.env` / `.env.*` for dotenv.
    pub fn from_file_name(name: &str) -> Option<Format> {
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        if base == ".env" || base.starts_with(".env.") {
            return Some(Format::Dotenv);
        }
        let (stem, extension) = base.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        Format::from_extension(extension)
    }

    /// Whether this build can parse the format (its feature is enabled).
    pub fn is_enabled(self) -> bool {
        match self {
            Format::Json | Format::Ini | Format::Dotenv => true,
            Format::Yaml => cfg!(feature = "yaml"),
            Format::Toml => cfg!(feature = "toml"),
            Format::Xml => cfg!(feature = "xml"),
        }
    }

    /// Guess the format of `content`, conservatively.
    ///
    /// A `name_hint` whose file name maps to an enabled format wins outright
    /// (a parse error then explains the problem better than a guess would).
    /// Otherwise the content is tried against each enabled format in this
    /// order, most distinctive first:
    ///
    /// 1. **JSON** — starts with `{` or `[` and parses as an object or array.
    ///    First because every JSON document is also YAML.
    /// 2. **XML** — starts with `<?xml` or `<` and a name, and parses.
    /// 3. **TOML** — parses and defines at least one key or table. Before
    ///    dotenv and INI because a document valid as TOML *and* as either of
    ///    those gets typed values from TOML.
    /// 4. **dotenv** — every non-comment line is `KEY=VALUE` (optionally
    ///    `export KEY=VALUE`), `KEY` matching `[A-Za-z_][A-Za-z0-9_.]*`, with
    ///    no spaces around `=` and unquoted values free of whitespace.
    /// 5. **INI** — has a `[section]` header, at least one `key = value` or
    ///    `key: value` line, and nothing else but comments and continuations.
    ///    Only reached when the text is not TOML (if TOML is compiled in).
    /// 6. **YAML** — last, because it accepts almost anything. Every
    ///    top-level line must be structural (`key:` with an identifier-like
    ///    key, `- item`, `---`, a comment), there must be at least two
    ///    such lines and at least one `key:` line, and every document must be
    ///    a mapping or sequence. A lone `key: value` line, prose, a bullet
    ///    list and Markdown are rejected.
    ///
    /// CSV and other tabular text is not a supported format: it matches none
    /// of the rules above, so `detect` returns `None` for it.
    ///
    /// ```
    /// use rich_ext::data::Format;
    ///
    /// assert_eq!(Format::detect("{\"a\": 1}", None), Some(Format::Json));
    /// assert_eq!(Format::detect("A=1\nB=two", None).is_some(), true);
    /// assert_eq!(Format::detect("just some prose", None), None);
    /// assert_eq!(Format::detect("name,age\nbob,3", None), None);
    /// assert_eq!(Format::detect("anything", Some("app.json")), Some(Format::Json));
    /// ```
    pub fn detect(content: &str, name_hint: Option<&str>) -> Option<Format> {
        if let Some(format) = name_hint.and_then(Format::from_file_name) {
            if format.is_enabled() {
                return Some(format);
            }
        }
        let trimmed = content.trim_start_matches('\u{feff}').trim();
        if trimmed.is_empty() {
            return None;
        }
        if (trimmed.starts_with('{') || trimmed.starts_with('['))
            && serde_json::from_str::<serde_json::Value>(trimmed)
                .is_ok_and(|v| v.is_object() || v.is_array())
        {
            return Some(Format::Json);
        }
        #[cfg(feature = "xml")]
        if xml::looks_like(trimmed) && xml::parse(content).is_ok() {
            return Some(Format::Xml);
        }
        #[cfg(feature = "toml")]
        if toml_doc::looks_like(content) {
            return Some(Format::Toml);
        }
        if dotenv::looks_like(content) {
            return Some(Format::Dotenv);
        }
        if ini::looks_like(content) {
            return Some(Format::Ini);
        }
        #[cfg(feature = "yaml")]
        if yaml::looks_like(content) {
            return Some(Format::Yaml);
        }
        None
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Format::Json => "JSON",
            Format::Yaml => "YAML",
            Format::Toml => "TOML",
            Format::Xml => "XML",
            Format::Ini => "INI",
            Format::Dotenv => "dotenv",
        })
    }
}

/// A parse (or conversion) failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataError {
    pub format: Format,
    pub message: String,
    pub position: Option<Position>,
}

impl DataError {
    /// Control characters in `message` are escaped (`\u001b`): parser
    /// messages often repeat the offending input, which must not reach the
    /// terminal raw.
    pub fn new(format: Format, message: impl Into<String>, position: Option<Position>) -> Self {
        DataError {
            format,
            message: escape_controls(&message.into()),
            position,
        }
    }

    /// This error as a diagnostic over `source` (shown as `name`), with the
    /// offending line underlined when the position is known.
    ///
    /// ```
    /// use rich::Console;
    /// use rich_ext::data::{parse, Format};
    ///
    /// let source = "{\n  \"a\" 1\n}";
    /// let error = parse(Format::Json, source).unwrap_err();
    /// let out = Console::builder()
    ///     .width(60)
    ///     .build()
    ///     .render_export(&error.to_diagnostic(source, "a.json"));
    /// assert!(out.contains("a.json:2:7"), "{out}");
    /// assert!(out.contains("2 |   \"a\" 1\n  |       ^ expected `:`"), "{out}");
    /// ```
    pub fn to_diagnostic(&self, source: &str, name: &str) -> crate::diagnostic::Diagnostic {
        use crate::diagnostic::{Diagnostic, Location, SourceSnippet};
        use crate::event::EventView;
        let mut diagnostic =
            Diagnostic::error(format!("invalid {}: {}", self.format, self.message))
                .view(EventView::Expanded);
        let Some(position) = self.position else {
            return diagnostic;
        };
        diagnostic = diagnostic.location(Location::new(
            name,
            Some(position.line),
            Some(position.column),
        ));
        let start = byte_offset(source, position);
        let end = source[start..]
            .chars()
            .next()
            .filter(|c| *c != '\n' && *c != '\r')
            .map_or(start, |c| start + c.len_utf8());
        // The snippet shows source lines as they are, so control characters
        // become one-column pictures first (keeping the caret aligned).
        let (source, [start, end]) = picture_controls(source, [start, end]);
        if let Ok(snippet) = SourceSnippet::new(name.to_string(), source, start..end, 1) {
            diagnostic = diagnostic.snippet(snippet.primary_label(self.message.clone()));
        }
        diagnostic
    }
}

/// `source` with each control character but newline, tab and a CR ending a
/// line replaced by one visible character: its Control Picture (`␛` for
/// ESC, `␡` for DEL), or `�` for a C1 control. `offsets` (byte offsets
/// into `source`) come back mapped into the result.
fn picture_controls(source: &str, mut offsets: [usize; 2]) -> (String, [usize; 2]) {
    let mut out = String::with_capacity(source.len());
    let original = offsets;
    let mut chars = source.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        for (offset, at) in offsets.iter_mut().zip(original) {
            if at == i {
                *offset = out.len();
            }
        }
        let keep = matches!(c, '\n' | '\t')
            || (c == '\r' && matches!(chars.peek(), None | Some((_, '\n'))));
        out.push(match c {
            _ if keep || !c.is_control() => c,
            '\u{0}'..='\u{1f}' => char::from_u32(0x2400 + c as u32).unwrap_or('\u{fffd}'),
            '\u{7f}' => '\u{2421}',
            _ => '\u{fffd}',
        });
    }
    for (offset, at) in offsets.iter_mut().zip(original) {
        if at >= source.len() {
            *offset = out.len();
        }
    }
    (out, offsets)
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `message` is public, so escape again for errors built by hand.
        let message = escape_controls(&self.message);
        match self.position {
            Some(p) => write!(
                f,
                "invalid {} at line {}, column {}: {message}",
                self.format, p.line, p.column
            ),
            None => write!(f, "invalid {}: {message}", self.format),
        }
    }
}

impl std::error::Error for DataError {}

/// Line starts of a source, for byte offset → [`Position`].
#[cfg(any(feature = "toml", feature = "xml"))]
pub(crate) struct LineIndex<'a> {
    source: &'a str,
    starts: Vec<usize>,
}

#[cfg(any(feature = "toml", feature = "xml"))]
impl<'a> LineIndex<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { source, starts }
    }

    pub(crate) fn position(&self, offset: usize) -> Position {
        let mut offset = offset.min(self.source.len());
        while !self.source.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.starts.partition_point(|s| *s <= offset);
        let start = self.starts[line - 1];
        let column = self.source[start..offset].chars().count() + 1;
        Position::new(line, column)
    }
}

/// The byte offset of `position` in `source`, clamped to the line's end.
pub(crate) fn byte_offset(source: &str, position: Position) -> usize {
    let mut offset = 0;
    for _ in 1..position.line {
        match source[offset..].find('\n') {
            Some(i) => offset += i + 1,
            None => return source.len(),
        }
    }
    let line_end = source[offset..]
        .find('\n')
        .map_or(source.len(), |i| offset + i);
    source[offset..line_end]
        .char_indices()
        .nth(position.column.saturating_sub(1))
        .map_or(line_end, |(i, _)| offset + i)
}

/// Parse `content` as `format`.
///
/// ```
/// use rich_ext::data::{parse, Format, Value};
///
/// let node = parse(Format::Dotenv, "export NAME=\"a\\nb\"\n").unwrap();
/// assert_eq!(node.get("NAME").unwrap().value, Value::String("a\nb".into()));
/// ```
pub fn parse(format: Format, content: &str) -> Result<Node, DataError> {
    match format {
        Format::Json => json::parse(content),
        Format::Ini => ini::parse(content),
        Format::Dotenv => dotenv::parse(content),
        #[cfg(feature = "yaml")]
        Format::Yaml => yaml::parse(content),
        #[cfg(feature = "toml")]
        Format::Toml => toml_doc::parse(content),
        #[cfg(feature = "xml")]
        Format::Xml => xml::parse(content),
        #[allow(unreachable_patterns)]
        other => Err(DataError::new(
            other,
            format!(
                "{} support is not compiled in (enable the `{}` feature)",
                other,
                other.name()
            ),
            None,
        )),
    }
}

/// Parse JSON (object key order is kept; the last duplicate key wins). The
/// integer `-0` reads as `Int(0)`, as Python's `json` reads it; `-0.0` stays
/// a float.
pub fn parse_json(content: &str) -> Result<Node, DataError> {
    json::parse(content)
}

/// Parse an INI file. See [`ConfigFileView`] for the shape it produces.
///
/// Sections (`[name]`, `[a.b]` kept literally) become maps under the root;
/// keys before the first section sit in the root. `key = value` and
/// `key: value` both work, values are strings (no type guessing, no inline
/// comment stripping), indented lines continue the previous value, and a
/// repeated key keeps its first slot but takes the last value and position.
/// `;` and `#` comment lines directly above an entry (no blank line between)
/// become its `meta.comment`. A section named like a key before the first
/// section is an error: both would live in the root map.
pub fn parse_ini(content: &str) -> Result<Node, DataError> {
    ini::parse(content)
}

/// Parse a dotenv file.
///
/// `KEY=VALUE` and `export KEY=VALUE`; single-quoted values are literal,
/// double-quoted values understand `\n`, `\t`, `\r`, `\"` and `\\` (and may
/// span lines), unquoted values lose a trailing ` #comment`. Values are
/// strings and **no variable expansion** is done: `$HOME` stays `$HOME`.
/// Comment lines directly above an entry (and an inline comment) become its
/// `meta.comment`. A repeated key keeps its first slot but takes the last
/// value and position.
pub fn parse_dotenv(content: &str) -> Result<Node, DataError> {
    dotenv::parse(content)
}

/// Parse YAML (1.2 core schema). See the `yaml` feature.
///
/// Anchors and aliases are recorded in [`Meta`]; an alias node holds a copy
/// of its anchor's value, and expansion stops with an error past one million
/// copied nodes or 64 MiB of copied strings (the "billion laughs" guard;
/// only anchors that an alias uses are copied, and those copies count too).
/// Merge keys (`<<`) are kept as ordinary keys; a key repeated in one mapping
/// is an error. Several documents parse to a sequence of documents.
/// Comments are not kept. Nesting deeper than 512 levels is an error.
#[cfg(feature = "yaml")]
pub fn parse_yaml(content: &str) -> Result<Node, DataError> {
    yaml::parse(content)
}

/// Parse TOML; tables keep document order and date-times keep their text.
#[cfg(feature = "toml")]
pub fn parse_toml(content: &str) -> Result<Node, DataError> {
    toml_doc::parse(content)
}

/// Parse XML into `{root: …}`: attributes as `@name` keys, repeated child
/// elements as sequences, text as the element's value or, beside
/// attributes or children, under `#text`. Namespace prefixes are kept as
/// written; comments and processing instructions are dropped, and text
/// outside the root element is an error. Parsing
/// streams, so size is not limited here (the views have limits), but nesting
/// deeper than 512 elements is an error.
#[cfg(feature = "xml")]
pub fn parse_xml(content: &str) -> Result<Node, DataError> {
    xml::parse(content)
}
