//! The format-neutral schema model: named fields with a type, nullability
//! and children (0.0.16 workstream 1).
//!
//! This is the shape every schema source maps into, so tabular data, JSON
//! Schema, Arrow and SQL DDL can be shown and compared the same way. It is
//! deliberately small: a [`Schema`] is a list of [`Field`]s, and a field's
//! [`DataType`] carries its children (a struct's fields, a list's item, a
//! map's key and value). Detail a format has and the model does not, such
//! as Arrow's `Int32` or SQL's `VARCHAR(20)`, is kept as the field's
//! [native type](Field::native_type), as written.
//!
//! [`SchemaTree`](super::SchemaTree) and [`SchemaDiff`](super::SchemaDiff)
//! still read JSON Schema directly; the mappings into this model and
//! constraints arrive with the schema workstream.
//!
//! ```
//! use rich_ext::schema::{DataType, Field, Schema};
//!
//! let schema = Schema::new([
//!     Field::new("id", DataType::Integer).nullable(false),
//!     Field::new("email", DataType::String),
//!     Field::new("tags", DataType::list(DataType::String)),
//!     Field::new(
//!         "address",
//!         DataType::Struct(vec![
//!             Field::new("city", DataType::String),
//!             Field::new("zip", DataType::String).with_native_type("char(5)"),
//!         ]),
//!     ),
//! ]);
//! assert_eq!(schema.index_of("email"), Some(1));
//! assert_eq!(schema.fields()[2].data_type().to_string(), "list<string>");
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

/// One named field: a type, whether it may be null, and optionally the
/// source's own name for the type and a description.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Field {
    name: String,
    data_type: DataType,
    nullable: bool,
    native_type: Option<String>,
    description: Option<String>,
}

impl Field {
    /// A nullable field.
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Field {
            name: name.into(),
            data_type,
            nullable: true,
            native_type: None,
            description: None,
        }
    }

    /// Whether the field may be null (default `true`).
    pub fn nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }

    /// The source format's own name for the type, kept as written
    /// (`Int32`, `VARCHAR(20)`, `string/email`).
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

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The type.
    pub fn data_type(&self) -> &DataType {
        &self.data_type
    }

    /// Whether the field may be null.
    pub fn is_nullable(&self) -> bool {
        self.nullable
    }

    /// The source's own type name, if one was recorded.
    pub fn native_type(&self) -> Option<&str> {
        self.native_type.as_deref()
    }

    /// The description, if any.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The nested fields ([`DataType::children`]).
    pub fn children(&self) -> impl Iterator<Item = &Field> {
        self.data_type.children()
    }
}

/// An ordered list of top-level fields, optionally named (a table, a
/// record type, a document).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Schema {
    name: Option<String>,
    fields: Vec<Field>,
}

impl Schema {
    /// A schema of `fields`, in order.
    pub fn new(fields: impl IntoIterator<Item = Field>) -> Self {
        Schema {
            name: None,
            fields: fields.into_iter().collect(),
        }
    }

    /// Name the schema (a table or type name).
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The name, if any.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
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
}
