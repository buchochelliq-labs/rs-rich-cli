//! Nodes: the retained tree an app is made of.
//!
//! A [`Node`] is built once and kept. It remembers the rectangle it was
//! laid out in, so the app can route a click to it without the author
//! storing anything, and it draws again only when a [signal](crate::signal)
//! it read changed, or its size did. A frame writes the nodes that drew
//! into the [`Screen`] and reports their rectangles as damage.
//!
//! Builders:
//!
//! - [`text`] and the [`text!`](crate::text!) macro: console markup that may
//!   read signals; [`label`] for markup that never changes;
//! - [`renderable`]: any rich renderable (a `Table`, `Markdown`, `Syntax`,
//!   a chart), rebuilt when the signals it read change;
//! - [`column`] and [`row`]: children laid out along an axis, each sized
//!   [`Size::Fixed`], [`Size::Flex`] or [`Size::Percent`];
//! - [`each`]: one child per key of a list, kept by key;
//! - [`Node::panel`]: a rounded border with a title, highlighted while the
//!   focus is inside it.
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
use crate::reactive::{next_node, NodeId, Runtime, Signal};
use crate::screen::{Rect, Screen};

/// How much of its parent's axis a child takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// Exactly this many cells.
    Fixed(u16),
    /// This percentage of the parent.
    Percent(u16),
    /// A share, by weight, of what the fixed and percentage children leave.
    Flex(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

type Draw = Box<dyn Fn(&Console, u16, u16) -> Vec<Vec<Segment>>>;
pub(crate) type Handler = Box<dyn FnMut(&mut Ctx)>;

pub(crate) enum Kind {
    Leaf(Draw),
    Stack(Axis, Vec<Node>),
    Panel { title: String, child: Box<Node> },
    Each(Box<dyn Reconcile>),
    Log(crate::log::LogView),
    Host(Box<dyn Hosted>),
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
    pub(crate) kind: RefCell<Kind>,
    /// Where it was last laid out.
    pub(crate) rect: Cell<Rect>,
    /// The size it last drew at; `None` before its first draw.
    drawn: Cell<Option<(u16, u16)>>,
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
    /// highlighted while the focus is on the node or inside it.
    pub fn panel(self, title: &str) -> Node {
        let size = self.size;
        Node::new(Kind::Panel {
            title: title.to_string(),
            child: Box::new(self),
        })
        .size(size)
    }

    /// Where the node was last laid out.
    pub fn rect(&self) -> Rect {
        self.rect.get()
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Visit this node and every node inside it, depth first.
    pub(crate) fn walk(&self, f: &mut dyn FnMut(&Node, &[NodeId])) {
        let mut path = Vec::new();
        self.walk_inner(&mut path, f);
    }

    fn walk_inner(&self, path: &mut Vec<NodeId>, f: &mut dyn FnMut(&Node, &[NodeId])) {
        path.push(self.id);
        f(self, path);
        match &*self.kind.borrow() {
            Kind::Leaf(_) => {}
            Kind::Stack(_, children) => {
                for child in children {
                    child.walk_inner(path, f);
                }
            }
            Kind::Panel { child, .. } => child.walk_inner(path, f),
            Kind::Each(list) => {
                for child in list.children() {
                    child.walk_inner(path, f);
                }
            }
            Kind::Log(_) | Kind::Host(_) => {}
        }
        path.pop();
    }

    /// Lay out and draw into `screen`. A node draws if it is dirty, has a
    /// new size, or `force` says its area was overwritten; damage records
    /// what was written.
    pub(crate) fn draw(
        &self,
        frame: &mut FrameState,
        rect: Rect,
        screen: &mut Screen,
        force: bool,
    ) {
        self.rect.set(rect);
        let resized = self.drawn.get() != Some((rect.width, rect.height));
        let mut kind = self.kind.borrow_mut();
        match &mut *kind {
            Kind::Leaf(draw) => {
                if resized || force || frame.dirty.contains(&self.id) {
                    let console = frame.console;
                    let lines = frame
                        .runtime
                        .observe_node(self.id, || draw(console, rect.width, rect.height));
                    screen.write_lines(rect, &lines);
                    frame.damage.push(rect);
                    frame.drawn += 1;
                }
            }
            Kind::Stack(axis, children) => {
                let force = force || resized;
                for (child, area) in children.iter().zip(split(*axis, children, rect)) {
                    child.draw(frame, area, screen, force);
                }
            }
            Kind::Panel { title, child } => {
                let focused = frame.runtime.observe_node(self.id, || {
                    frame.focus_path.with(|path| path.contains(&self.id))
                });
                if resized || force || frame.dirty.contains(&self.id) {
                    let style = frame.theme.border(focused);
                    let title_style = style.combine(&Style::parse("bold").unwrap());
                    screen.write_lines(rect, &border(title, &style, &title_style, rect));
                    frame.damage.push(rect);
                    frame.drawn += 1;
                    child.draw(frame, rect.inner(1), screen, true);
                } else {
                    child.draw(frame, rect.inner(1), screen, false);
                }
            }
            Kind::Host(host) => {
                if resized || force || frame.dirty.contains(&self.id) {
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
            Kind::Log(view) => {
                let total = frame
                    .runtime
                    .observe_node(self.id, || view.log.data.with(|data| data.total));
                let height = rect.height as usize;
                let arrived = view
                    .drawn_total
                    .map(|drawn| total.saturating_sub(drawn) as usize);
                view.log.data.with_untracked(|data| {
                    match arrived {
                        Some(0) if !(resized || force) => {}
                        Some(n) if !(resized || force) && n < height => {
                            let len = data.lines.len();
                            let before = len.saturating_sub(n);
                            let render = |from: usize| -> Vec<Vec<Segment>> {
                                data.lines
                                    .range(from..)
                                    .map(|line| {
                                        crate::log::render_line(frame.console, line, rect.width)
                                    })
                                    .collect()
                            };
                            if before >= height {
                                // Full: scroll what is on screen up, and render
                                // only the new lines into the rows that opened.
                                screen.scroll_up(rect, n as u16);
                                let rows = Rect::new(
                                    rect.x,
                                    rect.bottom() - n as u16,
                                    rect.width,
                                    n as u16,
                                );
                                screen.write_lines(rows, &render(before));
                                frame.damage.push(rect);
                            } else {
                                // Room left: the new lines go under the old.
                                let rows =
                                    Rect::new(rect.x, rect.y + before as u16, rect.width, n as u16);
                                screen.write_lines(rows, &render(before));
                                frame.damage.push(rows);
                            }
                            frame.drawn += 1;
                        }
                        _ => {
                            let start = data.lines.len().saturating_sub(height);
                            let lines: Vec<Vec<Segment>> = data
                                .lines
                                .range(start..)
                                .map(|line| {
                                    crate::log::render_line(frame.console, line, rect.width)
                                })
                                .collect();
                            screen.write_lines(rect, &lines);
                            frame.damage.push(rect);
                            frame.drawn += 1;
                        }
                    }
                });
                view.drawn_total = Some(total);
            }
            Kind::Each(list) => {
                let reshaped = frame
                    .runtime
                    .observe_node(self.id, || list.reconcile(frame.runtime));
                let force = force || resized || reshaped;
                if force {
                    screen.clear(rect);
                    frame.damage.push(rect);
                }
                let mut y = rect.y;
                for child in list.children() {
                    let height = match child.size {
                        Size::Fixed(n) => n,
                        _ => 1,
                    };
                    if y >= rect.bottom() {
                        child.rect.set(Rect::default());
                        continue;
                    }
                    let height = height.min(rect.bottom() - y);
                    child.draw(
                        frame,
                        Rect::new(rect.x, y, rect.width, height),
                        screen,
                        force,
                    );
                    y += height;
                }
            }
        }
        self.drawn.set(Some((rect.width, rect.height)));
    }

    /// Forget this subtree's subscriptions (it left the tree).
    pub(crate) fn forget(&self, runtime: &Runtime) {
        self.walk(&mut |node, _| runtime.forget(node.id));
    }
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

/// Each child's rectangle along `axis`: fixed and percentage sizes first,
/// then the rest shared by flex weight, the remainder to the last flexible
/// child, so the children always fill the parent exactly.
fn split(axis: Axis, children: &[Node], rect: Rect) -> Vec<Rect> {
    let total = if axis == Axis::Vertical {
        rect.height
    } else {
        rect.width
    } as u32;
    let mut sizes: Vec<u32> = children
        .iter()
        .map(|c| match c.size {
            Size::Fixed(n) => n as u32,
            Size::Percent(p) => total * p.min(100) as u32 / 100,
            Size::Flex(_) => 0,
        })
        .collect();
    let mut taken: u32 = sizes.iter().sum();
    // Too much asked for: shrink from the end.
    for size in sizes.iter_mut().rev() {
        if taken <= total {
            break;
        }
        let cut = (*size).min(taken - total);
        *size -= cut;
        taken -= cut;
    }
    let weights: u32 = children
        .iter()
        .map(|c| match c.size {
            Size::Flex(w) => w as u32,
            _ => 0,
        })
        .sum();
    let free = total - taken;
    let last_flex = children
        .iter()
        .rposition(|c| matches!(c.size, Size::Flex(_)));
    let mut left = free;
    for (i, child) in children.iter().enumerate() {
        if let Size::Flex(w) = child.size {
            sizes[i] = if Some(i) == last_flex {
                left
            } else {
                let n = free * w as u32 / weights.max(1);
                left -= n;
                n
            };
        }
    }
    let mut at = if axis == Axis::Vertical {
        rect.y
    } else {
        rect.x
    };
    sizes
        .into_iter()
        .map(|n| {
            let n = n as u16;
            let area = match axis {
                Axis::Vertical => Rect::new(rect.x, at, rect.width, n),
                Axis::Horizontal => Rect::new(at, rect.y, n, rect.height),
            };
            at += n;
            area
        })
        .collect()
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
    lines.push(vec![
        edge("╭─".into()),
        Segment::new(title, Some(title_style.clone())),
        edge(format!("{}╮", "─".repeat(w - 3 - title_len))),
    ]);
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
/// the width and the height. Signals it reads make it draw again.
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
        let options = console
            .options()
            .update_dimensions(width.max(1) as usize, height.max(1) as usize);
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
    Node::new(Kind::Stack(Axis::Vertical, children.into_iter().collect()))
}

/// Children side by side.
pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    Node::new(Kind::Stack(
        Axis::Horizontal,
        children.into_iter().collect(),
    ))
}

/// A keyed list reconciled against its keys.
pub(crate) trait Reconcile {
    /// Bring the children in line with the keys; whether the list changed.
    fn reconcile(&mut self, runtime: &Runtime) -> bool;
    fn children(&self) -> &[Node];
}

struct Each<K, F> {
    keys: Rc<dyn Fn() -> Vec<K>>,
    build: F,
    order: Vec<K>,
    children: Vec<Node>,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node> Reconcile for Each<K, F> {
    fn reconcile(&mut self, runtime: &Runtime) -> bool {
        let keys = (self.keys)();
        if keys == self.order {
            return false;
        }
        let mut old: HashMap<K, Node> = self.order.drain(..).zip(self.children.drain(..)).collect();
        for key in &keys {
            let node = old.remove(key).unwrap_or_else(|| (self.build)(key.clone()));
            self.children.push(node);
        }
        for (_, gone) in old {
            gone.forget(runtime);
        }
        self.order = keys;
        true
    }

    fn children(&self) -> &[Node] {
        &self.children
    }
}

/// One child per key, top to bottom, each one row tall unless it is
/// [`fixed`](Node::fixed) to more. `keys` may read signals; a key that stays
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
    Node::new(Kind::Each(Box::new(Each {
        keys: Rc::new(keys),
        build,
        order: Vec::new(),
        children: Vec::new(),
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
