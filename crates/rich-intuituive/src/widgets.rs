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
use rich_interact::{Key, KeyCode, MouseKind};

use crate::layout::{solve, Size, Track};
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

pub use crate::calendar::{calendar, calendar_with, Date};
pub use crate::split::{hsplit, split, split_with, vsplit};
pub use crate::tree::{tree, tree_with, TreeItem};

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
    })
}

fn table_from(columns: Vec<Column>, rows: Rows, selected: Signal<usize>) -> Node {
    widget(Table {
        name: "table",
        columns,
        rows,
        selected,
        header: true,
        blur: "table.selected",
        first: Cell::new(0),
        body: Cell::new(1),
    })
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
}

impl Widget for Table {
    fn name(&self) -> &'static str {
        self.name
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
        let rows = self.rows.window(first, body);
        // Column widths: content columns fit their title and the rows in
        // view.
        let tracks: Vec<Track> = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, column)| {
                let mut track = Track::new(column.size);
                if column.size == Size::Auto {
                    track.content = rows
                        .iter()
                        .filter_map(|row| row.get(i))
                        .map(|cell| markup_width(cell))
                        .chain([markup_width(&column.title)])
                        .max()
                        .unwrap_or(0);
                }
                track
            })
            .collect();
        let widths = solve(width, 1, &tracks);
        let console = cx.console();
        let line = |cells: &mut dyn Iterator<Item = &str>| -> Vec<Segment> {
            let mut out = Vec::new();
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
            let header = line(&mut self.columns.iter().map(|c| c.title.as_str()));
            canvas.lines_at(0, 0, width, 1, &[over(header, &header_style)]);
        }
        let highlight = if cx.focused() {
            cx.style("selected", "reverse")
        } else {
            cx.style(self.blur, "underline")
        };
        for (i, row) in rows.iter().enumerate() {
            let mut cells = row.iter().map(String::as_str);
            let mut segments = line(&mut cells);
            if first + i == selected {
                let used: usize = segments.iter().map(Segment::cell_length).sum();
                if used < width as usize {
                    segments.push(Segment::new(" ".repeat(width as usize - used), None));
                }
                segments = over(segments, &highlight);
            }
            canvas.lines_at(0, top + i as u16, width, 1, &[segments]);
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
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(_) if mouse.row >= self.header as u16 => {
                    let row = self.first.get() + (mouse.row - self.header as u16) as usize;
                    if row < self.rows.len() {
                        self.selected.set(row);
                    }
                }
                MouseKind::ScrollUp => self.step(-3),
                MouseKind::ScrollDown => self.step(3),
                _ => return Used::No,
            },
        }
        let _ = cx;
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
                canvas.print(x, 0, "│", Some(&divider));
                x += 1;
            }
            let w = markup_width(title) + 2;
            let line = cell_line(console, &format!(" {title} "), w);
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
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}
