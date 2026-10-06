//! JSON Schema in the format-neutral model.
//!
//! [`to_field`] maps a whole schema into one root [`Field`], the shape
//! [`SchemaTree`](super::SchemaTree) draws: each property a child with its
//! type (as JSON Schema writes it, kept as the [native
//! type](Field::native_type)), `required`, its [`Constraint`]s and its
//! description; `patternProperties`, `additionalProperties`, tuple and array
//! items, `oneOf`/`anyOf`/`allOf` and `not`/`if`/`then`/`else` are children
//! of their own [`FieldKind`]. A `$ref` within the document is followed and
//! drawn in place, and kept as the field's [reference](Field::reference);
//! one that refers back to a schema it is inside, or that cannot be
//! followed, is [`Unexpanded`]. [`to_model`] gives the same as a
//! [`Schema`] of the root's fields.
//!
//! Shared definitions are drawn wherever they are used, so the mapping
//! stops at [`MAX_ENTRIES`] fields (and marks the schema
//! [truncated](Schema::is_truncated)) and at a depth limit.
//!
//! ```
//! use rich_ext::schema::{json, Constraint, DataType};
//!
//! let schema = serde_json::json!({
//!     "title": "User",
//!     "required": ["email"],
//!     "properties": {
//!         "email": {"type": "string", "format": "email"},
//!         "tags": {"type": "array", "items": {"type": "string"}}
//!     }
//! });
//! let model = json::to_model(&schema);
//! assert_eq!(model.name(), Some("User"));
//! let email = model.field("email").unwrap();
//! assert!(email.is_required());
//! assert_eq!(email.data_type(), &DataType::String);
//! assert_eq!(email.constraints(), [Constraint::Format("email".into())]);
//! assert_eq!(model.field("tags").unwrap().data_type().to_string(), "list<string>");
//! ```

use std::borrow::Cow;
use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::diff::{Identity, Items, Node};
use super::model::{
    Composition, Constraint, DataType, Field, FieldKind, Literal, Schema, Unexpanded,
};
use super::{DEPTH, MAX_ENTRIES};

/// The constraint keywords shown and compared, in the order shown.
pub(crate) const CONSTRAINTS: &[&str] = &[
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

// --------------------------------------------------------------- resolving

/// Resolves `$ref`s within one document.
#[derive(Clone, Copy)]
pub(crate) struct Resolver<'a> {
    pub(crate) root: &'a Value,
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
    pub(crate) fn resolve(&self, reference: &str) -> Result<&'a Value, String> {
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

// --------------------------------------------------------------- keywords

/// A schema's type, in words: `string`, `string | null`, `object`, `enum`, …
pub(crate) fn type_of(schema: &Value) -> String {
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

/// The model's type for a JSON Schema type in words, and whether it
/// accepts null. Nested types are set from the children instead.
fn scalar(label: &str) -> (DataType, bool) {
    let names: Vec<&str> = label.split(" | ").collect();
    let nullable = names.contains(&"null") || label == "any";
    let named: Vec<&str> = names.into_iter().filter(|n| *n != "null").collect();
    let data_type = match named.as_slice() {
        ["string"] => DataType::String,
        ["integer"] => DataType::Integer,
        ["number"] => DataType::Float,
        ["boolean"] => DataType::Boolean,
        ["object"] => DataType::Struct(Vec::new()),
        [] | ["never"] => DataType::Unknown,
        _ => DataType::Any,
    };
    (data_type, nullable)
}

/// A schema's constraints, in the order a tree shows them: `const`, `enum`,
/// the [`CONSTRAINTS`] keywords, then `additionalProperties`. Values that
/// show nothing (`uniqueItems: false`) are kept, for comparing.
pub(crate) fn constraints(schema: &Value) -> Vec<Constraint> {
    let mut out = Vec::new();
    if let Some(value) = schema.get("const") {
        out.push(Constraint::Const(Literal::json(value)));
    }
    if let Some(Value::Array(values)) = schema.get("enum") {
        out.push(Constraint::Enum(values.iter().map(Literal::json).collect()));
    }
    for key in CONSTRAINTS {
        let Some(value) = schema.get(*key) else {
            continue;
        };
        let literal = Literal::json(value);
        out.push(match (*key, value) {
            ("format", Value::String(format)) => Constraint::Format(format.clone()),
            ("pattern", Value::String(pattern)) => Constraint::Pattern(pattern.clone()),
            ("minLength", _) => Constraint::MinLength(literal),
            ("maxLength", _) => Constraint::MaxLength(literal),
            ("minimum", _) => Constraint::Minimum(literal),
            ("exclusiveMinimum", _) => Constraint::ExclusiveMinimum(literal),
            ("maximum", _) => Constraint::Maximum(literal),
            ("exclusiveMaximum", _) => Constraint::ExclusiveMaximum(literal),
            ("multipleOf", _) => Constraint::MultipleOf(literal),
            ("minItems", _) => Constraint::MinItems(literal),
            ("maxItems", _) => Constraint::MaxItems(literal),
            ("uniqueItems", _) => Constraint::UniqueItems(literal),
            ("default", _) => Constraint::Default(literal),
            ("deprecated", _) => Constraint::Deprecated(literal),
            ("readOnly", _) => Constraint::ReadOnly(literal),
            ("writeOnly", _) => Constraint::WriteOnly(literal),
            (name, _) => Constraint::Other {
                name: name.to_string(),
                value: literal,
            },
        });
    }
    if let Some(value) = schema.get("additionalProperties") {
        out.push(Constraint::Additional(Literal::json(value)));
    }
    out
}

pub(crate) fn required_names(schema: &Value) -> BTreeSet<String> {
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

// --------------------------------------------------------------- the model

/// A schema in the model, as one root field named `name` (else its
/// `title`, else `schema`), drawing at most `max_depth` levels below the
/// root; and whether it stopped at [`MAX_ENTRIES`].
pub fn to_field(schema: &Value, name: Option<&str>, max_depth: usize) -> (Field, bool) {
    let name = name
        .map(str::to_string)
        .or_else(|| {
            schema
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "schema".into());
    let mut walk = Walk {
        resolver: Resolver { root: schema },
        refs: Vec::new(),
        // The root is a schema every other is inside: `#` is recursive.
        targets: vec![schema as *const Value],
        budget: MAX_ENTRIES,
        truncated: false,
        max_depth,
    };
    let field = walk
        .node(schema, &name, false, 0, FieldKind::Field)
        .unwrap_or_else(|| Field::new(name, DataType::Any));
    (field, walk.truncated)
}

/// A schema in the model: named after its `title`, described by its
/// `description`, its fields the root's children, and its own constraints
/// (`additionalProperties: false`, say) the schema's.
pub fn to_model(schema: &Value) -> Schema {
    let (root, truncated) = to_field(schema, None, DEPTH);
    let fields: Vec<Field> = root.children().cloned().collect();
    let mut model = Schema::new(fields).truncated(truncated);
    if let Some(title) = schema.get("title").and_then(Value::as_str) {
        model = model.named(title);
    }
    if let Some(description) = root.description() {
        model = model.with_description(description);
    }
    *model.constraints_mut() = root.constraints().to_vec();
    model
}

/// A walk building the model: the `$ref`s being drawn (as written, and the
/// schemas they name), and how many fields may still be made.
struct Walk<'a> {
    resolver: Resolver<'a>,
    refs: Vec<String>,
    targets: Vec<*const Value>,
    budget: usize,
    truncated: bool,
    max_depth: usize,
}

impl<'a> Walk<'a> {
    fn node(
        &mut self,
        schema: &'a Value,
        name: &str,
        required: bool,
        depth: usize,
        kind: FieldKind,
    ) -> Option<Field> {
        if self.budget == 0 {
            self.truncated = true;
            return None;
        }
        self.budget -= 1;
        // Follow `$ref`s to the schema they name, guarding against cycles:
        // a reference is recursive when it is spelled like one being drawn,
        // or names (however it is spelled) a schema being drawn.
        let mut target = schema;
        let mut followed = Vec::new();
        let mut followed_targets: Vec<*const Value> = Vec::new();
        let mut unexpanded: Option<Unexpanded> = None;
        while let Some(reference) = target.get("$ref").and_then(Value::as_str) {
            let resolved = self.resolver.resolve(reference);
            let inside = |value: &Value| {
                self.targets
                    .iter()
                    .chain(&followed_targets)
                    .any(|&seen| std::ptr::eq(seen, value))
            };
            if self.refs.iter().any(|seen| seen == reference)
                || followed.contains(&reference)
                || resolved.as_ref().is_ok_and(|value| inside(value))
            {
                unexpanded = Some(Unexpanded::Recursive(reference.to_string()));
                if let Ok(resolved) = resolved {
                    target = resolved;
                }
                break;
            }
            match resolved {
                Ok(resolved) => {
                    followed.push(reference);
                    followed_targets.push(resolved as *const Value);
                    // Sibling keywords next to `$ref` still apply; the
                    // referenced schema's own come first.
                    target = resolved;
                }
                Err(reason) => {
                    unexpanded = Some(Unexpanded::Unresolved {
                        reference: reference.to_string(),
                        reason,
                    });
                    break;
                }
            }
        }
        let label = type_of(target);
        let (data_type, nullable) = scalar(&label);
        let mut field = Field::new(name, data_type)
            .nullable(nullable)
            .required(required)
            .with_kind(kind)
            .with_native_type(label);
        let mut shown = constraints(target);
        if !std::ptr::eq(target, schema) {
            for extra in constraints(schema) {
                let text = extra.to_string();
                let repeated = if text.is_empty() {
                    shown.contains(&extra)
                } else {
                    shown.iter().any(|c| c.to_string() == text)
                };
                if !repeated {
                    shown.push(extra);
                }
            }
        }
        *field.constraints_mut() = shown;
        if let Some(reference) = followed.last() {
            field = field.with_reference(*reference);
        }
        let description = schema
            .get("description")
            .or_else(|| target.get("description"))
            .and_then(Value::as_str);
        if let Some(description) = description {
            field = field.with_description(description);
        }
        if let Some(unexpanded) = unexpanded {
            return Some(field.with_unexpanded(unexpanded));
        }
        if depth >= self.max_depth {
            return Some(field.elided(true));
        }
        let pushed = followed.len();
        self.refs.extend(followed.iter().map(|r| r.to_string()));
        self.targets.extend(followed_targets);
        let mut children = self.children(target, depth);
        if !std::ptr::eq(target, schema) {
            // Keywords beside the `$ref` (draft 2019-09 and later).
            children.extend(self.children(schema, depth));
        }
        self.refs.truncate(self.refs.len() - pushed);
        self.targets.truncate(self.targets.len() - pushed);
        Some(with_children(field, children))
    }

    fn children(&mut self, schema: &'a Value, depth: usize) -> Vec<Field> {
        let mut out = Vec::new();
        let required = required_names(schema);
        let next = depth + 1;
        if let Some(Value::Object(properties)) = schema.get("properties") {
            for (name, property) in properties {
                let required = required.contains(name);
                out.extend(self.node(property, name, required, next, FieldKind::Field));
            }
        }
        if let Some(Value::Object(patterns)) = schema.get("patternProperties") {
            for (pattern, property) in patterns {
                let name = format!("/{pattern}/");
                out.extend(self.node(property, &name, false, next, FieldKind::Pattern));
            }
        }
        if let Some(additional @ Value::Object(_)) = schema.get("additionalProperties") {
            let name = "[other properties]";
            out.extend(self.node(additional, name, false, next, FieldKind::Additional));
        }
        let tuple = schema
            .get("prefixItems")
            .or_else(|| schema.get("items").filter(|items| items.is_array()));
        if let Some(Value::Array(items)) = tuple {
            for (index, item) in items.iter().enumerate() {
                let name = format!("[{index}]");
                out.extend(self.node(item, &name, false, next, FieldKind::TupleItem));
            }
        }
        if let Some(items) = schema.get("items").filter(|items| !items.is_array()) {
            out.extend(self.node(items, "[items]", false, next, FieldKind::Items));
        }
        for composition in Composition::ALL {
            let keyword = keyword(composition);
            if let Some(Value::Array(branches)) = schema.get(keyword) {
                let kind = FieldKind::Branch(composition);
                let mut group = Vec::new();
                for (index, branch) in branches.iter().enumerate() {
                    let name = branch
                        .get("title")
                        .and_then(Value::as_str)
                        .map(|title| format!("[{}] {title}", index + 1))
                        .unwrap_or_else(|| format!("[{}]", index + 1));
                    group.extend(self.node(branch, &name, false, next, kind));
                }
                // A schema that is only the branches already says "one of"
                // on its own line: the branches go straight under it.
                if type_of(schema) == composition.title() {
                    out.extend(group);
                } else {
                    out.push(
                        Field::new(composition.title(), DataType::Struct(group))
                            .with_kind(FieldKind::Group(composition)),
                    );
                }
            }
        }
        for keyword in ["not", "if", "then", "else"] {
            if let Some(branch) = schema.get(keyword) {
                if let Some(mut child) =
                    self.node(branch, keyword, false, next, FieldKind::Condition)
                {
                    // A condition's line is the keyword, then the type and
                    // constraints of the schema it names.
                    let target = branch
                        .get("$ref")
                        .and_then(Value::as_str)
                        .and_then(|r| self.resolver.resolve(r).ok())
                        .unwrap_or(branch);
                    child = child.with_native_type(type_of(target));
                    *child.constraints_mut() = constraints(target);
                    out.push(child);
                }
            }
        }
        out
    }
}

/// The JSON Schema keyword for a composition.
pub(crate) fn keyword(composition: Composition) -> &'static str {
    match composition {
        Composition::OneOf => "oneOf",
        Composition::AnyOf => "anyOf",
        Composition::AllOf => "allOf",
    }
}

/// `field` with `children`: an array's one item makes a list, anything
/// else a struct of them in order.
fn with_children(field: Field, mut children: Vec<Field>) -> Field {
    if children.is_empty() {
        return field;
    }
    if children.len() == 1 && children[0].kind() == FieldKind::Items {
        let item = children.remove(0);
        return field.with_type(DataType::List(Box::new(item)));
    }
    field.with_type(DataType::Struct(children))
}

// --------------------------------------------------------------- diffing

/// A schema compared by [`SchemaDiff`](super::SchemaDiff): its `$ref`s
/// followed, with the keywords beside them laid over the schema they name.
pub(crate) struct JsonNode<'v> {
    resolver: Resolver<'v>,
    raw_label: String,
    value: Cow<'v, Value>,
    followed: bool,
    identity: Identity,
}

impl<'v> JsonNode<'v> {
    pub(crate) fn new(resolver: Resolver<'v>, raw: Cow<'v, Value>) -> Self {
        let raw_label = type_of(&raw);
        let (value, followed, identity) = follow(resolver, raw);
        JsonNode {
            resolver,
            raw_label,
            value,
            followed,
            identity,
        }
    }

    /// A child schema: borrowed when this one is, else copied.
    fn child(&self, value: &'v Value) -> Self {
        JsonNode::new(self.resolver, Cow::Borrowed(value))
    }

    fn children_of(&self, key: &str) -> Vec<(String, Value)> {
        match self.value.get(key) {
            Some(Value::Object(map)) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            _ => Vec::new(),
        }
    }

    /// The child at `get(&value)`, borrowed through the document when the
    /// value is, else an owned copy.
    fn nested(&self, get: impl Fn(&Value) -> Option<&Value>) -> Option<Self> {
        match &self.value {
            Cow::Borrowed(value) => get(value).map(|child| self.child(child)),
            Cow::Owned(value) => {
                get(value).map(|child| JsonNode::new(self.resolver, Cow::Owned(child.clone())))
            }
        }
    }

    fn list(&self, get: impl Fn(&Value) -> Option<&Vec<Value>>) -> Vec<Self> {
        match &self.value {
            Cow::Borrowed(value) => get(value)
                .map(|items| items.iter().map(|item| self.child(item)).collect())
                .unwrap_or_default(),
            Cow::Owned(value) => get(value)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| JsonNode::new(self.resolver, Cow::Owned(item.clone())))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// Follow `$ref`s, returning the schema, whether a reference was followed,
/// and the identity of the schema it named. Keywords beside a `$ref` (draft
/// 2019-09 and later) still apply, so they are laid over the schema it
/// names, the outermost last.
fn follow<'v>(resolver: Resolver<'v>, schema: Cow<'v, Value>) -> (Cow<'v, Value>, bool, Identity) {
    let own = match &schema {
        Cow::Borrowed(value) => Identity::of(value),
        Cow::Owned(_) => Identity::fresh(),
    };
    let Some(first) = schema.get("$ref").and_then(Value::as_str) else {
        return (schema, false, own);
    };
    let resolved = match resolver.resolve(first) {
        Ok(resolved) if !std::ptr::eq(resolved, schema.as_ref()) => resolved,
        _ => return (schema, false, own),
    };
    let mut siblings: Vec<Map<String, Value>> = Vec::new();
    if let Value::Object(map) = schema.as_ref() {
        if map.len() > 1 {
            siblings.push(map.clone());
        }
    }
    let mut target: &'v Value = resolved;
    for _ in 1..DEPTH {
        let Some(reference) = target.get("$ref").and_then(Value::as_str) else {
            break;
        };
        match resolver.resolve(reference) {
            Ok(resolved) if !std::ptr::eq(resolved, target) => {
                if let Value::Object(map) = target {
                    if map.len() > 1 {
                        siblings.push(map.clone());
                    }
                }
                target = resolved;
            }
            _ => break,
        }
    }
    let named = Identity::of(target);
    let mut merged = match target {
        _ if siblings.is_empty() => return (Cow::Borrowed(target), true, named),
        Value::Object(map) => map.clone(),
        Value::Bool(true) => Map::new(),
        _ => return (Cow::Borrowed(target), true, named),
    };
    // Siblings apply alongside the target, not instead of it: a keyword
    // both set keeps the target's value, and the sibling's goes in an
    // `allOf` branch, so a change to either one is still seen.
    let mut both = Map::new();
    for map in siblings.into_iter().rev() {
        for (key, value) in map.into_iter().filter(|(key, _)| key != "$ref") {
            match merged.get(&key) {
                Some(existing) if existing != &value => {
                    both.insert(key, value);
                }
                _ => {
                    merged.insert(key, value);
                }
            }
        }
    }
    if !both.is_empty() {
        let all_of = merged
            .entry("allOf")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(branches) = all_of {
            branches.push(Value::Object(both));
        } else {
            *all_of = Value::Array(vec![Value::Object(both)]);
        }
    }
    (Cow::Owned(Value::Object(merged)), true, named)
}

impl Node for JsonNode<'_> {
    fn noun(&self) -> &'static str {
        "property"
    }

    fn same(&self, other: &Self) -> bool {
        self.value == other.value
    }

    fn reached_by_reference(&self) -> bool {
        self.followed
    }

    fn identity(&self) -> Identity {
        self.identity
    }

    fn label(&self) -> String {
        type_of(&self.value)
    }

    fn raw_label(&self) -> String {
        self.raw_label.clone()
    }

    fn constraints(&self) -> Vec<Constraint> {
        constraints(&self.value)
    }

    fn required(&self) -> BTreeSet<String> {
        required_names(&self.value)
    }

    fn properties(&self) -> Vec<(String, Self)> {
        match &self.value {
            Cow::Borrowed(value) => match value.get("properties") {
                Some(Value::Object(map)) => map
                    .iter()
                    .map(|(name, child)| (name.clone(), self.child(child)))
                    .collect(),
                _ => Vec::new(),
            },
            Cow::Owned(_) => self
                .children_of("properties")
                .into_iter()
                .map(|(name, child)| (name, JsonNode::new(self.resolver, Cow::Owned(child))))
                .collect(),
        }
    }

    /// Whether a document with property `name` that the old schema accepted
    /// may be refused now that this one no longer lists it: by a
    /// `patternProperties` schema it matches, else by `additionalProperties`.
    /// Only `true` and `{}` are taken to accept whatever the old one did.
    fn removal_breaks(&self, name: &str) -> bool {
        let new = self.value.as_ref();
        let accepts_all = |schema: &Value| {
            schema == &Value::Bool(true) || schema.as_object().is_some_and(|map| map.is_empty())
        };
        let matching: Vec<&Value> = new
            .get("patternProperties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(pattern, _)| {
                // A pattern that does not compile might match: assume it does.
                fancy_regex::Regex::new(pattern)
                    .map_or(true, |regex| regex.is_match(name).unwrap_or(true))
            })
            .map(|(_, schema)| schema)
            .collect();
        if !matching.is_empty() {
            return !matching.into_iter().all(accepts_all);
        }
        new.get("additionalProperties")
            .is_some_and(|schema| !accepts_all(schema))
    }

    fn items(&self) -> Items<Self> {
        match self.value.get("items") {
            None => Items::None,
            Some(Value::Array(_)) => Items::Tuple,
            Some(_) => self
                .nested(|value| value.get("items"))
                .map_or(Items::None, Items::One),
        }
    }

    fn additional(&self) -> Option<Self> {
        self.nested(|value| value.get("additionalProperties").filter(|v| v.is_object()))
    }

    fn branches(&self, composition: Composition) -> Vec<Self> {
        self.list(|value| value.get(keyword(composition)).and_then(Value::as_array))
    }
}
