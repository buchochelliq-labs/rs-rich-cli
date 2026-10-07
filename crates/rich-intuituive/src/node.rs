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
//!
//! Every node is a [`Widget`]: the builders above make built-in ones, and
//! [`widget`](crate::widget()) makes a node of your own.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use rich::{Console, Renderable, Segment, Style};
use rich_interact::{Component, Key, Mouse};

use crate::app::Ctx;
use crate::builtin::{Each, Grid, Host, HostWidget, Leaf, Pad, Panel, ScrollView, Stack, Switch};
pub use crate::layout::Size;
use crate::layout::Track;
use crate::reactive::{next_node, NodeId, Runtime, Signal};
use crate::screen::{Rect, Screen};
use crate::widget::{Canvas, DrawCx, MeasureCx, ScrollCx, Watchers, Widget};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

pub(crate) type Handler = Box<dyn FnMut(&mut Ctx)>;
/// A mouse handler: the event in the node's coordinates; whether it used
/// it.
pub(crate) type MouseHandler = Box<dyn FnMut(&mut Ctx, Mouse) -> bool>;

/// One node of the tree: a [`Widget`] with a place in the layout, key
/// bindings and handlers.
pub struct Node {
    pub(crate) id: NodeId,
    pub(crate) size: Size,
    pub(crate) min: u16,
    pub(crate) max: u16,
    /// Columns and rows it spans in a [`grid`].
    pub(crate) span: (u16, u16),
    pub(crate) body: RefCell<Body>,
    /// Where it was last laid out.
    pub(crate) rect: Cell<Rect>,
    /// Where it last drew; `None` before its first draw, or while hidden.
    drawn: Cell<Option<Rect>>,
    pub(crate) focusable: bool,
    pub(crate) keys: RefCell<Vec<(Vec<Key>, String, Handler)>>,
    pub(crate) click: RefCell<Option<Handler>>,
    pub(crate) mouse: RefCell<Option<MouseHandler>>,
    /// Where the widget wants the text caret, on its surface.
    pub(crate) caret: Cell<Option<(u16, u16)>>,
    /// What the inspector calls it: its [`name`](Node::name), else the
    /// builder that made it.
    name: Option<String>,
    what: &'static str,
    /// A style (or theme style name) laid over the node while it has the
    /// focus.
    focus_style: Option<String>,
}

/// A node's widget, and what the framework keeps for it between frames.
pub(crate) struct Body {
    pub widget: Box<dyn Widget>,
    /// Where its children were laid out last, and which children.
    areas: Vec<Rect>,
    laid: Vec<NodeId>,
    /// A viewport's content, drawn offscreen, and what of it is shown.
    view: Option<View>,
}

/// A [viewport](Widget::viewport)'s offscreen content.
struct View {
    buffer: Screen,
    /// The window on the widget's surface, and the content's point at its
    /// top left, when it was last copied out.
    shown: Option<(Rect, (u16, u16))>,
}

impl Node {
    pub(crate) fn from_widget(widget: Box<dyn Widget>, what: &'static str) -> Node {
        Node {
            id: next_node(),
            size: Size::Flex(1),
            min: 0,
            max: u16::MAX,
            span: (1, 1),
            body: RefCell::new(Body {
                widget,
                areas: Vec::new(),
                laid: Vec::new(),
                view: None,
            }),
            rect: Cell::new(Rect::default()),
            drawn: Cell::new(None),
            focusable: false,
            keys: RefCell::new(Vec::new()),
            click: RefCell::new(None),
            mouse: RefCell::new(None),
            caret: Cell::new(None),
            name: None,
            what,
            focus_style: None,
        }
    }

    fn what(mut self, what: &'static str) -> Node {
        self.what = what;
        self
    }

    /// Name this node for the [inspector](crate::App::inspector).
    pub fn name(mut self, name: &str) -> Node {
        self.name = Some(name.to_string());
        self
    }

    /// Its name, if it was given one: how the command palette and the help
    /// group its bindings.
    pub(crate) fn label(&self) -> String {
        self.name.clone().unwrap_or_default()
    }

    /// How the inspector shows this node: its name or builder, and what it
    /// holds.
    pub(crate) fn describe(&self) -> String {
        let base = match &self.name {
            Some(name) => format!("{name} ({})", self.what),
            None => self.what.to_string(),
        };
        match self.body.borrow().widget.describe() {
            Some(more) => format!("{base} {more}"),
            None => base,
        }
    }

    /// The widget, if it is a `W`.
    fn widget_mut<W: Widget, R>(&self, f: impl FnOnce(&mut W) -> R) -> Option<R> {
        let mut body = self.body.borrow_mut();
        let widget: &mut dyn std::any::Any = &mut *body.widget;
        widget.downcast_mut::<W>().map(f)
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
        self.widget_mut(|stack: &mut Stack| stack.gap = cells);
        self.widget_mut(|grid: &mut Grid| grid.gap = (cells, cells));
        self
    }

    /// For a [`grid`]: the rows' sizes. Rows past the last one given take
    /// its size, so `.rows([Size::Auto])` makes every row as tall as its
    /// content; without any, rows share the height evenly. Ignored on other
    /// nodes.
    pub fn rows(self, rows: impl IntoIterator<Item = Size>) -> Node {
        let rows: Vec<Size> = rows.into_iter().collect();
        self.widget_mut(|grid: &mut Grid| grid.rows = rows);
        self
    }

    /// Put this node in the Tab order, so it can hold the focus.
    pub fn focusable(mut self) -> Node {
        self.focusable = true;
        self
    }

    /// Take this node out of the Tab order: a [`list`] that only shows, or
    /// a component you drive yourself.
    pub fn no_focus(mut self) -> Node {
        self.focusable = false;
        self
    }

    /// While this node has the focus, draw it in `style` over its own: a
    /// style (`"reverse"`, `"on grey23"`) or a theme style name
    /// (`"accent"`). Its whole rectangle takes the style, so a list shows
    /// which row is selected; a container's children draw over it, so it
    /// shows only between them. The node is also made
    /// [focusable](Self::focusable).
    pub fn focus_style(mut self, style: &str) -> Node {
        self.focus_style = Some(style.to_string());
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

    /// Run `handler` for every mouse event over this node (presses, releases,
    /// drags, movement, the wheel), in the node's own coordinates, before
    /// its ancestors see it. It returns whether it used the event; one it
    /// did not use bubbles up. A press does not focus the node unless it is
    /// [focusable](Self::focusable).
    pub fn on_mouse(self, handler: impl FnMut(&mut Ctx, Mouse) -> bool + 'static) -> Node {
        *self.mouse.borrow_mut() = Some(Box::new(handler));
        self
    }

    /// For a [`component`] node: run `handler` when the component is
    /// cancelled (Esc). Without one, the cancelling key bubbles to the
    /// node's ancestors like any unused key. Ignored on other nodes.
    pub fn on_cancel(self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.widget_mut(|host: &mut HostWidget| host.host.set_cancel(Box::new(handler)));
        self
    }

    /// Wrap this node in a rounded border with `title`. The border is
    /// highlighted while the focus is on the node or inside it. The panel
    /// takes the node's size.
    pub fn panel(self, title: &str) -> Node {
        let title = title.to_string();
        self.wrap(|child| Box::new(Panel::new(title, child)), "panel")
    }

    /// Leave `vertical` empty rows above and below this node, and
    /// `horizontal` empty columns either side.
    pub fn padding(self, vertical: u16, horizontal: u16) -> Node {
        self.wrap(
            |child| {
                Box::new(Pad {
                    child,
                    edges: [vertical, horizontal, vertical, horizontal],
                })
            },
            "padding",
        )
    }

    /// A node of `widget` round this one, taking over its size and span.
    fn wrap(self, widget: impl FnOnce(Node) -> Box<dyn Widget>, what: &'static str) -> Node {
        let (size, min, max, span) = (self.size, self.min, self.max, self.span);
        let mut outer = Node::from_widget(widget(self), what);
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

    /// How it asks to be sized along its parent's axis.
    pub fn size_hint(&self) -> Size {
        self.size
    }

    pub(crate) fn track(&self) -> Track {
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
        self.each_child(&mut |child| child.walk_inner(path, f));
        path.pop();
    }

    /// Call `f` with each child that is shown.
    fn each_child(&self, f: &mut dyn FnMut(&Node)) {
        for child in self.body.borrow().widget.children() {
            f(child);
        }
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
        self.body
            .borrow_mut()
            .widget
            .measure(&MeasureCx { console }, axis, width, height)
    }

    /// Its extent along `axis` as a stack sizes it: its size, within its
    /// minimum and maximum.
    pub(crate) fn extent(&self, console: &Console, axis: Axis, width: u16, height: u16) -> u16 {
        let along = match axis {
            Axis::Vertical => height,
            Axis::Horizontal => width,
        };
        let cells = match self.size {
            Size::Fixed(n) => n,
            Size::Percent(p) => (along as u32 * p.min(100) as u32 / 100) as u16,
            Size::Auto | Size::Flex(_) => self.measure(console, axis, width, height),
        };
        cells.min(self.max).max(self.min)
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
        let last = self.drawn.get();
        let moved = last != Some(rect);
        let dirty = frame.dirty.contains(&self.id);
        let redraw = moved || force || dirty;
        let id = self.id;
        let mut body = self.body.borrow_mut();
        let Body {
            widget,
            areas,
            laid,
            view,
        } = &mut *body;
        let viewport = widget.viewport();
        let console = frame.console;
        // What the widget wrote, and whether it painted all of its rectangle.
        let mut written: Vec<Rect> = Vec::new();
        let mut repaint = false;
        let mut cleared = false;
        let mut relaid = false;
        if redraw {
            if let Some(last) = last.filter(|last| !last.is_empty()) {
                if (last.width, last.height) != (rect.width, rect.height) {
                    frame.resized.push(id);
                }
            }
            // Lay out first, subscribing to what that reads; a viewport
            // lays out in its content's coordinates.
            let space = if viewport {
                Rect::new(0, 0, rect.width, rect.height)
            } else {
                rect
            };
            let new_areas = frame
                .runtime
                .observe_node(id, || widget.layout(&MeasureCx { console }, space));
            let ids: Vec<NodeId> = widget.children().iter().map(|c| c.id).collect();
            relaid =
                (!laid.is_empty() || !areas.is_empty()) && (ids != *laid || new_areas != *areas);
            *areas = new_areas;
            *laid = ids;
            // A widget that keeps what it drew is cleared only when its own
            // children moved: when it moved, or was drawn over, whoever did
            // that cleared the area already.
            cleared = !widget.retained() || self.focus_style.is_some() || (relaid && !viewport);
            repaint = moved || force || cleared;
            if cleared {
                screen.clear(rect);
            }
        }
        // A viewport's children draw offscreen before it decides where to
        // look; it draws itself after, so its scrollbar matches.
        let mut window = None;
        if viewport {
            window = Some(self.draw_view(frame, widget, areas, view, rect, relaid));
        }
        if redraw {
            let focus_path = frame.focus_path;
            let focus_style = self.focus_style.as_deref();
            let caret = {
                let mut cx = DrawCx {
                    console,
                    theme: frame.theme,
                    id,
                    rect,
                    repaint,
                    focus_path,
                    hover_path: frame.hover_path,
                    pointer: frame.pointer,
                    shift: frame.shift,
                    wants_hover: &frame.wants_hover,
                    watchers: frame.watchers,
                };
                let mut canvas = Canvas {
                    screen: &mut *screen,
                    rect,
                    written: &mut written,
                };
                frame.runtime.observe_node_more(id, || {
                    widget.draw(&mut cx, &mut canvas);
                    // Reading the focus subscribes the node, so it draws
                    // again when the focus comes or goes.
                    if let Some(style) = focus_style {
                        if focus_path.with(|path| path.last() == Some(&id)) {
                            let style = cx.style(style, "reverse");
                            canvas.restyle(0, 0, rect.width, rect.height, &style);
                        }
                    }
                });
                widget.caret()
            };
            self.caret.set(caret.and_then(|(x, y)| {
                (x < rect.width && y < rect.height).then_some((rect.x + x, rect.y + y))
            }));
            if cleared {
                frame.damage.push(rect);
            } else {
                frame.damage.extend(written.iter().copied());
            }
            if cleared || !written.is_empty() {
                frame.drew(id);
            }
        }
        match window {
            Some((window, offset, inner)) => {
                // The rows in view, copied out when they changed.
                let shown = view.as_ref().and_then(|v| v.shown);
                if redraw || inner || shown != Some((window, offset)) {
                    let view = view.as_mut().expect("a viewport has a view");
                    screen.clear(window);
                    screen.blit(
                        &view.buffer,
                        Rect::new(offset.0, offset.1, window.width, window.height),
                        window.x,
                        window.y,
                    );
                    frame.damage.push(window);
                    if !redraw {
                        frame.drew(id);
                    }
                    view.shown = Some((window, offset));
                }
            }
            None => {
                for (i, child) in widget.children().iter().enumerate() {
                    match areas.get(i).map(|area| area.intersection(rect)) {
                        Some(area) if !area.is_empty() => {
                            // A child draws again where the widget drew over
                            // it.
                            let over = written.iter().any(|w| !w.intersection(area).is_empty());
                            child.draw(frame, area, screen, repaint || over);
                        }
                        _ => child.hide(),
                    }
                }
            }
        }
        self.drawn.set(Some(rect));
    }

    /// Draw a viewport's children into its offscreen content and ask it
    /// where to look: the window on the screen, the content's point at its
    /// top left, and whether anything inside drew.
    fn draw_view(
        &self,
        frame: &mut FrameState,
        widget: &mut Box<dyn Widget>,
        areas: &[Rect],
        view: &mut Option<View>,
        rect: Rect,
        relaid: bool,
    ) -> (Rect, (u16, u16), bool) {
        let content = areas.iter().fold((0u16, 0u16), |(w, h), area| {
            (w.max(area.right()), h.max(area.bottom()))
        });
        let view = view.get_or_insert_with(|| View {
            buffer: Screen::new(0, 0),
            shown: None,
        });
        let previous = view.shown;
        let mut force = relaid;
        let size = view.buffer.area();
        if (size.width, size.height) != content {
            view.buffer = Screen::new(content.0, content.1);
            force = true;
        } else if relaid {
            // The children moved: what they leave between them (gaps,
            // padding) must not keep old cells.
            view.buffer.clear(size);
        }
        if force {
            view.shown = None;
        }
        // Inside, the screen's coordinates are shifted by the window and
        // the scroll, as they were last shown.
        let outer = frame.shift;
        if let Some((window, offset)) = previous {
            frame.shift = (
                outer.0 - window.x as i32 + offset.0 as i32,
                outer.1 - window.y as i32 + offset.1 as i32,
            );
        }
        let saved = std::mem::take(&mut frame.damage);
        let all = view.buffer.area();
        for (i, child) in widget.children().iter().enumerate() {
            match areas.get(i).map(|area| area.intersection(all)) {
                Some(area) if !area.is_empty() => child.draw(frame, area, &mut view.buffer, force),
                _ => child.hide(),
            }
        }
        let inner = !std::mem::replace(&mut frame.damage, saved).is_empty();
        frame.shift = outer;
        // Where the focus is inside, for scrolling it into view.
        let focused = frame.focus_path.with_untracked(|path| {
            let last = *path.last()?;
            if !path.contains(&self.id) || last == self.id {
                return None;
            }
            let mut found = None;
            for child in widget.children() {
                with_node(child, last, &mut |node| found = Some(node.rect()));
            }
            found.map(|area| (last, area))
        });
        let cx = ScrollCx { content, focused };
        let (local, offset) = frame.runtime.untracked(|| widget.scroll(&cx));
        let window = Rect::new(
            rect.x.saturating_add(local.x),
            rect.y.saturating_add(local.y),
            local.width,
            local.height,
        )
        .intersection(rect);
        let offset = (
            offset.0.min(content.0.saturating_sub(window.width)),
            offset.1.min(content.1.saturating_sub(window.height)),
        );
        if previous != Some((window, offset)) {
            // Widgets inside that read the pointer drew with the old
            // scroll: they draw again with the new one.
            let watchers = frame.watchers.pointer.borrow();
            if !watchers.is_empty() {
                for child in widget.children() {
                    child.walk(&mut |node, _| {
                        if watchers.contains(&node.id) {
                            frame.runtime.mark_dirty(node.id);
                        }
                    });
                }
            }
        }
        (window, offset, inner)
    }

    /// Forget this subtree's subscriptions (it left the tree), including
    /// the nodes a widget keeps hidden.
    pub(crate) fn forget(&self, runtime: &Runtime) {
        runtime.forget(self.id);
        let body = self.body.borrow();
        for child in body.widget.children() {
            child.forget(runtime);
        }
        for child in body.widget.hidden_children() {
            child.forget(runtime);
        }
    }
}

/// A box's four edges: top and bottom rows, left and right columns between
/// them.
pub(crate) fn edges(rect: Rect) -> [Rect; 4] {
    let inner = rect.height.saturating_sub(2);
    [
        Rect::new(rect.x, rect.y, rect.width, 1),
        Rect::new(rect.x, rect.bottom().saturating_sub(1), rect.width, 1),
        Rect::new(rect.x, rect.y + 1, 1, inner),
        Rect::new(rect.right().saturating_sub(1), rect.y + 1, 1, inner),
    ]
}

/// A box's lines (from [`border`]) as its four edges, in the order of
/// [`edges`]: the top and bottom rows whole, and the first and last
/// segment of each row between them. Whole segments, so a title's
/// grapheme clusters (an emoji sequence) stay intact.
pub(crate) fn edge_lines(lines: &[Vec<Segment>]) -> [Vec<Vec<Segment>>; 4] {
    let middle = &lines[1.min(lines.len())..lines.len().saturating_sub(1).max(1)];
    let side = |pick: fn(&Vec<Segment>) -> Option<&Segment>| -> Vec<Vec<Segment>> {
        middle
            .iter()
            .map(|line| pick(line).cloned().into_iter().collect())
            .collect()
    };
    [
        lines.first().cloned().into_iter().collect(),
        lines.last().cloned().into_iter().collect(),
        side(|line| line.first()),
        side(|line| line.last()),
    ]
}

/// `lines` with `style` laid over every segment, each row filled out to
/// the rectangle's width in it.
fn highlight(lines: Vec<Vec<Segment>>, style: &Style, rect: Rect) -> Vec<Vec<Segment>> {
    lines
        .into_iter()
        .take(rect.height as usize)
        .map(|line| {
            let width: usize = line.iter().map(Segment::cell_length).sum();
            let mut line: Vec<Segment> = line
                .into_iter()
                .map(|segment| {
                    let combined = match &segment.style {
                        Some(own) => own.combine(style),
                        None => style.clone(),
                    };
                    Segment::new(segment.text, Some(combined))
                })
                .collect();
            let fill = (rect.width as usize).saturating_sub(width);
            if fill > 0 {
                line.push(Segment::new(" ".repeat(fill), Some(style.clone())));
            }
            line
        })
        .collect()
}

/// A one-column scrollbar `rows` tall for content `content` rows tall,
/// scrolled to `top`.
pub(crate) fn scrollbar(console: &Console, rows: u16, top: u16, content: u16) -> Vec<Vec<Segment>> {
    let style = |name: &str, fallback: &str| {
        console
            .get_style(&rich::style::StyleType::Name(name.to_string()))
            .ok()
            .or_else(|| Style::parse(fallback).ok())
    };
    let (track, thumb) = (
        style("scrollbar", "bright_black"),
        style("scrollbar.thumb", "white"),
    );
    let (rows_f, content_f) = (rows as f64, content.max(1) as f64);
    let size = (rows_f * rows_f / content_f).round().clamp(1.0, rows_f);
    let start = (top as f64 * rows_f / content_f).round().min(rows_f - size);
    (0..rows)
        .map(|row| {
            let on = (row as f64) >= start && (row as f64) < start + size;
            if on {
                vec![Segment::new("┃", thumb.clone())]
            } else {
                vec![Segment::new("│", track.clone())]
            }
        })
        .collect()
}

/// A visit in [`walk_screen`]: the node, its path, the shift to screen
/// coordinates and the part of the screen it can show in.
pub(crate) type ScreenVisit<'a> = dyn FnMut(&Node, &[NodeId], (i32, i32), Rect) + 'a;

/// Visit every node that is shown, with the translation that takes its
/// coordinates to the screen's and the part of the screen it can show in
/// (both change inside a [`scroll`]).
pub(crate) fn walk_screen(root: &Node, f: &mut ScreenVisit) {
    fn inner(
        node: &Node,
        path: &mut Vec<NodeId>,
        shift: (i32, i32),
        clip: Rect,
        f: &mut ScreenVisit,
    ) {
        path.push(node.id);
        f(node, path, shift, clip);
        let scrolled = node.body.borrow().view.as_ref().and_then(|view| {
            let (window, offset) = view.shown?;
            let window = translate(window, shift);
            Some((
                (
                    window.x as i32 - offset.0 as i32,
                    window.y as i32 - offset.1 as i32,
                ),
                window.intersection(clip),
            ))
        });
        let (shift, clip) = scrolled.unwrap_or((shift, clip));
        node.each_child(&mut |child| inner(child, path, shift, clip, f));
        path.pop();
    }
    let all = Rect::new(0, 0, u16::MAX, u16::MAX);
    inner(root, &mut Vec::new(), (0, 0), all, f);
}

/// `rect` moved by `shift`, clamped to the screen.
pub(crate) fn translate(rect: Rect, (dx, dy): (i32, i32)) -> Rect {
    let x = rect.x as i32 + dx;
    let y = rect.y as i32 + dy;
    let (x0, y0) = (x.max(0), y.max(0));
    let width = (rect.width as i32 - (x0 - x)).max(0);
    let height = (rect.height as i32 - (y0 - y)).max(0);
    Rect::new(
        x0.min(u16::MAX as i32) as u16,
        y0.min(u16::MAX as i32) as u16,
        width.min(u16::MAX as i32) as u16,
        height.min(u16::MAX as i32) as u16,
    )
}

/// What a frame needs while it draws.
pub(crate) struct FrameState<'a> {
    pub console: &'a Console,
    pub runtime: &'a Runtime,
    pub dirty: HashSet<NodeId>,
    pub damage: Vec<Rect>,
    pub focus_path: Signal<Vec<NodeId>>,
    /// The nodes under the mouse pointer, outermost first.
    pub hover_path: Signal<Vec<NodeId>>,
    /// A widget asked whether it is hovered: the app turns on the
    /// terminal's pointer movement reports.
    pub wants_hover: Cell<bool>,
    /// The nodes that asked about the pointer.
    pub watchers: &'a Watchers,
    /// Where the pointer last was, on the screen.
    pub pointer: Option<(u16, u16)>,
    /// From the screen to the surface being drawn on (inside a viewport,
    /// its content).
    pub shift: (i32, i32),
    /// Nodes laid out at a new size this frame.
    pub resized: Vec<NodeId>,
    pub theme: &'a crate::app::Theme,
    /// Nodes drawn this frame.
    pub drawn: usize,
    /// Which, when the inspector is watching.
    pub drawn_ids: Option<Vec<NodeId>>,
}

impl FrameState<'_> {
    /// `id` drew this frame.
    pub fn drew(&mut self, id: NodeId) {
        self.drawn += 1;
        if let Some(ids) = &mut self.drawn_ids {
            ids.push(id);
        }
    }
}

/// A rounded border round `rect`, with `title` in the top edge; the inside
/// is left to the child.
pub(crate) fn border(
    title: &str,
    style: &Style,
    title_style: &Style,
    rect: Rect,
) -> Vec<Vec<Segment>> {
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
    Node::from_widget(Box::new(Leaf(Box::new(draw))), "leaf")
}

/// Console markup, from a closure that may read signals:
/// `text(move || format!("[b]{}[/] items", count.get()))`. The
/// [`text!`](crate::text!) macro writes the closure for you.
pub fn text(markup: impl Fn() -> String + 'static) -> Node {
    let node = leaf(move |console, width, _| {
        let markup = markup();
        let text = rich::Text::from_markup(&markup).unwrap_or_else(|_| rich::Text::new(markup));
        console.render_lines(
            &text,
            &console.options().update_width(width.max(1) as usize),
            false,
        )
    });
    node.what("text")
}

/// Console markup that never changes.
pub fn label(markup: impl Into<String>) -> Node {
    let markup = markup.into();
    text(move || markup.clone()).what("label")
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
    .what("renderable")
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
    let mut node = Node::from_widget(
        Box::new(HostWidget::new(Box::new(Host {
            component,
            make: None,
            on_done: Box::new(on_done),
            on_cancel: None,
        }))),
        "component",
    );
    node.focusable = true;
    node.what("component")
}

/// A [`component`] that starts again after each answer: `make` builds it,
/// and builds a fresh one each time it is done, so an `Input` clears for
/// the next entry (a chat box, a to-do entry, a command line).
///
/// ```
/// use intuituive::prelude::*;
/// use rich_interact::Input;
///
/// let app = App::new(|| {
///     let added = signal(Vec::<String>::new());
///     column([
///         repeating(|| Input::new("Add"), move |item, _| added.update(|v| v.push(item))),
///         text(move || added.get().join(", ")),
///     ])
///     .on_key("esc", |cx| cx.quit())
/// });
/// let keys = ["a", "enter", "b", "enter", "esc"];
/// let screen = app.render_with(&keys, 30, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "a, b");
/// ```
pub fn repeating<C: Component + 'static>(
    make: impl Fn() -> C + 'static,
    on_done: impl FnMut(C::Output, &mut Ctx) + 'static,
) -> Node {
    let mut node = Node::from_widget(
        Box::new(HostWidget::new(Box::new(Host {
            component: make(),
            make: Some(Box::new(make)),
            on_done: Box::new(on_done),
            on_cancel: None,
        }))),
        "component",
    );
    node.focusable = true;
    node.what("component")
}

/// A scrolling list of markup rows, one selected: ratatui's `List` and
/// `ListState` in one node. `items` may read signals; `selected` is the
/// index of the selected row, which you read and write like any signal.
///
/// The list keeps the selected row in view, scrolling only as far as it
/// must, and draws it in the theme's `selected` style, filled to the full
/// width. It is focusable, and while it has the focus ↑/k, ↓/j, Home/g,
/// End/G, PageUp and PageDown move the selection; bind Enter (or anything
/// else) yourself with [`on_key`](Node::on_key).
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let selected = signal(0usize);
///     let items = || (1..=50).map(|n| format!("item {n}")).collect();
///     list(items, selected).on_key("q", |cx| cx.quit())
/// });
/// // Down 7 times in a 5-row list: rows 4-8 (items 4-8) show, item 8 selected.
/// let keys = ["j", "j", "j", "j", "j", "j", "j", "q"];
/// let screen = app.render_with(&keys, 20, 5).unwrap();
/// assert_eq!(screen[0].trim_end(), "item 4");
/// assert_eq!(screen[4].trim_end(), "item 8");
/// ```
pub fn list(items: impl Fn() -> Vec<String> + 'static, selected: Signal<usize>) -> Node {
    let items = Rc::new(items);
    // The first row shown, and the height last drawn (for a page's size).
    let offset = Rc::new(Cell::new(0usize));
    let rows = Rc::new(Cell::new(1usize));
    let draw = {
        let (items, offset, rows) = (items.clone(), offset.clone(), rows.clone());
        move |console: &Console, width: u16, height: u16| {
            let items = items();
            let len = items.len();
            // Height 0 is a measurement: every row.
            let height = if height == 0 { len } else { height as usize };
            rows.set(height.max(1));
            let selected = selected.get().min(len.saturating_sub(1));
            let mut first = offset.get().min(len.saturating_sub(height));
            if selected < first {
                first = selected;
            } else if selected >= first + height {
                first = selected + 1 - height;
            }
            offset.set(first);
            let mut options = console.options().update_width(width.max(1) as usize);
            options.no_wrap = Some(true);
            options.overflow = Some(rich::Overflow::Ellipsis);
            let style = console
                .get_style(&rich::style::StyleType::Name("selected".into()))
                .unwrap_or_else(|_| Style::parse("reverse").expect("parses"));
            items
                .iter()
                .enumerate()
                .skip(first)
                .take(height)
                .map(|(i, markup)| {
                    let text = rich::Text::from_markup(markup)
                        .unwrap_or_else(|_| rich::Text::new(markup.clone()));
                    let line = console
                        .render_lines(&text, &options, false)
                        .into_iter()
                        .next()
                        .unwrap_or_default();
                    if i == selected {
                        let row = Rect::new(0, 0, width, 1);
                        highlight(vec![line], &style, row).remove(0)
                    } else {
                        line
                    }
                })
                .collect()
        }
    };
    let len = {
        let items = items.clone();
        move || items().len()
    };
    let step = move |by: isize| {
        let len = len();
        let last = len.saturating_sub(1) as isize;
        selected.update(|s| *s = (*s as isize + by).clamp(0, last.max(0)) as usize);
    };
    let (rows_up, rows_down) = (rows.clone(), rows);
    let (up, down, pgup, pgdn) = (step.clone(), step.clone(), step.clone(), step);
    leaf(draw)
        .what("list")
        .focusable()
        .on_key("up k", move |_| up(-1))
        .on_key("down j", move |_| down(1))
        .on_key("pageup", move |_| pgup(-(rows_up.get() as isize)))
        .on_key("pagedown", move |_| pgdn(rows_down.get() as isize))
        .on_key("home g", move |_| selected.set(0))
        .on_key("end G", move |_| {
            let len = items().len();
            selected.set(len.saturating_sub(1));
        })
}

/// A viewport onto `child`, laid out at its full height (up to 4000 rows)
/// and scrolled with the arrow keys, PgUp/PgDn and Home/End (when nothing
/// inside used them) and the mouse wheel, with a scrollbar when it does not
/// fit. Focus moving to a node inside scrolls it into view. Only the nodes
/// inside that changed draw; the rows in view are copied out.
pub fn scroll(child: Node) -> Node {
    scroll_with(child, crate::reactive::signal(0))
}

/// [`scroll`], with the first row in view in `offset`: read it, or set it
/// to scroll from code.
pub fn scroll_with(child: Node, offset: Signal<u16>) -> Node {
    Node::from_widget(Box::new(ScrollView::new(child, offset)), "scroll").focusable()
}

/// Children one above the other.
pub fn column(children: impl IntoIterator<Item = Node>) -> Node {
    stack(Axis::Vertical, children).what("column")
}

/// Children side by side.
pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    stack(Axis::Horizontal, children).what("row")
}

fn stack(axis: Axis, children: impl IntoIterator<Item = Node>) -> Node {
    Node::from_widget(
        Box::new(Stack {
            axis,
            gap: 0,
            children: children.into_iter().collect(),
        }),
        "stack",
    )
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
    Node::from_widget(
        Box::new(Grid {
            columns: columns.into_iter().collect(),
            rows: Vec::new(),
            gap: (0, 0),
            children: children.into_iter().collect(),
        }),
        "grid",
    )
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
    Node::from_widget(
        Box::new(Each {
            keys: Rc::new(keys),
            build,
            order: Vec::new(),
            children: Vec::new(),
        }),
        "each",
    )
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
    Node::from_widget(
        Box::new(Switch {
            key: Box::new(key),
            build,
            built: HashMap::new(),
            current: None,
        }),
        "switch",
    )
}

/// The nodes under `column`, `row`, outermost first (the path to the
/// deepest node whose shown rectangle holds the point), each with the
/// translation from screen coordinates to its own.
pub(crate) fn hit_path(root: &Node, column: u16, row: u16) -> Vec<(NodeId, (i32, i32))> {
    let mut found: Vec<(NodeId, (i32, i32))> = Vec::new();
    let mut shifts: Vec<(i32, i32)> = Vec::new();
    walk_screen(root, &mut |node, path, shift, clip| {
        shifts.truncate(path.len() - 1);
        shifts.push(shift);
        let shown = translate(node.rect(), shift).intersection(clip);
        if shown.contains(column, row) && path.len() > found.len() {
            found = path
                .iter()
                .zip(&shifts)
                .map(|(id, (dx, dy))| (*id, (-dx, -dy)))
                .collect();
        }
    });
    found
}

/// Every node shown, with the translation from screen coordinates to its
/// own (they differ inside a viewport).
pub(crate) fn shifts(root: &Node) -> Vec<(NodeId, (i32, i32))> {
    let mut out = Vec::new();
    walk_screen(root, &mut |node, _, shift, _| {
        out.push((node.id, (-shift.0, -shift.1)))
    });
    out
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
