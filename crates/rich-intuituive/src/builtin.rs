//! The built-in nodes, each a [`Widget`]: text and other leaves, columns
//! and rows, grids, panels, padding, keyed lists, switches, hosted
//! components and scrolls. They use only what the trait gives any widget,
//! which is the test that it is enough.

use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use rich::{Console, Segment};
use rich_interact::{Component, Context, Event, Flow, KeyCode, MouseKind};

use crate::app::Ctx;
use crate::layout::{grow_for_span, offsets, place, solve, Size, Track};
use crate::node::{Axis, Handler, Node};
use crate::reactive::{NodeId, Runtime, Signal};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, ScrollCx, Used, Widget, WidgetEvent};

pub(crate) type Draw = Box<dyn Fn(&Console, u16, u16) -> Vec<Vec<Segment>>>;

/// The most rows a [`scroll`](crate::scroll) lays its child out in.
pub(crate) const MAX_SCROLL_ROWS: u16 = 4000;

/// How far rendered `lines` reach along `axis`.
fn extent(lines: &[Vec<Segment>], axis: Axis) -> u16 {
    match axis {
        Axis::Vertical => lines.len(),
        Axis::Horizontal => lines
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0),
    }
    .min(u16::MAX as usize) as u16
}

/// A node drawn from rendered lines ([`leaf`](crate::leaf), text, labels,
/// renderables, lists).
pub(crate) struct Leaf(pub Draw);

impl Widget for Leaf {
    fn name(&self) -> &'static str {
        "leaf"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        extent(&(self.0)(cx.console(), width.max(1), height), axis)
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let lines = (self.0)(cx.console(), canvas.width(), canvas.height());
        canvas.lines(&lines);
    }
}

/// Children along an axis: [`column`](crate::column) and [`row`](crate::row).
pub(crate) struct Stack {
    pub axis: Axis,
    pub gap: u16,
    pub children: Vec<Node>,
}

impl Widget for Stack {
    fn name(&self) -> &'static str {
        match self.axis {
            Axis::Vertical => "column",
            Axis::Horizontal => "row",
        }
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let gaps = self
            .gap
            .saturating_mul(self.children.len().saturating_sub(1) as u16);
        if self.axis == axis {
            self.children.iter().fold(gaps, |sum, child| {
                sum.saturating_add(cx.extent(child, axis, width, height))
            })
        } else {
            self.children
                .iter()
                .map(|child| cx.measure(child, axis, width, height))
                .max()
                .unwrap_or(0)
        }
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        cx.stack(self.axis, self.gap, &self.children, rect)
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// Children in rows and columns: [`grid`](crate::grid).
pub(crate) struct Grid {
    pub columns: Vec<Size>,
    pub rows: Vec<Size>,
    /// Between rows, and between columns.
    pub gap: (u16, u16),
    pub children: Vec<Node>,
}

impl Widget for Grid {
    fn name(&self) -> &'static str {
        "grid"
    }

    fn describe(&self) -> Option<String> {
        let columns = self.columns.len().max(1);
        Some(format!(
            "{columns}x{}",
            self.children.len().div_ceil(columns)
        ))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        grid_areas(cx, self, Rect::new(0, 0, width, height), axis)
            .iter()
            .map(|area| match axis {
                Axis::Vertical => area.bottom(),
                Axis::Horizontal => area.right(),
            })
            .max()
            .unwrap_or(0)
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        grid_areas(cx, self, rect, Axis::Vertical)
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// Each child's rectangle in a grid. Columns are solved first, then rows,
/// so a row of [`Size::Auto`] knows how wide its children are. `axis`
/// is [`Axis::Horizontal`] when measuring the grid's own width, which
/// leaves flexible columns at their content.
fn grid_areas(cx: &MeasureCx, grid: &Grid, rect: Rect, axis: Axis) -> Vec<Rect> {
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
    let mut column_tracks: Vec<Track> = (0..columns)
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
                    .map(|(_, child)| cx.measure(child, Axis::Horizontal, rect.width, rect.height))
                    .max()
                    .unwrap_or(0);
            }
            track
        })
        .collect();
    // Children spanning several columns grow the content columns they span.
    for (p, child) in places.iter().zip(&grid.children) {
        if p.columns > 1 {
            let need = cx.measure(child, Axis::Horizontal, rect.width, rect.height);
            grow_for_span(
                &mut column_tracks,
                rect.width,
                column_gap,
                p.column,
                p.columns,
                need,
            );
        }
    }
    let widths = solve(rect.width, column_gap, &column_tracks);
    let xs = offsets(rect.x, column_gap, &widths);
    // Rows.
    let row_size = |r: usize| match grid.rows.get(r).or(grid.rows.last()) {
        Some(size) => *size,
        None => Size::Flex(1),
    };
    let mut row_tracks: Vec<Track> = (0..rows)
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
                        cx.measure(child, Axis::Vertical, width, 0)
                    })
                    .max()
                    .unwrap_or(0);
            }
            track
        })
        .collect();
    for (p, child) in places.iter().zip(&grid.children) {
        if p.rows > 1 {
            let width = span_length(&widths, p.column, p.columns, column_gap);
            let need = cx.measure(child, Axis::Vertical, width, 0);
            grow_for_span(&mut row_tracks, rect.height, row_gap, p.row, p.rows, need);
        }
    }
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

/// A rounded border with a title round one child: [`Node::panel`].
pub(crate) struct Panel {
    pub title: String,
    pub child: Node,
    /// Whether the border was last drawn as focused.
    shown_focus: Option<bool>,
}

impl Panel {
    pub fn new(title: String, child: Node) -> Panel {
        Panel {
            title,
            child,
            shown_focus: None,
        }
    }
}

impl Widget for Panel {
    fn name(&self) -> &'static str {
        "panel"
    }

    fn describe(&self) -> Option<String> {
        (!self.title.is_empty()).then(|| format!("\"{}\"", self.title))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        cx.measure(
            &self.child,
            axis,
            width.saturating_sub(2),
            height.saturating_sub(2),
        )
        .saturating_add(2)
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![rect.inner(1)]
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        // The focus is all a panel reads: when it moves elsewhere in the
        // app, the panel stays as it is, and only its edges change colour
        // when it comes or goes.
        let focused = cx.focus_within();
        if cx.repaint() || self.shown_focus != Some(focused) {
            let style = cx.theme().border(focused);
            let title_style = style.combine(&cx.theme().title);
            canvas.border(&self.title, &style, &title_style);
        }
        self.shown_focus = Some(focused);
    }

    fn retained(&self) -> bool {
        true
    }
}

/// Space round one child: [`Node::padding`].
pub(crate) struct Pad {
    pub child: Node,
    /// Top, right, bottom, left.
    pub edges: [u16; 4],
}

impl Widget for Pad {
    fn name(&self) -> &'static str {
        "padding"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let [top, right, bottom, left] = self.edges;
        let inner = cx.measure(
            &self.child,
            axis,
            width.saturating_sub(left + right),
            height.saturating_sub(top + bottom),
        );
        match axis {
            Axis::Vertical => inner.saturating_add(top + bottom),
            Axis::Horizontal => inner.saturating_add(left + right),
        }
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let [top, right, bottom, left] = self.edges;
        vec![Rect::new(
            rect.x.saturating_add(left),
            rect.y.saturating_add(top),
            rect.width.saturating_sub(left.saturating_add(right)),
            rect.height.saturating_sub(top.saturating_add(bottom)),
        )]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// A row's height in an [`each`](crate::each) list: its fixed size, its
/// content for [`Size::Auto`], otherwise one row.
fn row_height(cx: &MeasureCx, child: &Node, width: u16) -> u16 {
    let height = match child.size {
        Size::Fixed(n) => n,
        Size::Auto => cx.measure(child, Axis::Vertical, width, 0),
        _ => 1,
    };
    height.min(child.max).max(child.min)
}

/// One child per key, kept by key: [`each`](crate::each).
pub(crate) struct Each<K, F> {
    pub keys: Rc<dyn Fn() -> Vec<K>>,
    pub build: F,
    pub order: Vec<K>,
    pub children: Vec<Node>,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node> Each<K, F> {
    /// Bring the children in line with the keys.
    fn reconcile(&mut self) {
        let keys = (self.keys)();
        if keys == self.order {
            return;
        }
        let mut old: HashMap<K, Node> = self.order.drain(..).zip(self.children.drain(..)).collect();
        let runtime = Runtime::current();
        for key in &keys {
            // Built untracked: what a new row reads is the row's, not the
            // list's.
            let node = old
                .remove(key)
                .unwrap_or_else(|| runtime.untracked(|| (self.build)(key.clone())));
            self.children.push(node);
        }
        for (_, gone) in old {
            gone.forget(&runtime);
        }
        self.order = keys;
    }
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node + 'static> Widget for Each<K, F> {
    fn name(&self) -> &'static str {
        "each"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("[{}]", self.children.len()))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        self.reconcile();
        match axis {
            Axis::Vertical => self.children.iter().fold(0u16, |sum, child| {
                sum.saturating_add(row_height(cx, child, width))
            }),
            Axis::Horizontal => self
                .children
                .iter()
                .map(|child| cx.measure(child, axis, width, height))
                .max()
                .unwrap_or(0),
        }
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        self.reconcile();
        let mut y = rect.y;
        self.children
            .iter()
            .map(|child| {
                if y >= rect.bottom() {
                    // Below the viewport: not drawn, so it draws afresh
                    // whenever it comes into view.
                    return Rect::default();
                }
                let height = row_height(cx, child, rect.width).min(rect.bottom() - y);
                let area = Rect::new(rect.x, y, rect.width, height);
                y += height;
                area
            })
            .collect()
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// One child at a time, chosen by a key, the others kept:
/// [`switch`](crate::switch).
pub(crate) struct Switch<K, F> {
    pub key: Box<dyn Fn() -> K>,
    pub build: F,
    pub built: HashMap<K, Node>,
    pub current: Option<K>,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node> Switch<K, F> {
    /// Read the key (subscribing to what it reads), and build its child,
    /// untracked, if it is new.
    fn settle(&mut self) {
        let key = (self.key)();
        if !self.built.contains_key(&key) {
            let node = Runtime::current().untracked(|| (self.build)(key.clone()));
            self.built.insert(key.clone(), node);
        }
        self.current = Some(key);
    }

    fn shown(&self) -> Option<&Node> {
        self.current.as_ref().and_then(|key| self.built.get(key))
    }
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node + 'static> Widget for Switch<K, F> {
    fn name(&self) -> &'static str {
        "switch"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        self.settle();
        self.shown()
            .map_or(0, |child| cx.measure(child, axis, width, height))
    }

    fn children(&self) -> &[Node] {
        self.shown().map(std::slice::from_ref).unwrap_or(&[])
    }

    fn hidden_children(&self) -> Vec<&Node> {
        let shown = self.shown().map(|node| node.id());
        self.built
            .values()
            .filter(|node| Some(node.id()) != shown)
            .collect()
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        self.settle();
        vec![rect]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// A `rich-interact` component living in the tree.
pub(crate) trait Hosted {
    fn handle(&mut self, event: &Event, context: &Context<'_>, cx: &mut Ctx) -> Used;
    fn render(&self, context: &Context<'_>) -> rich_interact::View;
    fn set_cancel(&mut self, cancel: Handler);
}

type OnDone<T> = Box<dyn FnMut(T, &mut Ctx)>;

pub(crate) struct Host<C: Component> {
    pub component: C,
    /// Makes a fresh component after each answer ([`repeating`](crate::repeating)).
    pub make: Option<Box<dyn Fn() -> C>>,
    pub on_done: OnDone<C::Output>,
    pub on_cancel: Option<Handler>,
}

impl<C: Component> Hosted for Host<C> {
    fn handle(&mut self, event: &Event, context: &Context<'_>, cx: &mut Ctx) -> Used {
        match self.component.handle(event, context) {
            Flow::Ignored => Used::No,
            Flow::Continue => Used::Yes,
            Flow::Done(value) => {
                if let Some(make) = &self.make {
                    self.component = make();
                }
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

    fn render(&self, context: &Context<'_>) -> rich_interact::View {
        self.component.render(context)
    }

    fn set_cancel(&mut self, cancel: Handler) {
        self.on_cancel = Some(cancel);
    }
}

/// A hosted component as a widget: [`component`](crate::component).
pub(crate) struct HostWidget {
    pub host: Box<dyn Hosted>,
    caret: Option<(u16, u16)>,
}

impl HostWidget {
    pub fn new(host: Box<dyn Hosted>) -> HostWidget {
        HostWidget { host, caret: None }
    }
}

impl Widget for HostWidget {
    fn name(&self) -> &'static str {
        "component"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let context = Context {
            console: cx.console(),
            width: width.max(1) as usize,
            height: if height == 0 {
                u16::MAX as usize
            } else {
                height as usize
            },
        };
        extent(&self.host.render(&context).lines, axis)
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        let context = Context {
            console: cx.console(),
            width: width as usize,
            height: height as usize,
        };
        // A component that reads signals while it renders (a ratatui
        // widget drawing app state) draws again when they change.
        let view = self.host.render(&context);
        canvas.lines(&view.lines);
        self.caret = view.cursor.and_then(|(row, column)| {
            let (row, column) = (row as u16, column as u16);
            (row < height && column < width).then_some((column, row))
        });
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let event = match event {
            WidgetEvent::Key(key) => Event::Key(*key),
            WidgetEvent::Paste(text) => Event::Paste(text.clone()),
            // A component sees the mouse only while it has the focus (a
            // press focuses it first).
            WidgetEvent::Mouse(mouse) if cx.focused() => Event::Mouse(*mouse),
            _ => return Used::No,
        };
        let console = cx.console().clone();
        let (width, height) = cx.size();
        let context = Context {
            console: &console,
            width: width as usize,
            height: height as usize,
        };
        let used = self.host.handle(&event, &context, cx.app());
        if used == Used::Yes {
            cx.redraw();
        }
        used
    }

    fn focusable(&self) -> bool {
        true
    }

    fn caret(&self) -> Option<(u16, u16)> {
        self.caret
    }
}

/// A viewport onto one child laid out at its full height:
/// [`scroll`](crate::scroll).
pub(crate) struct ScrollView {
    pub child: Node,
    /// The first row in view.
    pub offset: Signal<u16>,
    /// The content's width (less the scrollbar's column) and height.
    content: (u16, u16),
    /// Rows in view.
    rows: u16,
    /// The focused node inside when it was last scrolled into view.
    focus: Option<NodeId>,
}

impl ScrollView {
    pub fn new(child: Node, offset: Signal<u16>) -> ScrollView {
        ScrollView {
            child,
            offset,
            content: (0, 0),
            rows: 1,
            focus: None,
        }
    }

    fn by(&self, rows: i32) {
        let max = self.content.1.saturating_sub(self.rows) as i32;
        self.offset
            .update(|o| *o = (*o as i32 + rows).clamp(0, max.max(0)) as u16);
    }

    fn top(&self) -> u16 {
        self.offset
            .get_untracked()
            .min(self.content.1.saturating_sub(self.rows))
    }
}

impl Widget for ScrollView {
    fn name(&self) -> &'static str {
        "scroll"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        cx.measure(&self.child, axis, width, height)
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn viewport(&self) -> bool {
        true
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        // A scrollbar takes the last column when the content is taller.
        let full = cx.measure(&self.child, Axis::Vertical, rect.width, 0);
        let (w, h) = if full > rect.height && rect.width > 1 {
            let w = rect.width - 1;
            (w, cx.measure(&self.child, Axis::Vertical, w, 0))
        } else {
            (rect.width, full)
        };
        self.offset.get();
        self.rows = rect.height;
        self.content = (w, h.max(rect.height).min(MAX_SCROLL_ROWS));
        vec![Rect::new(0, 0, self.content.0, self.content.1)]
    }

    fn scroll(&mut self, cx: &ScrollCx) -> (Rect, (u16, u16)) {
        // Keep a newly focused node inside in view, once it is laid out.
        let focused = cx.focused();
        if focused.map(|(id, _)| id) != self.focus {
            self.focus = focused.map(|(id, _)| id);
            if let Some((_, area)) = focused.filter(|(_, area)| !area.is_empty()) {
                let top = self.top();
                let bottom = top.saturating_add(self.rows);
                let next = if area.y < top {
                    area.y
                } else if area.bottom() > bottom {
                    area.bottom().saturating_sub(self.rows)
                } else {
                    top
                };
                if next != top {
                    Runtime::current().untracked(|| self.offset.set(next));
                }
            }
        }
        (Rect::new(0, 0, self.content.0, self.rows), (0, self.top()))
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        // The focus moving inside may scroll the view.
        cx.focus_within();
        let (width, content) = self.content;
        if width < canvas.width() && content > self.rows {
            let lines = crate::node::scrollbar(cx.console(), self.rows, self.top(), content);
            canvas.lines_at(canvas.width() - 1, 0, 1, self.rows, &lines);
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let page = self.rows.max(2) as i32 - 1;
        match event {
            WidgetEvent::Key(key) if key.modifiers == rich_interact::Modifiers::NONE => {
                match key.code {
                    KeyCode::Up => self.by(-1),
                    KeyCode::Down => self.by(1),
                    KeyCode::PageUp => self.by(-page),
                    KeyCode::PageDown => self.by(page),
                    KeyCode::Home => self.offset.set(0),
                    KeyCode::End => self.offset.set(MAX_SCROLL_ROWS),
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::ScrollUp => self.by(-3),
                MouseKind::ScrollDown => self.by(3),
                _ => return Used::No,
            },
            _ => return Used::No,
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}
