//! SQL-shaped result sets (#235): a query's rows as a database shell shows
//! them.
//!
//! A [`ResultSet`] is column names, their types (a [`Schema`], the model a
//! row source carries) and rows, rendered with:
//!
//! - **typed alignment**: integers, floats and decimals right-justified,
//!   booleans centred, text, dates, timestamps and everything else left. A
//!   column with no type in the schema is right-justified when every
//!   sampled non-null cell is a number.
//! - **NULL styling**: a null cell reads `NULL` in the `table.null` style
//!   (dim italic), so it never looks like an empty string, which stays
//!   empty.
//! - **a row count**: `(3 rows)`, `(1 row)`, with the elapsed time when the
//!   caller gives one: `(3 rows, 12ms)`.
//!
//! There is no database connection: rows come from an adapter ([`Rows`],
//! a [`RowWindow`] of any [`RowSource`]) or from the caller through
//! [`VirtualRows`]. Rendering goes through `rich_ext`'s
//! [`VirtualTable`], so a large result shows one window
//! ([`limit`](ResultSet::limit) rows from [`offset`](ResultSet::offset),
//! default the first [`DEFAULT_LIMIT`]) with its position above the count.
//!
//! ```
//! use std::time::Duration;
//!
//! use rich::Console;
//! use rich_data::sql::ResultSet;
//! use rich_data::{DataType, Field, Rows, Schema, Value};
//!
//! let schema = Schema::new([
//!     Field::new("id", DataType::Integer),
//!     Field::new("name", DataType::String),
//!     Field::new("active", DataType::Boolean),
//!     Field::new("balance", DataType::decimal()),
//! ]);
//! let mut rows = Rows::new(["id", "name", "active", "balance"]).with_schema(schema);
//! rows.push([Value::Int(1), "ada".into(), "true".into(), Value::Float(12.5)]);
//! rows.push([Value::Int(2), "".into(), "false".into(), Value::Null]);
//! rows.push([Value::Int(10), Value::Null, "true".into(), Value::Float(-3.0)]);
//!
//! let result = ResultSet::new(rows).elapsed(Duration::from_millis(12));
//! let out = Console::builder().width(50).build().render_export(&result);
//! assert_eq!(
//!     out,
//!     "\
//! ┏━━━━┳━━━━━━┳━━━━━━━━┳━━━━━━━━━┓
//! ┃ id ┃ name ┃ active ┃ balance ┃
//! ┡━━━━╇━━━━━━╇━━━━━━━━╇━━━━━━━━━┩
//! │  1 │ ada  │  true  │    12.5 │
//! │  2 │      │ false  │    NULL │
//! │ 10 │ NULL │  true  │      -3 │
//! └────┴──────┴────────┴─────────┘
//! (3 rows, 12ms)
//! "
//! );
//! ```

use std::time::Duration;

use rich::{Console, ConsoleOptions, Justify, Renderable, Segment};
use rich_ext::table::virtualized::{DEFAULT_SAMPLE, MAX_HEIGHT};
use rich_ext::table::{Column, VirtualRows, VirtualTable};

use crate::window::RowWindow;
use crate::{DataError, DataType, Row, RowSource, Rows, Schema, Value};

/// Rows a [`ResultSet`] shows by default.
pub const DEFAULT_LIMIT: usize = 1_000;

/// How a column of `data_type` is justified: numbers right, booleans
/// centred, everything else left.
pub fn alignment(data_type: &DataType) -> Justify {
    match data_type {
        DataType::Integer | DataType::Float | DataType::Decimal { .. } => Justify::Right,
        DataType::Boolean => Justify::Center,
        _ => Justify::Left,
    }
}

/// A column per name, justified by its type in `schema` (by name, else by
/// position), or, without one, right-justified when every non-null cell of
/// `sample` in that column is a number.
pub fn typed_columns(columns: &[String], schema: Option<&Schema>, sample: &[Row]) -> Vec<Column> {
    columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let field = schema.and_then(|s| s.field(name).or_else(|| s.fields().get(index)));
            let justify = match field.map(|f| f.data_type()) {
                Some(DataType::Any | DataType::Unknown) | None => {
                    let mut cells = sample
                        .iter()
                        .filter_map(|row| row.get(index))
                        .filter(|v| !matches!(v, Value::Null))
                        .peekable();
                    let numeric = cells.peek().is_some()
                        && cells.all(|v| matches!(v, Value::Int(_) | Value::Float(_)));
                    if numeric {
                        Justify::Right
                    } else {
                        Justify::Left
                    }
                }
                Some(data_type) => alignment(data_type),
            };
            Column::new(name.clone()).justify(justify)
        })
        .collect()
}

/// A query result: columns, their types and rows; see the
/// [module docs](self).
#[derive(Clone, Debug)]
pub struct ResultSet<S = Rows> {
    columns: Vec<String>,
    schema: Option<Schema>,
    source: S,
    offset: usize,
    limit: usize,
    elapsed: Option<Duration>,
    null: String,
    title: Option<String>,
}

impl ResultSet<Rows> {
    /// Rows held in memory, typed by their schema.
    pub fn new(rows: Rows) -> Self {
        let columns = rows.columns().to_vec();
        let schema = rows.schema().cloned();
        ResultSet::from_parts(columns, schema, rows)
    }
}

impl ResultSet<RowWindow> {
    /// One pass over `source`, keeping the `limit` rows from `offset` and
    /// counting the rest: any length of input in one window of memory.
    pub fn read(source: impl RowSource, offset: usize, limit: usize) -> Result<Self, DataError> {
        Ok(ResultSet::window(RowWindow::read(source, offset, limit)?))
    }

    /// A window already read.
    pub fn window(window: RowWindow) -> Self {
        let columns = window.columns().to_vec();
        let schema = window.schema().cloned();
        let (offset, limit) = (window.start(), window.rows().len());
        ResultSet::from_parts(columns, schema, window)
            .offset(offset)
            .limit(limit)
    }
}

impl<S: VirtualRows> ResultSet<S> {
    /// Rows from any source under `columns`, typed by `schema` (one field
    /// per column, matched by name, else by position).
    pub fn from_parts(
        columns: impl IntoIterator<Item = impl Into<String>>,
        schema: Option<Schema>,
        source: S,
    ) -> Self {
        ResultSet {
            columns: columns.into_iter().map(Into::into).collect(),
            schema,
            source,
            offset: 0,
            limit: DEFAULT_LIMIT,
            elapsed: None,
            null: "NULL".to_string(),
            title: None,
        }
    }

    /// Show rows from `offset` (0-based).
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Show at most `rows` rows (default [`DEFAULT_LIMIT`], at most
    /// `MAX_HEIGHT`).
    pub fn limit(mut self, rows: usize) -> Self {
        self.limit = rows.min(MAX_HEIGHT);
        self
    }

    /// How long the query took, for the footer.
    pub fn elapsed(mut self, elapsed: Duration) -> Self {
        self.elapsed = Some(elapsed);
        self
    }

    /// The marker for null cells (default `NULL`).
    pub fn null_marker(mut self, marker: impl Into<String>) -> Self {
        self.null = marker.into();
        self
    }

    /// A title above the table (console markup).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The column names.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The schema the columns are typed by.
    pub fn schema(&self) -> Option<&Schema> {
        self.schema.as_ref()
    }

    /// The source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// How many rows the result has, when the source knows.
    pub fn row_count(&self) -> Option<usize> {
        self.source.row_count()
    }

    /// The footer: `(3 rows)`, `(1 row)`, `(1,001+ rows)` when the source
    /// cannot count past the window, with the elapsed time when given
    /// (`(3 rows, 12ms)`).
    pub fn footer(&self, ascii: bool) -> String {
        let count = match self.row_count() {
            Some(count) => rows(count, ""),
            None => {
                let page = self.table(true).page();
                match page.total {
                    Some(total) => rows(total, ""),
                    None => rows(page.range().end + usize::from(page.more), "+"),
                }
            }
        };
        match self.elapsed {
            Some(elapsed) => format!(
                "({count}, {})",
                rich_ext::format::duration_with(elapsed, ascii)
            ),
            None => format!("({count})"),
        }
    }

    /// Whether the table shows only part of the rows.
    pub fn is_windowed(&self) -> bool {
        match self.row_count() {
            Some(count) => (self.offset > 0 && count > 0) || count > self.limit,
            None => true,
        }
    }

    /// The result as a virtualised table (without the footer), typed and
    /// with nulls marked; change its frame with its builders.
    pub fn to_virtual_table(&self) -> VirtualTable<&S> {
        self.table(self.is_windowed())
    }

    fn table(&self, position: bool) -> VirtualTable<&S> {
        let sample = self.source.rows(0, DEFAULT_SAMPLE);
        let mut table = VirtualTable::new(
            typed_columns(&self.columns, self.schema.as_ref(), &sample),
            &self.source,
        )
        .offset(self.offset)
        .height(self.limit)
        .null_marker(self.null.clone())
        .fit_window(true)
        .show_position(position);
        if let Some(title) = &self.title {
            table = table.title(title.clone());
        }
        table
    }
}

/// `3 rows`, `1 row`, with a suffix after the number.
fn rows(count: usize, suffix: &str) -> String {
    let number = rich_ext::format::number(i64::try_from(count).unwrap_or(i64::MAX));
    let noun = if count == 1 && suffix.is_empty() {
        "row"
    } else {
        "rows"
    };
    format!("{number}{suffix} {noun}")
}

impl<S: VirtualRows> Renderable for ResultSet<S> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.to_virtual_table()
            .footnote(self.footer(console.ascii_only()))
            .rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.to_virtual_table()
            .footnote(self.footer(console.ascii_only()))
            .measure(console, options)
    }
}

#[cfg(test)]
mod tests {
    use rich_ext::table::FnRows;

    use super::*;
    use crate::Field;

    fn render(result: &impl Renderable, width: usize) -> String {
        Console::builder()
            .width(width)
            .color_system(None)
            .build()
            .render_export(result)
    }

    #[test]
    fn counts_read_as_words() {
        let one = ResultSet::new({
            let mut rows = Rows::new(["x"]);
            rows.push([Value::Int(1)]);
            rows
        });
        assert_eq!(one.footer(false), "(1 row)");
        assert_eq!(ResultSet::new(Rows::new(["x"])).footer(false), "(0 rows)");
        let many = ResultSet::from_parts(["x"], None, FnRows::new(Some(1_234_567), |_| None));
        assert_eq!(many.footer(false), "(1,234,567 rows)");
    }

    #[test]
    fn columns_without_types_align_by_their_cells() {
        let sample = vec![
            vec![Value::Int(1), "a".into(), Value::Null, Value::Null],
            vec![Value::Float(2.5), "b".into(), Value::Null, Value::Int(3)],
        ];
        let names: Vec<String> = ["n", "s", "empty", "mixed"].map(String::from).to_vec();
        let columns = typed_columns(&names, None, &sample);
        let justify: Vec<Justify> = columns.iter().map(|c| c.column_options().justify).collect();
        assert_eq!(
            justify,
            [Justify::Right, Justify::Left, Justify::Left, Justify::Right]
        );
    }

    #[test]
    fn schema_types_win_over_cells_and_match_by_name() {
        let schema = Schema::new([
            Field::new("b", DataType::Integer),
            Field::new("a", DataType::Date),
        ]);
        let sample = vec![vec![Value::Int(1), "x".into()]];
        let names: Vec<String> = ["a", "b"].map(String::from).to_vec();
        let columns = typed_columns(&names, Some(&schema), &sample);
        assert_eq!(columns[0].column_options().justify, Justify::Left);
        assert_eq!(columns[1].column_options().justify, Justify::Right);
    }

    #[test]
    fn a_large_result_shows_its_window_and_count() {
        let source = FnRows::new(Some(1_000_000), |i| Some(vec![Value::from(i)]));
        let result = ResultSet::from_parts(["i"], None, source)
            .offset(10)
            .limit(2);
        let out = render(&result, 40);
        assert!(
            out.ends_with("│ 10 │\n│ 11 │\n└────┘\nrows 11–12 of 1,000,000\n(1,000,000 rows)\n"),
            "{out}"
        );
    }

    #[test]
    fn an_uncounted_window_says_at_least() {
        let mut rows = Rows::new(["n"]);
        for n in 0..50 {
            rows.push([Value::Int(n)]);
        }
        let window = RowWindow::reader()
            .len(5)
            .count(false)
            .read(rows.into_source())
            .unwrap();
        let result = ResultSet::window(window);
        assert_eq!(result.footer(false), "(6+ rows)");
        let out = render(&result, 30);
        assert!(out.ends_with("rows 1–5 of 6+\n(6+ rows)\n"), "{out}");
    }

    #[test]
    fn the_measurement_covers_the_footer() {
        let mut rows = Rows::new(["n"]);
        rows.push([Value::Int(1)]);
        let result = ResultSet::new(rows).elapsed(Duration::from_millis(12));
        let console = Console::builder().width(40).build();
        let measured = rich::measure::Measurement::get(&console, &console.options(), &result);
        assert_eq!(measured.maximum, "(1 row, 12ms)".len());
    }

    #[test]
    fn a_short_result_has_no_position_line() {
        let mut rows = Rows::new(["a"]);
        rows.push([Value::from("")]);
        let out = render(&ResultSet::new(rows).title("q"), 30);
        assert!(!out.contains("rows 1"), "{out}");
        assert!(out.ends_with("(1 row)\n"), "{out}");
    }
}
