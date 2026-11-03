//! A paged strip of items side by side: [`carousel`]. A shelf of
//! products, a row of videos, photos one at a time.

use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::Role;
use crate::controls::look;
use crate::node::{Axis, Node};
use crate::reactive::{Runtime, Signal};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// The cells between two items.
const GAP: u16 = 1;

/// What [`carousel`] makes.
pub(crate) struct Carousel {
    len: Box<dyn Fn() -> usize>,
    first: Signal<usize>,
    item: Box<dyn Fn(usize) -> Node>,
    per_page: Option<usize>,
    item_width: u16,
    on_page: Option<Box<dyn FnMut(usize)>>,
    /// The items shown, by index, and their nodes and rectangles.
    keys: Vec<usize>,
    children: Vec<Node>,
    areas: Vec<Rect>,
    /// As last laid out: the first item shown, how many, of how many, and
    /// how many make a page.
    page: Page,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Page {
    first: usize,
    shown: usize,
    len: usize,
    per: usize,
}

impl Page {
    /// Where the previous page starts, if there is one.
    fn previous(self) -> Option<usize> {
        (self.first > 0).then(|| self.first.saturating_sub(self.per))
    }

    /// Where the next page starts: a page further, or as far as keeps the
    /// last page full.
    fn next(self) -> Option<usize> {
        let last = self.len.saturating_sub(self.per);
        let to = (self.first + self.per).min(last);
        (to > self.first).then_some(to)
    }

    /// The last page's start.
    fn last(self) -> usize {
        self.len.saturating_sub(self.per)
    }

    /// `1–4 of 12`, `3 of 12`, `0 of 0`.
    fn counter(self, dash: &str) -> String {
        match self.shown {
            0 => format!("0 of {}", self.len),
            1 => format!("{} of {}", self.first + 1, self.len),
            n => format!("{}{dash}{} of {}", self.first + 1, self.first + n, self.len),
        }
    }
}

/// A strip of `len()` items, a page at a time, side by side: the item at
/// index `i` is the node `item(i)` builds, built when its page is shown
/// (untracked: what it reads is its own) and kept while it stays shown.
/// `first` holds the first item shown: set it to page from the app (it is
/// kept within the items), read it to know the page. `‹` and `›` at the
/// strip's ends page back and on (dimmed at the ends of the items), and a
/// counter under it says what is shown, `1–4 of 12`.
///
/// A page is as many items as fit at [`item_width`](Node::item_width)
/// (16 cells by default) with a cell between them, so the strip follows
/// the room it has; [`per_page`](Node::per_page) fixes the count. The
/// items share the width; the last page is kept full (`9–12 of 12`, not
/// `11–12`).
///
/// `[` and `]` page back and on from anywhere inside it; with the strip
/// itself focused (it takes the focus), ← and → do too, PageUp and
/// PageDown, and Home and End go to the first and last pages. A click on
/// `‹` or `›` pages, and so does the wheel with Shift (the wheel alone
/// scrolls what the strip is in). When the focus was inside an item
/// paged away, it moves to the first item of the new page that takes it.
/// [`on_page`](Node::on_page) hears each page the user turns to.
///
/// The arrows are in the theme's `carousel.control` style (else
/// `accent`), dimmed to `muted` at an end; the counter is
/// `carousel.counter` (else `muted`). Its role is a
/// [`Region`](crate::a11y::Role::Region) (name it with
/// [`label`](Node::label)), its value the counter.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::carousel;
///
/// let app = App::new(|| {
///     let first = signal(0usize);
///     carousel(|| 12, first, |i| label(format!("item {}", i + 1)).panel(""))
///         .per_page(3)
///         .label("Shelf")
/// });
/// let mut driver = app.driver(40, 4);
/// driver.settle();
/// let rows = driver.screen().plain();
/// assert!(rows[1].contains("item 1") && rows[1].contains("item 3"));
/// assert!(rows[3].contains("1–3 of 12"));
/// driver.press("tab ]");
/// assert!(driver.screen().plain()[1].contains("item 4"));
/// assert_eq!(driver.accessibility()[0].describe(), "Shelf, region: 4–6 of 12");
/// ```
pub fn carousel(
    len: impl Fn() -> usize + 'static,
    first: Signal<usize>,
    item: impl Fn(usize) -> Node + 'static,
) -> Node {
    widget(Carousel {
        len: Box::new(len),
        first,
        item: Box::new(item),
        per_page: None,
        item_width: 16,
        on_page: None,
        keys: Vec::new(),
        children: Vec::new(),
        areas: Vec::new(),
        page: Page::default(),
    })
}

impl Carousel {
    /// How many items a page holds in `width` cells.
    fn per_page(&self, width: u16) -> usize {
        match self.per_page {
            Some(n) => n.max(1),
            None => ((width + GAP) / (self.item_width.max(1) + GAP)).max(1) as usize,
        }
    }

    /// Whether the strip draws its arrows: when it has room for them.
    fn arrows(width: u16) -> bool {
        width >= 6
    }

    /// Turn to the page starting at `to`; `by_key` when a key did it.
    fn turn(&mut self, cx: &mut EventCx, to: usize, by_key: bool) {
        if to == self.first.get_untracked() {
            return;
        }
        self.first.set(to);
        if let Some(on_page) = self.on_page.as_mut() {
            on_page(to);
        }
        // A key that reached the strip without its having the focus came
        // from inside an item, which may go: the focus goes into the new
        // page's first item, built now (the next layout keeps it).
        if by_key && !cx.focused() {
            let index = to.min(self.page.len.saturating_sub(1));
            let at = match self.keys.iter().position(|k| *k == index) {
                Some(at) => at,
                None => {
                    let node = Runtime::current().untracked(|| (self.item)(index));
                    self.keys.push(index);
                    self.children.push(node);
                    self.keys.len() - 1
                }
            };
            cx.app().focus_within(self.children[at].id());
        }
    }
}

impl Widget for Carousel {
    fn name(&self) -> &'static str {
        "carousel"
    }

    fn describe(&self) -> Option<String> {
        Some(self.page.counter("–"))
    }

    fn role(&self) -> Role {
        Role::Region
    }

    fn access_value(&self) -> Option<String> {
        Some(self.page.counter("–"))
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => width,
            Axis::Vertical => {
                let inner = if Self::arrows(width) {
                    width.saturating_sub(4)
                } else {
                    width
                };
                let per = self.per_page(inner) as u16;
                let slot = inner.saturating_sub(GAP * per.saturating_sub(1)) / per.max(1);
                let tallest = self
                    .children
                    .iter()
                    .map(|child| cx.measure(child, Axis::Vertical, slot.max(1), 0))
                    .max()
                    .unwrap_or(1);
                tallest.max(1) + 1
            }
        }
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let _ = cx;
        let len = (self.len)();
        let (inner_x, inner_w) = if Self::arrows(rect.width) {
            (rect.x + 2, rect.width.saturating_sub(4))
        } else {
            (rect.x, rect.width)
        };
        let items_h = if rect.height >= 2 {
            rect.height - 1
        } else {
            rect.height
        };
        let per = self.per_page(inner_w);
        let first = self.first.get().min(len.saturating_sub(1));
        let shown = len.saturating_sub(first).min(per);
        self.page = Page {
            first,
            shown,
            len,
            per,
        };
        let slots = per as u16;
        let room = inner_w.saturating_sub(GAP * slots.saturating_sub(1));
        let (slot, spare) = (room / slots.max(1), room % slots.max(1));
        let runtime = Runtime::current();
        let mut old: Vec<(usize, Node)> =
            self.keys.drain(..).zip(self.children.drain(..)).collect();
        self.areas.clear();
        let mut x = inner_x;
        for (n, index) in (first..first + shown).enumerate() {
            // The first items take a cell more each, until the spare is
            // used.
            let width = slot + u16::from((n as u16) < spare);
            let node = match old.iter().position(|(k, _)| *k == index) {
                Some(at) => old.swap_remove(at).1,
                None => runtime.untracked(|| (self.item)(index)),
            };
            self.keys.push(index);
            self.children.push(node);
            self.areas.push(Rect::new(x, rect.y, width, items_h));
            x += width + GAP;
        }
        for (_, gone) in old {
            gone.forget(&runtime);
        }
        self.areas.clone()
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        let text_mode = crate::a11y::text_mode();
        let on = look(cx, &["carousel.control", "accent"], "bold");
        let off = look(cx, &["muted"], "dim");
        let items_h = if height >= 2 { height - 1 } else { height };
        if Self::arrows(width) {
            let mid = items_h.saturating_sub(1) / 2;
            let (back, on_glyph) = if text_mode {
                ("<", ">")
            } else {
                ("‹", "›")
            };
            let page = self.page;
            let back_style = if page.previous().is_some() { &on } else { &off };
            let next_style = if page.next().is_some() { &on } else { &off };
            canvas.print(0, mid, back, Some(back_style));
            canvas.print(width - 1, mid, on_glyph, Some(next_style));
        }
        if height >= 2 {
            let counter = self.page.counter(if text_mode { "-" } else { "–" });
            let counter_w = rich::cells::cell_len(&counter) as u16;
            let style = look(cx, &["carousel.counter", "muted"], "dim");
            let x = width.saturating_sub(counter_w) / 2;
            canvas.print(x, height - 1, &counter, Some(&style));
        }
        if cx.focused() {
            let focus = cx.style("control.focus", "reverse");
            let at = width.saturating_sub(1);
            if Self::arrows(width) {
                let mid = items_h.saturating_sub(1) / 2;
                canvas.restyle(0, mid, 1, 1, &focus);
                canvas.restyle(at, mid, 1, 1, &focus);
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let page = self.page;
        let to = match event {
            WidgetEvent::Key(key) if key.modifiers.ctrl || key.modifiers.alt => return Used::No,
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Char('[') => page.previous(),
                KeyCode::Char(']') => page.next(),
                _ if !cx.focused() => return Used::No,
                KeyCode::Left | KeyCode::PageUp => page.previous(),
                KeyCode::Right | KeyCode::PageDown => page.next(),
                KeyCode::Home => Some(0),
                KeyCode::End => Some(page.last()),
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) => {
                let (width, height) = cx.size();
                let items_h = if height >= 2 { height - 1 } else { height };
                match mouse.kind {
                    MouseKind::Down(MouseButton::Left)
                        if Self::arrows(width) && mouse.row < items_h =>
                    {
                        if mouse.column == 0 {
                            page.previous()
                        } else if mouse.column + 1 == width {
                            page.next()
                        } else {
                            return Used::No;
                        }
                    }
                    MouseKind::ScrollUp if mouse.modifiers.shift => page.previous(),
                    MouseKind::ScrollDown if mouse.modifiers.shift => page.next(),
                    _ => return Used::No,
                }
            }
            _ => return Used::No,
        };
        if let Some(to) = to {
            let by_key = matches!(event, WidgetEvent::Key(_));
            self.turn(cx, to, by_key);
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// Options of a [`carousel`]. On any other node they do nothing.
impl Node {
    /// On a [`carousel`]: how many items a page holds, whatever its width
    /// (by default, as many as fit at [`item_width`](Node::item_width)).
    pub fn per_page(self, items: usize) -> Node {
        self.widget_mut(|strip: &mut Carousel| strip.per_page = Some(items.max(1)));
        self
    }

    /// On a [`carousel`]: the narrowest an item may be when it counts how
    /// many fit a page (16 cells by default).
    pub fn item_width(self, cells: u16) -> Node {
        self.widget_mut(|strip: &mut Carousel| strip.item_width = cells.max(1));
        self
    }

    /// On a [`carousel`]: run `on_page` with the first item's index each
    /// time the user turns a page (a key, a click, the wheel): to load
    /// what comes next, or to scroll something else along.
    pub fn on_page(self, on_page: impl FnMut(usize) + 'static) -> Node {
        self.widget_mut(|strip: &mut Carousel| strip.on_page = Some(Box::new(on_page)));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(first: usize, len: usize, per: usize) -> Page {
        Page {
            first,
            shown: len.saturating_sub(first).min(per),
            len,
            per,
        }
    }

    #[test]
    fn pages_turn_by_a_page_and_keep_the_last_one_full() {
        assert_eq!(page(0, 12, 4).next(), Some(4));
        assert_eq!(page(4, 10, 4).next(), Some(6));
        assert_eq!(page(6, 10, 4).next(), None);
        assert_eq!(page(0, 10, 4).previous(), None);
        assert_eq!(page(2, 10, 4).previous(), Some(0));
        assert_eq!(page(0, 3, 4).next(), None);
        assert_eq!(page(0, 12, 4).counter("–"), "1–4 of 12");
        assert_eq!(page(2, 12, 1).counter("–"), "3 of 12");
        assert_eq!(page(0, 0, 4).counter("–"), "0 of 0");
    }
}
