//! Putting a list in order (#720): move the focused item up and down with
//! the keyboard or drag it with the mouse, then Enter returns the items in
//! their new order.
//!
//! Keys (context `reorder`, rebindable): ↑/↓ (or `k`/`j`) move the focus;
//! Alt+↑/↓, Ctrl+↑/↓ and `K`/`J` move the focused item one place, and
//! Alt+Home/End (or Ctrl+Home/End) to the top or the bottom. Alt and Ctrl
//! with an arrow need a terminal that reports modifiers on arrows (xterm's
//! encoding, which most do, or the kitty keyboard protocol); `K`/`J` and
//! grabbing work in any terminal: Space grabs the focused item, the arrows,
//! PgUp/PgDn and Home/End then carry it, Space drops it and Esc puts it
//! back where it was grabbed. Ctrl+R restores the first order.
//!
//! With the mouse on, a press on a row focuses it, and dragging carries it:
//! the list moves under the pointer as it goes, and scrolls when the
//! pointer passes the top or the bottom row.
//!
//! Without a terminal it lists the items numbered and reads the new order
//! as numbers (`3 1 2`, or `3, 1`): those come first, in that order, and
//! the rest keep theirs. An empty line keeps the order.

use std::cell::Cell;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, question, text, Theme};
use crate::event::{Button, Event, Key, MouseKind};
use crate::item::Item;
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};

/// The keys of a [`Reorder`], in context `reorder`.
pub fn reorder_keymap() -> Keymap {
    Keymap::new("reorder")
        .bind("pick", keys("enter"), "done: return the new order")
        .bind(
            "cancel",
            keys("escape"),
            "cancel (while carrying: put it back)",
        )
        .bind("up", keys("up k ctrl+p"), "focus up")
        .bind("down", keys("down j ctrl+n"), "focus down")
        .bind("page-up", keys("pageup"), "page up")
        .bind("page-down", keys("pagedown"), "page down")
        .bind("first", keys("home"), "first")
        .bind("last", keys("end"), "last")
        .bind("move-up", keys("alt+up ctrl+up K"), "move the item up")
        .bind(
            "move-down",
            keys("alt+down ctrl+down J"),
            "move the item down",
        )
        .bind(
            "move-first",
            keys("alt+home ctrl+home"),
            "move the item to the top",
        )
        .bind(
            "move-last",
            keys("alt+end ctrl+end"),
            "move the item to the bottom",
        )
        .bind("grab", keys("space"), "grab or drop the item")
        .bind("reset", keys("ctrl+r"), "restore the first order")
}

/// Move the element at `from` to `to`, shifting the ones between: how a
/// [`Reorder`] applies each move, for a caller keeping its own copy.
///
/// ```
/// let mut items = vec!["a", "b", "c", "d"];
/// rich_interact::components::move_item(&mut items, 0, 2);
/// assert_eq!(items, ["b", "c", "a", "d"]);
/// ```
pub fn move_item<T>(items: &mut [T], from: usize, to: usize) {
    if from < to {
        items[from..=to].rotate_left(1);
    } else if to < from {
        items[to..=from].rotate_right(1);
    }
}

/// A list put in order by the user. Returns the items' values in the new
/// order; [`order`](Self::order) has it as the items' first indices.
///
/// ```
/// use rich_interact::headless::{self, Script};
/// use rich_interact::{Outcome, Reorder};
///
/// let reorder = Reorder::new("Priority", ["alpha", "beta", "gamma"]);
/// // Focus gamma, carry it to the top, done.
/// let script = Script::new().keys("down down alt+up alt+up enter");
/// let (outcome, _) = headless::run(reorder, script, 40, 8);
/// assert_eq!(outcome.unwrap(), Outcome::Done(vec!["gamma", "alpha", "beta"]));
/// ```
pub struct Reorder<T> {
    prompt: String,
    items: Vec<Item<T>>,
    /// The items' first indices, in the current order.
    order: Vec<usize>,
    cursor: usize,
    offset: usize,
    height: usize,
    theme: Theme,
    mouse: bool,
    /// Grabbed with Space: where the item was when it was grabbed.
    grabbed: Option<usize>,
    /// A drag with the mouse in progress.
    dragging: bool,
    keymap: Keymap,
    /// Once finished: the answer shown in place of the list (`None`
    /// inside means cancelled).
    answer: Option<Option<String>>,
    /// Rows of items on screen, from the last render.
    page: Cell<usize>,
}

impl<T> Reorder<T> {
    pub fn new<I, V>(prompt: impl Into<String>, items: I) -> Reorder<T>
    where
        I: IntoIterator<Item = V>,
        V: Into<Item<T>>,
    {
        let items: Vec<Item<T>> = items.into_iter().map(Into::into).collect();
        Reorder {
            prompt: prompt.into(),
            order: (0..items.len()).collect(),
            items,
            cursor: 0,
            offset: 0,
            height: 10,
            theme: Theme::default(),
            mouse: false,
            grabbed: None,
            dragging: false,
            keymap: reorder_keymap(),
            answer: None,
            page: Cell::new(10),
        }
    }

    /// Show at most `rows` items at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Focus this position first.
    pub fn focus(mut self, position: usize) -> Self {
        self.cursor = position.min(self.items.len().saturating_sub(1));
        self
    }

    /// Report the mouse (#476): a press focuses a row and a drag carries
    /// it.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    /// Make `keys` do `action` (see [`reorder_keymap`]) on this list only;
    /// no keys unbinds it.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    pub fn items(&self) -> &[Item<T>] {
        &self.items
    }

    /// The order so far, as the items' indices when the list was made:
    /// `[2, 0, 1]` once the third item was moved to the top.
    pub fn order(&self) -> &[usize] {
        &self.order
    }

    /// The focused position.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether an item is grabbed (Space) and moves with the arrows.
    pub fn is_grabbed(&self) -> bool {
        self.grabbed.is_some()
    }

    fn follow(&mut self) {
        self.offset = self.offset_for(self.page.get());
    }

    /// The first position shown on a page of `rows`: the offset as the
    /// last event left it, kept on the focused row and the page full in
    /// case the terminal changed size since (or `focus` moved it before
    /// the first paint). Drawing and the mouse both use it.
    fn offset_for(&self, rows: usize) -> usize {
        let rows = rows.max(1);
        let offset = self.offset.min(self.order.len().saturating_sub(rows));
        if self.cursor < offset {
            self.cursor
        } else if self.cursor >= offset + rows {
            self.cursor + 1 - rows
        } else {
            offset
        }
    }

    /// Move the focus to `position`, clamped to the list.
    fn focus_on(&mut self, position: usize) {
        self.cursor = position.min(self.order.len().saturating_sub(1));
        self.follow();
    }

    /// Carry the focused item to `position`, clamped to the list.
    fn carry_to(&mut self, position: usize) {
        if self.order.is_empty() {
            return;
        }
        let to = position.min(self.order.len() - 1);
        move_item(&mut self.order, self.cursor, to);
        self.focus_on(to);
    }

    fn by(&self, delta: isize) -> usize {
        self.cursor.saturating_add_signed(delta)
    }

    fn finish(&mut self) -> Flow<Vec<usize>> {
        let labels: Vec<&str> = self
            .order
            .iter()
            .map(|&index| self.items[index].label.as_str())
            .collect();
        self.answer = Some(Some(labels.join(", ")));
        self.grabbed = None;
        Flow::Done(self.order.clone())
    }

    fn act(&mut self, action: &str) -> Flow<Vec<usize>> {
        let page = self.page.get().max(1) as isize;
        let carrying = self.grabbed.is_some();
        // While an item is grabbed, the moves carry it.
        let (target, carry) = match action {
            "up" => (self.by(-1), carrying),
            "down" => (self.by(1), carrying),
            "page-up" => (self.by(-page), carrying),
            "page-down" => (self.by(page), carrying),
            "first" => (0, carrying),
            "last" => (usize::MAX, carrying),
            "move-up" => (self.by(-1), true),
            "move-down" => (self.by(1), true),
            "move-first" => (0, true),
            "move-last" => (usize::MAX, true),
            "pick" => return self.finish(),
            "cancel" => {
                if let Some(from) = self.grabbed.take() {
                    self.carry_to(from);
                    return Flow::Continue;
                }
                self.answer = Some(None);
                return Flow::Cancel;
            }
            "grab" => {
                self.grabbed = match self.grabbed {
                    Some(_) => None,
                    None if !self.order.is_empty() => Some(self.cursor),
                    None => None,
                };
                return Flow::Continue;
            }
            "reset" => {
                // The focused item stays focused, wherever it lands.
                let focused = self.order.get(self.cursor).copied();
                self.order = (0..self.items.len()).collect();
                self.grabbed = None;
                if let Some(index) = focused {
                    self.focus_on(index);
                }
                return Flow::Continue;
            }
            _ => return Flow::Ignored,
        };
        if carry {
            self.carry_to(target);
        } else {
            self.focus_on(target);
        }
        Flow::Continue
    }

    fn mouse_event(&mut self, kind: MouseKind, row: usize) -> Flow<Vec<usize>> {
        let page = self.page.get().max(1);
        // The list starts under the question.
        let top = 1;
        let shown = self.order.len().saturating_sub(self.offset).min(page);
        match kind {
            // The wheel is ↑ and ↓: while an item is grabbed, it carries it.
            MouseKind::ScrollUp => return self.act("up"),
            MouseKind::ScrollDown => return self.act("down"),
            MouseKind::Down(Button::Left) if row >= top && row < top + shown => {
                self.grabbed = None;
                self.focus_on(self.offset + row - top);
                self.dragging = true;
            }
            MouseKind::Drag(Button::Left) if self.dragging => {
                // Past the top or the bottom row, the list scrolls a row at
                // a time under the item.
                let position = if row < top {
                    self.offset.saturating_sub(1)
                } else if row >= top + shown {
                    self.offset + shown
                } else {
                    self.offset + row - top
                };
                self.carry_to(position);
            }
            MouseKind::Up(_) => self.dragging = false,
            _ => return Flow::Ignored,
        }
        Flow::Continue
    }

    fn event(&mut self, event: &Event, context: &Context<'_>) -> Flow<Vec<usize>> {
        self.page.set(self.rows(context.height));
        // The rows as drawn, for a click to land on the one under it.
        self.follow();
        if let Some(mouse) = event.mouse() {
            return self.mouse_event(mouse.kind, mouse.row as usize);
        }
        let Some(key) = event.key() else {
            return Flow::Ignored;
        };
        match self.keymap.action(key).map(str::to_string) {
            Some(action) => self.act(&action),
            None => Flow::Ignored,
        }
    }

    /// Rows for the items on a terminal `height` rows tall: the question
    /// and the footer stay.
    fn rows(&self, height: usize) -> usize {
        self.height
            .min(self.order.len().max(1))
            .min(height.saturating_sub(2).max(1))
    }

    fn render_view(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        let mut header = question(theme, &self.prompt);
        if let Some(answer) = &self.answer {
            header.push(match answer {
                Some(answer) => text(answer.clone(), &theme.answer),
                None => text("cancelled", &theme.hint),
            });
            return View::new(vec![fit(header, width)]);
        }
        let rows = self.rows(context.height);
        self.page.set(rows);
        let mut lines = vec![fit(header, width)];
        let offset = self.offset_for(rows);
        let reverse = rich::Style::parse("reverse").expect("a built-in style");
        for (position, &index) in self.order.iter().enumerate().skip(offset).take(rows) {
            let item = &self.items[index];
            let focused = position == self.cursor;
            let carried = focused && (self.grabbed.is_some() || self.dragging);
            let mut line = if carried {
                // A mark of its own, so it shows without colour.
                vec![text("↕ ", &theme.pointer_style)]
            } else if focused {
                vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
            } else {
                vec![plain("  ")]
            };
            line.push(if carried {
                text(item.label.clone(), &theme.focused.combine(&reverse))
            } else if focused {
                text(item.label.clone(), &theme.focused)
            } else {
                plain(item.label.clone())
            });
            if let Some(description) = &item.description {
                line.push(text(format!("  {description}"), &theme.hint));
            }
            lines.push(fit(line, width));
        }
        if self.order.is_empty() {
            lines.push(vec![text("  nothing to order", &theme.hint)]);
        }
        let hint = match self.grabbed {
            Some(_) => {
                let label = &self.items[self.order[self.cursor]].label;
                format!(
                    "  moving {label} ({}/{}) · ↑↓ move · space drop · esc put back · enter done",
                    self.cursor + 1,
                    self.order.len()
                )
            }
            None => format!(
                "  {}/{} · ↑↓ focus · alt+↑↓ or K/J move · space grab · enter done · esc cancel",
                (self.cursor + 1).min(self.order.len()),
                self.order.len()
            ),
        };
        lines.push(fit(vec![text(hint, &theme.hint)], width));
        View::new(lines)
    }

    /// Ask without a terminal: the items numbered, then the new order as
    /// numbers.
    fn ask(&mut self, io: &mut dyn LineIo) -> Result<Option<Vec<usize>>, NotInteractive> {
        io.write(&format!("{}\n", self.prompt));
        for (number, &index) in self.order.iter().enumerate() {
            let item = &self.items[index];
            let mut row = format!("  {}) {}", number + 1, item.label);
            if let Some(description) = &item.description {
                row.push_str(&format!(" — {description}"));
            }
            io.write(&format!("{row}\n"));
        }
        io.write("New order, as numbers (the rest keep theirs; empty keeps it): ");
        // At the end of input, or on an empty line, the order stands.
        let Some(line) = io.read_line() else {
            return Ok(Some(self.order.clone()));
        };
        let order = parse_order(&line, self.order.len()).map_err(NotInteractive::Invalid)?;
        self.order = order.into_iter().map(|at| self.order[at]).collect();
        Ok(Some(self.order.clone()))
    }
}

/// Read a new order of `len` items from `line`: positions from 1,
/// separated by spaces or commas. Those named come first, in that order,
/// and the rest keep theirs. Returns positions from 0.
pub(crate) fn parse_order(line: &str, len: usize) -> Result<Vec<usize>, String> {
    let mut named = Vec::new();
    // Which positions are named, so a long list is read in one pass.
    let mut used = vec![false; len];
    for word in line
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|word| !word.is_empty())
    {
        let number = word
            .parse::<usize>()
            .ok()
            .filter(|number| (1..=len).contains(number))
            .ok_or_else(|| format!("{word:?} is not a number from 1 to {len}"))?;
        if std::mem::replace(&mut used[number - 1], true) {
            return Err(format!("{number} is given twice"));
        }
        named.push(number - 1);
    }
    named.extend((0..len).filter(|&at| !used[at]));
    Ok(named)
}

impl<T: Clone> Reorder<T> {
    fn values(&self, order: &[usize]) -> Vec<T> {
        order
            .iter()
            .map(|&index| self.items[index].value.clone())
            .collect()
    }
}

impl<T: Clone> Component for Reorder<T> {
    type Output = Vec<T>;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<Vec<T>> {
        match self.event(event, context) {
            Flow::Done(order) => Flow::Done(self.values(&order)),
            Flow::Cancel => Flow::Cancel,
            Flow::Ignored => Flow::Ignored,
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.render_view(context)
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    fn keymap(&self) -> Keymap {
        self.keymap.clone()
    }

    /// The order as it stands: without a terminal and asked for defaults,
    /// nothing moves.
    fn default_value(&self) -> Option<Vec<T>> {
        Some(self.values(&self.order))
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<Vec<T>>, NotInteractive> {
        Ok(self.ask(io)?.map(|order| self.values(&order)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_shift_the_items_between() {
        let mut items = [0, 1, 2, 3, 4];
        move_item(&mut items, 4, 1);
        assert_eq!(items, [0, 4, 1, 2, 3]);
        move_item(&mut items, 1, 4);
        assert_eq!(items, [0, 1, 2, 3, 4]);
        move_item(&mut items, 2, 2);
        assert_eq!(items, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn orders_parse_from_numbers() {
        assert_eq!(parse_order("3 1 2", 3), Ok(vec![2, 0, 1]));
        assert_eq!(parse_order("3,1", 4), Ok(vec![2, 0, 1, 3]));
        assert_eq!(parse_order("", 3), Ok(vec![0, 1, 2]));
        assert_eq!(parse_order(" 2 ,, ", 2), Ok(vec![1, 0]));
        assert!(parse_order("4", 3).is_err());
        assert!(parse_order("0", 3).is_err());
        assert!(parse_order("1 1", 3).is_err());
        assert!(parse_order("one", 3).is_err());
    }
}
