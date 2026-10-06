//! JSON records as rows, shared by the [`jsonl`](crate::jsonl) and
//! [`serialize`](crate::serialize) adapters.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use serde_json::{Map, Value as Json};

use crate::{DataError, DataType, Field, Row, RowSource, Schema, Value};

/// How many levels of nested arrays and objects the schema describes; below
/// that a value's type is `any`.
pub const MAX_DEPTH: usize = 32;

/// How many records a streaming source reads ahead to choose its columns and
/// schema, unless told otherwise.
pub const DEFAULT_SAMPLE: usize = 1000;

/// How many keys seen after the sample a source names in
/// [`RecordSource::unknown_keys`].
pub const MAX_UNKNOWN_KEYS: usize = 100;

/// Rows from a stream of JSON records.
///
/// The source reads the first `sample` records ahead: their keys, in the
/// order first seen, are the columns, and their values decide the schema
/// (`integer` and `float` widen to `float`; disagreeing types become `any`;
/// a key some records lack, or that holds `null`, is nullable). A record
/// that is not an object is one `value` column. Scalars become cells as they
/// are (`true`/`false` as text, since [`Value`] has no boolean; whole
/// numbers in a `float` column as floats), and arrays and objects become
/// their compact JSON text.
///
/// Keys first seen after the sample have no column; their values are
/// dropped and their names kept in [`unknown_keys`](Self::unknown_keys), so
/// a caller can say so: the first [`MAX_UNKNOWN_KEYS`] of them, so a stream
/// whose every record has a new key stays bounded
/// ([`more_unknown_keys`](Self::more_unknown_keys) says there were more).
/// Read everything at once (the adapters' `read`) to sample every record.
#[derive(Debug)]
pub struct RecordSource<I> {
    records: I,
    buffer: VecDeque<Map<String, Json>>,
    columns: Vec<String>,
    /// `columns`, to look keys up in.
    names: HashSet<String>,
    schema: Schema,
    unknown: BTreeSet<String>,
    more_unknown: bool,
}

impl<I> RecordSource<I>
where
    I: Iterator<Item = Result<Json, DataError>>,
{
    /// Read ahead `sample` records (at least one) from `records`.
    pub(crate) fn new(mut records: I, sample: usize) -> Result<Self, DataError> {
        let mut buffer = VecDeque::new();
        let mut fields: Vec<Field> = Vec::new();
        // Each field's position in `fields`, the record it was first seen in
        // and how many records held it: a hash and counts rather than a scan
        // of every field per key, which made a record of many keys quadratic.
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut first: Vec<usize> = Vec::new();
        let mut held: Vec<usize> = Vec::new();
        for number in 0..sample.max(1) {
            let Some(record) = records.next() else {
                break;
            };
            let record = object(record?);
            for (key, value) in &record {
                let seen = field_of(key, value, 0);
                match index.get(key) {
                    Some(&at) => {
                        fields[at] = merge(fields[at].clone(), seen);
                        held[at] += 1;
                    }
                    // Absent from every earlier record.
                    None => {
                        index.insert(key.clone(), fields.len());
                        fields.push(seen.nullable(number > 0 || value.is_null()));
                        first.push(number);
                        held.push(1);
                    }
                }
            }
            buffer.push_back(record);
        }
        // A column some record since its first lacks may be null.
        let read = buffer.len();
        for ((field, first), held) in fields.iter_mut().zip(first).zip(held) {
            if held < read - first {
                *field = field.clone().nullable(true);
            }
        }
        Ok(RecordSource {
            records,
            buffer,
            names: index.into_keys().collect(),
            columns: fields.iter().map(|f| f.name().to_string()).collect(),
            schema: Schema::new(fields),
            unknown: BTreeSet::new(),
            more_unknown: false,
        })
    }

    /// Keys seen after the sample, which have no column (so far): the first
    /// [`MAX_UNKNOWN_KEYS`] seen, sorted.
    pub fn unknown_keys(&self) -> impl Iterator<Item = &str> {
        self.unknown.iter().map(String::as_str)
    }

    /// Whether more keys than [`unknown_keys`](Self::unknown_keys) holds were
    /// seen after the sample.
    pub fn more_unknown_keys(&self) -> bool {
        self.more_unknown
    }

    fn row(&mut self, record: Map<String, Json>) -> Row {
        for key in record.keys() {
            if !self.names.contains(key) && !self.unknown.contains(key) {
                if self.unknown.len() < MAX_UNKNOWN_KEYS {
                    self.unknown.insert(key.clone());
                } else {
                    self.more_unknown = true;
                }
            }
        }
        self.columns
            .iter()
            .zip(self.schema.fields())
            .map(
                |(column, field)| match record.get(column).map_or(Value::Null, cell) {
                    // A float column's whole numbers are floats too.
                    Value::Int(n) if field.data_type() == &DataType::Float => {
                        Value::Float(n as f64)
                    }
                    other => other,
                },
            )
            .collect()
    }
}

impl<I> RowSource for RecordSource<I>
where
    I: Iterator<Item = Result<Json, DataError>>,
{
    fn columns(&self) -> &[String] {
        &self.columns
    }

    fn schema(&self) -> Option<&Schema> {
        Some(&self.schema)
    }

    fn next_row(&mut self) -> Option<Result<Row, DataError>> {
        let record = match self.buffer.pop_front() {
            Some(record) => record,
            None => match self.records.next()? {
                Ok(record) => object(record),
                Err(error) => return Some(Err(error)),
            },
        };
        Some(Ok(self.row(record)))
    }
}

/// A record as an object; anything else is `{"value": record}`.
fn object(record: Json) -> Map<String, Json> {
    match record {
        Json::Object(map) => map,
        other => Map::from_iter([("value".to_string(), other)]),
    }
}

/// A JSON value as a cell.
pub(crate) fn cell(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Str(b.to_string()),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => n.as_f64().map_or(Value::Null, Value::Float),
        },
        Json::String(s) => Value::Str(s.clone()),
        other => Value::Str(other.to_string()),
    }
}

/// The field one value describes: non-null unless it is `null`.
fn field_of(name: &str, value: &Json, depth: usize) -> Field {
    let data_type = match value {
        Json::Null => return Field::new(name, DataType::Unknown),
        _ if depth >= MAX_DEPTH => DataType::Any,
        Json::Bool(_) => DataType::Boolean,
        Json::Number(n) if n.is_f64() => DataType::Float,
        Json::Number(_) => DataType::Integer,
        Json::String(_) => DataType::String,
        Json::Array(items) => {
            let item = items
                .iter()
                .map(|item| field_of("item", item, depth + 1))
                .reduce(merge)
                .unwrap_or_else(|| Field::new("item", DataType::Unknown));
            DataType::List(Box::new(item))
        }
        Json::Object(map) => {
            let fields = map.iter().map(|(k, v)| field_of(k, v, depth + 1)).collect();
            DataType::Struct(fields)
        }
    };
    Field::new(name, data_type).nullable(false)
}

/// Two observations of one field, merged.
fn merge(a: Field, b: Field) -> Field {
    let nullable = a.is_nullable() || b.is_nullable();
    let data_type = match (a.data_type().clone(), b.data_type().clone()) {
        (DataType::Unknown, other) | (other, DataType::Unknown) => other,
        (x, y) if x == y => x,
        (DataType::Integer, DataType::Float) | (DataType::Float, DataType::Integer) => {
            DataType::Float
        }
        (DataType::List(x), DataType::List(y)) => DataType::List(Box::new(merge(*x, *y))),
        (DataType::Struct(xs), DataType::Struct(ys)) => {
            // By name through a hash, not a scan of `ys` per field of `xs`.
            let at: HashMap<String, usize> = ys
                .iter()
                .enumerate()
                .map(|(i, y)| (y.name().to_string(), i))
                .collect();
            let mut ys: Vec<Option<Field>> = ys.into_iter().map(Some).collect();
            let mut fields: Vec<Field> = xs
                .into_iter()
                .map(|x| match at.get(x.name()).and_then(|&i| ys[i].take()) {
                    Some(y) => merge(x, y),
                    None => x.nullable(true),
                })
                .collect();
            fields.extend(ys.into_iter().flatten().map(|y| y.nullable(true)));
            DataType::Struct(fields)
        }
        _ => DataType::Any,
    };
    a.with_type(data_type).nullable(nullable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source(
        records: Vec<Json>,
        sample: usize,
    ) -> RecordSource<impl Iterator<Item = Result<Json, DataError>>> {
        RecordSource::new(records.into_iter().map(Ok), sample).unwrap()
    }

    #[test]
    fn the_sample_decides_columns_and_types() {
        let source = source(
            vec![
                json!({"id": 1, "score": 2, "tags": ["a"], "meta": {"x": 1}}),
                json!({"id": 2, "score": 2.5, "tags": [], "meta": {"y": "z"}, "extra": null}),
            ],
            10,
        );
        assert_eq!(source.columns(), ["id", "score", "tags", "meta", "extra"]);
        let schema = source.schema().unwrap();
        let types: Vec<String> = schema
            .fields()
            .iter()
            .map(|f| f.data_type().to_string())
            .collect();
        assert_eq!(
            types,
            ["integer", "float", "list<string>", "struct", "unknown"]
        );
        assert!(!schema.fields()[0].is_nullable());
        assert!(schema.fields()[4].is_nullable());
        let meta: Vec<(&str, bool)> = schema.fields()[3]
            .children()
            .map(|f| (f.name(), f.is_nullable()))
            .collect();
        assert_eq!(meta, [("x", true), ("y", true)]);
    }

    #[test]
    fn keys_after_the_sample_are_reported_not_invented() {
        let mut source = source(
            vec![json!({"a": 1}), json!({"a": "x", "b": true}), json!(7)],
            1,
        );
        assert_eq!(source.columns(), ["a"]);
        assert_eq!(source.next_row().unwrap().unwrap(), [Value::Int(1)]);
        assert_eq!(source.next_row().unwrap().unwrap(), [Value::from("x")]);
        // A scalar record is a `value` column, which this source lacks.
        assert_eq!(source.next_row().unwrap().unwrap(), [Value::Null]);
        assert!(source.next_row().is_none());
        assert_eq!(source.unknown_keys().collect::<Vec<_>>(), ["b", "value"]);
    }

    #[test]
    fn cells_keep_scalars_and_write_nested_values_as_json() {
        assert_eq!(cell(&json!(true)), Value::from("true"));
        assert_eq!(cell(&json!(1.5)), Value::Float(1.5));
        assert_eq!(cell(&json!(u64::MAX)), Value::Float(u64::MAX as f64));
        assert_eq!(cell(&json!({"a": [1, 2]})), Value::from(r#"{"a":[1,2]}"#));
    }
}
