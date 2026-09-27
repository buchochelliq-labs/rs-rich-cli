//! A scrollable window over rendered lines (#495).
//!
//! Components that show more than fits (a pager, a preview pane, a long
//! list) keep a [`Viewport`] and render its visible lines; the painter
//! repaints only the cells that changed, so scrolling one line rewrites what
//! moved, not the whole screen. A viewport is also a [`Component`] on its
//! own: a minimal pager that returns where it was left.

use rich::{Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::event::{Event, KeyCode, MouseKind};

/// Lines, and the first one shown.
#[derive(Clone, Debug, Default)]
pub struct Viewport {
    lines: Vec<Vec<Segment>>,
    offset: usize,
    /// Rows shown; `None` fills the context's height (less the status line
    /// when the viewport runs as a component).
    height: Option<usize>,
}

impl Viewport {
    pub fn new(lines: Vec<Vec<Segment>>) -> Viewport {
        Viewport {
            lines,
            offset: 0,
            height: None,
        }
    }

    /// Show `height` rows instead of filling the space.
    pub fn with_height(mut self, height: usize) -> Viewport {
        self.height = Some(height);
        self
    }

    /// Replace the lines, keeping the offset where it still fits.
    pub fn set_lines(&mut self, lines: Vec<Vec<Segment>>) {
        self.lines = lines;
        self.offset = self.offset.min(self.lines.len().saturating_sub(1));
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
        self.offset
    }

    /// Rows shown, given `available` rows of space.
    pub fn page(&self, available: usize) -> usize {
        self.height.unwrap_or(available).max(1)
    }

    fn last_offset(&self, page: usize) -> usize {
        self.lines.len().saturating_sub(page)
    }

    /// Scroll by `delta` lines (negative: up), within the lines.
    pub fn scroll(&mut self, delta: isize, page: usize) -> bool {
        let target = self
            .offset
            .saturating_add_signed(delta)
            .min(self.last_offset(page));
        let changed = target != self.offset;
        self.offset = target;
        changed
    }

    pub fn scroll_to(&mut self, line: usize, page: usize) -> bool {
        let target = line.min(self.last_offset(page));
        let changed = target != self.offset;
        self.offset = target;
        changed
    }

    /// Scroll just enough that `line` is shown.
    pub fn show(&mut self, line: usize, page: usize) -> bool {
        if line < self.offset {
            self.scroll_to(line, page)
        } else if line >= self.offset + page {
            self.scroll_to(line + 1 - page, page)
        } else {
            false
        }
    }

    /// Move with the usual keys and the mouse wheel: arrows and `j`/`k` by a
    /// line, PageUp/PageDown and Space by a page, Home/End and `g`/`G` to
    /// the ends. Returns whether the event was a scroll key (moved or not).
    pub fn handle_scroll(&mut self, event: &Event, page: usize) -> bool {
        let page_step = page.max(1) as isize;
        match event {
            Event::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => self.scroll(-1, page),
                    KeyCode::Down | KeyCode::Char('j') => self.scroll(1, page),
                    KeyCode::PageUp | KeyCode::Char('b') => self.scroll(-page_step, page),
                    KeyCode::PageDown | KeyCode::Char(' ') => self.scroll(page_step, page),
                    KeyCode::Home | KeyCode::Char('g') => self.scroll_to(0, page),
                    KeyCode::End | KeyCode::Char('G') => self.scroll_to(usize::MAX, page),
                    _ => return false,
                };
                true
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::ScrollUp => {
                    self.scroll(-3, page);
                    true
                }
                MouseKind::ScrollDown => {
                    self.scroll(3, page);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// The lines shown for a page of `page` rows.
    pub fn visible(&self, page: usize) -> &[Vec<Segment>] {
        let end = (self.offset + page).min(self.lines.len());
        &self.lines[self.offset.min(end)..end]
    }

    /// "lines 1–20 of 240", or "all 12 lines" when everything fits.
    pub fn status(&self, page: usize) -> String {
        let total = self.lines.len();
        if total <= page {
            return format!("all {total} lines");
        }
        let last = (self.offset + page).min(total);
        format!("lines {}–{last} of {total}", self.offset + 1)
    }
}

/// As a component: a pager over its lines with a status line. Enter
/// finishes with the offset reached; `q` and Escape cancel.
impl Component for Viewport {
    type Output = usize;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<usize> {
        let page = self.page(context.height.saturating_sub(1));
        if self.handle_scroll(event, page) {
            return Flow::Continue;
        }
        match event.key().map(|key| key.code) {
            Some(KeyCode::Enter) => Flow::Done(self.offset),
            Some(KeyCode::Escape | KeyCode::Char('q')) => Flow::Cancel,
            _ => Flow::Continue,
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

    fn default_value(&self) -> Option<usize> {
        Some(self.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Key;

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

    #[test]
    fn replacing_lines_keeps_a_valid_offset() {
        let mut viewport = Viewport::new(lines(50));
        viewport.scroll_to(40, 10);
        viewport.set_lines(lines(5));
        assert_eq!(viewport.offset(), 4);
        assert_eq!(viewport.visible(10).len(), 1);
    }
}
