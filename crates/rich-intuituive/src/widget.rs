//! Widgets: what every node is made of, and how to write your own with
//! the same powers the built-in nodes have.
//!
//! A [`Widget`] sizes itself, draws cells on a [`Canvas`], takes keys,
//! mouse events and paste in its own coordinates, learns when it gains or
//! loses the focus or the pointer and when it is resized, holds the focus
//! and places the text caret, and may hold children: ordinary [`Node`]s it
//! lays out, or shows through a scrolling window. Make a node from one
//! with [`widget`]; it takes every builder (`.flex`, `.panel`, `.on_key`,
//! `.name`). Text, columns, grids, panels, lists, logs, components and
//! scrolls are all widgets built on this trait.
//!
//! A widget draws again when a [signal](crate::signal) it read while
//! drawing or laying out changes, when it moves or changes size, or when
//! an event handler asked for a redraw with [`EventCx::redraw`]. Its
//! [`layout`](Widget::layout) runs each time it draws, before
//! [`draw`](Widget::draw), so sizing signals belong in either.
//!
//! ```
//! use intuituive::prelude::*;
//! use intuituive::widget::{widget, Canvas, DrawCx, EventCx, MouseExt, Used, Widget, WidgetEvent};
//!
//! /// A counter that a click or `+` counts up.
//! struct Counter(u32);
//!
//! impl Widget for Counter {
//!     fn name(&self) -> &'static str {
//!         "counter"
//!     }
//!
//!     fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
//!         canvas.print(0, 0, &format!("count {}", self.0), None);
//!     }
//!
//!     fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
//!         match event {
//!             WidgetEvent::Key(key) if *key == intuituive::interact::Key::char('+') => {}
//!             WidgetEvent::Mouse(mouse) if mouse.is_press() => {}
//!             _ => return Used::No,
//!         }
//!         self.0 += 1;
//!         cx.redraw();
//!         Used::Yes
//!     }
//!
//!     fn focusable(&self) -> bool {
//!         true
//!     }
//! }
//!
//! let app = App::new(|| widget(Counter(0)).on_key("q", |cx| cx.quit()));
//! let screen = app.render_with(&["+", "+", "q"], 20, 1).unwrap();
//! assert_eq!(screen[0].trim_end(), "count 2");
//! ```

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use rich::{Console, Renderable, Segment, Style};
use rich_interact::{Key, Mouse, MouseKind};

use crate::app::{Ctx, Theme};
use crate::layout::{offsets, solve, Size};
use crate::node::{Axis, Node};
use crate::reactive::{NodeId, Signal};
use crate::screen::{Rect, Screen};

/// Whether a widget or handler used an event. One it did not use bubbles
/// to the node's ancestors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Used {
    Yes,
    No,
}

/// An event for a widget, in its own coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetEvent {
    /// A key, while the focus is on the widget or inside it; with nothing
    /// focused, while the pointer is over it.
    Key(Key),
    /// The mouse over the widget, or captured by it.
    Mouse(Mouse),
    /// Pasted text, while the focus is on the widget or inside it.
    Paste(String),
    /// The focus came to this widget (`true`) or left it (`false`).
    Focus(bool),
    /// The pointer came over this widget or a node inside it (`true`), or
    /// left (`false`).
    Hover(bool),
    /// It was laid out at a new size (after its first layout).
    Resize { width: u16, height: u16 },
}

/// A node's behaviour. Every method but [`draw`](Self::draw) has a default.
pub trait Widget: Any {
    /// What the inspector calls it (`"table"`, `"chart"`).
    fn name(&self) -> &'static str {
        "widget"
    }

    /// More for the inspector to show after the name (a panel's title, a
    /// list's length).
    fn describe(&self) -> Option<String> {
        None
    }

    /// Cells along `axis` the widget needs, given `width` x `height` (a
    /// height of 0: as tall as it likes). Called for a [`Size::Auto`]
    /// node, and by a modal sized to its content. Default: all it is
    /// given.
    ///
    /// [`Size::Auto`]: crate::Size::Auto
    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let _ = cx;
        match axis {
            Axis::Horizontal => width,
            Axis::Vertical => height,
        }
    }

    /// Its children, for a widget that holds nodes: the ones shown, in
    /// Tab order.
    fn children(&self) -> &[Node] {
        &[]
    }

    /// Nodes it keeps but does not show (pages of a [`switch`](crate::switch)
    /// waiting to be shown again): forgotten with the widget.
    fn hidden_children(&self) -> Vec<&Node> {
        Vec::new()
    }

    /// Where each of its children goes, in the order of
    /// [`children`](Self::children), given the widget's own `rect` (in
    /// screen coordinates; for a [`viewport`](Self::viewport), in the
    /// content's, from 0, 0). A child given no rectangle, or an empty one,
    /// is hidden. Runs each time the widget draws, before
    /// [`draw`](Self::draw); signals read here make it lay out again.
    /// Default: none.
    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let _ = (cx, rect);
        Vec::new()
    }

    /// Draw itself, not its children, on `canvas`: the widget's rectangle,
    /// in its own coordinates. Signals read here make it draw again when
    /// they change. The canvas is cleared first unless the widget is
    /// [`retained`](Self::retained) and only its own state changed.
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas);

    /// Whether the widget keeps what it drew between frames. A retained
    /// widget's canvas is not cleared when a signal it read changes (only
    /// when it moved, or what is under it was drawn over:
    /// [`DrawCx::repaint`] says which), it writes only what changed, and
    /// only those cells are sent and drawn over its children. A border that
    /// changes colour, a log that scrolls. Default: no.
    fn retained(&self) -> bool {
        false
    }

    /// Whether its children are shown through a window: laid out in
    /// content coordinates, drawn offscreen with their own damage, and
    /// scrolled by [`scroll`](Self::scroll). Clicks and the caret are
    /// translated through it. Default: no.
    fn viewport(&self) -> bool {
        false
    }

    /// For a [`viewport`](Self::viewport): where the window is, in the
    /// widget's own coordinates, and the point of the content at its top
    /// left. Called each frame after the children are laid out, before
    /// [`draw`](Self::draw).
    fn scroll(&mut self, cx: &ScrollCx) -> (Rect, (u16, u16)) {
        let _ = cx;
        (Rect::default(), (0, 0))
    }

    /// An event. One it does not use bubbles to its ancestors, whose
    /// [`on_key`](Node::on_key) bindings may take it. Focus, hover and
    /// resize events are told to the widget alone; what it returns for
    /// them does not matter.
    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let _ = (cx, event);
        Used::No
    }

    /// Whether Tab stops here (read once, when the node is made).
    fn focusable(&self) -> bool {
        false
    }

    /// Where the text caret goes while it has the focus, in its own
    /// coordinates.
    fn caret(&self) -> Option<(u16, u16)> {
        None
    }
}

/// A node with `widget`'s behaviour.
pub fn widget(widget: impl Widget) -> Node {
    let focusable = widget.focusable();
    let name = widget.name();
    let node = Node::from_widget(Box::new(widget), name);
    if focusable {
        node.focusable()
    } else {
        node
    }
}

/// What a widget sees while it measures or lays out.
pub struct MeasureCx<'a> {
    pub(crate) console: &'a Console,
}

impl MeasureCx<'_> {
    /// The console the app renders with: its width, styles and theme.
    pub fn console(&self) -> &Console {
        self.console
    }

    /// How many cells along `axis` `child`'s content needs in `width` x
    /// `height` (a height of 0: as tall as it likes).
    pub fn measure(&self, child: &Node, axis: Axis, width: u16, height: u16) -> u16 {
        child.measure(self.console, axis, width, height)
    }

    /// The cells along `axis` `child` takes as a column or row sizes it:
    /// its [`Size`] (cells, a percentage of `width` or `height`, or its
    /// content for [`Size::Auto`] and [`Size::Flex`]), within its minimum
    /// and maximum.
    pub fn extent(&self, child: &Node, axis: Axis, width: u16, height: u16) -> u16 {
        child.extent(self.console, axis, width, height)
    }

    /// Lay `children` out along `axis` in `rect`, `gap` cells apart, as a
    /// [`column`](crate::column) or [`row`](crate::row) does: each by its
    /// [`Size`], the flexible ones sharing what is left.
    pub fn stack(&self, axis: Axis, gap: u16, children: &[Node], rect: Rect) -> Vec<Rect> {
        let tracks: Vec<_> = children
            .iter()
            .map(|child| {
                let mut track = child.track();
                if track.size == Size::Auto {
                    // Content is measured in the whole rectangle across the
                    // axis, and unbounded along it.
                    track.content = match axis {
                        Axis::Vertical => self.measure(child, Axis::Vertical, rect.width, 0),
                        Axis::Horizontal => {
                            self.measure(child, Axis::Horizontal, rect.width, rect.height)
                        }
                    };
                }
                track
            })
            .collect();
        let (start, total) = match axis {
            Axis::Vertical => (rect.y, rect.height),
            Axis::Horizontal => (rect.x, rect.width),
        };
        let sizes = solve(total, gap, &tracks);
        offsets(start, gap, &sizes)
            .into_iter()
            .zip(sizes)
            .map(|(at, n)| match axis {
                Axis::Vertical => Rect::new(rect.x, at, rect.width, n),
                Axis::Horizontal => Rect::new(at, rect.y, n, rect.height),
            })
            .collect()
    }
}

/// The nodes that asked for the pointer: told when it enters or leaves
/// them, or moves over them.
#[derive(Default)]
pub(crate) struct Watchers {
    pub hover: RefCell<HashSet<NodeId>>,
    pub pointer: RefCell<HashSet<NodeId>>,
}

/// What a widget sees while it draws.
pub struct DrawCx<'a> {
    pub(crate) console: &'a Console,
    pub(crate) theme: &'a Theme,
    pub(crate) id: NodeId,
    pub(crate) rect: Rect,
    pub(crate) repaint: bool,
    pub(crate) focus_path: Signal<Vec<NodeId>>,
    pub(crate) hover_path: Signal<Vec<NodeId>>,
    pub(crate) pointer: Option<(u16, u16)>,
    /// From the screen to the surface the widget draws on (they differ
    /// inside a viewport).
    pub(crate) shift: (i32, i32),
    pub(crate) wants_hover: &'a Cell<bool>,
    pub(crate) watchers: &'a Watchers,
}

impl DrawCx<'_> {
    /// The console the app renders with: render any rich renderable with
    /// it, and look up theme styles by name.
    pub fn console(&self) -> &Console {
        self.console
    }

    /// The app's theme.
    pub fn theme(&self) -> &Theme {
        self.theme
    }

    /// This widget's node.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Whether everything must be drawn, on a clear canvas: always for a
    /// widget that is not [retained](Widget::retained); for one that is,
    /// when it moved, was drawn over, drew for the first time or its
    /// children moved (the area is clear then, whoever cleared it).
    pub fn repaint(&self) -> bool {
        self.repaint
    }

    /// Whether this widget has the focus. Asking makes it draw again when
    /// that changes.
    pub fn focused(&self) -> bool {
        self.focus_path.with(|path| path.last() == Some(&self.id))
    }

    /// Whether the focus is on this widget or a node inside it.
    pub fn focus_within(&self) -> bool {
        self.focus_path.with(|path| path.contains(&self.id))
    }

    /// Whether the mouse pointer is over this widget (or a node inside
    /// it). Asking makes it draw again when that changes (and only then),
    /// and turns on the terminal's reports of pointer movement.
    pub fn hovered(&self) -> bool {
        self.wants_hover.set(true);
        self.watchers.hover.borrow_mut().insert(self.id);
        self.hover_path
            .with_untracked(|path| path.contains(&self.id))
    }

    /// Turn on the terminal's reports of pointer movement, so the widget
    /// gets [`MouseKind::Moved`] events over it, without drawing again
    /// when the pointer moves (it decides that itself, from the events).
    pub fn report_movement(&self) {
        self.wants_hover.set(true);
    }

    /// Where the mouse pointer is over this widget, in its own
    /// coordinates; `None` when it is elsewhere. Asking makes the widget
    /// draw again whenever the pointer moves over it or leaves it: for
    /// hover effects on a part of a widget (a cell, a divider).
    pub fn pointer(&self) -> Option<(u16, u16)> {
        self.wants_hover.set(true);
        self.watchers.pointer.borrow_mut().insert(self.id);
        let (x, y) = self.pointer?;
        let x = x as i32 + self.shift.0 - self.rect.x as i32;
        let y = y as i32 + self.shift.1 - self.rect.y as i32;
        (x >= 0 && y >= 0 && x < self.rect.width as i32 && y < self.rect.height as i32)
            .then_some((x as u16, y as u16))
    }

    /// A theme style by name (`"accent"`, `"selected"`), or `fallback`.
    pub fn style(&self, name: &str, fallback: &str) -> Style {
        theme_style(self.console, name, fallback)
    }
}

/// A theme style by name, or `fallback` parsed.
pub(crate) fn theme_style(console: &Console, name: &str, fallback: &str) -> Style {
    console
        .get_style(&rich::style::StyleType::Name(name.to_string()))
        .ok()
        .or_else(|| Style::parse(fallback).ok())
        .unwrap_or_default()
}

/// What a [viewport](Widget::viewport) sees when it decides where to
/// scroll.
pub struct ScrollCx {
    pub(crate) content: (u16, u16),
    pub(crate) focused: Option<(NodeId, Rect)>,
}

impl ScrollCx {
    /// The content's width and height: as far as its children reach.
    pub fn content(&self) -> (u16, u16) {
        self.content
    }

    /// The focused node, if it is inside the viewport, and where it is in
    /// the content: to scroll it into view.
    pub fn focused(&self) -> Option<(NodeId, Rect)> {
        self.focused
    }
}

/// What a widget can do while it handles an event: what any handler can
/// (through [`app`](Self::app)), and ask to draw again or to keep the
/// mouse.
pub struct EventCx<'a> {
    pub(crate) ctx: &'a mut Ctx,
    pub(crate) console: &'a Console,
    pub(crate) size: (u16, u16),
    pub(crate) rect: Rect,
    pub(crate) focused: bool,
    pub(crate) redraw: bool,
    pub(crate) capture: Option<bool>,
}

impl EventCx<'_> {
    /// Quit, open a screen or a modal, move the focus, set the theme.
    pub fn app(&mut self) -> &mut Ctx {
        self.ctx
    }

    /// The console the app renders with.
    pub fn console(&self) -> &Console {
        self.console
    }

    /// Draw again: the widget's own state changed (state in signals
    /// redraws by itself).
    pub fn redraw(&mut self) {
        self.redraw = true;
    }

    /// Keep receiving the mouse's drags and its release while the pointer
    /// leaves the widget (a drag to scroll or resize). Released on the
    /// button's release, or with [`release_mouse`](Self::release_mouse).
    pub fn capture_mouse(&mut self) {
        self.capture = Some(true);
    }

    pub fn release_mouse(&mut self) {
        self.capture = Some(false);
    }

    /// The widget's width and height when it last drew.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// Where the widget is on the screen: to place a
    /// [pop-up](Ctx::popup) next to a part of it.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Whether this widget has the focus.
    pub fn focused(&self) -> bool {
        self.focused
    }
}

/// The widget's rectangle of the screen, in its own coordinates. Writes
/// outside it are clipped, and every write is recorded as damage.
pub struct Canvas<'a> {
    pub(crate) screen: &'a mut Screen,
    pub(crate) rect: Rect,
    pub(crate) written: &'a mut Vec<Rect>,
}

impl Canvas<'_> {
    pub fn width(&self) -> u16 {
        self.rect.width
    }

    pub fn height(&self) -> u16 {
        self.rect.height
    }

    /// Write rendered `lines` over the whole canvas; what they leave is
    /// cleared.
    pub fn lines(&mut self, lines: &[Vec<Segment>]) {
        self.screen.write_lines(self.rect, lines);
        self.written.push(self.rect);
    }

    /// Write rendered `lines` into the part of the canvas at `x`, `y`,
    /// `width` x `height`; the rest of that part is cleared.
    pub fn lines_at(&mut self, x: u16, y: u16, width: u16, height: u16, lines: &[Vec<Segment>]) {
        if let Some(area) = self.area(x, y, width, height) {
            self.screen.write_lines(area, lines);
            self.written.push(area);
        }
    }

    /// Write `text` from `x`, `y`, in `style`; it is cut at the canvas's
    /// right edge.
    pub fn print(&mut self, x: u16, y: u16, text: &str, style: Option<&Style>) {
        let width = rich::cells::cell_len(text).min(u16::MAX as usize) as u16;
        if let Some(area) = self.area(x, y, width, 1) {
            let line = vec![Segment::new(text.to_string(), style.cloned())];
            self.screen.write_lines(area, &[line]);
            self.written.push(area);
        }
    }

    /// Write console `markup` on one row from `x`, `y`, at most `width`
    /// cells (an ellipsis where it is cut), with `style` under its own;
    /// the cells it takes.
    pub fn markup(
        &mut self,
        console: &Console,
        x: u16,
        y: u16,
        width: u16,
        markup: &str,
        style: Option<&Style>,
    ) -> u16 {
        let Some(area) = self.area(x, y, width, 1) else {
            return 0;
        };
        let line = markup_line(console, markup, area.width);
        let used: usize = line.iter().map(Segment::cell_length).sum();
        let line: Vec<Segment> = match style {
            Some(base) => line
                .into_iter()
                .map(|s| {
                    let combined = match &s.style {
                        Some(own) => base.combine(own),
                        None => base.clone(),
                    };
                    Segment::new(s.text, Some(combined))
                })
                .collect(),
            None => line,
        };
        let area = Rect::new(area.x, area.y, (used as u16).min(area.width), 1);
        if !area.is_empty() {
            self.screen.write_lines(area, &[line]);
            self.written.push(area);
        }
        area.width
    }

    /// Render any rich renderable into the part at `x`, `y`, `width` x
    /// `height`.
    pub fn render(
        &mut self,
        console: &Console,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        renderable: &dyn Renderable,
    ) {
        if let Some(area) = self.area(x, y, width, height) {
            let options = console
                .options()
                .update_width(area.width as usize)
                .update_height(area.height as usize);
            let lines = console.render_lines(renderable, &options, false);
            self.screen.write_lines(area, &lines);
            self.written.push(area);
        }
    }

    /// Set the cell at `x`, `y` to `c` in `style`.
    pub fn set(&mut self, x: u16, y: u16, c: char, style: Option<&Style>) {
        let mut buffer = [0u8; 4];
        self.print(x, y, c.encode_utf8(&mut buffer), style);
    }

    /// Fill the part at `x`, `y`, `width` x `height` with spaces in
    /// `style` (a background).
    pub fn fill(&mut self, x: u16, y: u16, width: u16, height: u16, style: Option<&Style>) {
        if let Some(area) = self.area(x, y, width, height) {
            let row = vec![Segment::new(
                " ".repeat(area.width as usize),
                style.cloned(),
            )];
            let lines = vec![row; area.height as usize];
            self.screen.write_lines(area, &lines);
            self.written.push(area);
        }
    }

    /// Clear the part at `x`, `y`, `width` x `height`.
    pub fn clear(&mut self, x: u16, y: u16, width: u16, height: u16) {
        if let Some(area) = self.area(x, y, width, height) {
            self.screen.clear(area);
            self.written.push(area);
        }
    }

    /// Lay `style` over the cells at `x`, `y`, `width` x `height`, keeping
    /// their text (a highlight, a selection).
    pub fn restyle(&mut self, x: u16, y: u16, width: u16, height: u16, style: &Style) {
        if let Some(area) = self.area(x, y, width, height) {
            for row in area.y..area.bottom() {
                for column in area.x..area.right() {
                    self.screen.restyle(column, row, style);
                }
            }
            self.written.push(area);
        }
    }

    /// Move everything on the canvas up `rows` rows; the rows that open at
    /// the bottom are cleared. Cheaper than drawing it all again (a log).
    pub fn scroll_up(&mut self, rows: u16) {
        self.screen.scroll_up(self.rect, rows);
        self.written.push(self.rect);
    }

    /// A rounded border on the canvas's edges, with `title` in the top
    /// one; the inside is left as it is.
    pub fn border(&mut self, title: &str, style: &Style, title_style: &Style) {
        let lines = crate::node::border(title, style, title_style, self.rect);
        if lines.is_empty() {
            return;
        }
        for (edge, part) in crate::node::edges(self.rect)
            .into_iter()
            .zip(crate::node::edge_lines(&lines))
        {
            if !edge.is_empty() {
                self.screen.write_lines(edge, &part);
                self.written.push(edge);
            }
        }
    }

    /// The screen rectangle for a part of the canvas, clipped; `None` if
    /// nothing of it is on the canvas.
    fn area(&self, x: u16, y: u16, width: u16, height: u16) -> Option<Rect> {
        if x >= self.rect.width || y >= self.rect.height {
            return None;
        }
        let width = width.min(self.rect.width - x);
        let height = height.min(self.rect.height - y);
        let area = Rect::new(self.rect.x + x, self.rect.y + y, width, height);
        (!area.is_empty()).then_some(area)
    }
}

/// How many cells console `markup` takes on one line.
pub fn markup_width(markup: &str) -> u16 {
    rich::Text::from_markup(markup)
        .map(|t| t.cell_len())
        .unwrap_or_else(|_| rich::cells::cell_len(markup))
        .min(u16::MAX as usize) as u16
}

/// Console `markup` rendered on one line, `width` cells at most (an
/// ellipsis where it is cut).
pub fn markup_line(console: &Console, markup: &str, width: u16) -> Vec<Segment> {
    let text =
        rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.to_string()));
    let mut options = console.options().update_width(width.max(1) as usize);
    options.no_wrap = Some(true);
    options.overflow = Some(rich::Overflow::Ellipsis);
    console
        .render_lines(&text, &options, false)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// Handy questions about a mouse event.
pub trait MouseExt {
    /// A button went down.
    fn is_press(&self) -> bool;
    /// The wheel turned: -1 up, 1 down, 0 otherwise.
    fn wheel(&self) -> i32;
}

impl MouseExt for Mouse {
    fn is_press(&self) -> bool {
        matches!(self.kind, MouseKind::Down(_))
    }

    fn wheel(&self) -> i32 {
        match self.kind {
            MouseKind::ScrollUp => -1,
            MouseKind::ScrollDown => 1,
            _ => 0,
        }
    }
}
