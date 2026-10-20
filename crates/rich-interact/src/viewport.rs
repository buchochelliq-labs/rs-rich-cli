//! A scrollable window over rendered lines (#495).
//!
//! Components that show more than fits (a pager, a preview pane, a long
//! list) keep a [`Viewport`] and render its visible lines; the painter
//! repaints only the cells that changed, so scrolling one line rewrites what
//! moved, not the whole screen. A viewport is also a [`Component`] on its
//! own: a minimal pager that returns where it was left.
//!
//! Since 0.0.7 its first lines can be sticky (#716): a table's header
//! stays on top while the rows under it scroll. On a page too short for
//! them all, the last that fit stay (a table's column names) and the ones
//! before scroll away with the rest.

use rich::{Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::event::{Event, Key};
use crate::keymap::{keys, Keymap};
use crate::kit::ScrollState;

/// The keys of a [`Viewport`] run as a component, in context `viewport`:
/// [`ScrollState::keymap`]'s and `done` and `quit`.
pub fn viewport_keymap() -> Keymap {
    let mut keymap = Keymap::new("viewport")
        .bind("done", keys("enter"), "finish here")
        .bind("quit", keys("q escape"), "quit");
    keymap.extend(ScrollState::keymap("viewport"));
    keymap
}

/// Lines, and the first one shown: a [`ScrollState`] over them.
#[derive(Clone, Debug)]
pub struct Viewport {
    lines: Vec<Vec<Segment>>,
    scroll: ScrollState,
    /// Rows shown; `None` fills the context's height (less the status line
    /// when the viewport runs as a component).
    height: Option<usize>,
    keymap: Keymap,
    /// The first lines, shown on every page above the lines scrolled to.
    sticky: usize,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport::new(Vec::new())
    }
}

impl Viewport {
    pub fn new(lines: Vec<Vec<Segment>>) -> Viewport {
        Viewport {
            scroll: ScrollState::new(lines.len()),
            lines,
            height: None,
            keymap: viewport_keymap(),
            sticky: 0,
        }
    }

    /// Keep the first `rows` lines on every page, above the lines that
    /// scroll: a table's header (#716). The scroll position
    /// ([`offset`](Self::offset), [`scroll_to`](Self::scroll_to)) then
    /// counts the lines under them. A page always keeps one row to scroll:
    /// on one too short for every sticky line, the last that fit stay, and
    /// the ones before them scroll away first, so every line is shown.
    #[must_use]
    pub fn sticky(mut self, rows: usize) -> Viewport {
        self.set_sticky(rows);
        self
    }

    /// Change the sticky lines (see [`sticky`](Self::sticky)), keeping the
    /// offset where it still fits.
    pub fn set_sticky(&mut self, rows: usize) {
        self.sticky = rows;
        self.scroll.set_len(self.lines.len() - self.stuck());
    }

    /// The sticky lines asked for.
    pub fn sticky_rows(&self) -> usize {
        self.sticky
    }

    /// The sticky lines there are.
    fn stuck(&self) -> usize {
        self.sticky.min(self.lines.len())
    }

    /// The sticky lines that stay on a page of `page` rows, the last of
    /// them: one row at least is left to scroll.
    fn pinned(&self, page: usize) -> usize {
        self.stuck().min(page.saturating_sub(1))
    }

    /// The sticky lines that do not fit on a page of `page` rows: they
    /// scroll away first, before the others stay.
    fn unpinned(&self, page: usize) -> usize {
        self.stuck() - self.pinned(page)
    }

    /// The rows of a page of `page` that scroll.
    fn body(&self, page: usize) -> usize {
        page - self.pinned(page)
    }

    /// The scroll state for a page of `page` rows: its length is the lines
    /// less those that stay, so the last page ends with the last line.
    fn scroll_for(&mut self, page: usize) -> (&mut ScrollState, usize) {
        let (len, body) = (self.lines.len() - self.pinned(page), self.body(page));
        self.scroll.set_len(len);
        (&mut self.scroll, body)
    }

    /// Make `keys` do `action` (see [`viewport_keymap`]) when the viewport
    /// runs as a component. Installed overrides (`viewport.page-down = n`)
    /// apply too.
    #[must_use]
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Viewport {
        self.keymap.rebind(action, keys);
        self
    }

    /// Show `height` rows instead of filling the space.
    #[must_use]
    pub fn with_height(mut self, height: usize) -> Viewport {
        self.height = Some(height);
        self
    }

    /// Replace the lines, keeping the offset where it still fits.
    pub fn set_lines(&mut self, lines: Vec<Vec<Segment>>) {
        self.lines = lines;
        self.scroll.set_len(self.lines.len() - self.stuck());
    }

    pub fn lines(&self) -> &[Vec<Segment>] {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The first line shown: with [sticky](Self::sticky) lines, the
    /// number of lines scrolled past under them.
    pub fn offset(&self) -> usize {
        self.scroll.offset()
    }

    /// The scroll position.
    pub fn scroll_state(&self) -> &ScrollState {
        &self.scroll
    }

    /// Rows shown, given `available` rows of space.
    pub fn page(&self, available: usize) -> usize {
        self.height.unwrap_or(available).max(1)
    }

    /// Scroll by `delta` lines (negative: up), within the lines.
    pub fn scroll(&mut self, delta: isize, page: usize) -> bool {
        let (scroll, body) = self.scroll_for(page);
        scroll.scroll(delta, body)
    }

    /// Scroll to the [offset](Self::offset) `line`, or as near as the last
    /// page allows.
    pub fn scroll_to(&mut self, line: usize, page: usize) -> bool {
        let (scroll, body) = self.scroll_for(page);
        scroll.scroll_to(line, body)
    }

    /// Scroll just enough that `line` (counted from the first line, sticky
    /// lines included) is shown. A sticky line that stays needs no scroll.
    pub fn show(&mut self, line: usize, page: usize) -> bool {
        let (unpinned, pinned) = (self.unpinned(page), self.pinned(page));
        // The offsets that show it: from the one that puts it last on the
        // page, to the one that puts it first under the lines that stay
        // (a sticky line that scrolls away: first on the page).
        let first = (line + 1).saturating_sub(page);
        let last = if line < unpinned {
            line
        } else if line < self.stuck() {
            usize::MAX
        } else {
            line - pinned
        };
        let offset = self.offset();
        if offset < first {
            self.scroll_to(first, page)
        } else if offset > last {
            self.scroll_to(last, page)
        } else {
            false
        }
    }

    /// Do one of [`ScrollState::keymap`]'s actions (`scroll-up`,
    /// `page-down`, ...), for a component that looks its keys up in a
    /// keymap. Returns whether `action` is a scroll action.
    pub fn act(&mut self, action: &str, page: usize) -> bool {
        let (scroll, body) = self.scroll_for(page);
        scroll.act(action, body)
    }

    /// Move with the usual keys and the mouse wheel: arrows and `j`/`k` by a
    /// line, PageUp/PageDown and Space by a page, Home/End and `g`/`G` to
    /// the ends. Returns whether the event was a scroll key (moved or not).
    pub fn handle_scroll(&mut self, event: &Event, page: usize) -> bool {
        let (scroll, body) = self.scroll_for(page);
        scroll.handle(event, body)
    }

    /// Whether the sticky lines that fit stay on a page of `page` rows:
    /// once those that do not fit have scrolled away.
    fn stays(&self, page: usize) -> bool {
        self.offset() >= self.unpinned(page)
    }

    /// The index of the first line that scrolls on a page of `page` rows:
    /// the first shown under the sticky lines that stay, or the first shown
    /// when none stay yet.
    pub fn top(&self, page: usize) -> usize {
        let offset = self.offset();
        if self.stays(page) {
            offset + self.pinned(page)
        } else {
            offset
        }
    }

    /// The lines that scroll, as shown on a page of `page` rows: every
    /// line shown when none are [sticky](Self::sticky).
    pub fn visible(&self, page: usize) -> &[Vec<Segment>] {
        let end = (self.offset() + page).min(self.lines.len());
        &self.lines[self.top(page).min(end)..end]
    }

    /// The lines shown on a page of `page` rows, each with its index: the
    /// sticky lines that stay, then those scrolled to.
    pub fn rows(&self, page: usize) -> Vec<(usize, &[Segment])> {
        let staying = if self.stays(page) {
            self.unpinned(page)..self.stuck()
        } else {
            0..0
        };
        let end = (self.offset() + page).min(self.lines.len());
        staying
            .chain(self.top(page).min(end)..end)
            .map(|index| (index, self.lines[index].as_slice()))
            .collect()
    }

    /// "lines 1–20 of 240", or "all 12 lines" when everything fits. With
    /// sticky lines, the range is that of the lines under them, once
    /// scrolled.
    pub fn status(&self, page: usize) -> String {
        let total = self.lines.len();
        if total <= page {
            return format!("all {total} lines");
        }
        let first = match self.offset() {
            0 => 0,
            _ => self.top(page),
        };
        let last = (self.offset() + page).min(total);
        format!("lines {}–{last} of {total}", first + 1)
    }
}

/// As a component: a pager over its lines with a status line. Enter
/// finishes with the offset reached; `q` and Escape cancel. Its keys come
/// from its [keymap](viewport_keymap), so they can be rebound.
impl Component for Viewport {
    type Output = usize;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<usize> {
        let page = self.page(context.height.saturating_sub(1));
        let Some(key) = event.key() else {
            // The wheel.
            return if self.handle_scroll(event, page) {
                Flow::Continue
            } else {
                Flow::Ignored
            };
        };
        match self.keymap.action(key).map(str::to_string).as_deref() {
            Some("done") => Flow::Done(self.offset()),
            Some("quit") => Flow::Cancel,
            Some(action) if self.act(action, page) => Flow::Continue,
            _ => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let page = self.page(context.height.saturating_sub(1));
        let mut lines: Vec<Vec<Segment>> = self
            .rows(page)
            .into_iter()
            .map(|(_, line)| line.to_vec())
            .collect();
        let status = format!("{} · ↑↓ PgUp PgDn · q quits", self.status(page));
        lines.push(vec![Segment::new(
            status,
            Some(Style::parse("dim").expect("style")),
        )]);
        View::new(lines)
    }

    fn keymap(&self) -> Keymap {
        self.keymap.clone()
    }

    fn default_value(&self) -> Option<usize> {
        Some(self.offset())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Key, MouseKind};

    fn lines(n: usize) -> Vec<Vec<Segment>> {
        (1..=n)
            .map(|i| vec![Segment::new(format!("line {i}"), None)])
            .collect()
    }

    #[test]
    fn scrolls_within_bounds() {
        let mut viewport = Viewport::new(lines(10));
        assert!(!viewport.scroll(-1, 4));
        assert!(viewport.scroll(3, 4));
        assert_eq!(viewport.offset(), 3);
        assert!(viewport.scroll(100, 4));
        assert_eq!(viewport.offset(), 6, "the last page is full");
        assert_eq!(viewport.visible(4).len(), 4);
        assert_eq!(viewport.status(4), "lines 7–10 of 10");
        viewport.show(1, 4);
        assert_eq!(viewport.offset(), 1);
        viewport.show(8, 4);
        assert_eq!(viewport.offset(), 5);
        assert_eq!(Viewport::new(lines(3)).status(4), "all 3 lines");
    }

    #[test]
    fn keys_and_wheel_move_it() {
        let mut viewport = Viewport::new(lines(50));
        let key = |name: &str| Event::Key(Key::parse(name).unwrap());
        assert!(viewport.handle_scroll(&key("pagedown"), 10));
        assert_eq!(viewport.offset(), 10);
        assert!(viewport.handle_scroll(&key("j"), 10));
        assert_eq!(viewport.offset(), 11);
        assert!(viewport.handle_scroll(&key("G"), 10));
        assert_eq!(viewport.offset(), 40);
        assert!(viewport.handle_scroll(&key("home"), 10));
        assert_eq!(viewport.offset(), 0);
        assert!(!viewport.handle_scroll(&key("x"), 10));
        let wheel = Event::Mouse(crate::event::Mouse {
            kind: MouseKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: Default::default(),
        });
        assert!(viewport.handle_scroll(&wheel, 10));
        assert_eq!(viewport.offset(), 3);
    }

    /// Run as a component, a viewport's keys follow its keymap, so
    /// rebinding them works (0.0.14 release-test audit B).
    #[test]
    fn rebound_keys_scroll_it() {
        use crate::headless::{self, Script};

        let viewport = Viewport::new(lines(50)).rebind("scroll-down", [Key::char('n')]);
        let (outcome, _) = headless::run(viewport, Script::new().keys("n n j enter"), 20, 6);
        assert_eq!(outcome.unwrap(), crate::Outcome::Done(2));
        let viewport = Viewport::new(lines(50)).rebind("done", [Key::char('x')]);
        let (outcome, _) = headless::run(viewport, Script::new().keys("end x"), 20, 6);
        assert_eq!(outcome.unwrap(), crate::Outcome::Done(45));
    }

    #[test]
    fn replacing_lines_keeps_a_valid_offset() {
        let mut viewport = Viewport::new(lines(50));
        viewport.scroll_to(40, 10);
        viewport.set_lines(lines(5));
        assert_eq!(viewport.offset(), 4);
        assert_eq!(viewport.visible(10).len(), 1);
    }
}
