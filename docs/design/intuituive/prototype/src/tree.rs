//! A retained tree of nodes, rendered with fine-grained invalidation.
//!
//! Every [`Node`] keeps the lines it last rendered and the size it rendered
//! them at. A frame renders the tree top-down; a node renders again only if
//!
//! - a [signal](crate::reactive) it read last time has been written,
//! - its size changed, or
//! - it is a container and one of its children rendered again.
//!
//! Otherwise it hands back its cached lines (an `Rc`, so a clean subtree
//! costs a pointer copy). A tick that changes one label renders that label
//! and recomposes its ancestors; nothing else is touched.
//!
//! Nodes have identity: a [`keyed`] list reconciles its children by key, so
//! an item that moves keeps its node, its cache and its subscriptions, and a
//! removed item drops its subscriptions.
//!
//! [`App`] is the root and implements [`rich_interact::Component`], so a tree
//! runs under the existing event loop, the blocking driver and the headless
//! driver, and nests inside the 0.0.14 composition containers. It is built
//! on `rich-interact`, not beside it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use rich::{Console, Segment, Style};
use rich_interact::{Context, Event, Flow, View};

use crate::reactive::{self, NodeId, Signal};

/// Rendered rows, shared between a node's cache and its parent's.
pub type Lines = Rc<Vec<Vec<Segment>>>;

fn next_id() -> NodeId {
    thread_local!(static NEXT: Cell<NodeId> = const { Cell::new(1) });
    NEXT.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

/// How much of its parent's axis a child takes.
#[derive(Clone, Copy, Debug)]
pub enum Size {
    /// Exactly this many cells.
    Fixed(usize),
    /// A share of what the fixed children leave, by weight.
    Flex(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

type Draw = Box<dyn Fn(&Console, usize, usize) -> Vec<Vec<Segment>>>;

enum Kind {
    /// Draws itself; reads signals while it does.
    Leaf(Draw),
    Stack(Axis, Vec<Node>),
    /// A rounded border with a title, round one child.
    Border(String, Style, Box<Node>),
    /// Children rebuilt from a list of keys, one row each.
    Keyed(Box<dyn Reconcile>),
}

/// One node of the tree.
pub struct Node {
    id: NodeId,
    size: Size,
    kind: RefCell<Kind>,
    /// (width, height, lines) of the last render.
    cache: RefCell<Option<(usize, usize, Lines)>>,
}

/// Counters a frame fills in, for the spike's measurements.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub rendered: usize,
    pub reused: usize,
}

struct Frame<'a> {
    console: &'a Console,
    /// Render every node, as an immediate-mode UI does: the benchmarks'
    /// baseline for "re-render everything each frame".
    immediate: bool,
    dirty: &'a HashSet<NodeId>,
    stats: &'a Cell<Stats>,
}

impl Node {
    fn new(kind: Kind) -> Node {
        Node {
            id: next_id(),
            size: Size::Flex(1),
            kind: RefCell::new(kind),
            cache: RefCell::new(None),
        }
    }

    /// Give the node a size along its parent's axis.
    pub fn size(mut self, size: Size) -> Node {
        self.size = size;
        self
    }

    #[allow(dead_code)]
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Render at `width` x `height`; returns the lines and whether they are
    /// new (so the parent knows to recompose).
    fn render(&self, frame: &Frame, width: usize, height: usize) -> (Lines, bool) {
        let cached = self.cache.borrow().clone();
        let same_size = matches!(&cached, Some((w, h, _)) if *w == width && *h == height);
        let mut kind = self.kind.borrow_mut();
        // A container renders its children first: it is clean only if they
        // all are.
        let children_changed = match &mut *kind {
            Kind::Leaf(_) => false,
            _ => self.children_changed(&mut kind, frame, width, height),
        };
        if !frame.immediate && same_size && !children_changed && !frame.dirty.contains(&self.id) {
            let mut stats = frame.stats.get();
            stats.reused += 1;
            frame.stats.set(stats);
            return (cached.unwrap().2, false);
        }
        let lines = match &mut *kind {
            Kind::Leaf(draw) => {
                let lines = reactive::observe(self.id, || draw(frame.console, width, height));
                Segment::set_shape(lines, width, height)
            }
            // The children rendered above; compose from their caches.
            Kind::Stack(axis, children) => compose(*axis, children, height),
            Kind::Border(title, style, child) => {
                border(title, style, &child.cached(), width, height)
            }
            Kind::Keyed(list) => {
                let rows: Vec<Lines> = list
                    .children()
                    .iter()
                    .take(height)
                    .map(|c| c.cached())
                    .collect();
                let mut lines: Vec<Vec<Segment>> =
                    rows.iter().flat_map(|r| r.iter().cloned()).collect();
                lines.truncate(height);
                Segment::set_shape(lines, width, height)
            }
        };
        let mut stats = frame.stats.get();
        stats.rendered += 1;
        frame.stats.set(stats);
        let lines = Rc::new(lines);
        *self.cache.borrow_mut() = Some((width, height, lines.clone()));
        (lines, true)
    }

    /// Render a container's children (reconciling a keyed list first) and
    /// report whether any of them changed.
    fn children_changed(
        &self,
        kind: &mut Kind,
        frame: &Frame,
        width: usize,
        height: usize,
    ) -> bool {
        match kind {
            Kind::Leaf(_) => false,
            Kind::Stack(axis, children) => {
                let spans = split(*axis, children, width, height);
                let mut changed = false;
                for (child, (w, h)) in children.iter().zip(spans) {
                    changed |= child.render(frame, w, h).1;
                }
                changed
            }
            Kind::Border(_, _, child) => {
                child
                    .render(frame, width.saturating_sub(2), height.saturating_sub(2))
                    .1
            }
            Kind::Keyed(list) => {
                // The key list is the keyed node's own dependency.
                let reshaped = reactive::observe(self.id, || list.reconcile());
                let mut changed = reshaped || frame.dirty.contains(&self.id);
                for child in list.children().iter().take(height) {
                    changed |= child.render(frame, width, 1).1;
                }
                // Rows below the viewport are not drawn, so a change to them
                // would be lost: drop their caches, and they draw afresh when
                // they come into view.
                for child in list.children().iter().skip(height) {
                    child.cache.borrow_mut().take();
                }
                changed
            }
        }
    }

    fn cached(&self) -> Lines {
        self.cache.borrow().as_ref().expect("rendered").2.clone()
    }
}

/// Each child's (width, height) along `axis`: fixed sizes first, then the
/// rest shared by flex weight, the remainder going to the last flex child.
fn split(axis: Axis, children: &[Node], width: usize, height: usize) -> Vec<(usize, usize)> {
    let total = if axis == Axis::Vertical {
        height
    } else {
        width
    };
    let fixed: usize = children
        .iter()
        .map(|c| match c.size {
            Size::Fixed(n) => n,
            Size::Flex(_) => 0,
        })
        .sum();
    let weights: usize = children
        .iter()
        .map(|c| match c.size {
            Size::Flex(w) => w,
            Size::Fixed(_) => 0,
        })
        .sum();
    let free = total.saturating_sub(fixed);
    let mut left = free;
    let last_flex = children
        .iter()
        .rposition(|c| matches!(c.size, Size::Flex(_)));
    let mut out = Vec::with_capacity(children.len());
    // Fixed sizes that add up to more than the parent are cut to what is
    // left, so no child is laid out over its neighbours.
    let mut room = total;
    for (i, child) in children.iter().enumerate() {
        let n = match child.size {
            Size::Fixed(n) => n.min(room),
            Size::Flex(_) if Some(i) == last_flex => left,
            Size::Flex(w) => {
                let n = free * w / weights.max(1);
                left -= n;
                n
            }
        };
        let n = n.min(room);
        room -= n;
        out.push(if axis == Axis::Vertical {
            (width, n)
        } else {
            (n, height)
        });
    }
    out
}

/// Lay the children's cached lines out along `axis`.
fn compose(axis: Axis, children: &[Node], height: usize) -> Vec<Vec<Segment>> {
    let rendered: Vec<Lines> = children.iter().map(Node::cached).collect();
    match axis {
        Axis::Vertical => rendered.iter().flat_map(|l| l.iter().cloned()).collect(),
        Axis::Horizontal => (0..height)
            .map(|row| {
                rendered
                    .iter()
                    .flat_map(|l| l.get(row).cloned().unwrap_or_default())
                    .collect()
            })
            .collect(),
    }
}

/// A rounded border round `inner`, with `title` in the top edge.
fn border(
    title: &str,
    style: &Style,
    inner: &[Vec<Segment>],
    width: usize,
    height: usize,
) -> Vec<Vec<Segment>> {
    if width < 2 || height < 2 {
        return Segment::set_shape(Vec::new(), width, height);
    }
    let edge = |s: String| Segment::new(s, Some(style.clone()));
    let title = format!(" {title} ");
    let title_len = rich::cells::cell_len(&title).min(width.saturating_sub(4));
    let top = if width < 4 {
        vec![edge(format!("╭{}╮", "─".repeat(width - 2)))]
    } else {
        // `title_len` is at most `width - 4`, so one dash always follows it.
        vec![
            edge("╭─".into()),
            Segment::new(
                rich::cells::set_cell_size(&title, title_len),
                Some(style.combine(&Style::parse("bold").unwrap())),
            ),
            edge(format!("{}╮", "─".repeat(width - 3 - title_len))),
        ]
    };
    let mut lines = vec![top];
    for row in inner.iter().take(height - 2) {
        let mut line = vec![edge("│".into())];
        line.extend(row.iter().cloned());
        line.push(edge("│".into()));
        lines.push(line);
    }
    lines.push(vec![edge(format!("╰{}╯", "─".repeat(width - 2)))]);
    Segment::set_shape(lines, width, height)
}

// Builders.

/// A node that draws itself. `draw` may read signals; the node renders
/// again only when one of them changes (or its size does).
pub fn leaf(draw: impl Fn(&Console, usize, usize) -> Vec<Vec<Segment>> + 'static) -> Node {
    Node::new(Kind::Leaf(Box::new(draw)))
}

/// A leaf showing console markup, built by `markup` from signals.
pub fn text(markup: impl Fn() -> String + 'static) -> Node {
    leaf(move |console, width, _| {
        let text = rich::Text::from_markup(&markup()).unwrap_or_else(|_| rich::Text::new(""));
        console.render_lines(&text, &console.options().update_width(width.max(1)), false)
    })
}

pub fn column(children: Vec<Node>) -> Node {
    Node::new(Kind::Stack(Axis::Vertical, children))
}

pub fn row(children: Vec<Node>) -> Node {
    Node::new(Kind::Stack(Axis::Horizontal, children))
}

pub fn bordered(title: &str, child: Node) -> Node {
    let style = Style::parse("blue").unwrap();
    Node::new(Kind::Border(title.to_string(), style, Box::new(child)))
}

/// A keyed list reconciled against `keys`.
trait Reconcile {
    /// Bring the children in line with the keys; whether the list changed.
    fn reconcile(&mut self) -> bool;
    fn children(&self) -> &[Node];
}

struct KeyedList<K, F> {
    keys: Signal<Vec<K>>,
    build: F,
    order: Vec<K>,
    children: Vec<Node>,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(&K) -> Node> Reconcile for KeyedList<K, F> {
    fn reconcile(&mut self) -> bool {
        let keys = self.keys.get();
        if keys == self.order {
            return false;
        }
        let mut old: HashMap<K, Node> = self.order.drain(..).zip(self.children.drain(..)).collect();
        for key in &keys {
            let node = old.remove(key).unwrap_or_else(|| (self.build)(key));
            self.children.push(node);
        }
        for (_, gone) in old {
            gone.forget();
        }
        self.order = keys;
        true
    }

    fn children(&self) -> &[Node] {
        &self.children
    }
}

impl Node {
    /// Drop this subtree's subscriptions (a removed keyed child).
    fn forget(&self) {
        reactive::forget(self.id);
        match &*self.kind.borrow() {
            Kind::Leaf(_) => {}
            Kind::Stack(_, children) => children.iter().for_each(Node::forget),
            Kind::Border(_, _, child) => child.forget(),
            Kind::Keyed(list) => list.children().iter().for_each(Node::forget),
        }
    }
}

/// One row per key in `keys`, built by `build` and kept by key: a key that
/// stays keeps its node (and its cache) however the list is reordered.
pub fn keyed<K: Clone + Eq + Hash + 'static>(
    keys: Signal<Vec<K>>,
    build: impl Fn(&K) -> Node + 'static,
) -> Node {
    Node::new(Kind::Keyed(Box::new(KeyedList {
        keys,
        build,
        order: Vec::new(),
        children: Vec::new(),
    })))
}

type Handler<T> = Box<dyn FnMut(&Event) -> Flow<T>>;

/// The root: a tree and the handler that turns events into signal writes.
pub struct App<T> {
    root: Node,
    on_event: RefCell<Handler<T>>,
    stats: Cell<Stats>,
    immediate: bool,
}

impl<T> App<T> {
    pub fn new(root: Node, on_event: impl FnMut(&Event) -> Flow<T> + 'static) -> App<T> {
        App {
            root,
            on_event: RefCell::new(Box::new(on_event)),
            stats: Cell::new(Stats::default()),
            immediate: false,
        }
    }

    /// Turn caching off: every frame renders every node.
    pub fn immediate(mut self, on: bool) -> App<T> {
        self.immediate = on;
        self
    }

    /// Render a frame directly (the benchmarks call this).
    pub fn frame(&self, console: &Console, width: usize, height: usize) -> Lines {
        let dirty = reactive::take_dirty();
        self.stats.set(Stats::default());
        let frame = Frame {
            console,
            immediate: self.immediate,
            dirty: &dirty,
            stats: &self.stats,
        };
        self.root.render(&frame, width, height).0
    }

    pub fn stats(&self) -> Stats {
        self.stats.get()
    }
}

impl<T> rich_interact::Component for App<T> {
    type Output = T;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<T> {
        (self.on_event.borrow_mut())(event)
    }

    fn render(&self, context: &Context<'_>) -> View {
        View::new(
            self.frame(context.console, context.width, context.height)
                .to_vec(),
        )
    }
}

// The cell-buffer path: the screen is a retained ratatui `Buffer`.
//
// Containers do not compose lines at all. A frame walks the tree with
// rectangles: a leaf that must render (a signal it read changed, or its
// size did) renders and is written into its rectangle; a clean leaf writes
// nothing unless an ancestor overwrote its area (`force`). Every write is
// recorded as damage, and only damaged rectangles are compared with the
// previous screen. Rich renders only what changed; the buffer, the diff and
// the encoder are ratatui's, so a ratatui widget is just another leaf.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// What a buffer frame changed.
pub struct Damage {
    pub rects: Vec<Rect>,
}

impl Node {
    fn size_changed(&self, rect: Rect) -> bool {
        !matches!(&*self.cache.borrow(), Some((w, h, _)) if *w == rect.width as usize && *h == rect.height as usize)
    }

    fn blit(
        &self,
        frame: &Frame,
        rect: Rect,
        buffer: &mut Buffer,
        force: bool,
        damage: &mut Damage,
    ) {
        let resized = self.size_changed(rect);
        let mut kind = self.kind.borrow_mut();
        match &mut *kind {
            Kind::Leaf(draw) => {
                let render = frame.immediate || resized || frame.dirty.contains(&self.id);
                if render {
                    let (w, h) = (rect.width as usize, rect.height as usize);
                    let lines = reactive::observe(self.id, || draw(frame.console, w, h));
                    let lines = Rc::new(Segment::set_shape(lines, w, h));
                    *self.cache.borrow_mut() = Some((w, h, lines));
                    count(frame, true);
                } else {
                    count(frame, false);
                }
                if render || force {
                    write(&self.cached(), rect, buffer, damage);
                }
            }
            Kind::Stack(axis, children) => {
                let spans = split(*axis, children, rect.width as usize, rect.height as usize);
                let (mut x, mut y) = (rect.x, rect.y);
                for (child, (w, h)) in children.iter().zip(spans) {
                    let area = Rect::new(x, y, w as u16, h as u16);
                    child.blit(frame, area, buffer, force || resized, damage);
                    match axis {
                        Axis::Vertical => y += h as u16,
                        Axis::Horizontal => x += w as u16,
                    }
                }
                self.mark(rect);
            }
            Kind::Border(title, style, child) => {
                if resized || force {
                    let blank = vec![
                        vec![Segment::new(
                            " ".repeat((rect.width as usize).saturating_sub(2)),
                            None
                        )];
                        (rect.height as usize).saturating_sub(2)
                    ];
                    let edges = border(
                        title,
                        style,
                        &blank,
                        rect.width as usize,
                        rect.height as usize,
                    );
                    write(&edges, rect, buffer, damage);
                    self.mark(rect);
                }
                let inner = Rect::new(
                    rect.x + 1,
                    rect.y + 1,
                    rect.width.saturating_sub(2),
                    rect.height.saturating_sub(2),
                );
                child.blit(frame, inner, buffer, force || resized, damage);
            }
            Kind::Keyed(list) => {
                let reshaped = reactive::observe(self.id, || list.reconcile());
                let force = force || resized || reshaped;
                if force {
                    // Rows past the last child are cleared.
                    write(
                        &Segment::set_shape(Vec::new(), rect.width as usize, rect.height as usize),
                        rect,
                        buffer,
                        damage,
                    );
                }
                for (i, child) in list
                    .children()
                    .iter()
                    .take(rect.height as usize)
                    .enumerate()
                {
                    let row = Rect::new(rect.x, rect.y + i as u16, rect.width, 1);
                    child.blit(frame, row, buffer, force, damage);
                }
                for child in list.children().iter().skip(rect.height as usize) {
                    child.cache.borrow_mut().take();
                }
                self.mark(rect);
            }
        }
    }

    /// Record a container's size, so it knows when it is resized.
    fn mark(&self, rect: Rect) {
        let mut cache = self.cache.borrow_mut();
        if self.size_changed_in(&cache, rect) {
            *cache = Some((
                rect.width as usize,
                rect.height as usize,
                Rc::new(Vec::new()),
            ));
        }
    }

    fn size_changed_in(&self, cache: &Option<(usize, usize, Lines)>, rect: Rect) -> bool {
        !matches!(cache, Some((w, h, _)) if *w == rect.width as usize && *h == rect.height as usize)
    }
}

fn count(frame: &Frame, rendered: bool) {
    let mut stats = frame.stats.get();
    if rendered {
        stats.rendered += 1;
    } else {
        stats.reused += 1;
    }
    frame.stats.set(stats);
}

/// Replace `rect`'s cells with `lines` and record the damage.
fn write(lines: &[Vec<Segment>], rect: Rect, buffer: &mut Buffer, damage: &mut Damage) {
    let rect = rect.intersection(buffer.area);
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            buffer[(x, y)].reset();
        }
    }
    crate::interop::lines_to_buffer(lines, rect, buffer);
    damage.rects.push(rect);
}

impl<T> App<T> {
    /// Render a frame into a retained `buffer`; returns what was written.
    pub fn frame_into(&self, console: &Console, buffer: &mut Buffer) -> Damage {
        let dirty = reactive::take_dirty();
        self.stats.set(Stats::default());
        let frame = Frame {
            console,
            immediate: self.immediate,
            dirty: &dirty,
            stats: &self.stats,
        };
        let mut damage = Damage { rects: Vec::new() };
        let area = buffer.area;
        self.root.blit(&frame, area, buffer, false, &mut damage);
        damage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::signal;

    fn console() -> Console {
        Console::builder().width(40).color_system(None).build()
    }

    fn plain(lines: &Lines) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect()
    }

    #[test]
    fn only_what_read_a_changed_signal_renders_again() {
        let count = signal(0);
        let title = signal("ops".to_string());
        let app: App<()> = App::new(
            column(vec![
                text(move || format!("[bold]{}[/]", title.get())).size(Size::Fixed(1)),
                bordered("Body", text(|| "static".into())),
                text(move || format!("tick {}", count.get())).size(Size::Fixed(1)),
            ]),
            |_| Flow::Continue,
        );
        let c = console();
        let first = app.frame(&c, 30, 6);
        assert_eq!(plain(&first)[5].trim_end(), "tick 0");
        // Five nodes: the column, the title, the border and its child, the
        // counter.
        assert_eq!(app.stats().rendered, 5);

        // Nothing changed: every node reuses its cache.
        app.frame(&c, 30, 6);
        assert_eq!(app.stats().rendered, 0);

        // The counter's leaf and its ancestor (the column) render; the
        // title, the border and its child do not.
        count.set(1);
        let lines = app.frame(&c, 30, 6);
        assert_eq!(plain(&lines)[5].trim_end(), "tick 1");
        assert_eq!(app.stats().rendered, 2);
        assert_eq!(app.stats().reused, 3);

        // A resize renders everything.
        app.frame(&c, 32, 6);
        assert_eq!(app.stats().rendered, 5);
    }

    #[test]
    fn the_buffer_path_draws_what_the_lines_path_draws() {
        // Each app gets its own signals: the prototype's store has one dirty
        // set, and the first app to render a frame takes it.
        let build = || {
            let count = signal(0);
            let keys = signal(vec![1, 2, 3]);
            let node = column(vec![
                text(move || format!("[bold]tick[/] {}", count.get())).size(Size::Fixed(1)),
                row(vec![
                    bordered(
                        "List",
                        keyed(keys, |k: &i32| {
                            let k = *k;
                            text(move || format!("item {k}"))
                        }),
                    ),
                    bordered("Body", text(|| "日本語 ok".into())),
                ]),
            ]);
            (node, count, keys)
        };
        let (lines_root, count_a, keys_a) = build();
        let (buffer_root, count_b, keys_b) = build();
        let lines_app: App<()> = App::new(lines_root, |_| Flow::Continue);
        let buffer_app: App<()> = App::new(buffer_root, |_| Flow::Continue);
        let c = console();
        let area = ratatui::layout::Rect::new(0, 0, 30, 6);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        let check = |buffer: &ratatui::buffer::Buffer, lines: &Lines| {
            let from_buffer: Vec<String> = crate::interop::buffer_to_lines(buffer)
                .iter()
                .map(|l| l.iter().map(|s| s.text.as_str()).collect())
                .collect();
            assert_eq!(from_buffer, plain(lines));
        };
        buffer_app.frame_into(&c, &mut buffer);
        check(&buffer, &lines_app.frame(&c, 30, 6));
        count_b.set(5);
        keys_b.set(vec![3, 1]);
        let damage = buffer_app.frame_into(&c, &mut buffer);
        count_a.set(5);
        keys_a.set(vec![3, 1]);
        check(&buffer, &lines_app.frame(&c, 30, 6));
        // The counter's row and the list, not the body.
        assert!(damage.rects.iter().all(|r| r.x < 15), "{:?}", damage.rects);
    }

    #[test]
    fn keyed_children_keep_their_nodes_when_reordered() {
        let keys = signal(vec!["a", "b", "c"]);
        let built = Rc::new(Cell::new(0));
        let counter = built.clone();
        let app: App<()> = App::new(
            keyed(keys, move |k: &&str| {
                counter.set(counter.get() + 1);
                let k = k.to_string();
                text(move || k.clone())
            }),
            |_| Flow::Continue,
        );
        let c = console();
        app.frame(&c, 10, 3);
        assert_eq!(built.get(), 3);
        keys.set(vec!["c", "a", "b"]);
        let lines = app.frame(&c, 10, 3);
        assert_eq!(
            plain(&lines)
                .iter()
                .map(|l| l.trim_end())
                .collect::<Vec<_>>(),
            ["c", "a", "b"]
        );
        // Reordering built nothing, and no row rendered again: only the
        // list recomposed.
        assert_eq!(built.get(), 3);
        assert_eq!(app.stats().rendered, 1);
        keys.set(vec!["c", "d"]);
        app.frame(&c, 10, 3);
        assert_eq!(built.get(), 4);
    }

    #[test]
    fn the_app_runs_under_the_headless_driver() {
        let count = signal(0u32);
        let app = App::new(
            column(vec![text(move || format!("count {}", count.get()))]),
            move |event| match event {
                Event::Key(key) if key.code == rich_interact::KeyCode::Char('+') => {
                    count.update(|c| *c += 1);
                    Flow::Continue
                }
                Event::Key(key) if key.code == rich_interact::KeyCode::Enter => {
                    Flow::Done(count.get_untracked())
                }
                _ => Flow::Ignored,
            },
        );
        let script = rich_interact::headless::Script::new().keys("+ + + enter");
        let (outcome, record) = rich_interact::headless::run(app, script, 20, 3);
        assert_eq!(outcome.unwrap(), rich_interact::Outcome::Done(3));
        assert!(
            record.last_frame().contains("count 3"),
            "{}",
            record.last_frame()
        );
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::reactive::signal;

    fn plain(lines: &Lines) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect()
    }

    /// A border two or three columns wide underflowed its width.
    #[test]
    fn narrow_borders_draw_plain_boxes() {
        let c = Console::builder().width(10).color_system(None).build();
        for width in 2..6 {
            let app: App<()> = App::new(bordered("Title", text(|| "x".into())), |_| Flow::Continue);
            let lines = app.frame(&c, width, 3);
            assert!(lines
                .iter()
                .all(|l| l.iter().map(Segment::cell_length).sum::<usize>() == width));
        }
    }

    /// Fixed sizes adding up to more than the parent are cut, so a child
    /// never overlaps its neighbour.
    #[test]
    fn fixed_sizes_are_clamped_to_the_parent() {
        let children = vec![
            text(|| "a".into()).size(Size::Fixed(4)),
            text(|| "b".into()).size(Size::Fixed(4)),
            text(|| "c".into()),
        ];
        let spans = split(Axis::Horizontal, &children, 6, 1);
        assert_eq!(spans.iter().map(|s| s.0).collect::<Vec<_>>(), [4, 2, 0]);
    }

    /// A keyed list longer than its height: rows below the viewport that
    /// change while hidden draw their new value once they show.
    #[test]
    fn hidden_keyed_rows_redraw_when_shown() {
        let keys = signal(vec![0, 1, 2]);
        let labels = signal(vec!["a".to_string(), "b".into(), "c".into()]);
        let app: App<()> = App::new(
            keyed(keys, move |&k: &i32| {
                text(move || labels.get()[k as usize].clone())
            }),
            |_| Flow::Continue,
        );
        let c = Console::builder().width(10).color_system(None).build();
        app.frame(&c, 4, 2);
        labels.update(|l| l[2] = "C".into());
        app.frame(&c, 4, 2);
        keys.set(vec![2, 0, 1]);
        let lines = app.frame(&c, 4, 2);
        assert_eq!(plain(&lines)[0].trim_end(), "C");
    }
}
