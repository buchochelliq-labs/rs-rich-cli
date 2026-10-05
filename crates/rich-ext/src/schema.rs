//! JSON Schema, drawn as a tree (#244), and what changed between two.
//!
//! [`SchemaTree`] draws a schema as a [`Tree`] of its properties: each with
//! its type, a `(required)` marker, its constraints (`minLength=1`,
//! `format=email`, `one of "a", "b"`, …) and the first line of its
//! description. Array items, `patternProperties`, `additionalProperties`,
//! `oneOf` / `anyOf` / `allOf` branches, `not` and `if` / `then` / `else`
//! are branches of their own. A `$ref` within the document (`#/$defs/…`,
//! `#/definitions/…`, any JSON pointer, or a `$anchor`) is resolved and
//! drawn in place; one that refers back to a schema it is already inside is
//! marked `(recursive)` instead of drawn again, and one to another document
//! is shown as a reference.
//!
//! [`SchemaDiff`] compares two versions: properties added and removed, type
//! changes, properties that became (or stopped being) required, enum values
//! and constraints, each marked `+`, `-` or `~`, and `breaking` where a
//! document the old schema accepted may now be refused.
//!
//! Neither reads colour alone: markers and words carry the meaning, and the
//! styles come from the theme keys in [`STYLES`].
//!
//! ```
//! use rich::Console;
//! use rich_ext::schema::SchemaTree;
//!
//! let schema = serde_json::json!({
//!     "title": "User",
//!     "type": "object",
//!     "required": ["email"],
//!     "properties": {
//!         "email": {"type": "string", "format": "email"},
//!         "age": {"type": "integer", "minimum": 0}
//!     }
//! });
//! let console = Console::builder().width(60).color_system(None).build();
//! assert_eq!(
//!     console.render_to_string(&SchemaTree::new(schema)),
//!     "User  object\n\
//!      ├── email (required)  string  format=email\n\
//!      └── age  integer  minimum=0"
//! );
//! ```

use std::collections::BTreeSet;
use std::fmt;

use rich::table::Table;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text, Tree};
use serde_json::Value;

/// Theme keys for schema trees and diffs, with the styles used when a theme
/// lacks them.
pub const STYLES: &[(&str, &str)] = &[
    ("schema.name", "bold"),
    ("schema.required", "bold red"),
    ("schema.type", "cyan"),
    ("schema.constraint", "yellow"),
    ("schema.description", "dim"),
    ("schema.ref", "dim italic"),
    ("schema.branch", "magenta"),
    ("schema.added", "green"),
    ("schema.removed", "red"),
    ("schema.changed", "yellow"),
    ("schema.breaking", "bold red"),
];

fn theme_style(console: &Console, key: &str) -> Style {
    if let Some(style) = console.theme().get(key) {
        return style.clone();
    }
    STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, spec)| Style::parse(spec).ok())
        .unwrap_or_default()
}

/// Why a schema could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaError(String);

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SchemaError {}

/// Read a schema from JSON text. A schema is an object or a boolean.
pub fn parse(json: &str) -> Result<Value, SchemaError> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| SchemaError(format!("not JSON: {e}")))?;
    if !(value.is_object() || value.is_boolean()) {
        return Err(SchemaError(
            "a JSON Schema is an object (or true or false)".into(),
        ));
    }
    Ok(value)
}

/// The constraint keywords shown and compared, in the order shown.
const CONSTRAINTS: &[&str] = &[
    "format",
    "pattern",
    "minLength",
    "maxLength",
    "minimum",
    "exclusiveMinimum",
    "maximum",
    "exclusiveMaximum",
    "multipleOf",
    "minItems",
    "maxItems",
    "uniqueItems",
    "minContains",
    "maxContains",
    "minProperties",
    "maxProperties",
    "contentEncoding",
    "contentMediaType",
    "default",
    "deprecated",
    "readOnly",
    "writeOnly",
];

/// The most levels drawn or compared below the root.
const DEPTH: usize = 32;

// --------------------------------------------------------------- resolving

/// Resolves `$ref`s within one document.
struct Resolver<'a> {
    root: &'a Value,
}

/// `%XX` escapes decoded, as a URI fragment's are.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl<'a> Resolver<'a> {
    /// The schema `reference` names, or why it is not followed.
    fn resolve(&self, reference: &str) -> Result<&'a Value, String> {
        let Some(fragment) = reference.strip_prefix('#') else {
            return Err("another document".into());
        };
        let fragment = percent_decode(fragment);
        if fragment.is_empty() {
            return Ok(self.root);
        }
        if fragment.starts_with('/') {
            return self
                .root
                .pointer(&fragment)
                .ok_or_else(|| "not found".to_string());
        }
        find_anchor(self.root, &fragment).ok_or_else(|| "not found".to_string())
    }
}

/// The schema with `$anchor` (or a `$id` of `#name`) `name`.
fn find_anchor<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            let anchor = map.get("$anchor").and_then(Value::as_str) == Some(name)
                || map
                    .get("$id")
                    .and_then(Value::as_str)
                    .and_then(|id| id.strip_prefix('#'))
                    == Some(name);
            if anchor {
                return Some(value);
            }
            map.values().find_map(|child| find_anchor(child, name))
        }
        Value::Array(items) => items.iter().find_map(|child| find_anchor(child, name)),
        _ => None,
    }
}

/// A schema's type, in words: `string`, `string | null`, `object`, `enum`, …
fn type_of(schema: &Value) -> String {
    match schema {
        Value::Bool(true) => return "any".into(),
        Value::Bool(false) => return "never".into(),
        _ => {}
    }
    match schema.get("type") {
        Some(Value::String(name)) => return name.clone(),
        Some(Value::Array(names)) => {
            let names: Vec<&str> = names.iter().filter_map(Value::as_str).collect();
            if !names.is_empty() {
                return names.join(" | ");
            }
        }
        _ => {}
    }
    let has = |key: &str| schema.get(key).is_some();
    if has("const") {
        "const".into()
    } else if has("enum") {
        "enum".into()
    } else if has("properties") || has("patternProperties") || has("required") {
        "object".into()
    } else if has("items") || has("prefixItems") {
        "array".into()
    } else if has("oneOf") {
        "one of".into()
    } else if has("anyOf") {
        "any of".into()
    } else if has("allOf") {
        "all of".into()
    } else if has("$ref") {
        "ref".into()
    } else {
        "any".into()
    }
}

/// A JSON value written compactly, long ones cut.
fn compact(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 40 {
        let cut: String = text.chars().take(39).collect();
        format!("{cut}…")
    } else {
        text
    }
}

/// The constraints shown on a schema's line.
fn constraints(schema: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(value) = schema.get("const") {
        out.push(format!("= {}", compact(value)));
    }
    if let Some(Value::Array(values)) = schema.get("enum") {
        let shown: Vec<String> = values.iter().take(8).map(compact).collect();
        let more = if values.len() > 8 {
            format!(", … {} more", values.len() - 8)
        } else {
            String::new()
        };
        out.push(format!("one of {}{more}", shown.join(", ")));
    }
    for key in CONSTRAINTS {
        match (key, schema.get(*key)) {
            (_, None) => {}
            (&"deprecated", Some(Value::Bool(true))) => out.push("deprecated".into()),
            (&"readOnly", Some(Value::Bool(true))) => out.push("read-only".into()),
            (&"writeOnly", Some(Value::Bool(true))) => out.push("write-only".into()),
            (&"uniqueItems", Some(Value::Bool(true))) => out.push("unique items".into()),
            (&"deprecated" | &"readOnly" | &"writeOnly" | &"uniqueItems", _) => {}
            (&"pattern", Some(Value::String(pattern))) => out.push(format!("pattern=/{pattern}/")),
            (&"format", Some(Value::String(format))) => out.push(format!("format={format}")),
            (key, Some(value)) => out.push(format!("{key}={}", compact(value))),
        }
    }
    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        out.push("no other properties".into());
    }
    out
}

fn required_names(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

// --------------------------------------------------------------- the tree

/// A JSON Schema drawn as a tree. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct SchemaTree {
    schema: Value,
    title: Option<String>,
    max_depth: usize,
}

impl SchemaTree {
    pub fn new(schema: Value) -> Self {
        SchemaTree {
            schema,
            title: None,
            max_depth: DEPTH,
        }
    }

    /// Read a schema from JSON text ([`parse`]).
    pub fn from_json(json: &str) -> Result<Self, SchemaError> {
        parse(json).map(SchemaTree::new)
    }

    /// The root's name, over the schema's `title` (default `schema`).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Draw at most this many levels below the root (default 32).
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    pub fn schema(&self) -> &Value {
        &self.schema
    }

    /// Build the [`Tree`] this renders as.
    pub fn tree(&self, console: &Console) -> Tree {
        let resolver = Resolver { root: &self.schema };
        let name = self
            .title
            .clone()
            .or_else(|| {
                self.schema
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "schema".into());
        let mut stack = Vec::new();
        self.node(
            &resolver,
            &self.schema,
            &name,
            false,
            0,
            &mut stack,
            console,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn node(
        &self,
        resolver: &Resolver<'_>,
        schema: &Value,
        name: &str,
        required: bool,
        depth: usize,
        stack: &mut Vec<String>,
        console: &Console,
    ) -> Tree {
        let mut label = Text::new("");
        label.append(name, Some(theme_style(console, "schema.name").into()));
        if required {
            label.append(
                " (required)",
                Some(theme_style(console, "schema.required").into()),
            );
        }
        // Follow `$ref`s to the schema they name, guarding against cycles.
        let mut target = schema;
        let mut followed = Vec::new();
        let mut note: Option<String> = None;
        while let Some(reference) = target.get("$ref").and_then(Value::as_str) {
            if stack.iter().any(|seen| seen == reference) || followed.contains(&reference) {
                note = Some(format!("→ {reference} (recursive)"));
                if let Ok(resolved) = resolver.resolve(reference) {
                    target = resolved;
                }
                break;
            }
            match resolver.resolve(reference) {
                Ok(resolved) => {
                    followed.push(reference);
                    // Sibling keywords next to `$ref` still apply; show the
                    // referenced schema's own.
                    target = resolved;
                }
                Err(why) => {
                    note = Some(format!("→ {reference} ({why})"));
                    break;
                }
            }
        }
        let recursive = note.as_deref().is_some_and(|n| n.ends_with("(recursive)"));
        let unresolved = note.is_some() && !recursive;
        if !unresolved {
            label.append("  ", None);
            label.append(
                &type_of(target),
                Some(theme_style(console, "schema.type").into()),
            );
        }
        let mut shown = constraints(target);
        if !std::ptr::eq(target, schema) {
            for extra in constraints(schema) {
                if !shown.contains(&extra) {
                    shown.push(extra);
                }
            }
        }
        if !shown.is_empty() && !recursive {
            label.append("  ", None);
            label.append(
                &shown.join(", "),
                Some(theme_style(console, "schema.constraint").into()),
            );
        }
        if let Some(reference) = followed.last() {
            label.append(
                &format!("  → {reference}"),
                Some(theme_style(console, "schema.ref").into()),
            );
        }
        if let Some(note) = &note {
            label.append("  ", None);
            label.append(note, Some(theme_style(console, "schema.ref").into()));
        }
        let description = schema
            .get("description")
            .or_else(|| target.get("description"))
            .and_then(Value::as_str)
            .and_then(|d| d.lines().map(str::trim).find(|l| !l.is_empty()));
        if let Some(description) = description {
            label.append(
                &format!("  {description}"),
                Some(theme_style(console, "schema.description").into()),
            );
        }
        let mut tree = Tree::new(label);
        if note.is_some() {
            return tree;
        }
        if depth >= self.max_depth {
            tree.add(Text::styled(
                "…",
                theme_style(console, "schema.description"),
            ));
            return tree;
        }
        let pushed = followed.len();
        stack.extend(followed.iter().map(|r| r.to_string()));
        self.children(resolver, target, depth, stack, console, &mut tree);
        if !std::ptr::eq(target, schema) {
            // Keywords beside the `$ref` (draft 2019-09 and later).
            self.children(resolver, schema, depth, stack, console, &mut tree);
        }
        stack.truncate(stack.len() - pushed);
        tree
    }

    fn children(
        &self,
        resolver: &Resolver<'_>,
        schema: &Value,
        depth: usize,
        stack: &mut Vec<String>,
        console: &Console,
        tree: &mut Tree,
    ) {
        let required = required_names(schema);
        let next = depth + 1;
        if let Some(Value::Object(properties)) = schema.get("properties") {
            for (name, property) in properties {
                let child = self.node(
                    resolver,
                    property,
                    name,
                    required.contains(name),
                    next,
                    stack,
                    console,
                );
                tree.add_tree(child);
            }
        }
        if let Some(Value::Object(patterns)) = schema.get("patternProperties") {
            for (pattern, property) in patterns {
                let child = self.node(
                    resolver,
                    property,
                    &format!("/{pattern}/"),
                    false,
                    next,
                    stack,
                    console,
                );
                tree.add_tree(child);
            }
        }
        if let Some(additional @ Value::Object(_)) = schema.get("additionalProperties") {
            let child = self.node(
                resolver,
                additional,
                "[other properties]",
                false,
                next,
                stack,
                console,
            );
            tree.add_tree(child);
        }
        let tuple = schema
            .get("prefixItems")
            .or_else(|| schema.get("items").filter(|items| items.is_array()));
        if let Some(Value::Array(items)) = tuple {
            for (index, item) in items.iter().enumerate() {
                let child = self.node(
                    resolver,
                    item,
                    &format!("[{index}]"),
                    false,
                    next,
                    stack,
                    console,
                );
                tree.add_tree(child);
            }
        }
        if let Some(items) = schema.get("items").filter(|items| !items.is_array()) {
            let child = self.node(resolver, items, "[items]", false, next, stack, console);
            tree.add_tree(child);
        }
        for (keyword, title) in [
            ("oneOf", "one of"),
            ("anyOf", "any of"),
            ("allOf", "all of"),
        ] {
            if let Some(Value::Array(branches)) = schema.get(keyword) {
                let mut group =
                    Tree::new(Text::styled(title, theme_style(console, "schema.branch")));
                for (index, branch) in branches.iter().enumerate() {
                    let name = branch
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| format!("[{}] {title}", index + 1))
                        .unwrap_or_else(|| format!("[{}]", index + 1));
                    group.add_tree(self.node(resolver, branch, &name, false, next, stack, console));
                }
                // A schema that is only the branches already says "one of"
                // on its own line: the branches go straight under it.
                if type_of(schema) == title {
                    for branch in std::mem::take(group.children_mut()) {
                        tree.add_tree(branch);
                    }
                } else {
                    tree.add_tree(group);
                }
            }
        }
        for keyword in ["not", "if", "then", "else"] {
            if let Some(branch) = schema.get(keyword) {
                let mut child = self.node(resolver, branch, keyword, false, next, stack, console);
                child.set_label(self.relabel(resolver, branch, keyword, console));
                tree.add_tree(child);
            }
        }
    }

    /// A `not` / `if` / `then` / `else` branch's label: the keyword styled
    /// as a branch, then the schema's type.
    fn relabel(
        &self,
        resolver: &Resolver<'_>,
        branch: &Value,
        keyword: &str,
        console: &Console,
    ) -> Text {
        let mut label = Text::styled(keyword, theme_style(console, "schema.branch"));
        let target = branch
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| resolver.resolve(r).ok())
            .unwrap_or(branch);
        label.append("  ", None);
        label.append(
            &type_of(target),
            Some(theme_style(console, "schema.type").into()),
        );
        let shown = constraints(target);
        if !shown.is_empty() {
            label.append("  ", None);
            label.append(
                &shown.join(", "),
                Some(theme_style(console, "schema.constraint").into()),
            );
        }
        label
    }
}

impl Renderable for SchemaTree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.tree(console).rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.tree(console).measure(console, options)
    }
}

// --------------------------------------------------------------- the diff

/// What kind of change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

impl ChangeKind {
    /// `+`, `-` or `~`.
    pub fn marker(self) -> &'static str {
        match self {
            ChangeKind::Added => "+",
            ChangeKind::Removed => "-",
            ChangeKind::Changed => "~",
        }
    }
}

/// One difference between two schemas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub kind: ChangeKind,
    /// Where: `(root)`, `user.email`, `tags[]`, `payment.one of[2]`.
    pub path: String,
    /// What changed, in words.
    pub detail: String,
    /// Whether a document the old schema accepted may now be refused.
    pub breaking: bool,
}

/// The differences between two schemas. See the [module docs](self).
///
/// ```
/// use rich_ext::schema::{ChangeKind, SchemaDiff};
///
/// let old = serde_json::json!({"properties": {"id": {"type": "integer"}}});
/// let new = serde_json::json!({
///     "required": ["id"],
///     "properties": {"id": {"type": "string"}, "name": {"type": "string"}}
/// });
/// let diff = SchemaDiff::new(&old, &new);
/// let summary: Vec<(ChangeKind, &str, &str, bool)> = diff
///     .changes()
///     .iter()
///     .map(|c| (c.kind, c.path.as_str(), c.detail.as_str(), c.breaking))
///     .collect();
/// assert_eq!(
///     summary,
///     [
///         (ChangeKind::Changed, "id", "became required", true),
///         (ChangeKind::Changed, "id", "type integer → string", true),
///         (ChangeKind::Added, "name", "property added (string)", false),
///     ]
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaDiff {
    changes: Vec<Change>,
    old_name: String,
    new_name: String,
}

impl SchemaDiff {
    pub fn new(old: &Value, new: &Value) -> Self {
        let mut differ = Differ {
            old: Resolver { root: old },
            new: Resolver { root: new },
            changes: Vec::new(),
            seen: Vec::new(),
        };
        differ.compare(old, new, "", 0);
        SchemaDiff {
            changes: differ.changes,
            old_name: "old".into(),
            new_name: "new".into(),
        }
    }

    /// Name the two versions in the summary line (default `old` and `new`).
    pub fn names(mut self, old: impl Into<String>, new: impl Into<String>) -> Self {
        self.old_name = old.into();
        self.new_name = new.into();
        self
    }

    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// How many changes may refuse a document the old schema accepted.
    pub fn breaking(&self) -> usize {
        self.changes.iter().filter(|c| c.breaking).count()
    }

    fn summary(&self) -> String {
        let count = self.changes.len();
        let breaking = self.breaking();
        format!(
            "{} → {}: {count} change{}, {breaking} breaking",
            self.old_name,
            self.new_name,
            if count == 1 { "" } else { "s" }
        )
    }
}

impl Renderable for SchemaDiff {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.changes.is_empty() {
            let text = Text::new(format!("{} → {}: no changes", self.old_name, self.new_name));
            return text.rich_render(console, options);
        }
        let mut table = Table::new()
            .caption(self.summary())
            .caption_justify(rich::Justify::Left);
        table.add_column("");
        table.add_column("Where");
        table.add_column("Change");
        table.add_column("");
        for change in &self.changes {
            let key = match change.kind {
                ChangeKind::Added => "schema.added",
                ChangeKind::Removed => "schema.removed",
                ChangeKind::Changed => "schema.changed",
            };
            let style = theme_style(console, key);
            let breaking = if change.breaking {
                Text::styled("breaking", theme_style(console, "schema.breaking"))
            } else {
                Text::new("")
            };
            table.add_row_text(vec![
                Text::styled(change.kind.marker(), style.clone()),
                Text::styled(change.path.clone(), theme_style(console, "schema.name")),
                Text::styled(change.detail.clone(), style),
                breaking,
            ]);
        }
        table.rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        if self.changes.is_empty() {
            return Text::new(format!("{} → {}: no changes", self.old_name, self.new_name))
                .measure(console, options);
        }
        let mut table = Table::new();
        table.add_column("");
        table.add_column("Where");
        table.add_column("Change");
        table.add_column("");
        for change in &self.changes {
            let breaking = if change.breaking { "breaking" } else { "" };
            table.add_row(&[change.kind.marker(), &change.path, &change.detail, breaking]);
        }
        table.measure(console, options)
    }
}

struct Differ<'a> {
    old: Resolver<'a>,
    new: Resolver<'a>,
    changes: Vec<Change>,
    /// `$ref` pairs being compared, so a recursive schema ends.
    seen: Vec<(String, String)>,
}

fn join(path: &str, name: &str) -> String {
    let simple = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '$');
    let name = if simple {
        name.to_string()
    } else {
        format!("[{name:?}]")
    };
    if path.is_empty() || name.starts_with('[') {
        format!("{path}{name}")
    } else {
        format!("{path}.{name}")
    }
}

fn shown(path: &str) -> String {
    if path.is_empty() {
        "(root)".into()
    } else {
        path.to_string()
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

impl<'a> Differ<'a> {
    fn push(&mut self, kind: ChangeKind, path: &str, detail: String, breaking: bool) {
        self.changes.push(Change {
            kind,
            path: shown(path),
            detail,
            breaking,
        });
    }

    /// Follow `$ref`s, returning the schema and the last reference followed.
    fn follow(resolver: &Resolver<'a>, mut schema: &'a Value) -> (&'a Value, Option<String>) {
        let mut last = None;
        for _ in 0..DEPTH {
            let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
                break;
            };
            match resolver.resolve(reference) {
                Ok(resolved) if !std::ptr::eq(resolved, schema) => {
                    last = Some(reference.to_string());
                    schema = resolved;
                }
                _ => break,
            }
        }
        (schema, last)
    }

    fn compare(&mut self, old: &'a Value, new: &'a Value, path: &str, depth: usize) {
        if depth > DEPTH {
            return;
        }
        let (old, old_ref) = Self::follow(&self.old, old);
        let (new, new_ref) = Self::follow(&self.new, new);
        if old_ref.is_some() || new_ref.is_some() {
            let pair = (
                old_ref.clone().unwrap_or_default(),
                new_ref.clone().unwrap_or_default(),
            );
            if self.seen.contains(&pair) {
                return;
            }
            self.seen.push(pair);
            self.compare_resolved(old, new, path, depth);
            self.seen.pop();
        } else {
            self.compare_resolved(old, new, path, depth);
        }
    }

    fn compare_resolved(&mut self, old: &'a Value, new: &'a Value, path: &str, depth: usize) {
        if old == new {
            return;
        }
        let (old_type, new_type) = (type_of(old), type_of(new));
        if old_type != new_type {
            // Widening (`string` → `string | null`, anything → `any`) accepts
            // everything it did.
            let widened = new_type == "any"
                || new_type
                    .split(" | ")
                    .collect::<BTreeSet<_>>()
                    .is_superset(&old_type.split(" | ").collect());
            self.push(
                ChangeKind::Changed,
                path,
                format!("type {old_type} → {new_type}"),
                !widened,
            );
        }
        self.compare_enum(old, new, path);
        self.compare_constraints(old, new, path);
        self.compare_properties(old, new, path, depth);
        // Items.
        match (old.get("items"), new.get("items")) {
            (Some(a), Some(b)) if !a.is_array() && !b.is_array() => {
                self.compare(a, b, &format!("{path}[]"), depth + 1)
            }
            (None, Some(b)) if !b.is_array() => self.push(
                ChangeKind::Added,
                &format!("{path}[]"),
                format!("items constrained ({})", type_of(b)),
                true,
            ),
            (Some(a), None) if !a.is_array() => self.push(
                ChangeKind::Removed,
                &format!("{path}[]"),
                "items no longer constrained".into(),
                false,
            ),
            _ => {}
        }
        for (keyword, title) in [
            ("oneOf", "one of"),
            ("anyOf", "any of"),
            ("allOf", "all of"),
        ] {
            let branches = |schema: &'a Value| -> &'a [Value] {
                schema
                    .get(keyword)
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
            };
            let (a, b) = (branches(old), branches(new));
            if a.len() != b.len() {
                let breaking = match keyword {
                    "allOf" => b.len() > a.len(),
                    _ => b.len() < a.len(),
                };
                let detail = match (a.len(), b.len()) {
                    (0, n) => format!("{title}: {n} branches added"),
                    (n, 0) => format!("{title}: {n} branches removed"),
                    (x, y) => format!("{title}: {x} → {y} branches"),
                };
                self.push(ChangeKind::Changed, path, detail, breaking);
            }
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                let at = format!(
                    "{}{title}[{}]",
                    if path.is_empty() {
                        String::new()
                    } else {
                        format!("{path}.")
                    },
                    index + 1
                );
                self.compare(x, y, &at, depth + 1);
            }
        }
    }

    fn compare_enum(&mut self, old: &Value, new: &Value, path: &str) {
        let values = |schema: &Value| -> Option<Vec<String>> {
            schema
                .get("enum")
                .and_then(Value::as_array)
                .map(|values| values.iter().map(|v| v.to_string()).collect())
        };
        match (values(old), values(new)) {
            (Some(a), Some(b)) => {
                let removed: Vec<&String> = a.iter().filter(|v| !b.contains(v)).collect();
                let added: Vec<&String> = b.iter().filter(|v| !a.contains(v)).collect();
                if !added.is_empty() {
                    let list: Vec<&str> = added.iter().map(|s| s.as_str()).collect();
                    self.push(
                        ChangeKind::Added,
                        path,
                        format!("enum value {} added", list.join(", ")),
                        false,
                    );
                }
                if !removed.is_empty() {
                    let list: Vec<&str> = removed.iter().map(|s| s.as_str()).collect();
                    self.push(
                        ChangeKind::Removed,
                        path,
                        format!("enum value {} removed", list.join(", ")),
                        true,
                    );
                }
            }
            (None, Some(b)) => self.push(
                ChangeKind::Added,
                path,
                format!("restricted to {} values", b.len()),
                true,
            ),
            (Some(_), None) => self.push(
                ChangeKind::Removed,
                path,
                "no longer restricted to listed values".into(),
                false,
            ),
            (None, None) => {}
        }
    }

    fn compare_constraints(&mut self, old: &'a Value, new: &'a Value, path: &str) {
        let mut keys: Vec<&str> = CONSTRAINTS.to_vec();
        keys.extend(["const", "additionalProperties"]);
        for key in keys {
            let (a, b) = (old.get(key), new.get(key));
            if a == b {
                continue;
            }
            // `additionalProperties` as a schema is compared as a branch.
            if key == "additionalProperties"
                && (a.is_some_and(Value::is_object) || b.is_some_and(Value::is_object))
            {
                if let (Some(x), Some(y)) = (a, b) {
                    if x.is_object() && y.is_object() {
                        self.compare(x, y, &join(path, "[other properties]"), 0);
                        continue;
                    }
                }
            }
            let informational = matches!(key, "default" | "deprecated" | "readOnly" | "writeOnly");
            let tighter = match key {
                "minLength" | "minimum" | "exclusiveMinimum" | "minItems" | "minContains"
                | "minProperties" => number(b) > number(a),
                "maxLength" | "maximum" | "exclusiveMaximum" | "maxItems" | "maxContains"
                | "maxProperties" => {
                    number(a).is_none()
                        || number(b).is_some_and(|b| number(a).is_some_and(|a| b < a))
                }
                "additionalProperties" => b == Some(&Value::Bool(false)),
                "uniqueItems" => b == Some(&Value::Bool(true)),
                _ => true,
            };
            let name = if key == "additionalProperties" {
                "additional properties"
            } else {
                key
            };
            let show = |v: &Value| match (key, v) {
                ("additionalProperties", Value::Bool(false)) => "refused".to_string(),
                ("additionalProperties", Value::Bool(true)) => "allowed".to_string(),
                ("pattern", Value::String(pattern)) => format!("/{pattern}/"),
                ("format", Value::String(format)) => format.clone(),
                (_, other) => compact(other),
            };
            match (a, b) {
                (None, Some(b)) => self.push(
                    ChangeKind::Added,
                    path,
                    format!("{name} {} added", show(b)),
                    !informational && tighter,
                ),
                (Some(a), None) => self.push(
                    ChangeKind::Removed,
                    path,
                    format!("{name} {} removed", show(a)),
                    false,
                ),
                (Some(a), Some(b)) => self.push(
                    ChangeKind::Changed,
                    path,
                    format!("{name} {} → {}", show(a), show(b)),
                    !informational && tighter,
                ),
                (None, None) => {}
            }
        }
    }

    fn compare_properties(&mut self, old: &'a Value, new: &'a Value, path: &str, depth: usize) {
        let (was, now) = (required_names(old), required_names(new));
        for name in now.difference(&was) {
            self.push(
                ChangeKind::Changed,
                &join(path, name),
                "became required".into(),
                true,
            );
        }
        for name in was.difference(&now) {
            self.push(
                ChangeKind::Changed,
                &join(path, name),
                "no longer required".into(),
                false,
            );
        }
        let old_props = old.get("properties").and_then(Value::as_object);
        let new_props = new.get("properties").and_then(Value::as_object);
        for (name, schema) in old_props.into_iter().flatten() {
            if new_props.is_none_or(|props| !props.contains_key(name)) {
                let (resolved, _) = Self::follow(&self.old, schema);
                self.push(
                    ChangeKind::Removed,
                    &join(path, name),
                    format!("property removed ({})", type_of(resolved)),
                    true,
                );
            }
        }
        for (name, schema) in new_props.into_iter().flatten() {
            match old_props.and_then(|props| props.get(name)) {
                Some(before) => self.compare(before, schema, &join(path, name), depth + 1),
                None => {
                    let (resolved, _) = Self::follow(&self.new, schema);
                    self.push(
                        ChangeKind::Added,
                        &join(path, name),
                        format!("property added ({})", type_of(resolved)),
                        false,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn render(r: &dyn Renderable, width: usize) -> String {
        Console::builder()
            .width(width)
            .color_system(None)
            .build()
            .render_to_string(r)
    }

    #[test]
    fn refs_resolve_and_cycles_stop() {
        let schema = json!({
            "$defs": {
                "node": {
                    "type": "object",
                    "properties": {
                        "value": {"type": "string"},
                        "next": {"$ref": "#/$defs/node"}
                    }
                }
            },
            "$ref": "#/$defs/node"
        });
        assert_eq!(
            render(&SchemaTree::new(schema), 80),
            "schema  object  → #/$defs/node\n\
             ├── value  string\n\
             └── next  object  → #/$defs/node (recursive)"
        );
    }

    #[test]
    fn branches_items_and_external_refs() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {"type": "array", "items": {"type": "string", "minLength": 1}, "uniqueItems": true},
                "pay": {"oneOf": [{"title": "Card", "type": "object"}, {"type": "null"}]},
                "other": {"$ref": "other.json#/x"},
                "missing": {"$ref": "#/nope"}
            }
        });
        assert_eq!(
            render(&SchemaTree::new(schema), 80),
            "schema  object\n\
             ├── tags  array  unique items\n\
             │   └── [items]  string  minLength=1\n\
             ├── pay  one of\n\
             │   ├── [1] Card  object\n\
             │   └── [2]  null\n\
             ├── other  → other.json#/x (another document)\n\
             └── missing  → #/nope (not found)"
        );
    }

    #[test]
    fn boolean_schemas_and_anchors() {
        let schema = json!({
            "properties": {
                "anything": true,
                "nothing": false,
                "anchored": {"$ref": "#thing"}
            },
            "$defs": {"t": {"$anchor": "thing", "type": "integer"}}
        });
        let out = render(&SchemaTree::new(schema), 80);
        assert!(out.contains("anything  any"), "{out}");
        assert!(out.contains("nothing  never"), "{out}");
        assert!(out.contains("anchored  integer  → #thing"), "{out}");
    }

    #[test]
    fn diff_reports_each_kind() {
        let old = json!({
            "type": "object",
            "required": ["id"],
            "properties": {
                "id": {"type": "integer"},
                "kind": {"enum": ["a", "b"]},
                "name": {"type": "string", "maxLength": 10},
                "gone": {"type": "boolean"}
            }
        });
        let new = json!({
            "type": "object",
            "required": ["id", "name"],
            "properties": {
                "id": {"type": ["integer", "null"]},
                "kind": {"enum": ["a", "c"]},
                "name": {"type": "string", "maxLength": 5},
                "fresh": {"type": "number"}
            }
        });
        let diff = SchemaDiff::new(&old, &new);
        let lines: Vec<String> = diff
            .changes()
            .iter()
            .map(|c| {
                format!(
                    "{} {} {}{}",
                    c.kind.marker(),
                    c.path,
                    c.detail,
                    if c.breaking { " !" } else { "" }
                )
            })
            .collect();
        assert_eq!(
            lines,
            [
                "~ name became required !",
                "- gone property removed (boolean) !",
                "~ id type integer → integer | null",
                "+ kind enum value \"c\" added",
                "- kind enum value \"b\" removed !",
                "~ name maxLength 10 → 5 !",
                "+ fresh property added (number)",
            ]
        );
        assert_eq!(diff.breaking(), 4);
        let out = render(&diff.names("v1", "v2"), 80);
        assert!(out.contains("v1 → v2: 7 changes, 4 breaking"), "{out}");
        assert!(out.contains("breaking"), "{out}");
    }

    #[test]
    fn recursive_schemas_diff_without_looping() {
        let schema = |extra: &str| {
            json!({
                "$defs": {"node": {"properties": {
                    "next": {"$ref": "#/$defs/node"},
                    extra: {"type": "string"}
                }}},
                "$ref": "#/$defs/node"
            })
        };
        let diff = SchemaDiff::new(&schema("a"), &schema("b"));
        assert_eq!(diff.changes().len(), 2, "{:?}", diff.changes());
        assert!(SchemaDiff::new(&schema("a"), &schema("a")).is_empty());
        assert_eq!(
            render(&SchemaDiff::new(&schema("a"), &schema("a")), 40),
            "old → new: no changes"
        );
    }

    #[test]
    fn parse_refuses_non_schemas() {
        assert!(parse("[1]").is_err());
        assert!(parse("{").is_err());
        assert!(parse("true").is_ok());
    }
}
