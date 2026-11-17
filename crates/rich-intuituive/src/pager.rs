//! A pager strip: [`pager`], `‹ 2 of 5 ›` or a row of dots, that moves
//! a page signal over a list, a table or a carousel of your own.

use rich::Segment;
use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::Role;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// What a click on the strip does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Back,
    On,
    Page(usize),
}

/// A [`pager`]'s widget.
struct Pager {
    page: Signal<usize>,
    pages: Box<dyn Fn() -> usize>,
    dots: bool,
    hits: Vec<(u16, u16, Hit)>,
}

/// A strip that shows which page of `pages` (read again whenever a signal
/// it reads changes) `page` is on, from 0: `‹ 2 of 5 ›`, or with
/// [`pager_dots`](Node::pager_dots) a dot a page, `○ ● ○ ○ ○`. With the
/// focus, ← and → (PageUp and PageDown too) go back and on a page, Home
/// and End to the ends; a click on `‹`, `›` or a dot goes there. The app
/// shows the page: the rows of a list or a table from `page * per_page`,
/// a [`switch`](crate::switch) of pages. The arrows are in the theme's
/// `pager.arrow` style (else `accent`), greyed (`pager.disabled`, dim) at
/// either end; the page in `pager.current` (bold) and the other dots in
/// `pager.dot` (dim). Its role is a slider, its value `page 2 of 5`.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::pager;
///
/// let app = App::new(|| {
///     let page = signal(0usize);
///     let rows: Vec<String> = (1..=7).map(|n| format!("row {n}")).collect();
///     column([
///         text(move || rows.iter().skip(page.get() * 3).take(3).cloned().collect::<Vec<_>>().join("\n")).fixed(3),
///         pager(page, || 3).fixed(1),
///     ])
/// });
/// let mut driver = app.driver(12, 4);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[3].trim_end(), "‹ 1 of 3 ›");
/// assert!(driver.click_text("›"));
/// assert!(driver.click_text("›"));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "row 7");
/// assert_eq!(driver.screen().plain()[3].trim_end(), "‹ 3 of 3 ›");
/// ```
pub fn pager(page: Signal<usize>, pages: impl Fn() -> usize + 'static) -> Node {
    widget(Pager {
        page,
        pages: Box::new(pages),
        dots: false,
        hits: Vec::new(),
    })
    .what("pager")
    .focusable()
    .auto()
}

impl Node {
    /// For a [`pager`]: a dot a page (`●` the one shown, `○` the others)
    /// in place of `‹ n of m ›`. Ignored on other nodes.
    pub fn pager_dots(self) -> Node {
        self.widget_mut(|pager: &mut Pager| pager.dots = true);
        self
    }
}

impl Pager {
    /// The strip's cells, and where each part of it is.
    fn line(&self, cx: &crate::widget::DrawCx) -> (Vec<Segment>, Vec<(u16, u16, Hit)>) {
        let pages = (self.pages)().max(1);
        let page = self.page.get().min(pages - 1);
        let arrow = cx.style("pager.arrow", "accent");
        let grey = cx.style("pager.disabled", "dim");
        let current = cx.style("pager.current", "bold");
        let dot = cx.style("pager.dot", "dim");
        let mut line = Vec::new();
        let mut hits = Vec::new();
        let mut push = |line: &mut Vec<Segment>, text: String, style, hit: Option<Hit>| {
            let start: usize = line.iter().map(Segment::cell_length).sum();
            let w = rich::cells::cell_len(&text);
            line.push(Segment::new(text, Some(style)));
            if let Some(hit) = hit {
                hits.push((start as u16, (start + w) as u16, hit));
            }
        };
        if self.dots {
            for i in 0..pages {
                if i > 0 {
                    push(&mut line, " ".into(), dot.clone(), None);
                }
                let (mark, style) = if i == page {
                    ("●", current.clone())
                } else {
                    ("○", dot.clone())
                };
                push(&mut line, mark.into(), style, Some(Hit::Page(i)));
            }
        } else {
            let back = if page > 0 {
                arrow.clone()
            } else {
                grey.clone()
            };
            let on = if page + 1 < pages { arrow } else { grey };
            push(&mut line, "‹".into(), back, Some(Hit::Back));
            push(
                &mut line,
                format!(" {} of {pages} ", page + 1),
                current,
                None,
            );
            push(&mut line, "›".into(), on, Some(Hit::On));
        }
        (line, hits)
    }

    fn go(&self, to: usize) {
        let last = (self.pages)().max(1) - 1;
        self.page.set(to.min(last));
    }
}

impl Widget for Pager {
    fn name(&self) -> &'static str {
        "pager"
    }

    fn role(&self) -> Role {
        Role::Slider
    }

    fn access_value(&self) -> Option<String> {
        let pages = (self.pages)().max(1);
        let page = self.page.get_untracked().min(pages - 1);
        Some(format!("page {} of {pages}", page + 1))
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        let pages = (self.pages)().max(1);
        let wanted = if self.dots {
            pages * 2 - 1
        } else {
            4 + format!("{pages} of {pages}").len()
        };
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal if width > 0 => (wanted.min(u16::MAX as usize) as u16).min(width),
            Axis::Horizontal => wanted.min(u16::MAX as usize) as u16,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (line, hits) = self.line(cx);
        canvas.lines(&[line]);
        if cx.focused()
            && let Some(&(start, end, _)) = hits.iter().find(|(_, _, hit)| match hit {
                Hit::Page(i) => *i == self.page.get(),
                _ => false,
            })
        {
            canvas.restyle(
                start,
                0,
                end - start,
                1,
                &cx.style("pager.focus", "reverse"),
            );
        }
        self.hits = hits;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let page = self.page.get_untracked();
        let last = (self.pages)().max(1) - 1;
        match event {
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => {
                match key.code {
                    KeyCode::Left | KeyCode::PageUp if page > 0 => self.go(page - 1),
                    KeyCode::Right | KeyCode::PageDown if page < last => self.go(page + 1),
                    KeyCode::Home => self.go(0),
                    KeyCode::End => self.go(last),
                    _ => return Used::No,
                }
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left) | MouseKind::Up(MouseButton::Left)
                ) =>
            {
                let Some(&(_, _, hit)) = self
                    .hits
                    .iter()
                    .find(|(start, end, _)| (*start..*end).contains(&mouse.column))
                else {
                    return Used::No;
                };
                if mouse.kind == MouseKind::Up(MouseButton::Left) {
                    match hit {
                        Hit::Back => self.go(page.saturating_sub(1)),
                        Hit::On => self.go(page + 1),
                        Hit::Page(i) => self.go(i),
                    }
                }
                Used::Yes
            }
            _ => Used::No,
        }
    }
}
