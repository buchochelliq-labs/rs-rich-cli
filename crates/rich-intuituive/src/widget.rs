//! Widgets: controls written outside this crate with the powers the
//! built-in nodes have.
//!
//! A [`Widget`] sizes itself, draws cells on a [`Canvas`], takes keys and
//! mouse events in its own coordinates, holds the focus and places the
//! text caret, and may hold children: ordinary [`Node`]s it lays out. Make
//! a node from one with [`widget`]; it takes every builder (`.flex`,
//! `.panel`, `.on_key`, `.name`).
//!
//! Like a [`leaf`](crate::leaf), a widget draws again when a
//! [signal](crate::signal) it read while drawing changes, when it moves,
//! or when its own state changed and an event handler asked for a redraw
//! with [`EventCx::redraw`].
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

use std::cell::Cell;

use rich::{Console, Segment, Style};
use rich_interact::{Key, Mouse, MouseKind};

use crate::app::{Ctx, Theme};
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

/// An event for a widget: a key while the focus is on it or inside it, or
/// the mouse over it (or captured by it), in its own coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetEvent {
    Key(Key),
    Mouse(Mouse),
}

/// A node's behaviour. Every method but [`draw`](Self::draw) has a default.
pub trait Widget: 'static {
    /// What the inspector calls it (`"table"`, `"chart"`).
    fn name(&self) -> &'static str {
        "widget"
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

    /// Its children, for a widget that holds nodes.
    fn children(&self) -> &[Node] {
        &[]
    }

    /// Where each of its children goes, in the order of
    /// [`children`](Self::children), given the widget's own `rect` (in
    /// screen coordinates). A child given no rectangle, or an empty one, is
    /// hidden. Default: none.
    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let _ = (cx, rect);
        Vec::new()
    }

    /// Draw itself, not its children, on `canvas`: the widget's rectangle,
    /// cleared, in its own coordinates. Signals read here make it draw
    /// again when they change.
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas);

    /// A key or mouse event. One it does not use bubbles to its ancestors,
    /// whose [`on_key`](Node::on_key) bindings may take it.
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
    let node = Node::new_kind(
        crate::node::Kind::Custom {
            widget: Box::new(widget),
            areas: Vec::new(),
        },
        name,
    );
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
}

/// What a widget sees while it draws.
pub struct DrawCx<'a> {
    pub(crate) console: &'a Console,
    pub(crate) theme: &'a Theme,
    pub(crate) id: NodeId,
    pub(crate) focus_path: Signal<Vec<NodeId>>,
    pub(crate) hover_path: Signal<Vec<NodeId>>,
    pub(crate) wants_hover: &'a Cell<bool>,
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
    /// it). Asking makes it draw again when that changes, and turns on the
    /// terminal's reports of pointer movement.
    pub fn hovered(&self) -> bool {
        self.wants_hover.set(true);
        self.hover_path.with(|path| path.contains(&self.id))
    }

    /// A theme style by name (`"accent"`, `"selected"`), or `fallback`.
    pub fn style(&self, name: &str, fallback: &str) -> Style {
        self.console
            .get_style(&rich::style::StyleType::Name(name.to_string()))
            .ok()
            .or_else(|| Style::parse(fallback).ok())
            .unwrap_or_default()
    }
}

/// What a widget can do while it handles an event: what any handler can
/// (through [`app`](Self::app)), and ask to draw again or to keep the
/// mouse.
pub struct EventCx<'a> {
    pub(crate) ctx: &'a mut Ctx,
    pub(crate) size: (u16, u16),
    pub(crate) redraw: bool,
    pub(crate) capture: Option<bool>,
}

impl EventCx<'_> {
    /// Quit, open a screen or a modal, move the focus, set the theme.
    pub fn app(&mut self) -> &mut Ctx {
        self.ctx
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
}

/// The widget's rectangle of the screen, cleared, in its own coordinates.
/// Writes outside it are clipped.
pub struct Canvas<'a> {
    pub(crate) screen: &'a mut Screen,
    pub(crate) rect: Rect,
}

impl Canvas<'_> {
    pub fn width(&self) -> u16 {
        self.rect.width
    }

    pub fn height(&self) -> u16 {
        self.rect.height
    }

    /// Write rendered `lines` over the whole canvas, as a leaf does.
    pub fn lines(&mut self, lines: &[Vec<Segment>]) {
        self.screen.write_lines(self.rect, lines);
    }

    /// Write rendered `lines` into the part of the canvas at `x`, `y`,
    /// `width` x `height`; the rest of that part is cleared.
    pub fn lines_at(&mut self, x: u16, y: u16, width: u16, height: u16, lines: &[Vec<Segment>]) {
        if let Some(area) = self.area(x, y, width, height) {
            self.screen.write_lines(area, lines);
        }
    }

    /// Write `text` from `x`, `y`, in `style`; it is cut at the canvas's
    /// right edge.
    pub fn print(&mut self, x: u16, y: u16, text: &str, style: Option<&Style>) {
        let width = rich::cells::cell_len(text).min(u16::MAX as usize) as u16;
        if let Some(area) = self.area(x, y, width, 1) {
            let line = vec![Segment::new(text.to_string(), style.cloned())];
            self.screen.write_lines(area, &[line]);
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
