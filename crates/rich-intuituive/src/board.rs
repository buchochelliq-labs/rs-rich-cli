//! A board of columns whose cards move within and between them:
//! [`board`], a task board's or a review queue's lanes, each move handed
//! to the app as a [`BoardMove`] to make in its own data.

use rich::{Console, Segment, Style};
use rich_interact::{Button as MouseButton, Key, KeyCode, MouseKind};

use crate::a11y::Role;
use crate::app::Ctx;
use crate::node::{Axis, Node};
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// The narrowest a column is drawn; fewer columns show, the rest paged,
/// where the board is narrower than this a column.
const MIN_COLUMN: u16 = 18;

/// One move on a [`board`]: the card at index `from.1` of column `from.0`
/// now sits at index `to.1` of column `to.0` (in the same column, those
/// between shift a place towards where it was).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardMove {
    pub from: (usize, usize),
    pub to: (usize, usize),
}

impl BoardMove {
    /// Make the same move in `columns`, the app's own cards.
    ///
    /// ```
    /// use intuituive::BoardMove;
    ///
    /// let mut columns = vec![vec!["a", "b"], vec!["c"]];
    /// BoardMove { from: (0, 0), to: (1, 1) }.apply(&mut columns);
    /// assert_eq!(columns, [vec!["b"], vec!["c", "a"]]);
    /// ```
    pub fn apply<T>(&self, columns: &mut [Vec<T>]) {
        let ((fc, fi), (tc, ti)) = (self.from, self.to);
        if fc >= columns.len() || tc >= columns.len() || fi >= columns[fc].len() {
            return;
        }
        let card = columns[fc].remove(fi);
        let at = ti.min(columns[tc].len());
        columns[tc].insert(at, card);
    }
}

type OnMove = Box<dyn FnMut(BoardMove, &mut Ctx)>;

/// A [`board`]'s widget.
struct Board {
    columns: Box<dyn Fn() -> Vec<String>>,
    cards: Box<dyn Fn() -> Vec<Vec<String>>>,
    on_move: Option<OnMove>,
    /// The selected card: its column and index (the index may be one past
    /// the end of an empty column).
    at: (usize, usize),
    /// Where a grabbed card started, while it is carried.
    grabbed: Option<(usize, usize)>,
    /// A mouse drag under way.
    dragging: bool,
    /// The first card shown in each column, and the first column shown.
    top: Vec<usize>,
    first: usize,
    /// Where the last draw put each card: its column, index, and its
    /// first column, width, first row and height on the canvas; and each
    /// column's first cell and width.
    drawn: Vec<(usize, usize, u16, u16, u16, u16)>,
    lanes: Vec<(usize, u16, u16)>,
}

/// A board of `columns` (titles, plain text) side by side, each a stack
/// of its `cards` (console markup, a column's cards in order), both
/// closures that may read signals. Each card is a box round its text,
/// wrapped to the column's width; each column scrolls on its own to keep
/// the selected card in view, and where the board is narrower than 18
/// cells a column, fewer columns show and the rest are paged, `‹` and `›`
/// on the title row saying there are more.
///
/// With the focus ← and → (or `h` and `l`) go from column to column, ↑
/// and ↓ (`k`, `j`) from card to card. A card moves with Alt+← and Alt+→
/// (or `H`, `L`) to the next column, Alt+↑ and Alt+↓ (`K`, `J`) within
/// its column; Space grabs it, the arrows then carry it, Space drops it
/// and Esc puts it back where it was. With the mouse a press selects a
/// card and dragging carries it. Each move goes to
/// [`on_board_move`](Node::on_board_move) as a [`BoardMove`] for the app
/// to make in its own cards ([`BoardMove::apply`]); the selection follows
/// the card.
///
/// Titles are in the theme's `board.title` style (bold), cards' boxes in
/// `board.card` (dim), the selected one's in `board.selected` (`bold`,
/// and `reverse` while the board has the focus), a carried card marked
/// `↕`. Its role is a grid; its value says the selected card, its column
/// and place (`Doing, card 2 of 3: Review`), and each move is announced.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{board, BoardMove};
///
/// let app = App::new(|| {
///     let cards = signal(vec![vec!["Write".to_string(), "Test".into()], vec![]]);
///     board(|| vec!["Todo".into(), "Done".into()], move || cards.get())
///         .on_board_move(move |step: BoardMove, _| cards.update(|c| step.apply(c)))
/// });
/// let mut driver = app.driver(40, 8);
/// driver.press("alt+right");
/// let rows = driver.screen().plain();
/// assert_eq!(rows[0].trim_end(), "Todo (1)            Done (1)");
/// assert!(rows[2].contains("Test") && rows[2].contains("Write"));
/// ```
pub fn board(
    columns: impl Fn() -> Vec<String> + 'static,
    cards: impl Fn() -> Vec<Vec<String>> + 'static,
) -> Node {
    widget(Board {
        columns: Box::new(columns),
        cards: Box::new(cards),
        on_move: None,
        at: (0, 0),
        grabbed: None,
        dragging: false,
        top: Vec::new(),
        first: 0,
        drawn: Vec::new(),
        lanes: Vec::new(),
    })
    .what("board")
    .focusable()
}

impl Node {
    /// For a [`board`]: call `on_move` with each move of a card, for the
    /// app to make in its own cards ([`BoardMove::apply`]). Without it the
    /// cards do not move. Ignored on other nodes.
    pub fn on_board_move(self, on_move: impl FnMut(BoardMove, &mut Ctx) + 'static) -> Node {
        self.widget_mut(|board: &mut Board| board.on_move = Some(Box::new(on_move)));
        self
    }
}

/// A card's lines inside its box, `inner` cells wide.
fn card_lines(console: &Console, markup: &str, inner: u16) -> Vec<Vec<Segment>> {
    let text = rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup));
    let options = console.options().update_width(inner.max(1) as usize);
    let lines = console.render_lines(&text, &options, false);
    if lines.is_empty() {
        vec![Vec::new()]
    } else {
        lines
    }
}

fn plain(markup: &str) -> String {
    rich::Text::from_markup(markup)
        .map(|text| text.plain().to_string())
        .unwrap_or_else(|_| markup.to_string())
}

impl Board {
    /// The cards in each column, as many columns as there are titles.
    fn counts(&self) -> Vec<usize> {
        let titles = (self.columns)().len();
        let cards = (self.cards)();
        (0..titles)
            .map(|c| cards.get(c).map_or(0, Vec::len))
            .collect()
    }

    /// Keep the selection on a card (or the empty column's place).
    fn clamp(&mut self, counts: &[usize]) {
        if counts.is_empty() {
            self.at = (0, 0);
            return;
        }
        let column = self.at.0.min(counts.len() - 1);
        let index = self.at.1.min(counts[column].saturating_sub(1));
        self.at = (column, index);
    }

    /// Hand the move of the selected card to `to` to the app, and select
    /// it there.
    fn move_to(&mut self, cx: &mut EventCx, to: (usize, usize)) {
        let from = self.at;
        if to == from {
            return;
        }
        let Some(on_move) = &mut self.on_move else {
            return;
        };
        on_move(BoardMove { from, to }, cx.app());
        self.at = to;
        let titles = (self.columns)();
        let counts = self.counts();
        let n = counts.get(to.0).copied().unwrap_or(0);
        let title = titles.get(to.0).cloned().unwrap_or_default();
        cx.app()
            .announce(format!("Moved to {title}, {} of {n}", to.1 + 1), false);
    }

    /// Where the selected card goes on a step `(across, down)`, carried.
    fn step(&self, counts: &[usize], across: i32, down: i32) -> Option<(usize, usize)> {
        let (column, index) = self.at;
        if across != 0 {
            let to = column as i64 + across as i64;
            if to < 0 || to as usize >= counts.len() {
                return None;
            }
            let to = to as usize;
            // Into another column: at the same place, or at its end.
            return Some((to, index.min(counts[to])));
        }
        let len = counts.get(column).copied().unwrap_or(0);
        let to = index as i64 + down as i64;
        (to >= 0 && (to as usize) < len).then_some((column, to as usize))
    }

    fn key(&mut self, cx: &mut EventCx, key: Key) -> Used {
        let counts = self.counts();
        if counts.is_empty() {
            return Used::No;
        }
        self.clamp(&counts);
        let alt = key.modifiers.alt && !key.modifiers.ctrl;
        let carrying = self.grabbed.is_some();
        let has_card = counts[self.at.0] > 0;
        // A move: carried by the arrows, or Alt and an arrow, or H J K L.
        let step = match key.code {
            KeyCode::Left if alt || carrying => Some((-1, 0)),
            KeyCode::Right if alt || carrying => Some((1, 0)),
            KeyCode::Up if alt || carrying => Some((0, -1)),
            KeyCode::Down if alt || carrying => Some((0, 1)),
            KeyCode::Char('H') => Some((-1, 0)),
            KeyCode::Char('L') => Some((1, 0)),
            KeyCode::Char('K') => Some((0, -1)),
            KeyCode::Char('J') => Some((0, 1)),
            _ => None,
        };
        if let Some((across, down)) = step {
            if !has_card || self.on_move.is_none() {
                return Used::No;
            }
            if let Some(to) = self.step(&counts, across, down) {
                self.move_to(cx, to);
            }
            cx.redraw();
            return Used::Yes;
        }
        if key.modifiers.ctrl || key.modifiers.alt {
            return Used::No;
        }
        match key.code {
            KeyCode::Char(' ') if carrying => self.grabbed = None,
            KeyCode::Char(' ') if has_card && self.on_move.is_some() => {
                self.grabbed = Some(self.at);
                cx.app()
                    .announce("Grabbed; the arrows carry it, Space drops it", false);
            }
            KeyCode::Escape if carrying => {
                if let Some(origin) = self.grabbed.take() {
                    // Back where it was: into its column, at its place.
                    self.move_to(cx, origin);
                }
            }
            KeyCode::Left | KeyCode::Char('h') if self.at.0 > 0 => {
                self.at = (self.at.0 - 1, self.at.1);
                self.clamp(&counts);
            }
            KeyCode::Right | KeyCode::Char('l') if self.at.0 + 1 < counts.len() => {
                self.at = (self.at.0 + 1, self.at.1);
                self.clamp(&counts);
            }
            KeyCode::Up | KeyCode::Char('k') if self.at.1 > 0 => self.at.1 -= 1,
            KeyCode::Down | KeyCode::Char('j') if self.at.1 + 1 < counts[self.at.0] => {
                self.at.1 += 1;
            }
            KeyCode::Home => self.at.1 = 0,
            KeyCode::End => self.at.1 = counts[self.at.0].saturating_sub(1),
            _ => return Used::No,
        }
        cx.redraw();
        Used::Yes
    }

    /// The card, or the place in a column, under `column`, `row`.
    fn place_at(&self, column: u16, row: u16) -> Option<(usize, usize)> {
        if let Some(&(c, i, ..)) = self
            .drawn
            .iter()
            .find(|&&(_, _, x, w, y, h)| column >= x && column < x + w && row >= y && row < y + h)
        {
            return Some((c, i));
        }
        // Below a column's last card: its end.
        let &(c, ..) = self
            .lanes
            .iter()
            .find(|&&(_, x, w)| column >= x && column < x + w)?;
        let count = self.counts().get(c).copied().unwrap_or(0);
        Some((c, count))
    }

    fn mouse(&mut self, cx: &mut EventCx, kind: MouseKind, column: u16, row: u16) -> Used {
        match kind {
            MouseKind::Down(MouseButton::Left) => {
                let Some((c, i)) = self.place_at(column, row) else {
                    return Used::No;
                };
                let count = self.counts().get(c).copied().unwrap_or(0);
                self.at = (c, i.min(count.saturating_sub(1)));
                self.dragging = count > 0;
                cx.capture_mouse();
                cx.redraw();
                Used::Yes
            }
            MouseKind::Drag(MouseButton::Left) if self.dragging => {
                if let Some((c, i)) = self.place_at(column, row) {
                    let counts = self.counts();
                    // In its own column the last place is its last card.
                    let limit = if c == self.at.0 {
                        counts[c].saturating_sub(1)
                    } else {
                        counts.get(c).copied().unwrap_or(0)
                    };
                    self.move_to(cx, (c, i.min(limit)));
                    cx.redraw();
                }
                Used::Yes
            }
            MouseKind::Up(MouseButton::Left) if self.dragging => {
                self.dragging = false;
                Used::Yes
            }
            _ => Used::No,
        }
    }
}

impl Widget for Board {
    fn name(&self) -> &'static str {
        "board"
    }

    fn role(&self) -> Role {
        Role::Grid
    }

    fn access_text(&self) -> Option<String> {
        Some("Board".into())
    }

    fn access_value(&self) -> Option<String> {
        let titles = (self.columns)();
        let cards = (self.cards)();
        let (c, i) = self.at;
        let title = titles.get(c)?;
        let column = cards.get(c).map(Vec::as_slice).unwrap_or_default();
        Some(match column.get(i) {
            Some(card) => format!(
                "{title}, card {} of {}: {}",
                i + 1,
                column.len(),
                plain(card)
            ),
            None => format!("{title}, no cards"),
        })
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => width,
            Axis::Vertical => {
                // The title row and the tallest column's cards.
                let titles = (self.columns)().len().max(1) as u16;
                let shown = (width / MIN_COLUMN).clamp(1, titles);
                let lane = (width.saturating_sub(shown - 1) / shown).max(4);
                let cards = (self.cards)();
                let tallest = cards
                    .iter()
                    .map(|column| {
                        column
                            .iter()
                            .map(|card| card_lines(cx.console(), card, lane - 4).len() + 2)
                            .sum::<usize>()
                    })
                    .max()
                    .unwrap_or(0);
                (tallest + 1).min(u16::MAX as usize) as u16
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let titles = (self.columns)();
        let cards = (self.cards)();
        let counts: Vec<usize> = (0..titles.len())
            .map(|c| cards.get(c).map_or(0, Vec::len))
            .collect();
        self.clamp(&counts);
        let (width, height) = (canvas.width(), canvas.height());
        if titles.is_empty() || width == 0 || height == 0 {
            return;
        }
        let shown = ((width / MIN_COLUMN) as usize).clamp(1, titles.len());
        // Page so the selected column shows.
        if self.at.0 < self.first {
            self.first = self.at.0;
        }
        if self.at.0 >= self.first + shown {
            self.first = self.at.0 + 1 - shown;
        }
        self.first = self.first.min(titles.len() - shown);
        let lane = (width.saturating_sub(shown as u16 - 1) / shown as u16).max(4);
        let focused = cx.focused();
        let title_style = cx.style("board.title", "bold");
        let card_style = cx.style("board.card", "dim");
        let mut selected_style = cx.style("board.selected", "bold");
        if focused {
            selected_style = selected_style.combine(&Style::parse("reverse").unwrap_or_default());
        }
        let console = cx.console();
        self.top.resize(titles.len(), 0);
        self.drawn.clear();
        self.lanes.clear();
        for (slot, c) in (self.first..self.first + shown).enumerate() {
            let x = slot as u16 * (lane + 1);
            self.lanes.push((c, x, lane));
            // Room kept for `‹` before the first title and `›` after the
            // last, where columns are paged out of view.
            let before = u16::from(slot == 0 && self.first > 0) * 2;
            let after = u16::from(slot + 1 == shown && self.first + shown < titles.len()) * 2;
            let room = lane.saturating_sub(before + after) as usize;
            let title = format!("{} ({})", titles[c], counts[c]);
            canvas.print(
                x + before,
                0,
                &rich::cells::set_cell_size(&title, room),
                Some(&title_style),
            );
            let column = cards.get(c).map(Vec::as_slice).unwrap_or_default();
            let boxes: Vec<Vec<Vec<Segment>>> = column
                .iter()
                .map(|card| card_lines(console, card, lane.saturating_sub(4)))
                .collect();
            // Scroll the column to keep its selected card in view.
            let room = height.saturating_sub(1) as usize;
            let selected = (c == self.at.0).then_some(self.at.1);
            let top = &mut self.top[c];
            *top = (*top).min(column.len().saturating_sub(1));
            if let Some(selected) = selected {
                if selected < *top {
                    *top = selected;
                }
                while *top < selected
                    && boxes[*top..=selected]
                        .iter()
                        .map(|b| b.len() + 2)
                        .sum::<usize>()
                        > room
                {
                    *top += 1;
                }
            }
            let mut y = 1u16;
            for (i, lines) in boxes.iter().enumerate().skip(*top) {
                let h = lines.len() as u16 + 2;
                if y >= height {
                    break;
                }
                let is_selected = selected == Some(i);
                let style = if is_selected {
                    &selected_style
                } else {
                    &card_style
                };
                let carried = is_selected && self.grabbed.is_some();
                let inner = lane.saturating_sub(2) as usize;
                let top_edge = if carried {
                    format!("╭↕{}╮", "─".repeat(inner.saturating_sub(1)))
                } else {
                    format!("╭{}╮", "─".repeat(inner))
                };
                canvas.print(x, y, &top_edge, Some(style));
                for (k, line) in lines.iter().enumerate() {
                    let row = y + 1 + k as u16;
                    if row >= height {
                        break;
                    }
                    canvas.print(x, row, "│", Some(style));
                    canvas.lines_at(
                        x + 2,
                        row,
                        lane.saturating_sub(4),
                        1,
                        std::slice::from_ref(line),
                    );
                    canvas.print(x + lane - 1, row, "│", Some(style));
                }
                if y + h - 1 < height {
                    canvas.print(
                        x,
                        y + h - 1,
                        &format!("╰{}╯", "─".repeat(inner)),
                        Some(style),
                    );
                }
                self.drawn.push((c, i, x, lane, y, h));
                y = y.saturating_add(h);
            }
        }
        // More columns than show: `‹` and `›` on the title row.
        if self.first > 0 {
            canvas.print(0, 0, "‹", Some(&title_style));
        }
        if self.first + shown < titles.len() {
            canvas.print(width - 1, 0, "›", Some(&title_style));
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Key(key) => self.key(cx, *key),
            WidgetEvent::Mouse(mouse) => self.mouse(cx, mouse.kind, mouse.column, mouse.row),
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}
