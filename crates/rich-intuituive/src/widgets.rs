//! Ready-made widgets, built on the public [`Widget`] trait as an app's own
//! would be: a [`table`] with a selectable row and sized columns, a
//! [`virtual_list`] of any length, a [`tabs`] strip, a [`tree`], [`split`]
//! panes and a [`calendar`].
//!
//! [`scroll`](crate::scroll) and anchored pop-ups
//! ([`Ctx::popup`](crate::Ctx::popup)) are part of the framework itself,
//! because they change how a subtree is drawn or layered.

use std::cell::{Cell, RefCell};

use rich::{Console, Segment, Style};
use rich_interact::{Button, Key, KeyCode, MouseKind};

use crate::layout::{solve, Size, Track};
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

pub use crate::calendar::{calendar, calendar_with, Date};
pub use crate::split::{hsplit, split, split_with, vsplit};
pub use crate::tree::{tree, tree_lazy, tree_with, LazyItem, TreeItem};

/// A table column: its title and how wide it is (cells, a percentage, a
/// flexible share, or [`Size::Auto`]: as wide as its title and the cells
/// in view).
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub title: String,
    pub size: Size,
}

impl Column {
    pub fn new(title: &str, size: Size) -> Column {
        Column {
            title: title.to_string(),
            size,
        }
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

struct Table {
    name: &'static str,
    columns: Vec<Column>,
    rows: Rows,
    selected: Signal<usize>,
    /// Whether the header row is drawn (a list has none).
    header: bool,
    /// The theme style of the selection while it does not have the focus.
    blur: &'static str,
    /// The first row in view, and the rows of the body last drawn.
    first: Cell<usize>,
    body: Cell<usize>,
    options: TableOptions,
    /// Widths the user dragged a column to, by column.
    dragged: RefCell<Vec<Option<u16>>>,
    /// Where each column was last drawn: its first column and width.
    spans: RefCell<Vec<(u16, u16)>>,
    /// The column whose right edge is being dragged, and where the drag
    /// started.
    resizing: Cell<Option<(usize, u16)>>,
}

/// Which way a table's column is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Ascending,
    Descending,
}

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
}

/// A table of markup cells under a header row that stays in view, with one
/// row selected: ↑/↓ (or k/j), PgUp/PgDn and Home/End (g/G) move it, a
/// click selects a row, and the wheel moves it. The selection is
/// highlighted in the theme's `selected` style while the table has the
/// focus, and `table.selected` otherwise. Columns share the width by their
/// [`Size`]; `rows` may read signals.
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
        selected,
        header: false,
        blur: "list.selected",
        first: Cell::new(0),
        body: Cell::new(1),
        options: TableOptions::default(),
        dragged: RefCell::new(vec![None]),
        spans: RefCell::new(Vec::new()),
        resizing: Cell::new(None),
    })
}

fn table_from(columns: Vec<Column>, rows: Rows, selected: Signal<usize>) -> Node {
    table_options(columns, rows, selected, TableOptions::default())
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
    table_options(columns, Rows::All(Box::new(rows)), selected, options)
}

fn table_options(
    columns: Vec<Column>,
    rows: Rows,
    selected: Signal<usize>,
    options: TableOptions,
) -> Node {
    let count = columns.len();
    widget(Table {
        name: "table",
        columns,
        rows,
        selected,
        header: true,
        blur: "table.selected",
        first: Cell::new(0),
        body: Cell::new(1),
        options,
        dragged: RefCell::new(vec![None; count]),
        spans: RefCell::new(Vec::new()),
        resizing: Cell::new(None),
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

impl Table {
    fn step(&self, by: isize) {
        let last = self.rows.len().saturating_sub(1) as isize;
        self.selected
            .update(|s| *s = (*s as isize + by).clamp(0, last.max(0)) as usize);
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

    /// The column drawn at `x`, if any.
    fn column_at(&self, x: u16) -> Option<usize> {
        self.spans
            .borrow()
            .iter()
            .position(|(start, width)| (*start..start + width).contains(&x))
    }

    /// The column whose right edge (the gap after it) is at `x`.
    fn edge_at(&self, x: u16) -> Option<usize> {
        let spans = self.spans.borrow();
        (0..spans.len().saturating_sub(1)).find(|&i| spans[i].0 + spans[i].1 == x)
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
        let row = self
            .selected
            .get_untracked()
            .checked_sub(self.first.get())?;
        if row >= self.body.get().max(1) {
            return None;
        }
        let y = row as u16 + u16::from(self.header);
        // With a cell cursor, the cell; else the row.
        let cell = self.options.column.and_then(|column| {
            let spans = self.spans.borrow();
            spans.get(column.get_untracked()).copied()
        });
        Some(match cell {
            Some((x, w)) => crate::screen::Rect::new(x, y, w, 1),
            None => crate::screen::Rect::new(0, y, u16::MAX, 1),
        })
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        crate::a11y::AccessState::item(self.selected.get_untracked(), self.rows.len())
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => {
                (self.rows.len() + self.header as usize).min(u16::MAX as usize) as u16
            }
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        let top = self.header as u16;
        let body = height.saturating_sub(top) as usize;
        self.body.set(body.max(1));
        let len = self.rows.len();
        let selected = self.selected.get().min(len.saturating_sub(1));
        // Keep the selection in view.
        let mut first = self.first.get().min(len.saturating_sub(body.max(1)));
        if selected < first {
            first = selected;
        } else if body > 0 && selected >= first + body {
            first = selected + 1 - body;
        }
        self.first.set(first);
        let rows = self.shown(first, body);
        let sort = self.options.sort.and_then(|sort| sort.get());
        // A sorted column's title ends with an arrow.
        let titles: Vec<String> = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, column)| match sort {
                Some((c, Order::Ascending)) if c == i => format!("{} ▲", column.title),
                Some((c, Order::Descending)) if c == i => format!("{} ▼", column.title),
                _ => column.title.clone(),
            })
            .collect();
        let dragged = self.dragged.borrow().clone();
        // Column widths: content columns fit their title and the rows in
        // view; a column the user dragged keeps that width.
        let tracks: Vec<Track> = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, column)| {
                let size = match dragged.get(i).copied().flatten() {
                    Some(width) => Size::Fixed(width),
                    None => column.size,
                };
                let mut track = Track::new(size);
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
            .collect();
        // In text mode every row starts with a marker column: `>` on the
        // selected row.
        let marked = crate::a11y::text_mode();
        let indent: u16 = if marked { 2 } else { 0 };
        let widths = solve(width.saturating_sub(indent), 1, &tracks);
        {
            let mut spans = self.spans.borrow_mut();
            spans.clear();
            let mut x = indent;
            for w in &widths {
                spans.push((x, *w));
                x += w + 1;
            }
        }
        let cell = self.options.column.map(|column| column.get());
        let console = cx.console();
        let line = |cells: &mut dyn Iterator<Item = &str>, mark: bool| -> Vec<Segment> {
            let mut out = Vec::new();
            if marked {
                out.push(Segment::new(if mark { "> " } else { "  " }, None));
            }
            for (i, w) in widths.iter().enumerate() {
                if i > 0 {
                    out.push(Segment::new(" ", None));
                }
                out.extend(cell_line(console, cells.next().unwrap_or(""), *w));
            }
            out
        };
        if self.header {
            let header_style = cx.style("table.header", "bold");
            let header = line(&mut titles.iter().map(String::as_str), false);
            canvas.lines_at(0, 0, width, 1, &[over(header, &header_style)]);
        }
        let highlight = if cx.focused() {
            cx.style("selected", "reverse")
        } else {
            cx.style(self.blur, "underline")
        };
        for (i, row) in rows.iter().enumerate() {
            let mut cells = row.iter().map(String::as_str);
            let mut segments = line(&mut cells, first + i == selected);
            if first + i == selected {
                let used: usize = segments.iter().map(Segment::cell_length).sum();
                if used < width as usize {
                    segments.push(Segment::new(" ".repeat(width as usize - used), None));
                }
                segments = over(segments, &highlight);
            }
            canvas.lines_at(0, top + i as u16, width, 1, &[segments]);
            // The cell cursor, over the selected row's cell.
            if let (true, Some(column)) = (first + i == selected, cell) {
                if let Some(&(x, w)) = self.spans.borrow().get(column) {
                    let style = cx.style("selected.cell", "reverse bold");
                    canvas.restyle(x, top + i as u16, w, 1, &style);
                }
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let page = self.body.get().max(1) as isize;
        match event {
            WidgetEvent::Key(key) => {
                let len = self.rows.len();
                match (key.code, !key.modifiers.ctrl && !key.modifiers.alt) {
                    (KeyCode::Up, true) | (KeyCode::Char('k'), true) => self.step(-1),
                    (KeyCode::Down, true) | (KeyCode::Char('j'), true) => self.step(1),
                    (KeyCode::PageUp, true) => self.step(-page),
                    (KeyCode::PageDown, true) => self.step(page),
                    (KeyCode::Home, true) | (KeyCode::Char('g'), true) => self.selected.set(0),
                    (KeyCode::End, true) => self.selected.set(len.saturating_sub(1)),
                    _ if *key == Key::char('G') => self.selected.set(len.saturating_sub(1)),
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
                    let row = self.first.get() + (mouse.row - self.header as u16) as usize;
                    // Another button on a row selects it and leaves the
                    // press to the node's own handler: a right click's menu
                    // is about the row it selected. Below the rows it is
                    // used up, so no menu speaks for an old selection.
                    if row < self.rows.len() {
                        self.selected.set(row);
                        if button != Button::Left {
                            return Used::No;
                        }
                    }
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

struct Tabs {
    titles: Box<dyn Fn() -> Vec<String>>,
    selected: Signal<usize>,
    /// Where each title was drawn: its first column and width.
    spans: RefCell<Vec<(u16, u16)>>,
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
pub fn tabs(titles: impl Fn() -> Vec<String> + 'static, selected: Signal<usize>) -> Node {
    widget(Tabs {
        titles: Box::new(titles),
        selected,
        spans: RefCell::new(Vec::new()),
    })
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
            Axis::Horizontal => (self.titles)()
                .iter()
                .map(|t| markup_width(t) + 3)
                .sum::<u16>()
                .saturating_sub(1),
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let selected = self.selected.get();
        let chosen = if cx.focus_within() {
            cx.style("tabs.selected", "bold reverse")
        } else {
            cx.style("tabs.selected.blur", "bold underline")
        };
        let divider = cx.style("tabs.divider", "bright_black");
        let console = cx.console();
        let mut spans = Vec::new();
        let mut x = 0u16;
        for (i, title) in (self.titles)().iter().enumerate() {
            if i > 0 {
                let line = if crate::a11y::text_mode() { " " } else { "│" };
                canvas.print(x, 0, line, Some(&divider));
                x += 1;
            }
            let w = markup_width(title) + 2;
            // In text mode the selected title is marked, not only styled.
            let mark = if crate::a11y::text_mode() && i == selected {
                ">"
            } else {
                " "
            };
            let line = cell_line(console, &format!("{mark}{title} "), w);
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
