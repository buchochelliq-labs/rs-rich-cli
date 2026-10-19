//! Ready-made widgets, built on the public [`Widget`] trait as an app's own
//! would be: a [`table`] with a selectable row and sized columns, a
//! [`virtual_list`] of any length, a [`tabs`] strip, a [`tree`], [`split`]
//! panes, a [`calendar`], a [`time_picker`], a [`datetime_picker`] and a
//! [`hex_view`].
//!
//! [`scroll`](crate::scroll) and anchored pop-ups
//! ([`Ctx::popup`](crate::Ctx::popup)) are part of the framework itself,
//! because they change how a subtree is drawn or layered.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rich::{Console, Overflow, Segment, Style};
use rich_interact::{Button, Key, KeyCode, MouseKind};

use crate::layout::{solve, Align, Size, Track};
use crate::list::Selection;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::scrollbar::Bar;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

pub use crate::bars::{
    breadcrumb, cells, key_hints, live, seg, seg_gauge, status_bar, Caps, StatusSegment,
};
pub use crate::calendar::{
    calendar, calendar_options, calendar_with, month, CalendarOptions, Date, DateTime, Time,
    Weekday,
};
pub use crate::datetime::{
    datetime_picker, time_picker, time_picker_with, DateTimeOptions, TimeOptions,
};
pub use crate::hex::hex_view;
pub use crate::list::{
    list_lazy, list_optional, list_rows, HighlightLayer, HighlightSpacing, ListDirection, ListRow,
    RowRects,
};
pub use crate::reorder::Move;
pub use crate::scrollbar::{scrollbar, Scrollbar, ScrollbarSymbols, Show, Side};
pub use crate::split::{hsplit, split, split_with, vsplit};
pub use crate::tab_set::{tab_set, tabs_for, TabSet};
pub use crate::text_area::text_area;
pub use crate::tree::{tree, tree_lazy, tree_with, LazyItem, TreeItem};

/// A table column: its title, how wide it is (cells, a percentage, a
/// flexible share, or [`Size::Auto`]: as wide as its title and the cells
/// in view), the least and the most it may be, and where its cells sit in
/// it.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub title: String,
    pub size: Size,
    /// The fewest cells it takes, and the most.
    pub min: u16,
    pub max: u16,
    /// Where its cells (and its title) sit: [`Align::Start`] (the
    /// default), [`Align::Center`] or [`Align::End`] (numbers on the
    /// right). [`Align::Stretch`] is the start.
    pub align: Align,
    /// How a cell too wide for the column is cut: [`Overflow::Ellipsis`]
    /// (the default, with `…`) or [`Overflow::Crop`] (cut at the edge, as
    /// ratatui's cells are). Others are the ellipsis.
    pub overflow: Overflow,
    /// The fewest and the most it takes as a percentage of the table's
    /// width, as well as [`min`](Self::min) and [`max`](Self::max) cells.
    pub min_percent: Option<u8>,
    pub max_percent: Option<u8>,
    /// How long it stays when the columns do not fit: `None` (the
    /// default) is never dropped; of the others, the lowest goes first,
    /// whole ([`priority`](Self::priority)).
    pub priority: Option<u8>,
}

impl Column {
    pub fn new(title: &str, size: Size) -> Column {
        Column {
            title: title.to_string(),
            size,
            min: 0,
            max: u16::MAX,
            align: Align::Start,
            overflow: Overflow::Ellipsis,
            min_percent: None,
            max_percent: None,
            priority: None,
        }
    }

    /// How its cells are cut when they do not fit: `Column::new("Name",
    /// Size::Percent(30)).overflow(Overflow::Crop)` cuts them at the edge,
    /// with no `…`, as ratatui's table does.
    pub fn overflow(mut self, overflow: Overflow) -> Column {
        self.overflow = overflow;
        self
    }

    /// Where its cells and title sit: `Column::new("Qty", Size::Fixed(7))
    /// .align(Align::End)`.
    pub fn align(mut self, align: Align) -> Column {
        self.align = align;
        self
    }

    /// At least `cells` wide.
    pub fn min(mut self, cells: u16) -> Column {
        self.min = cells;
        self
    }

    /// At most `cells` wide.
    pub fn max(mut self, cells: u16) -> Column {
        self.max = cells;
        self
    }

    /// At least `percent`% of the table's width (where there is room).
    pub fn min_percent(mut self, percent: u8) -> Column {
        self.min_percent = Some(percent.min(100));
        self
    }

    /// At most `percent`% of the table's width: csvlens caps a fitted
    /// column at 30%, `Column::new("Text", Size::Auto).max_percent(30)`.
    /// Scrolled across ([`TableOptions::scroll_x`]), the cap holds too.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table, Column};
    ///
    /// let app = App::new(|| {
    ///     let long = Column::new("Text", Size::Auto).max_percent(50);
    ///     let columns = vec![long, Column::new("N", Size::Auto)];
    ///     table(columns, || vec![vec!["a long cell of text".into(), "1".into()]], signal(0))
    /// });
    /// let mut driver = app.driver(20, 2);
    /// driver.settle();
    /// // Ten cells (half the width), cut with `…`.
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "a long ce… 1");
    /// ```
    pub fn max_percent(mut self, percent: u8) -> Column {
        self.max_percent = Some(percent.min(100));
        self
    }

    /// Drop the column, whole, when the columns do not fit, the lowest
    /// `priority` first (the rightmost of equals first); a column with no
    /// priority is never dropped. What is dropped leaves its room to the
    /// rest, so a [`Size::Flex`] column keeps at least its
    /// [`min`](Self::min), or its title, rather than shrinking to nothing
    /// (bottom's Name column, its R/s and W/s gone at 50 cells). Not with
    /// [`TableOptions::scroll_x`], which scrolls instead.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table, Column};
    ///
    /// let app = App::new(|| {
    ///     let columns = vec![
    ///         Column::new("PID", Size::Fixed(5)),
    ///         Column::new("Name", Size::Flex(1)).min(6).priority(9),
    ///         Column::new("CPU%", Size::Fixed(5)).priority(5),
    ///         Column::new("R/s", Size::Fixed(5)).priority(1),
    ///     ];
    ///     let rows = || vec![vec!["1".into(), "systemd".into(), "0.3".into(), "12K".into()]];
    ///     table(columns, rows, signal(0))
    /// });
    /// let mut driver = app.driver(18, 2);
    /// driver.settle();
    /// // R/s is dropped; Name keeps its six cells.
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "PID   Name   CPU%");
    /// ```
    pub fn priority(mut self, priority: u8) -> Column {
        self.priority = Some(priority);
        self
    }

    /// Its track in a table `room` cells wide.
    fn track(&self, size: Size, room: u16) -> Track {
        let percent = |p: u8| (room as u32 * p as u32 / 100) as u16;
        let mut track = Track::new(size);
        track.min = self.min.max(self.min_percent.map_or(0, percent));
        track.max = self.max.min(self.max_percent.map_or(u16::MAX, percent));
        track.max = track.max.max(track.min);
        track
    }
}

/// Where a table's rows come from.
#[derive(Clone)]
pub(crate) enum Rows {
    /// Every row, from a closure that may read signals.
    All(Rc<dyn Fn() -> Vec<Vec<String>>>),
    /// A count and one row at a time: only the rows in view are asked for.
    Lazy {
        len: Rc<dyn Fn() -> usize>,
        row: Rc<dyn Fn(usize) -> Vec<String>>,
    },
}

impl Rows {
    pub(crate) fn len(&self) -> usize {
        match self {
            Rows::All(rows) => rows().len(),
            Rows::Lazy { len, .. } => len(),
        }
    }

    fn window(&self, from: usize, count: usize) -> Vec<Vec<String>> {
        match self {
            Rows::All(rows) => rows().into_iter().skip(from).take(count).collect(),
            Rows::Lazy { len, row } => (from..(from + count).min(len())).map(|i| row(i)).collect(),
        }
    }
}

pub(crate) struct Table {
    name: &'static str,
    /// The columns the table was made with.
    source: Vec<Column>,
    /// The columns shown, left to right ([`TableOptions::columns_with`],
    /// [`TableOptions::columns_shown`], [`Column::priority`]), and each
    /// one's index among all the columns; whether that is all of them in
    /// order.
    columns: Vec<Column>,
    order: Vec<usize>,
    identity: bool,
    pub(crate) rows: Rows,
    pub(crate) selection: Selection,
    /// Whether the header row is drawn (a list has none).
    header: bool,
    /// The theme style of the selection while it does not have the focus.
    blur: &'static str,
    /// The first row in view, and the page the keys move by: the rows of
    /// the body last drawn.
    first: Cell<usize>,
    body: Cell<usize>,
    /// Where each row in view was last drawn: its index, its first line
    /// and its height, for the cursor and for clicks.
    placed: RefCell<Vec<(usize, u16, u16)>>,
    options: TableOptions,
    /// Widths the user dragged a column to, by column.
    dragged: RefCell<Vec<Option<u16>>>,
    /// Where each column was last drawn: its first column and width.
    spans: RefCell<Vec<(u16, u16)>>,
    /// The column whose right edge is being dragged, and where the drag
    /// started.
    resizing: Cell<Option<(usize, u16)>>,
    /// The scroll across of a table wider than its view (#717).
    across: crate::table_scroll::Across,
    pub(crate) bar: Bar,
    /// Where the bar was drawn, and what it showed: the first row, the
    /// rows and the rows in view.
    bar_at: Cell<Option<(crate::screen::Rect, usize, usize, usize)>>,
    /// The [`offset`](TableOptions::offset) signal's value as last read or
    /// set, and [`offset_x`](TableOptions::offset_x)'s.
    seen: Cell<Option<usize>>,
    seen_x: Cell<Option<usize>>,
    /// Where the rows in view were drawn, on the screen
    /// ([`Node::row_rects`]).
    pub(crate) rects: Option<crate::list::RowRects>,
    /// Marker cells by theme style, before
    /// [`marker`](TableOptions::marker)'s: [`Node::selection_markers`] and
    /// [`Node::marks_by`].
    pub(crate) marks: Option<crate::list::Marks>,
}

/// Which way a table's column is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Ascending,
    Descending,
}

/// A table's footer: its cells, from a closure that may read signals.
type Footer = Box<dyn Fn() -> Vec<String>>;
/// A table's or list's row style: for row `i`, a style (or theme style
/// name) for the whole row, or none.
type RowStyle = Box<dyn Fn(usize) -> Option<String>>;

/// What a [`table_with`] adds to a [`table`]. Every field is off by default.
#[derive(Default)]
pub struct TableOptions {
    /// The column and order rows are sorted by, which a click on a header
    /// (or `s`) sets and the header shows with an arrow. The rows closure
    /// reads it and sorts, so the selection stays an index into the app's
    /// own order; set [`sort_rows`](Self::sort_rows) to have the table sort
    /// plain rows itself.
    pub sort: Option<Signal<Option<(usize, Order)>>>,
    /// Sort the rows by [`sort`](Self::sort) in the table: by number where
    /// both cells are numbers, else by text without case. The selection is
    /// then an index into the sorted rows. Not for a [`virtual_table`],
    /// whose rows are never all in hand.
    pub sort_rows: bool,
    /// A cell cursor: the selected column, moved by ←/→ (or h/l) and
    /// clicks, and drawn in the theme's `selected.cell` style over the
    /// selected row.
    pub column: Option<Signal<usize>>,
    /// Columns resize by dragging the gap after a header.
    pub resizable: bool,
    /// Columns keep their natural widths (a [`Size::Fixed`] column its
    /// cells, the others their title and the cells in view) when they do
    /// not fit, and the table scrolls across (#717): ←/→ (or h/l) without
    /// a cell cursor, Shift+←/→ with one, Shift+Home/End to the ends and
    /// Shift with the wheel. The header is cut where the rows are, `‹` and
    /// `›` in it say that columns are out of view on that side, and the
    /// view follows the cell cursor. A table that fits shares the width by
    /// [`Size`] as before.
    pub scroll_x: bool,
    /// With [`scroll_x`](Self::scroll_x), the leading columns that stay in
    /// place while the rest scroll.
    pub freeze: usize,
    /// A row of cells under the rows that stays in view (totals), in the
    /// theme's `table.footer` style.
    pub footer: Option<Footer>,
    /// Blank lines between the header and the rows: ratatui's header
    /// `bottom_margin`.
    pub header_gap: u16,
    /// Blank lines after each row: ratatui's `Row::bottom_margin`.
    pub row_gap: u16,
    /// Cells between columns; `None` is one.
    pub column_spacing: Option<u16>,
    /// Console markup drawn before the selected row (`"» "`): ratatui's
    /// `highlight_symbol`. The header, the footer and the other rows get
    /// as many blank cells while its column is kept.
    pub symbol: Option<String>,
    /// When the symbol keeps its column.
    pub highlight_spacing: HighlightSpacing,
    /// A style for row `i`'s whole row, gaps included (zebra stripes),
    /// under the cells' own markup and the selection.
    pub row_style: Option<RowStyle>,
    /// Rows as tall as their tallest cell, cells wrapping to their column.
    /// A [`virtual_table`] lays out only the rows near the view (sized
    /// [`Size::Auto`] down, it counts a line a row, or
    /// [`row_height_for`](Self::row_height_for)'s).
    pub multiline: bool,
    /// Every row this many lines tall (cells wrap with
    /// [`multiline`](Self::multiline), and are cut at it).
    pub row_height: Option<u16>,
    /// With a cell cursor ([`cells`](Self::cells)), the cursor's column
    /// down the rows in the theme's `selected.column` style.
    pub column_highlight: bool,
    /// Rows kept in view either side of the selected one, where they fit.
    pub scroll_padding: usize,
    /// No header row: the rows start at the top. For columns that need no
    /// titles, such as names with their sizes beside them.
    pub hide_header: bool,
    /// The first row in view: read it, or set it to scroll. The selected
    /// row is still kept in view.
    pub offset: Option<Signal<usize>>,
    /// The columns, from a closure that may read signals, in place of the
    /// ones the table was made with: titles, sizes and the set change with
    /// no new table, so the scroll across and the cursor stay.
    pub columns_with: Option<Box<dyn Fn() -> Vec<Column>>>,
    /// Which columns show, and in what order, by their index among the
    /// columns: the others are hidden. May read signals.
    pub shown: Option<Box<dyn Fn() -> Vec<usize>>>,
    /// The frozen columns' count, from a closure that may read signals, in
    /// place of [`freeze`](Self::freeze).
    pub freeze_with: Option<Box<dyn Fn() -> usize>>,
    /// Each column's width, by index: `None` keeps its [`Size`]. The
    /// mouse's drags ([`resizable`](Self::resizable)) write here too, so a
    /// key can widen a column the mouse narrowed.
    pub widths: Option<Signal<Vec<Option<u16>>>>,
    /// Whether rows wrap ([`multiline`](Self::multiline)), from a closure
    /// that may read signals.
    pub multiline_when: Option<Box<dyn Fn() -> bool>>,
    /// Whether the cell cursor ([`column`](Self::column)) is on, from a
    /// closure that may read signals: off, the table selects rows.
    pub cells_when: Option<Box<dyn Fn() -> bool>>,
    /// A sorted column's title, from its title and the order: `"Name ▲"`
    /// by default.
    pub sort_indicator: Option<Box<dyn Fn(&str, Order) -> String>>,
    /// The style (or theme style name) of row `i`'s cell in a one-cell
    /// marker column at the left, outside the highlight; `None` is blank.
    pub marker: Option<RowStyle>,
    /// The symbol column's markup for each row, and the column's width:
    /// see [`symbol_for`](Self::symbol_for).
    pub symbols: Option<(u16, Symbols)>,
    /// Where the selection's highlight goes: over the cells' own styles
    /// (the default) or under them.
    pub highlight_layer: HighlightLayer,
    /// The first scrolled column in view, with [`scroll_x`](Self::scroll_x):
    /// read it, or set it to scroll across.
    pub offset_x: Option<Signal<usize>>,
    /// Row `i`'s height in lines, in place of
    /// [`row_height`](Self::row_height).
    pub row_height_for: Option<Box<dyn Fn(usize) -> u16>>,
    /// Where the last draw put the columns and the rows.
    pub layout: Option<TableLayout>,
}

/// A table's symbol for row `i`, given whether it is selected: markup, or
/// `None` for the plain [`symbol`](TableOptions::symbol) on the selected
/// row and a blank on the others.
pub type Symbols = Box<dyn Fn(usize, bool) -> Option<String>>;

/// What a table's last draw laid out: each column's width, the columns
/// and rows in view. Give one to [`TableOptions::layout`] and read it in a
/// handler: a key that widens the selected column by four cells starts from
/// [`width_of`](Self::width_of), a page across from
/// [`columns_in_view`](Self::columns_in_view).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{table_with, Column, TableLayout, TableOptions};
///
/// let layout = TableLayout::new();
/// let shown = layout.clone();
/// let app = App::new(move || {
///     let columns = vec![Column::new("Name", Size::Auto), Column::new("Size", Size::Flex(1))];
///     let rows = || vec![vec!["notes.txt".into(), "12K".into()]];
///     table_with(columns, rows, signal(0), TableOptions::default().layout(&shown))
/// });
/// let mut driver = app.driver(20, 3);
/// driver.settle();
/// assert_eq!(layout.width_of(0), Some(9));
/// assert_eq!(layout.width_of(1), Some(10));
/// assert_eq!(layout.columns_in_view(), [0, 1]);
/// assert_eq!(layout.rows_in_view(), 2);
/// ```
#[derive(Clone, Default)]
pub struct TableLayout(Rc<RefCell<LaidOut>>);

#[derive(Default)]
struct LaidOut {
    /// Each column drawn, by index among the columns, with its width.
    widths: Vec<(usize, u16)>,
    /// The first row in view, and how many rows the body holds.
    first: usize,
    rows: usize,
}

impl TableLayout {
    /// A handle with nothing laid out yet.
    pub fn new() -> TableLayout {
        TableLayout::default()
    }

    /// The width column `column` (its index among the columns) was drawn
    /// at; `None` when it was hidden, dropped or scrolled out of view.
    pub fn width_of(&self, column: usize) -> Option<u16> {
        let laid = self.0.borrow();
        laid.widths
            .iter()
            .find(|(c, _)| *c == column)
            .map(|(_, w)| *w)
    }

    /// The columns in view, left to right, by index among the columns.
    pub fn columns_in_view(&self) -> Vec<usize> {
        let laid = self.0.borrow();
        laid.widths.iter().map(|(c, _)| *c).collect()
    }

    /// The first row in view.
    pub fn first_row(&self) -> usize {
        self.0.borrow().first
    }

    /// How many rows the body showed: a page.
    pub fn rows_in_view(&self) -> usize {
        self.0.borrow().rows
    }
}

impl TableOptions {
    /// Sorting by `sort`, which the app's rows closure reads.
    pub fn sort(mut self, sort: Signal<Option<(usize, Order)>>) -> TableOptions {
        self.sort = Some(sort);
        self
    }

    /// Sorting by `sort`, done by the table on plain rows.
    pub fn sort_rows(mut self, sort: Signal<Option<(usize, Order)>>) -> TableOptions {
        self.sort = Some(sort);
        self.sort_rows = true;
        self
    }

    /// A cell cursor in `column`.
    pub fn cells(mut self, column: Signal<usize>) -> TableOptions {
        self.column = Some(column);
        self
    }

    /// Columns the mouse resizes.
    pub fn resizable(mut self) -> TableOptions {
        self.resizable = true;
        self
    }

    /// Scroll across when the columns do not fit (#717): see
    /// [`scroll_x`](Self::scroll_x).
    pub fn scroll_x(mut self) -> TableOptions {
        self.scroll_x = true;
        self
    }

    /// Keep the first `columns` columns in place while the rest scroll
    /// across; turns [`scroll_x`](Self::scroll_x) on.
    pub fn freeze_columns(mut self, columns: usize) -> TableOptions {
        self.scroll_x = true;
        self.freeze = columns;
        self
    }

    /// A footer row of `cells` (which may read signals) under the rows.
    pub fn footer(mut self, cells: impl Fn() -> Vec<String> + 'static) -> TableOptions {
        self.footer = Some(Box::new(cells));
        self
    }

    /// `lines` blank lines between the header and the rows.
    pub fn header_gap(mut self, lines: u16) -> TableOptions {
        self.header_gap = lines;
        self
    }

    /// `lines` blank lines after each row.
    pub fn row_gap(mut self, lines: u16) -> TableOptions {
        self.row_gap = lines;
        self
    }

    /// `cells` between columns (one by default).
    pub fn column_spacing(mut self, cells: u16) -> TableOptions {
        self.column_spacing = Some(cells);
        self
    }

    /// Console `markup` before the selected row.
    pub fn symbol(mut self, markup: &str) -> TableOptions {
        self.symbol = Some(markup.to_string());
        self
    }

    /// When the symbol keeps its column.
    pub fn highlight_spacing(mut self, spacing: HighlightSpacing) -> TableOptions {
        self.highlight_spacing = spacing;
        self
    }

    /// A style for each row: `.row_style(|i| (i % 2 == 1).then(|| "on
    /// grey11".into()))`.
    pub fn row_style(mut self, style: impl Fn(usize) -> Option<String> + 'static) -> TableOptions {
        self.row_style = Some(Box::new(style));
        self
    }

    /// Rows as tall as their tallest cell.
    pub fn multiline(mut self) -> TableOptions {
        self.multiline = true;
        self
    }

    /// Every row `lines` tall.
    pub fn row_height(mut self, lines: u16) -> TableOptions {
        self.row_height = Some(lines.max(1));
        self
    }

    /// The cell cursor's column highlighted down the rows.
    pub fn column_highlight(mut self) -> TableOptions {
        self.column_highlight = true;
        self
    }

    /// Keep `rows` rows in view either side of the selected one.
    pub fn scroll_padding(mut self, rows: usize) -> TableOptions {
        self.scroll_padding = rows;
        self
    }

    /// Whether the header row is drawn (it is by default): `header(false)`
    /// leaves it out, and the rows start at the top.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let size = Column::new("Size", Size::Fixed(4)).align(Align::End);
    ///     let columns = vec![Column::new("Name", Size::Flex(1)), size];
    ///     let rows = || vec![vec!["a.txt".into(), "1K".into()], vec!["src".into(), "4".into()]];
    ///     table_with(columns, rows, signal(0), TableOptions::default().header(false))
    /// });
    /// let mut driver = app.driver(10, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "a.txt   1K");
    /// assert_eq!(driver.screen().plain()[1], "src      4");
    /// ```
    pub fn header(mut self, shown: bool) -> TableOptions {
        self.hide_header = !shown;
        self
    }

    /// The first row in view, in `first`: read it, or set it to scroll.
    pub fn offset(mut self, first: Signal<usize>) -> TableOptions {
        self.offset = Some(first);
        self
    }

    /// The columns from `columns` (which may read signals), in place of
    /// the ones the table was made with: a filter of the columns, titles
    /// that follow state (`PID` → `Count`), a new set for another report,
    /// in the same table. The horizontal scroll, the cursor and the widths
    /// stay; a column's widths and sort are by its index.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let grouped = signal(false);
    ///     let columns = move || {
    ///         let first = if grouped.get() { "Count" } else { "PID" };
    ///         vec![Column::new(first, Size::Auto), Column::new("Name", Size::Auto)]
    ///     };
    ///     let rows = || vec![vec!["1".into(), "init".into()]];
    ///     let options = TableOptions::default().columns_with(columns);
    ///     table_with(vec![], rows, signal(0), options).on_key("tab", move |_| grouped.set(true))
    /// });
    /// let mut driver = app.driver(16, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "PID Name");
    /// driver.press("tab");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "Count Name");
    /// ```
    pub fn columns_with(mut self, columns: impl Fn() -> Vec<Column> + 'static) -> TableOptions {
        self.columns_with = Some(Box::new(columns));
        self
    }

    /// Show only the columns `shown` names (by index, which may read
    /// signals), in its order: csvlens's column filter, a hidden empty
    /// column, columns moved. The cell cursor and the sort keep their
    /// columns' indices.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let shown = signal(vec![0, 1, 2]);
    ///     let columns = ["a", "b", "c"].map(|t| Column::new(t, Size::Auto)).to_vec();
    ///     let rows = || vec![vec!["1".into(), "2".into(), "3".into()]];
    ///     let options = TableOptions::default().columns_shown(move || shown.get());
    ///     table_with(columns, rows, signal(0), options).on_key("x", move |_| shown.set(vec![2, 0]))
    /// });
    /// let mut driver = app.driver(10, 2);
    /// driver.press("x");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "c a");
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "3 1");
    /// ```
    pub fn columns_shown(mut self, shown: impl Fn() -> Vec<usize> + 'static) -> TableOptions {
        self.shown = Some(Box::new(shown));
        self
    }

    /// Keep the first `columns()` columns (which may read signals) in place
    /// while the rest scroll across: csvlens's `f<n>`. Turns
    /// [`scroll_x`](Self::scroll_x) on.
    pub fn freeze_columns_with(mut self, columns: impl Fn() -> usize + 'static) -> TableOptions {
        self.scroll_x = true;
        self.freeze_with = Some(Box::new(columns));
        self
    }

    /// Each column's width in `widths`, by index (`None` keeps its size),
    /// shared with the mouse's drags: set it from a key to resize a column.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let widths = signal(vec![None, None]);
    ///     let columns = vec![Column::new("Name", Size::Auto), Column::new("Size", Size::Auto)];
    ///     let rows = || vec![vec!["notes.txt".into(), "12K".into()]];
    ///     let options = TableOptions::default().widths(widths);
    ///     table_with(columns, rows, signal(0), options)
    ///         .on_key(">", move |_| widths.update(|w| w[0] = Some(12)))
    /// });
    /// let mut driver = app.driver(20, 2);
    /// driver.press(">");
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "notes.txt    12K");
    /// ```
    pub fn widths(mut self, widths: Signal<Vec<Option<u16>>>) -> TableOptions {
        self.widths = Some(widths);
        self
    }

    /// Rows wrap, each as tall as its tallest cell, while `on()` (which may
    /// read signals) is true: csvlens's `-S`.
    pub fn multiline_when(mut self, on: impl Fn() -> bool + 'static) -> TableOptions {
        self.multiline_when = Some(Box::new(on));
        self
    }

    /// A cell cursor in `column` while `on()` (which may read signals) is
    /// true, rows selected otherwise: csvlens's Tab between its row, column
    /// and cell modes. Off, ←/→ go to the table's ancestors (or scroll
    /// across, with [`scroll_x`](Self::scroll_x)).
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let (cells, at) = (signal(false), signal(0));
    ///     let columns = vec![Column::new("a", Size::Auto), Column::new("b", Size::Auto)];
    ///     let rows = || vec![vec!["1".into(), "2".into()]];
    ///     let options = TableOptions::default().cells_when(move || cells.get(), at);
    ///     column([table_with(columns, rows, signal(0), options).fixed(2), text!("{at}")])
    ///         .on_key("tab", move |_| cells.update(|c| *c = !*c))
    /// });
    /// let mut driver = app.driver(10, 3);
    /// driver.press("right");
    /// assert_eq!(driver.screen().plain()[2].trim_end(), "0");
    /// driver.press("tab right");
    /// assert_eq!(driver.screen().plain()[2].trim_end(), "1");
    /// ```
    pub fn cells_when(
        mut self,
        on: impl Fn() -> bool + 'static,
        column: Signal<usize>,
    ) -> TableOptions {
        self.column = Some(column);
        self.cells_when = Some(Box::new(on));
        self
    }

    /// A sorted column's title: `indicator(title, order)`, which may read
    /// signals, in place of `"Name ▲"`. csvlens's `Name [▴]`, and `[▴N]`
    /// for its natural order.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, Order, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let columns = vec![Column::new("Name", Size::Auto)];
    ///     let rows = || vec![vec!["a".into()]];
    ///     let options = TableOptions::default()
    ///         .sort(signal(Some((0, Order::Descending))))
    ///         .sort_indicator(|title, order| match order {
    ///             Order::Ascending => format!("{title} [▴]"),
    ///             Order::Descending => format!("{title} [▾]"),
    ///         });
    ///     table_with(columns, rows, signal(0), options)
    /// });
    /// let mut driver = app.driver(12, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "Name [▾]");
    /// ```
    pub fn sort_indicator(
        mut self,
        indicator: impl Fn(&str, Order) -> String + 'static,
    ) -> TableOptions {
        self.sort_indicator = Some(Box::new(indicator));
        self
    }

    /// A one-cell marker column at the left of the rows, outside their
    /// style and the highlight: a blank cell in the style (or theme style
    /// name) `style(i)` gives row `i`, or an unstyled one for `None`, as a
    /// list's [`Node::marker`]. The column is kept with no row marked.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let columns = vec![Column::new("Name", Size::Auto)];
    ///     let rows = || vec![vec!["a".into()], vec!["b".into()]];
    ///     let options = TableOptions::default().marker(|i| (i == 1).then(|| "on yellow".into()));
    ///     table_with(columns, rows, signal(0), options)
    /// });
    /// let mut driver = app.driver(6, 3);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), " Name");
    /// let styled = intuituive::testing::styled(&driver);
    /// assert!(styled.lines().nth(2).unwrap().starts_with("[on yellow] [/]b"), "{styled}");
    /// ```
    pub fn marker(mut self, style: impl Fn(usize) -> Option<String> + 'static) -> TableOptions {
        self.marker = Some(Box::new(style));
        self
    }

    /// A symbol column `width` cells wide whose markup for row `i` is
    /// `symbol(i, selected)`: termusic's `►` on the playing track and `🚀`
    /// on the cursor, taskwarrior-tui's `⦿`/`⦾` on a marked or unmarked
    /// selected row. `None` shows the plain [`symbol`](Self::symbol) on the
    /// selected row and a blank on the others. The column is always kept.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let columns = vec![Column::new("Track", Size::Auto)];
    ///     let rows = || vec![vec!["one".into()], vec!["two".into()]];
    ///     let playing = 1;
    ///     let options = TableOptions::default()
    ///         .symbol("> ")
    ///         .symbol_for(2, move |i, _| (i == playing).then(|| "► ".into()));
    ///     table_with(columns, rows, signal(0), options)
    /// });
    /// let mut driver = app.driver(10, 3);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "> one");
    /// assert_eq!(driver.screen().plain()[2].trim_end(), "► two");
    /// ```
    pub fn symbol_for(
        mut self,
        width: u16,
        symbol: impl Fn(usize, bool) -> Option<String> + 'static,
    ) -> TableOptions {
        self.symbols = Some((width, Box::new(symbol)));
        self
    }

    /// Where the selection's highlight goes: [`HighlightLayer::Under`]
    /// keeps what a cell's markup styles (a match in red on yellow) in its
    /// own style on the selected row.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{table_with, Column, HighlightLayer, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let columns = vec![Column::new("City", Size::Auto)];
    ///     let rows = || vec![vec!["[red on yellow]Re[/]ading".into()]];
    ///     let options = TableOptions::default().highlight_layer(HighlightLayer::Under);
    ///     table_with(columns, rows, signal(0), options).theme_styles([("selected", "on grey30")])
    /// });
    /// let mut driver = app.driver(10, 2);
    /// driver.settle();
    /// let styled = intuituive::testing::styled(&driver);
    /// assert!(styled.contains("[red on yellow]Re[/][on grey30]ading"), "{styled}");
    /// ```
    pub fn highlight_layer(mut self, layer: HighlightLayer) -> TableOptions {
        self.highlight_layer = layer;
        self
    }

    /// The first scrolled column in view, in `first`, with
    /// [`scroll_x`](Self::scroll_x): its place among the columns shown
    /// (the frozen ones count). Read it (csvlens's `Col n`), or set it to
    /// scroll across (a window across, the ends); the cell cursor is still
    /// kept in view.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{virtual_table_with, Column, TableOptions};
    ///
    /// let app = App::new(|| {
    ///     let first = signal(0usize);
    ///     let columns = (0..6).map(|i| Column::new(&format!("col{i}"), Size::Auto)).collect();
    ///     let row = |r: usize| (0..6).map(|c| format!("r{r}c{c}")).collect();
    ///     let options = TableOptions::default().scroll_x().offset_x(first);
    ///     let table = virtual_table_with(columns, || 10, row, signal(0), options);
    ///     column([table.fixed(2), text!("{first}")]).on_key("e", move |_| first.set(5))
    /// });
    /// let mut driver = app.driver(20, 3);
    /// driver.press("right");
    /// assert_eq!(driver.screen().plain()[2].trim_end(), "1");
    /// driver.press("e");
    /// // Set past the end: scrolled as far as it goes, and read back.
    /// assert_eq!(driver.screen().plain()[0], "‹col2 col3 col4 col5");
    /// assert_eq!(driver.screen().plain()[2].trim_end(), "2");
    /// ```
    pub fn offset_x(mut self, first: Signal<usize>) -> TableOptions {
        self.offset_x = Some(first);
        self
    }

    /// Row `i` is `height(i)` lines tall (gaps aside), in place of one
    /// [`row_height`](Self::row_height) for all: a virtual table asks only
    /// for the rows in view. Cells wrap to it with
    /// [`multiline`](Self::multiline), else show their first line.
    pub fn row_height_for(mut self, height: impl Fn(usize) -> u16 + 'static) -> TableOptions {
        self.row_height_for = Some(Box::new(height));
        self
    }

    /// Record where each draw puts the columns and rows in `layout`.
    pub fn layout(mut self, layout: &TableLayout) -> TableOptions {
        self.layout = Some(layout.clone());
        self
    }

    /// Whether rows wrap now.
    fn wraps(&self) -> bool {
        self.multiline || self.multiline_when.as_ref().is_some_and(|on| on())
    }

    /// The cell cursor's signal, while the cursor is on.
    fn cell_cursor(&self) -> Option<Signal<usize>> {
        self.column
            .filter(|_| self.cells_when.as_ref().is_none_or(|on| on()))
    }

    /// The frozen columns' count now.
    fn frozen(&self) -> usize {
        self.freeze_with.as_ref().map_or(self.freeze, |f| f())
    }

    /// Cells between columns.
    fn spacing(&self) -> u16 {
        self.column_spacing.unwrap_or(1)
    }

    /// Whether every row is one line: the old arithmetic, with no heights
    /// to find.
    fn single_line(&self) -> bool {
        !self.wraps()
            && self.row_height.unwrap_or(1) == 1
            && self.row_height_for.is_none()
            && self.row_gap == 0
    }
}

/// A cell as ratatui's table draws one, outside a table: console markup
/// (which may read signals) on one line, cut at its edge rather than
/// wrapped, with no `…`. A [`row`](crate::row) of them, sized as the
/// table's columns are and [`gap`](Node::gap) cells apart (ratatui's
/// `column_spacing`, one by default there), is a one-row table whose cells
/// each draw again only when what they read changes: scope-tui's header.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::cell;
///
/// let app = App::new(|| {
///     row([
///         cell(|| "[bold]oscilloscope::scope-tui".into()).percent(50),
///         cell(|| "60fps".into()).percent(50),
///     ])
///     .gap(1)
/// });
/// let mut driver = app.driver(21, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "oscillosco 60fps");
/// ```
pub fn cell(markup: impl Fn() -> String + 'static) -> Node {
    crate::node::text(markup).no_wrap().overflow(Overflow::Crop)
}

/// A table of markup cells under a header row that stays in view, with one
/// row selected: ↑/↓ (or k/j), PgUp/PgDn and Home/End (g/G) move it, a
/// click selects a row, and the wheel moves it. The selection is
/// highlighted in the theme's `selected` style while the table has the
/// focus, and `table.selected` otherwise. Columns share the width by their
/// [`Size`]; `rows` may read signals. [`table_with`] takes
/// [`TableOptions`]: sorting, a cell cursor, a footer, multi-line rows, a
/// highlight symbol, row styles and more.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{table, Column};
///
/// let app = App::new(|| {
///     let selected = signal(0);
///     table(
///         vec![Column::new("Name", Size::Auto), Column::new("Size", Size::Flex(1))],
///         || vec![vec!["a.txt".into(), "1K".into()], vec!["b.txt".into(), "2K".into()]],
///         selected,
///     )
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["down", "q"], 16, 3).unwrap();
/// assert_eq!(screen[0].trim_end(), "Name  Size");
/// assert_eq!(screen[2].trim_end(), "b.txt 2K");
/// ```
pub fn table(
    columns: Vec<Column>,
    rows: impl Fn() -> Vec<Vec<String>> + 'static,
    selected: Signal<usize>,
) -> Node {
    table_from(columns, Rows::All(Rc::new(rows)), selected)
}

/// A [`table`] that asks only for the rows in view: `len` is how many
/// there are and `row` makes one, so a table of a million rows costs a
/// screenful.
pub fn virtual_table(
    columns: Vec<Column>,
    len: impl Fn() -> usize + 'static,
    row: impl Fn(usize) -> Vec<String> + 'static,
    selected: Signal<usize>,
) -> Node {
    let rows = Rows::Lazy {
        len: Rc::new(len),
        row: Rc::new(row),
    };
    table_from(columns, rows, selected)
}

/// A [`virtual_table`] with a cell cursor, resizable columns or a scroll
/// across, as `options` asks. Its rows are never all in hand, so it does
/// not sort them itself ([`TableOptions::sort_rows`]); `row` may read a
/// [`sort`](TableOptions::sort) signal. Its rows are as tall as
/// [`TableOptions::row_height`] or [`TableOptions::row_height_for`] says,
/// or (with [`TableOptions::multiline`]) as their cells, of the rows near
/// the view only.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{virtual_table_with, Column, TableOptions};
///
/// let app = App::new(|| {
///     let columns = (0..6).map(|i| Column::new(&format!("col{i}"), Size::Auto)).collect();
///     let row = |r: usize| (0..6).map(|c| format!("r{r}c{c}")).collect();
///     let options = TableOptions::default().freeze_columns(1);
///     virtual_table_with(columns, || 1000, row, signal(0), options).on_key("q", |cx| cx.quit())
/// });
/// // A column across, the first one frozen; `‹` and `›` say more are
/// // out of view.
/// let screen = app.render_with(&["right", "q"], 20, 2).unwrap();
/// assert_eq!(screen[0], "col0 ‹col2 col3 col›");
/// assert_eq!(screen[1], "r0c0  r0c2 r0c3 r0c4");
/// ```
pub fn virtual_table_with(
    columns: Vec<Column>,
    len: impl Fn() -> usize + 'static,
    row: impl Fn(usize) -> Vec<String> + 'static,
    selected: Signal<usize>,
    options: TableOptions,
) -> Node {
    let rows = Rows::Lazy {
        len: Rc::new(len),
        row: Rc::new(row),
    };
    table_options(columns, rows, Selection::One(selected), options)
}

/// A list of markup rows, one selected, that asks only for the rows in
/// view: `len` is how many there are and `row` makes one, so a list of a
/// million rows costs a screenful. The keys, clicks and the wheel move the
/// selection as in a [`table`]. The selection is highlighted in the theme's
/// `selected` style while the list has the focus, and `list.selected`
/// otherwise.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::virtual_list;
///
/// let app = App::new(|| {
///     let selected = signal(0usize);
///     virtual_list(|| 1_000_000, |i| format!("row {i}"), selected).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["end", "q"], 16, 3).unwrap();
/// assert_eq!(screen[0].trim_end(), "row 999997");
/// assert_eq!(screen[2].trim_end(), "row 999999");
/// ```
pub fn virtual_list(
    len: impl Fn() -> usize + 'static,
    row: impl Fn(usize) -> String + 'static,
    selected: Signal<usize>,
) -> Node {
    let rows = Rows::Lazy {
        len: Rc::new(len),
        row: Rc::new(move |i| vec![row(i)]),
    };
    widget(Table {
        name: "virtual_list",
        source: vec![Column::new("", Size::Flex(1))],
        columns: vec![Column::new("", Size::Flex(1))],
        order: vec![0],
        identity: true,
        rows,
        selection: Selection::One(selected),
        header: false,
        blur: "list.selected",
        first: Cell::new(0),
        body: Cell::new(1),
        placed: RefCell::new(Vec::new()),
        options: TableOptions::default(),
        dragged: RefCell::new(vec![None]),
        spans: RefCell::new(Vec::new()),
        resizing: Cell::new(None),
        across: Default::default(),
        bar: Bar::default(),
        bar_at: Cell::new(None),
        seen: Cell::new(None),
        seen_x: Cell::new(None),
        rects: None,
        marks: None,
    })
}

fn table_from(columns: Vec<Column>, rows: Rows, selected: Signal<usize>) -> Node {
    table_options(
        columns,
        rows,
        Selection::One(selected),
        TableOptions::default(),
    )
}

/// A [`table`] with sorting, a cell cursor or resizable columns, as
/// `options` asks.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{table_with, Column, Order, TableOptions};
///
/// let app = App::new(|| {
///     let sort = signal(None);
///     let rows = || {
///         vec![
///             vec!["b.txt".into(), "20".into()],
///             vec!["a.txt".into(), "100".into()],
///             vec!["c.txt".into(), "3".into()],
///         ]
///     };
///     let columns = vec![Column::new("Name", Size::Auto), Column::new("Size", Size::Auto)];
///     table_with(columns, rows, signal(0), TableOptions::default().sort_rows(sort))
///         .on_key("q", |cx| cx.quit())
/// });
/// // `s` (or a click on a title) sorts by the first column.
/// let screen = app.render_with(&["s", "q"], 16, 4).unwrap();
/// assert_eq!(screen[0].trim_end(), "Name ▲ Size");
/// assert_eq!(screen[1].trim_end(), "a.txt  100");
/// assert_eq!(screen[3].trim_end(), "c.txt  3");
/// ```
pub fn table_with(
    columns: Vec<Column>,
    rows: impl Fn() -> Vec<Vec<String>> + 'static,
    selected: Signal<usize>,
    options: TableOptions,
) -> Node {
    table_options(
        columns,
        Rows::All(Rc::new(rows)),
        Selection::One(selected),
        options,
    )
}

/// A [`table_with`] whose selection may be empty, as ratatui's
/// `TableState::select(None)`: `None` shows no highlight, and ↓ (or Home)
/// selects the first row, ↑ (or End) the last.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{table_optional, Column, TableOptions};
///
/// let app = App::new(|| {
///     let columns = vec![Column::new("Name", Size::Auto)];
///     let rows = || vec![vec!["a.txt".into()], vec!["b.txt".into()]];
///     table_optional(columns, rows, signal(None), TableOptions::default().symbol("> "))
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 12, 3).unwrap();
/// assert_eq!(screen[1].trim_end(), "a.txt");
/// ```
pub fn table_optional(
    columns: Vec<Column>,
    rows: impl Fn() -> Vec<Vec<String>> + 'static,
    selected: Signal<Option<usize>>,
    options: TableOptions,
) -> Node {
    table_options(
        columns,
        Rows::All(Rc::new(rows)),
        Selection::Maybe(selected),
        options,
    )
}

fn table_options(
    columns: Vec<Column>,
    rows: Rows,
    selection: Selection,
    options: TableOptions,
) -> Node {
    let count = columns.len();
    widget(Table {
        name: "table",
        source: columns.clone(),
        order: (0..count).collect(),
        identity: true,
        columns,
        rows,
        selection,
        header: !options.hide_header,
        blur: "table.selected",
        first: Cell::new(0),
        body: Cell::new(1),
        placed: RefCell::new(Vec::new()),
        options,
        dragged: RefCell::new(vec![None; count]),
        spans: RefCell::new(Vec::new()),
        resizing: Cell::new(None),
        across: Default::default(),
        bar: Bar::default(),
        bar_at: Cell::new(None),
        seen: Cell::new(None),
        seen_x: Cell::new(None),
        rects: None,
        marks: None,
    })
}

/// How two cells compare when a table sorts its rows: as numbers when both
/// are, else as text without case or markup.
fn compare_cells(a: &str, b: &str) -> std::cmp::Ordering {
    let plain = |markup: &str| {
        rich::Text::from_markup(markup)
            .map(|text| text.plain().to_string())
            .unwrap_or_else(|_| markup.to_string())
    };
    let (a, b) = (plain(a), plain(b));
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// `markup` rendered on one line `width` cells wide, padded or cut with an
/// ellipsis.
pub(crate) fn cell_line(console: &Console, markup: &str, width: u16) -> Vec<Segment> {
    cut_line(console, markup, width, Overflow::Ellipsis)
}

/// `markup` rendered on one line `width` cells wide, padded, or cut by
/// `overflow`: at the edge for [`Overflow::Crop`], else with an ellipsis.
fn cut_line(console: &Console, markup: &str, width: u16, overflow: Overflow) -> Vec<Segment> {
    if width == 0 {
        return Vec::new();
    }
    let text =
        rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
    let mut options = console.options().update_width(width as usize);
    options.no_wrap = Some(true);
    options.overflow = Some(cut_by(overflow));
    let mut line = console
        .render_lines(&text, &options, true)
        .into_iter()
        .next()
        .unwrap_or_default();
    let used: usize = line.iter().map(Segment::cell_length).sum();
    if used < width as usize {
        line.push(Segment::new(" ".repeat(width as usize - used), None));
    }
    line
}

/// What a cell's overflow cuts with: the edge for [`Overflow::Crop`], else
/// an ellipsis.
fn cut_by(overflow: Overflow) -> Overflow {
    match overflow {
        Overflow::Crop => Overflow::Crop,
        _ => Overflow::Ellipsis,
    }
}

/// `line` padded to `width` cells, sitting at the start, centre or end.
fn aligned(line: Vec<Segment>, width: u16, align: Align) -> Vec<Segment> {
    let used: usize = line.iter().map(Segment::cell_length).sum();
    let room = (width as usize).saturating_sub(used);
    let pad = |n: usize| Segment::new(" ".repeat(n), None);
    let (before, after) = match align {
        Align::Start | Align::Stretch => (0, room),
        Align::Center => (room / 2, room - room / 2),
        Align::End => (room, 0),
    };
    let mut out = Vec::with_capacity(line.len() + 2);
    if before > 0 {
        out.push(pad(before));
    }
    out.extend(line);
    if after > 0 {
        out.push(pad(after));
    }
    out
}

/// A cell's lines in a column `width` wide, each padded to it: one line
/// cut with an ellipsis, or (with `wrap`) every line it wraps to.
fn cell_lines(
    console: &Console,
    markup: &str,
    width: u16,
    align: Align,
    overflow: Overflow,
    wrap: bool,
) -> Vec<Vec<Segment>> {
    if !wrap && matches!(align, Align::Start | Align::Stretch) {
        return vec![cut_line(console, markup, width, overflow)];
    }
    if width == 0 {
        return vec![Vec::new()];
    }
    let text =
        rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
    let mut options = console.options().update_width(width as usize);
    if !wrap {
        options.no_wrap = Some(true);
        options.overflow = Some(cut_by(overflow));
    }
    let mut lines = console.render_lines(&text, &options, false);
    if !wrap {
        lines.truncate(1);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
        .into_iter()
        .map(|line| aligned(line, width, align))
        .collect()
}

/// The cells a cell of markup takes.
pub(crate) fn markup_width(markup: &str) -> u16 {
    let text =
        rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
    text.cell_len().min(u16::MAX as usize) as u16
}

/// `style` laid over every segment of `line`.
pub(crate) fn over(line: Vec<Segment>, style: &Style) -> Vec<Segment> {
    line.into_iter()
        .map(|segment| {
            let combined = match &segment.style {
                Some(own) => own.combine(style),
                None => style.clone(),
            };
            Segment::new(segment.text, Some(combined))
        })
        .collect()
}

/// `line` with `style` under its own styles, filled to `width` cells in it:
/// a row's style.
fn under(line: Vec<Segment>, style: &Style, width: u16) -> Vec<Segment> {
    let used: usize = line.iter().map(Segment::cell_length).sum();
    let mut out: Vec<Segment> = line
        .into_iter()
        .map(|segment| {
            let combined = match &segment.style {
                Some(own) => style.combine(own),
                None => style.clone(),
            };
            Segment::new(segment.text, Some(combined))
        })
        .collect();
    if used < width as usize {
        out.push(Segment::new(
            " ".repeat(width as usize - used),
            Some(style.clone()),
        ));
    }
    out
}

/// The rows a draw looks at: all of them once, or one at a time, each
/// with its cells in the order the columns are shown.
enum Fetch<'a> {
    All(Vec<Vec<String>>),
    Lazy {
        row: &'a dyn Fn(usize) -> Vec<String>,
        order: &'a [usize],
        identity: bool,
        cache: HashMap<usize, Vec<String>>,
    },
}

impl Fetch<'_> {
    fn row(&mut self, i: usize) -> Vec<String> {
        match self {
            Fetch::All(rows) => rows.get(i).cloned().unwrap_or_default(),
            Fetch::Lazy {
                row,
                order,
                identity,
                cache,
            } => cache
                .entry(i)
                .or_insert_with(|| pick(order, *identity, row(i)))
                .clone(),
        }
    }
}

/// A row's cells for the columns shown: `order` gives each one's index
/// among all the columns (`identity` when that is all of them in order).
fn pick(order: &[usize], identity: bool, row: Vec<String>) -> Vec<String> {
    if identity {
        return row;
    }
    order
        .iter()
        .map(|&c| row.get(c).cloned().unwrap_or_default())
        .collect()
}

/// A track's length clamped to its least and most.
fn clamped(track: &Track, cells: u32) -> u32 {
    cells.clamp(track.min as u32, track.max.max(track.min) as u32)
}

impl Table {
    pub(crate) fn options_mut(&mut self) -> &mut TableOptions {
        &mut self.options
    }

    /// The columns to show now: [`TableOptions::columns_with`]'s or the
    /// table's own, as [`TableOptions::columns_shown`] picks them.
    fn refresh_columns(&mut self) {
        if self.options.columns_with.is_none() && self.options.shown.is_none() {
            if self.columns.len() != self.source.len() || !self.identity {
                self.columns = self.source.clone();
                self.order = (0..self.source.len()).collect();
                self.identity = true;
            }
            return;
        }
        let all = match &self.options.columns_with {
            Some(columns) => columns(),
            None => self.source.clone(),
        };
        let order: Vec<usize> = match &self.options.shown {
            Some(shown) => shown().into_iter().filter(|&c| c < all.len()).collect(),
            None => (0..all.len()).collect(),
        };
        self.identity = order.len() == all.len() && order.iter().enumerate().all(|(i, &c)| i == c);
        self.columns = order.iter().map(|&c| all[c].clone()).collect();
        self.order = order;
    }

    /// Where column `column` (its index among all the columns) is shown,
    /// if it is.
    fn place_of(&self, column: usize) -> Option<usize> {
        self.order.iter().position(|&c| c == column)
    }

    /// The index among all the columns of the column shown at `at`.
    fn original(&self, at: usize) -> usize {
        self.order.get(at).copied().unwrap_or(at)
    }

    /// Where column `i`'s cells sit, and how they are cut.
    fn cell_rules(&self, i: usize) -> (Align, Overflow) {
        self.columns
            .get(i)
            .map_or((Align::Start, Overflow::Ellipsis), |c| {
                (c.align, c.overflow)
            })
    }

    fn step(&self, by: isize) {
        self.selection.step(self.rows.len(), by);
    }

    /// Rows `from..from + count` in the order shown: sorted here when the
    /// table sorts its own rows.
    fn shown(&self, from: usize, count: usize) -> Vec<Vec<String>> {
        let order = self.options.sort.filter(|_| self.options.sort_rows);
        let rows = match (&self.rows, order.and_then(|sort| sort.get())) {
            (Rows::All(rows), Some((column, order))) => {
                let mut all = rows();
                all.sort_by(|a, b| {
                    let (a, b) = (a.get(column), b.get(column));
                    let by = compare_cells(a.map_or("", |s| s), b.map_or("", |s| s));
                    match order {
                        Order::Ascending => by,
                        Order::Descending => by.reverse(),
                    }
                });
                all.into_iter().skip(from).take(count).collect()
            }
            _ => self.rows.window(from, count),
        };
        rows.into_iter()
            .map(|row| pick(&self.order, self.identity, row))
            .collect()
    }

    /// Sort by `column` (its index among all the columns): ascending
    /// first, then the other way on the same column.
    fn sort_by(&self, column: usize) {
        if let Some(sort) = self.options.sort {
            sort.update(|sort| {
                *sort = match *sort {
                    Some((c, Order::Ascending)) if c == column => Some((c, Order::Descending)),
                    _ => Some((column, Order::Ascending)),
                }
            });
        }
    }

    /// Move the cell cursor `by` columns shown.
    fn move_column(&self, by: isize) {
        if let Some(column) = self.options.cell_cursor() {
            let last = self.columns.len().saturating_sub(1) as isize;
            let at = self.place_of(column.get_untracked()).unwrap_or(0) as isize;
            let to = (at + by).clamp(0, last.max(0)) as usize;
            column.set(self.original(to));
        }
    }

    /// How a key scrolls a table across (#717), if it does: ←/→ and h/l
    /// without a cell cursor (which they move), Shift+←/→ either way, and
    /// Shift+Home/End to the ends.
    fn across_by(&self, key: &Key) -> Option<isize> {
        let modifiers = key.modifiers;
        if !self.options.scroll_x || modifiers.ctrl || modifiers.alt {
            return None;
        }
        let cells = self.options.cell_cursor().is_some();
        let free = modifiers.shift || !cells;
        match key.code {
            KeyCode::Left if free => Some(-1),
            KeyCode::Right if free => Some(1),
            KeyCode::Char('h') if !cells => Some(-1),
            KeyCode::Char('l') if !cells => Some(1),
            KeyCode::Home if modifiers.shift => Some(isize::MIN),
            KeyCode::End if modifiers.shift => Some(isize::MAX),
            _ => None,
        }
    }

    /// The column drawn at `x`, if any: its place among the columns shown.
    fn column_at(&self, x: u16) -> Option<usize> {
        self.spans
            .borrow()
            .iter()
            .position(|(start, width)| (*start..start + width).contains(&x))
    }

    /// The column whose right edge (the gap after it) is at `x`. Scrolled
    /// across, a column out of view is drawn nowhere and has none; a column
    /// squeezed to nothing in a table that fits still has one.
    fn edge_at(&self, x: u16) -> Option<usize> {
        let spans = self.spans.borrow();
        let scrolled = self.across.wide.get();
        (0..spans.len().saturating_sub(1))
            .find(|&i| (spans[i].1 > 0 || !scrolled) && spans[i].0 + spans[i].1 == x)
    }

    /// The row drawn on line `y` of the widget, if any.
    fn row_at(&self, y: u16) -> Option<usize> {
        self.placed
            .borrow()
            .iter()
            .find(|(_, top, height)| (*top..top + height).contains(&y))
            .map(|(i, _, _)| *i)
    }

    /// The lines above the rows (the header and its gap) and below them
    /// (the footer).
    fn chrome(&self) -> (u16, u16) {
        let top = if self.header {
            1 + self.options.header_gap
        } else {
            0
        };
        (top, u16::from(self.options.footer.is_some()))
    }

    /// The cells before a row's first column: the marker column, the text
    /// mode's marker and the highlight symbol's column.
    fn indents(&self, selected: bool) -> (u16, u16, u16) {
        let options = &self.options;
        let gutter = u16::from(options.marker.is_some() || self.marks.is_some());
        let marker = if crate::a11y::text_mode() { 2 } else { 0 };
        let symbol = match (&options.symbols, &options.symbol) {
            (Some((width, _)), _) => *width,
            (None, Some(symbol)) if options.highlight_spacing.keeps(selected) => {
                markup_width(symbol)
            }
            _ => 0,
        };
        (gutter, marker, symbol)
    }

    /// Each column's track in a table `room` cells wide: its size (or the
    /// width it was set or dragged to), its least and most, and for
    /// [`Size::Auto`] the widest of its title and `rows`.
    fn tracks(&self, rows: &[Vec<String>], titles: &[String], room: u16) -> Vec<Track> {
        let set = self.options.widths.map(|widths| widths.get());
        let dragged = self.dragged.borrow();
        self.columns
            .iter()
            .enumerate()
            .map(|(i, column)| {
                let index = self.original(i);
                let width = match &set {
                    Some(set) => set.get(index).copied().flatten(),
                    None => dragged.get(index).copied().flatten(),
                };
                let size = match width {
                    Some(width) => Size::Fixed(width),
                    None => column.size,
                };
                let mut track = column.track(size, room);
                if size == Size::Auto {
                    track.content = rows
                        .iter()
                        .filter_map(|row| row.get(i))
                        .map(|cell| markup_width(cell))
                        .chain([markup_width(&titles[i])])
                        .max()
                        .unwrap_or(0);
                }
                track
            })
            .collect()
    }

    /// Which of the columns shown [`Column::priority`] drops so the rest
    /// fit in `room` cells at their least: the lowest priority first, the
    /// rightmost of equals first.
    fn dropped(&self, tracks: &[Track], titles: &[String], room: u16, spacing: u16) -> Vec<bool> {
        let n = tracks.len();
        let mut gone = vec![false; n];
        if self.columns.iter().all(|c| c.priority.is_none()) {
            return gone;
        }
        let room = room as u32;
        let least = |i: usize| -> u32 {
            let track = &tracks[i];
            match track.size {
                Size::Fixed(cells) => clamped(track, cells as u32),
                Size::Percent(p) => clamped(track, room * p.min(100) as u32 / 100),
                Size::Ratio(a, b) => clamped(track, room * a / b.max(1)),
                Size::Auto => clamped(track, track.content as u32),
                // A flexible column with a priority keeps its title.
                Size::Flex(_) if self.columns[i].priority.is_some() => {
                    clamped(track, markup_width(&titles[i]) as u32).max(track.min as u32)
                }
                Size::Flex(_) => track.min as u32,
            }
        };
        loop {
            let shown: Vec<usize> = (0..n).filter(|&i| !gone[i]).collect();
            let gaps = spacing as u32 * shown.len().saturating_sub(1) as u32;
            let need: u32 = shown.iter().map(|&i| least(i)).sum::<u32>() + gaps;
            if need <= room {
                break;
            }
            let victim = shown
                .iter()
                .copied()
                .filter(|&i| self.columns[i].priority.is_some())
                .min_by_key(|&i| (self.columns[i].priority, std::cmp::Reverse(i)));
            match victim {
                Some(i) => gone[i] = true,
                None => break,
            }
        }
        gone
    }

    /// The titles, a sorted column's with an arrow (or its
    /// [`sort_indicator`](TableOptions::sort_indicator)).
    fn titles(&self, sort: Option<(usize, Order)>) -> Vec<String> {
        self.columns
            .iter()
            .enumerate()
            .map(|(i, column)| match sort {
                Some((c, order)) if c == self.original(i) => match &self.options.sort_indicator {
                    Some(indicator) => indicator(&column.title, order),
                    None if order == Order::Ascending => format!("{} ▲", column.title),
                    None => format!("{} ▼", column.title),
                },
                _ => column.title.clone(),
            })
            .collect()
    }

    /// How many lines row `i`, of `cells`, takes at `widths`, gap included.
    fn height_of(&self, console: &Console, i: usize, cells: &[String], widths: &[u16]) -> usize {
        let options = &self.options;
        let lines = match (&options.row_height_for, options.row_height) {
            (Some(height), _) => height(i).max(1) as usize,
            (None, Some(height)) => height as usize,
            (None, None) if options.wraps() => widths
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let markup = cells.get(i).map_or("", String::as_str);
                    let (align, overflow) = self.cell_rules(i);
                    cell_lines(console, markup, *w, align, overflow, true).len()
                })
                .max()
                .unwrap_or(1)
                .max(1),
            (None, None) => 1,
        };
        lines + options.row_gap as usize
    }

    /// A row's lines: `cells` in their columns at `widths`, `spacing` apart,
    /// `height` lines tall, after `indent` (the marker and the symbol's
    /// column, drawn by the caller).
    fn row_lines(
        &self,
        console: &Console,
        cells: &[String],
        widths: &[u16],
        height: usize,
        spacing: u16,
    ) -> Vec<Vec<Segment>> {
        let wrap = self.options.wraps();
        let columns: Vec<Vec<Vec<Segment>>> = widths
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let markup = cells.get(i).map_or("", String::as_str);
                let (align, overflow) = self.cell_rules(i);
                cell_lines(console, markup, *w, align, overflow, wrap)
            })
            .collect();
        (0..height.max(1))
            .map(|line| {
                let mut out = Vec::new();
                for (i, (cell, w)) in columns.iter().zip(widths).enumerate() {
                    if i > 0 && spacing > 0 {
                        out.push(Segment::new(" ".repeat(spacing as usize), None));
                    }
                    match cell.get(line) {
                        Some(segments) => out.extend(segments.iter().cloned()),
                        None if *w > 0 => out.push(Segment::new(" ".repeat(*w as usize), None)),
                        None => {}
                    }
                }
                out
            })
            .collect()
    }

    /// The marker column's style for row `i`, if it is marked.
    fn mark_of(&self, console: &Console, i: usize) -> Option<Style> {
        let options = &self.options;
        let marked = self
            .marks
            .as_ref()
            .and_then(|marks| marks(i))
            .map(|(name, fallback)| crate::widget::theme_style(console, name, fallback));
        marked.or_else(|| {
            options
                .marker
                .as_ref()
                .and_then(|marker| marker(i))
                .map(|name| crate::widget::theme_style(console, &name, &name))
        })
    }
}

impl Widget for Table {
    fn name(&self) -> &'static str {
        self.name
    }

    fn role(&self) -> crate::a11y::Role {
        if self.header || self.name == "table" {
            crate::a11y::Role::Table
        } else {
            crate::a11y::Role::List
        }
    }

    fn cursor(&self) -> Option<crate::screen::Rect> {
        let selected = self.selection.get_untracked()?;
        let (_, top, height) = self
            .placed
            .borrow()
            .iter()
            .copied()
            .find(|(i, _, _)| *i == selected)?;
        // With a cell cursor, the cell; else the row.
        let cell = self.options.cell_cursor().and_then(|column| {
            let spans = self.spans.borrow();
            let at = self.place_of(column.get_untracked())?;
            spans.get(at).copied()
        });
        let height = height.saturating_sub(self.options.row_gap).max(1);
        Some(match cell {
            Some((x, w)) => crate::screen::Rect::new(x, top, w, height),
            None => {
                let (gutter, marker, symbol) = self.indents(true);
                let x = gutter + if symbol > 0 { marker + symbol } else { 0 };
                crate::screen::Rect::new(x, top, u16::MAX, height)
            }
        })
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        match self.selection.get_untracked() {
            Some(selected) => crate::a11y::AccessState::item(selected, self.rows.len()),
            None => crate::a11y::AccessState::default(),
        }
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => {
                self.refresh_columns();
                let (top, footer) = self.chrome();
                let chrome = (top + footer) as usize;
                let len = self.rows.len();
                let options = &self.options;
                let gap = options.row_gap as usize;
                let lazy = matches!(self.rows, Rows::Lazy { .. });
                let lines = if options.single_line() {
                    len
                } else if let (Some(height), false, None) =
                    (options.row_height, options.wraps(), &options.row_height_for)
                {
                    len * (height as usize + gap)
                } else if lazy {
                    // A virtual table reads only the rows in view: its
                    // rows' own heights where it is told them, else a
                    // line each.
                    match &options.row_height_for {
                        Some(height) if len <= 4096 => {
                            (0..len).map(|i| height(i).max(1) as usize + gap).sum()
                        }
                        _ => len * (options.row_height.unwrap_or(1) as usize + gap),
                    }
                } else {
                    // Every row, laid out at this width.
                    let rows = self.shown(0, len);
                    let titles = self.titles(None);
                    let (gutter, marker, symbol) =
                        self.indents(self.selection.get_untracked().is_some());
                    let room = width.saturating_sub(gutter + marker + symbol);
                    let spacing = self.options.spacing();
                    let tracks = self.tracks(&rows, &titles, room);
                    let widths = solve(room, spacing, &tracks);
                    rows.iter()
                        .enumerate()
                        .map(|(i, row)| self.height_of(cx.console(), i, row, &widths))
                        .sum()
                };
                (lines + chrome).min(u16::MAX as usize) as u16
            }
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        self.refresh_columns();
        let (width, height) = (canvas.width(), canvas.height());
        let (top, footer_lines) = self.chrome();
        let body = height.saturating_sub(top + footer_lines) as usize;
        let len = self.rows.len();
        let selected = self
            .selection
            .get()
            .filter(|_| len > 0)
            .map(|s| s.min(len - 1));
        // The offset signals, set from outside, move the view.
        if let Some(offset) = self.options.offset {
            let value = offset.get();
            if self.seen.get() != Some(value) {
                self.first.set(value);
                self.seen.set(Some(value));
            }
        }
        if let Some(offset) = self.options.offset_x {
            let value = offset.get();
            if self.seen_x.get() != Some(value) {
                self.across.left.set(value);
                self.seen_x.set(Some(value));
            }
        }
        let sort = self.options.sort.and_then(|sort| sort.get());
        // A sorted column's title ends with an arrow.
        let titles = self.titles(sort);
        let console = cx.console();
        let spacing = self.options.spacing();
        let (gutter, marker, symbol) = self.indents(selected.is_some());
        let indent = marker + symbol;
        let single = self.options.single_line();
        // The rows in hand: every row once (multi-line rows need their
        // heights), or the ones looked at.
        let lazy_row = match &self.rows {
            Rows::Lazy { row, .. } if !self.options.sort_rows => Some(&**row),
            _ => None,
        };
        let mut fetch = match (single, lazy_row) {
            (false, Some(row)) => Fetch::Lazy {
                row,
                order: &self.order,
                identity: self.identity,
                cache: HashMap::new(),
            },
            (false, None) => Fetch::All(self.shown(0, len)),
            // One line a row: fetched once the first row is known.
            (true, _) => Fetch::All(Vec::new()),
        };
        // Keep the selection in view: with one line a row as it always was;
        // else by the rows' heights at the widths the rows near the old
        // first row give.
        let padding = self.options.scroll_padding;
        let guess = self.first.get().min(len.saturating_sub(1));
        let widths_at = |fetch: &mut Fetch, room: u16| -> Vec<u16> {
            let near: Vec<Vec<String>> = (guess..(guess + body.max(1)).min(len))
                .map(|i| fetch.row(i))
                .collect();
            let room = room.saturating_sub(indent);
            let tracks = self.tracks(&near, &titles, room);
            solve(room, spacing, &tracks)
        };
        let first_at = |fetch: &mut Fetch, widths: &[u16]| -> usize {
            let mut height = |i: usize| self.height_of(console, i, &fetch.row(i), widths);
            crate::list::first_in_view(len, body, self.first.get(), selected, padding, &mut height)
        };
        // The rows from `first` that fit in the body.
        let in_view = |fetch: &mut Fetch, first: usize, widths: &[u16]| -> usize {
            if single {
                return len.saturating_sub(first).min(body.max(1));
            }
            let (mut used, mut count) = (0, 0);
            for i in first..len {
                let h = self.height_of(console, i, &fetch.row(i), widths);
                if count > 0 && used + h > body {
                    break;
                }
                used += h;
                count += 1;
            }
            count
        };
        // The marker column keeps the first cell; the rows are drawn after
        // it.
        let mut content_width = width.saturating_sub(gutter);
        let mut widths = Vec::new();
        let mut first = if single {
            crate::list::first_in_view(len, body, self.first.get(), selected, padding, &mut |_| 1)
        } else {
            widths = widths_at(&mut fetch, content_width);
            first_at(&mut fetch, &widths)
        };
        // The bar keeps a column once the rows do not all fit.
        let mut bar_rect = None;
        if let Some(look) = self.bar.look.clone() {
            if look.wanted(len, in_view(&mut fetch, first, &widths)) {
                let (rest, at) = look.split(width, height);
                content_width = rest.width.saturating_sub(gutter);
                let at = crate::screen::Rect::new(at.x, top, at.width, body as u16);
                bar_rect = Some((rest.x, at));
                if !single {
                    widths = widths_at(&mut fetch, content_width);
                    first = first_at(&mut fetch, &widths);
                }
            }
        }
        let gutter_x = bar_rect.map_or(0, |(x, _)| x);
        let left = gutter_x + gutter;
        self.first.set(first);
        if let Some(offset) = self.options.offset {
            if self.seen.get() != Some(first) {
                self.seen.set(Some(first));
                crate::reactive::Runtime::current().untracked(|| offset.set(first));
            }
        }
        // The rows in view, and their heights.
        let mut placed: Vec<(usize, u16, u16)> = Vec::new();
        let mut rows: Vec<Vec<String>> = if single {
            let rows = match &fetch {
                Fetch::All(all) if !all.is_empty() => {
                    all.iter().skip(first).take(body).cloned().collect()
                }
                _ => self.shown(first, body),
            };
            for i in 0..rows.len() {
                placed.push((first + i, top + i as u16, 1));
            }
            rows
        } else {
            let mut rows = Vec::new();
            let mut used = 0usize;
            for i in first..len {
                let row = fetch.row(i);
                let h = self.height_of(console, i, &row, &widths);
                if !rows.is_empty() && used + h > body {
                    break;
                }
                placed.push((i, top + used as u16, h as u16));
                used += h;
                rows.push(row);
                if used >= body {
                    break;
                }
            }
            rows
        };
        drop(fetch);
        self.body.set(if single {
            body.max(1)
        } else {
            rows.len().max(1)
        });
        // Column widths: content columns fit their title and the rows in
        // view; a column the user dragged keeps that width.
        let room = content_width.saturating_sub(indent);
        let mut titles = titles;
        let mut tracks = self.tracks(&rows, &titles, room);
        // Columns with a priority that do not fit are dropped, whole.
        if !self.options.scroll_x {
            let gone = self.dropped(&tracks, &titles, room, spacing);
            if gone.contains(&true) {
                let kept = |i: &usize| !gone[*i];
                let keep = |cells: Vec<String>| -> Vec<String> {
                    cells
                        .into_iter()
                        .enumerate()
                        .filter(|(i, _)| kept(i))
                        .map(|(_, cell)| cell)
                        .collect()
                };
                titles = keep(titles);
                rows = rows.into_iter().map(keep).collect();
                let columns = std::mem::take(&mut self.columns);
                self.columns = columns
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| kept(i))
                    .map(|(_, column)| column)
                    .collect();
                let order = std::mem::take(&mut self.order);
                self.order = order
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| kept(i))
                    .map(|(_, c)| c)
                    .collect();
                self.identity = false;
                tracks = self.tracks(&rows, &titles, room);
            }
        }
        let cell = self
            .options
            .cell_cursor()
            .and_then(|column| self.place_of(column.get()));
        // Scrolling across (#717), columns that do not fit keep their
        // natural widths and a window shows some of them.
        let window = self
            .options
            .scroll_x
            .then(|| {
                let natural: Vec<u16> = tracks
                    .iter()
                    .enumerate()
                    .map(|(i, track)| match track.size {
                        Size::Fixed(width) => width,
                        _ => rows
                            .iter()
                            .filter_map(|row| row.get(i))
                            .map(|cell| markup_width(cell))
                            .chain([markup_width(&titles[i])])
                            .max()
                            .unwrap_or(0)
                            .clamp(track.min, track.max.max(track.min))
                            .max(1),
                    })
                    .collect();
                let window = self.across.window(
                    indent,
                    &natural,
                    content_width,
                    self.options.frozen(),
                    cell,
                    spacing,
                )?;
                Some((natural, window))
            })
            .flatten();
        if let Some(offset) = self.options.offset_x {
            let at = self.across.left.get();
            if self.seen_x.get() != Some(at) {
                self.seen_x.set(Some(at));
                crate::reactive::Runtime::current().untracked(|| offset.set(at));
            }
        }
        let widths = match &window {
            Some((natural, _)) => natural.clone(),
            None => solve(room, spacing, &tracks),
        };
        {
            let mut spans = self.spans.borrow_mut();
            spans.clear();
            match &window {
                Some((_, window)) => spans.extend(
                    window
                        .spans
                        .iter()
                        .map(|&(x, w)| (if w > 0 { x + left } else { x }, w)),
                ),
                None => {
                    let mut x = left + indent;
                    for w in &widths {
                        spans.push((x, *w));
                        x += w + spacing;
                    }
                }
            }
        }
        if let Some(layout) = &self.options.layout {
            let spans = self.spans.borrow();
            let mut laid = layout.0.borrow_mut();
            laid.widths = spans
                .iter()
                .enumerate()
                .filter(|(_, (_, w))| *w > 0 || window.is_none())
                .map(|(i, (_, w))| (self.original(i), *w))
                .collect();
            laid.first = first;
            laid.rows = self.body.get();
        }
        let window = window.map(|(_, window)| window);
        let blank_indent = (indent > 0).then(|| Segment::new(" ".repeat(indent as usize), None));
        // A header or footer line: blank where the marker and symbol go.
        let plain_line = |cells: &[String]| -> Vec<Segment> {
            let mut out: Vec<Segment> = blank_indent.iter().cloned().collect();
            let line = self
                .row_lines(console, cells, &widths, 1, spacing)
                .remove(0);
            out.extend(line);
            match &window {
                Some(window) => window.header(&out),
                None => out,
            }
        };
        if self.header {
            let header_style = cx.style("table.header", "bold");
            let header = plain_line(&titles);
            canvas.lines_at(left, 0, content_width, 1, &[over(header, &header_style)]);
        }
        if let Some(footer) = &self.options.footer {
            let style = cx.style("table.footer", "bold");
            let cells = pick(&self.order, self.identity, footer());
            let mut line = plain_line(&cells);
            if let Some(window) = &window {
                // The footer has no `‹` and `›`: those are the header's.
                let mut full: Vec<Segment> = blank_indent.iter().cloned().collect();
                full.extend(
                    self.row_lines(console, &cells, &widths, 1, spacing)
                        .remove(0),
                );
                line = window.cut(&full);
            }
            let y = height.saturating_sub(1);
            if y >= top {
                canvas.lines_at(left, y, content_width, 1, &[over(line, &style)]);
            }
        }
        let highlight = if cx.focused() {
            cx.style("selected", "reverse")
        } else {
            cx.style(self.blur, "underline")
        };
        let symbol_markup = |markup: &str| {
            let line = crate::widget::markup_line(console, markup, symbol);
            aligned(line, symbol, Align::Start)
        };
        let symbol_line = self
            .options
            .symbol
            .as_ref()
            .filter(|_| symbol > 0)
            .map(|markup| symbol_markup(markup));
        let row_gap = self.options.row_gap as usize;
        let layer = self.options.highlight_layer;
        for ((i, y, h), cells) in placed.iter().zip(&rows) {
            let is_selected = selected == Some(*i);
            let lines_tall = (*h as usize).saturating_sub(row_gap).max(1);
            let row_style = self
                .options
                .row_style
                .as_ref()
                .and_then(|style| style(*i))
                .map(|name| crate::widget::theme_style(console, &name, &name));
            // This row's own symbol, before the plain one.
            let own_symbol = self
                .options
                .symbols
                .as_ref()
                .filter(|_| symbol > 0)
                .and_then(|(_, symbols)| symbols(*i, is_selected))
                .map(|markup| symbol_markup(&markup));
            let lines = self.row_lines(console, cells, &widths, lines_tall, spacing);
            let mut out_lines = Vec::with_capacity(lines.len());
            for (n, line) in lines.into_iter().enumerate() {
                let mut segments = Vec::new();
                if marked_text() {
                    let mark = if is_selected && n == 0 { "> " } else { "  " };
                    segments.push(Segment::new(mark, None));
                }
                if symbol > 0 {
                    match (&own_symbol, &symbol_line) {
                        (Some(own), _) if n == 0 => segments.extend(own.iter().cloned()),
                        (None, Some(sym)) if is_selected && n == 0 => {
                            segments.extend(sym.iter().cloned())
                        }
                        _ => segments.push(Segment::new(" ".repeat(symbol as usize), None)),
                    }
                }
                segments.extend(line);
                if let Some(window) = &window {
                    segments = window.cut(&segments);
                }
                match (&row_style, is_selected, layer) {
                    // Under the cells' markup: the row style, then the
                    // highlight over it, as one style under the cells' own.
                    (row_style, true, HighlightLayer::Under) => {
                        let style = match row_style {
                            Some(row) => row.combine(&highlight),
                            None => highlight.clone(),
                        };
                        segments = under(segments, &style, content_width);
                    }
                    (row_style, is_selected, _) => {
                        if let Some(style) = row_style {
                            segments = under(segments, style, content_width);
                        }
                        if is_selected {
                            segments = crate::list::over(segments, &highlight, content_width);
                        }
                    }
                }
                out_lines.push(segments);
            }
            // The canvas starts clear: a row's gap lines stay blank.
            let room = height.saturating_sub(*y + footer_lines);
            let shown = (out_lines.len() as u16).min(room);
            canvas.lines_at(left, *y, content_width, shown, &out_lines);
            // The marker column, outside the row's style and highlight.
            if gutter > 0 {
                let mark = vec![Segment::new(" ", self.mark_of(console, *i))];
                for line in 0..shown {
                    canvas.lines_at(gutter_x, *y + line, 1, 1, std::slice::from_ref(&mark));
                }
            }
            // The cell cursor, over the selected row's cell.
            if let (true, Some(column)) = (is_selected, cell) {
                if let Some(&(x, w)) = self.spans.borrow().get(column) {
                    let style = cx.style("selected.cell", "reverse bold");
                    canvas.restyle(x, *y, w, shown, &style);
                }
            }
        }
        // The cursor's column, down the rows, under the cell cursor.
        if let (true, Some(column)) = (self.options.column_highlight, cell) {
            if let Some(&(x, w)) = self.spans.borrow().get(column) {
                let style = cx.style("selected.column", "bold");
                for (i, y, h) in &placed {
                    let lines = h.saturating_sub(self.options.row_gap).max(1);
                    let lines = lines.min(height.saturating_sub(*y + footer_lines));
                    canvas.restyle(x, *y, w, lines, &style);
                    if selected == Some(*i) {
                        let style = cx.style("selected.cell", "reverse bold");
                        canvas.restyle(x, *y, w, lines, &style);
                    }
                }
            }
        }
        // Where each row is on the screen, for a popup at one.
        if let Some(rects) = &self.rects {
            let shift = (cx.rect.x as i32 - cx.shift.0, cx.rect.y as i32 - cx.shift.1);
            rects.set(
                placed
                    .iter()
                    .filter(|(_, y, _)| *y < height.saturating_sub(footer_lines))
                    .map(|&(i, y, h)| {
                        let lines = h.saturating_sub(self.options.row_gap).max(1);
                        let lines = lines.min(height.saturating_sub(y + footer_lines));
                        let rect = crate::screen::Rect::new(left, y, content_width, lines);
                        (i, crate::node::translate(rect, shift))
                    })
                    .collect(),
            );
        }
        *self.placed.borrow_mut() = placed;
        match bar_rect {
            Some((_, at)) => {
                let look = self.bar.look.clone().expect("a bar was laid out");
                let view = rows.len().max(1);
                look.paint(console, canvas, at, first, len, view);
                self.bar_at.set(Some((at, first, len, view)));
            }
            None => self.bar_at.set(None),
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let page = self.body.get().max(1) as isize;
        // A press or a drag on the bar scrolls by it, carrying the
        // selection into view.
        if let (WidgetEvent::Mouse(mouse), Some((rect, first, len, view))) =
            (event, self.bar_at.get())
        {
            match self.bar.mouse(cx, mouse, rect, first, len, view) {
                Some(Some(to)) => {
                    self.first.set(to);
                    if let Some(selected) = self.selection.get_untracked() {
                        let last = (to + view).saturating_sub(1);
                        let inside = selected.clamp(to, last.max(to));
                        if inside != selected {
                            self.selection.set(inside);
                        }
                    }
                    cx.redraw();
                    return Used::Yes;
                }
                Some(None) => return Used::Yes,
                None => {}
            }
        }
        let cells = self.options.cell_cursor();
        match event {
            // A table that fits takes these keys as it always did.
            WidgetEvent::Key(key) if self.across.wide.get() && self.across_by(key).is_some() => {
                let by = self.across_by(key).expect("a scroll across");
                self.across.scroll(by, self.options.frozen());
                cx.redraw();
            }
            WidgetEvent::Key(key) => {
                let len = self.rows.len();
                match (key.code, !key.modifiers.ctrl && !key.modifiers.alt) {
                    (KeyCode::Up, true) | (KeyCode::Char('k'), true) => self.step(-1),
                    (KeyCode::Down, true) | (KeyCode::Char('j'), true) => self.step(1),
                    (KeyCode::PageUp, true) => self.step(-page),
                    (KeyCode::PageDown, true) => self.step(page),
                    (KeyCode::Home, true) | (KeyCode::Char('g'), true) => self.selection.first(len),
                    (KeyCode::End, true) => self.selection.last(len),
                    _ if *key == Key::char('G') => self.selection.last(len),
                    (KeyCode::Left, true) | (KeyCode::Char('h'), true) if cells.is_some() => {
                        self.move_column(-1)
                    }
                    (KeyCode::Right, true) | (KeyCode::Char('l'), true) if cells.is_some() => {
                        self.move_column(1)
                    }
                    // `s` sorts by the cursor's column, or the next one.
                    (KeyCode::Char('s'), true) if self.options.sort.is_some() => {
                        let shown = self.columns.len().max(1);
                        let column = match (cells, self.options.sort) {
                            (Some(column), _) => column.get_untracked(),
                            (None, Some(sort)) => match sort.get_untracked() {
                                Some((c, Order::Descending)) => {
                                    self.original((self.place_of(c).unwrap_or(0) + 1) % shown)
                                }
                                Some((c, Order::Ascending)) => c,
                                None => self.original(0),
                            },
                            (None, None) => 0,
                        };
                        self.sort_by(column);
                    }
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) if self.resizing.get().is_some() => match mouse.kind {
                MouseKind::Drag(Button::Left) => {
                    let (column, _) = self.resizing.get().expect("resizing");
                    let start = self.spans.borrow().get(column).map_or(0, |s| s.0);
                    let width = mouse.column.saturating_sub(start).max(1);
                    let index = self.original(column);
                    let set = |widths: &mut Vec<Option<u16>>| {
                        if widths.len() <= index {
                            widths.resize(index + 1, None);
                        }
                        widths[index] = Some(width);
                    };
                    match self.options.widths {
                        Some(widths) => widths.update(set),
                        None => set(&mut self.dragged.borrow_mut()),
                    }
                    cx.redraw();
                }
                MouseKind::Up(_) => {
                    self.resizing.set(None);
                    cx.release_mouse();
                }
                _ => return Used::No,
            },
            // The header is pressed and dragged; the wheel over it scrolls
            // the rows, as anywhere on the table.
            WidgetEvent::Mouse(mouse)
                if self.header
                    && mouse.row == 0
                    && !matches!(mouse.kind, MouseKind::ScrollUp | MouseKind::ScrollDown) =>
            {
                match mouse.kind {
                    MouseKind::Down(Button::Left) => {
                        if let Some(edge) = self
                            .edge_at(mouse.column)
                            .filter(|_| self.options.resizable)
                        {
                            self.resizing.set(Some((edge, mouse.column)));
                            cx.capture_mouse();
                        } else if let Some(column) = self.column_at(mouse.column) {
                            let column = self.original(column);
                            self.sort_by(column);
                            if let Some(cursor) = cells {
                                cursor.set(column);
                            }
                        }
                    }
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(button) if mouse.row >= self.header as u16 => {
                    if let (Some(cursor), Some(column)) = (cells, self.column_at(mouse.column)) {
                        cursor.set(self.original(column));
                    }
                    // Another button on a row selects it and leaves the
                    // press to the node's own handler: a right click's menu
                    // is about the row it selected. Below the rows it is
                    // used up, so no menu speaks for an old selection.
                    if let Some(row) = self.row_at(mouse.row) {
                        self.selection.set(row);
                        if button != Button::Left {
                            return Used::No;
                        }
                    }
                }
                // Shift with the wheel scrolls across (#717).
                MouseKind::ScrollUp | MouseKind::ScrollDown
                    if self.options.scroll_x && mouse.modifiers.shift =>
                {
                    let by = if mouse.kind == MouseKind::ScrollUp {
                        -1
                    } else {
                        1
                    };
                    if !self.across.scroll(by, self.options.frozen()) {
                        return Used::No;
                    }
                    cx.redraw();
                }
                MouseKind::ScrollUp => self.step(-3),
                MouseKind::ScrollDown => self.step(3),
                _ => return Used::No,
            },
            _ => return Used::No,
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// Whether rows are marked for text mode: `>` on the selected one.
fn marked_text() -> bool {
    crate::a11y::text_mode()
}

/// What a [`tabs`] strip calls with the title picked
/// ([`Node::on_select`]).
type OnSelect = Box<dyn FnMut(usize, &mut crate::app::Ctx)>;

struct Tabs {
    titles: std::rc::Rc<dyn Fn() -> Vec<String>>,
    /// Told the title picked, instead of the strip setting it.
    on_select: Option<OnSelect>,
    selected: Signal<usize>,
    /// Where each title was drawn: its first column and width.
    spans: RefCell<Vec<(u16, u16)>>,
    /// The markup between titles; `None` is `│`.
    divider: Option<String>,
    /// The markup either side of each title.
    padding: (String, String),
    /// Where the strip sits in its width.
    align: Align,
    /// Titles cut to share the width when they do not fit.
    fit: bool,
}

/// A strip of tab titles with one selected: ←/→ (or h/l) and the number
/// keys move it while the strip has the focus, and a click selects a
/// title. `titles` may read signals. Pair it with [`switch`](crate::switch)
/// on the same signal for the content:
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::tabs;
///
/// let app = App::new(|| {
///     let tab = signal(0usize);
///     column([
///         tabs(|| vec!["Overview".into(), "Logs".into()], tab).fixed(1),
///         switch(move || tab.get(), |tab| match tab {
///             0 => label("All systems go"),
///             _ => label("No logs yet"),
///         }),
///     ])
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["right", "q"], 30, 2).unwrap();
/// assert_eq!(screen[0].trim_end(), " Overview │ Logs");
/// assert_eq!(screen[1].trim_end(), "No logs yet");
/// ```
///
/// Its options are builders on the node: [`Node::divider`],
/// [`Node::tab_padding`], [`Node::titles_align`], [`Node::fit`],
/// [`Node::auto_hide`] and [`Node::on_select`]. For a set of tabs' states
/// to show, see [`tab_set`] and [`tabs_for`].
pub fn tabs(titles: impl Fn() -> Vec<String> + 'static, selected: Signal<usize>) -> Node {
    widget(Tabs {
        titles: std::rc::Rc::new(titles),
        on_select: None,
        selected,
        spans: RefCell::new(Vec::new()),
        divider: None,
        padding: (" ".into(), " ".into()),
        align: Align::Start,
        fit: false,
    })
}

impl Tabs {
    /// The divider, and the cells it takes.
    fn divider(&self) -> (String, u16) {
        let divider = match &self.divider {
            Some(divider) => divider.clone(),
            // In text mode, no line: a space.
            None if crate::a11y::text_mode() => " ".into(),
            None => "│".into(),
        };
        let width = markup_width(&divider);
        (divider, width)
    }

    /// The cells each title takes with its padding.
    fn widths(&self, titles: &[String]) -> Vec<u16> {
        let (left, right) = (markup_width(&self.padding.0), markup_width(&self.padding.1));
        titles
            .iter()
            .map(|title| {
                let left = if crate::a11y::text_mode() {
                    left.max(1)
                } else {
                    left
                };
                left.saturating_add(markup_width(title))
                    .saturating_add(right)
            })
            .collect()
    }

    /// The cells each title's text gets in a strip `width` wide: all it
    /// needs, or, [fitted](Node::fit) when they do not all fit, a fair
    /// share of the room the padding and dividers leave: the short titles
    /// whole, the long ones cut to an equal share.
    fn rooms(&self, titles: &[String], width: u16) -> Vec<u16> {
        let natural: Vec<u16> = titles.iter().map(|t| markup_width(t)).collect();
        if !self.fit || self.total(titles) <= width {
            return natural;
        }
        let padding: u16 = self
            .widths(titles)
            .iter()
            .zip(&natural)
            .map(|(w, n)| w - n)
            .fold(0, u16::saturating_add);
        let (_, divider) = self.divider();
        let gaps = divider.saturating_mul(titles.len().saturating_sub(1) as u16);
        let mut left = width.saturating_sub(padding).saturating_sub(gaps);
        let mut order: Vec<usize> = (0..titles.len()).collect();
        order.sort_by_key(|i| natural[*i]);
        let mut rooms = vec![0; titles.len()];
        for (done, i) in order.into_iter().enumerate() {
            let share = left / (titles.len() - done) as u16;
            rooms[i] = natural[i].min(share);
            left -= rooms[i];
        }
        rooms
    }

    /// How wide the whole strip is.
    fn total(&self, titles: &[String]) -> u16 {
        let (_, divider) = self.divider();
        let gaps = divider.saturating_mul(titles.len().saturating_sub(1) as u16);
        self.widths(titles)
            .iter()
            .fold(gaps, |sum, w| sum.saturating_add(*w))
    }
}

impl Widget for Tabs {
    fn name(&self) -> &'static str {
        "tabs"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::TabList
    }

    fn cursor(&self) -> Option<crate::screen::Rect> {
        let spans = self.spans.borrow();
        spans
            .get(self.selected.get_untracked())
            .map(|&(x, w)| crate::screen::Rect::new(x, 0, w, 1))
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        crate::a11y::AccessState::item(self.selected.get_untracked(), (self.titles)().len())
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => self.total(&(self.titles)()),
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let selected = self.selected.get();
        let chosen = if cx.focus_within() {
            cx.style("tabs.selected", "bold reverse")
        } else {
            cx.style("tabs.selected.blur", "bold underline")
        };
        let divider_style = cx.style("tabs.divider", "bright_black");
        let unselected = cx.style("tabs.unselected", "");
        let console = cx.console();
        let titles = (self.titles)();
        let rooms = self.rooms(&titles, canvas.width());
        let widths: Vec<u16> = self
            .widths(&titles)
            .iter()
            .zip(&titles)
            .zip(&rooms)
            .map(|((w, title), room)| w - markup_width(title) + room)
            .collect();
        let (divider, divider_width) = self.divider();
        let used = widths.iter().fold(
            divider_width.saturating_mul(titles.len().saturating_sub(1) as u16),
            |sum, w| sum.saturating_add(*w),
        );
        let room = canvas.width().saturating_sub(used);
        let mut x = match self.align {
            Align::Start | Align::Stretch => 0,
            Align::Center => room / 2,
            Align::End => room,
        };
        let text_mode = crate::a11y::text_mode();
        let mut spans = Vec::new();
        for (i, title) in titles.iter().enumerate() {
            if i > 0 {
                if self.divider.is_none() {
                    canvas.print(x, 0, &divider, Some(&divider_style));
                } else if divider_width > 0 {
                    canvas.markup(console, x, 0, divider_width, &divider, Some(&divider_style));
                }
                x = x.saturating_add(divider_width);
            }
            let w = widths[i];
            // In text mode the selected title is marked, not only styled:
            // `>` takes the first cell of its left padding.
            let left = if text_mode {
                let rest: String = self.padding.0.chars().skip(1).collect();
                let mark = if i == selected { ">" } else { " " };
                format!("{mark}{rest}")
            } else {
                self.padding.0.clone()
            };
            let line = if rooms[i] < markup_width(title) {
                // Cut to its share with `…`, its padding kept.
                let mut line = cell_line(console, &left, markup_width(&left));
                line.extend(cell_line(console, title, rooms[i]));
                line.extend(cell_line(
                    console,
                    &self.padding.1,
                    markup_width(&self.padding.1),
                ));
                line
            } else {
                cell_line(console, &format!("{left}{title}{}", self.padding.1), w)
            };
            let line = if i == selected {
                over(line, &chosen)
            } else if unselected.is_null() {
                line
            } else {
                // Under the title's own markup.
                line.into_iter()
                    .map(|segment| {
                        let style = match &segment.style {
                            Some(own) => unselected.combine(own),
                            None => unselected.clone(),
                        };
                        Segment::new(segment.text, Some(style))
                    })
                    .collect()
            };
            canvas.lines_at(x, 0, w, 1, &[line]);
            spans.push((x, w));
            x = x.saturating_add(w);
        }
        *self.spans.borrow_mut() = spans;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let last = (self.titles)().len().saturating_sub(1);
        let now = self.selected.get_untracked();
        let to = match event {
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Left | KeyCode::Char('h') => now.saturating_sub(1),
                KeyCode::Right | KeyCode::Char('l') => (now + 1).min(last),
                KeyCode::Char(c @ '1'..='9') => {
                    let i = c as usize - '1' as usize;
                    if i > last {
                        return Used::No;
                    }
                    i
                }
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) => {
                if !matches!(mouse.kind, MouseKind::Down(_)) {
                    return Used::No;
                }
                let hit = self
                    .spans
                    .borrow()
                    .iter()
                    .position(|(x, w)| mouse.column >= *x && mouse.column < x + w);
                match hit {
                    Some(i) => i,
                    None => return Used::No,
                }
            }
            _ => return Used::No,
        };
        match &mut self.on_select {
            Some(on_select) => on_select(to, cx.app()),
            None => self.selected.set(to),
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// A [`tabs`] strip's options. On any other node they do nothing.
impl Node {
    /// Console `markup` between the titles (`" • "`, or `""` for none)
    /// instead of `│`, in the theme's `tabs.divider` style.
    pub fn divider(self, markup: &str) -> Node {
        self.widget_mut(|tabs: &mut Tabs| tabs.divider = Some(markup.to_string()));
        self
    }

    /// The markup either side of each title, instead of a space: demo2's
    /// strip is `.tab_padding("", "")`. (A node's own
    /// [`padding`](Node::padding) is the space round the whole strip.)
    pub fn tab_padding(self, left: &str, right: &str) -> Node {
        self.widget_mut(|tabs: &mut Tabs| tabs.padding = (left.to_string(), right.to_string()));
        self
    }

    /// Where the titles sit in the strip's width: at the start (the
    /// default), the centre or the end.
    pub fn titles_align(self, align: Align) -> Node {
        self.widget_mut(|tabs: &mut Tabs| tabs.align = align);
        self
    }

    /// Cut the titles with `…` to share the strip's width when they do not
    /// all fit, rather than losing the last ones off its end: the short
    /// titles stay whole and the long ones are cut to an equal share, each
    /// keeping its padding (Yazi's tab bar).
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::tabs;
    ///
    /// let app = App::new(|| {
    ///     let titles = || vec!["src".to_string(), "documents".into(), "downloads".into()];
    ///     tabs(titles, signal(0)).fit()
    /// });
    /// let mut driver = app.driver(20, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], " src │ doc… │ down… ");
    /// ```
    ///
    /// Titles not selected are drawn in the theme's `tabs.unselected` style
    /// (none by default), under their own markup, fitted or not.
    pub fn fit(self) -> Node {
        self.widget_mut(|tabs: &mut Tabs| tabs.fit = true);
        self
    }

    /// Hide the strip while it has fewer than two titles: it takes no row
    /// then, and comes back with the second tab (Yazi's). Call it on the
    /// strip, before `.fixed(..)` or anything that wraps it.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::tabs;
    ///
    /// let app = App::new(|| {
    ///     let titles = signal(vec!["~".to_string()]);
    ///     column([
    ///         tabs(move || titles.get(), signal(0)).auto_hide().fixed(1),
    ///         label("files"),
    ///     ])
    ///     .on_key("t", move |_| titles.update(|t| t.push("src".into())))
    /// });
    /// let mut driver = app.driver(12, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "files");
    /// driver.press("t");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), " ~ │ src");
    /// ```
    pub fn auto_hide(self) -> Node {
        let titles = self
            .widget_mut(|tabs: &mut Tabs| tabs.titles.clone())
            .map(|titles| move || titles().len() >= 2);
        match titles {
            Some(shown) => self.shown_if("auto_hide", shown),
            None => self,
        }
    }

    /// Call `handler` with the title picked (a click, ←/→, a number key)
    /// instead of setting the strip's signal: the app moves to it its own
    /// way (Yazi's tab switch also ends visual mode), and sets the signal
    /// itself.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::tabs;
    ///
    /// let app = App::new(|| {
    ///     let active = signal(0usize);
    ///     let picks = signal(0);
    ///     column([
    ///         tabs(|| vec!["one".into(), "two".into()], active)
    ///             .on_select(move |to, _| {
    ///                 picks.update(|p| *p += 1);
    ///                 active.set(to);
    ///             })
    ///             .fixed(1),
    ///         text!("{active} after {picks}"),
    ///     ])
    /// });
    /// let mut driver = app.driver(20, 2);
    /// driver.press("right");
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "1 after 1");
    /// ```
    pub fn on_select(self, handler: impl FnMut(usize, &mut crate::app::Ctx) + 'static) -> Node {
        self.widget_mut(|tabs: &mut Tabs| tabs.on_select = Some(Box::new(handler)));
        self
    }
}
