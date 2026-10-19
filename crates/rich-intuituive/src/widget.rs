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
use crate::layout::{
    align_cross, break_lines, justify_offsets, offsets, place_lines, solve, Align, Justify, Size,
    StackLayout, Track,
};
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
    /// A key let go, offered as [`Key`](Self::Key) is but never to a
    /// binding. Only from a terminal with the kitty keyboard protocol (or
    /// the Windows console); see [`App::legacy_keys`](crate::App::legacy_keys).
    KeyUp(Key),
    /// The mouse over the widget, or captured by it.
    Mouse(Mouse),
    /// Pasted text, while the focus is on the widget or inside it.
    Paste(String),
    /// The focus came to this widget (`true`) or left it (`false`),
    /// including when a modal opens over it or closes. Told after the
    /// frame that lays out the change, so the widget knows where it is.
    Focus(bool),
    /// The pointer came over this widget or a node inside it (`true`), or
    /// left (`false`).
    Hover(bool),
    /// It was laid out at a new size (after its first layout).
    Resize { width: u16, height: u16 },
    /// A key on its way to the focused node inside this widget, offered
    /// first, outermost widget first; using it stops it there. Only to
    /// widgets whose [`previews_keys`](Widget::previews_keys) is true.
    Preview(Key),
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

    /// What assistive technology reads for it, as it is now: a meter's
    /// `Level 42%`, where its cells are block glyphs. A widget that says
    /// something is read as text (unless its [`role`](Self::role) is
    /// another), its accessible name what this returns, after a
    /// [`label`](crate::Node::label) code gave it; the accessibility tree,
    /// [linear](crate::App::linear) mode and a screen reader use it. Called
    /// after a draw, so it can say what the last one showed. Default: none,
    /// and the widget's cells are read.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widget::{Canvas, DrawCx};
    ///
    /// struct Meter(u8);
    /// impl Widget for Meter {
    ///     fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
    ///         canvas.print(0, 0, &"█".repeat(self.0 as usize / 10), None);
    ///     }
    ///     fn access_text(&self) -> Option<String> {
    ///         Some(format!("Level {}%", self.0))
    ///     }
    /// }
    ///
    /// let app = App::new(|| widget(Meter(42)));
    /// let mut driver = app.driver(10, 1);
    /// driver.settle();
    /// assert_eq!(driver.accessibility()[0].name, "Level 42%");
    /// ```
    fn access_text(&self) -> Option<String> {
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

    /// Draw over what its children drew: a badge, a highlight, an effect.
    /// It runs after the children, in every frame where the widget or a
    /// node inside it drew, on a canvas that is not cleared:
    /// [`Canvas::cell`] reads what is there, and what it writes is sent.
    /// When only a child drew, the rest of the canvas still holds what
    /// this wrote last time, so an overlay that changes cells it reads
    /// (reversing them, moving them) should be one that comes out the same
    /// when it runs twice, or the widget should not be
    /// [`retained`](Self::retained), which draws everything inside afresh
    /// whenever it draws. Signals read here make it draw again. Default:
    /// nothing.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::screen::Rect;
    /// use intuituive::widget::{widget, Canvas, DrawCx, MeasureCx, Widget};
    ///
    /// /// Its child, with a mark in the top right corner.
    /// struct Badge(Node);
    ///
    /// impl Widget for Badge {
    ///     fn children(&self) -> &[Node] {
    ///         std::slice::from_ref(&self.0)
    ///     }
    ///
    ///     fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
    ///         vec![rect]
    ///     }
    ///
    ///     fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}
    ///
    ///     fn draw_over(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
    ///         canvas.set_cell(canvas.width() - 1, 0, "●", None);
    ///     }
    /// }
    ///
    /// let app = App::new(|| widget(Badge(label("inbox"))));
    /// let mut driver = app.driver(8, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "inbox  ●");
    /// ```
    fn draw_over(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let _ = (cx, canvas);
    }

    /// Whether its children may overlap, later ones over earlier ones (as
    /// [`layers`](crate::layers) does): a child then draws again where an
    /// earlier sibling drew over it in the same frame, and a click goes to
    /// the last child under the pointer. Default: no.
    fn overlapping(&self) -> bool {
        false
    }

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

    /// Whether to see keys before the focused node inside this widget
    /// does, as [`WidgetEvent::Preview`] (a container's own shortcuts that
    /// win over its children's).
    fn previews_keys(&self) -> bool {
        false
    }

    /// Whether it takes `key` itself while it is on the focused path, so
    /// the app does not: Ctrl+Z then reaches it rather than suspending the
    /// app, and Ctrl+C rather than quitting. Default: no key.
    fn claims_key(&self, key: rich_interact::Key) -> bool {
        let _ = key;
        false
    }

    /// Where the text caret goes while it has the focus, in its own
    /// coordinates.
    fn caret(&self) -> Option<(u16, u16)> {
        None
    }

    /// What it is, for assistive technology. Layout widgets keep the
    /// default, [`Role::Group`](crate::a11y::Role::Group).
    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Group
    }

    /// Where its selected item is (a list's row, a table's cell, a tab), in
    /// its own coordinates: in [accessible](crate::App::accessible) mode
    /// the terminal's cursor goes to its top left while it has the focus,
    /// so a screen reader reads it, and the accessibility tree gives what
    /// it shows as the widget's value. Wider than the widget is fine: it
    /// is clipped.
    fn cursor(&self) -> Option<crate::screen::Rect> {
        None
    }

    /// Its states, for assistive technology: whether its selected item is
    /// expanded, checked, selected and where it is among the items, and
    /// whether it is busy. The node's own
    /// [`expanded_when`](Node::expanded_when) and the like win. Default:
    /// none.
    fn access_state(&self) -> crate::a11y::AccessState {
        crate::a11y::AccessState::default()
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
        self.stack_flow(
            axis,
            StackLayout {
                gap,
                ..StackLayout::default()
            },
            children,
            rect,
        )
    }

    /// [`stack`](Self::stack) with alignment and wrapping, as a
    /// [`column`](crate::column) or [`row`](crate::row) given
    /// [`align_items`](Node::align_items), [`justify`](Node::justify) or
    /// [`wrap`](Node::wrap) does. A child's own
    /// [`align_self`](Node::align_self) wins over `flow.align_items`.
    pub fn stack_flow(
        &self,
        axis: Axis,
        flow: StackLayout,
        children: &[Node],
        rect: Rect,
    ) -> Vec<Rect> {
        if flow.wrap {
            return self.stack_wrapped(axis, flow, children, rect);
        }
        let gap = flow.gap;
        // Docked children go first or last; hidden ones take no room and
        // no gap.
        use crate::sheet::Dock;
        let rank = |child: &Node| match (child.dock(), axis) {
            (Some(Dock::Top), Axis::Vertical) | (Some(Dock::Left), Axis::Horizontal) => 0,
            (Some(Dock::Bottom), Axis::Vertical) | (Some(Dock::Right), Axis::Horizontal) => 2,
            _ => 1,
        };
        let mut order: Vec<usize> = (0..children.len())
            .filter(|&i| !children[i].hidden())
            .collect();
        order.sort_by_key(|&i| rank(&children[i]));
        let tracks: Vec<_> = order
            .iter()
            .map(|&i| self.main_track(&children[i], axis, rect))
            .collect();
        let (start, total) = main_span(axis, rect);
        if flow.solver != crate::solver::Solver::Native {
            // Ratatui's solver places them all, justified as its `Flex`.
            let placed =
                crate::solver::split(start, total, gap, &tracks, flow.solver, flow.justify);
            let (across, room) = cross_span(axis, rect);
            let mut areas = vec![Rect::default(); children.len()];
            for ((at, n), &i) in placed.into_iter().zip(&order) {
                let (c, cn) = self.cross(&children[i], axis, flow.align_items, n, across, room);
                areas[i] = along(axis, at, n, c, cn);
            }
            return areas;
        }
        let sizes = solve(total, gap, &tracks);
        let mut at = offsets(start, gap, &sizes);
        // Children docked to the far edge keep to it when the rest leave
        // room over.
        let tail = order.iter().filter(|&&i| rank(&children[i]) == 2).count();
        if tail > 0 {
            let first = sizes.len() - tail;
            let length: u32 = sizes[first..].iter().map(|&n| n as u32).sum::<u32>()
                + gap as u32 * (tail as u32 - 1);
            let from = (start as u32 + total as u32).saturating_sub(length) as u16;
            let from = from.max(at[first]);
            for (k, offset) in offsets(from, gap, &sizes[first..]).into_iter().enumerate() {
                at[first + k] = offset;
            }
        }
        // The children between the docked ones share what is left by
        // `justify`.
        let head = order.iter().filter(|&&i| rank(&children[i]) == 0).count();
        let middle = head..order.len() - tail;
        if !middle.is_empty() && flow.justify != Justify::Start {
            let from = at[middle.start];
            let end = if tail > 0 {
                at[middle.end].saturating_sub(gap)
            } else {
                start.saturating_add(total)
            };
            let placed = justify_offsets(
                from,
                end.saturating_sub(from),
                gap,
                &sizes[middle.clone()],
                flow.justify,
            );
            at[middle].copy_from_slice(&placed);
        }
        let (across, room) = cross_span(axis, rect);
        let mut areas = vec![Rect::default(); children.len()];
        for ((at, n), &i) in at.into_iter().zip(sizes).zip(&order) {
            let (c, cn) = self.cross(&children[i], axis, flow.align_items, n, across, room);
            areas[i] = along(axis, at, n, c, cn);
        }
        areas
    }

    /// A child's track along `axis` in `rect`, its content measured for
    /// [`Size::Auto`]: in the whole rectangle across the axis, and
    /// unbounded along it.
    pub(crate) fn main_track(&self, child: &Node, axis: Axis, rect: Rect) -> Track {
        let mut track = child.track();
        if track.size == Size::Auto {
            track.content = self.main_content(child, axis, rect);
        }
        track
    }

    fn main_content(&self, child: &Node, axis: Axis, rect: Rect) -> u16 {
        match axis {
            Axis::Vertical => self.measure(child, Axis::Vertical, rect.width, 0),
            Axis::Horizontal => self.measure(child, Axis::Horizontal, rect.width, rect.height),
        }
    }

    /// Across `axis`: where `child`, `main` cells along it, sits in `room`
    /// cells from `at`, and how many it takes.
    fn cross(
        &self,
        child: &Node,
        axis: Axis,
        align: Align,
        main: u16,
        at: u16,
        room: u16,
    ) -> (u16, u16) {
        let align = child.self_align().unwrap_or(align);
        if align == Align::Stretch {
            return (at, room);
        }
        align_cross(at, room, self.cross_content(child, axis, main, room), align)
    }

    /// The cells `child` needs across `axis` when it is `main` cells along
    /// it, with `room` cells to take.
    fn cross_content(&self, child: &Node, axis: Axis, main: u16, room: u16) -> u16 {
        match axis {
            Axis::Vertical => self.measure(child, Axis::Horizontal, room, main),
            Axis::Horizontal => self.measure(child, Axis::Vertical, main, 0),
        }
    }

    /// The lines a wrapping stack breaks `children` into along `total`
    /// cells, with `room` across: each line's children (indices into
    /// `children`), their sizes along the axis and the line's size across
    /// it (its largest child).
    pub(crate) fn wrap_lines(
        &self,
        axis: Axis,
        gap: u16,
        children: &[Node],
        total: u16,
        room: u16,
    ) -> Vec<(Vec<usize>, Vec<u16>, u16)> {
        let shown: Vec<usize> = (0..children.len())
            .filter(|&i| !children[i].hidden())
            .collect();
        let rect = match axis {
            Axis::Vertical => Rect::new(0, 0, room, total),
            Axis::Horizontal => Rect::new(0, 0, total, room),
        };
        // Each child's size before the lines share out what is left: its
        // own, or its content for a flexible one, as a CSS flex basis.
        let tracks: Vec<Track> = shown
            .iter()
            .map(|&i| {
                let mut track = self.main_track(&children[i], axis, rect);
                if matches!(track.size, Size::Flex(_)) {
                    track.content = self.main_content(&children[i], axis, rect);
                }
                track
            })
            .collect();
        let bases: Vec<u16> = tracks
            .iter()
            .map(|t| {
                let wanted = match t.size {
                    Size::Fixed(cells) => cells as u32,
                    Size::Percent(p) => total as u32 * p.min(100) as u32 / 100,
                    Size::Ratio(a, b) => crate::layout::ratio_of(total as u32, a, b),
                    Size::Auto | Size::Flex(_) => t.content as u32,
                };
                wanted
                    .min(t.max as u32)
                    .max(t.min as u32)
                    .min(u16::MAX as u32) as u16
            })
            .collect();
        break_lines(total, gap, &bases)
            .into_iter()
            .map(|line| {
                // As CSS: a percentage is of the container, as the line was
                // broken by, and a flexible item starts at its content (its
                // basis) and grows by its share of what the line leaves. So
                // the bases come off the line first, and only the rest is
                // shared, the clamps moved down by the basis to match.
                let basis = |k: usize| match tracks[k].size {
                    Size::Flex(_) => bases[k].min(total),
                    _ => 0,
                };
                let line_tracks: Vec<Track> = line
                    .clone()
                    .map(|k| match tracks[k].size {
                        Size::Percent(_) | Size::Ratio(..) => Track {
                            size: Size::Fixed(bases[k]),
                            ..tracks[k]
                        },
                        Size::Flex(_) => Track {
                            min: tracks[k].min.saturating_sub(basis(k)),
                            max: tracks[k].max.saturating_sub(basis(k)),
                            ..tracks[k]
                        },
                        _ => tracks[k],
                    })
                    .collect();
                let held: u32 = line.clone().map(|k| basis(k) as u32).sum();
                let rest = (total as u32).saturating_sub(held).min(u16::MAX as u32) as u16;
                let sizes: Vec<u16> = solve(rest, gap, &line_tracks)
                    .into_iter()
                    .zip(line.clone())
                    .map(|(size, k)| size.saturating_add(basis(k)))
                    .collect();
                let items: Vec<usize> = line.map(|k| shown[k]).collect();
                let cross = items
                    .iter()
                    .zip(&sizes)
                    .map(|(&i, &n)| self.cross_content(&children[i], axis, n, room))
                    .max()
                    .unwrap_or(0);
                (items, sizes, cross)
            })
            .collect()
    }

    fn stack_wrapped(
        &self,
        axis: Axis,
        flow: StackLayout,
        children: &[Node],
        rect: Rect,
    ) -> Vec<Rect> {
        let (start, total) = main_span(axis, rect);
        let (across, room) = cross_span(axis, rect);
        let lines = self.wrap_lines(axis, flow.gap, children, total, room);
        let crosses: Vec<u16> = lines.iter().map(|(_, _, cross)| *cross).collect();
        let placed = place_lines(across, room, flow.gap, &crosses, flow.align_content);
        let mut areas = vec![Rect::default(); children.len()];
        for ((items, sizes, _), (line_at, line_size)) in lines.into_iter().zip(placed) {
            let at = justify_offsets(start, total, flow.gap, &sizes, flow.justify);
            for ((i, n), at) in items.into_iter().zip(sizes).zip(at) {
                let (c, cn) =
                    self.cross(&children[i], axis, flow.align_items, n, line_at, line_size);
                // Lines that do not fit are cut at the container's edge.
                areas[i] = along(axis, at, n, c, cn).intersection(rect);
            }
        }
        areas
    }
}

/// Where `rect` starts along `axis`, and its length.
fn main_span(axis: Axis, rect: Rect) -> (u16, u16) {
    match axis {
        Axis::Vertical => (rect.y, rect.height),
        Axis::Horizontal => (rect.x, rect.width),
    }
}

/// Where `rect` starts across `axis`, and its breadth.
fn cross_span(axis: Axis, rect: Rect) -> (u16, u16) {
    match axis {
        Axis::Vertical => (rect.x, rect.width),
        Axis::Horizontal => (rect.y, rect.height),
    }
}

/// The rectangle `n` cells along `axis` from `at`, `cn` across from `c`.
fn along(axis: Axis, at: u16, n: u16, c: u16, cn: u16) -> Rect {
    match axis {
        Axis::Vertical => Rect::new(c, at, cn, n),
        Axis::Horizontal => Rect::new(at, c, n, cn),
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
    /// The app's clock: the time since it started, as its loop (or a test's
    /// [`Driver::update`](crate::Driver::update) and
    /// [`advance`](crate::Driver::advance)) last gave it. A peak that holds
    /// for a second, a stall noticed after five, tested frame by frame.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widget::{Canvas, DrawCx};
    /// use std::time::Duration;
    ///
    /// struct Clock;
    /// impl Widget for Clock {
    ///     fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
    ///         canvas.print(0, 0, &format!("{}ms", cx.now().as_millis()), None);
    ///     }
    /// }
    ///
    /// let app = App::new(|| widget(Clock));
    /// let mut driver = app.driver(10, 1);
    /// driver.update(Duration::from_millis(250));
    /// driver.render();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "250ms");
    /// ```
    pub fn now(&self) -> std::time::Duration {
        crate::reactive::Runtime::current().now.get()
    }

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

    /// Where the widget is on the screen (as [`EventCx::rect`] tells a
    /// handler): for what is drawn outside the cells and must know where
    /// they are, a terminal graphic placed over the widget. Inside a
    /// [viewport](Widget::viewport) it is the rectangle in the viewport's
    /// content, which [`pointer`](Self::pointer) and the canvas share.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widget::{widget, Canvas, DrawCx, Widget};
    ///
    /// struct Where;
    /// impl Widget for Where {
    ///     fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
    ///         let r = cx.rect();
    ///         canvas.print(0, 0, &format!("{},{} {}x{}", r.x, r.y, r.width, r.height), None);
    ///     }
    /// }
    ///
    /// let app = App::new(|| column([text("top").fixed(1), widget(Where)]));
    /// let mut driver = app.driver(12, 3);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "0,1 12x2");
    /// ```
    pub fn rect(&self) -> Rect {
        self.rect
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

/// A theme style by name, if the console's theme has it.
pub(crate) fn named_style(console: &Console, name: &str) -> Option<Style> {
    console
        .get_style(&rich::style::StyleType::Name(name.to_string()))
        .ok()
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

    /// Set the cell at `x`, `y` to `ch` with foreground `fg` and background
    /// `bg` in true colour (either missing), without building a segment or
    /// a style: the fast way to draw a picture a cell at a time, as the
    /// [`pixels`](crate::pixels::pixels) node does. `ch` takes one cell (a
    /// wide character becomes a space); off the canvas nothing is written.
    /// See [`Screen::put_rgb`](rich_ext::surface::Screen::put_rgb).
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widget::{widget, Canvas, DrawCx, Widget};
    ///
    /// struct Gradient;
    /// impl Widget for Gradient {
    ///     fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
    ///         for x in 0..canvas.width() {
    ///             let v = (x * 60) as u8;
    ///             canvas.put_rgb(x, 0, '▀', Some([v, 0, 0]), Some([0, 0, v]));
    ///         }
    ///     }
    /// }
    ///
    /// let mut driver = App::new(|| widget(Gradient)).driver(4, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "▀▀▀▀");
    /// ```
    pub fn put_rgb(&mut self, x: u16, y: u16, ch: char, fg: Option<[u8; 3]>, bg: Option<[u8; 3]>) {
        if x >= self.rect.width || y >= self.rect.height {
            return;
        }
        let (sx, sy) = (self.rect.x + x, self.rect.y + y);
        self.screen.put_rgb(sx, sy, ch, fg, bg);
        // A row drawn left to right is one rectangle of damage.
        match self.written.last_mut() {
            Some(last) if last.y == sy && last.height == 1 && last.right() == sx => last.width += 1,
            Some(last) if last.contains(sx, sy) => {}
            _ => self.written.push(Rect::new(sx, sy, 1, 1)),
        }
    }

    /// Set the cell at `x`, `y` to `c` in `style`.
    pub fn set(&mut self, x: u16, y: u16, c: char, style: Option<&Style>) {
        let mut buffer = [0u8; 4];
        self.print(x, y, c.encode_utf8(&mut buffer), style);
    }

    /// What is on the screen at `x`, `y` of the canvas now: what this
    /// widget, its parent, an earlier sibling or (in
    /// [`draw_over`](Widget::draw_over)) its children drew there. `None`
    /// off the canvas.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widget::{widget, Canvas, DrawCx, Widget};
    ///
    /// /// Each cell's symbol, in capitals.
    /// struct Shout;
    ///
    /// impl Widget for Shout {
    ///     fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
    ///         canvas.print(0, 0, "hey", None);
    ///         for x in 0..canvas.width() {
    ///             let Some(cell) = canvas.cell(x, 0) else { continue };
    ///             let (upper, style) = (cell.symbol().to_uppercase(), cell.style().cloned());
    ///             canvas.set_cell(x, 0, &upper, style.as_ref());
    ///         }
    ///     }
    /// }
    ///
    /// let mut driver = App::new(|| widget(Shout)).driver(3, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0], "HEY");
    /// ```
    pub fn cell(&self, x: u16, y: u16) -> Option<CellView<'_>> {
        if x >= self.rect.width || y >= self.rect.height {
            return None;
        }
        let cell = self.screen.get(self.rect.x + x, self.rect.y + y)?;
        Some(CellView {
            symbol: &cell.text,
            style: self.screen.style(cell.style),
            width: cell.width,
        })
    }

    /// Set the cell at `x`, `y` to `symbol` (one grapheme, which may be
    /// two cells wide) in `style`.
    pub fn set_cell(&mut self, x: u16, y: u16, symbol: &str, style: Option<&Style>) {
        self.print(x, y, symbol, style);
    }

    /// Fill the part at `x`, `y`, `width` x `height` with `symbol` (one
    /// grapheme) in `style`: ratatui's `Fill`. A wide symbol that does not
    /// fit at the end of a row leaves a space.
    pub fn fill_with(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        symbol: &str,
        style: Option<&Style>,
    ) {
        let Some(area) = self.area(x, y, width, height) else {
            return;
        };
        let cells = rich::cells::cell_len(symbol).max(1);
        let times = area.width as usize / cells;
        let rest = area.width as usize - times * cells;
        let row = format!("{}{}", symbol.repeat(times), " ".repeat(rest));
        let lines = vec![vec![Segment::new(row, style.cloned())]; area.height as usize];
        self.screen.write_lines(area, &lines);
        self.written.push(area);
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

    /// A rounded border on the canvas's edges, with `title` (plain text)
    /// in the top one; the inside is left as it is.
    pub fn border(&mut self, title: &str, style: &Style, title_style: &Style) {
        let edges = crate::node::draw_box(
            self.screen,
            crate::border::Border::Rounded,
            title,
            style,
            title_style,
            self.rect,
        );
        self.written.extend(edges);
    }

    /// A panel's `frame` on the canvas's edges, a cell in from the right
    /// and the bottom when `shadowed`, where `shadow` (when given) is
    /// drawn.
    pub(crate) fn frame(
        &mut self,
        console: &Console,
        frame: &crate::border::Frame,
        shadowed: bool,
        shadow: Option<(crate::border::Shadow, Style)>,
    ) {
        let mut rect = self.rect;
        if shadowed {
            if let Some((shadow, style)) = shadow {
                let strip = crate::border::draw_shadow(self.screen, shadow, &style, rect);
                self.written.extend(strip);
            }
            rect = Rect::new(
                rect.x,
                rect.y,
                rect.width.saturating_sub(1),
                rect.height.saturating_sub(1),
            );
        }
        let edges = crate::border::draw_frame(self.screen, console, frame, rect);
        self.written.extend(edges);
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

/// One cell as [`Canvas::cell`] reads it.
#[derive(Clone, Copy, Debug)]
pub struct CellView<'a> {
    symbol: &'a str,
    style: Option<&'a Style>,
    width: u8,
}

impl CellView<'_> {
    /// Its grapheme: empty for the second column of a wide one.
    pub fn symbol(&self) -> &str {
        self.symbol
    }

    /// Its style, if it has one.
    pub fn style(&self) -> Option<&Style> {
        self.style
    }

    /// The columns its grapheme takes: 1, 2 for a wide one, and 0 for the
    /// second column of a wide one.
    pub fn width(&self) -> u16 {
        u16::from(self.width)
    }

    /// Whether it is the second column of a wide grapheme.
    pub fn is_continuation(&self) -> bool {
        self.width == 0
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
