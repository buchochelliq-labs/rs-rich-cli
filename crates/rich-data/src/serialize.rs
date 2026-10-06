//! Rows from any `serde::Serialize` values: a struct's fields (or a map's
//! keys) are columns, and any other value is one `value` column.
//!
//! Values go through `serde_json`'s data model, so a field is typed as JSON
//! would type it (see [`RecordSource`]): numbers stay numbers, `Option`s are
//! nullable, nested structs and sequences are described in the schema and
//! shown as compact JSON. [`read`] samples every value; [`source`] streams,
//! sampling the first [`DEFAULT_SAMPLE`].
//!
//! ```
//! use rich_data::{serialize, Value};
//!
//! #[derive(serde::Serialize)]
//! struct Deploy { service: &'static str, replicas: u32, healthy: Option<bool> }
//!
//! let rows = serialize::read([
//!     Deploy { service: "web", replicas: 3, healthy: Some(true) },
//!     Deploy { service: "db", replicas: 1, healthy: None },
//! ])
//! .unwrap();
//! assert_eq!(rows.columns(), ["service", "replicas", "healthy"]);
//! assert_eq!(rows.rows()[1], [Value::from("db"), Value::Int(1), Value::Null]);
//! let healthy = rows.schema().unwrap().field("healthy").unwrap();
//! assert!(healthy.is_nullable());
//! ```

use serde::Serialize;
use serde_json::Value as Json;

use crate::{DataError, RecordSource, RowSource, Rows};

pub use crate::record::DEFAULT_SAMPLE;

/// Read `items`, sampling every one for the columns and schema.
pub fn read<T: Serialize>(items: impl IntoIterator<Item = T>) -> Result<Rows, DataError> {
    RecordSource::new(Records::new(items.into_iter()), usize::MAX)?.collect_rows()
}

/// Stream `items`, sampling the first [`DEFAULT_SAMPLE`] for the columns and
/// schema.
pub fn source<T, I>(items: I) -> Result<SerializeSource<I::IntoIter>, DataError>
where
    T: Serialize,
    I: IntoIterator<Item = T>,
{
    source_with_sample(items, DEFAULT_SAMPLE)
}

/// Stream `items`, sampling the first `sample`.
pub fn source_with_sample<T, I>(
    items: I,
    sample: usize,
) -> Result<SerializeSource<I::IntoIter>, DataError>
where
    T: Serialize,
    I: IntoIterator<Item = T>,
{
    RecordSource::new(Records::new(items.into_iter()), sample)
}

/// A streaming [`RowSource`] over `Serialize` values.
pub type SerializeSource<I> = RecordSource<Records<I>>;

/// Values as JSON records, numbered from 1 in errors.
#[derive(Debug)]
pub struct Records<I> {
    items: I,
    index: usize,
}

impl<I> Records<I> {
    /// Records from `items`.
    pub fn new(items: I) -> Self {
        Records { items, index: 0 }
    }
}

impl<T: Serialize, I: Iterator<Item = T>> Iterator for Records<I> {
    type Item = Result<Json, DataError>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.items.next()?;
        self.index += 1;
        Some(serde_json::to_value(item).map_err(|e| DataError::at(self.index, e.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::Value;

    #[test]
    fn maps_and_scalars_become_rows() {
        let rows = read([BTreeMap::from([("b", 2), ("a", 1)])]).unwrap();
        assert_eq!(rows.columns(), ["a", "b"]);
        let rows = read([1.5, 2.0]).unwrap();
        assert_eq!(rows.columns(), ["value"]);
        assert_eq!(rows.rows()[0], [Value::Float(1.5)]);
    }

    #[test]
    fn errors_name_the_record() {
        // A map with non-string keys cannot be a JSON object.
        let bad = BTreeMap::from([((1, 2), 3)]);
        let error = read([bad]).unwrap_err();
        assert_eq!(error.line(), Some(1));
        let mut source = source(Vec::<u8>::new()).unwrap();
        assert!(source.next_row().is_none());
    }
}
