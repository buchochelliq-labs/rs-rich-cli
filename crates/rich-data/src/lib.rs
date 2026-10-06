//! # rich-data
//!
//! Tabular data for [rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli):
//! one row source contract with adapters behind it, so a file, a stream of
//! records or a query result becomes a table without hand-written glue. Not
//! a port of anything upstream; it builds on `rich-ext`'s public API only.
//!
//! - [`RowSource`]: column names, an optional [`Schema`] (`rich_ext`'s
//!   format-neutral model) and rows of [`Value`]s, one at a time. [`Rows`]
//!   holds them in memory and renders through `rich_ext`'s
//!   [`TableData`](rich_ext::table::TableData).
//! - Adapters: [`csv`] (CSV and TSV through the port of Python's `csv`
//!   sniffer and reader that `rich --csv` uses), [`jsonl`] (JSON Lines),
//!   [`serialize`] (any `serde::Serialize` rows), and `arrow` (Arrow
//!   `RecordBatch`es, behind the off-by-default `arrow` feature).
//! - `er`: an ER diagram of a [`Schema`] or of SQL DDL, through
//!   `rs-rich-diagram`, behind the off-by-default `er` feature.
//! - [`infer`]: column types (integers, floats, booleans, dates, timestamps,
//!   nulls, text) with the evidence for each, on request.
//! - [`stats`]: per-column count, nulls, distinct values, min, max, mean,
//!   median, quantiles and top values, as a table or under each heading.
//! - [`window`]: rows for `rich_ext`'s virtualised tables: [`Rows`] by
//!   index, and [`RowWindow`](window::RowWindow), one window of a
//!   forward-only source read in constant memory.
//! - [`sql`]: a query result set ([`ResultSet`](sql::ResultSet)) with typed
//!   alignment, `NULL` marked apart from empty text, and a row count.
//!
//! Conditional styles for the tables these rows become are
//! `rich_ext::table::rules`.
//!
//! ```
//! use rich::Console;
//! use rich_data::{csv, infer::Inferrer, RowSource};
//!
//! let mut rows = csv::CsvReader::new()
//!     .read("service,p99,up\nweb,120,true\napi,35.5,false\n")
//!     .unwrap();
//! assert_eq!(rows.columns(), ["service", "p99", "up"]);
//!
//! // Types are only inferred when asked, and every guess carries its evidence.
//! let inference = Inferrer::new().infer(&rows);
//! assert_eq!(inference.columns()[1].data_type().to_string(), "float");
//! assert_eq!(inference.columns()[1].evidence().floats, 2);
//! inference.apply(&mut rows);
//!
//! let console = Console::builder().width(40).build();
//! let out = console.render_export(&rows.to_table_data());
//! assert!(out.contains("│ api     │ 35.5 │ false │"));
//! ```

pub mod csv;
#[cfg(feature = "er")]
pub mod er;
pub mod infer;
pub mod jsonl;
mod record;
pub mod serialize;
pub mod sql;
pub mod stats;
pub mod window;

#[cfg(feature = "arrow")]
pub mod arrow;

use std::fmt;

use rich::Justify;
use rich_ext::table::{Column, TableData};

pub use record::RecordSource;
pub use rich_ext::schema::{DataType, Field, Schema};
pub use rich_ext::table::Value;

/// One row of cells.
pub type Row = Vec<Value>;

/// Why rows could not be read: what went wrong, and where when the source
/// knows (a 1-based line or record number).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataError {
    line: Option<usize>,
    message: String,
}

impl DataError {
    /// An error with no position.
    pub fn new(message: impl Into<String>) -> Self {
        DataError {
            line: None,
            message: message.into(),
        }
    }

    /// An error at a 1-based line or record.
    pub fn at(line: usize, message: impl Into<String>) -> Self {
        DataError {
            line: Some(line),
            message: message.into(),
        }
    }

    /// The 1-based line or record, when known.
    pub fn line(&self) -> Option<usize> {
        self.line
    }

    /// The message, without the position.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for DataError {}

/// The row source contract: column names, an optional schema, and rows.
///
/// A source yields rows one at a time, so a renderer or a profile can stop
/// early or hold one window of a long input; [`collect_rows`] reads the rest
/// into [`Rows`]. Every row a source yields has exactly one cell per column.
///
/// [`collect_rows`]: RowSource::collect_rows
pub trait RowSource {
    /// The column names, in order.
    fn columns(&self) -> &[String];

    /// What the source knows about the columns' types, if anything. A
    /// source's schema has one field per column, in the same order.
    fn schema(&self) -> Option<&Schema> {
        None
    }

    /// The next row, `None` at the end, or why it could not be read.
    fn next_row(&mut self) -> Option<Result<Row, DataError>>;

    /// Read every remaining row into memory.
    fn collect_rows(mut self) -> Result<Rows, DataError>
    where
        Self: Sized,
    {
        let mut rows = Rows::new(self.columns().to_vec());
        rows.schema = self.schema().cloned();
        while let Some(row) = self.next_row() {
            rows.push(row?);
        }
        Ok(rows)
    }
}

/// Rows held in memory: column names, an optional schema, and the rows.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    columns: Vec<String>,
    schema: Option<Schema>,
    rows: Vec<Row>,
}

impl Rows {
    /// No rows under `columns`.
    pub fn new(columns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Rows {
            columns: columns.into_iter().map(Into::into).collect(),
            schema: None,
            rows: Vec::new(),
        }
    }

    /// Attach a schema (one field per column).
    pub fn with_schema(mut self, schema: Schema) -> Self {
        self.schema = Some(schema);
        self
    }

    /// Replace the schema.
    pub fn set_schema(&mut self, schema: Option<Schema>) {
        self.schema = schema;
    }

    /// Append a row: missing cells are null, and extra cells are dropped.
    pub fn push(&mut self, row: impl IntoIterator<Item = Value>) -> &mut Self {
        let mut row: Row = row.into_iter().take(self.columns.len()).collect();
        row.resize(self.columns.len(), Value::Null);
        self.rows.push(row);
        self
    }

    /// The column names.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The schema, if any.
    pub fn schema(&self) -> Option<&Schema> {
        self.schema.as_ref()
    }

    /// The rows, in order.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The rows, to change in place. Keep one cell per column.
    pub fn rows_mut(&mut self) -> &mut [Row] {
        &mut self.rows
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether there are no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The position of the first column called `name`.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    /// Every cell of the column at `index`, top to bottom.
    pub fn column(&self, index: usize) -> impl Iterator<Item = &Value> {
        self.rows.iter().filter_map(move |row| row.get(index))
    }

    /// The columns, schema and rows.
    pub fn into_parts(self) -> (Vec<String>, Option<Schema>, Vec<Row>) {
        (self.columns, self.schema, self.rows)
    }

    /// A row source over these rows.
    pub fn into_source(self) -> RowsSource {
        RowsSource {
            columns: self.columns,
            schema: self.schema,
            rows: self.rows.into_iter(),
        }
    }

    /// A table of these rows: one column per name, numeric columns (by the
    /// schema) right-justified. Sort, group, total or style it with
    /// `rich_ext::table`.
    pub fn to_table_data(&self) -> TableData {
        let columns = self.columns.iter().enumerate().map(|(index, name)| {
            let numeric = self
                .schema
                .as_ref()
                .and_then(|s| s.fields().get(index))
                .is_some_and(|f| f.data_type().is_numeric());
            let column = Column::new(name.clone());
            if numeric {
                column.justify(Justify::Right)
            } else {
                column
            }
        });
        let mut data = TableData::new(columns);
        data.extend(self.rows.iter().cloned());
        data
    }
}

/// [`Rows`] as a [`RowSource`], from [`Rows::into_source`].
#[derive(Debug)]
pub struct RowsSource {
    columns: Vec<String>,
    schema: Option<Schema>,
    rows: std::vec::IntoIter<Row>,
}

impl RowSource for RowsSource {
    fn columns(&self) -> &[String] {
        &self.columns
    }

    fn schema(&self) -> Option<&Schema> {
        self.schema.as_ref()
    }

    fn next_row(&mut self) -> Option<Result<Row, DataError>> {
        self.rows.next().map(Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_pad_and_cut_to_their_columns() {
        let mut rows = Rows::new(["a", "b"]);
        rows.push([Value::Int(1)]);
        rows.push([Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(rows.rows()[0], [Value::Int(1), Value::Null]);
        assert_eq!(rows.rows()[1], [Value::Int(1), Value::Int(2)]);
        assert_eq!(rows.column_index("b"), Some(1));
        assert_eq!(rows.column(0).count(), 2);
    }

    #[test]
    fn a_source_collects_back_into_the_same_rows() {
        let mut rows =
            Rows::new(["x"]).with_schema(Schema::new([Field::new("x", DataType::Integer)]));
        rows.push([Value::Int(7)]);
        let back = rows.clone().into_source().collect_rows().unwrap();
        assert_eq!(back.rows(), rows.rows());
        assert_eq!(back.schema(), rows.schema());
    }

    #[test]
    fn errors_name_their_line() {
        assert_eq!(DataError::at(3, "bad").to_string(), "line 3: bad");
        assert_eq!(DataError::new("bad").to_string(), "bad");
    }
}
