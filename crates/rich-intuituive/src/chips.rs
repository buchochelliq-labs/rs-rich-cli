//! Chips: a short label on a coloured ground, rs-rich-ext's
//! [`Badge`](rich_ext::badge::Badge) as a node ([`chip`], [`chip_of`]),
//! that may be removed with a `×`; and a [`tag_input`] that types tags
//! into a row of them.

use rich::{Console, Segment};
use rich_ext::badge::Badge;
use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::Role;
use crate::app::Ctx;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// A badge's cells, with a `×` before its closing pad when it can be
/// removed: the cells, and the column of the `×`.
fn chip_cells(console: &Console, badge: &Badge, removable: bool) -> (Vec<Segment>, Option<u16>) {
    let text = badge.to_text(console);
    let options = console.options().update_width(text.cell_len().max(1));
    let mut cells: Vec<Segment> = console
        .render_lines(&text, &options, false)
        .into_iter()
        .next()
        .unwrap_or_default();
    if !removable {
        return (cells, None);
    }
    let style = cells.last().and_then(|s| s.style.clone());
    let used: usize = cells.iter().map(Segment::cell_length).sum();
    // On a coloured chip the `×` goes inside the closing pad; on a plain
    // one (`[beta]`), after it.
    let inside = cells.last().is_some_and(|s| s.text.ends_with(' '));
    let at = if inside { used } else { used + 1 };
    if inside {
        cells.push(Segment::new("× ", style));
    } else {
        cells.push(Segment::new(" ×", style));
    }
    (cells, Some(at.min(u16::MAX as usize) as u16))
}

type Remove = Box<dyn FnMut(&mut Ctx)>;

/// A [`chip`]'s widget.
struct Chip {
    badge: Box<dyn Fn() -> Badge>,
    remove: Option<Remove>,
    /// Where the `×` was last drawn.
    close: Option<u16>,
}

/// A chip: `text` on the theme's `badge.label` style (black on white),
/// padded (` beta `), or `[beta]` where the console shows no colour, as
/// rs-rich-ext's [`Badge::label`] draws it. [`chip_of`] shows any badge
/// (a status, a key and value); [`on_remove`](Node::on_remove) adds a `×`
/// that removes it.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::chip;
///
/// let app = App::new(|| row([chip("beta"), chip("linux")]));
/// let mut driver = app.driver(16, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), " beta  linux");
/// ```
pub fn chip(text: impl Into<String>) -> Node {
    let text = text.into();
    chip_of(move || Badge::label(text.clone()))
}

/// A chip showing `badge`, read again whenever a signal it reads changes:
/// a status (`Badge::status(Status::Ok, "build")`), a key and value
/// (`Badge::meta("version", "1.2")`), a label in a style of its own
/// (`Badge::label("new").style("badge.info")`).
pub fn chip_of(badge: impl Fn() -> Badge + 'static) -> Node {
    let mut node = widget(Chip {
        badge: Box::new(badge),
        remove: None,
        close: None,
    })
    .what("chip");
    node.size = Size::Auto;
    node
}

impl Node {
    /// For a [`chip`]: draw a `×` in it that runs `remove` when it is
    /// clicked; the chip takes the focus, and Delete or Backspace runs it
    /// too. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::chip;
    ///
    /// let app = App::new(|| {
    ///     let shown = signal(true);
    ///     column([chip("beta").on_remove(move |_| shown.set(false)).shown_when(move || shown.get())])
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), " beta ×");
    /// assert!(driver.click_text("×"));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "");
    /// ```
    pub fn on_remove(self, remove: impl FnMut(&mut Ctx) + 'static) -> Node {
        let mut set = false;
        self.widget_mut(|chip: &mut Chip| {
            chip.remove = Some(Box::new(remove));
            set = true;
        });
        if set { self.focusable() } else { self }
    }
}

impl Widget for Chip {
    fn name(&self) -> &'static str {
        "chip"
    }

    fn access_text(&self) -> Option<String> {
        Some((self.badge)().plain())
    }

    fn role(&self) -> Role {
        if self.remove.is_some() {
            Role::Button
        } else {
            Role::Text
        }
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => {
                let (cells, _) = chip_cells(cx.console(), &(self.badge)(), self.remove.is_some());
                let wanted = cells.iter().map(Segment::cell_length).sum::<usize>();
                let wanted = wanted.min(u16::MAX as usize) as u16;
                if width == 0 {
                    wanted
                } else {
                    wanted.min(width)
                }
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (cells, close) = chip_cells(cx.console(), &(self.badge)(), self.remove.is_some());
        canvas.lines(&[cells]);
        if let (true, Some(x)) = (cx.focused(), close) {
            let style = cx.style("chip.focus", "reverse");
            canvas.restyle(x, 0, 1, 1, &style);
        }
        self.close = close;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let Some(remove) = &mut self.remove else {
            return Used::No;
        };
        match event {
            WidgetEvent::Mouse(mouse)
                if Some(mouse.column) == self.close
                    && matches!(
                        mouse.kind,
                        MouseKind::Down(MouseButton::Left) | MouseKind::Up(MouseButton::Left)
                    ) =>
            {
                if mouse.kind == MouseKind::Up(MouseButton::Left) {
                    remove(cx.app());
                }
                Used::Yes
            }
            WidgetEvent::Key(key)
                if matches!(key.code, KeyCode::Delete | KeyCode::Backspace)
                    && !key.modifiers.ctrl
                    && !key.modifiers.alt =>
            {
                remove(cx.app());
                Used::Yes
            }
            _ => Used::No,
        }
    }
}

/// A [`tag_input`]'s widget.
struct TagInput {
    tags: Signal<Vec<String>>,
    /// What is being typed: a signal, so it lays out again as it wraps.
    draft: Signal<String>,
    /// The caret in what is being typed, in chars.
    caret: usize,
    /// Where each tag's `×` was last drawn, by index: its column and row.
    closes: Vec<(usize, u16, u16)>,
}

/// One piece of a [`tag_input`]'s rows: a chip's cells, or what is being
/// typed (`None`).
struct Piece {
    cells: Option<(Vec<Segment>, Option<u16>)>,
    width: usize,
}

/// A box that types tags into `tags`: each tag a removable [`chip`], then
/// what is being typed. Enter, Tab or a comma adds what was typed (trimmed,
/// once: a tag already there is not added again); Backspace with nothing
/// typed removes the last tag, and a click on a chip's `×` removes it.
/// ← and →, Home and End move the caret in what is typed, Backspace and
/// Delete take the character before or after it.
///
/// The chips are in the theme's `badge.label` style, what is typed after
/// them with a block caret (the theme's `tag_input.caret`, `reverse`)
/// while it has the focus. The chips wrap onto more rows when they do not
/// fit its width; it is one row unless sized otherwise, and
/// [`grow(min, max)`](Node::grow) makes it as tall as its rows, from `min`
/// to `max`, as a [`text_area`](crate::text_area)'s. Where there are more
/// rows than room, the last ones are shown, with what is typed. Its role
/// is a text box, its value the tags.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::tag_input;
///
/// let app = App::new(|| {
///     let tags = signal(vec!["rust".to_string()]);
///     column([tag_input(tags), text(move || tags.get().join("+"))])
/// });
/// let mut driver = app.driver(30, 2);
/// driver.type_text("tui,");
/// driver.type_text("cli");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "rust+tui+cli");
/// assert_eq!(driver.screen().plain()[0].trim_end(), " rust ×   tui ×   cli ×");
/// driver.press("backspace");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "rust+tui");
/// ```
pub fn tag_input(tags: Signal<Vec<String>>) -> Node {
    widget(TagInput {
        tags,
        draft: crate::reactive::signal(String::new()),
        caret: 0,
        closes: Vec::new(),
    })
    .what("tag_input")
    .focusable()
    .fixed(1)
}

impl TagInput {
    /// Add what was typed as a tag.
    fn commit(&mut self) {
        let tag = self.draft.get_untracked().trim().to_string();
        self.draft.set(String::new());
        self.caret = 0;
        if tag.is_empty() {
            return;
        }
        self.tags.update(|tags| {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        });
    }

    /// The byte offset of the caret in what is typed.
    fn caret_byte(&self, draft: &str) -> usize {
        draft
            .char_indices()
            .nth(self.caret)
            .map_or(draft.len(), |(at, _)| at)
    }

    fn insert(&mut self, c: char) {
        let at = self.draft.with_untracked(|draft| self.caret_byte(draft));
        self.draft.update(|draft| draft.insert(at, c));
        self.caret += 1;
    }

    /// Take the character at the caret.
    fn remove(&mut self) {
        let at = self.draft.with_untracked(|draft| self.caret_byte(draft));
        self.draft.update(|draft| {
            draft.remove(at);
        });
    }

    /// The chips, then what is typed (with a cell for the caret after it).
    fn pieces(&self, console: &Console) -> Vec<Piece> {
        let (tags, draft) = (self.tags.get(), self.draft.get());
        let mut pieces: Vec<Piece> = tags
            .iter()
            .map(|tag| {
                let (cells, close) = chip_cells(console, &Badge::label(tag.clone()), true);
                let width = cells.iter().map(Segment::cell_length).sum();
                Piece {
                    cells: Some((cells, close)),
                    width,
                }
            })
            .collect();
        pieces.push(Piece {
            cells: None,
            width: rich::cells::cell_len(&draft) + 1,
        });
        pieces
    }
}

/// Where each piece goes, as its row and column: on the row it starts
/// unless it does not fit after what is there, a cell between pieces.
fn wrap_pieces(pieces: &[Piece], width: u16) -> Vec<(u16, usize)> {
    let width = (width as usize).max(1);
    let mut out = Vec::with_capacity(pieces.len());
    let (mut row, mut x) = (0u16, 0usize);
    for piece in pieces {
        let at = if x == 0 { 0 } else { x + 1 };
        if x > 0 && at + piece.width > width {
            row = row.saturating_add(1);
            out.push((row, 0));
            x = piece.width;
        } else {
            out.push((row, at));
            x = at + piece.width;
        }
    }
    out
}

impl Widget for TagInput {
    fn name(&self) -> &'static str {
        "tag_input"
    }

    fn role(&self) -> Role {
        Role::TextBox
    }

    fn access_value(&self) -> Option<String> {
        let mut value = self.tags.get_untracked().join(", ");
        let draft = self.draft.get_untracked();
        if !draft.is_empty() {
            if !value.is_empty() {
                value.push_str(", ");
            }
            value.push_str(&draft);
        }
        Some(value)
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => {
                let pieces = self.pieces(cx.console());
                wrap_pieces(&pieces, width)
                    .last()
                    .map_or(1, |(row, _)| row.saturating_add(1))
            }
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let console = cx.console();
        let (width, height) = (canvas.width(), canvas.height().max(1));
        let pieces = self.pieces(console);
        let draft = self.draft.get_untracked();
        let places = wrap_pieces(&pieces, width);
        let rows = places.last().map_or(1, |(row, _)| row + 1);
        // More rows than room: the last ones, with what is typed.
        let skip = rows.saturating_sub(height);
        let mut lines: Vec<Vec<Segment>> = vec![Vec::new(); (rows - skip) as usize];
        let mut used = vec![0usize; lines.len()];
        let mut closes = Vec::new();
        let mut caret = None;
        for (i, (piece, &(row, x))) in pieces.into_iter().zip(&places).enumerate() {
            let Some(y) = row.checked_sub(skip) else {
                continue;
            };
            let line = &mut lines[y as usize];
            if x > used[y as usize] {
                line.push(Segment::new(" ".repeat(x - used[y as usize]), None));
            }
            used[y as usize] = x + piece.width;
            match piece.cells {
                Some((cells, close)) => {
                    if let Some(close) = close {
                        let at = (x + close as usize).min(u16::MAX as usize) as u16;
                        closes.push((i, at, y));
                    }
                    line.extend(cells);
                }
                None => {
                    // Typed text wider than the row: the part round the caret.
                    let before: usize = draft.chars().take(self.caret).map(char_cells).sum();
                    let room = (width as usize).saturating_sub(x).max(1);
                    let shift = (before + 1).saturating_sub(room);
                    let mut skipped = 0;
                    let shown: String = draft
                        .chars()
                        .skip_while(|&c| {
                            let skip = skipped < shift;
                            if skip {
                                skipped += char_cells(c);
                            }
                            skip
                        })
                        .collect();
                    line.push(Segment::new(shown, None));
                    caret = Some(((x + before - skipped).min(u16::MAX as usize) as u16, y));
                }
            }
        }
        canvas.lines(&lines);
        if let (true, Some((x, y))) = (cx.focused(), caret) {
            let style = cx.style("tag_input.caret", "reverse");
            canvas.restyle(x, y, 1, 1, &style);
        }
        self.closes = closes;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left) | MouseKind::Up(MouseButton::Left)
                ) =>
            {
                let Some(&(i, _, _)) = self
                    .closes
                    .iter()
                    .find(|(_, x, y)| *x == mouse.column && *y == mouse.row)
                else {
                    return Used::No;
                };
                if mouse.kind == MouseKind::Up(MouseButton::Left) {
                    self.tags.update(|tags| {
                        if i < tags.len() {
                            tags.remove(i);
                        }
                    });
                }
                Used::Yes
            }
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => {
                let draft = self.draft.get_untracked();
                let chars = draft.chars().count();
                match key.code {
                    KeyCode::Enter | KeyCode::Char(',') => self.commit(),
                    KeyCode::Tab if !draft.trim().is_empty() => self.commit(),
                    KeyCode::Backspace if draft.is_empty() => {
                        if self.tags.get_untracked().is_empty() {
                            return Used::No;
                        }
                        self.tags.update(|tags| {
                            tags.pop();
                        });
                    }
                    KeyCode::Backspace => {
                        if self.caret > 0 {
                            self.caret -= 1;
                            self.remove();
                        }
                    }
                    KeyCode::Delete if self.caret < chars => self.remove(),
                    KeyCode::Left if self.caret > 0 => self.caret -= 1,
                    KeyCode::Right if self.caret < chars => self.caret += 1,
                    KeyCode::Home if !draft.is_empty() => self.caret = 0,
                    KeyCode::End if !draft.is_empty() => self.caret = chars,
                    KeyCode::Char(c) => self.insert(c),
                    _ => return Used::No,
                }
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Paste(text) => {
                for c in text.chars() {
                    if c == ',' || c == '\n' {
                        self.commit();
                    } else {
                        self.insert(c);
                    }
                }
                cx.redraw();
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

fn char_cells(c: char) -> usize {
    rich::cells::cell_len(c.encode_utf8(&mut [0; 4]))
}
