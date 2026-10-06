//! Windows of rows for `rich_ext`'s virtualised tables (#260).
//!
//! [`VirtualTable`] fetches rows by index
//! through [`VirtualRows`]. [`Rows`] implements it directly. A [`RowSource`]
//! reads forward only, so [`RowWindow`] reads one pass of it and keeps just
//! the window, the first rows (to sample column widths from) and the count:
//! a file of any length costs one window of memory.
//!
//! ```
//! use rich::Console;
//! use rich_data::window::RowWindow;
//! use rich_data::{Rows, Value};
//!
//! let mut rows = Rows::new(["n", "square"]);
//! for n in 0..100_000i64 {
//!     rows.push([Value::Int(n), Value::Int(n * n)]);
//! }
//! let window = RowWindow::reader()
//!     .offset(50_000)
//!     .len(2)
//!     .read(rows.into_source())
//!     .unwrap();
//! assert_eq!((window.start(), window.rows().len(), window.total()), (50_000, 2, Some(100_000)));
//!
//! let out = Console::builder().width(40).build().render_export(&window.to_virtual_table());
//! assert!(out.contains("│ 50001 │ 2500100001 │"), "{out}");
//! assert!(out.ends_with("rows 50,001–50,002 of 100,000\n"), "{out}");
//! ```

use std::collections::VecDeque;

use rich_ext::table::virtualized::{DEFAULT_SAMPLE, MAX_HEIGHT, MAX_SAMPLE};
use rich_ext::table::{VirtualRows, VirtualTable};

use crate::sql::typed_columns;
use crate::{DataError, Row, RowSource, Rows, Schema, Value};

impl VirtualRows for Rows {
    fn row_count(&self) -> Option<usize> {
        Some(self.len())
    }

    fn row(&self, index: usize) -> Option<Vec<Value>> {
        self.rows().get(index).cloned()
    }

    fn rows(&self, start: usize, len: usize) -> Vec<Vec<Value>> {
        let rows = Rows::rows(self);
        let start = start.min(rows.len());
        let end = start.saturating_add(len).min(rows.len());
        rows[start..end].to_vec()
    }
}

/// How [`RowWindow`] reads a source; from [`RowWindow::reader`].
#[derive(Clone, Debug)]
pub struct WindowReader {
    offset: usize,
    len: usize,
    sample: usize,
    count: bool,
}

impl Default for WindowReader {
    fn default() -> Self {
        WindowReader {
            offset: 0,
            len: 20,
            sample: DEFAULT_SAMPLE,
            count: true,
        }
    }
}

impl WindowReader {
    /// Start the window at row `offset` (0-based, default 0).
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Keep `rows` rows (default 20, at most `MAX_HEIGHT`).
    pub fn len(mut self, rows: usize) -> Self {
        self.len = rows.min(MAX_HEIGHT);
        self
    }

    /// Also keep the first `rows` rows, for column widths and alignment
    /// (default `DEFAULT_SAMPLE`, at most `MAX_SAMPLE`).
    pub fn sample(mut self, rows: usize) -> Self {
        self.sample = rows.min(MAX_SAMPLE);
        self
    }

    /// Read to the end to count the rows (default on). Off, reading stops one
    /// row past the window, so the position says `of 1,041+`.
    pub fn count(mut self, count: bool) -> Self {
        self.count = count;
        self
    }

    /// Read `source` once, keeping the window. An offset past the end gives
    /// the last full window, as a [`VirtualTable`] does. A row that cannot be
    /// read is an error, wherever it is.
    pub fn read(&self, mut source: impl RowSource) -> Result<RowWindow, DataError> {
        let columns = source.columns().to_vec();
        let schema = source.schema().cloned();
        let width = columns.len();
        let end = self.offset.saturating_add(self.len);
        let mut head = Vec::new();
        let mut ring: VecDeque<Row> = VecDeque::with_capacity(self.len);
        let mut next = None;
        let mut index = 0usize;
        while let Some(row) = source.next_row() {
            let mut row = row?;
            row.resize(width, Value::Null);
            if index < self.sample {
                head.push(row.clone());
            }
            if index < end {
                if self.len > 0 {
                    if ring.len() == self.len {
                        ring.pop_front();
                    }
                    ring.push_back(row);
                }
            } else if !self.count {
                next = Some(row);
                break;
            }
            index += 1;
        }
        let start = index.min(end) - ring.len();
        Ok(RowWindow {
            columns,
            schema,
            head,
            start,
            rows: ring.into(),
            total: next.is_none().then_some(index),
            next,
        })
    }
}

/// One window of a [`RowSource`], with its first rows and its count; see
/// the [module docs](self).
#[derive(Clone, Debug)]
pub struct RowWindow {
    columns: Vec<String>,
    schema: Option<Schema>,
    head: Vec<Row>,
    start: usize,
    rows: Vec<Row>,
    total: Option<usize>,
    next: Option<Row>,
}

impl RowWindow {
    /// How to read: offset, length, sample and counting.
    pub fn reader() -> WindowReader {
        WindowReader::default()
    }

    /// The rows `offset..offset + len` of `source`, counting the rest.
    pub fn read(source: impl RowSource, offset: usize, len: usize) -> Result<Self, DataError> {
        Self::reader().offset(offset).len(len).read(source)
    }

    /// The column names.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The source's schema, if it had one.
    pub fn schema(&self) -> Option<&Schema> {
        self.schema.as_ref()
    }

    /// The 0-based index of the window's first row.
    pub fn start(&self) -> usize {
        self.start
    }

    /// The window's rows.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// How many rows the source had, when it was read to the end.
    pub fn total(&self) -> Option<usize> {
        self.total
    }

    /// The window as a virtualised table: typed alignment from the schema
    /// (or the sampled cells), positioned on the window.
    pub fn to_virtual_table(&self) -> VirtualTable<&RowWindow> {
        let sample = if self.head.is_empty() {
            &self.rows
        } else {
            &self.head
        };
        VirtualTable::new(
            typed_columns(&self.columns, self.schema.as_ref(), sample),
            self,
        )
        .offset(self.start)
        .height(self.rows.len())
        .fit_window(true)
    }
}

impl VirtualRows for RowWindow {
    fn row_count(&self) -> Option<usize> {
        self.total
    }

    fn row(&self, index: usize) -> Option<Vec<Value>> {
        if let Some(offset) = index.checked_sub(self.start) {
            if let Some(row) = self.rows.get(offset) {
                return Some(row.clone());
            }
            if offset == self.rows.len() {
                if let Some(next) = &self.next {
                    return Some(next.clone());
                }
            }
        }
        self.head.get(index).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(count: i64) -> Rows {
        let mut rows = Rows::new(["n"]);
        for n in 0..count {
            rows.push([Value::Int(n)]);
        }
        rows
    }

    #[test]
    fn a_window_past_the_end_is_the_last_full_window() {
        let window = RowWindow::read(numbers(10).into_source(), 50, 3).unwrap();
        assert_eq!(window.start(), 7);
        assert_eq!(window.rows()[0], [Value::Int(7)]);
        assert_eq!(window.total(), Some(10));
        let short = RowWindow::read(numbers(2).into_source(), 0, 5).unwrap();
        assert_eq!((short.start(), short.rows().len()), (0, 2));
    }

    #[test]
    fn without_counting_one_row_past_the_window_is_kept() {
        let window = RowWindow::reader()
            .offset(4)
            .len(2)
            .count(false)
            .read(numbers(1_000).into_source())
            .unwrap();
        assert_eq!(window.total(), None);
        assert_eq!(window.row(6), Some(vec![Value::Int(6)]));
        assert_eq!(window.row(7), None);
        let table = window.to_virtual_table();
        assert_eq!(table.page().position(false), "rows 5–6 of 7+");
    }

    #[test]
    fn the_head_and_the_window_answer_by_index() {
        let window = RowWindow::reader()
            .offset(500)
            .len(2)
            .sample(3)
            .read(numbers(1_000).into_source())
            .unwrap();
        assert_eq!(window.row(2), Some(vec![Value::Int(2)]));
        assert_eq!(window.row(3), None);
        assert_eq!(window.row(501), Some(vec![Value::Int(501)]));
        assert_eq!(VirtualRows::rows(&window, 0, 10).len(), 3);
    }

    #[test]
    fn read_errors_surface() {
        struct Broken(usize);
        impl RowSource for Broken {
            fn columns(&self) -> &[String] {
                &[]
            }
            fn next_row(&mut self) -> Option<Result<Row, DataError>> {
                self.0 += 1;
                Some(if self.0 < 3 {
                    Ok(Vec::new())
                } else {
                    Err(DataError::at(self.0, "bad row"))
                })
            }
        }
        let error = RowWindow::read(Broken(0), 0, 1).unwrap_err();
        assert_eq!(error.to_string(), "line 3: bad row");
    }
}
