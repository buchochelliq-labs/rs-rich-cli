//! A scrollable window over rendered lines (#495).
//!
//! Components that show more than fits (a pager, a preview pane, a long
//! list) keep a [`Viewport`] and render its visible lines; the painter
//! repaints only the cells that changed, so scrolling one line rewrites what
//! moved, not the whole screen. A viewport is also a [`Component`] on its
//! own: a minimal pager that returns where it was left.

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
        }
    }

    /// Make `keys` do `action` (see [`viewport_keymap`]) when the viewport
    /// runs as a component. Installed overrides (`viewport.page-down = n`)
    /// apply too.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Viewport {
        self.keymap.rebind(action, keys);
        self
    }

    /// Show `height` rows instead of filling the space.
    pub fn with_height(mut self, height: usize) -> Viewport {
        self.height = Some(height);
        self
    }

    /// Replace the lines, keeping the offset where it still fits.
    pub fn set_lines(&mut self, lines: Vec<Vec<Segment>>) {
        self.scroll.set_len(lines.len());
        self.lines = lines;
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

    /// The first line shown.
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
        self.scroll.scroll(delta, page)
    }

    pub fn scroll_to(&mut self, line: usize, page: usize) -> bool {
        self.scroll.scroll_to(line, page)
    }

    /// Scroll just enough that `line` is shown.
    pub fn show(&mut self, line: usize, page: usize) -> bool {
        self.scroll.show(line, page)
    }

    /// Do one of [`ScrollState::keymap`]'s actions (`scroll-up`,
    /// `page-down`, ...), for a component that looks its keys up in a
    /// keymap. Returns whether `action` is a scroll action.
    pub fn act(&mut self, action: &str, page: usize) -> bool {
        self.scroll.act(action, page)
    }

    /// Move with the usual keys and the mouse wheel: arrows and `j`/`k` by a
    /// line, PageUp/PageDown and Space by a page, Home/End and `g`/`G` to
    /// the ends. Returns whether the event was a scroll key (moved or not).
    pub fn handle_scroll(&mut self, event: &Event, page: usize) -> bool {
        self.scroll.handle(event, page)
    }

    /// The lines shown for a page of `page` rows.
    pub fn visible(&self, page: usize) -> &[Vec<Segment>] {
        &self.lines[self.scroll.visible(page)]
    }

    /// "lines 1–20 of 240", or "all 12 lines" when everything fits.
    pub fn status(&self, page: usize) -> String {
        let total = self.lines.len();
        if total <= page {
            return format!("all {total} lines");
        }
        let offset = self.scroll.offset();
        let last = (offset + page).min(total);
        format!("lines {}–{last} of {total}", offset + 1)
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
        match self.keymap.action(key) {
            Some("done") => Flow::Done(self.offset()),
            Some("quit") => Flow::Cancel,
            Some(action) if self.scroll.act(action, page) => Flow::Continue,
            _ => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let page = self.page(context.height.saturating_sub(1));
        let mut lines = self.visible(page).to_vec();
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
