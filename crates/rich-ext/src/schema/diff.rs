//! The comparison behind [`SchemaDiff`](super::SchemaDiff), for any schema
//! in the model.
//!
//! The engine walks two schemas side by side through the [`Node`] trait:
//! the type in words, the [`Constraint`]s by key, the required names, the
//! named fields, an array's items, `oneOf`/`anyOf`/`allOf` branches and the
//! schema for other fields. Model [`Field`]s implement it directly; JSON
//! Schema implements it over the document, following `$ref`s as it goes so
//! shared definitions are compared only where they differ.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

use super::json::CONSTRAINTS;
use super::model::{Composition, Constraint, DataType, Field, FieldKind, Literal, Schema};
use super::{Change, ChangeKind, DEPTH, MAX_CHANGES, MAX_COMPARISONS};

/// Which schema a node is, so a comparison that comes back to the same pair
/// through references ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Identity(usize);

/// Identities for schemas that are not in a document (copies), counted
/// down from the top so they never meet an address.
static FRESH: AtomicUsize = AtomicUsize::new(usize::MAX);

impl Identity {
    pub(crate) fn of(value: &Value) -> Self {
        Identity(value as *const Value as usize)
    }

    pub(crate) fn fresh() -> Self {
        Identity(FRESH.fetch_sub(1, Ordering::Relaxed))
    }
}

/// An array's items: none described, a tuple (not compared), or one schema
/// for every item.
pub(crate) enum Items<N> {
    None,
    Tuple,
    One(N),
}

/// One schema, as the comparison sees it.
pub(crate) trait Node: Sized {
    /// What a named child is called in the change list (`property`).
    fn noun(&self) -> &'static str;
    /// Whether the two are the same schema (nothing to compare).
    fn same(&self, other: &Self) -> bool;
    /// Whether this schema was reached through a reference.
    fn reached_by_reference(&self) -> bool {
        false
    }
    /// Which schema this is, when it was reached through a reference.
    fn identity(&self) -> Identity {
        Identity(0)
    }
    /// The type, in words.
    fn label(&self) -> String;
    /// Whether two types in words are the same type.
    fn same_type(&self, a: &str, b: &str) -> bool {
        a == b
    }
    /// The type in words before following references.
    fn raw_label(&self) -> String {
        self.label()
    }
    /// The constraints.
    fn constraints(&self) -> Vec<Constraint>;
    /// The names of the fields that must be present.
    fn required(&self) -> BTreeSet<String>;
    /// The named fields, in order.
    fn properties(&self) -> Vec<(String, Self)>;
    /// Whether removing field `name` may refuse what this one accepted.
    fn removal_breaks(&self, name: &str) -> bool;
    /// An array's items.
    fn items(&self) -> Items<Self>;
    /// The schema for fields not otherwise listed.
    fn additional(&self) -> Option<Self>;
    /// The branches of a composition.
    fn branches(&self, composition: Composition) -> Vec<Self>;
    /// Whether the value may be null, where the type in words does not say.
    fn nullable(&self) -> Option<bool> {
        None
    }
    /// Whether a value must be present, for a node that knows it itself.
    fn required_flag(&self) -> Option<bool> {
        None
    }
    /// Metadata entries.
    fn metadata(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// The keys compared, in the order changes are listed; keys not here follow
/// in the order they first appear.
fn key_order() -> impl Iterator<Item = &'static str> {
    CONSTRAINTS.iter().copied().chain([
        "const",
        "additionalProperties",
        "primaryKey",
        "unique",
        "references",
    ])
}

/// What a constraint is compared by: its key, with the columns of a key
/// that spans several (a table has one primary key but may have several
/// unique keys and foreign keys).
fn compare_key(constraint: &Constraint) -> String {
    match constraint {
        Constraint::Unique(columns) if !columns.is_empty() => {
            format!("unique ({})", columns.join(", "))
        }
        Constraint::References(key) if !key.columns.is_empty() => {
            format!("references ({})", key.columns.join(", "))
        }
        other => other.key().to_string(),
    }
}

/// Two literals are the same when they are written the same, or are the
/// same JSON value written another way (object keys in another order).
fn same_literal(a: &Literal, b: &Literal) -> bool {
    a == b
        || matches!(
            (
                serde_json::from_str::<Value>(a.as_str()),
                serde_json::from_str::<Value>(b.as_str()),
            ),
            (Ok(x), Ok(y)) if x == y
        )
}

fn same_constraint(a: &Constraint, b: &Constraint) -> bool {
    match (a.literal(), b.literal()) {
        (Some(x), Some(y)) if std::mem::discriminant(a) == std::mem::discriminant(b) => {
            same_literal(x, y)
        }
        _ => a == b,
    }
}

pub(crate) fn join(path: &str, name: &str) -> String {
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

/// A comparison under way.
pub(crate) struct Differ {
    pub(crate) changes: Vec<Change>,
    /// The pairs of schemas references led to that are being compared, so
    /// a recursive schema ends.
    seen: Vec<(Identity, Identity)>,
    /// Comparisons of differing schemas left before the diff stops.
    budget: usize,
    /// Whether it stopped (at the budget, or at [`MAX_CHANGES`]).
    pub(crate) truncated: bool,
}

impl Differ {
    pub(crate) fn new() -> Self {
        Differ {
            changes: Vec::new(),
            seen: Vec::new(),
            budget: MAX_COMPARISONS,
            truncated: false,
        }
    }

    pub(crate) fn push(&mut self, kind: ChangeKind, path: &str, detail: String, breaking: bool) {
        if self.changes.len() >= MAX_CHANGES {
            self.truncated = true;
            return;
        }
        self.changes.push(Change {
            kind,
            path: shown(path),
            detail,
            breaking,
        });
    }

    pub(crate) fn compare<N: Node>(&mut self, old: &N, new: &N, path: &str, depth: usize) {
        if depth > DEPTH {
            return;
        }
        if old.reached_by_reference() || new.reached_by_reference() {
            let pair = (old.identity(), new.identity());
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

    fn compare_resolved<N: Node>(&mut self, old: &N, new: &N, path: &str, depth: usize) {
        if old.same(new) || self.truncated {
            return;
        }
        // Shared definitions are compared wherever they are used, which a
        // small schema can make exponentially many places: stop at a budget.
        if self.budget == 0 {
            self.truncated = true;
            return;
        }
        self.budget -= 1;
        let (old_type, new_type) = (old.label(), new.label());
        if !new.same_type(&old_type, &new_type) {
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
        // Nullability, where the type in words does not carry it (model
        // fields; JSON Schema writes `| null` in the type). A field that also
        // became (or stopped being) required is listed once, as that.
        if let (Some(x), Some(y)) = (old.nullable(), new.nullable()) {
            if x != y && old.required_flag() == new.required_flag() {
                let detail = if y {
                    "became nullable"
                } else {
                    "no longer nullable"
                };
                self.push(ChangeKind::Changed, path, detail.into(), !y);
            }
        }
        let (old_constraints, new_constraints) = (old.constraints(), new.constraints());
        self.compare_enum(&old_constraints, &new_constraints, path);
        self.compare_constraints(old, new, &old_constraints, &new_constraints, path, depth);
        self.compare_metadata(old, new, path);
        self.compare_properties(old, new, path, depth);
        // Items.
        let at = format!("{path}[]");
        match (old.items(), new.items()) {
            (Items::One(a), Items::One(b)) => self.compare(&a, &b, &at, depth + 1),
            (Items::None, Items::One(b)) => self.push(
                ChangeKind::Added,
                &at,
                format!("items constrained ({})", b.raw_label()),
                true,
            ),
            (Items::One(_), Items::None) => self.push(
                ChangeKind::Removed,
                &at,
                "items no longer constrained".into(),
                false,
            ),
            _ => {}
        }
        for composition in Composition::ALL {
            let title = composition.title();
            let (a, b) = (old.branches(composition), new.branches(composition));
            if a.len() != b.len() {
                let breaking = match composition {
                    Composition::AllOf => b.len() > a.len(),
                    _ => b.len() < a.len(),
                };
                let detail = match (a.len(), b.len()) {
                    (0, n) => format!("{title}: {n} branches added"),
                    (n, 0) => format!("{title}: {n} branches removed"),
                    (x, y) => format!("{title}: {x} → {y} branches"),
                };
                self.push(ChangeKind::Changed, path, detail, breaking);
            }
            for (index, (x, y)) in a.iter().zip(&b).enumerate() {
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

    fn compare_enum(&mut self, old: &[Constraint], new: &[Constraint], path: &str) {
        let values = |constraints: &[Constraint]| -> Option<Vec<String>> {
            constraints.iter().find_map(|c| match c {
                Constraint::Enum(values) => {
                    Some(values.iter().map(|v| v.as_str().to_string()).collect())
                }
                _ => None,
            })
        };
        match (values(old), values(new)) {
            (Some(a), Some(b)) => {
                let removed: Vec<&str> = a
                    .iter()
                    .filter(|v| !b.contains(v))
                    .map(String::as_str)
                    .collect();
                let added: Vec<&str> = b
                    .iter()
                    .filter(|v| !a.contains(v))
                    .map(String::as_str)
                    .collect();
                if !added.is_empty() {
                    self.push(
                        ChangeKind::Added,
                        path,
                        format!("enum value {} added", added.join(", ")),
                        false,
                    );
                }
                if !removed.is_empty() {
                    self.push(
                        ChangeKind::Removed,
                        path,
                        format!("enum value {} removed", removed.join(", ")),
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

    fn compare_constraints<N: Node>(
        &mut self,
        old_node: &N,
        new_node: &N,
        old: &[Constraint],
        new: &[Constraint],
        path: &str,
        depth: usize,
    ) {
        let mut keys: Vec<String> = key_order().map(str::to_string).collect();
        let mut listed: HashSet<String> = keys.iter().cloned().collect();
        // Each list's first constraint by key, so a table with thousands of
        // keys is not searched once per key.
        let by_key = |list: &'_ [Constraint]| -> HashMap<String, Constraint> {
            let mut found = HashMap::new();
            for constraint in list {
                found
                    .entry(compare_key(constraint))
                    .or_insert_with(|| constraint.clone());
            }
            found
        };
        let (old_by_key, new_by_key) = (by_key(old), by_key(new));
        for constraint in old.iter().chain(new) {
            let key = compare_key(constraint);
            if key != "enum" && listed.insert(key.clone()) {
                keys.push(key);
            }
        }
        for key in keys {
            let (a, b) = (old_by_key.get(&key).cloned(), new_by_key.get(&key).cloned());
            match (&a, &b) {
                (None, None) => continue,
                (Some(x), Some(y)) if same_constraint(x, y) => continue,
                _ => {}
            }
            // A schema for other fields is compared as a branch.
            if key == "additionalProperties" {
                if let (Some(x), Some(y)) = (old_node.additional(), new_node.additional()) {
                    self.compare(&x, &y, &join(path, "[other properties]"), depth + 1);
                    continue;
                }
            }
            let informational = matches!(
                key.as_str(),
                "default" | "deprecated" | "readOnly" | "writeOnly"
            );
            let number = |c: &Option<Constraint>| {
                c.as_ref()
                    .and_then(|c| c.literal())
                    .and_then(Literal::as_f64)
            };
            let literal_is = |c: &Option<Constraint>, text: &str| {
                c.as_ref()
                    .and_then(|c| c.literal())
                    .is_some_and(|v| v.as_str() == text)
            };
            let tighter = match key.as_str() {
                "minLength" | "minimum" | "exclusiveMinimum" | "minItems" | "minContains"
                | "minProperties" => number(&b) > number(&a),
                "maxLength" | "maximum" | "exclusiveMaximum" | "maxItems" | "maxContains"
                | "maxProperties" => {
                    number(&a).is_none()
                        || number(&b).is_some_and(|b| number(&a).is_some_and(|a| b < a))
                }
                "additionalProperties" => literal_is(&b, "false"),
                "uniqueItems" => literal_is(&b, "true"),
                _ => true,
            };
            // A field's own key or `unique` has no value to write.
            let words = |c: &Constraint| match c {
                Constraint::PrimaryKey(columns) | Constraint::Unique(columns)
                    if columns.is_empty() =>
                {
                    c.name().to_string()
                }
                _ => format!("{} {}", c.name(), c.value_text()),
            };
            match (a, b) {
                (None, Some(b)) => self.push(
                    ChangeKind::Added,
                    path,
                    format!("{} added", words(&b)),
                    !informational && tighter,
                ),
                (Some(a), None) => self.push(
                    ChangeKind::Removed,
                    path,
                    format!("{} removed", words(&a)),
                    false,
                ),
                (Some(a), Some(b)) => self.push(
                    ChangeKind::Changed,
                    path,
                    format!("{} {} → {}", a.name(), a.value_text(), b.value_text()),
                    !informational && tighter,
                ),
                (None, None) => {}
            }
        }
    }

    fn compare_metadata<N: Node>(&mut self, old: &N, new: &N, path: &str) {
        let (a, b) = (old.metadata(), new.metadata());
        let get = |list: &[(String, String)], key: &str| {
            list.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        };
        let mut keys: Vec<&String> = Vec::new();
        for (key, _) in a.iter().chain(&b) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        for key in keys {
            let short = |v: &str| Literal::new(v).short();
            match (get(&a, key), get(&b, key)) {
                (None, Some(v)) => self.push(
                    ChangeKind::Added,
                    path,
                    format!("metadata {key}={} added", short(&v)),
                    false,
                ),
                (Some(v), None) => self.push(
                    ChangeKind::Removed,
                    path,
                    format!("metadata {key}={} removed", short(&v)),
                    false,
                ),
                (Some(x), Some(y)) if x != y => self.push(
                    ChangeKind::Changed,
                    path,
                    format!("metadata {key} {} → {}", short(&x), short(&y)),
                    false,
                ),
                _ => {}
            }
        }
    }

    fn compare_properties<N: Node>(&mut self, old: &N, new: &N, path: &str, depth: usize) {
        let (was, now) = (old.required(), new.required());
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
        let noun = new.noun();
        let old_props = old.properties();
        let new_props = new.properties();
        // By name (the first of a name), so wide tables are not searched once
        // per field.
        let mut old_index: HashMap<&str, usize> = HashMap::new();
        for (index, (name, _)) in old_props.iter().enumerate() {
            old_index.entry(name.as_str()).or_insert(index);
        }
        let new_names: HashSet<&str> = new_props.iter().map(|(n, _)| n.as_str()).collect();
        for (name, schema) in &old_props {
            if !new_names.contains(name.as_str()) {
                self.push(
                    ChangeKind::Removed,
                    &join(path, name),
                    format!("{noun} removed ({})", schema.label()),
                    new.removal_breaks(name),
                );
            }
        }
        for (name, schema) in &new_props {
            match old_index.get(name.as_str()).map(|&i| &old_props[i]) {
                Some((_, before)) => self.compare(before, schema, &join(path, name), depth + 1),
                None => self.push(
                    ChangeKind::Added,
                    &join(path, name),
                    format!("{noun} added ({})", schema.label()),
                    false,
                ),
            }
        }
    }
}

// --------------------------------------------------------------- the model

/// A model field, compared.
pub(crate) struct FieldNode<'a>(pub(crate) &'a Field);

impl<'a> FieldNode<'a> {
    fn kids(&self) -> &'a [Field] {
        match self.0.data_type() {
            DataType::Struct(fields) => fields,
            _ => &[],
        }
    }
}

impl Node for FieldNode<'_> {
    fn noun(&self) -> &'static str {
        "field"
    }

    fn same(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    /// The native type, else the model's; a nested type only by its kind,
    /// since its children are compared one by one.
    fn label(&self) -> String {
        if let Some(native) = self.0.native_type() {
            return native.to_string();
        }
        match self.0.data_type() {
            DataType::List(_) => "list".into(),
            DataType::Map { .. } => "map".into(),
            other => other.to_string(),
        }
    }

    /// SQL's `int` is its `INT`, and JSON Schema's `integer` a SQL
    /// `INTEGER`.
    fn same_type(&self, a: &str, b: &str) -> bool {
        a.eq_ignore_ascii_case(b)
    }

    fn constraints(&self) -> Vec<Constraint> {
        self.0.constraints().to_vec()
    }

    fn required(&self) -> BTreeSet<String> {
        self.properties()
            .into_iter()
            .filter(|(_, f)| f.0.is_required())
            .map(|(name, _)| name)
            .collect()
    }

    fn properties(&self) -> Vec<(String, Self)> {
        let named = |f: &&Field| f.kind() == FieldKind::Field;
        match self.0.data_type() {
            DataType::Map { key, value } => vec![
                (key.name().to_string(), FieldNode(key)),
                (value.name().to_string(), FieldNode(value)),
            ],
            _ => self
                .kids()
                .iter()
                .filter(named)
                .map(|f| (f.name().to_string(), FieldNode(f)))
                .collect(),
        }
    }

    /// A field removed from a table or record type: rows that have it no
    /// longer fit.
    fn removal_breaks(&self, _name: &str) -> bool {
        true
    }

    fn items(&self) -> Items<Self> {
        if let DataType::List(item) = self.0.data_type() {
            return Items::One(FieldNode(item));
        }
        let kids = self.kids();
        if let Some(item) = kids.iter().find(|f| f.kind() == FieldKind::Items) {
            Items::One(FieldNode(item))
        } else if kids.iter().any(|f| f.kind() == FieldKind::TupleItem) {
            Items::Tuple
        } else {
            Items::None
        }
    }

    fn additional(&self) -> Option<Self> {
        self.kids()
            .iter()
            .find(|f| f.kind() == FieldKind::Additional)
            .map(FieldNode)
    }

    fn branches(&self, composition: Composition) -> Vec<Self> {
        let mut out = Vec::new();
        for kid in self.kids() {
            match kid.kind() {
                FieldKind::Group(c) if c == composition => {
                    out.extend(kid.children().map(FieldNode))
                }
                FieldKind::Branch(c) if c == composition => out.push(FieldNode(kid)),
                _ => {}
            }
        }
        out
    }

    fn nullable(&self) -> Option<bool> {
        Some(self.0.is_nullable())
    }

    fn required_flag(&self) -> Option<bool> {
        Some(self.0.is_required())
    }

    fn metadata(&self) -> Vec<(String, String)> {
        self.0.metadata().to_vec()
    }
}

/// A schema's own fields and constraints as one field, as a diff and a
/// tree see a table.
pub(crate) fn record(schema: &Schema, name: &str) -> Field {
    let mut field = Field::new(name, DataType::Struct(schema.fields().to_vec()))
        .nullable(false)
        .with_native_type("table");
    *field.constraints_mut() = schema.constraints().to_vec();
    for (key, value) in schema.metadata() {
        field = field.with_metadata(key, value);
    }
    if let Some(description) = schema.description() {
        field = field.with_description(description);
    }
    field
}

/// A schema's tables by name (the first of a name, as [`Schema::table`]
/// finds), so thousands of tables are not searched once per table.
fn tables_by_name(schema: &Schema) -> HashMap<&str, &Schema> {
    let mut tables = HashMap::new();
    for table in schema.tables() {
        if let Some(name) = table.name() {
            tables.entry(name).or_insert(table);
        }
    }
    tables
}

/// Compare two model schemas: their fields, then their tables by name.
pub(crate) fn compare_schemas(differ: &mut Differ, old: &Schema, new: &Schema) {
    let (a, b) = (record(old, ""), record(new, ""));
    differ.compare(&FieldNode(&a), &FieldNode(&b), "", 0);
    let count = |table: &Schema| {
        let n = table.len();
        format!("{n} field{}", if n == 1 { "" } else { "s" })
    };
    let (old_tables, new_tables) = (tables_by_name(old), tables_by_name(new));
    for table in old.tables() {
        let name = table.name().unwrap_or_default();
        if !new_tables.contains_key(name) {
            differ.push(
                ChangeKind::Removed,
                &join("", name),
                format!("table removed ({})", count(table)),
                true,
            );
        }
    }
    for table in new.tables() {
        let name = table.name().unwrap_or_default();
        match old_tables.get(name) {
            Some(before) => {
                let (a, b) = (record(before, name), record(table, name));
                differ.compare(&FieldNode(&a), &FieldNode(&b), &join("", name), 1);
            }
            None => differ.push(
                ChangeKind::Added,
                &join("", name),
                format!("table added ({})", count(table)),
                false,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ForeignKey;

    fn lines(old: &Schema, new: &Schema) -> Vec<String> {
        let mut differ = Differ::new();
        compare_schemas(&mut differ, old, new);
        differ
            .changes
            .iter()
            .map(|c| {
                let mark = if c.breaking { " !" } else { "" };
                format!("{} {} {}{mark}", c.kind.marker(), c.path, c.detail)
            })
            .collect()
    }

    #[test]
    fn nested_fields_items_and_maps_compare_by_path() {
        let schema = |item_nullable: bool, value: DataType| {
            Schema::new([
                Field::new(
                    "tags",
                    DataType::List(Box::new(
                        Field::new("item", DataType::String).nullable(item_nullable),
                    )),
                ),
                Field::new("scores", DataType::map(DataType::String, value)),
                Field::new(
                    "address",
                    DataType::Struct(vec![Field::new("city", DataType::String)]),
                ),
            ])
        };
        let old = schema(true, DataType::Integer);
        let new = schema(false, DataType::Float);
        assert_eq!(
            lines(&old, &new),
            [
                "~ tags[] no longer nullable !",
                "~ scores.value type integer → float !",
            ]
        );
        assert!(lines(&old, &old).is_empty());
    }

    #[test]
    fn nullability_is_compared_for_every_field() {
        let schema = |nullable: bool| {
            Schema::new([
                Field::new("x", DataType::String).nullable(nullable),
                Field::new(
                    "s",
                    DataType::Struct(vec![Field::new("y", DataType::Integer).nullable(nullable)]),
                ),
                Field::new("m", DataType::map(DataType::String, DataType::Float))
                    .nullable(nullable),
            ])
        };
        assert_eq!(
            lines(&schema(true), &schema(false)),
            [
                "~ x no longer nullable !",
                "~ s.y no longer nullable !",
                "~ m no longer nullable !",
            ]
        );
        assert_eq!(
            lines(&schema(false), &schema(true)),
            [
                "~ x became nullable",
                "~ s.y became nullable",
                "~ m became nullable"
            ]
        );
        // A field that also became required says so once, as required.
        let old = Schema::new([Field::new("x", DataType::String)]);
        let new = Schema::new([Field::new("x", DataType::String)
            .nullable(false)
            .required(true)]);
        assert_eq!(lines(&old, &new), ["~ x became required !"]);
    }

    #[test]
    fn keys_and_tables_compare() {
        let table = |unique: bool, key: Option<ForeignKey>| {
            let mut email = Field::new("email", DataType::String);
            if unique {
                email = email.with_constraint(Constraint::Unique(vec![]));
            }
            if let Some(key) = key {
                email = email.with_constraint(Constraint::References(key));
            }
            Schema::new([email]).named("users")
        };
        let old = Schema::of_tables([table(false, None), Schema::new([]).named("gone")]);
        let new = Schema::of_tables([
            table(true, Some(ForeignKey::to("emails", "address"))),
            Schema::new([Field::new("x", DataType::Integer)]).named("fresh"),
        ]);
        assert_eq!(
            lines(&old, &new),
            [
                "- gone table removed (0 fields) !",
                "+ users.email unique added !",
                "+ users.email references emails.address added !",
                "+ fresh table added (1 field)",
            ]
        );
    }

    /// Tables, fields and keys were each looked up among all the others:
    /// these took minutes.
    #[test]
    fn large_schemas_compare_in_linear_time() {
        use std::time::{Duration, Instant};
        let compare = |old: &Schema, new: &Schema| {
            let started = Instant::now();
            let mut differ = Differ::new();
            compare_schemas(&mut differ, old, new);
            assert!(started.elapsed() < Duration::from_secs(20));
            differ
        };
        let tables =
            |data_type: DataType| {
                Schema::of_tables((0..100_000).map(|i| {
                    Schema::new([Field::new("a", data_type.clone())]).named(format!("t{i}"))
                }))
            };
        let differ = compare(&tables(DataType::Integer), &tables(DataType::Float));
        assert!(differ.truncated);
        assert_eq!(differ.changes.len(), MAX_CHANGES);

        let wide = |data_type: DataType| {
            Schema::of_tables((0..100).map(|t| {
                Schema::new((0..4_000).map(|i| Field::new(format!("c{i}"), data_type.clone())))
                    .named(format!("t{t}"))
            }))
        };
        let differ = compare(&wide(DataType::Integer), &wide(DataType::Float));
        assert_eq!(differ.changes[0].path, "t0.c0");

        let keys = |turn: bool| {
            let mut table =
                Schema::new((0..2_000).map(|i| Field::new(format!("c{i}"), DataType::Integer)));
            for i in 0..2_000 {
                let (a, b) = (format!("c{i}"), format!("c{}", (i + 1) % 2_000));
                let columns = if turn { vec![b, a] } else { vec![a, b] };
                table.constraints_mut().push(Constraint::Unique(columns));
            }
            Schema::of_tables([table.named("t")])
        };
        let differ = compare(&keys(false), &keys(true));
        assert_eq!(differ.changes.len(), 4_000);
        assert_eq!(differ.changes[0].detail, "unique (c0, c1) removed");
    }

    #[test]
    fn literals_compare_as_values() {
        let a = Literal::new(r#"{"a":1,"b":2}"#);
        let b = Literal::new(r#"{"b":2,"a":1}"#);
        assert!(same_literal(&a, &b));
        assert!(!same_literal(&a, &Literal::new("1")));
        assert_eq!(join("", "a b"), r#"["a b"]"#);
        assert_eq!(join("x", "y"), "x.y");
    }
}
