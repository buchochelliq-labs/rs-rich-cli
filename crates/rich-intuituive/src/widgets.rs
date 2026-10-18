//! Ready-made widgets, built on the public [`Widget`] trait as an app's own
//! would be: a [`table`] with a selectable row and sized columns, a
//! [`virtual_list`] of any length, a [`tabs`] strip, a [`tree`], [`split`]
//! panes, a [`calendar`], a [`time_picker`] and a [`datetime_picker`].
//!
//! [`scroll`](crate::scroll) and anchored pop-ups
//! ([`Ctx::popup`](crate::Ctx::popup)) are part of the framework itself,
//! because they change how a subtree is drawn or layered.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use rich::{Console, Segment, Style};
use rich_interact::{Button, Key, KeyCode, MouseKind};

use crate::layout::{solve, Align, Size, Track};
use crate::list::Selection;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::scrollbar::Bar;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

pub use crate::calendar::{
    calendar, calendar_options, calendar_with, month, CalendarOptions, Date, DateTime, Time,
    Weekday,
};
pub use crate::datetime::{
    datetime_picker, time_picker, time_picker_with, DateTimeOptions, TimeOptions,
};
pub use crate::list::{list_optional, HighlightSpacing, ListDirection};
pub use crate::reorder::Move;
pub use crate::scrollbar::{scrollbar, Scrollbar, ScrollbarSymbols, Show, Side};
pub use crate::split::{hsplit, split, split_with, vsplit};
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
}

impl Column {
    pub fn new(title: &str, size: Size) -> Column {
        Column {
            title: title.to_string(),
            size,
            min: 0,
            max: u16::MAX,
            align: Align::Start,
        }
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

    fn track(&self, size: Size) -> Track {
        let mut track = Track::new(size);
        track.min = self.min;
        track.max = self.max.max(self.min);
        track
    }
}

/// Where a table's rows come from.
enum Rows {
    /// Every row, from a closure that may read signals.
    All(Box<dyn Fn() -> Vec<Vec<String>>>),
    /// A count and one row at a time: only the rows in view are asked for.
    Lazy {
        len: Box<dyn Fn() -> usize>,
        row: Box<dyn Fn(usize) -> Vec<String>>,
    },
}

impl Rows {
    fn len(&self) -> usize {
        match self {
            Rows::All(rows) => rows().len(),
            Rows::Lazy { len, .. } => len(),
        }
    }

    fn window(&self, from: usize, count: usize) -> Vec<Vec<String>> {
        match self {
            Rows::All(rows) => rows().into_iter().skip(from).take(count).collect(),
            Rows::Lazy { len, row } => (from..(from + count).min(len())).map(row).collect(),
        }
    }
}

pub(crate) struct Table {
    name: &'static str,
    columns: Vec<Column>,
    rows: Rows,
    selection: Selection,
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
    /// Not for a [`virtual_table`], which takes
    /// [`row_height`](Self::row_height).
    pub multiline: bool,
    /// Every row this many lines tall (cells wrap with
    /// [`multiline`](Self::multiline), and are cut at it).
    pub row_height: Option<u16>,
    /// With a cell cursor ([`cells`](Self::cells)), the cursor's column
    /// down the rows in the theme's `selected.column` style.
    pub column_highlight: bool,
    /// Rows kept in view either side of the selected one, where they fit.
    pub scroll_padding: usize,
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

    /// Cells between columns.
    fn spacing(&self) -> u16 {
        self.column_spacing.unwrap_or(1)
    }

    /// Whether every row is one line: the old arithmetic, with no heights
    /// to find.
    fn single_line(&self) -> bool {
        !self.multiline && self.row_height.unwrap_or(1) == 1 && self.row_gap == 0
    }
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
    table_from(columns, Rows::All(Box::new(rows)), selected)
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
        len: Box::new(len),
        row: Box::new(row),
    };
    table_from(columns, rows, selected)
}

/// A [`virtual_table`] with a cell cursor, resizable columns or a scroll
/// across, as `options` asks. Its rows are never all in hand, so it does
/// not sort them itself ([`TableOptions::sort_rows`]); `row` may read a
/// [`sort`](TableOptions::sort) signal. Its rows are as tall as
/// [`TableOptions::row_height`] says.
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
        len: Box::new(len),
        row: Box::new(row),
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
        len: Box::new(len),
        row: Box::new(move |i| vec![row(i)]),
    };
    widget(Table {
        name: "virtual_list",
        columns: vec![Column::new("", Size::Flex(1))],
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
        Rows::All(Box::new(rows)),
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
        Rows::All(Box::new(rows)),
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
        columns,
        rows,
        selection,
        header: true,
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
    if width == 0 {
        return Vec::new();
    }
    let text =
        rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
    let mut options = console.options().update_width(width as usize);
    options.no_wrap = Some(true);
    options.overflow = Some(rich::Overflow::Ellipsis);
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
    wrap: bool,
) -> Vec<Vec<Segment>> {
    if !wrap && matches!(align, Align::Start | Align::Stretch) {
        return vec![cell_line(console, markup, width)];
    }
    if width == 0 {
        return vec![Vec::new()];
    }
    let text =
        rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
    let mut options = console.options().update_width(width as usize);
    if !wrap {
        options.no_wrap = Some(true);
        options.overflow = Some(rich::Overflow::Ellipsis);
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

/// The rows a draw looks at: all of them once, or one at a time.
enum Fetch<'a> {
    All(Vec<Vec<String>>),
    Lazy(
        &'a dyn Fn(usize) -> Vec<String>,
        HashMap<usize, Vec<String>>,
    ),
}

impl Fetch<'_> {
    fn row(&mut self, i: usize) -> Vec<String> {
        match self {
            Fetch::All(rows) => rows.get(i).cloned().unwrap_or_default(),
            Fetch::Lazy(row, cache) => cache.entry(i).or_insert_with(|| row(i)).clone(),
        }
    }
}

impl Table {
    fn step(&self, by: isize) {
        self.selection.step(self.rows.len(), by);
    }

    /// Rows `from..from + count` in the order shown: sorted here when the
    /// table sorts its own rows.
    fn shown(&self, from: usize, count: usize) -> Vec<Vec<String>> {
        let order = self.options.sort.filter(|_| self.options.sort_rows);
        match (&self.rows, order.and_then(|sort| sort.get())) {
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
        }
    }

    /// Sort by `column`: ascending first, then the other way on the same
    /// column.
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

    fn move_column(&self, by: isize) {
        if let Some(column) = self.options.column {
            let last = self.columns.len().saturating_sub(1) as isize;
            column.update(|c| *c = (*c as isize + by).clamp(0, last.max(0)) as usize);
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
        let free = modifiers.shift || self.options.column.is_none();
        match key.code {
            KeyCode::Left if free => Some(-1),
            KeyCode::Right if free => Some(1),
            KeyCode::Char('h') if self.options.column.is_none() => Some(-1),
            KeyCode::Char('l') if self.options.column.is_none() => Some(1),
            KeyCode::Home if modifiers.shift => Some(isize::MIN),
            KeyCode::End if modifiers.shift => Some(isize::MAX),
            _ => None,
        }
    }

    /// The column drawn at `x`, if any.
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

    /// The cells before a row's first column: the text mode's marker and the
    /// highlight symbol's column.
    fn indents(&self, selected: bool) -> (u16, u16) {
        let marker = if crate::a11y::text_mode() { 2 } else { 0 };
        let symbol = match &self.options.symbol {
            Some(symbol) if self.options.highlight_spacing.keeps(selected) => markup_width(symbol),
            _ => 0,
        };
        (marker, symbol)
    }

    /// Each column's track: its size (or the width it was dragged to), its
    /// least and most, and for [`Size::Auto`] the widest of its title and
    /// `rows`.
    fn tracks(&self, rows: &[Vec<String>], titles: &[String]) -> Vec<Track> {
        let dragged = self.dragged.borrow();
        self.columns
            .iter()
            .enumerate()
            .map(|(i, column)| {
                let size = match dragged.get(i).copied().flatten() {
                    Some(width) => Size::Fixed(width),
                    None => column.size,
                };
                let mut track = column.track(size);
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

    /// The titles, a sorted column's with an arrow.
    fn titles(&self, sort: Option<(usize, Order)>) -> Vec<String> {
        self.columns
            .iter()
            .enumerate()
            .map(|(i, column)| match sort {
                Some((c, Order::Ascending)) if c == i => format!("{} ▲", column.title),
                Some((c, Order::Descending)) if c == i => format!("{} ▼", column.title),
                _ => column.title.clone(),
            })
            .collect()
    }

    /// How many lines row `cells` takes at `widths`, gap included.
    fn height_of(&self, console: &Console, cells: &[String], widths: &[u16]) -> usize {
        let options = &self.options;
        let lines = match options.row_height {
            Some(height) => height as usize,
            None if options.multiline => widths
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    let markup = cells.get(i).map_or("", String::as_str);
                    let align = self.columns.get(i).map_or(Align::Start, |c| c.align);
                    cell_lines(console, markup, *w, align, true).len()
                })
                .max()
                .unwrap_or(1)
                .max(1),
            None => 1,
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
        let wrap = self.options.multiline;
        let columns: Vec<Vec<Vec<Segment>>> = widths
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let markup = cells.get(i).map_or("", String::as_str);
                let align = self.columns.get(i).map_or(Align::Start, |c| c.align);
                cell_lines(console, markup, *w, align, wrap)
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
}

impl Widget for Table {
    fn name(&self) -> &'static str {
        self.name
    }

    fn role(&self) -> crate::a11y::Role {
        if self.header {
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
        let cell = self.options.column.and_then(|column| {
            let spans = self.spans.borrow();
            spans.get(column.get_untracked()).copied()
        });
        let height = height.saturating_sub(self.options.row_gap).max(1);
        Some(match cell {
            Some((x, w)) => crate::screen::Rect::new(x, top, w, height),
            None => {
                let (marker, symbol) = self.indents(true);
                let x = if symbol > 0 { marker + symbol } else { 0 };
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
                let (top, footer) = self.chrome();
                let chrome = (top + footer) as usize;
                let len = self.rows.len();
                let lines = if self.options.single_line() {
                    len
                } else if let (Some(height), false) =
                    (self.options.row_height, self.options.multiline)
                {
                    len * (height as usize + self.options.row_gap as usize)
                } else {
                    // Every row, laid out at this width.
                    let rows = self.shown(0, len);
                    let titles = self.titles(None);
                    let (marker, symbol) = self.indents(self.selection.get_untracked().is_some());
                    let spacing = self.options.spacing();
                    let tracks = self.tracks(&rows, &titles);
                    let widths = solve(width.saturating_sub(marker + symbol), spacing, &tracks);
                    rows.iter()
                        .map(|row| self.height_of(cx.console(), row, &widths))
                        .sum()
                };
                (lines + chrome).min(u16::MAX as usize) as u16
            }
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        let (top, footer_lines) = self.chrome();
        let body = height.saturating_sub(top + footer_lines) as usize;
        let len = self.rows.len();
        let selected = self
            .selection
            .get()
            .filter(|_| len > 0)
            .map(|s| s.min(len - 1));
        let sort = self.options.sort.and_then(|sort| sort.get());
        // A sorted column's title ends with an arrow.
        let titles = self.titles(sort);
        let console = cx.console();
        let spacing = self.options.spacing();
        let (marker, symbol) = self.indents(selected.is_some());
        let indent = marker + symbol;
        let single = self.options.single_line();
        // The rows in hand: every row once (multi-line rows need their
        // heights), or the ones looked at.
        let lazy_row = match &self.rows {
            Rows::Lazy { row, .. } if !self.options.sort_rows => Some(&**row),
            _ => None,
        };
        let mut fetch = match (single, lazy_row) {
            (false, Some(row)) => Fetch::Lazy(row, HashMap::new()),
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
            let tracks = self.tracks(&near, &titles);
            solve(room.saturating_sub(indent), spacing, &tracks)
        };
        let first_at = |fetch: &mut Fetch, widths: &[u16]| -> usize {
            let mut height = |i: usize| self.height_of(console, &fetch.row(i), widths);
            crate::list::first_in_view(len, body, self.first.get(), selected, padding, &mut height)
        };
        // The rows from `first` that fit in the body.
        let in_view = |fetch: &mut Fetch, first: usize, widths: &[u16]| -> usize {
            if single {
                return len.saturating_sub(first).min(body.max(1));
            }
            let (mut used, mut count) = (0, 0);
            for i in first..len {
                let h = self.height_of(console, &fetch.row(i), widths);
                if count > 0 && used + h > body {
                    break;
                }
                used += h;
                count += 1;
            }
            count
        };
        let mut content_width = width;
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
                content_width = rest.width;
                let at = crate::screen::Rect::new(at.x, top, at.width, body as u16);
                bar_rect = Some((rest.x, at));
                if !single {
                    widths = widths_at(&mut fetch, content_width);
                    first = first_at(&mut fetch, &widths);
                }
            }
        }
        let left = bar_rect.map_or(0, |(x, _)| x);
        self.first.set(first);
        // The rows in view, and their heights.
        let mut placed: Vec<(usize, u16, u16)> = Vec::new();
        let rows: Vec<Vec<String>> = if single {
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
                let h = self.height_of(console, &row, &widths);
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
        self.body.set(if single {
            body.max(1)
        } else {
            rows.len().max(1)
        });
        // Column widths: content columns fit their title and the rows in
        // view; a column the user dragged keeps that width.
        let tracks = self.tracks(&rows, &titles);
        let cell = self.options.column.map(|column| column.get());
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
                    self.options.freeze,
                    cell,
                    spacing,
                )?;
                Some((natural, window))
            })
            .flatten();
        let widths = match &window {
            Some((natural, _)) => natural.clone(),
            None => solve(content_width.saturating_sub(indent), spacing, &tracks),
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
            let cells = footer();
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
        let symbol_line = self
            .options
            .symbol
            .as_ref()
            .filter(|_| symbol > 0)
            .map(|markup| {
                let line = crate::widget::markup_line(console, markup, symbol);
                aligned(line, symbol, Align::Start)
            });
        let row_gap = self.options.row_gap as usize;
        for ((i, y, h), cells) in placed.iter().zip(&rows) {
            let is_selected = selected == Some(*i);
            let lines_tall = (*h as usize).saturating_sub(row_gap).max(1);
            let row_style = self
                .options
                .row_style
                .as_ref()
                .and_then(|style| style(*i))
                .map(|name| crate::widget::theme_style(console, &name, &name));
            let lines = self.row_lines(console, cells, &widths, lines_tall, spacing);
            let mut out_lines = Vec::with_capacity(lines.len());
            for (n, line) in lines.into_iter().enumerate() {
                let mut segments = Vec::new();
                if marked_text() {
                    let mark = if is_selected && n == 0 { "> " } else { "  " };
                    segments.push(Segment::new(mark, None));
                }
                if symbol > 0 {
                    match &symbol_line {
                        Some(sym) if is_selected && n == 0 => segments.extend(sym.iter().cloned()),
                        _ => segments.push(Segment::new(" ".repeat(symbol as usize), None)),
                    }
                }
                segments.extend(line);
                if let Some(window) = &window {
                    segments = window.cut(&segments);
                }
                if let Some(style) = &row_style {
                    segments = under(segments, style, content_width);
                }
                if is_selected {
                    segments = crate::list::over(segments, &highlight, content_width);
                }
                out_lines.push(segments);
            }
            // The canvas starts clear: a row's gap lines stay blank.
            let room = height.saturating_sub(*y + footer_lines);
            let shown = (out_lines.len() as u16).min(room);
            canvas.lines_at(left, *y, content_width, shown, &out_lines);
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
        match event {
            // A table that fits takes these keys as it always did.
            WidgetEvent::Key(key) if self.across.wide.get() && self.across_by(key).is_some() => {
                let by = self.across_by(key).expect("a scroll across");
                self.across.scroll(by, self.options.freeze);
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
                    (KeyCode::Left, true) | (KeyCode::Char('h'), true)
                        if self.options.column.is_some() =>
                    {
                        self.move_column(-1)
                    }
                    (KeyCode::Right, true) | (KeyCode::Char('l'), true)
                        if self.options.column.is_some() =>
                    {
                        self.move_column(1)
                    }
                    // `s` sorts by the cursor's column, or the next one.
                    (KeyCode::Char('s'), true) if self.options.sort.is_some() => {
                        let column = match (self.options.column, self.options.sort) {
                            (Some(column), _) => column.get_untracked(),
                            (None, Some(sort)) => match sort.get_untracked() {
                                Some((c, Order::Descending)) => (c + 1) % self.columns.len().max(1),
                                Some((c, Order::Ascending)) => c,
                                None => 0,
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
                    if let Some(slot) = self.dragged.borrow_mut().get_mut(column) {
                        *slot = Some(width);
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
                            self.sort_by(column);
                            if let Some(cursor) = self.options.column {
                                cursor.set(column);
                            }
                        }
                    }
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(button) if mouse.row >= self.header as u16 => {
                    if let (Some(cursor), Some(column)) =
                        (self.options.column, self.column_at(mouse.column))
                    {
                        cursor.set(column);
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
                    if !self.across.scroll(by, self.options.freeze) {
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

struct Tabs {
    titles: Box<dyn Fn() -> Vec<String>>,
    selected: Signal<usize>,
    /// Where each title was drawn: its first column and width.
    spans: RefCell<Vec<(u16, u16)>>,
    /// The markup between titles; `None` is `│`.
    divider: Option<String>,
    /// The markup either side of each title.
    padding: (String, String),
    /// Where the strip sits in its width.
    align: Align,
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
/// [`Node::tab_padding`] and [`Node::titles_align`].
pub fn tabs(titles: impl Fn() -> Vec<String> + 'static, selected: Signal<usize>) -> Node {
    widget(Tabs {
        titles: Box::new(titles),
        selected,
        spans: RefCell::new(Vec::new()),
        divider: None,
        padding: (" ".into(), " ".into()),
        align: Align::Start,
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
        let console = cx.console();
        let titles = (self.titles)();
        let widths = self.widths(&titles);
        let (divider, divider_width) = self.divider();
        let room = canvas.width().saturating_sub(self.total(&titles));
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
            let line = cell_line(console, &format!("{left}{title}{}", self.padding.1), w);
            let line = if i == selected {
                over(line, &chosen)
            } else {
                line
            };
            canvas.lines_at(x, 0, w, 1, &[line]);
            spans.push((x, w));
            x = x.saturating_add(w);
        }
        *self.spans.borrow_mut() = spans;
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let last = (self.titles)().len().saturating_sub(1);
        match event {
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Left | KeyCode::Char('h') => {
                    self.selected.update(|s| *s = s.saturating_sub(1))
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    self.selected.update(|s| *s = (*s + 1).min(last))
                }
                KeyCode::Char(c @ '1'..='9') => {
                    let i = c as usize - '1' as usize;
                    if i > last {
                        return Used::No;
                    }
                    self.selected.set(i);
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
                    Some(i) => self.selected.set(i),
                    None => return Used::No,
                }
            }
            _ => return Used::No,
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
}
