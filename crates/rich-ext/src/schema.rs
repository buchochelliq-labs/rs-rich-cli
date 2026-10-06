//! Schemas drawn as trees (#244), what changed between two (#268), and how
//! a schema evolved (#347), for JSON Schema, SQL DDL, Arrow and anything
//! else in the format-neutral [`model`].
//!
//! [`SchemaTree`] draws a schema as a [`Tree`] of its fields: each with its
//! type, a `(required)` marker, its constraints (`minLength=1`,
//! `format=email`, `one of "a", "b"`, `primary key`, `→ users.id`, …) and
//! the first line of its description. For JSON Schema, array items,
//! `patternProperties`, `additionalProperties`, `oneOf` / `anyOf` / `allOf`
//! branches, `not` and `if` / `then` / `else` are branches of their own. A
//! `$ref` within the document (`#/$defs/…`, `#/definitions/…`, any JSON
//! pointer, or a `$anchor`) is resolved and drawn in place; one that refers
//! back to a schema it is already inside is marked `(recursive)` instead of
//! drawn again, and one to another document is shown as a reference.
//!
//! [`SchemaDiff`] compares two versions: fields added and removed, type
//! changes, fields that became (or stopped being) required, enum values,
//! constraints and keys, each marked `+`, `-` or `~`, and `breaking` where a
//! document (or row) the old schema accepted may now be refused.
//! [`SchemaDiff::new`] compares two JSON Schemas, following their `$ref`s;
//! [`SchemaDiff::models`] compares any two [`Schema`]s, so DDL and Arrow
//! versions (or one of each) diff the same way. [`SchemaTimeline`] lays a
//! series of versions on a [`Timeline`](crate::chart::Timeline), each
//! marked with what changed.
//!
//! Every view reads the model: [`json`] maps JSON Schema into it, [`sql`]
//! reads a `CREATE TABLE` subset, and `rs-rich-data` maps Arrow schemas
//! (behind its `arrow` feature).
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

use std::borrow::Cow;
use std::fmt;

use rich::table::Table;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text, Tree};
use serde_json::Value;

mod diff;
pub mod json;
pub mod model;
pub mod sql;
mod timeline;

pub use model::{
    Composition, Constraint, DataType, Field, FieldKind, ForeignKey, Literal, Schema, Unexpanded,
};
pub use timeline::SchemaTimeline;

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

/// The most levels drawn or compared below the root.
const DEPTH: usize = 32;

/// The most entries a [`SchemaTree`] draws. Definitions shared through
/// `$ref`s are drawn in place wherever they are used, so a small schema can
/// name exponentially many paths; past this the tree ends with a note.
pub const MAX_ENTRIES: usize = 10_000;

/// The most changes a [`SchemaDiff`] lists, and the most pairs of differing
/// schemas it compares, before it stops and says so.
pub const MAX_CHANGES: usize = 10_000;
pub const MAX_COMPARISONS: usize = 100_000;

// --------------------------------------------------------------- the tree

/// What a [`SchemaTree`] draws.
#[derive(Clone, Debug)]
enum Source {
    Json(Value),
    Model(Schema),
}

/// A schema drawn as a tree. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct SchemaTree {
    source: Source,
    title: Option<String>,
    max_depth: usize,
}

impl SchemaTree {
    /// A JSON Schema's tree.
    pub fn new(schema: Value) -> Self {
        SchemaTree {
            source: Source::Json(schema),
            title: None,
            max_depth: DEPTH,
        }
    }

    /// Read a JSON Schema from JSON text ([`parse`]).
    pub fn from_json(json: &str) -> Result<Self, SchemaError> {
        parse(json).map(SchemaTree::new)
    }

    /// A model schema's tree: its fields, then each of its tables with its
    /// columns.
    pub fn from_model(schema: Schema) -> Self {
        SchemaTree {
            source: Source::Model(schema),
            title: None,
            max_depth: DEPTH,
        }
    }

    /// The root's name, over the schema's own (its `title`, or its name;
    /// default `schema`).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Draw at most this many levels below the root (default 32).
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// The JSON Schema, for a tree made from one.
    pub fn schema(&self) -> Option<&Value> {
        match &self.source {
            Source::Json(value) => Some(value),
            Source::Model(_) => None,
        }
    }

    /// The model schema, for a tree made from one.
    pub fn model(&self) -> Option<&Schema> {
        match &self.source {
            Source::Json(_) => None,
            Source::Model(schema) => Some(schema),
        }
    }

    /// The root field the tree draws, and whether it stopped at
    /// [`MAX_ENTRIES`].
    pub fn root(&self) -> (Field, bool) {
        match &self.source {
            Source::Json(value) => json::to_field(value, self.title.as_deref(), self.max_depth),
            Source::Model(schema) => {
                let mut root = model_root(schema, self.title.as_deref());
                // The root is the first entry, as in the JSON Schema walk.
                let mut budget = MAX_ENTRIES - 1;
                let cut = limit(&mut root, &mut budget);
                (root, cut)
            }
        }
    }

    /// Build the [`Tree`] this renders as.
    pub fn tree(&self, console: &Console) -> Tree {
        let (root, truncated) = self.root();
        let mut tree = draw(&root, 0, self.max_depth, console);
        let truncated = truncated || self.model().is_some_and(Schema::is_truncated);
        if truncated {
            tree.add(Text::styled(
                format!("… (the tree stops at {MAX_ENTRIES} entries)"),
                theme_style(console, "schema.description"),
            ));
        }
        tree
    }
}

/// A model schema as the root field a tree draws: its fields, then its
/// tables (each a [`diff::record`]).
fn model_root(schema: &Schema, title: Option<&str>) -> Field {
    let name = title.or(schema.name()).unwrap_or("schema");
    let plural = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
    let label = match (schema.len(), schema.tables().len()) {
        (0, tables) if tables > 0 => plural(tables, "table"),
        (fields, 0) => plural(fields, "field"),
        (fields, tables) => format!("{}, {}", plural(fields, "field"), plural(tables, "table")),
    };
    let mut root = diff::record(schema, name).with_native_type(label);
    if let DataType::Struct(children) = root.data_type_mut() {
        for table in schema.tables() {
            let name = table.name().unwrap_or("table");
            children.push(diff::record(table, name));
        }
    }
    root
}

/// Cut a model tree to the entries `budget` allows below `field` (which is
/// already counted), as the JSON Schema walk stops at [`MAX_ENTRIES`]: a
/// struct keeps the fields that fit, and a list or map whose children do
/// not all fit is elided. Whether anything was cut.
fn limit(field: &mut Field, budget: &mut usize) -> bool {
    let mut cut = false;
    let mut elide = false;
    match field.data_type_mut() {
        DataType::Struct(children) => {
            let mut keep = 0;
            for child in children.iter_mut() {
                if *budget == 0 {
                    cut = true;
                    break;
                }
                *budget -= 1;
                keep += 1;
                cut |= limit(child, budget);
            }
            children.truncate(keep);
        }
        DataType::List(item) => {
            if *budget == 0 {
                elide = true;
            } else {
                *budget -= 1;
                cut |= limit(item, budget);
            }
        }
        DataType::Map { key, value } => {
            if *budget < 2 {
                elide = true;
            } else {
                *budget -= 2;
                cut |= limit(key, budget);
                cut |= limit(value, budget);
            }
        }
        _ => {}
    }
    if elide {
        let taken = std::mem::replace(field, Field::new("", DataType::Any));
        *field = taken.elided(true);
        return true;
    }
    cut
}

/// One field's line, as the tree writes it.
fn label(field: &Field, console: &Console) -> Text {
    let constraint_text = |field: &Field| -> Vec<String> {
        let mut shown: Vec<String> = field
            .constraints()
            .iter()
            .map(ToString::to_string)
            .filter(|text| !text.is_empty())
            .collect();
        shown.extend(
            field
                .metadata()
                .iter()
                .map(|(key, value)| format!("{key}={}", Literal::new(value.as_str()).short())),
        );
        shown
    };
    match field.kind() {
        FieldKind::Group(_) => {
            return Text::styled(field.name(), theme_style(console, "schema.branch"));
        }
        FieldKind::Condition => {
            // The keyword, then the type and constraints of the schema it
            // names.
            let mut label = Text::styled(field.name(), theme_style(console, "schema.branch"));
            label.append("  ", None);
            label.append(
                &field.type_label(),
                Some(theme_style(console, "schema.type").into()),
            );
            let shown = constraint_text(field);
            if !shown.is_empty() {
                label.append("  ", None);
                label.append(
                    &shown.join(", "),
                    Some(theme_style(console, "schema.constraint").into()),
                );
            }
            return label;
        }
        _ => {}
    }
    let mut label = Text::new("");
    label.append(
        field.name(),
        Some(theme_style(console, "schema.name").into()),
    );
    if field.is_required() {
        label.append(
            " (required)",
            Some(theme_style(console, "schema.required").into()),
        );
    }
    let recursive = matches!(field.unexpanded(), Some(Unexpanded::Recursive(_)));
    let unresolved = matches!(field.unexpanded(), Some(Unexpanded::Unresolved { .. }));
    if !unresolved {
        label.append("  ", None);
        label.append(
            &field.type_label(),
            Some(theme_style(console, "schema.type").into()),
        );
    }
    let shown = constraint_text(field);
    if !shown.is_empty() && !recursive {
        label.append("  ", None);
        label.append(
            &shown.join(", "),
            Some(theme_style(console, "schema.constraint").into()),
        );
    }
    if let Some(reference) = field.reference() {
        label.append(
            &format!("  → {reference}"),
            Some(theme_style(console, "schema.ref").into()),
        );
    }
    if let Some(unexpanded) = field.unexpanded() {
        label.append("  ", None);
        label.append(
            &unexpanded.to_string(),
            Some(theme_style(console, "schema.ref").into()),
        );
    }
    let description = field
        .description()
        .and_then(|d| d.lines().map(str::trim).find(|l| !l.is_empty()));
    if let Some(description) = description {
        label.append(
            &format!("  {description}"),
            Some(theme_style(console, "schema.description").into()),
        );
    }
    label
}

/// A field and everything under it, `depth` levels below the root.
fn draw(field: &Field, depth: usize, max_depth: usize, console: &Console) -> Tree {
    let mut tree = Tree::new(label(field, console));
    if field.unexpanded().is_some() {
        return tree;
    }
    let has_children = field.children().next().is_some();
    if field.is_elided() || (has_children && depth >= max_depth) {
        tree.add(Text::styled(
            "…",
            theme_style(console, "schema.description"),
        ));
        return tree;
    }
    for child in field.children() {
        // A group is a heading: its branches are at its own level.
        let next = match child.kind() {
            FieldKind::Group(_) => depth,
            _ => depth + 1,
        };
        tree.add_tree(draw(child, next, max_depth, console));
    }
    tree
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

    /// The theme key a change of this kind is drawn with.
    pub fn style_key(self) -> &'static str {
        match self {
            ChangeKind::Added => "schema.added",
            ChangeKind::Removed => "schema.removed",
            ChangeKind::Changed => "schema.changed",
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
///
/// Any two model schemas compare the same way, here two versions of a SQL
/// table:
///
/// ```
/// use rich_ext::schema::{sql, SchemaDiff};
///
/// let old = sql::parse("CREATE TABLE users (id INT PRIMARY KEY, name TEXT);").unwrap();
/// let new = sql::parse(
///     "CREATE TABLE users (id BIGINT PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE);",
/// )
/// .unwrap();
/// let diff = SchemaDiff::models(&old.schema, &new.schema);
/// let lines: Vec<String> = diff
///     .changes()
///     .iter()
///     .map(|c| format!("{} {} {}", c.kind.marker(), c.path, c.detail))
///     .collect();
/// assert_eq!(
///     lines,
///     [
///         "~ users.name became required",
///         "~ users.id type INT → BIGINT",
///         "+ users.email field added (TEXT)",
///     ]
/// );
/// assert_eq!(diff.breaking(), 2);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaDiff {
    changes: Vec<Change>,
    truncated: bool,
    old_name: String,
    new_name: String,
}

impl SchemaDiff {
    /// Compare two JSON Schemas.
    pub fn new(old: &Value, new: &Value) -> Self {
        let mut differ = diff::Differ::new();
        let a = json::JsonNode::new(json::Resolver { root: old }, Cow::Borrowed(old));
        let b = json::JsonNode::new(json::Resolver { root: new }, Cow::Borrowed(new));
        differ.compare(&a, &b, "", 0);
        SchemaDiff::from_differ(differ)
    }

    /// Compare two model schemas: their fields (a table's columns, a
    /// record's fields, nested ones by path), then their tables by name.
    /// A table added or removed is one change; a field removed is breaking,
    /// since rows that have it no longer fit.
    pub fn models(old: &Schema, new: &Schema) -> Self {
        let mut differ = diff::Differ::new();
        diff::compare_schemas(&mut differ, old, new);
        if old.is_truncated() || new.is_truncated() {
            differ.truncated = true;
        }
        SchemaDiff::from_differ(differ)
    }

    fn from_differ(differ: diff::Differ) -> Self {
        SchemaDiff {
            changes: differ.changes,
            truncated: differ.truncated,
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

    /// Whether the comparison stopped early, at its budget, so more may
    /// differ than [`SchemaDiff::changes`] lists.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// What the summary adds when the comparison stopped early.
    fn stopped(&self) -> String {
        if self.truncated {
            format!(
                " (stopped after {MAX_CHANGES} changes or {MAX_COMPARISONS} comparisons: \
                 more may differ)"
            )
        } else {
            String::new()
        }
    }

    /// How many changes may refuse a document the old schema accepted.
    pub fn breaking(&self) -> usize {
        self.changes.iter().filter(|c| c.breaking).count()
    }

    /// The summary line: `old → new: 3 changes, 1 breaking`, or
    /// `old → new: no changes`.
    pub fn summary(&self) -> String {
        if self.changes.is_empty() {
            return format!(
                "{} → {}: no changes{}",
                self.old_name,
                self.new_name,
                self.stopped()
            );
        }
        let count = self.changes.len();
        let breaking = self.breaking();
        format!(
            "{} → {}: {count} change{}, {breaking} breaking{}",
            self.old_name,
            self.new_name,
            if count == 1 { "" } else { "s" },
            self.stopped()
        )
    }
}

impl Renderable for SchemaDiff {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.changes.is_empty() {
            let text = Text::new(format!(
                "{} → {}: no changes{}",
                self.old_name,
                self.new_name,
                self.stopped()
            ));
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
            let style = theme_style(console, change.kind.style_key());
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
            return Text::new(format!(
                "{} → {}: no changes{}",
                self.old_name,
                self.new_name,
                self.stopped()
            ))
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
                "- gone property removed (boolean)",
                "~ id type integer → integer | null",
                "+ kind enum value \"c\" added",
                "- kind enum value \"b\" removed !",
                "~ name maxLength 10 → 5 !",
                "+ fresh property added (number)",
            ]
        );
        assert_eq!(diff.breaking(), 3);
        let out = render(&diff.names("v1", "v2"), 80);
        assert!(out.contains("v1 → v2: 7 changes, 3 breaking"), "{out}");
        assert!(out.contains("breaking"), "{out}");
    }

    #[test]
    fn removing_a_property_breaks_only_what_now_refuses_it() {
        let old = json!({"properties": {"gone": {"type": "string"}, "x_tag": {}}});
        let removed = |new: Value| -> Vec<bool> {
            SchemaDiff::new(&old, &new)
                .changes()
                .iter()
                .filter(|c| c.kind == ChangeKind::Removed)
                .map(|c| c.breaking)
                .collect()
        };
        // Absent or `true`: the name is still accepted, as anything.
        assert_eq!(removed(json!({})), [false, false]);
        assert_eq!(
            removed(json!({"additionalProperties": true})),
            [false, false]
        );
        assert_eq!(removed(json!({"additionalProperties": {}})), [false, false]);
        // `false`, or a schema of its own: it may be refused.
        assert_eq!(
            removed(json!({"additionalProperties": false})),
            [true, true]
        );
        assert_eq!(
            removed(json!({"additionalProperties": {"type": "integer"}})),
            [true, true]
        );
        // A matching `patternProperties` schema decides instead.
        assert_eq!(
            removed(json!({
                "patternProperties": {"^x_": true},
                "additionalProperties": false
            })),
            [true, false]
        );
        assert_eq!(
            removed(json!({"patternProperties": {"^x_": {"type": "integer"}}})),
            [false, true]
        );
    }

    #[test]
    fn keywords_beside_a_ref_are_compared() {
        let schema = |max: u64| {
            json!({
                "$defs": {"s": {"type": "string"}},
                "properties": {"name": {"$ref": "#/$defs/s", "maxLength": max}}
            })
        };
        let diff = SchemaDiff::new(&schema(10), &schema(5));
        let details: Vec<(&str, &str, bool)> = diff
            .changes()
            .iter()
            .map(|c| (c.path.as_str(), c.detail.as_str(), c.breaking))
            .collect();
        assert_eq!(details, [("name", "maxLength 10 → 5", true)]);
        // A change in the target is still found through a ref with siblings.
        let mut new = schema(10);
        new["$defs"]["s"]["type"] = json!("integer");
        assert_eq!(SchemaDiff::new(&schema(10), &new).breaking(), 1);
        assert!(SchemaDiff::new(&schema(10), &schema(10)).is_empty());
    }

    #[test]
    fn a_keyword_in_both_the_ref_and_its_target_keeps_both() {
        let schema = |target: u64, sibling: u64| {
            json!({
                "$defs": {"s": {"type": "string", "maxLength": target}},
                "properties": {"name": {"$ref": "#/$defs/s", "maxLength": sibling}}
            })
        };
        // The target tightens under an unchanged sibling: still a change.
        let diff = SchemaDiff::new(&schema(5, 3), &schema(2, 3));
        assert_eq!(diff.breaking(), 1, "{:?}", diff.changes());
        // The sibling tightens under an unchanged target.
        let diff = SchemaDiff::new(&schema(5, 3), &schema(5, 1));
        assert_eq!(diff.breaking(), 1, "{:?}", diff.changes());
        assert!(SchemaDiff::new(&schema(5, 3), &schema(5, 3)).is_empty());
    }

    #[test]
    fn recursive_refs_with_siblings_diff_without_looping() {
        let schema = |extra: &str| {
            json!({
                "$defs": {"node": {"properties": {
                    "next": {"$ref": "#/$defs/node", "description": "the next one"},
                    extra: {"type": "string"}
                }}},
                "$ref": "#/$defs/node",
                "title": "List"
            })
        };
        let diff = SchemaDiff::new(&schema("a"), &schema("b"));
        assert_eq!(diff.changes().len(), 2, "{:?}", diff.changes());
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

    #[test]
    fn model_trees_stop_at_max_entries() {
        let wide = Schema::new(
            (0..MAX_ENTRIES + 5).map(|i| Field::new(format!("f{i}"), DataType::Integer)),
        );
        let (_, truncated) = SchemaTree::from_model(wide.clone()).root();
        assert!(truncated);
        let out = render(&SchemaTree::from_model(wide), 60);
        // The root and MAX_ENTRIES - 1 fields, then the note.
        assert_eq!(out.lines().count(), MAX_ENTRIES + 1);
        assert!(
            out.ends_with("… (the tree stops at 10000 entries)"),
            "{}",
            &out[out.len() - 80..]
        );
        // Under the limit nothing is cut.
        let small = Schema::new([Field::new("a", DataType::list(DataType::Integer))]);
        assert!(!SchemaTree::from_model(small).root().1);
        // A list whose item does not fit is elided, not dropped.
        let deep = Schema::new(
            (0..MAX_ENTRIES - 1)
                .map(|i| Field::new(format!("f{i}"), DataType::list(DataType::Integer))),
        );
        let (root, truncated) = SchemaTree::from_model(deep).root();
        assert!(truncated);
        assert!(root.children().last().unwrap().is_elided());
    }

    #[test]
    fn model_trees_draw_tables_and_stop_at_their_depth() {
        let schema = Schema::new([Field::new(
            "a",
            DataType::Struct(vec![Field::new(
                "b",
                DataType::Struct(vec![Field::new("c", DataType::Integer)]),
            )]),
        )
        .with_description("first\nsecond")]);
        assert_eq!(
            render(&SchemaTree::from_model(schema.clone()), 40),
            "schema  1 field\n\
             └── a  struct  first\n    \
                 └── b  struct\n        \
                     └── c  integer"
        );
        assert_eq!(
            render(&SchemaTree::from_model(schema.clone()).max_depth(1), 40),
            "schema  1 field\n\
             └── a  struct  first\n    \
                 └── …"
        );
        let tree = SchemaTree::from_model(schema.truncated(true));
        assert!(tree.schema().is_none() && tree.model().is_some());
        assert!(render(&tree, 60).ends_with("(the tree stops at 10000 entries)"));
    }
}
