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
use rich_interact::Key;

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
}

impl Node {
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
