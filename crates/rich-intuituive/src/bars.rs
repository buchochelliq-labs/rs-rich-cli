//! Bars and rows an app's chrome is made of: a [`status_bar`] of
//! powerline segments, a [`breadcrumb`] trail, a footer of [`key_hints`],
//! a row of percentage [`cells`], and a [`live`] node that draws at most so
//! many times a second.

use std::time::Duration;

use rich::{Console, Renderable, Segment, Style};

use crate::layout::Size;
use crate::node::{leaf, row, Axis, Node};
use crate::reactive::{Runtime, Signal};
use crate::widget::{theme_style, Canvas, DrawCx, MeasureCx, Widget};

/// The glyphs between a [`status_bar`]'s segments.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Caps {
    /// None: the segments side by side (the default).
    #[default]
    None,
    /// Powerline's arrows (U+E0B0, U+E0B2), as vim-airline's and
    /// lualine's: no cap before the left group or after the right one.
    Powerline,
    /// Rounded ends (U+E0B6, U+E0B4), as Yazi's, which also round the
    /// outer ends of both groups. Both need a Nerd Font.
    Rounded,
}

impl Caps {
    /// The glyphs that open a segment (pointing left) and close one
    /// (pointing right); `None` for none.
    fn glyphs(self) -> Option<(&'static str, &'static str)> {
        match self {
            Caps::None => None,
            Caps::Powerline => Some(("\u{e0b2}", "\u{e0b0}")),
            Caps::Rounded => Some(("\u{e0b6}", "\u{e0b4}")),
        }
    }
}

/// One segment of a [`status_bar`]: markup (which may read signals) on a
/// style of its own. Made by [`seg`].
pub struct StatusSegment {
    markup: Box<dyn Fn() -> String>,
    style: Box<dyn Fn() -> String>,
}

/// A [`status_bar`] segment showing console `markup`, which may read
/// signals; with no [`style`](StatusSegment::style) it is plain text, with
/// no cap round it. Pad it yourself: `seg(|| " NOR ".into())`.
pub fn seg(markup: impl Fn() -> String + 'static) -> StatusSegment {
    StatusSegment {
        markup: Box::new(markup),
        style: Box::new(String::new),
    }
}

impl StatusSegment {
    /// Its style (or theme style name): its background is the colour its
    /// caps take.
    pub fn style(self, style: &str) -> StatusSegment {
        let style = style.to_string();
        self.style_with(move || style.clone())
    }

    /// A style that changes, read from `style` (which may read signals):
    /// Yazi's mode is blue, red in visual mode.
    pub fn style_with(mut self, style: impl Fn() -> String + 'static) -> StatusSegment {
        self.style = Box::new(style);
        self
    }
}

/// A [`status_bar`]'s widget.
struct StatusBar {
    left: Vec<StatusSegment>,
    right: Vec<StatusSegment>,
    caps: Caps,
}

/// A one-row status line of segments, `left` from the left edge and
/// `right` against the right one, each on its own style, with [`Caps`]
/// between them (set with [`Node::caps`]): a cap is drawn in the colour of
/// the segment it ends, on the background of the next, so the colours
/// flow into each other as Yazi's, lualine's and vim-airline's do. A
/// segment with no background gets no cap; an empty one is left out. When
/// the two groups do not fit, the left one is cut with `…`.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{seg, status_bar, Caps};
///
/// let app = App::new(|| {
///     let mode = signal("NOR");
///     status_bar(
///         [
///             seg(move || format!(" {} ", mode.get())).style("bold white on blue"),
///             seg(|| " 4K ".into()).style("blue on white"),
///             seg(|| " notes.txt".into()),
///         ],
///         [seg(|| " 1/12 ".into()).style("white on blue")],
///     )
///     .caps(Caps::Rounded)
/// });
/// let mut driver = app.driver(34, 1);
/// driver.settle();
/// assert_eq!(
///     driver.screen().plain()[0],
///     "\u{e0b6} NOR \u{e0b4} 4K \u{e0b4} notes.txt    \u{e0b6} 1/12 \u{e0b4}"
/// );
/// ```
pub fn status_bar(
    left: impl IntoIterator<Item = StatusSegment>,
    right: impl IntoIterator<Item = StatusSegment>,
) -> Node {
    let mut node = crate::widget::widget(StatusBar {
        left: left.into_iter().collect(),
        right: right.into_iter().collect(),
        caps: Caps::None,
    });
    node.size = Size::Fixed(1);
    node
}

impl Node {
    /// For a [`status_bar`]: the glyphs between its segments. Ignored on
    /// other nodes.
    pub fn caps(self, caps: Caps) -> Node {
        self.widget_mut(|bar: &mut StatusBar| bar.caps = caps);
        self
    }
}

/// A segment's markup and style, read for one draw.
struct Shown {
    markup: String,
    style: Style,
}

impl StatusBar {
    /// `segments` as shown now, the empty ones left out.
    fn read(console: &Console, segments: &[StatusSegment]) -> Vec<Shown> {
        segments
            .iter()
            .filter_map(|segment| {
                let markup = (segment.markup)();
                if markup.is_empty() {
                    return None;
                }
                let name = (segment.style)();
                let style = theme_style(console, &name, &name);
                Some(Shown { markup, style })
            })
            .collect()
    }

    /// A group's cells: each segment on its style, with the caps. The left
    /// group closes each segment; the right one opens each.
    fn group(&self, console: &Console, shown: &[Shown], left: bool) -> Vec<Segment> {
        let glyphs = self.caps.glyphs();
        let rounded = self.caps == Caps::Rounded;
        let bg = |i: usize| shown.get(i).and_then(|s| s.style.bgcolor().cloned());
        let cap = |glyph: &str, fg: Option<rich::Color>, bg: Option<rich::Color>| {
            Segment::new(glyph, Some(Style::from_color(fg, bg)))
        };
        let mut out = Vec::new();
        for (i, segment) in shown.iter().enumerate() {
            let own = bg(i);
            if let (Some((open, _)), Some(own)) = (glyphs, own.clone()) {
                // Before the left group (rounded), and before each segment
                // of the right group, on the one before it.
                if left && i == 0 && rounded {
                    out.push(cap(open, Some(own), None));
                } else if !left {
                    let before = if i == 0 { None } else { bg(i - 1) };
                    out.push(cap(open, Some(own), before));
                }
            }
            let width = crate::widget::markup_width(&segment.markup);
            let line = crate::widget::markup_line(console, &segment.markup, width);
            out.extend(line.into_iter().map(|piece| {
                let style = match &piece.style {
                    Some(own) => segment.style.combine(own),
                    None => segment.style.clone(),
                };
                Segment::new(piece.text, Some(style))
            }));
            if let (Some((_, close)), Some(own)) = (glyphs, own) {
                // After each segment of the left group, on the next; after
                // the right group (rounded).
                let last = i + 1 == shown.len();
                if left {
                    out.push(cap(close, Some(own), bg(i + 1)));
                } else if last && rounded {
                    out.push(cap(close, Some(own), None));
                }
            }
        }
        out
    }
}

/// `line` cut to `width` cells, with `…` where it was cut.
fn cut_end(line: Vec<Segment>, width: usize) -> Vec<Segment> {
    let used: usize = line.iter().map(Segment::cell_length).sum();
    if used <= width {
        return line;
    }
    if width == 0 {
        return Vec::new();
    }
    let mut out = Segment::adjust_line_length(&line, width - 1, None);
    let style = out.last().and_then(|s| s.style.clone());
    out.push(Segment::new("…", style));
    out
}

/// `line` cut to its last `width` cells, with `…` where it was cut, in
/// the style of the first cell kept.
pub(crate) fn cut_start(line: Vec<Segment>, width: usize) -> Vec<Segment> {
    let used: usize = line.iter().map(Segment::cell_length).sum();
    if used <= width {
        return line;
    }
    if width == 0 {
        return Vec::new();
    }
    let mut skip = used - (width - 1);
    let mut kept = Vec::new();
    for segment in line {
        if skip == 0 {
            kept.push(segment);
            continue;
        }
        let cells = segment.cell_length();
        if cells <= skip {
            skip -= cells;
            continue;
        }
        let mut text = String::new();
        let mut dropped = 0;
        for c in segment.text.chars() {
            if dropped < skip {
                dropped += rich::cells::cell_len(c.encode_utf8(&mut [0; 4]));
                continue;
            }
            text.push(c);
        }
        // Half of a wide character was dropped: a space keeps the width.
        let pad = " ".repeat(dropped - skip);
        skip = 0;
        kept.push(Segment::new(pad + &text, segment.style));
    }
    let style = kept.first().and_then(|s| s.style.clone());
    let mut out = vec![Segment::new("…", style)];
    out.extend(kept);
    out
}

fn cells_of(line: &[Segment]) -> usize {
    line.iter().map(Segment::cell_length).sum()
}

impl Widget for StatusBar {
    fn name(&self) -> &'static str {
        "status_bar"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => {
                let console = cx.console();
                let left = self.group(console, &StatusBar::read(console, &self.left), true);
                let right = self.group(console, &StatusBar::read(console, &self.right), false);
                let wanted = (cells_of(&left) + cells_of(&right)).min(u16::MAX as usize) as u16;
                if width == 0 {
                    wanted
                } else {
                    wanted.min(width)
                }
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let console = cx.console();
        let width = canvas.width() as usize;
        let left = self.group(console, &StatusBar::read(console, &self.left), true);
        let right = self.group(console, &StatusBar::read(console, &self.right), false);
        let right = cut_start(right, width);
        let room = width - cells_of(&right);
        let mut line = cut_end(left, room);
        let gap = room - cells_of(&line);
        if gap > 0 {
            line.push(Segment::new(" ".repeat(gap), None));
        }
        line.extend(right);
        canvas.lines(&[line]);
    }
}

/// A [`breadcrumb`]'s widget.
struct Breadcrumb {
    parts: Box<dyn Fn() -> Vec<String>>,
    separator: String,
    after: Option<Box<dyn Fn() -> String>>,
}

/// A trail of parts as a one-row header: the parts (where you are last)
/// joined by a separator (` › ` by default; [`Node::crumb_separator`]),
/// the last part in the theme's `breadcrumb.last` style (bold by
/// default), the others in `breadcrumb` and the separators in
/// `breadcrumb.separator`. When it does not fit, it loses its start, with
/// `…` where it was cut, so where you are stays in view. `parts` may read
/// signals; an empty part shows nothing, so a leading empty part puts a
/// separator first (a path's root). [`Node::crumb_suffix`] adds markup
/// after the trail that is never cut (a filter, a count). It knows
/// nothing of what the parts are: a file path is one with
/// `files::path_crumb` (the `files` feature), a tree's ancestors or a
/// wizard's steps are others.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::breadcrumb;
///
/// let shown = |width| {
///     let app = App::new(|| {
///         let at = signal(vec!["Settings".to_string(), "Display".into(), "Colours".into()]);
///         breadcrumb(move || at.get()).crumb_suffix(|| " (3)".into())
///     });
///     let mut driver = app.driver(width, 1);
///     driver.settle();
///     driver.screen().plain()[0].trim_end().to_string()
/// };
/// assert_eq!(shown(40), "Settings › Display › Colours (3)");
/// assert_eq!(shown(16), "…y › Colours (3)");
/// ```
pub fn breadcrumb(parts: impl Fn() -> Vec<String> + 'static) -> Node {
    let mut node = crate::widget::widget(Breadcrumb {
        parts: Box::new(parts),
        separator: " › ".into(),
        after: None,
    });
    node.size = Size::Fixed(1);
    node
}

impl Node {
    /// For a [`breadcrumb`]: the text between its parts (` › ` by
    /// default; `/` for a path). Ignored on other nodes.
    pub fn crumb_separator(self, separator: &str) -> Node {
        let separator = separator.to_string();
        self.widget_mut(|crumb: &mut Breadcrumb| crumb.separator = separator);
        self
    }

    /// For a [`breadcrumb`]: console markup after the trail (which may read
    /// signals), kept whole while the trail is cut. Ignored on other nodes.
    pub fn crumb_suffix(self, markup: impl Fn() -> String + 'static) -> Node {
        self.widget_mut(|crumb: &mut Breadcrumb| crumb.after = Some(Box::new(markup)));
        self
    }
}

impl Breadcrumb {
    fn line(&self, console: &Console) -> (Vec<Segment>, Vec<Segment>) {
        let parts = (self.parts)();
        let base = theme_style(console, "breadcrumb", "");
        let last = theme_style(console, "breadcrumb.last", "bold");
        let sep = theme_style(console, "breadcrumb.separator", "");
        let mut line = Vec::new();
        let count = parts.len();
        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                line.push(Segment::new(self.separator.clone(), Some(sep.clone())));
            }
            if part.is_empty() {
                continue;
            }
            let style = if i + 1 == count { &last } else { &base };
            line.push(Segment::new(part, Some(style.clone())));
        }
        let after = match &self.after {
            Some(after) => {
                let markup = after();
                let width = crate::widget::markup_width(&markup);
                crate::widget::markup_line(console, &markup, width)
            }
            None => Vec::new(),
        };
        (line, after)
    }
}

impl Widget for Breadcrumb {
    fn name(&self) -> &'static str {
        "breadcrumb"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => {
                let (line, after) = self.line(cx.console());
                let wanted = (cells_of(&line) + cells_of(&after)).min(u16::MAX as usize) as u16;
                if width == 0 {
                    wanted
                } else {
                    wanted.min(width)
                }
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let width = canvas.width() as usize;
        let (line, after) = self.line(cx.console());
        let after = cut_end(after, width);
        let mut line = cut_start(line, width - cells_of(&after));
        line.extend(after);
        canvas.lines(&[line]);
    }
}

/// A [`key_hints`]'s widget.
struct KeyHints {
    hints: Signal<Vec<(String, String)>>,
}

/// A footer of the keys that work now: each described binding
/// ([`bind`](Node::bind)) of the focused node and its ancestors, innermost
/// first, as `key description`, in the theme's `key_hints.key` (bold by
/// default) and `key_hints.desc` (dim) styles, `·` apart in
/// `key_hints.separator`. It follows the focus, and a pop-up's keys while
/// one is open. Hints that do not fit are left out, whole: Bubbles'
/// `help`, Textual's `Footer`.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::key_hints;
///
/// let app = App::new(|| {
///     column([
///         label("files").bind("d", "trash", |_| {}).autofocus(),
///         key_hints(),
///     ])
///     .bind("q", "quit", |cx| cx.quit())
/// });
/// let mut driver = app.driver(30, 2);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[1].trim_end(), "d trash · q quit");
/// ```
pub fn key_hints() -> Node {
    let runtime = Runtime::current_for("key_hints()");
    let hints = match runtime.hints.get() {
        Some(hints) => hints,
        None => {
            let hints = crate::reactive::signal(Vec::new()).untraced();
            runtime.hints.set(Some(hints));
            hints
        }
    };
    let mut node = crate::widget::widget(KeyHints { hints });
    node.size = Size::Fixed(1);
    node
}

impl Widget for KeyHints {
    fn name(&self) -> &'static str {
        "key_hints"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 1,
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let key = cx.style("key_hints.key", "bold");
        let desc = cx.style("key_hints.desc", "dim");
        let sep = cx.style("key_hints.separator", "dim");
        let width = canvas.width() as usize;
        let mut line: Vec<Segment> = Vec::new();
        let mut used = 0;
        self.hints.with(|hints| {
            for (keys, description) in hints {
                let between = if line.is_empty() { 0 } else { 3 };
                let cells = rich::cells::cell_len(keys) + 1 + rich::cells::cell_len(description);
                if used + between + cells > width {
                    break;
                }
                if between > 0 {
                    line.push(Segment::new(" · ", Some(sep.clone())));
                }
                line.push(Segment::new(keys.clone(), Some(key.clone())));
                line.push(Segment::new(" ", None));
                line.push(Segment::new(description.clone(), Some(desc.clone())));
                used += between + cells;
            }
        });
        canvas.lines(&[line]);
    }
}

/// A row of cells sized by percentages, as ratatui's one-row `Table` lays
/// out its header: each `(percent, node)` a share of the width, with
/// ratatui's solver ([`Solver::Ratatui`](crate::Solver::Ratatui)) and a
/// cell of space between them (ratatui's `column_spacing`; change it with
/// [`gap`](Node::gap)), one row tall. Text in a cell is cut at its edge,
/// with no `…`, as in [`cell`](crate::widgets::cell): scope-tui's header.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::cells;
///
/// let app = App::new(|| {
///     cells([
///         (50, label("[bold]oscilloscope::scope-tui")),
///         (25, text!("60fps")),
///         (25, label("|>")),
///     ])
/// });
/// let mut driver = app.driver(21, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "oscillosc 60fps |>");
/// ```
pub fn cells(cells: impl IntoIterator<Item = (u16, Node)>) -> Node {
    let children: Vec<Node> = cells
        .into_iter()
        .map(|(percent, node)| {
            node.no_wrap()
                .overflow(rich::Overflow::Crop)
                .percent(percent)
        })
        .collect();
    row(children)
        .solver(crate::solver::Solver::Ratatui)
        .gap(1)
        .fixed(1)
}

/// Rendered lines, from a console, a width and a height.
type Render = Box<dyn Fn(&Console, u16, u16) -> Vec<Vec<Segment>>>;

/// When a [`live`] node last drew, at what size, and the lines.
type Drawn = (Duration, (u16, u16), Vec<Vec<Segment>>);

/// A [`live`] node's widget.
struct Live {
    render: Render,
    /// The shortest time between two draws.
    every: Duration,
    /// When it last drew, at what size, and the lines.
    last: Option<Drawn>,
    /// A draw is due, waited for on the app's clock.
    waiting: bool,
    /// Bumped when the wait is over: the node reads it, so it draws again.
    wake: Signal<u64>,
}

/// A [`renderable`](crate::renderable) that draws at most `fps_cap` times
/// a second, on the app's clock: a plot fed by a fast source (a sound
/// card's buffers, a sensor) draws at the cap, and the changes between are
/// dropped, the next draw showing the newest. A change within the cap
/// waits for its time, so the last one is always shown. `make` may read
/// signals. A cap of 0 draws on every change, as a renderable does.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::live;
/// use std::time::Duration;
///
/// use std::cell::Cell;
/// use std::rc::Rc;
///
/// let made = Rc::new(Cell::new(0));
/// let count = made.clone();
/// let app = App::new(move || {
///     let sample = signal(0);
///     // A source writing a hundred times a second, drawn ten.
///     every(Duration::from_millis(10), move |_| sample.update(|s| *s += 1));
///     live(10, move || {
///         count.set(count.get() + 1);
///         format!("sample {}", sample.get())
///     })
/// });
/// let mut driver = app.driver(12, 1);
/// driver.advance(Duration::from_millis(1000));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "sample 100");
/// assert!(made.get() <= 12, "drawn {} times", made.get());
/// ```
pub fn live<R: Renderable + 'static>(fps_cap: u32, make: impl Fn() -> R + 'static) -> Node {
    let render = move |console: &Console, width: u16, height: u16| {
        let options = console.options().update_width(width.max(1) as usize);
        let options = if height == 0 {
            options
        } else {
            options.update_height(height as usize)
        };
        console.render_lines(&make(), &options, false)
    };
    if fps_cap == 0 {
        return leaf(render).what("live");
    }
    crate::widget::widget(Live {
        render: Box::new(render),
        every: Duration::from_secs(1) / fps_cap,
        last: None,
        waiting: false,
        wake: crate::reactive::signal(0u64).untraced(),
    })
}

impl Widget for Live {
    fn name(&self) -> &'static str {
        "live"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let lines = (self.render)(cx.console(), width.max(1), 0);
        match axis {
            Axis::Vertical => lines.len().min(u16::MAX as usize) as u16,
            Axis::Horizontal => {
                let widest = lines.iter().map(|l| cells_of(l)).max().unwrap_or(0) as u16;
                let _ = height;
                widest.min(width.max(1))
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let now = cx.now();
        // Read, so the end of a wait draws it again.
        self.wake.get();
        let size = (canvas.width(), canvas.height());
        let early = match &self.last {
            Some((at, drawn, _)) if *drawn == size => {
                let next = *at + self.every;
                (now < next).then_some(next - now)
            }
            _ => None,
        };
        match early {
            // Too soon: what it showed stays, and it draws at its time.
            // Until then it reads only the wake, so the changes between
            // are dropped.
            Some(wait) => {
                if !self.waiting {
                    self.waiting = true;
                    let wake = self.wake;
                    crate::app::after(wait, move |_| wake.update(|n| *n += 1));
                }
            }
            None => {
                let lines = (self.render)(cx.console(), size.0, size.1);
                self.last = Some((now, size, lines));
                self.waiting = false;
            }
        }
        if let Some((_, _, lines)) = &self.last {
            canvas.lines(lines);
        }
    }
}
