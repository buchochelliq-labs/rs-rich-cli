//! The format-neutral schema model: named fields with a type, nullability,
//! constraints and children (0.0.16 workstreams 1 and 4).
//!
//! This is the shape every schema source maps into, so tabular data, JSON
//! Schema, Arrow and SQL DDL can be shown and compared the same way. A
//! [`Schema`] is a list of [`Field`]s (and, for a SQL file, further named
//! tables); a field's [`DataType`] carries its children (a struct's
//! fields, a list's item, a map's key and value). Detail a format has and
//! the model does not, such as Arrow's `Int32` or SQL's `VARCHAR(20)`, is
//! kept as the field's [native type](Field::native_type), as written.
//!
//! Beside its type a field has [`Constraint`]s: required and nullable,
//! enum values, bounds, length, pattern, format, a default, primary and
//! unique keys and a [`ForeignKey`] to `table.column`. JSON Schema's own
//! structure (pattern-named fields, tuple items, `oneOf` branches,
//! references) is kept with the [`FieldKind`] of each child and the
//! [reference](Field::reference) a type was found through, so
//! [`SchemaTree`](super::SchemaTree) draws any of them from the model.
//!
//! ```
//! use rich_ext::schema::{Constraint, DataType, Field, ForeignKey, Schema};
//!
//! let schema = Schema::new([
//!     Field::new("id", DataType::Integer)
//!         .nullable(false)
//!         .required(true)
//!         .with_constraint(Constraint::PrimaryKey(vec![])),
//!     Field::new("email", DataType::String).with_constraint(Constraint::Unique(vec![])),
//!     Field::new("team", DataType::Integer)
//!         .with_constraint(Constraint::References(ForeignKey::to("teams", "id"))),
//!     Field::new("tags", DataType::list(DataType::String)),
//!     Field::new(
//!         "address",
//!         DataType::Struct(vec![
//!             Field::new("city", DataType::String),
//!             Field::new("zip", DataType::String).with_native_type("char(5)"),
//!         ]),
//!     ),
//! ])
//! .named("users");
//! assert_eq!(schema.index_of("email"), Some(1));
//! assert_eq!(schema.primary_key(), ["id"]);
//! assert_eq!(schema.foreign_keys()[0].to_string(), "team → teams.id");
//! assert_eq!(schema.fields()[3].data_type().to_string(), "list<string>");
//! let address = schema.field("address").unwrap();
//! let children: Vec<&str> = address.children().map(|f| f.name()).collect();
//! assert_eq!(children, ["city", "zip"]);
//! ```

use std::fmt;

/// A field's logical type.
///
/// Scalars are the types tabular formats share; [`List`](DataType::List),
/// [`Struct`](DataType::Struct) and [`Map`](DataType::Map) nest.
/// [`Any`](DataType::Any) is a type the source declares to accept anything
/// (JSON Schema's `{}`); [`Unknown`](DataType::Unknown) is one the source did
/// not say or a reader could not tell (an empty column, a type a mapping does
/// not know).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DataType {
    /// Accepts any value.
    Any,
    /// Not known.
    Unknown,
    /// `true` or `false`.
    Boolean,
    /// A whole number, of any width or signedness.
    Integer,
    /// A binary floating-point number.
    Float,
    /// A fixed-point decimal, with its precision (total digits) and scale
    /// (digits after the point) when the source gives them.
    Decimal {
        /// Total digits.
        precision: Option<u32>,
        /// Digits after the decimal point.
        scale: Option<i32>,
    },
    /// Text.
    String,
    /// Bytes.
    Binary,
    /// A calendar date.
    Date,
    /// A date and time, with its time zone when the source gives one.
    Timestamp {
        /// The time zone, as the source writes it (`UTC`, `+01:00`).
        timezone: Option<String>,
    },
    /// A sequence of items, each described by the field.
    List(Box<Field>),
    /// Named fields.
    Struct(Vec<Field>),
    /// Keys to values.
    Map {
        /// The key field.
        key: Box<Field>,
        /// The value field.
        value: Box<Field>,
    },
}

impl DataType {
    /// A list of nullable `item`s, under the conventional name `item`.
    pub fn list(item: DataType) -> Self {
        DataType::List(Box::new(Field::new("item", item)))
    }

    /// A map from non-null `key`s to nullable `value`s, under the
    /// conventional names `key` and `value`.
    pub fn map(key: DataType, value: DataType) -> Self {
        DataType::Map {
            key: Box::new(Field::new("key", key).nullable(false)),
            value: Box::new(Field::new("value", value)),
        }
    }

    /// A timestamp without a time zone.
    pub fn timestamp() -> Self {
        DataType::Timestamp { timezone: None }
    }

    /// A decimal without a stated precision or scale.
    pub fn decimal() -> Self {
        DataType::Decimal {
            precision: None,
            scale: None,
        }
    }

    /// Whether values of this type are numbers (integer, float or decimal).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            DataType::Integer | DataType::Float | DataType::Decimal { .. }
        )
    }

    /// Whether this type nests other fields (list, struct or map).
    pub fn is_nested(&self) -> bool {
        matches!(
            self,
            DataType::List(_) | DataType::Struct(_) | DataType::Map { .. }
        )
    }

    /// The fields this type nests: a struct's fields, a list's item, a map's
    /// key and value. Empty for scalars.
    pub fn children(&self) -> impl Iterator<Item = &Field> {
        let (slice, pair): (&[Field], [Option<&Field>; 2]) = match self {
            DataType::Struct(fields) => (fields, [None, None]),
            DataType::List(item) => (&[], [Some(item), None]),
            DataType::Map { key, value } => (&[], [Some(key), Some(value)]),
            _ => (&[], [None, None]),
        };
        slice.iter().chain(pair.into_iter().flatten())
    }
}

/// The lowercase name: `integer`, `decimal(10,2)`, `timestamp[UTC]`,
/// `list<string>`, `map<string, integer>`, `struct`.
impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Any => f.write_str("any"),
            DataType::Unknown => f.write_str("unknown"),
            DataType::Boolean => f.write_str("boolean"),
            DataType::Integer => f.write_str("integer"),
            DataType::Float => f.write_str("float"),
            DataType::Decimal { precision, scale } => match (precision, scale) {
                (Some(p), Some(s)) => write!(f, "decimal({p},{s})"),
                (Some(p), None) => write!(f, "decimal({p})"),
                _ => f.write_str("decimal"),
            },
            DataType::String => f.write_str("string"),
            DataType::Binary => f.write_str("binary"),
            DataType::Date => f.write_str("date"),
            DataType::Timestamp { timezone: None } => f.write_str("timestamp"),
            DataType::Timestamp { timezone: Some(tz) } => write!(f, "timestamp[{tz}]"),
            DataType::List(item) => write!(f, "list<{}>", item.data_type),
            DataType::Struct(_) => f.write_str("struct"),
            DataType::Map { key, value } => {
                write!(f, "map<{}, {}>", key.data_type, value.data_type)
            }
        }
    }
}

/// A value as the source writes it: JSON (`"a"`, `5`, `true`), or a SQL
/// literal (`'a'`, `CURRENT_TIMESTAMP`).
///
/// Kept as text so every format's values compare and show the same way;
/// [`as_f64`](Literal::as_f64) reads a number back for comparisons.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Literal(String);

impl Literal {
    /// A literal written `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Literal(text.into())
    }

    /// A JSON value, written compactly.
    pub fn json(value: &serde_json::Value) -> Self {
        Literal(value.to_string())
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The value as a number, when it is written as one.
    pub fn as_f64(&self) -> Option<f64> {
        let text = self.0.trim();
        // Rust reads `inf` and `NaN` as numbers; no format here writes one so.
        let numeric = !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'));
        if numeric {
            text.parse().ok()
        } else {
            None
        }
    }

    /// The text, cut to 40 characters with `…`.
    pub fn short(&self) -> String {
        if self.0.chars().count() > 40 {
            let cut: String = self.0.chars().take(39).collect();
            format!("{cut}…")
        } else {
            self.0.clone()
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A foreign key: columns of one table that refer to columns of another.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ForeignKey {
    /// This table's columns; empty on a column's own constraint, where the
    /// column is the one.
    pub columns: Vec<String>,
    /// The table referred to.
    pub table: String,
    /// Its columns; empty when the source names only the table (its
    /// primary key).
    pub references: Vec<String>,
}

impl ForeignKey {
    /// A column's reference to `table.column`.
    pub fn to(table: impl Into<String>, column: impl Into<String>) -> Self {
        ForeignKey {
            columns: Vec::new(),
            table: table.into(),
            references: vec![column.into()],
        }
    }
}

/// `users.id`, or `(a, b) → t.(x, y)` with this table's columns.
impl fmt::Display for ForeignKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |names: &[String]| match names {
            [one] => one.clone(),
            many => format!("({})", many.join(", ")),
        };
        if !self.columns.is_empty() {
            write!(f, "{} → ", list(&self.columns))?;
        }
        if self.references.is_empty() {
            f.write_str(&self.table)
        } else {
            write!(f, "{}.{}", self.table, list(&self.references))
        }
    }
}

/// A rule a field's values follow, beside its type.
///
/// Each has a [key](Constraint::key) two versions are compared by, and
/// shows itself as the schema tree writes it ([`Display`](fmt::Display),
/// empty for a value that says nothing, such as `uniqueItems: false`).
/// Bounds and flags keep the source's [`Literal`], so `minimum: 1.5` and
/// `minimum: 2` compare as numbers and show as written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Constraint {
    /// Exactly this value (`= 5`).
    Const(Literal),
    /// One of these values.
    Enum(Vec<Literal>),
    /// A named format (`email`, `date`).
    Format(String),
    /// A regular expression the text matches.
    Pattern(String),
    /// The fewest characters.
    MinLength(Literal),
    /// The most characters.
    MaxLength(Literal),
    /// The smallest value.
    Minimum(Literal),
    /// Values are above this.
    ExclusiveMinimum(Literal),
    /// The largest value.
    Maximum(Literal),
    /// Values are below this.
    ExclusiveMaximum(Literal),
    /// Values are a multiple of this.
    MultipleOf(Literal),
    /// The fewest items.
    MinItems(Literal),
    /// The most items.
    MaxItems(Literal),
    /// Items are distinct (shown when `true`).
    UniqueItems(Literal),
    /// The value used when none is given.
    Default(Literal),
    /// The field is deprecated (shown when `true`).
    Deprecated(Literal),
    /// The field is read-only (shown when `true`).
    ReadOnly(Literal),
    /// The field is write-only (shown when `true`).
    WriteOnly(Literal),
    /// Whether fields other than those listed are allowed: `false` refuses
    /// them and `true` allows them; a schema (JSON Schema's
    /// `additionalProperties: {…}`) describes them, and is also a child of
    /// kind [`FieldKind::Additional`].
    Additional(Literal),
    /// The primary key: on a field, the field itself (no names); on a
    /// table, the key's columns.
    PrimaryKey(Vec<String>),
    /// Unique values: on a field, its own (no names); on a table, across
    /// the named columns.
    Unique(Vec<String>),
    /// Values refer to another table's.
    References(ForeignKey),
    /// Anything else, by the source's own keyword (`minContains=1`).
    Other {
        /// The source's keyword.
        name: String,
        /// Its value.
        value: Literal,
    },
}

impl Constraint {
    /// The key two versions are compared by: the JSON Schema keyword where
    /// there is one (`minLength`, `additionalProperties`), else a name of
    /// its own (`primaryKey`, `unique`, `references`).
    pub fn key(&self) -> &str {
        match self {
            Constraint::Const(_) => "const",
            Constraint::Enum(_) => "enum",
            Constraint::Format(_) => "format",
            Constraint::Pattern(_) => "pattern",
            Constraint::MinLength(_) => "minLength",
            Constraint::MaxLength(_) => "maxLength",
            Constraint::Minimum(_) => "minimum",
            Constraint::ExclusiveMinimum(_) => "exclusiveMinimum",
            Constraint::Maximum(_) => "maximum",
            Constraint::ExclusiveMaximum(_) => "exclusiveMaximum",
            Constraint::MultipleOf(_) => "multipleOf",
            Constraint::MinItems(_) => "minItems",
            Constraint::MaxItems(_) => "maxItems",
            Constraint::UniqueItems(_) => "uniqueItems",
            Constraint::Default(_) => "default",
            Constraint::Deprecated(_) => "deprecated",
            Constraint::ReadOnly(_) => "readOnly",
            Constraint::WriteOnly(_) => "writeOnly",
            Constraint::Additional(_) => "additionalProperties",
            Constraint::PrimaryKey(_) => "primaryKey",
            Constraint::Unique(_) => "unique",
            Constraint::References(_) => "references",
            Constraint::Other { name, .. } => name,
        }
    }

    /// The literal a bound or flag holds.
    pub fn literal(&self) -> Option<&Literal> {
        match self {
            Constraint::Const(v)
            | Constraint::MinLength(v)
            | Constraint::MaxLength(v)
            | Constraint::Minimum(v)
            | Constraint::ExclusiveMinimum(v)
            | Constraint::Maximum(v)
            | Constraint::ExclusiveMaximum(v)
            | Constraint::MultipleOf(v)
            | Constraint::MinItems(v)
            | Constraint::MaxItems(v)
            | Constraint::UniqueItems(v)
            | Constraint::Default(v)
            | Constraint::Deprecated(v)
            | Constraint::ReadOnly(v)
            | Constraint::WriteOnly(v)
            | Constraint::Additional(v)
            | Constraint::Other { value: v, .. } => Some(v),
            _ => None,
        }
    }

    /// The constraint's name as a change list writes it (`maxLength`,
    /// `additional properties`, `primary key`).
    pub fn name(&self) -> &str {
        match self {
            Constraint::Additional(_) => "additional properties",
            Constraint::PrimaryKey(_) => "primary key",
            other => other.key(),
        }
    }

    /// The value as a change list writes it: `5`, `/^a/`, `email`,
    /// `refused`, `users.id`; empty for a field's own key or `unique`.
    pub fn value_text(&self) -> String {
        match self {
            Constraint::Format(format) => format.clone(),
            Constraint::Pattern(pattern) => format!("/{pattern}/"),
            Constraint::Enum(values) => values
                .iter()
                .map(Literal::short)
                .collect::<Vec<_>>()
                .join(", "),
            Constraint::Additional(v) => match v.as_str() {
                "false" => "refused".into(),
                "true" => "allowed".into(),
                _ => v.short(),
            },
            Constraint::PrimaryKey(columns) | Constraint::Unique(columns) => {
                if columns.is_empty() {
                    String::new()
                } else {
                    format!("({})", columns.join(", "))
                }
            }
            Constraint::References(key) => key.to_string(),
            other => other.literal().map(Literal::short).unwrap_or_default(),
        }
    }
}

/// As the schema tree writes it: `minLength=1`, `pattern=/^a/`,
/// `one of "a", "b"`, `primary key`, `→ users.id`. Empty when there is
/// nothing to show (`uniqueItems: false`).
impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let flag = |v: &Literal| v.as_str() == "true";
        match self {
            Constraint::Const(v) => write!(f, "= {}", v.short()),
            Constraint::Enum(values) => {
                let shown: Vec<String> = values.iter().take(8).map(Literal::short).collect();
                write!(f, "one of {}", shown.join(", "))?;
                if values.len() > 8 {
                    write!(f, ", … {} more", values.len() - 8)?;
                }
                Ok(())
            }
            Constraint::Format(format) => write!(f, "format={format}"),
            Constraint::Pattern(pattern) => write!(f, "pattern=/{pattern}/"),
            Constraint::UniqueItems(v) if flag(v) => f.write_str("unique items"),
            Constraint::Deprecated(v) if flag(v) => f.write_str("deprecated"),
            Constraint::ReadOnly(v) if flag(v) => f.write_str("read-only"),
            Constraint::WriteOnly(v) if flag(v) => f.write_str("write-only"),
            Constraint::UniqueItems(_)
            | Constraint::Deprecated(_)
            | Constraint::ReadOnly(_)
            | Constraint::WriteOnly(_) => Ok(()),
            Constraint::Additional(v) if v.as_str() == "false" => {
                f.write_str("no other properties")
            }
            Constraint::Additional(_) => Ok(()),
            Constraint::PrimaryKey(columns) if columns.is_empty() => f.write_str("primary key"),
            Constraint::PrimaryKey(columns) => write!(f, "primary key ({})", columns.join(", ")),
            Constraint::Unique(columns) if columns.is_empty() => f.write_str("unique"),
            Constraint::Unique(columns) => write!(f, "unique ({})", columns.join(", ")),
            Constraint::References(key) => write!(f, "→ {key}"),
            other => write!(
                f,
                "{}={}",
                other.key(),
                other.literal().map(Literal::short).unwrap_or_default()
            ),
        }
    }
}

/// The composition a branch belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Composition {
    /// Exactly one branch holds.
    OneOf,
    /// At least one holds.
    AnyOf,
    /// All hold.
    AllOf,
}

impl Composition {
    /// All three, in the order a tree shows them.
    pub const ALL: [Composition; 3] = [Composition::OneOf, Composition::AnyOf, Composition::AllOf];

    /// `one of`, `any of`, `all of`.
    pub fn title(self) -> &'static str {
        match self {
            Composition::OneOf => "one of",
            Composition::AnyOf => "any of",
            Composition::AllOf => "all of",
        }
    }
}

/// How a nested field relates to the field it is under.
///
/// Most are plain [`Field`](FieldKind::Field)s: a struct's fields, a list's
/// item, a map's key and value. JSON Schema adds the rest: fields named by
/// a pattern, the schema for any other field, tuple and array items,
/// `oneOf`/`anyOf`/`allOf` branches (in a [`Group`](FieldKind::Group),
/// unless the parent is nothing but its branches), and `not`, `if`, `then`
/// and `else`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FieldKind {
    /// A named field.
    #[default]
    Field,
    /// Fields whose names match a pattern (the field's name, `/^x_/`).
    Pattern,
    /// Any field not otherwise listed (`[other properties]`).
    Additional,
    /// One position of a tuple (`[0]`).
    TupleItem,
    /// Every item of an array (`[items]`).
    Items,
    /// A group of branches (`one of`), whose children are the branches.
    Group(Composition),
    /// One branch (`[1] Card`).
    Branch(Composition),
    /// A condition, named by its keyword (`not`, `if`, `then`, `else`).
    Condition,
}

/// Why a type named by a reference is not drawn in place.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unexpanded {
    /// The reference names a type the field is already inside.
    Recursive(String),
    /// The reference could not be followed.
    Unresolved {
        /// The reference, as written.
        reference: String,
        /// Why (`another document`, `not found`).
        reason: String,
    },
}

impl Unexpanded {
    /// The reference, as written.
    pub fn reference(&self) -> &str {
        match self {
            Unexpanded::Recursive(reference) => reference,
            Unexpanded::Unresolved { reference, .. } => reference,
        }
    }
}

/// `→ #/$defs/node (recursive)`, `→ other.json (another document)`.
impl fmt::Display for Unexpanded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unexpanded::Recursive(reference) => write!(f, "→ {reference} (recursive)"),
            Unexpanded::Unresolved { reference, reason } => write!(f, "→ {reference} ({reason})"),
        }
    }
}

/// One named field: a type, whether it may be null and whether it must be
/// present, its constraints, and optionally the source's own name for the
/// type, a description and metadata.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Field {
    name: String,
    data_type: DataType,
    nullable: bool,
    required: bool,
    native_type: Option<String>,
    description: Option<String>,
    constraints: Vec<Constraint>,
    metadata: Vec<(String, String)>,
    kind: FieldKind,
    reference: Option<String>,
    unexpanded: Option<Unexpanded>,
    elided: bool,
}

impl Field {
    /// A nullable, optional field.
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Field {
            name: name.into(),
            data_type,
            nullable: true,
            required: false,
            native_type: None,
            description: None,
            constraints: Vec::new(),
            metadata: Vec::new(),
            kind: FieldKind::Field,
            reference: None,
            unexpanded: None,
            elided: false,
        }
    }

    /// Whether the field may be null (default `true`).
    pub fn nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }

    /// Whether a value must be present (default `false`): JSON Schema's
    /// `required`, SQL's `NOT NULL`, a non-nullable Arrow field.
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// The source format's own name for the type, kept as written
    /// (`Int32`, `VARCHAR(20)`, `string | null`). A schema tree shows it in
    /// place of the model's type.
    pub fn with_native_type(mut self, native: impl Into<String>) -> Self {
        self.native_type = Some(native.into());
        self
    }

    /// A description, as the source gives it.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Replace the type.
    pub fn with_type(mut self, data_type: DataType) -> Self {
        self.data_type = data_type;
        self
    }

    /// Add a constraint.
    pub fn with_constraint(mut self, constraint: Constraint) -> Self {
        self.constraints.push(constraint);
        self
    }

    /// Add a metadata entry (Arrow's field metadata).
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.push((key.into(), value.into()));
        self
    }

    /// How the field relates to its parent (default [`FieldKind::Field`]).
    pub fn with_kind(mut self, kind: FieldKind) -> Self {
        self.kind = kind;
        self
    }

    /// The named definition the type was found through (`#/$defs/item`).
    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }

    /// Mark the type as named by a reference that is not drawn in place.
    pub fn with_unexpanded(mut self, unexpanded: Unexpanded) -> Self {
        self.unexpanded = Some(unexpanded);
        self
    }

    /// Mark the children as left out, at a depth limit.
    pub fn elided(mut self, elided: bool) -> Self {
        self.elided = elided;
        self
    }

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The type.
    pub fn data_type(&self) -> &DataType {
        &self.data_type
    }

    /// The type, to change in place.
    pub fn data_type_mut(&mut self) -> &mut DataType {
        &mut self.data_type
    }

    /// Whether the field may be null.
    pub fn is_nullable(&self) -> bool {
        self.nullable
    }

    /// Whether a value must be present.
    pub fn is_required(&self) -> bool {
        self.required
    }

    /// The source's own type name, if one was recorded.
    pub fn native_type(&self) -> Option<&str> {
        self.native_type.as_deref()
    }

    /// The type as a tree shows it: the native type, else the model's.
    pub fn type_label(&self) -> String {
        self.native_type
            .clone()
            .unwrap_or_else(|| self.data_type.to_string())
    }

    /// The description, if any.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The constraints, in the order the source gives them.
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// The constraints, to change in place.
    pub fn constraints_mut(&mut self) -> &mut Vec<Constraint> {
        &mut self.constraints
    }

    /// The first constraint with `key` ([`Constraint::key`]).
    pub fn constraint(&self, key: &str) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.key() == key)
    }

    /// The metadata entries, in order.
    pub fn metadata(&self) -> &[(String, String)] {
        &self.metadata
    }

    /// The listed values, when the field is an enum.
    pub fn enum_values(&self) -> Option<&[Literal]> {
        self.constraints.iter().find_map(|c| match c {
            Constraint::Enum(values) => Some(values.as_slice()),
            _ => None,
        })
    }

    /// The default value, if any.
    pub fn default_value(&self) -> Option<&Literal> {
        self.constraints.iter().find_map(|c| match c {
            Constraint::Default(value) => Some(value),
            _ => None,
        })
    }

    /// Whether the field is (part of) the primary key.
    pub fn is_primary_key(&self) -> bool {
        self.constraints
            .iter()
            .any(|c| matches!(c, Constraint::PrimaryKey(_)))
    }

    /// Whether the field's values are unique on their own.
    pub fn is_unique(&self) -> bool {
        self.constraints
            .iter()
            .any(|c| matches!(c, Constraint::Unique(columns) if columns.is_empty()))
    }

    /// What the field refers to, if it is a foreign key.
    pub fn references(&self) -> Option<&ForeignKey> {
        self.constraints.iter().find_map(|c| match c {
            Constraint::References(key) => Some(key),
            _ => None,
        })
    }

    /// How the field relates to its parent.
    pub fn kind(&self) -> FieldKind {
        self.kind
    }

    /// The named definition the type was found through.
    pub fn reference(&self) -> Option<&str> {
        self.reference.as_deref()
    }

    /// Why the type is not drawn in place, when it is not.
    pub fn unexpanded(&self) -> Option<&Unexpanded> {
        self.unexpanded.as_ref()
    }

    /// Whether the children were left out at a depth limit.
    pub fn is_elided(&self) -> bool {
        self.elided
    }

    /// The nested fields ([`DataType::children`]).
    pub fn children(&self) -> impl Iterator<Item = &Field> {
        self.data_type.children()
    }
}

/// An ordered list of top-level fields, optionally named (a table, a
/// record type, a document), and the further tables a schema of several
/// holds (SQL DDL's `CREATE TABLE`s).
///
/// Keys that span columns (a composite primary key, `UNIQUE (a, b)`, a
/// `FOREIGN KEY`) are the schema's own [constraints](Schema::constraints);
/// a key on one column is that field's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Schema {
    name: Option<String>,
    description: Option<String>,
    fields: Vec<Field>,
    constraints: Vec<Constraint>,
    tables: Vec<Schema>,
    metadata: Vec<(String, String)>,
    truncated: bool,
}

impl Schema {
    /// A schema of `fields`, in order.
    pub fn new(fields: impl IntoIterator<Item = Field>) -> Self {
        Schema {
            fields: fields.into_iter().collect(),
            ..Schema::default()
        }
    }

    /// A schema of `tables`, with no fields of its own.
    pub fn of_tables(tables: impl IntoIterator<Item = Schema>) -> Self {
        Schema {
            tables: tables.into_iter().collect(),
            ..Schema::default()
        }
    }

    /// Name the schema (a table or type name).
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Describe the schema.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add a table-level constraint.
    pub fn with_constraint(mut self, constraint: Constraint) -> Self {
        self.constraints.push(constraint);
        self
    }

    /// Add a metadata entry (Arrow's schema metadata).
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.push((key.into(), value.into()));
        self
    }

    /// Mark the schema as cut short by a reader's limit.
    pub fn truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }

    /// The name, if any.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The description, if any.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The top-level fields, in order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The top-level fields, to change in place.
    pub fn fields_mut(&mut self) -> &mut Vec<Field> {
        &mut self.fields
    }

    /// Append a field.
    pub fn push(&mut self, field: Field) -> &mut Self {
        self.fields.push(field);
        self
    }

    /// The first top-level field called `name`.
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// The position of the first top-level field called `name`.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// The number of top-level fields.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether there are no fields.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The table-level constraints.
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// The table-level constraints, to change in place.
    pub fn constraints_mut(&mut self) -> &mut Vec<Constraint> {
        &mut self.constraints
    }

    /// The primary key's columns: the table's key when it spans columns,
    /// else every field marked as one.
    pub fn primary_key(&self) -> Vec<&str> {
        let table = self.constraints.iter().find_map(|c| match c {
            Constraint::PrimaryKey(columns) if !columns.is_empty() => Some(columns),
            _ => None,
        });
        match table {
            Some(columns) => columns.iter().map(String::as_str).collect(),
            None => self
                .fields
                .iter()
                .filter(|f| f.is_primary_key())
                .map(Field::name)
                .collect(),
        }
    }

    /// Every foreign key: the table's, then each column's (with the column
    /// filled in).
    pub fn foreign_keys(&self) -> Vec<ForeignKey> {
        let table = self.constraints.iter().filter_map(|c| match c {
            Constraint::References(key) => Some(key.clone()),
            _ => None,
        });
        let columns = self.fields.iter().filter_map(|f| {
            f.references().map(|key| ForeignKey {
                columns: vec![f.name.clone()],
                ..key.clone()
            })
        });
        table.chain(columns).collect()
    }

    /// The further tables the schema holds.
    pub fn tables(&self) -> &[Schema] {
        &self.tables
    }

    /// The further tables, to change in place.
    pub fn tables_mut(&mut self) -> &mut Vec<Schema> {
        &mut self.tables
    }

    /// Add a table.
    pub fn push_table(&mut self, table: Schema) -> &mut Self {
        self.tables.push(table);
        self
    }

    /// The table called `name`.
    pub fn table(&self, name: &str) -> Option<&Schema> {
        self.tables.iter().find(|t| t.name() == Some(name))
    }

    /// The metadata entries, in order.
    pub fn metadata(&self) -> &[(String, String)] {
        &self.metadata
    }

    /// Whether a reader's limit cut the schema short.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }
}

impl FromIterator<Field> for Schema {
    fn from_iter<I: IntoIterator<Item = Field>>(iter: I) -> Self {
        Schema::new(iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_display_with_their_parameters() {
        let cases = [
            (DataType::Any, "any"),
            (DataType::Unknown, "unknown"),
            (DataType::Boolean, "boolean"),
            (DataType::Float, "float"),
            (DataType::decimal(), "decimal"),
            (
                DataType::Decimal {
                    precision: Some(10),
                    scale: Some(2),
                },
                "decimal(10,2)",
            ),
            (
                DataType::Decimal {
                    precision: Some(38),
                    scale: None,
                },
                "decimal(38)",
            ),
            (DataType::Binary, "binary"),
            (DataType::Date, "date"),
            (DataType::timestamp(), "timestamp"),
            (
                DataType::Timestamp {
                    timezone: Some("UTC".into()),
                },
                "timestamp[UTC]",
            ),
            (
                DataType::list(DataType::list(DataType::Integer)),
                "list<list<integer>>",
            ),
            (
                DataType::map(DataType::String, DataType::Float),
                "map<string, float>",
            ),
            (DataType::Struct(vec![]), "struct"),
        ];
        for (data_type, expected) in cases {
            assert_eq!(data_type.to_string(), expected);
        }
    }

    #[test]
    fn children_cover_structs_lists_and_maps() {
        let map = DataType::map(DataType::String, DataType::Integer);
        let names: Vec<&str> = map.children().map(Field::name).collect();
        assert_eq!(names, ["key", "value"]);
        assert!(!map.children().next().unwrap().is_nullable());
        let list = DataType::list(DataType::Date);
        assert_eq!(list.children().count(), 1);
        assert_eq!(DataType::String.children().count(), 0);
        assert!(DataType::Struct(vec![]).is_nested());
        assert!(DataType::decimal().is_numeric());
        assert!(!DataType::String.is_numeric());
    }

    #[test]
    fn fields_carry_native_types_and_descriptions() {
        let field = Field::new("id", DataType::Integer)
            .nullable(false)
            .with_native_type("Int32")
            .with_description("primary key");
        assert_eq!(field.name(), "id");
        assert!(!field.is_nullable());
        assert_eq!(field.native_type(), Some("Int32"));
        assert_eq!(field.description(), Some("primary key"));
        let schema: Schema = [field.clone()].into_iter().collect();
        let schema = schema.named("users");
        assert_eq!(schema.name(), Some("users"));
        assert_eq!(schema.field("id"), Some(&field));
        assert_eq!(schema.index_of("missing"), None);
        assert_eq!(schema.len(), 1);
        assert!(!schema.is_empty());
    }

    #[test]
    fn constraints_show_compare_and_name_themselves() {
        let lit = Literal::new;
        let cases = [
            (Constraint::Const(lit("5")), "= 5", "const", "5"),
            (
                Constraint::Enum(vec![lit("\"a\""), lit("1")]),
                "one of \"a\", 1",
                "enum",
                "\"a\", 1",
            ),
            (
                Constraint::Format("email".into()),
                "format=email",
                "format",
                "email",
            ),
            (
                Constraint::Pattern("^a".into()),
                "pattern=/^a/",
                "pattern",
                "/^a/",
            ),
            (
                Constraint::MinLength(lit("1")),
                "minLength=1",
                "minLength",
                "1",
            ),
            (
                Constraint::UniqueItems(lit("true")),
                "unique items",
                "uniqueItems",
                "true",
            ),
            (
                Constraint::UniqueItems(lit("false")),
                "",
                "uniqueItems",
                "false",
            ),
            (
                Constraint::Deprecated(lit("true")),
                "deprecated",
                "deprecated",
                "true",
            ),
            (
                Constraint::Additional(lit("false")),
                "no other properties",
                "additional properties",
                "refused",
            ),
            (
                Constraint::Additional(lit("true")),
                "",
                "additional properties",
                "allowed",
            ),
            (
                Constraint::PrimaryKey(vec![]),
                "primary key",
                "primary key",
                "",
            ),
            (
                Constraint::PrimaryKey(vec!["a".into(), "b".into()]),
                "primary key (a, b)",
                "primary key",
                "(a, b)",
            ),
            (Constraint::Unique(vec![]), "unique", "unique", ""),
            (
                Constraint::References(ForeignKey::to("users", "id")),
                "→ users.id",
                "references",
                "users.id",
            ),
            (
                Constraint::Other {
                    name: "minContains".into(),
                    value: lit("2"),
                },
                "minContains=2",
                "minContains",
                "2",
            ),
        ];
        for (constraint, shown, name, value) in cases {
            assert_eq!(constraint.to_string(), shown, "{constraint:?}");
            assert_eq!(constraint.name(), name, "{constraint:?}");
            assert_eq!(constraint.value_text(), value, "{constraint:?}");
        }
        let long = Literal::new("x".repeat(50));
        assert_eq!(long.short().chars().count(), 40);
        assert!(long.short().ends_with('…'));
        assert_eq!(Literal::new("1.5e3").as_f64(), Some(1500.0));
        assert_eq!(Literal::new("\"3\"").as_f64(), None);
        assert_eq!(Literal::new("inf").as_f64(), None);
        assert_eq!(
            Literal::json(&serde_json::json!({"a": [1]})).as_str(),
            r#"{"a":[1]}"#
        );
    }

    #[test]
    fn schemas_hold_tables_and_keys() {
        let line = Schema::new([
            Field::new("order", DataType::Integer)
                .with_constraint(Constraint::References(ForeignKey::to("orders", "id"))),
            Field::new("n", DataType::Integer),
        ])
        .named("lines")
        .with_constraint(Constraint::PrimaryKey(vec!["order".into(), "n".into()]))
        .with_constraint(Constraint::References(ForeignKey {
            columns: vec!["order".into(), "n".into()],
            table: "x".into(),
            references: vec![],
        }));
        assert_eq!(line.primary_key(), ["order", "n"]);
        let keys: Vec<String> = line
            .foreign_keys()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(keys, ["(order, n) → x", "order → orders.id"]);
        let mut all = Schema::of_tables([line.clone()]);
        all.push_table(Schema::new([]).named("orders"));
        assert_eq!(all.tables().len(), 2);
        assert_eq!(all.table("lines"), Some(&line));
        assert!(all.is_empty());
        assert!(all.truncated(true).is_truncated());
    }

    #[test]
    fn unexpanded_references_say_why() {
        assert_eq!(
            Unexpanded::Recursive("#/$defs/n".into()).to_string(),
            "→ #/$defs/n (recursive)"
        );
        let unresolved = Unexpanded::Unresolved {
            reference: "a.json".into(),
            reason: "another document".into(),
        };
        assert_eq!(unresolved.to_string(), "→ a.json (another document)");
        assert_eq!(unresolved.reference(), "a.json");
        assert_eq!(Composition::AnyOf.title(), "any of");
    }
}
