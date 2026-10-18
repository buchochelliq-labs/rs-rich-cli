//! Scrollbars: the [`Scrollbar`] every scrolling widget draws as its own
//! chrome, and [`scrollbar`], a bar on its own for a view of your own.
//!
//! A bar keeps a column (or a row) of its widget for itself, on the side it
//! is given, while it is shown: by default only when the content does not
//! fit ([`Show::Auto`]). The thumb shows the part of the content in view;
//! with the mouse, a press on the track pages, a press on an arrow steps,
//! and dragging the thumb scrolls. Its styles are the theme's `scrollbar`
//! (the track), `scrollbar.thumb` and `scrollbar.arrow`.

use std::cell::Cell;

use rich::{Console, Segment, Style};
use rich_interact::{Button, Mouse, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::screen::Rect;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// The edge of its widget a [`Scrollbar`] sits on: [`Right`](Side::Right)
/// and [`Left`](Side::Left) scroll down, [`Bottom`](Side::Bottom) and
/// [`Top`](Side::Top) across.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Side {
    #[default]
    Right,
    Left,
    Bottom,
    Top,
}

impl Side {
    /// Whether a bar on this side runs down (it scrolls rows).
    pub fn is_vertical(self) -> bool {
        matches!(self, Side::Right | Side::Left)
    }
}

/// When a [`Scrollbar`] is shown, and keeps its column or row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Show {
    /// While the content does not fit.
    #[default]
    Auto,
    /// Always, a full thumb when everything fits.
    Always,
    /// Never: no column or row is kept.
    Never,
}

/// The four symbols a bar is drawn with: its track, its thumb, and the
/// arrows at its two ends (`""` for none).
///
/// ```text
/// ↑│┃┃│↓   begin, track, thumb, track, end
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScrollbarSymbols {
    pub track: &'static str,
    pub thumb: &'static str,
    pub begin: &'static str,
    pub end: &'static str,
}

impl ScrollbarSymbols {
    /// A thin line with a heavy thumb and no arrows: the default down
    /// ([`THIN_HORIZONTAL`](Self::THIN_HORIZONTAL) across).
    pub const THIN: ScrollbarSymbols = ScrollbarSymbols {
        track: "│",
        thumb: "┃",
        begin: "",
        end: "",
    };
    /// [`THIN`](Self::THIN) across: the default for a bar along the bottom
    /// or the top.
    pub const THIN_HORIZONTAL: ScrollbarSymbols = ScrollbarSymbols {
        track: "─",
        thumb: "━",
        begin: "",
        end: "",
    };
    /// A line with a full block for the thumb, and arrows.
    pub const VERTICAL: ScrollbarSymbols = ScrollbarSymbols {
        track: "│",
        thumb: "█",
        begin: "↑",
        end: "↓",
    };
    /// A double line with a full block, and triangles.
    pub const DOUBLE_VERTICAL: ScrollbarSymbols = ScrollbarSymbols {
        track: "║",
        thumb: "█",
        begin: "▲",
        end: "▼",
    };
    /// [`VERTICAL`](Self::VERTICAL) across.
    pub const HORIZONTAL: ScrollbarSymbols = ScrollbarSymbols {
        track: "─",
        thumb: "█",
        begin: "←",
        end: "→",
    };
    /// [`DOUBLE_VERTICAL`](Self::DOUBLE_VERTICAL) across.
    pub const DOUBLE_HORIZONTAL: ScrollbarSymbols = ScrollbarSymbols {
        track: "═",
        thumb: "█",
        begin: "◄",
        end: "►",
    };
}

/// How a scrollbar looks and when it shows: ratatui's `Scrollbar`, as an
/// option of the widget it scrolls (`.scrollbar(..)` on a [`scroll`],
/// [`list`], table, tree, text area or log), or on its own ([`scrollbar`]).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{Scrollbar, Show, Side};
///
/// let app = App::new(|| {
///     let items = || (1..=20).map(|n| format!("item {n}")).collect();
///     list(items, signal(0))
///         .scrollbar(Scrollbar::new().side(Side::Left).ends(Some("↑"), Some("↓")))
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 12, 4).unwrap();
/// assert!(screen[0].starts_with("↑item 1"), "{screen:?}");
/// assert!(screen[3].starts_with("↓item 4"), "{screen:?}");
/// ```
///
/// [`scroll`]: crate::scroll
/// [`list`]: crate::list
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scrollbar {
    pub(crate) side: Side,
    pub(crate) show: Show,
    symbols: Option<ScrollbarSymbols>,
    thumb: Option<String>,
    track: Option<Option<String>>,
    ends: Option<(Option<String>, Option<String>)>,
}

impl Scrollbar {
    /// A bar on the right, shown while the content does not fit, in the
    /// [`THIN`](ScrollbarSymbols::THIN) symbols.
    pub fn new() -> Scrollbar {
        Scrollbar::default()
    }

    /// The edge it sits on.
    pub fn side(mut self, side: Side) -> Scrollbar {
        self.side = side;
        self
    }

    /// When it shows.
    pub fn show(mut self, show: Show) -> Scrollbar {
        self.show = show;
        self
    }

    /// Draw it in `symbols`; [`thumb`](Self::thumb), [`track`](Self::track)
    /// and [`ends`](Self::ends) change single symbols over them.
    pub fn symbols(mut self, symbols: ScrollbarSymbols) -> Scrollbar {
        self.symbols = Some(symbols);
        self
    }

    /// The thumb's symbol.
    pub fn thumb(mut self, symbol: &str) -> Scrollbar {
        self.thumb = Some(symbol.to_string());
        self
    }

    /// The track's symbol; `None` leaves the track blank.
    pub fn track(mut self, symbol: Option<&str>) -> Scrollbar {
        self.track = Some(symbol.map(str::to_string));
        self
    }

    /// The arrows at its start and end, each `None` for none.
    pub fn ends(mut self, begin: Option<&str>, end: Option<&str>) -> Scrollbar {
        self.ends = Some((begin.map(str::to_string), end.map(str::to_string)));
        self
    }

    /// Whether it runs down, scrolling rows.
    pub fn is_vertical(&self) -> bool {
        self.side.is_vertical()
    }

    /// Whether it keeps its line for content `content` long with `view`
    /// of it in view.
    pub(crate) fn wanted(&self, content: usize, view: usize) -> bool {
        match self.show {
            Show::Never => false,
            Show::Always => true,
            Show::Auto => content > view,
        }
    }

    /// The symbols it draws: thumb, track (`None`: blank) and the two
    /// arrows.
    fn glyphs(&self) -> (String, Option<String>, Option<String>, Option<String>) {
        let set = self.symbols.unwrap_or(if self.is_vertical() {
            ScrollbarSymbols::THIN
        } else {
            ScrollbarSymbols::THIN_HORIZONTAL
        });
        let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let thumb = self.thumb.clone().unwrap_or_else(|| set.thumb.to_string());
        let track = match &self.track {
            Some(track) => track.clone(),
            None => some(set.track),
        };
        let (begin, end) = match &self.ends {
            Some((begin, end)) => (begin.clone(), end.clone()),
            None => (some(set.begin), some(set.end)),
        };
        (thumb, track, begin, end)
    }

    /// Cells the arrows take, at the start and the end.
    fn arrows(&self) -> (u16, u16) {
        let (_, _, begin, end) = self.glyphs();
        let cells = |s: &Option<String>| match s {
            Some(_) if self.is_vertical() => 1,
            Some(s) => rich::cells::cell_len(s).min(u16::MAX as usize) as u16,
            None => 0,
        };
        (cells(&begin), cells(&end))
    }

    /// Where the thumb is along a bar `length` cells long: the arrows
    /// before the track, the thumb's start in the track and its length,
    /// and the track's length.
    fn parts(&self, length: u16, position: usize, content: usize, view: usize) -> Parts {
        let (begin, end) = self.arrows();
        let track = length.saturating_sub(begin.saturating_add(end));
        let (start, size) = thumb(track, position, content, view);
        Parts {
            begin,
            track,
            start,
            size,
        }
    }

    /// The bar `length` cells long as one segment a cell.
    pub(crate) fn segments(
        &self,
        console: &Console,
        length: u16,
        position: usize,
        content: usize,
        view: usize,
    ) -> Vec<Segment> {
        let style = |name: &str, fallback: &str| {
            console
                .get_style(&rich::style::StyleType::Name(name.to_string()))
                .ok()
                .or_else(|| Style::parse(fallback).ok())
        };
        let track_style = style("scrollbar", "bright_black");
        let thumb_style = style("scrollbar.thumb", "white");
        let arrow_style = style("scrollbar.arrow", "bright_black");
        let (thumb, track, begin, end) = self.glyphs();
        let parts = self.parts(length, position, content, view);
        let mut out = Vec::with_capacity(length as usize);
        if let Some(begin) = begin.filter(|_| parts.begin > 0) {
            out.push(Segment::new(begin, arrow_style.clone()));
        }
        for at in 0..parts.track {
            if at >= parts.start && at < parts.start + parts.size {
                out.push(Segment::new(thumb.clone(), thumb_style.clone()));
            } else {
                match &track {
                    Some(track) => out.push(Segment::new(track.clone(), track_style.clone())),
                    None => out.push(Segment::new(" ", None)),
                }
            }
        }
        if let Some(end) = end.filter(|_| length > parts.begin + parts.track) {
            out.push(Segment::new(end, arrow_style));
        }
        out
    }

    /// Draw the bar on `canvas` in `rect` (a column for a bar down, a row
    /// across).
    pub(crate) fn paint(
        &self,
        console: &Console,
        canvas: &mut Canvas,
        rect: Rect,
        position: usize,
        content: usize,
        view: usize,
    ) {
        if rect.is_empty() {
            return;
        }
        if self.is_vertical() {
            let lines: Vec<Vec<Segment>> = self
                .segments(console, rect.height, position, content, view)
                .into_iter()
                .map(|segment| vec![segment])
                .collect();
            canvas.lines_at(rect.x, rect.y, rect.width, rect.height, &lines);
        } else {
            let line = self.segments(console, rect.width, position, content, view);
            canvas.lines_at(rect.x, rect.y, rect.width, rect.height, &[line]);
        }
    }

    /// What a press `along` cells into a bar `length` long does.
    fn hit(&self, along: u16, length: u16, position: usize, content: usize, view: usize) -> Hit {
        let parts = self.parts(length, position, content, view);
        if along < parts.begin {
            Hit::Step(-1)
        } else if along >= parts.begin + parts.track {
            Hit::Step(1)
        } else {
            let at = along - parts.begin;
            if at < parts.start {
                Hit::Page(-1)
            } else if at >= parts.start + parts.size {
                Hit::Page(1)
            } else {
                Hit::Thumb(at - parts.start)
            }
        }
    }

    /// The rectangle a bar on this side takes of a widget `width` x
    /// `height`, and what it leaves for the content.
    pub(crate) fn split(&self, width: u16, height: u16) -> (Rect, Rect) {
        let rest = |w: u16, h: u16| (w.saturating_sub(1), h.saturating_sub(1));
        let (w, h) = rest(width, height);
        match self.side {
            Side::Right => (
                Rect::new(0, 0, w, height),
                Rect::new(w, 0, 1.min(width), height),
            ),
            Side::Left => (
                Rect::new(1.min(width), 0, w, height),
                Rect::new(0, 0, 1.min(width), height),
            ),
            Side::Bottom => (
                Rect::new(0, 0, width, h),
                Rect::new(0, h, width, 1.min(height)),
            ),
            Side::Top => (
                Rect::new(0, 1.min(height), width, h),
                Rect::new(0, 0, width, 1.min(height)),
            ),
        }
    }
}

/// Where a bar's thumb is: the arrow before the track, the track's length,
/// and the thumb's start in it and its length.
struct Parts {
    begin: u16,
    track: u16,
    start: u16,
    size: u16,
}

/// What a press on a bar does: step a line, page, or grab the thumb this
/// many cells from its start.
enum Hit {
    Step(isize),
    Page(isize),
    Thumb(u16),
}

/// The thumb's start and length in a track `track` cells long, for content
/// `content` long with `view` of it in view from `position`. The thumb is
/// at least a cell, and at the far end when the view is.
fn thumb(track: u16, position: usize, content: usize, view: usize) -> (u16, u16) {
    if track == 0 {
        return (0, 0);
    }
    let (track_f, content_f) = (track as f64, content.max(1) as f64);
    let size = (track_f * view.min(content.max(1)) as f64 / content_f)
        .round()
        .clamp(1.0, track_f);
    let start = (position as f64 * track_f / content_f)
        .round()
        .min(track_f - size)
        .max(0.0);
    (start as u16, size as u16)
}

/// The position that puts the thumb's start `start` cells into a track
/// `track` long.
fn position_at(start: u16, track: u16, content: usize, view: usize) -> usize {
    if track == 0 {
        return 0;
    }
    let position = (start as f64 * content as f64 / track as f64).round() as usize;
    position.min(content.saturating_sub(view))
}

/// A widget's scrollbar: its look, if it has one, and a drag of its thumb
/// in progress.
#[derive(Default)]
pub(crate) struct Bar {
    pub look: Option<Scrollbar>,
    /// Where the thumb was grabbed, from its start, while it is dragged.
    grab: Cell<Option<u16>>,
}

impl Bar {
    pub(crate) fn new(look: Option<Scrollbar>) -> Bar {
        Bar {
            look,
            grab: Cell::new(None),
        }
    }

    /// A mouse event for the bar drawn in `rect` (the widget's own
    /// coordinates), scrolling content `content` long with `view` of it in
    /// view from `position`. `None` when it is not the bar's (not on it,
    /// and no drag of it in progress); else `Some` of where to scroll to,
    /// if anywhere.
    pub(crate) fn mouse(
        &self,
        cx: &mut EventCx,
        mouse: &Mouse,
        rect: Rect,
        position: usize,
        content: usize,
        view: usize,
    ) -> Option<Option<usize>> {
        let look = self.look.as_ref()?;
        let vertical = look.is_vertical();
        let (length, along) = if vertical {
            (rect.height, mouse.row as i32 - rect.y as i32)
        } else {
            (rect.width, mouse.column as i32 - rect.x as i32)
        };
        let along = along.clamp(0, length.saturating_sub(1) as i32) as u16;
        let last = content.saturating_sub(view);
        if let Some(grab) = self.grab.get() {
            return match mouse.kind {
                MouseKind::Drag(Button::Left) => {
                    let parts = look.parts(length, position, content, view);
                    let start = along.saturating_sub(parts.begin).saturating_sub(grab);
                    let start = start.min(parts.track.saturating_sub(parts.size));
                    Some(Some(position_at(start, parts.track, content, view)))
                }
                MouseKind::Up(_) => {
                    self.grab.set(None);
                    cx.release_mouse();
                    Some(None)
                }
                _ => Some(None),
            };
        }
        if rect.is_empty() || !rect.contains(mouse.column, mouse.row) {
            return None;
        }
        match mouse.kind {
            MouseKind::Down(Button::Left) => {
                let to = match look.hit(along, length, position, content, view) {
                    Hit::Step(by) => position.saturating_add_signed(by).min(last),
                    Hit::Page(by) => position
                        .saturating_add_signed(by * view.max(1) as isize)
                        .min(last),
                    Hit::Thumb(grab) => {
                        self.grab.set(Some(grab));
                        cx.capture_mouse();
                        return Some(None);
                    }
                };
                Some(Some(to))
            }
            // The wheel is the widget's.
            MouseKind::ScrollUp | MouseKind::ScrollDown => None,
            _ => Some(None),
        }
    }
}

/// A scrollbar of its own, for a view of your own: it shows `view` of
/// `len` from `position` (a `view` of 0: as many as the bar is long), and
/// the mouse sets `position`: a press on the track pages, on an arrow
/// steps, and dragging the thumb scrolls. `len` and `view` may read
/// signals. It is one cell across: put a bar down in a [`row`](crate::row)
/// beside its view, and a bar across in a [`column`](crate::column).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{scrollbar, Scrollbar, Show};
///
/// let app = App::new(|| {
///     let top = signal(0usize);
///     row([
///         text(move || format!("from line {}", top.get())),
///         scrollbar(top, || 100, || 10, Scrollbar::new().show(Show::Always)),
///     ])
///     .on_key("j", move |_| top.update(|t| *t += 10))
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["j", "q"], 16, 10).unwrap();
/// assert_eq!(screen[0], "from line 10   │");
/// assert_eq!(screen[1], "               ┃");
/// ```
pub fn scrollbar(
    position: Signal<usize>,
    len: impl Fn() -> usize + 'static,
    view: impl Fn() -> usize + 'static,
    bar: Scrollbar,
) -> Node {
    widget(Standalone {
        position,
        len: Box::new(len),
        view: Box::new(view),
        bar: Bar::new(Some(bar)),
        length: Cell::new(1),
    })
    .size(crate::Size::Auto)
}

struct Standalone {
    position: Signal<usize>,
    len: Box<dyn Fn() -> usize>,
    view: Box<dyn Fn() -> usize>,
    bar: Bar,
    /// The cells it was last drawn along.
    length: Cell<u16>,
}

impl Standalone {
    fn look(&self) -> &Scrollbar {
        self.bar.look.as_ref().expect("a standalone bar has a look")
    }

    /// The content's length and the view's, as of now.
    fn sizes(&self, length: u16) -> (usize, usize) {
        let view = match (self.view)() {
            0 => length as usize,
            view => view,
        };
        ((self.len)(), view)
    }
}

impl Widget for Standalone {
    fn name(&self) -> &'static str {
        "scrollbar"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        match (axis, self.look().is_vertical()) {
            (Axis::Horizontal, true) | (Axis::Vertical, false) => 1,
            (Axis::Horizontal, false) => width,
            (Axis::Vertical, true) => height.max(1),
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let look = self.look().clone();
        let (width, height) = (canvas.width(), canvas.height());
        let length = if look.is_vertical() { height } else { width };
        self.length.set(length);
        let (content, view) = self.sizes(length);
        let position = self.position.get().min(content.saturating_sub(view));
        if !look.wanted(content, view) {
            return;
        }
        let rect = if look.is_vertical() {
            Rect::new(0, 0, 1.min(width), height)
        } else {
            Rect::new(0, 0, width, 1.min(height))
        };
        look.paint(cx.console(), canvas, rect, position, content, view);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let WidgetEvent::Mouse(mouse) = event else {
            return Used::No;
        };
        let (width, height) = cx.size();
        let length = self.length.get();
        let (content, view) = self.sizes(length);
        let position = self
            .position
            .get_untracked()
            .min(content.saturating_sub(view));
        let rect = Rect::new(0, 0, width.max(1), height.max(1));
        let last = content.saturating_sub(view);
        let wheel = match mouse.kind {
            MouseKind::ScrollUp => Some(position.saturating_sub(3)),
            MouseKind::ScrollDown => Some(position.saturating_add(3).min(last)),
            _ => None,
        };
        let moved = match wheel {
            Some(to) => Some(Some(to)),
            None => self.bar.mouse(cx, mouse, rect, position, content, view),
        };
        match moved {
            Some(Some(to)) => {
                if to != position {
                    self.position.set(to);
                }
                Used::Yes
            }
            Some(None) => Used::Yes,
            None => Used::No,
        }
    }
}

impl Node {
    /// Give this widget a scrollbar drawn as `bar` says: a [`scroll`],
    /// [`scroll_x`] or [`scroll_both`] (a bar on the left or right is the
    /// one down, on the top or bottom the one across), a [`list`], a table
    /// or [`virtual_list`], a [`tree`], a [`text_area`] or a [`Log`]'s
    /// view. The bar keeps a column (or row) of the widget while it shows.
    /// Call it on the widget itself, before `.panel(..)` or anything else
    /// that wraps it; on any other node it does nothing.
    ///
    /// [`scroll`]: crate::scroll
    /// [`scroll_x`]: crate::scroll_x
    /// [`scroll_both`]: crate::scroll_both
    /// [`list`]: crate::list
    /// [`virtual_list`]: crate::widgets::virtual_list
    /// [`tree`]: crate::widgets::tree
    /// [`text_area`]: crate::text_area
    /// [`Log`]: crate::Log
    pub fn scrollbar(self, bar: Scrollbar) -> Node {
        let set = |slot: &mut Bar| *slot = Bar::new(Some(bar.clone()));
        let done = self
            .widget_mut(|view: &mut crate::builtin::ScrollView| view.set_bar(bar.clone()))
            .is_some()
            || crate::list::with_list(&self, |list| set(&mut list.bar)).is_some()
            || self
                .widget_mut(|table: &mut crate::widgets::Table| set(&mut table.bar))
                .is_some()
            || self
                .widget_mut(|tree: &mut crate::tree::Tree| set(&mut tree.bar))
                .is_some()
            || self
                .widget_mut(|area: &mut crate::text_area::TextArea| set(&mut area.bar))
                .is_some();
        if !done {
            self.widget_mut(|log: &mut crate::log::LogView| set(&mut log.bar));
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_matches_the_old_bar_without_arrows() {
        // Ten rows of a hundred, from row 50: the old arithmetic.
        assert_eq!(thumb(10, 50, 100, 10), (5, 1));
        assert_eq!(thumb(10, 0, 5, 10), (0, 10));
        assert_eq!(thumb(10, 90, 100, 10), (9, 1));
    }

    #[test]
    fn a_dragged_thumb_lands_where_it_shows() {
        for position in [0, 10, 45, 90] {
            let (start, _) = thumb(10, position, 100, 10);
            let back = position_at(start, 10, 100, 10);
            assert_eq!(thumb(10, back, 100, 10).0, start, "{position}");
        }
    }

    #[test]
    fn presses_step_page_and_grab() {
        let bar = Scrollbar::new().ends(Some("↑"), Some("↓"));
        // 12 cells: an arrow, ten of track, an arrow.
        assert!(matches!(bar.hit(0, 12, 0, 100, 10), Hit::Step(-1)));
        assert!(matches!(bar.hit(11, 12, 0, 100, 10), Hit::Step(1)));
        assert!(matches!(bar.hit(1, 12, 0, 100, 10), Hit::Thumb(0)));
        assert!(matches!(bar.hit(6, 12, 0, 100, 10), Hit::Page(1)));
        assert!(matches!(bar.hit(1, 12, 50, 100, 10), Hit::Page(-1)));
    }
}
