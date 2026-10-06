//! Arrow `RecordBatch`es as rows, and Arrow schemas in the
//! format-neutral model (the `arrow` feature).
//!
//! [`schema`] maps an Arrow schema into [`Schema`]: integers of every width
//! are `integer`, `Decimal*(p, s)` is `decimal(p,s)`, dictionaries take
//! their value type, lists, structs and maps nest, and each field keeps
//! Arrow's own type name as its [native type](Field::native_type). Types the
//! model has no name for (times, durations, intervals, unions) are
//! `unknown`, with the native type saying what they are.
//!
//! [`rows`] reads one batch, and [`BatchSource`] streams many. Cells are
//! [`Value::Int`] and [`Value::Float`] for numbers (a `UInt64` past
//! `i64::MAX` becomes a float), [`Value::Null`] for nulls, and text for the
//! rest: booleans as `true`/`false`, decimals exactly, dates as
//! `YYYY-MM-DD`, timestamps as `YYYY-MM-DDTHH:MM:SS[.fff]` in UTC (with a
//! `Z` when the type has a time zone), binary as `0x…` hex, and nested
//! values as compact JSON.
//!
//! ```
//! use std::sync::Arc;
//! use arrow_array::{ArrayRef, Int32Array, RecordBatch, StringArray};
//! use rich_data::{arrow, Value};
//!
//! let batch = RecordBatch::try_from_iter([
//!     ("id", Arc::new(Int32Array::from(vec![Some(1), None])) as ArrayRef),
//!     ("name", Arc::new(StringArray::from(vec!["ada", "alan"])) as ArrayRef),
//! ])
//! .unwrap();
//! let rows = arrow::rows(&batch);
//! assert_eq!(rows.columns(), ["id", "name"]);
//! assert_eq!(rows.rows()[1], [Value::Null, Value::from("alan")]);
//! let id = &rows.schema().unwrap().fields()[0];
//! assert_eq!((id.data_type().to_string(), id.native_type()), ("integer".into(), Some("Int32")));
//! ```

use arrow_array::cast::AsArray;
use arrow_array::types::{
    Date32Type, Date64Type, Decimal128Type, Decimal256Type, Decimal32Type, Decimal64Type,
    DurationMicrosecondType, DurationMillisecondType, DurationNanosecondType, DurationSecondType,
    Float16Type, Float32Type, Float64Type, Int16Type, Int32Type, Int64Type, Int8Type,
    Time32MillisecondType, Time32SecondType, Time64MicrosecondType, Time64NanosecondType,
    TimestampMicrosecondType, TimestampMillisecondType, TimestampNanosecondType,
    TimestampSecondType, UInt16Type, UInt32Type, UInt64Type, UInt8Type,
};
use arrow_array::{Array, RecordBatch};
use arrow_schema::{DataType as Arrow, Field as ArrowField, TimeUnit};
use serde_json::{Map, Value as Json};

use crate::record::MAX_DEPTH;
use crate::{DataError, DataType, Field, Row, RowSource, Rows, Schema, Value};

/// An Arrow schema in the format-neutral model.
pub fn schema(schema: &arrow_schema::Schema) -> Schema {
    Schema::new(schema.fields().iter().map(|f| field(f, 0)))
}

/// One Arrow field in the model.
pub fn field(field: &ArrowField, depth: usize) -> Field {
    Field::new(field.name().clone(), data_type(field.data_type(), depth))
        .nullable(field.is_nullable())
        .with_native_type(field.data_type().to_string())
}

/// One Arrow type in the model.
pub fn data_type(arrow: &Arrow, depth: usize) -> DataType {
    if depth >= MAX_DEPTH {
        return DataType::Any;
    }
    match arrow {
        Arrow::Null => DataType::Unknown,
        Arrow::Boolean => DataType::Boolean,
        Arrow::Int8
        | Arrow::Int16
        | Arrow::Int32
        | Arrow::Int64
        | Arrow::UInt8
        | Arrow::UInt16
        | Arrow::UInt32
        | Arrow::UInt64 => DataType::Integer,
        Arrow::Float16 | Arrow::Float32 | Arrow::Float64 => DataType::Float,
        Arrow::Decimal32(p, s)
        | Arrow::Decimal64(p, s)
        | Arrow::Decimal128(p, s)
        | Arrow::Decimal256(p, s) => DataType::Decimal {
            precision: Some(u32::from(*p)),
            scale: Some(i32::from(*s)),
        },
        Arrow::Utf8 | Arrow::LargeUtf8 | Arrow::Utf8View => DataType::String,
        Arrow::Binary | Arrow::LargeBinary | Arrow::BinaryView | Arrow::FixedSizeBinary(_) => {
            DataType::Binary
        }
        Arrow::Date32 | Arrow::Date64 => DataType::Date,
        Arrow::Timestamp(_, timezone) => DataType::Timestamp {
            timezone: timezone.as_deref().map(str::to_string),
        },
        Arrow::List(item)
        | Arrow::LargeList(item)
        | Arrow::ListView(item)
        | Arrow::LargeListView(item)
        | Arrow::FixedSizeList(item, _) => DataType::List(Box::new(field(item, depth + 1))),
        Arrow::Struct(fields) => {
            DataType::Struct(fields.iter().map(|f| field(f, depth + 1)).collect())
        }
        Arrow::Map(entries, _) => match entries.data_type() {
            Arrow::Struct(kv) if kv.len() == 2 => DataType::Map {
                key: Box::new(field(&kv[0], depth + 1)),
                value: Box::new(field(&kv[1], depth + 1)),
            },
            _ => DataType::Unknown,
        },
        Arrow::Dictionary(_, values) => data_type(values, depth),
        Arrow::RunEndEncoded(_, values) => data_type(values.data_type(), depth),
        _ => DataType::Unknown,
    }
}

/// One batch as rows, with its schema.
pub fn rows(batch: &RecordBatch) -> Rows {
    let arrow_schema = batch.schema();
    let mut rows = Rows::new(arrow_schema.fields().iter().map(|f| f.name().clone()))
        .with_schema(schema(&arrow_schema));
    for index in 0..batch.num_rows() {
        rows.push(batch.columns().iter().map(|c| cell(c.as_ref(), index)));
    }
    rows
}

/// A [`RowSource`] over record batches that share a schema: the first
/// batch's schema is the source's, and a later batch with different
/// columns is an error.
#[derive(Debug)]
pub struct BatchSource<I> {
    batches: I,
    columns: Vec<String>,
    schema: Schema,
    arrow_schema: arrow_schema::SchemaRef,
    current: Option<RecordBatch>,
    row: usize,
    number: usize,
}

impl<I: Iterator<Item = RecordBatch>> BatchSource<I> {
    /// Read `batches`; `None` when there are none (no batch, no schema).
    pub fn new(batches: impl IntoIterator<IntoIter = I>) -> Option<Self> {
        let mut batches = batches.into_iter();
        let first = batches.next()?;
        let arrow_schema = first.schema();
        Some(BatchSource {
            batches,
            columns: arrow_schema
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect(),
            schema: schema(&arrow_schema),
            arrow_schema,
            current: Some(first),
            row: 0,
            number: 1,
        })
    }
}

impl<I: Iterator<Item = RecordBatch>> RowSource for BatchSource<I> {
    fn columns(&self) -> &[String] {
        &self.columns
    }

    fn schema(&self) -> Option<&Schema> {
        Some(&self.schema)
    }

    fn next_row(&mut self) -> Option<Result<Row, DataError>> {
        loop {
            let batch = self.current.as_ref()?;
            if self.row < batch.num_rows() {
                let row = batch
                    .columns()
                    .iter()
                    .map(|c| cell(c.as_ref(), self.row))
                    .collect();
                self.row += 1;
                return Some(Ok(row));
            }
            let next = self.batches.next()?;
            self.number += 1;
            if next.schema().fields() != self.arrow_schema.fields() {
                self.current = None;
                return Some(Err(DataError::at(
                    self.number,
                    "batch schema differs from the first batch's",
                )));
            }
            self.current = Some(next);
            self.row = 0;
        }
    }
}

macro_rules! int {
    ($array:expr, $index:expr, $($variant:ident => $ty:ty),*) => {
        match $array.data_type() {
            $(Arrow::$variant => Some(i128::from($array.as_primitive::<$ty>().value($index))),)*
            _ => None,
        }
    };
}

/// The cell at `index` of `array`.
pub fn cell(array: &dyn Array, index: usize) -> Value {
    if array.is_null(index) {
        return Value::Null;
    }
    let integer = int!(array, index,
        Int8 => Int8Type, Int16 => Int16Type, Int32 => Int32Type, Int64 => Int64Type,
        UInt8 => UInt8Type, UInt16 => UInt16Type, UInt32 => UInt32Type, UInt64 => UInt64Type);
    if let Some(n) = integer {
        return i64::try_from(n).map_or(Value::Float(n as f64), Value::Int);
    }
    match array.data_type() {
        Arrow::Float16 => Value::Float(array.as_primitive::<Float16Type>().value(index).to_f64()),
        Arrow::Float32 => Value::Float(f64::from(array.as_primitive::<Float32Type>().value(index))),
        Arrow::Float64 => Value::Float(array.as_primitive::<Float64Type>().value(index)),
        Arrow::Duration(unit) => Value::Int(match unit {
            TimeUnit::Second => array.as_primitive::<DurationSecondType>().value(index),
            TimeUnit::Millisecond => array.as_primitive::<DurationMillisecondType>().value(index),
            TimeUnit::Microsecond => array.as_primitive::<DurationMicrosecondType>().value(index),
            TimeUnit::Nanosecond => array.as_primitive::<DurationNanosecondType>().value(index),
        }),
        Arrow::List(_)
        | Arrow::LargeList(_)
        | Arrow::FixedSizeList(..)
        | Arrow::Struct(_)
        | Arrow::Map(..) => Value::Str(json(array, index, 0).to_string()),
        Arrow::Dictionary(..) => {
            let dictionary = array.as_any_dictionary();
            let key = dictionary.normalized_keys()[index];
            cell(dictionary.values().as_ref(), key)
        }
        _ => Value::Str(text(array, index)),
    }
}

/// A scalar cell's text: booleans, decimals, strings, binary, dates, times
/// and timestamps.
fn text(array: &dyn Array, index: usize) -> String {
    match array.data_type() {
        Arrow::Boolean => array.as_boolean().value(index).to_string(),
        Arrow::Decimal32(_, s) => decimal(
            array
                .as_primitive::<Decimal32Type>()
                .value(index)
                .to_string(),
            *s,
        ),
        Arrow::Decimal64(_, s) => decimal(
            array
                .as_primitive::<Decimal64Type>()
                .value(index)
                .to_string(),
            *s,
        ),
        Arrow::Decimal128(_, s) => decimal(
            array
                .as_primitive::<Decimal128Type>()
                .value(index)
                .to_string(),
            *s,
        ),
        Arrow::Decimal256(_, s) => decimal(
            array
                .as_primitive::<Decimal256Type>()
                .value(index)
                .to_string(),
            *s,
        ),
        Arrow::Utf8 => array.as_string::<i32>().value(index).to_string(),
        Arrow::LargeUtf8 => array.as_string::<i64>().value(index).to_string(),
        Arrow::Utf8View => array.as_string_view().value(index).to_string(),
        Arrow::Binary => hex(array.as_binary::<i32>().value(index)),
        Arrow::LargeBinary => hex(array.as_binary::<i64>().value(index)),
        Arrow::BinaryView => hex(array.as_binary_view().value(index)),
        Arrow::FixedSizeBinary(_) => hex(array.as_fixed_size_binary().value(index)),
        Arrow::Date32 => date(i64::from(array.as_primitive::<Date32Type>().value(index))),
        Arrow::Date64 => date(
            array
                .as_primitive::<Date64Type>()
                .value(index)
                .div_euclid(86_400_000),
        ),
        Arrow::Timestamp(unit, timezone) => {
            let raw = match unit {
                TimeUnit::Second => array.as_primitive::<TimestampSecondType>().value(index),
                TimeUnit::Millisecond => array
                    .as_primitive::<TimestampMillisecondType>()
                    .value(index),
                TimeUnit::Microsecond => array
                    .as_primitive::<TimestampMicrosecondType>()
                    .value(index),
                TimeUnit::Nanosecond => {
                    array.as_primitive::<TimestampNanosecondType>().value(index)
                }
            };
            let (seconds, fraction) = split(i128::from(raw), *unit);
            let days = seconds.div_euclid(86_400) as i64;
            let mut out = format!(
                "{}T{}",
                date(days),
                time(seconds.rem_euclid(86_400), fraction)
            );
            if timezone.is_some() {
                out.push('Z');
            }
            out
        }
        Arrow::Time32(unit) | Arrow::Time64(unit) => {
            let raw = match unit {
                TimeUnit::Second => {
                    i64::from(array.as_primitive::<Time32SecondType>().value(index))
                }
                TimeUnit::Millisecond => {
                    i64::from(array.as_primitive::<Time32MillisecondType>().value(index))
                }
                TimeUnit::Microsecond => array.as_primitive::<Time64MicrosecondType>().value(index),
                TimeUnit::Nanosecond => array.as_primitive::<Time64NanosecondType>().value(index),
            };
            let (seconds, fraction) = split(i128::from(raw), *unit);
            time(seconds, fraction)
        }
        other => format!("<{other}>"),
    }
}

/// Seconds and the sub-second digits (as written, without trailing zeros).
fn split(raw: i128, unit: TimeUnit) -> (i128, String) {
    let (per_second, width) = match unit {
        TimeUnit::Second => return (raw, String::new()),
        TimeUnit::Millisecond => (1_000, 3),
        TimeUnit::Microsecond => (1_000_000, 6),
        TimeUnit::Nanosecond => (1_000_000_000, 9),
    };
    let fraction = format!("{:0width$}", raw.rem_euclid(per_second));
    (
        raw.div_euclid(per_second),
        fraction.trim_end_matches('0').to_string(),
    )
}

/// `HH:MM:SS[.fff]` for seconds into a day.
fn time(seconds: i128, fraction: String) -> String {
    let mut out = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(&fraction);
    }
    out
}

/// `YYYY-MM-DD` for days since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
fn date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// An unscaled decimal's digits with the point `scale` places from the end.
fn decimal(digits: String, scale: i8) -> String {
    let (sign, digits) = match digits.strip_prefix('-') {
        Some(rest) => ("-", rest.to_string()),
        None => ("", digits),
    };
    if scale <= 0 {
        let zeros = "0".repeat(usize::from(scale.unsigned_abs()));
        return format!("{sign}{digits}{zeros}");
    }
    let scale = usize::from(scale.unsigned_abs());
    let digits = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = digits.split_at(digits.len() - scale);
    format!("{sign}{whole}.{fraction}")
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("0x");
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// A value as JSON, for nested cells.
fn json(array: &dyn Array, index: usize, depth: usize) -> Json {
    if array.is_null(index) {
        return Json::Null;
    }
    if depth >= MAX_DEPTH {
        return Json::String("…".into());
    }
    let items = |values: &dyn Array, depth: usize| {
        Json::Array(
            (0..values.len())
                .map(|i| json(values, i, depth + 1))
                .collect(),
        )
    };
    match array.data_type() {
        Arrow::List(_) => items(array.as_list::<i32>().value(index).as_ref(), depth),
        Arrow::LargeList(_) => items(array.as_list::<i64>().value(index).as_ref(), depth),
        Arrow::FixedSizeList(..) => items(array.as_fixed_size_list().value(index).as_ref(), depth),
        Arrow::Struct(_) => {
            let array = array.as_struct();
            let map: Map<String, Json> = array
                .column_names()
                .into_iter()
                .zip(array.columns())
                .map(|(name, column)| (name.to_string(), json(column.as_ref(), index, depth + 1)))
                .collect();
            Json::Object(map)
        }
        Arrow::Map(..) => {
            let entries = array.as_map().value(index);
            let (keys, values) = (entries.column(0), entries.column(1));
            let map: Map<String, Json> = (0..entries.len())
                .map(|i| {
                    (
                        cell(keys.as_ref(), i).plain(),
                        json(values.as_ref(), i, depth + 1),
                    )
                })
                .collect();
            Json::Object(map)
        }
        _ => match cell(array, index) {
            Value::Null => Json::Null,
            Value::Int(n) => Json::from(n),
            Value::Float(f) => serde_json::Number::from_f64(f).map_or(Json::Null, Json::Number),
            Value::Str(s) if array.data_type() == &Arrow::Boolean => Json::Bool(s == "true"),
            other => Json::String(other.plain()),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::builder::{Int32Builder, ListBuilder, MapBuilder, StringBuilder};
    use arrow_array::{
        ArrayRef, BooleanArray, Date32Array, Decimal128Array, DictionaryArray, Float64Array,
        StructArray, TimestampMillisecondArray, UInt64Array,
    };
    use arrow_schema::Field as ArrowField;

    use super::*;

    #[test]
    fn scalars_become_cells() {
        let batch = RecordBatch::try_from_iter([
            (
                "big",
                Arc::new(UInt64Array::from(vec![u64::MAX])) as ArrayRef,
            ),
            ("f", Arc::new(Float64Array::from(vec![1.5])) as ArrayRef),
            ("ok", Arc::new(BooleanArray::from(vec![true])) as ArrayRef),
            (
                "price",
                Arc::new(
                    Decimal128Array::from(vec![-1205])
                        .with_precision_and_scale(10, 2)
                        .unwrap(),
                ) as ArrayRef,
            ),
            ("day", Arc::new(Date32Array::from(vec![19_782])) as ArrayRef),
            (
                "at",
                Arc::new(
                    TimestampMillisecondArray::from(vec![1_709_164_800_250]).with_timezone("UTC"),
                ) as ArrayRef,
            ),
            (
                "kind",
                Arc::new(DictionaryArray::<arrow_array::types::Int8Type>::from_iter(
                    ["x"],
                )) as ArrayRef,
            ),
        ])
        .unwrap();
        let rows = rows(&batch);
        assert_eq!(
            rows.rows()[0],
            [
                Value::Float(u64::MAX as f64),
                Value::Float(1.5),
                Value::from("true"),
                Value::from("-12.05"),
                Value::from("2024-02-29"),
                Value::from("2024-02-29T00:00:00.25Z"),
                Value::from("x"),
            ]
        );
        let types: Vec<String> = rows
            .schema()
            .unwrap()
            .fields()
            .iter()
            .map(|f| f.data_type().to_string())
            .collect();
        assert_eq!(
            types,
            [
                "integer",
                "float",
                "boolean",
                "decimal(10,2)",
                "date",
                "timestamp[UTC]",
                "string"
            ]
        );
    }

    #[test]
    fn nested_values_are_json_and_nest_in_the_schema() {
        let mut list = ListBuilder::new(Int32Builder::new());
        list.values().append_value(1);
        list.values().append_null();
        list.append(true);
        let list = Arc::new(list.finish()) as ArrayRef;
        let mut map = MapBuilder::new(None, StringBuilder::new(), Int32Builder::new());
        map.keys().append_value("a");
        map.values().append_value(2);
        map.append(true).unwrap();
        let map = Arc::new(map.finish()) as ArrayRef;
        let inner = Arc::new(StructArray::from(vec![(
            Arc::new(ArrowField::new("x", Arrow::Int32, true)),
            Arc::new(arrow_array::Int32Array::from(vec![7])) as ArrayRef,
        )])) as ArrayRef;
        let batch = RecordBatch::try_from_iter([("l", list), ("m", map), ("s", inner)]).unwrap();
        let rows = rows(&batch);
        assert_eq!(
            rows.rows()[0],
            [
                Value::from("[1,null]"),
                Value::from(r#"{"a":2}"#),
                Value::from(r#"{"x":7}"#)
            ]
        );
        let schema = rows.schema().unwrap();
        assert_eq!(schema.fields()[0].data_type().to_string(), "list<integer>");
        assert_eq!(
            schema.fields()[1].data_type().to_string(),
            "map<string, integer>"
        );
        assert_eq!(schema.fields()[2].children().next().unwrap().name(), "x");
    }

    #[test]
    fn a_batch_source_streams_and_checks_schemas() {
        let batch = |name: &str| {
            RecordBatch::try_from_iter([(
                name,
                Arc::new(arrow_array::Int32Array::from(vec![1, 2])) as ArrayRef,
            )])
            .unwrap()
        };
        let mut source = BatchSource::new([batch("a"), batch("a"), batch("b")]).unwrap();
        for _ in 0..4 {
            assert!(source.next_row().unwrap().is_ok());
        }
        let error = source.next_row().unwrap().unwrap_err();
        assert_eq!(error.line(), Some(3));
        assert!(source.next_row().is_none());
        assert!(BatchSource::new(Vec::<RecordBatch>::new()).is_none());
    }

    #[test]
    fn dates_and_decimals_format_exactly() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(-1), "1969-12-31");
        assert_eq!(decimal("5".into(), 3), "0.005");
        assert_eq!(decimal("-123".into(), 0), "-123");
        assert_eq!(decimal("12".into(), -2), "1200");
        assert_eq!(split(-1, TimeUnit::Millisecond), (-1, "999".to_string()));
        assert_eq!(time(3661, "5".into()), "01:01:01.5");
    }
}
