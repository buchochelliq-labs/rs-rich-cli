//! Virtualised tables (#260): one window of a large row source.
//!
//! A [`VirtualTable`] renders the rows `offset..offset + height` of a
//! [`VirtualRows`] source and nothing else, so a million-row source costs one
//! window of memory. The source is anything that can fetch a row by index: a
//! `Vec` of rows, a closure ([`FnRows`]), or a caller's own type (a file
//! index, a query cursor, `rs-rich-data`'s rows).
//!
//! Column widths do not follow the window, so scrolling never makes columns
//! jump: each is fixed by the caller ([`widths`](VirtualTable::widths), or a
//! core `ColumnOptions::width`) or sampled once from the header and the first
//! [`sample`](VirtualTable::sample) rows, capped at
//! [`max_column_width`](VirtualTable::max_column_width). Cells do not wrap by
//! default, so a row is one line and a viewport of N lines holds a known
//! number of rows ([`chrome_lines`](VirtualTable::chrome_lines)); longer
//! text ends in an ellipsis. A position line under the table says where the
//! window is (`rows 1,001–1,040 of 1,000,000`), or how many rows are known to
//! exist when the source cannot count them (`rows 1–40 of 41+`).
//!
//! A very wide table gets a window of columns too (#702): a
//! [column offset](VirtualTable::column_offset) and a
//! [count](VirtualTable::visible_columns) or
//! [as many as fit](VirtualTable::fit_columns), with
//! [frozen columns](VirtualTable::frozen_columns) kept on the left. Only
//! the columns shown are fetched ([`VirtualRows::rows_columns`]), sampled
//! and laid out, and the position line adds `columns 41–60 of 2,000`.
//!
//! This is a static renderer: interactive scrolling belongs to
//! `rs-rich-interact`'s viewport, which moves the window with
//! [`set_offset`](VirtualTable::set_offset) or
//! [`scroll_by`](VirtualTable::scroll_by) (and across with
//! [`scroll_columns_by`](VirtualTable::scroll_columns_by)) and renders
//! again.
//!
//! ```
//! use rich::{Console, Justify};
//! use rich_ext::table::{Column, FnRows, Value, VirtualTable};
//!
//! // Ten million rows, computed on demand: none of them is ever stored.
//! let rows = FnRows::new(Some(10_000_000), |i| {
//!     Some(vec![Value::Int(i as i64 + 1), format!("item {i}").into()])
//! });
//! let table = VirtualTable::new(
//!     [Column::new("id").justify(Justify::Right), Column::new("name")],
//!     rows,
//! )
//! .widths([8, 10])
//! .offset(1_000)
//! .height(3);
//!
//! let out = Console::builder().width(40).build().render_export(&table);
//! assert_eq!(
//!     out,
//!     "\
//! ┏━━━━━━━━━━┳━━━━━━━━━━━━┓
//! ┃       id ┃ name       ┃
//! ┡━━━━━━━━━━╇━━━━━━━━━━━━┩
//! │     1001 │ item 1000  │
//! │     1002 │ item 1001  │
//! │     1003 │ item 1002  │
//! └──────────┴────────────┘
//! rows 1,001–1,003 of 10,000,000
//! "
//! );
//! ```
//!
//! Without [`widths`](VirtualTable::widths), the widths would be sampled
//! from the header and the first 100 rows (`id` up to `100`, `name` up to
//! `item 99`), and the window's longer values would end in an ellipsis
//! (`10…`). Fix widths when the caller knows better, or sample more rows.

use std::sync::{Arc, Mutex, PoisonError};

use rich::{Console, ConsoleOptions, Justify, Overflow, Renderable, Segment, Table, Text};

use super::data::normalize;
use super::stream::table_lines;
use super::{frame_builders, style, Column, Frame, Value};

/// The most rows one window shows; a larger [`height`](VirtualTable::height)
/// is cut to this.
pub const MAX_HEIGHT: usize = 10_000;

/// The most rows sampled for column widths.
pub const MAX_SAMPLE: usize = 10_000;

/// Rows sampled for column widths by default.
pub const DEFAULT_SAMPLE: usize = 100;

/// The widest a sampled column gets by default, in cells.
pub const DEFAULT_MAX_COLUMN_WIDTH: usize = 40;

/// Rows fetched by index, for a [`VirtualTable`].
///
/// Only [`row`](VirtualRows::row) is required. A source that can fetch a
/// range more cheaply than row by row (a file index, a cursor) overrides
/// [`rows`](VirtualRows::rows); one that knows its length overrides
/// [`row_count`](VirtualRows::row_count). A row may have more or fewer
/// cells than the table has columns: missing cells are null and extra cells
/// are dropped.
///
/// The methods take `&self` because rendering does. A source that reads
/// from something stateful (a cursor) keeps that state behind a lock.
pub trait VirtualRows {
    /// How many rows there are, when the source knows.
    fn row_count(&self) -> Option<usize> {
        None
    }

    /// The row at `index` (0-based), or `None` past the end.
    fn row(&self, index: usize) -> Option<Vec<Value>>;

    /// Up to `len` rows from `start`, fewer at the end. The default calls
    /// [`row`](VirtualRows::row) until it returns `None`.
    fn rows(&self, start: usize, len: usize) -> Vec<Vec<Value>> {
        (start..start.saturating_add(len))
            .map_while(|index| self.row(index))
            .collect()
    }

    /// Up to `len` rows from `start`, each with only the cells of
    /// `columns` (0-based, in that order; a cell past the row's end is
    /// null): what a [column window](VirtualTable::visible_columns) of a
    /// wide table fetches. The default calls [`rows`](VirtualRows::rows)
    /// and picks the cells; a source that can read some columns without
    /// the rest (a columnar file, a query, cells computed on demand such
    /// as [`FnCells`]) overrides it.
    fn rows_columns(&self, start: usize, len: usize, columns: &[usize]) -> Vec<Vec<Value>> {
        self.rows(start, len)
            .iter()
            .map(|row| pick(row, columns))
            .collect()
    }
}

/// The cells of `columns` from `row`, null past its end.
fn pick(row: &[Value], columns: &[usize]) -> Vec<Value> {
    columns
        .iter()
        .map(|&c| row.get(c).cloned().unwrap_or(Value::Null))
        .collect()
}

impl VirtualRows for [Vec<Value>] {
    fn row_count(&self) -> Option<usize> {
        Some(self.len())
    }

    fn row(&self, index: usize) -> Option<Vec<Value>> {
        self.get(index).cloned()
    }

    fn rows(&self, start: usize, len: usize) -> Vec<Vec<Value>> {
        let start = start.min(self.len());
        let end = start.saturating_add(len).min(self.len());
        self[start..end].to_vec()
    }

    fn rows_columns(&self, start: usize, len: usize, columns: &[usize]) -> Vec<Vec<Value>> {
        let start = start.min(self.len());
        let end = start.saturating_add(len).min(self.len());
        self[start..end]
            .iter()
            .map(|row| pick(row, columns))
            .collect()
    }
}

impl VirtualRows for Vec<Vec<Value>> {
    fn row_count(&self) -> Option<usize> {
        Some(self.len())
    }

    fn row(&self, index: usize) -> Option<Vec<Value>> {
        self.as_slice().row(index)
    }

    fn rows(&self, start: usize, len: usize) -> Vec<Vec<Value>> {
        self.as_slice().rows(start, len)
    }

    fn rows_columns(&self, start: usize, len: usize, columns: &[usize]) -> Vec<Vec<Value>> {
        self.as_slice().rows_columns(start, len, columns)
    }
}

macro_rules! forward_rows {
    ($($ptr:ty),*) => {$(
        impl<T: VirtualRows + ?Sized> VirtualRows for $ptr {
            fn row_count(&self) -> Option<usize> {
                (**self).row_count()
            }

            fn row(&self, index: usize) -> Option<Vec<Value>> {
                (**self).row(index)
            }

            fn rows(&self, start: usize, len: usize) -> Vec<Vec<Value>> {
                (**self).rows(start, len)
            }

            fn rows_columns(
                &self,
                start: usize,
                len: usize,
                columns: &[usize],
            ) -> Vec<Vec<Value>> {
                (**self).rows_columns(start, len, columns)
            }
        }
    )*};
}
forward_rows!(&T, Box<T>, Arc<T>);

/// Cells computed by a closure: `cell(row, column)` for `rows` rows of
/// `columns` columns. Only the cells a window shows (and the width sample
/// of the columns it shows) are ever computed, so a table can be as wide
/// as it is long.
///
/// ```
/// use rich_ext::table::{FnCells, Value, VirtualRows};
///
/// let grid = FnCells::new(1_000_000, 2_000, |r, c| Value::from(r * c));
/// assert_eq!(grid.rows_columns(10, 2, &[3, 1_999]), vec![
///     vec![Value::from(30), Value::from(19_990)],
///     vec![Value::from(33), Value::from(21_989)],
/// ]);
/// assert_eq!(grid.row(1_000_000), None);
/// ```
pub struct FnCells<F> {
    rows: usize,
    columns: usize,
    cell: F,
}

impl<F> FnCells<F>
where
    F: Fn(usize, usize) -> Value,
{
    /// `rows` rows of `columns` cells from `cell(row, column)`.
    pub fn new(rows: usize, columns: usize, cell: F) -> Self {
        FnCells {
            rows,
            columns,
            cell,
        }
    }
}

impl<F> std::fmt::Debug for FnCells<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FnCells")
            .field("rows", &self.rows)
            .field("columns", &self.columns)
            .finish_non_exhaustive()
    }
}

impl<F> VirtualRows for FnCells<F>
where
    F: Fn(usize, usize) -> Value,
{
    fn row_count(&self) -> Option<usize> {
        Some(self.rows)
    }

    fn row(&self, index: usize) -> Option<Vec<Value>> {
        (index < self.rows).then(|| (0..self.columns).map(|c| (self.cell)(index, c)).collect())
    }

    fn rows_columns(&self, start: usize, len: usize, columns: &[usize]) -> Vec<Vec<Value>> {
        let end = start.saturating_add(len).min(self.rows);
        (start.min(end)..end)
            .map(|r| {
                columns
                    .iter()
                    .map(|&c| {
                        if c < self.columns {
                            (self.cell)(r, c)
                        } else {
                            Value::Null
                        }
                    })
                    .collect()
            })
            .collect()
    }
}

/// Rows computed by a closure: `row(index)` returns the row at `index`, or
/// `None` past the end. The count is optional.
///
/// ```
/// use rich_ext::table::{FnRows, Value, VirtualRows};
///
/// let squares = FnRows::new(None, |i| (i < 5).then(|| vec![Value::from(i * i)]));
/// assert_eq!(squares.row(3), Some(vec![Value::Int(9)]));
/// assert_eq!(squares.rows(3, 10).len(), 2);
/// assert_eq!(squares.row_count(), None);
/// ```
pub struct FnRows<F> {
    count: Option<usize>,
    row: F,
}

impl<F> FnRows<F>
where
    F: Fn(usize) -> Option<Vec<Value>>,
{
    /// Rows from `row`, `count` of them when known.
    pub fn new(count: Option<usize>, row: F) -> Self {
        FnRows { count, row }
    }
}

impl<F> std::fmt::Debug for FnRows<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FnRows")
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}

impl<F> VirtualRows for FnRows<F>
where
    F: Fn(usize) -> Option<Vec<Value>>,
{
    fn row_count(&self) -> Option<usize> {
        self.count
    }

    fn row(&self, index: usize) -> Option<Vec<Value>> {
        if self.count.is_some_and(|count| index >= count) {
            return None;
        }
        (self.row)(index)
    }
}

/// The rows one window shows, from [`VirtualTable::page`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Page {
    /// The 0-based index of the first row shown. It is the table's offset,
    /// moved back when that is past the end of a source that knows its
    /// length.
    pub start: usize,
    /// The rows shown, each with one cell per column shown (see
    /// [`columns`](Self::columns)).
    pub rows: Vec<Vec<Value>>,
    /// How many rows the source has: its own count, or the end when this
    /// window reached it.
    pub total: Option<usize>,
    /// Whether a row is known to follow the window.
    pub more: bool,
    /// The 0-based indices of the columns shown, in order: the frozen ones,
    /// then the window's. Every column when the table has no column window.
    pub columns: Vec<usize>,
    /// How many of [`columns`](Self::columns) are frozen.
    pub frozen: usize,
    /// How many columns the table has.
    pub column_total: usize,
}

impl Page {
    /// The 0-based indices of the rows shown.
    pub fn range(&self) -> std::ops::Range<usize> {
        self.start..self.start + self.rows.len()
    }

    /// The 0-based indices of the columns shown after the frozen ones.
    pub fn column_range(&self) -> std::ops::Range<usize> {
        match self.columns.get(self.frozen..) {
            Some([first, .., last]) => *first..*last + 1,
            Some([only]) => *only..*only + 1,
            _ => 0..0,
        }
    }

    /// Where the column window is, in words, when it leaves columns out:
    /// `columns 41–60 of 2,000` (counted from 1, frozen columns aside), or
    /// `column 7 of 12`. `None` when every column is shown. `ascii` writes
    /// the range with `-` instead of `–`.
    pub fn column_position(&self, ascii: bool) -> Option<String> {
        if self.columns.len() >= self.column_total {
            return None;
        }
        let number = |n: usize| crate::format::number(i64::try_from(n).unwrap_or(i64::MAX));
        let range = self.column_range();
        let total = number(self.column_total);
        Some(match range.len() {
            0 => format!("no columns of {total}"),
            1 => format!("column {} of {total}", number(range.start + 1)),
            _ => {
                let dash = if ascii { "-" } else { "–" };
                format!(
                    "columns {}{dash}{} of {total}",
                    number(range.start + 1),
                    number(range.end)
                )
            }
        })
    }

    /// Where the window is, in words: `rows 1,001–1,040 of 1,000,000`, `rows
    /// 1–40 of 41+` when the source cannot count past the window, `no rows`,
    /// or `no rows from 1,001` past the end of an uncounted source. `ascii`
    /// writes the range with `-` instead of `–`.
    pub fn position(&self, ascii: bool) -> String {
        let number = |n: usize| crate::format::number(i64::try_from(n).unwrap_or(i64::MAX));
        if self.rows.is_empty() {
            return match self.start {
                0 => "no rows".to_string(),
                start => format!("no rows from {}", number(start + 1)),
            };
        }
        let dash = if ascii { "-" } else { "–" };
        let range = format!(
            "rows {}{dash}{}",
            number(self.start + 1),
            number(self.start + self.rows.len())
        );
        match self.total {
            Some(total) => format!("{range} of {}", number(total)),
            None if self.more => {
                format!(
                    "{range} of {}+",
                    number(self.start.saturating_add(self.rows.len() + 1))
                )
            }
            None => range,
        }
    }
}

/// One window of a [`VirtualRows`] source as a table; see the
/// [module docs](self).
///
/// The frame (title, caption, box, edges, expand, border style) is set with
/// the same builders as [`TableData`](super::TableData).
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct VirtualTable<S> {
    source: S,
    columns: Vec<Column>,
    frame: Frame,
    offset: usize,
    height: usize,
    widths: Vec<Option<usize>>,
    sample: usize,
    max_width: usize,
    wrap: bool,
    position: bool,
    row_numbers: bool,
    fit_window: bool,
    null: Option<String>,
    footnote: Option<String>,
    /// Each column's sampled width, once sampled.
    sampled: Mutex<Vec<Option<usize>>>,
    window: ColumnWindow,
}

/// Which columns a [`VirtualTable`] shows: see
/// [`visible_columns`](VirtualTable::visible_columns).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ColumnWindow {
    offset: usize,
    count: Option<usize>,
    fit: bool,
    frozen: usize,
}

impl ColumnWindow {
    /// Whether it leaves any columns out, or could.
    fn on(&self) -> bool {
        self.offset > 0 || self.count.is_some() || self.fit || self.frozen > 0
    }
}

impl<S> std::fmt::Debug for VirtualTable<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualTable")
            .field("columns", &self.columns)
            .field("offset", &self.offset)
            .field("height", &self.height)
            .field("widths", &self.widths)
            .field("sample", &self.sample)
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

frame_builders!([S] VirtualTable<S>);

impl<S: VirtualRows> VirtualTable<S> {
    /// A window of 20 rows from the top of `source`, under `columns`.
    pub fn new(columns: impl IntoIterator<Item = Column>, source: S) -> Self {
        VirtualTable {
            source,
            columns: columns.into_iter().collect(),
            frame: Frame::default(),
            offset: 0,
            height: 20,
            widths: Vec::new(),
            sample: DEFAULT_SAMPLE,
            max_width: DEFAULT_MAX_COLUMN_WIDTH,
            wrap: false,
            position: true,
            row_numbers: false,
            fit_window: false,
            null: None,
            footnote: None,
            sampled: Mutex::new(Vec::new()),
            window: ColumnWindow::default(),
        }
    }

    /// Start the window at row `offset` (0-based).
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    /// Show `rows` rows (at most [`MAX_HEIGHT`]).
    pub fn height(mut self, rows: usize) -> Self {
        self.set_height(rows);
        self
    }

    /// Fix the column widths, in cells, in column order. A missing or `None`
    /// entry leaves that column to its `ColumnOptions::width`, or to the
    /// sample.
    pub fn widths(mut self, widths: impl IntoIterator<Item = impl Into<Option<usize>>>) -> Self {
        self.widths = widths.into_iter().map(Into::into).collect();
        self.resample();
        self
    }

    /// Sample the header and the first `rows` rows for column widths
    /// (default [`DEFAULT_SAMPLE`], at most [`MAX_SAMPLE`]). Zero samples
    /// only the header.
    pub fn sample(mut self, rows: usize) -> Self {
        self.sample = rows.min(MAX_SAMPLE);
        self.resample();
        self
    }

    /// Cap sampled widths at `cells` (default [`DEFAULT_MAX_COLUMN_WIDTH`],
    /// at least 1). Fixed widths are not capped.
    pub fn max_column_width(mut self, cells: usize) -> Self {
        self.max_width = cells.max(1);
        self.resample();
        self
    }

    /// Wrap long cells onto more lines instead of cutting them short with an
    /// ellipsis (default off, so a row is one line).
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Show the position line under the table (default on).
    pub fn show_position(mut self, show: bool) -> Self {
        self.position = show;
        self
    }

    /// Also widen sampled columns to fit the rows shown (default off), up to
    /// [`max_column_width`](Self::max_column_width). For a window that is
    /// rendered once: scrolling such a table can change its widths.
    pub fn fit_window(mut self, fit: bool) -> Self {
        self.fit_window = fit;
        self
    }

    /// Show each row's 1-based number in a first `#` column (default off).
    pub fn row_numbers(mut self, show: bool) -> Self {
        self.row_numbers = show;
        self
    }

    /// Show null cells as `marker` in the `table.null` style, instead of
    /// empty like an empty string. Column formatters are not called for
    /// them.
    pub fn null_marker(mut self, marker: impl Into<String>) -> Self {
        self.null = Some(marker.into());
        self.resample();
        self
    }

    /// A line under the position line, in the `table.position` style: a row
    /// count, a timing, a note.
    pub fn footnote(mut self, note: impl Into<String>) -> Self {
        self.footnote = Some(note.into());
        self
    }

    /// Start the column window at column `offset`, counted from 0 after the
    /// [frozen](Self::frozen_columns) ones. With a column window only the
    /// columns it shows are fetched, sampled and laid out; the position
    /// line adds `columns 41–60 of 2,000` when some are left out.
    pub fn column_offset(mut self, offset: usize) -> Self {
        self.window.offset = offset;
        self
    }

    /// Show `count` columns (at least 1) from the
    /// [column offset](Self::column_offset), besides the frozen ones. With
    /// [`fit_columns`](Self::fit_columns), at most `count`.
    pub fn visible_columns(mut self, count: usize) -> Self {
        self.window.count = Some(count.max(1));
        self
    }

    /// Show as many columns from the [column offset](Self::column_offset)
    /// as fit the width, besides the frozen ones, and always one (default
    /// off). Columns are measured by their widths (fixed, or sampled),
    /// sampled a few at a time, so a fit samples at most one column past
    /// those it shows.
    pub fn fit_columns(mut self, fit: bool) -> Self {
        self.window.fit = fit;
        self
    }

    /// Keep the first `count` columns on the left, whatever the column
    /// offset (an id column, a name): they are always shown and never
    /// scrolled.
    pub fn frozen_columns(mut self, count: usize) -> Self {
        self.window.frozen = count;
        self
    }

    /// Move the column window to column `offset` (after the frozen ones).
    pub fn set_column_offset(&mut self, offset: usize) {
        self.window.offset = offset;
    }

    /// Show `count` columns from the offset, or `None` for all of them (or
    /// as many as [fit](Self::fit_columns)).
    pub fn set_visible_columns(&mut self, count: Option<usize>) {
        self.window.count = count.map(|c| c.max(1));
    }

    /// Move the column window by `columns` (negative is left), stopping at
    /// the first column and at the [last offset](Self::max_column_offset).
    pub fn scroll_columns_by(&mut self, columns: isize) {
        let offset = self.window.offset.saturating_add_signed(columns);
        self.window.offset = offset.min(self.max_column_offset());
    }

    /// The column window's first column, after the frozen ones, as set.
    pub fn window_column_offset(&self) -> usize {
        self.window.offset
    }

    /// The largest column offset: the one whose window of
    /// [`visible_columns`](Self::visible_columns) ends at the last column,
    /// or that shows the last column alone without a count.
    pub fn max_column_offset(&self) -> usize {
        let scrollable = self.columns.len().saturating_sub(self.window.frozen);
        scrollable.saturating_sub(self.window.count.unwrap_or(1))
    }

    /// The 0-based indices of the columns a render `width` cells wide
    /// shows: the frozen ones, then the window's. Every column without a
    /// column window.
    pub fn shown_columns(&self, width: usize) -> Vec<usize> {
        let n = self.columns.len();
        if !self.window.on() {
            return (0..n).collect();
        }
        let frozen = self.window.frozen.min(n);
        let scrollable = n - frozen;
        let start = frozen + self.window.offset.min(scrollable.saturating_sub(1));
        let end = match self.window.count {
            Some(count) => start.saturating_add(count).min(n),
            None => n,
        };
        let mut shown: Vec<usize> = (0..frozen).collect();
        if !self.window.fit || start >= end {
            shown.extend(start..end);
            return shown;
        }
        // Fit: add columns while the table fits, sampling a batch at a time
        // that surely fits at the cap, and at least one.
        let mut widths = self.widths_of(&shown);
        if self.row_numbers {
            let last = self
                .row_count()
                .unwrap_or(self.offset.saturating_add(self.height));
            widths.insert(0, last.max(1).to_string().len());
        }
        let mut next = start;
        'fit: while next < end {
            let room = width.saturating_sub(self.table_width(&widths));
            let batch = (room / (self.max_width + 3)).max(1);
            let candidates: Vec<usize> = (next..end.min(next + batch)).collect();
            for (index, w) in candidates.iter().zip(self.widths_of(&candidates)) {
                widths.push(w);
                if shown.len() > frozen && self.table_width(&widths) > width {
                    break 'fit;
                }
                shown.push(*index);
            }
            next += candidates.len();
        }
        shown
    }

    /// The cells a table of columns `widths` wide takes, its borders and
    /// padding included.
    fn table_width(&self, widths: &[usize]) -> usize {
        let boxed = self.frame.box_set.is_some();
        let edges = if boxed && self.frame.show_edge { 2 } else { 0 };
        let dividers = if boxed {
            widths.len().saturating_sub(1)
        } else {
            0
        };
        widths.iter().map(|w| w + 2).sum::<usize>() + dividers + edges
    }

    /// Move the window to row `offset` (0-based).
    pub fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    /// Change how many rows the window shows (at most [`MAX_HEIGHT`]).
    pub fn set_height(&mut self, rows: usize) {
        self.height = rows.min(MAX_HEIGHT);
    }

    /// Move the window by `rows` (negative is up), stopping at the top and,
    /// when the source knows its length, at the last full window.
    pub fn scroll_by(&mut self, rows: isize) {
        let offset = self.offset.saturating_add_signed(rows);
        self.offset = match self.max_offset() {
            Some(max) => offset.min(max),
            None => offset,
        };
    }

    /// The window's first row (0-based), as set.
    pub fn window_offset(&self) -> usize {
        self.offset
    }

    /// How many rows the window shows.
    pub fn window_height(&self) -> usize {
        self.height
    }

    /// The source's row count, when it knows.
    pub fn row_count(&self) -> Option<usize> {
        self.source.row_count()
    }

    /// The offset of the last full window, when the source knows its length.
    pub fn max_offset(&self) -> Option<usize> {
        self.row_count()
            .map(|count| count.saturating_sub(self.height))
    }

    /// The columns.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The source.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// The source, to change. Call [`resample`](Self::resample) if the first
    /// rows changed.
    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }

    /// Forget the sampled widths, so the next render samples again.
    pub fn resample(&self) {
        self.sampled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// The rows the window shows, fetched from the source. An offset past
    /// the end of a source that knows its length moves back to the last full
    /// window; one past the end of a source that does not shows no rows.
    /// A window that [fits its columns](Self::fit_columns) to the width is
    /// laid out for 80 cells; [`page_at`](Self::page_at) takes the width.
    pub fn page(&self) -> Page {
        self.page_at(80)
    }

    /// The rows the window shows, with the columns a render `width` cells
    /// wide shows ([`shown_columns`](Self::shown_columns)): only those
    /// cells are fetched.
    pub fn page_at(&self, width: usize) -> Page {
        let columns = self.shown_columns(width);
        let count = self.source.row_count();
        let start = match count {
            Some(count) => self.offset.min(count.saturating_sub(self.height)),
            None => self.offset,
        };
        // One extra row says whether more follow when the source cannot count.
        let fetch = if count.is_some() {
            self.height
        } else {
            self.height + 1
        };
        let mut rows = if self.window.on() {
            self.source.rows_columns(start, fetch, &columns)
        } else {
            self.source.rows(start, fetch)
        };
        rows.truncate(fetch);
        let more = match count {
            Some(count) => start + rows.len().min(self.height) < count,
            None => rows.len() > self.height,
        };
        rows.truncate(self.height);
        let width = columns.len();
        let rows: Vec<Vec<Value>> = rows.into_iter().map(|row| normalize(row, width)).collect();
        let total = count.or_else(|| (!more && !rows.is_empty()).then(|| start + rows.len()));
        Page {
            start,
            rows,
            total,
            more,
            frozen: if self.window.on() {
                self.window.frozen.min(self.columns.len())
            } else {
                0
            },
            column_total: self.columns.len(),
            columns,
        }
    }

    /// Each column's width in cells: fixed, or sampled from the header and
    /// the first rows (and cached until [`resample`](Self::resample)). This
    /// samples every column; a column window samples only those it shows.
    pub fn column_widths(&self) -> Vec<usize> {
        let all: Vec<usize> = (0..self.columns.len()).collect();
        self.widths_of(&all)
    }

    /// Column `index`'s fixed width, if it has one.
    fn fixed_width(&self, index: usize) -> Option<usize> {
        self.widths
            .get(index)
            .copied()
            .flatten()
            .or(self.columns[index].column_options().width)
    }

    /// The widths of `indices`' columns, sampling those not sampled yet in
    /// one fetch of their cells.
    fn widths_of(&self, indices: &[usize]) -> Vec<usize> {
        let mut sampled = self.sampled.lock().unwrap_or_else(PoisonError::into_inner);
        if sampled.len() != self.columns.len() {
            *sampled = vec![None; self.columns.len()];
        }
        let missing: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&i| sampled[i].is_none() && self.fixed_width(i).is_none())
            .collect();
        if !missing.is_empty() {
            let mut widths: Vec<usize> = missing
                .iter()
                .map(|&i| Text::new(self.columns[i].header()).measurement().1)
                .collect();
            let rows = if missing.len() == self.columns.len() {
                // Every column: the rows as the source gives them.
                self.source.rows(0, self.sample)
            } else {
                self.source.rows_columns(0, self.sample, &missing)
            };
            for row in rows.into_iter().take(self.sample) {
                // Pad short rows as `page` does, so a missing cell is
                // sampled as the null marker it will show.
                let row = normalize(row, missing.len());
                for ((width, &index), value) in widths.iter_mut().zip(&missing).zip(&row) {
                    let cell = self.cell(&self.columns[index], value).measurement().1;
                    *width = (*width).max(cell);
                }
            }
            for (width, &index) in widths.into_iter().zip(&missing) {
                sampled[index] = Some(width.clamp(1, self.max_width));
            }
        }
        indices
            .iter()
            .map(|&i| self.fixed_width(i).or(sampled[i]).unwrap_or(self.max_width))
            .collect()
    }

    /// How many lines the table takes around its rows at `width` cells: the
    /// title, edges, header, caption, position line and footnote. A viewport
    /// of `lines` lines holds `lines - chrome_lines` rows when cells do not
    /// wrap.
    pub fn chrome_lines(&self, console: &Console, width: usize) -> usize {
        let options = console.options().update_width(width.max(1));
        let columns = self.shown_columns(width);
        let empty = Page {
            start: 0,
            rows: Vec::new(),
            total: None,
            more: false,
            frozen: 0,
            column_total: self.columns.len(),
            columns,
        };
        let table = self.build(console, &empty);
        table_lines(&table, console, &options).len()
            + usize::from(self.position)
            + usize::from(self.footnote.is_some())
    }

    /// The window as a core table (without the position line and footnote),
    /// its columns as many as the console's width shows.
    pub fn to_table(&self, console: &Console) -> Table {
        self.build(console, &self.page_at(console.width()))
    }

    fn cell(&self, column: &Column, value: &Value) -> Text {
        match (&self.null, value) {
            (Some(marker), Value::Null) => Text::new(marker.clone()),
            _ => column.cell(value),
        }
    }

    fn build(&self, console: &Console, page: &Page) -> Table {
        let mut widths = self.widths_of(&page.columns);
        if self.fit_window {
            for (k, (&index, width)) in page.columns.iter().zip(widths.iter_mut()).enumerate() {
                let column = &self.columns[index];
                if self.fixed_width(index).is_some() {
                    continue;
                }
                for row in &page.rows {
                    let cell = self.cell(column, &row[k]).measurement().1;
                    *width = (*width).max(cell.min(self.max_width));
                }
            }
        }
        let mut columns: Vec<Column> = Vec::with_capacity(page.columns.len() + 1);
        if self.row_numbers {
            let last = page
                .total
                .unwrap_or(page.start + page.rows.len())
                .max(page.start + page.rows.len());
            let options = rich::ColumnOptions {
                justify: Justify::Right,
                width: Some(last.max(1).to_string().len()),
                no_wrap: true,
                style: style(console, "table.row_number"),
                ..Default::default()
            };
            columns.push(Column::new("#").options(options));
        }
        for (&index, width) in page.columns.iter().zip(&widths) {
            let column = &self.columns[index];
            let mut options = column.column_options().clone();
            options.width = Some(*width);
            if !self.wrap {
                options.no_wrap = true;
                if options.overflow == Overflow::Fold {
                    options.overflow = Overflow::Ellipsis;
                }
            }
            columns.push(Column::new(column.header()).options(options));
        }
        let headers: Vec<Text> = columns.iter().map(|c| Text::new(c.header())).collect();
        let mut table = self.frame.table(&columns, &headers, true, true);
        let null_style = style(console, "table.null");
        for (index, row) in page.rows.iter().enumerate() {
            let mut cells = Vec::with_capacity(columns.len());
            if self.row_numbers {
                cells.push(Text::new((page.start + index + 1).to_string()));
            }
            for (&index, value) in page.columns.iter().zip(row) {
                let mut text = self.cell(&self.columns[index], value);
                if self.null.is_some() && matches!(value, Value::Null) {
                    let len = text.plain().len();
                    text.stylize(null_style.clone(), 0, len);
                }
                cells.push(text);
            }
            table.add_row_text(cells);
        }
        table
    }

    fn note_lines(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        text: String,
    ) -> Vec<Vec<Segment>> {
        let text = Text::styled(text, style(console, "table.position"))
            .no_wrap(true)
            .overflow(Overflow::Ellipsis);
        let mut options = options.clone();
        options.height = None;
        console.render_lines(&text, &options, false)
    }

    /// The position line: the rows, and the columns when some are left
    /// out.
    fn position_text(page: &Page, ascii: bool) -> String {
        let rows = page.position(ascii);
        match page.column_position(ascii) {
            Some(columns) => format!("{rows}, {columns}"),
            None => rows,
        }
    }

    fn render_lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Vec<Segment>> {
        let page = self.page_at(options.max_width);
        let table = self.build(console, &page);
        let mut lines = table_lines(&table, console, options);
        if self.position {
            let text = Self::position_text(&page, console.ascii_only());
            lines.extend(self.note_lines(console, options, text));
        }
        if let Some(note) = &self.footnote {
            lines.extend(self.note_lines(console, options, note.clone()));
        }
        lines
    }
}

impl<S: VirtualRows> Renderable for VirtualTable<S> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        crate::event::flatten(self.render_lines(console, options))
    }

    /// The table's measurement, widened to the position line and footnote:
    /// every line `rich_render` emits.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let page = self.page_at(options.max_width);
        let mut measurement = self.build(console, &page).measure(console, options);
        let notes = self
            .position
            .then(|| Self::position_text(&page, console.ascii_only()))
            .into_iter()
            .chain(self.footnote.clone());
        for note in notes {
            measurement.maximum = measurement.maximum.max(Text::new(note).cell_len());
        }
        measurement
    }
}

impl<S: VirtualRows> crate::a11y::AccessibleText for VirtualTable<S> {
    fn accessible_text(&self, width: usize) -> String {
        let console = Console::builder().width(width.max(1)).build();
        self.to_table(&console).accessible_text(width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(table: &impl Renderable, width: usize) -> String {
        Console::builder()
            .width(width)
            .color_system(None)
            .build()
            .render_export(table)
    }

    fn numbered(count: Option<usize>) -> FnRows<impl Fn(usize) -> Option<Vec<Value>>> {
        FnRows::new(count, |i| {
            Some(vec![Value::from(i), format!("r{i}").into()])
        })
    }

    #[test]
    fn an_offset_past_the_end_shows_the_last_window() {
        let rows: Vec<Vec<Value>> = (0..10).map(|i| vec![Value::from(i)]).collect();
        let table = VirtualTable::new([Column::new("n")], rows)
            .offset(50)
            .height(3);
        let page = table.page();
        assert_eq!(page.range(), 7..10);
        assert_eq!(page.total, Some(10));
        assert!(!page.more);
        assert_eq!(page.position(false), "rows 8–10 of 10");
    }

    #[test]
    fn an_uncounted_source_reports_what_it_knows() {
        let rows = FnRows::new(None, |i| (i < 5).then(|| vec![Value::from(i)]));
        let table = VirtualTable::new([Column::new("n")], rows).height(2);
        assert_eq!(table.page().position(false), "rows 1–2 of 3+");
        let mut table = table;
        table.set_offset(3);
        let page = table.page();
        assert_eq!((page.total, page.more), (Some(5), false));
        assert_eq!(page.position(true), "rows 4-5 of 5");
        table.set_offset(9);
        assert_eq!(table.page().position(false), "no rows from 10");
    }

    #[test]
    fn ragged_rows_are_padded_and_cut() {
        let rows = vec![
            vec![Value::from("a")],
            vec![Value::from("b"), Value::from(2), Value::from("extra")],
            vec![],
        ];
        let table = VirtualTable::new([Column::new("x"), Column::new("y")], rows);
        let page = table.page();
        assert!(page.rows.iter().all(|row| row.len() == 2));
        let out = render(&table, 30);
        assert!(out.contains("│ b │ 2 │"), "{out}");
    }

    #[test]
    fn widths_hold_still_while_scrolling() {
        let mut table = VirtualTable::new([Column::new("n"), Column::new("s")], numbered(None))
            .sample(10)
            .height(2);
        let first = render(&table, 40);
        table.scroll_by(500);
        let later = render(&table, 40);
        let width = |out: &str| out.lines().next().unwrap().chars().count();
        assert_eq!(width(&first), width(&later));
        // `n` was sampled from 0..10: one cell wide, so `500` is cut short.
        assert!(later.contains("│ … │ r… │"), "{later}");
        assert_eq!(table.column_widths(), [1, 2]);
    }

    #[test]
    fn fixed_widths_win_and_the_cap_bounds_samples() {
        let long = vec![vec![Value::from("x".repeat(500)), Value::from("y")]];
        let table = VirtualTable::new([Column::new("a"), Column::new("b")], long.clone())
            .max_column_width(8);
        assert_eq!(table.column_widths(), [8, 1]);
        let table =
            VirtualTable::new([Column::new("a"), Column::new("b")], long).widths([None, Some(5)]);
        assert_eq!(table.column_widths(), [40, 5]);
    }

    #[test]
    fn scrolling_stops_at_both_ends() {
        let mut table = VirtualTable::new([Column::new("n")], numbered(Some(100))).height(10);
        table.scroll_by(-5);
        assert_eq!(table.window_offset(), 0);
        table.scroll_by(1_000);
        assert_eq!(table.window_offset(), 90);
        assert_eq!(table.max_offset(), Some(90));
    }

    #[test]
    fn nulls_can_be_marked_apart_from_empty_text() {
        let rows = vec![vec![Value::Null], vec![Value::from("")]];
        let table = VirtualTable::new([Column::new("v")], rows)
            .null_marker("NULL")
            .show_position(false);
        assert_eq!(
            render(&table, 20),
            "┏━━━━━━┓\n┃ v    ┃\n┡━━━━━━┩\n│ NULL │\n│      │\n└──────┘\n"
        );
    }

    #[test]
    fn row_numbers_and_footnote() {
        let table = VirtualTable::new([Column::new("s")], numbered(Some(1_000)))
            .offset(998)
            .height(2)
            .row_numbers(true)
            .widths([3])
            .footnote("(1,000 rows)");
        let out = render(&table, 40);
        assert!(out.contains("│  999 │ 998 │"), "{out}");
        assert!(
            out.ends_with("rows 999–1,000 of 1,000\n(1,000 rows)\n"),
            "{out}"
        );
    }

    #[test]
    fn chrome_lines_count_everything_but_rows() {
        let table = VirtualTable::new([Column::new("n")], numbered(Some(100))).height(5);
        let console = Console::builder().width(40).build();
        let out = render(&table, 40);
        assert_eq!(out.lines().count(), 5 + table.chrome_lines(&console, 40));
    }

    #[test]
    fn an_empty_source_renders_a_header_and_no_rows() {
        let table = VirtualTable::new([Column::new("n")], Vec::<Vec<Value>>::new());
        let out = render(&table, 20);
        assert!(out.ends_with("no rows\n"), "{out}");
        let table = VirtualTable::new(Vec::<Column>::new(), numbered(Some(3)));
        render(&table, 20);
        // A column window over no columns shows none.
        let table = VirtualTable::new(Vec::<Column>::new(), numbered(Some(3))).fit_columns(true);
        assert_eq!(table.shown_columns(20), Vec::<usize>::new());
        render(&table, 20);
    }

    /// `rows` rows of `columns` cells `r{row}c{column}`, counting the cells
    /// fetched and noting their columns.
    struct Counting {
        rows: usize,
        columns: usize,
        cells: std::sync::atomic::AtomicUsize,
        seen: Mutex<std::collections::BTreeSet<usize>>,
    }

    impl Counting {
        fn new(rows: usize, columns: usize) -> Self {
            Counting {
                rows,
                columns,
                cells: Default::default(),
                seen: Default::default(),
            }
        }

        fn seen(&self) -> Vec<usize> {
            self.seen.lock().unwrap().iter().copied().collect()
        }

        fn cells(&self) -> usize {
            self.cells.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl VirtualRows for Counting {
        fn row_count(&self) -> Option<usize> {
            Some(self.rows)
        }

        fn row(&self, index: usize) -> Option<Vec<Value>> {
            let all: Vec<usize> = (0..self.columns).collect();
            self.rows_columns(index, 1, &all).pop()
        }

        fn rows_columns(&self, start: usize, len: usize, columns: &[usize]) -> Vec<Vec<Value>> {
            let end = start.saturating_add(len).min(self.rows);
            let rows: Vec<Vec<Value>> = (start.min(end)..end)
                .map(|r| {
                    columns
                        .iter()
                        .map(|c| Value::from(format!("r{r}c{c}")))
                        .collect()
                })
                .collect();
            self.cells.fetch_add(
                rows.len() * columns.len(),
                std::sync::atomic::Ordering::Relaxed,
            );
            self.seen.lock().unwrap().extend(columns.iter().copied());
            rows
        }
    }

    fn twelve() -> Vec<Column> {
        (0..12).map(|c| Column::new(format!("c{c}"))).collect()
    }

    #[test]
    fn a_column_window_fetches_only_its_columns() {
        let source = Counting::new(5, 12);
        let table = VirtualTable::new(twelve(), &source)
            .height(2)
            .column_offset(3)
            .visible_columns(2);
        assert_eq!(
            render(&table, 60),
            "\
┏━━━━━━┳━━━━━━┓
┃ c3   ┃ c4   ┃
┡━━━━━━╇━━━━━━┩
│ r0c3 │ r0c4 │
│ r1c3 │ r1c4 │
└──────┴──────┘
rows 1–2 of 5, columns 4–5 of 12
"
        );
        assert_eq!(source.seen(), vec![3, 4]);
        // The width sample (5 rows) and the window (2 rows), two columns.
        assert_eq!(source.cells(), 5 * 2 + 2 * 2);
        let page = table.page();
        assert_eq!(
            (page.columns.clone(), page.frozen, page.column_total),
            (vec![3, 4], 0, 12)
        );
        assert_eq!(page.column_range(), 3..5);
    }

    #[test]
    fn frozen_columns_stay_on_the_left() {
        let source = Counting::new(5, 12);
        let mut table = VirtualTable::new(twelve(), &source)
            .height(1)
            .frozen_columns(1)
            .column_offset(5)
            .visible_columns(2);
        assert_eq!(
            render(&table, 60),
            "\
┏━━━━━━┳━━━━━━┳━━━━━━┓
┃ c0   ┃ c6   ┃ c7   ┃
┡━━━━━━╇━━━━━━╇━━━━━━┩
│ r0c0 │ r0c6 │ r0c7 │
└──────┴──────┴──────┘
rows 1–1 of 5, columns 7–8 of 12
"
        );
        assert_eq!(source.seen(), vec![0, 6, 7]);
        // Scrolling stops at the first column and at the last full window.
        table.scroll_columns_by(-100);
        assert_eq!(table.window_column_offset(), 0);
        table.scroll_columns_by(100);
        assert_eq!(table.max_column_offset(), 9);
        assert_eq!(table.window_column_offset(), 9);
        assert_eq!(table.shown_columns(60), vec![0, 10, 11]);
        // One column: `column`, singular.
        table.set_visible_columns(Some(1));
        table.set_column_offset(50);
        assert_eq!(table.shown_columns(60), vec![0, 11]);
        assert_eq!(
            table.page().column_position(true).as_deref(),
            Some("column 12 of 12")
        );
    }

    #[test]
    fn fit_to_width_shows_what_fits_from_the_offset() {
        let source = Counting::new(5, 12);
        let table = VirtualTable::new(twelve(), &source)
            .height(1)
            .frozen_columns(1)
            .column_offset(2)
            .fit_columns(true);
        // Each column is 4 cells, 7 with its padding and border: 30 cells
        // hold four.
        assert_eq!(table.shown_columns(30), vec![0, 3, 4, 5]);
        assert_eq!(
            render(&table, 34),
            "\
┏━━━━━━┳━━━━━━┳━━━━━━┳━━━━━━┓
┃ c0   ┃ c3   ┃ c4   ┃ c5   ┃
┡━━━━━━╇━━━━━━╇━━━━━━╇━━━━━━┩
│ r0c0 │ r0c3 │ r0c4 │ r0c5 │
└──────┴──────┴──────┴──────┘
rows 1–1 of 5, columns 4–6 of 12
"
        );
        // Sampling looked one column past the window, no further.
        assert_eq!(source.seen(), vec![0, 3, 4, 5, 6]);
        // Narrower than one column: still one, past the frozen ones.
        assert_eq!(table.shown_columns(5), vec![0, 3]);
        // Wide enough for all: no column note.
        let all = VirtualTable::new(twelve(), &source)
            .height(1)
            .fit_columns(true);
        assert_eq!(all.shown_columns(200).len(), 12);
        assert_eq!(all.page_at(200).column_position(false), None);
        // A count caps the fit.
        let capped = VirtualTable::new(twelve(), &source)
            .fit_columns(true)
            .visible_columns(2);
        assert_eq!(capped.shown_columns(200), vec![0, 1]);
    }

    #[test]
    fn a_window_reads_its_columns_aloud() {
        use crate::a11y::AccessibleText;
        let table = VirtualTable::new(twelve(), Counting::new(3, 12))
            .height(1)
            .column_offset(10);
        assert_eq!(
            table.accessible_text(80),
            "Table with 1 row, columns: c10, c11\nRow 1: c10: r0c10; c11: r0c11"
        );
    }

    #[test]
    fn the_guides_wide_table() {
        let cells = FnCells::new(100_000, 2_000, |r, c| Value::from(r * c));
        let columns = (0..2_000).map(|c| Column::new(format!("c{c}")));
        let table = VirtualTable::new(columns, cells)
            .height(2)
            .frozen_columns(1)
            .column_offset(40)
            .fit_columns(true);
        assert_eq!(
            render(&table, 80),
            "\
┏━━━━┳━━━━━━┳━━━━━━┳━━━━━━┳━━━━━━┳━━━━━━┳━━━━━━┳━━━━━━┳━━━━━━┳━━━━━━┳━━━━━━┓
┃ c0 ┃ c41  ┃ c42  ┃ c43  ┃ c44  ┃ c45  ┃ c46  ┃ c47  ┃ c48  ┃ c49  ┃ c50  ┃
┡━━━━╇━━━━━━╇━━━━━━╇━━━━━━╇━━━━━━╇━━━━━━╇━━━━━━╇━━━━━━╇━━━━━━╇━━━━━━╇━━━━━━┩
│ 0  │ 0    │ 0    │ 0    │ 0    │ 0    │ 0    │ 0    │ 0    │ 0    │ 0    │
│ 0  │ 41   │ 42   │ 43   │ 44   │ 45   │ 46   │ 47   │ 48   │ 49   │ 50   │
└────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┴──────┘
rows 1–2 of 100,000, columns 42–51 of 2,000
"
        );
    }

    #[test]
    fn fn_cells_compute_only_what_is_asked() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let grid = FnCells::new(100_000, 2_000, |r, c| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Value::from(r + c)
        });
        let table = VirtualTable::new((0..2_000).map(|c| Column::new(format!("{c}"))), &grid)
            .offset(50_000)
            .height(3)
            .column_offset(1_000)
            .visible_columns(3)
            .sample(10);
        let out = render(&table, 60);
        assert!(
            out.ends_with("rows 50,001–50,003 of 100,000, columns 1,001–1,003 of 2,000\n"),
            "{out}"
        );
        // Ten sample rows and three window rows, three columns each.
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            (10 + 3) * 3
        );
        assert_eq!(grid.row(3).map(|r| r.len()), Some(2_000));
        assert_eq!(grid.rows_columns(99_999, 5, &[0, 5_000]).len(), 1);
        assert_eq!(grid.rows_columns(0, 1, &[5_000]), vec![vec![Value::Null]]);
    }
}
