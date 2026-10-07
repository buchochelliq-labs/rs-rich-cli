//! Nodes: the retained tree an app is made of.
//!
//! A [`Node`] is built once and kept. It remembers the rectangle it was
//! laid out in, so the app can route a click to it without the author
//! storing anything, and it draws again only when a [signal](crate::signal)
//! it read changed, or it moved. A frame writes the nodes that drew into
//! the [`Screen`] and reports their rectangles as damage.
//!
//! Builders:
//!
//! - [`text`](fn@text) and the [`text!`](crate::text!) macro: console markup that may
//!   read signals; [`label`] for markup that never changes;
//! - [`renderable`]: any rich renderable (a `Table`, `Markdown`, `Syntax`,
//!   a chart), rebuilt when the signals it read change;
//! - [`column`](fn@column) and [`row`]: children laid out along an axis, each sized
//!   [`Size::Fixed`], [`Size::Percent`], [`Size::Flex`] or
//!   [`Size::Auto`], within [`Node::min_size`] and [`Node::max_size`];
//! - [`grid`]: children in rows and columns, spanning several with
//!   [`Node::span`];
//! - [`each`]: one child per key of a list, kept by key;
//! - [`switch`]: one child at a time, chosen by a key, each kept while
//!   hidden (tabs, wizard steps);
//! - [`Node::panel`]: a rounded border with a title, highlighted while the
//!   focus is inside it; [`Node::padding`] for space round a node.
//!
//! and behaviour: [`Node::on_key`] binds keys (they bubble from the focused
//! node to its ancestors), [`Node::on_click`] handles clicks, and
//! [`Node::focusable`] puts a node in the Tab order.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use rich::{Console, Renderable, Segment, Style};
use rich_interact::{Component, Context, Event, Flow, Key, View};

use crate::app::Ctx;
pub use crate::layout::Size;
use crate::layout::{offsets, place, solve, Track};
use crate::reactive::{next_node, NodeId, Runtime, Signal};
use crate::screen::{Rect, Screen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

type Draw = Box<dyn Fn(&Console, u16, u16) -> Vec<Vec<Segment>>>;
pub(crate) type Handler = Box<dyn FnMut(&mut Ctx)>;

pub(crate) enum Kind {
    Leaf(Draw),
    Stack(Stack),
    Grid(Grid),
    Panel {
        title: String,
        child: Box<Node>,
    },
    Pad {
        child: Box<Node>,
        /// Top, right, bottom, left.
        edges: [u16; 4],
    },
    Each {
        list: Box<dyn Reconcile>,
        areas: Vec<Rect>,
    },
    Switch(Box<dyn Switching>),
    Log(crate::log::LogView),
    Host(Box<dyn Hosted>),
}

/// Children along an axis.
pub(crate) struct Stack {
    axis: Axis,
    gap: u16,
    children: Vec<Node>,
    /// Where the children were laid out last.
    areas: Vec<Rect>,
}

/// Children in rows and columns.
pub(crate) struct Grid {
    columns: Vec<Size>,
    rows: Vec<Size>,
    /// Between rows, and between columns.
    gap: (u16, u16),
    children: Vec<Node>,
    areas: Vec<Rect>,
}

/// What a hosted component did with an event.
pub(crate) enum Used {
    /// It used it (and may look different now).
    Yes,
    /// Not for it: the event bubbles to the node's ancestors.
    No,
}

/// A `rich-interact` component living in the tree.
pub(crate) trait Hosted {
    fn handle(&mut self, event: &Event, context: &Context<'_>, cx: &mut Ctx) -> Used;
    fn render(&self, context: &Context<'_>) -> View;
    fn set_cancel(&mut self, cancel: Handler);
}

type OnDone<T> = Box<dyn FnMut(T, &mut Ctx)>;

struct Host<C: Component> {
    component: C,
    on_done: OnDone<C::Output>,
    on_cancel: Option<Handler>,
}

impl<C: Component> Hosted for Host<C> {
    fn handle(&mut self, event: &Event, context: &Context<'_>, cx: &mut Ctx) -> Used {
        match self.component.handle(event, context) {
            Flow::Ignored => Used::No,
            Flow::Continue => Used::Yes,
            Flow::Done(value) => {
                (self.on_done)(value, cx);
                Used::Yes
            }
            // With no cancel handler, the key that cancelled (Esc) is the
            // app's: it bubbles, so an app-wide Esc binding still works.
            Flow::Cancel => match &mut self.on_cancel {
                Some(cancel) => {
                    cancel(cx);
                    Used::Yes
                }
                None => Used::No,
            },
            // Handing the terminal to another program is not supported
            // inside an app yet; the component carries on.
            Flow::Handoff(_) => Used::Yes,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.component.render(context)
    }

    fn set_cancel(&mut self, cancel: Handler) {
        self.on_cancel = Some(cancel);
    }
}

/// One node of the tree.
pub struct Node {
    pub(crate) id: NodeId,
    pub(crate) size: Size,
    min: u16,
    max: u16,
    /// Columns and rows it spans in a [`grid`].
    span: (u16, u16),
    pub(crate) kind: RefCell<Kind>,
    /// Where it was last laid out.
    pub(crate) rect: Cell<Rect>,
    /// Where it last drew; `None` before its first draw, or while hidden.
    drawn: Cell<Option<Rect>>,
    pub(crate) focusable: bool,
    pub(crate) keys: RefCell<Vec<(Vec<Key>, String, Handler)>>,
    pub(crate) click: RefCell<Option<Handler>>,
    /// Where a hosted component wants the text caret, on the screen.
    pub(crate) caret: Cell<Option<(u16, u16)>>,
}

impl Node {
    pub(crate) fn new_kind(kind: Kind) -> Node {
        Node::new(kind)
    }

    fn new(kind: Kind) -> Node {
        Node {
            id: next_node(),
            size: Size::Flex(1),
            min: 0,
            max: u16::MAX,
            span: (1, 1),
            kind: RefCell::new(kind),
            rect: Cell::new(Rect::default()),
            drawn: Cell::new(None),
            focusable: false,
            keys: RefCell::new(Vec::new()),
            click: RefCell::new(None),
            caret: Cell::new(None),
        }
    }

    /// Size this node along its parent's axis.
    pub fn size(mut self, size: Size) -> Node {
        self.size = size;
        self
    }

    /// Exactly `cells` along the parent's axis.
    pub fn fixed(self, cells: u16) -> Node {
        self.size(Size::Fixed(cells))
    }

    /// A flexible share, by `weight`, of the space left over.
    pub fn flex(self, weight: u16) -> Node {
        self.size(Size::Flex(weight))
    }

    /// `percent` of the parent's axis.
    pub fn percent(self, percent: u16) -> Node {
        self.size(Size::Percent(percent))
    }

    /// As much of the parent's axis as the content needs (see
    /// [`Size::Auto`]).
    pub fn auto(self) -> Node {
        self.size(Size::Auto)
    }

    /// Never less than `cells` along the parent's axis, while there is
    /// room.
    pub fn min_size(mut self, cells: u16) -> Node {
        self.min = cells;
        self
    }

    /// Never more than `cells` along the parent's axis.
    pub fn max_size(mut self, cells: u16) -> Node {
        self.max = cells;
        self
    }

    /// In a [`grid`]: span `columns` columns and `rows` rows.
    pub fn span(mut self, columns: u16, rows: u16) -> Node {
        self.span = (columns.max(1), rows.max(1));
        self
    }

    /// For a [`column`](fn@column), [`row`] or [`grid`]: leave `cells` empty
    /// between neighbouring children. Ignored on other nodes.
    pub fn gap(self, cells: u16) -> Node {
        match &mut *self.kind.borrow_mut() {
            Kind::Stack(stack) => stack.gap = cells,
            Kind::Grid(grid) => grid.gap = (cells, cells),
            _ => {}
        }
        self
    }

    /// For a [`grid`]: the rows' sizes. Rows past the last one given take
    /// its size, so `.rows([Size::Auto])` makes every row as tall as its
    /// content; without any, rows share the height evenly. Ignored on other
    /// nodes.
    pub fn rows(self, rows: impl IntoIterator<Item = Size>) -> Node {
        if let Kind::Grid(grid) = &mut *self.kind.borrow_mut() {
            grid.rows = rows.into_iter().collect();
        }
        self
    }

    /// Put this node in the Tab order, so it can hold the focus.
    pub fn focusable(mut self) -> Node {
        self.focusable = true;
        self
    }

    /// Run `handler` when one of `keys` (space-separated names: `"q"`,
    /// `"ctrl+s"`, `"up k"`) is pressed while this node or a node inside it
    /// has the focus, and nothing deeper used the key. The binding also
    /// shows in the app's help.
    pub fn on_key(self, keys: &str, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.bind(keys, "", handler)
    }

    /// [`on_key`](Self::on_key) with a description for the help.
    pub fn bind(
        self,
        keys: &str,
        description: &str,
        handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        let parsed = rich_interact::keymap::keys(keys);
        self.keys
            .borrow_mut()
            .push((parsed, description.to_string(), Box::new(handler)));
        self
    }

    /// Run `handler` when this node is clicked. A clickable node is also
    /// focusable, and a click focuses it.
    pub fn on_click(mut self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        *self.click.borrow_mut() = Some(Box::new(handler));
        self.focusable = true;
        self
    }

    /// For a [`component`] node: run `handler` when the component is
    /// cancelled (Esc). Without one, the cancelling key bubbles to the
    /// node's ancestors like any unused key. Ignored on other nodes.
    pub fn on_cancel(self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        if let Kind::Host(host) = &mut *self.kind.borrow_mut() {
            host.set_cancel(Box::new(handler));
        }
        self
    }

    /// Wrap this node in a rounded border with `title`. The border is
    /// highlighted while the focus is on the node or inside it. The panel
    /// takes the node's size.
    pub fn panel(self, title: &str) -> Node {
        let title = title.to_string();
        self.wrap(|child| Kind::Panel { title, child })
    }

    /// Leave `vertical` empty rows above and below this node, and
    /// `horizontal` empty columns either side.
    pub fn padding(self, vertical: u16, horizontal: u16) -> Node {
        self.wrap(|child| Kind::Pad {
            child,
            edges: [vertical, horizontal, vertical, horizontal],
        })
    }

    /// A node of `kind` round this one, taking over its size and span.
    fn wrap(self, kind: impl FnOnce(Box<Node>) -> Kind) -> Node {
        let (size, min, max, span) = (self.size, self.min, self.max, self.span);
        let mut outer = Node::new(kind(Box::new(self)));
        outer.size = size;
        outer.min = min;
        outer.max = max;
        outer.span = span;
        outer
    }

    /// Where the node was last laid out.
    pub fn rect(&self) -> Rect {
        self.rect.get()
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    fn track(&self) -> Track {
        Track {
            size: self.size,
            min: self.min,
            max: self.max,
            content: 0,
        }
    }

    /// Visit this node and every node inside it that is shown, depth first.
    pub(crate) fn walk(&self, f: &mut dyn FnMut(&Node, &[NodeId])) {
        let mut path = Vec::new();
        self.walk_inner(&mut path, f);
    }

    fn walk_inner(&self, path: &mut Vec<NodeId>, f: &mut dyn FnMut(&Node, &[NodeId])) {
        path.push(self.id);
        f(self, path);
        match &*self.kind.borrow() {
            Kind::Leaf(_) | Kind::Log(_) | Kind::Host(_) => {}
            Kind::Stack(stack) => {
                for child in &stack.children {
                    child.walk_inner(path, f);
                }
            }
            Kind::Grid(grid) => {
                for child in &grid.children {
                    child.walk_inner(path, f);
                }
            }
            Kind::Panel { child, .. } | Kind::Pad { child, .. } => child.walk_inner(path, f),
            Kind::Each { list, .. } => {
                for child in list.children() {
                    child.walk_inner(path, f);
                }
            }
            Kind::Switch(switch) => {
                if let Some(child) = switch.current() {
                    child.walk_inner(path, f);
                }
            }
        }
        path.pop();
    }

    /// Not shown this frame: no rectangle (so no clicks reach it or
    /// anything inside it), and it draws afresh when it comes back.
    fn hide(&self) {
        self.walk(&mut |node, _| {
            node.rect.set(Rect::default());
            node.drawn.set(None);
        });
    }

    /// How many cells of `axis` the content needs, given `width` x
    /// `height` to lay out in (a height of 0: as tall as it likes).
    pub(crate) fn measure(&self, console: &Console, axis: Axis, width: u16, height: u16) -> u16 {
        let extent = |lines: &[Vec<Segment>]| -> u16 {
            match axis {
                Axis::Vertical => lines.len(),
                Axis::Horizontal => lines
                    .iter()
                    .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
                    .max()
                    .unwrap_or(0),
            }
            .min(u16::MAX as usize) as u16
        };
        let along = |w: u16, h: u16| match axis {
            Axis::Vertical => h,
            Axis::Horizontal => w,
        };
        // A child's own extent, as its parent would size it.
        let sized = |child: &Node, w: u16, h: u16| -> u16 {
            let cells = match child.size {
                Size::Fixed(n) => n,
                Size::Percent(p) => (along(w, h) as u32 * p.min(100) as u32 / 100) as u16,
                Size::Auto | Size::Flex(_) => child.measure(console, axis, w, h),
            };
            cells.min(child.max).max(child.min)
        };
        match &mut *self.kind.borrow_mut() {
            Kind::Leaf(draw) => extent(&draw(console, width.max(1), height)),
            Kind::Stack(stack) => {
                let gaps = stack
                    .gap
                    .saturating_mul(stack.children.len().saturating_sub(1) as u16);
                if stack.axis == axis {
                    stack.children.iter().fold(gaps, |sum, child| {
                        sum.saturating_add(sized(child, width, height))
                    })
                } else {
                    stack
                        .children
                        .iter()
                        .map(|child| child.measure(console, axis, width, height))
                        .max()
                        .unwrap_or(0)
                }
            }
            Kind::Grid(grid) => {
                let areas = grid_areas(console, grid, Rect::new(0, 0, width, height), axis);
                areas
                    .iter()
                    .map(|area| match axis {
                        Axis::Vertical => area.bottom(),
                        Axis::Horizontal => area.right(),
                    })
                    .max()
                    .unwrap_or(0)
            }
            Kind::Panel { child, .. } => child
                .measure(
                    console,
                    axis,
                    width.saturating_sub(2),
                    height.saturating_sub(2),
                )
                .saturating_add(2),
            Kind::Pad { child, edges } => {
                let [top, right, bottom, left] = *edges;
                let inner = child.measure(
                    console,
                    axis,
                    width.saturating_sub(left + right),
                    height.saturating_sub(top + bottom),
                );
                match axis {
                    Axis::Vertical => inner.saturating_add(top + bottom),
                    Axis::Horizontal => inner.saturating_add(left + right),
                }
            }
            Kind::Each { list, .. } => {
                list.reconcile();
                match axis {
                    Axis::Vertical => list.children().iter().fold(0u16, |sum, child| {
                        sum.saturating_add(row_height(console, child, width))
                    }),
                    Axis::Horizontal => list
                        .children()
                        .iter()
                        .map(|child| child.measure(console, axis, width, height))
                        .max()
                        .unwrap_or(0),
                }
            }
            Kind::Switch(switch) => {
                switch.read_key();
                switch.settle();
                switch
                    .current()
                    .map_or(0, |child| child.measure(console, axis, width, height))
            }
            Kind::Log(view) => match axis {
                Axis::Vertical => view
                    .log
                    .data
                    .with(|data| data.lines.len())
                    .min(u16::MAX as usize) as u16,
                Axis::Horizontal => width,
            },
            Kind::Host(host) => {
                let context = Context {
                    console,
                    width: width.max(1) as usize,
                    height: if height == 0 {
                        u16::MAX as usize
                    } else {
                        height as usize
                    },
                };
                extent(&host.render(&context).lines)
            }
        }
    }

    /// Lay out and draw into `screen`. A node draws if it is dirty, has
    /// moved or changed size, or `force` says its area was overwritten;
    /// damage records what was written.
    pub(crate) fn draw(
        &self,
        frame: &mut FrameState,
        rect: Rect,
        screen: &mut Screen,
        force: bool,
    ) {
        self.rect.set(rect);
        let moved = self.drawn.get() != Some(rect);
        let dirty = frame.dirty.contains(&self.id);
        let mut kind = self.kind.borrow_mut();
        match &mut *kind {
            Kind::Leaf(draw) => {
                if moved || force || dirty {
                    let console = frame.console;
                    let lines = frame
                        .runtime
                        .observe_node(self.id, || draw(console, rect.width, rect.height));
                    screen.write_lines(rect, &lines);
                    frame.damage.push(rect);
                    frame.drawn += 1;
                }
            }
            Kind::Stack(stack) => {
                let areas = if moved || force || dirty || stack.areas.len() != stack.children.len()
                {
                    let measures = stack.children.iter().any(|c| c.size == Size::Auto);
                    let console = frame.console;
                    let lay_out = || stack_areas(console, stack, rect);
                    if measures {
                        frame.runtime.observe_node(self.id, lay_out)
                    } else {
                        lay_out()
                    }
                } else {
                    stack.areas.clone()
                };
                draw_children(
                    frame,
                    screen,
                    rect,
                    &stack.children,
                    areas,
                    &mut stack.areas,
                    force || moved,
                );
            }
            Kind::Grid(grid) => {
                let areas = if moved || force || dirty || grid.areas.len() != grid.children.len() {
                    let measures = grid
                        .columns
                        .iter()
                        .chain(&grid.rows)
                        .any(|s| *s == Size::Auto);
                    let console = frame.console;
                    let lay_out = || grid_areas(console, grid, rect, Axis::Vertical);
                    if measures {
                        frame.runtime.observe_node(self.id, lay_out)
                    } else {
                        lay_out()
                    }
                } else {
                    grid.areas.clone()
                };
                draw_children(
                    frame,
                    screen,
                    rect,
                    &grid.children,
                    areas,
                    &mut grid.areas,
                    force || moved,
                );
            }
            Kind::Panel { title, child } => {
                let focused = frame.runtime.observe_node(self.id, || {
                    frame.focus_path.with(|path| path.contains(&self.id))
                });
                if moved || force || dirty {
                    let style = frame.theme.border(focused);
                    let title_style = frame.theme.title.clone();
                    let title_style = style.combine(&title_style);
                    screen.write_lines(rect, &border(title, &style, &title_style, rect));
                    frame.damage.push(rect);
                    frame.drawn += 1;
                    child.draw(frame, rect.inner(1), screen, true);
                } else {
                    child.draw(frame, rect.inner(1), screen, false);
                }
            }
            Kind::Pad { child, edges } => {
                let force = force || moved;
                if force {
                    screen.clear(rect);
                    frame.damage.push(rect);
                }
                child.draw(frame, inset(rect, *edges), screen, force);
            }
            Kind::Host(host) => {
                if moved || force || dirty {
                    let context = Context {
                        console: frame.console,
                        width: rect.width as usize,
                        height: rect.height as usize,
                    };
                    let view = host.render(&context);
                    screen.write_lines(rect, &view.lines);
                    self.caret.set(view.cursor.and_then(|(row, column)| {
                        let (row, column) = (row as u16, column as u16);
                        (row < rect.height && column < rect.width)
                            .then_some((rect.x + column, rect.y + row))
                    }));
                    frame.damage.push(rect);
                    frame.drawn += 1;
                }
            }
            Kind::Log(view) => draw_log(self.id, view, frame, rect, screen, moved || force),
            Kind::Each { list, areas } => {
                let mut force = force || moved;
                let pending = list.take_pending();
                if force || dirty || pending || areas.len() != list.children().len() {
                    let console = frame.console;
                    let (reshaped, heights) = frame.runtime.observe_node(self.id, || {
                        let reshaped = list.reconcile() | pending;
                        list.take_pending();
                        let heights: Vec<u16> = list
                            .children()
                            .iter()
                            .map(|child| row_height(console, child, rect.width))
                            .collect();
                        (reshaped, heights)
                    });
                    let mut y = rect.y;
                    let laid: Vec<Rect> = heights
                        .into_iter()
                        .map(|height| {
                            if y >= rect.bottom() {
                                return Rect::default();
                            }
                            let height = height.min(rect.bottom() - y);
                            let area = Rect::new(rect.x, y, rect.width, height);
                            y += height;
                            area
                        })
                        .collect();
                    force = force || reshaped || laid != *areas;
                    *areas = laid;
                }
                if force {
                    screen.clear(rect);
                    frame.damage.push(rect);
                }
                for (child, area) in list.children().iter().zip(areas.iter()) {
                    if area.is_empty() {
                        // Below the viewport: not drawn, so it draws afresh
                        // whenever it comes into view.
                        child.hide();
                    } else {
                        child.draw(frame, *area, screen, force);
                    }
                }
            }
            Kind::Switch(switch) => {
                if moved || force || dirty || switch.current().is_none() {
                    frame.runtime.observe_node(self.id, || switch.read_key());
                    frame.runtime.untracked(|| switch.settle());
                }
                let changed = switch.take_pending();
                let force = force || moved || changed;
                if force {
                    screen.clear(rect);
                    frame.damage.push(rect);
                }
                if let Some(child) = switch.current() {
                    child.draw(frame, rect, screen, force);
                }
            }
        }
        self.drawn.set(Some(rect));
    }

    /// Forget this subtree's subscriptions (it left the tree), including
    /// the nodes a [`switch`] keeps hidden.
    pub(crate) fn forget(&self, runtime: &Runtime) {
        runtime.forget(self.id);
        match &*self.kind.borrow() {
            Kind::Leaf(_) | Kind::Log(_) | Kind::Host(_) => {}
            Kind::Stack(stack) => stack.children.iter().for_each(|c| c.forget(runtime)),
            Kind::Grid(grid) => grid.children.iter().for_each(|c| c.forget(runtime)),
            Kind::Panel { child, .. } | Kind::Pad { child, .. } => child.forget(runtime),
            Kind::Each { list, .. } => list.children().iter().for_each(|c| c.forget(runtime)),
            Kind::Switch(switch) => switch.all().iter().for_each(|c| c.forget(runtime)),
        }
    }
}

/// A row's height in an [`each`] list: its fixed size, its content for
/// [`Size::Auto`], otherwise one row.
fn row_height(console: &Console, child: &Node, width: u16) -> u16 {
    let height = match child.size {
        Size::Fixed(n) => n,
        Size::Auto => child.measure(console, Axis::Vertical, width, 0),
        _ => 1,
    };
    height.min(child.max).max(child.min)
}

/// Draw `children` into `areas`. When the areas differ from the last
/// layout (`old`), the container's rectangle is cleared, so gaps and
/// space no child covers do not keep stale cells, and every child draws.
fn draw_children(
    frame: &mut FrameState,
    screen: &mut Screen,
    rect: Rect,
    children: &[Node],
    areas: Vec<Rect>,
    old: &mut Vec<Rect>,
    force: bool,
) {
    let relaid = !old.is_empty() && *old != areas;
    let force = force || relaid;
    if relaid {
        screen.clear(rect);
        frame.damage.push(rect);
    }
    for (child, area) in children.iter().zip(&areas) {
        if area.is_empty() {
            child.hide();
        } else {
            child.draw(frame, *area, screen, force);
        }
    }
    *old = areas;
}

/// Each child's rectangle in a stack.
fn stack_areas(console: &Console, stack: &Stack, rect: Rect) -> Vec<Rect> {
    let tracks: Vec<Track> = stack
        .children
        .iter()
        .map(|child| {
            let mut track = child.track();
            if track.size == Size::Auto {
                // Content is measured in the whole rectangle across the
                // axis, and unbounded along it.
                track.content = match stack.axis {
                    Axis::Vertical => child.measure(console, Axis::Vertical, rect.width, 0),
                    Axis::Horizontal => {
                        child.measure(console, Axis::Horizontal, rect.width, rect.height)
                    }
                };
            }
            track
        })
        .collect();
    let (start, total) = match stack.axis {
        Axis::Vertical => (rect.y, rect.height),
        Axis::Horizontal => (rect.x, rect.width),
    };
    let sizes = solve(total, stack.gap, &tracks);
    offsets(start, stack.gap, &sizes)
        .into_iter()
        .zip(sizes)
        .map(|(at, n)| match stack.axis {
            Axis::Vertical => Rect::new(rect.x, at, rect.width, n),
            Axis::Horizontal => Rect::new(at, rect.y, n, rect.height),
        })
        .collect()
}

/// Each child's rectangle in a grid. Columns are solved first, then rows,
/// so a row of [`Size::Auto`] knows how wide its children are. `axis`
/// is [`Axis::Horizontal`] when measuring the grid's own width, which
/// leaves flexible columns at their content.
fn grid_areas(console: &Console, grid: &Grid, rect: Rect, axis: Axis) -> Vec<Rect> {
    let columns = grid.columns.len().max(1);
    let spans: Vec<(u16, u16)> = grid.children.iter().map(|c| c.span).collect();
    let (places, rows) = place(columns, &spans);
    let (row_gap, column_gap) = grid.gap;
    let span_length = |sizes: &[u16], from: usize, n: usize, gap: u16| -> u16 {
        sizes[from..(from + n).min(sizes.len())]
            .iter()
            .fold(gap.saturating_mul(n.saturating_sub(1) as u16), |sum, s| {
                sum.saturating_add(*s)
            })
    };
    // Columns.
    let column_tracks: Vec<Track> = (0..columns)
        .map(|c| {
            let size = grid.columns.get(c).copied().unwrap_or(Size::Flex(1));
            let mut track = Track::new(size);
            let content = matches!(size, Size::Auto)
                || (axis == Axis::Horizontal && matches!(size, Size::Flex(_)));
            if content {
                track.size = Size::Auto;
                track.content = places
                    .iter()
                    .zip(&grid.children)
                    .filter(|(p, _)| p.column == c && p.columns == 1)
                    .map(|(_, child)| {
                        child.measure(console, Axis::Horizontal, rect.width, rect.height)
                    })
                    .max()
                    .unwrap_or(0);
            }
            track
        })
        .collect();
    let widths = solve(rect.width, column_gap, &column_tracks);
    let xs = offsets(rect.x, column_gap, &widths);
    // Rows.
    let row_size = |r: usize| match grid.rows.get(r).or(grid.rows.last()) {
        Some(size) => *size,
        None => Size::Flex(1),
    };
    let row_tracks: Vec<Track> = (0..rows)
        .map(|r| {
            let size = row_size(r);
            let mut track = Track::new(size);
            // A grid measured for its height sizes flexible rows by content.
            let content =
                matches!(size, Size::Auto) || (rect.height == 0 && matches!(size, Size::Flex(_)));
            if content {
                track.size = Size::Auto;
                track.content = places
                    .iter()
                    .zip(&grid.children)
                    .filter(|(p, _)| p.row == r && p.rows == 1)
                    .map(|(p, child)| {
                        let width = span_length(&widths, p.column, p.columns, column_gap);
                        child.measure(console, Axis::Vertical, width, 0)
                    })
                    .max()
                    .unwrap_or(0);
            }
            track
        })
        .collect();
    let total_height = if rect.height == 0 {
        u16::MAX
    } else {
        rect.height
    };
    let heights = solve(total_height, row_gap, &row_tracks);
    let ys = offsets(rect.y, row_gap, &heights);
    places
        .iter()
        .map(|p| {
            let width = span_length(&widths, p.column, p.columns, column_gap);
            let height = span_length(&heights, p.row, p.rows, row_gap);
            let area = Rect::new(xs[p.column], ys[p.row], width, height);
            if rect.height == 0 {
                area
            } else {
                area.intersection(rect)
            }
        })
        .collect()
}

/// `rect` without `edges` (top, right, bottom, left).
fn inset(rect: Rect, [top, right, bottom, left]: [u16; 4]) -> Rect {
    Rect::new(
        rect.x.saturating_add(left),
        rect.y.saturating_add(top),
        rect.width.saturating_sub(left.saturating_add(right)),
        rect.height.saturating_sub(top.saturating_add(bottom)),
    )
}

/// A log draws only what arrived since its last frame, moving what is on
/// screen up, unless it moved or was overwritten.
fn draw_log(
    id: NodeId,
    view: &mut crate::log::LogView,
    frame: &mut FrameState,
    rect: Rect,
    screen: &mut Screen,
    full: bool,
) {
    let total = frame
        .runtime
        .observe_node(id, || view.log.data.with(|data| data.total));
    let height = rect.height as usize;
    let arrived = view
        .drawn_total
        .map(|drawn| total.saturating_sub(drawn) as usize);
    view.log.data.with_untracked(|data| {
        let render = |from: usize| -> Vec<Vec<Segment>> {
            data.lines
                .range(from..)
                .map(|line| crate::log::render_line(frame.console, line, rect.width))
                .collect()
        };
        match arrived {
            Some(0) if !full => {}
            Some(n) if !full && n < height => {
                let len = data.lines.len();
                let shown = view.shown;
                if len == shown + n && shown + n <= height {
                    // Room left and nothing dropped: the new lines go under
                    // the old.
                    let rows = Rect::new(rect.x, rect.y + shown as u16, rect.width, n as u16);
                    screen.write_lines(rows, &render(shown));
                    frame.damage.push(rows);
                } else if shown == height && len >= height {
                    // Full: scroll what is on screen up, and render only the
                    // new lines into the rows that opened.
                    screen.scroll_up(rect, n as u16);
                    let rows = Rect::new(rect.x, rect.bottom() - n as u16, rect.width, n as u16);
                    screen.write_lines(rows, &render(len - n));
                    frame.damage.push(rect);
                } else {
                    // Lines were dropped from a log shorter than the view:
                    // draw what it keeps.
                    screen.write_lines(rect, &render(len.saturating_sub(height)));
                    frame.damage.push(rect);
                }
                frame.drawn += 1;
            }
            _ => {
                screen.write_lines(rect, &render(data.lines.len().saturating_sub(height)));
                frame.damage.push(rect);
                frame.drawn += 1;
            }
        }
    });
    view.drawn_total = Some(total);
    view.shown = view
        .log
        .data
        .with_untracked(|data| data.lines.len())
        .min(height);
}

/// What a frame needs while it draws.
pub(crate) struct FrameState<'a> {
    pub console: &'a Console,
    pub runtime: &'a Runtime,
    pub dirty: HashSet<NodeId>,
    pub damage: Vec<Rect>,
    pub focus_path: Signal<Vec<NodeId>>,
    pub theme: &'a crate::app::Theme,
    /// Nodes drawn this frame.
    pub drawn: usize,
}

/// A rounded border round `rect`, with `title` in the top edge; the inside
/// is left to the child.
fn border(title: &str, style: &Style, title_style: &Style, rect: Rect) -> Vec<Vec<Segment>> {
    let (w, h) = (rect.width as usize, rect.height as usize);
    if w < 2 || h < 2 {
        return Vec::new();
    }
    let edge = |s: String| Segment::new(s, Some(style.clone()));
    let title = if title.is_empty() {
        String::new()
    } else {
        format!(" {title} ")
    };
    let room = w.saturating_sub(4);
    let title = rich::cells::set_cell_size(&title, rich::cells::cell_len(&title).min(room));
    let title_len = rich::cells::cell_len(&title);
    let mut lines = Vec::with_capacity(h);
    if w < 4 {
        // Too narrow for a title: a plain box.
        lines.push(vec![edge(format!("╭{}╮", "─".repeat(w - 2)))]);
    } else {
        // `title_len` is at most `w - 4`, so one dash always follows it.
        lines.push(vec![
            edge("╭─".into()),
            Segment::new(title, Some(title_style.clone())),
            edge(format!("{}╮", "─".repeat(w - 3 - title_len))),
        ]);
    }
    for _ in 0..h - 2 {
        lines.push(vec![
            edge("│".into()),
            Segment::new(" ".repeat(w - 2), None),
            edge("│".into()),
        ]);
    }
    lines.push(vec![edge(format!("╰{}╯", "─".repeat(w - 2)))]);
    lines
}

// Builders.

/// A node that draws itself from rendered lines; `draw` gets the console,
/// the width and the height. Signals it reads make it draw again. A height
/// of 0 asks how tall the node would like to be: a parent laying out a
/// [`Size::Auto`] child measures it so.
pub fn leaf(draw: impl Fn(&Console, u16, u16) -> Vec<Vec<Segment>> + 'static) -> Node {
    Node::new(Kind::Leaf(Box::new(draw)))
}

/// Console markup, from a closure that may read signals:
/// `text(move || format!("[b]{}[/] items", count.get()))`. The
/// [`text!`](crate::text!) macro writes the closure for you.
pub fn text(markup: impl Fn() -> String + 'static) -> Node {
    leaf(move |console, width, _| {
        let markup = markup();
        let text = rich::Text::from_markup(&markup).unwrap_or_else(|_| rich::Text::new(markup));
        console.render_lines(
            &text,
            &console.options().update_width(width.max(1) as usize),
            false,
        )
    })
}

/// Console markup that never changes.
pub fn label(markup: impl Into<String>) -> Node {
    let markup = markup.into();
    text(move || markup.clone())
}

/// Any rich renderable, built by `f` (which may read signals) and rendered
/// at the node's size: a `Table`, `Markdown`, `Syntax`, `Panel`, a chart.
pub fn renderable<R: Renderable + 'static>(f: impl Fn() -> R + 'static) -> Node {
    leaf(move |console, width, height| {
        let options = console.options().update_width(width.max(1) as usize);
        // Height 0 is a measurement: as tall as the renderable likes.
        let options = if height == 0 {
            options
        } else {
            options.update_height(height as usize)
        };
        console.render_lines(&f(), &options, false)
    })
}

/// A `rich-interact` component as a node: an `Input`, a `Select`, a
/// `Form`, a `Pager`, or one of your own. It takes keys while it has the
/// focus (keys it does not use bubble on), shows its text caret, and calls
/// `on_done` with its answer (Enter in an `Input`, a pick in a `Select`).
///
/// ```
/// use intuituive::prelude::*;
/// use rich_interact::Input;
///
/// let app = App::new(|| {
///     let name = signal(String::new());
///     column([
///         component(Input::new("Name"), move |value, _| name.set(value)),
///         text!("Hello, {name}"),
///     ])
///     .on_key("esc", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["A", "d", "a", "enter", "esc"], 30, 3).unwrap();
/// assert!(screen.iter().any(|l| l.contains("Hello, Ada")), "{screen:?}");
/// ```
pub fn component<C: Component + 'static>(
    component: C,
    on_done: impl FnMut(C::Output, &mut Ctx) + 'static,
) -> Node {
    let mut node = Node::new(Kind::Host(Box::new(Host {
        component,
        on_done: Box::new(on_done),
        on_cancel: None,
    })));
    node.focusable = true;
    node
}

/// Children one above the other.
pub fn column(children: impl IntoIterator<Item = Node>) -> Node {
    stack(Axis::Vertical, children)
}

/// Children side by side.
pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    stack(Axis::Horizontal, children)
}

fn stack(axis: Axis, children: impl IntoIterator<Item = Node>) -> Node {
    Node::new(Kind::Stack(Stack {
        axis,
        gap: 0,
        children: children.into_iter().collect(),
        areas: Vec::new(),
    }))
}

/// Children in a grid with `columns`, filled row by row. A child can span
/// several columns and rows with [`Node::span`]; it takes the first place
/// where it fits. Rows share the height evenly unless sized with
/// [`Node::rows`]; [`Node::gap`] spaces the cells.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     grid(
///         [Size::Flex(1), Size::Flex(1)],
///         [
///             label("CPU").panel("1"),
///             label("Memory").panel("2"),
///             label("Disk").panel("3").span(2, 1),
///         ],
///     )
///     .rows([Size::Fixed(3)])
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 20, 6).unwrap();
/// assert!(screen[1].contains("CPU") && screen[1].contains("Memory"));
/// assert!(screen[4].contains("Disk"));
/// ```
pub fn grid(
    columns: impl IntoIterator<Item = Size>,
    children: impl IntoIterator<Item = Node>,
) -> Node {
    Node::new(Kind::Grid(Grid {
        columns: columns.into_iter().collect(),
        rows: Vec::new(),
        gap: (0, 0),
        children: children.into_iter().collect(),
        areas: Vec::new(),
    }))
}

/// A keyed list reconciled against its keys.
pub(crate) trait Reconcile {
    /// Bring the children in line with the keys; whether the list changed.
    fn reconcile(&mut self) -> bool;
    /// Whether a reconcile outside a draw (a parent measuring the list)
    /// changed it since the list last drew.
    fn take_pending(&mut self) -> bool;
    fn children(&self) -> &[Node];
}

struct Each<K, F> {
    keys: Rc<dyn Fn() -> Vec<K>>,
    build: F,
    order: Vec<K>,
    children: Vec<Node>,
    pending: bool,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node> Reconcile for Each<K, F> {
    fn reconcile(&mut self) -> bool {
        let keys = (self.keys)();
        if keys == self.order {
            return false;
        }
        let mut old: HashMap<K, Node> = self.order.drain(..).zip(self.children.drain(..)).collect();
        for key in &keys {
            let node = old.remove(key).unwrap_or_else(|| (self.build)(key.clone()));
            self.children.push(node);
        }
        let runtime = Runtime::current();
        for (_, gone) in old {
            gone.forget(&runtime);
        }
        self.order = keys;
        self.pending = true;
        true
    }

    fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }

    fn children(&self) -> &[Node] {
        &self.children
    }
}

/// One child per key, top to bottom, each one row tall unless it is
/// [`fixed`](Node::fixed) to more or [`auto`](Node::auto) (as tall as its
/// content). `keys` may read signals; a key that stays
/// keeps its node (its state, its focus, its cached drawing) however the
/// list is reordered, and only new keys are built (`build` gets a clone of
/// the key).
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let items = signal(vec!["a", "b"]);
///     each(move || items.get(), |item| text!("• {item}"))
/// });
/// ```
pub fn each<K, F>(keys: impl Fn() -> Vec<K> + 'static, build: F) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K) -> Node + 'static,
{
    Node::new(Kind::Each {
        list: Box::new(Each {
            keys: Rc::new(keys),
            build,
            order: Vec::new(),
            children: Vec::new(),
            pending: false,
        }),
        areas: Vec::new(),
    })
}

/// One child shown at a time, chosen by a key.
pub(crate) trait Switching {
    /// Read the key (the switch subscribes to what it reads); whether it
    /// changed.
    fn read_key(&mut self) -> bool;
    /// Whether a key read outside a draw (a parent measuring the switch)
    /// changed it since the switch last drew.
    fn take_pending(&mut self) -> bool;
    /// Build the child for the key if it is new.
    fn settle(&mut self);
    fn current(&self) -> Option<&Node>;
    /// Every child built, shown or not.
    fn all(&self) -> Vec<&Node>;
}

struct Switch<K, F> {
    key: Box<dyn Fn() -> K>,
    build: F,
    built: HashMap<K, Node>,
    current: Option<K>,
    pending: bool,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node> Switching for Switch<K, F> {
    fn read_key(&mut self) -> bool {
        let key = (self.key)();
        if self.current.as_ref() == Some(&key) {
            return false;
        }
        self.current = Some(key);
        self.pending = true;
        true
    }

    fn take_pending(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }

    fn settle(&mut self) {
        if let Some(key) = &self.current {
            if !self.built.contains_key(key) {
                let node = (self.build)(key.clone());
                self.built.insert(key.clone(), node);
            }
        }
    }

    fn current(&self) -> Option<&Node> {
        self.current.as_ref().and_then(|key| self.built.get(key))
    }

    fn all(&self) -> Vec<&Node> {
        self.built.values().collect()
    }
}

/// The child for the current `key`, built by `build` the first time the key
/// is shown and kept while other keys are: tabs, the steps of a wizard, a
/// detail pane per item. `key` may read signals; when it changes, the
/// switch shows the other child, which keeps its state (and its focus
/// order) from when it was last shown. Only the shown child is in the Tab
/// order and takes clicks.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let tab = signal(0usize);
///     column([
///         label("[b]1[/] Overview · [b]2[/] Logs").fixed(1),
///         switch(move || tab.get(), |tab| match tab {
///             0 => label("All systems go"),
///             _ => label("No logs yet"),
///         }),
///     ])
///     .on_key("1", move |_| tab.set(0))
///     .on_key("2", move |_| tab.set(1))
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["2", "q"], 30, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "No logs yet");
/// ```
pub fn switch<K, F>(key: impl Fn() -> K + 'static, build: F) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K) -> Node + 'static,
{
    Node::new(Kind::Switch(Box::new(Switch {
        key: Box::new(key),
        build,
        built: HashMap::new(),
        current: None,
        pending: false,
    })))
}

/// Run `f` on the node with `id`.
pub(crate) fn with_node(root: &Node, id: NodeId, f: &mut dyn FnMut(&Node)) {
    let mut done = false;
    root.walk(&mut |node, _| {
        if !done && node.id == id {
            f(node);
            done = true;
        }
    });
}
