//! The built-in nodes, each a [`Widget`]: text and other leaves, columns
//! and rows, grids, panels, padding, keyed lists, switches, hosted
//! components and scrolls. They use only what the trait gives any widget,
//! which is the test that it is enough.

use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use rich::{Console, Segment, Style};
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
/// The widest content a view that scrolls across lays out.
pub(crate) const MAX_SCROLL_COLUMNS: u16 = 2000;

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
        let shown = self.children.iter().filter(|c| !c.hidden()).count();
        let gaps = self.gap.saturating_mul(shown.saturating_sub(1) as u16);
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
    /// Whether code gave the columns (else a stylesheet may).
    pub code_columns: bool,
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
    // Children a stylesheet hides take no cell and get an empty rectangle.
    let children: Vec<&Node> = grid.children.iter().filter(|c| !c.hidden()).collect();
    let mut shown = shown_areas(cx, grid, &children, rect, axis).into_iter();
    grid.children
        .iter()
        .map(|c| match c.hidden() {
            true => Rect::default(),
            false => shown.next().unwrap_or_default(),
        })
        .collect()
}

fn shown_areas(
    cx: &MeasureCx,
    grid: &Grid,
    children: &[&Node],
    rect: Rect,
    axis: Axis,
) -> Vec<Rect> {
    let columns = grid.columns.len().max(1);
    let spans: Vec<(u16, u16)> = children.iter().map(|c| c.span).collect();
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
                    .zip(children)
                    .filter(|(p, _)| p.column == c && p.columns == 1)
                    .map(|(_, child)| cx.measure(child, Axis::Horizontal, rect.width, rect.height))
                    .max()
                    .unwrap_or(0);
            }
            track
        })
        .collect();
    // Children spanning several columns grow the content columns they span.
    for (p, child) in places.iter().zip(children) {
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
                    .zip(children)
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
    for (p, child) in places.iter().zip(children) {
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
    /// What a stylesheet says about it.
    pub look: PanelLook,
}

/// A stylesheet's border, title and padding for a [`Panel`].
#[derive(Clone, Debug, Default)]
pub(crate) struct PanelLook {
    pub border: Option<(crate::sheet::BoxKind, Style)>,
    /// `border: none`: no box, and no cells kept for one.
    pub no_border: bool,
    pub title: Option<String>,
    /// Inside the border: top, right, bottom, left.
    pub padding: [u16; 4],
}

impl Panel {
    /// The cells its border takes on each side: none after `border: none`.
    fn edge(&self) -> u16 {
        u16::from(!self.look.no_border)
    }

    pub fn new(title: String, child: Node) -> Panel {
        Panel {
            title,
            child,
            shown_focus: None,
            look: PanelLook::default(),
        }
    }

    /// The title: the one code gave, else the stylesheet's.
    fn title(&self) -> &str {
        match (&self.title[..], &self.look.title) {
            ("", Some(title)) => title,
            (title, _) => title,
        }
    }
}

impl Widget for Panel {
    fn name(&self) -> &'static str {
        "panel"
    }

    fn describe(&self) -> Option<String> {
        let title = self.title();
        (!title.is_empty()).then(|| format!("\"{title}\""))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let [top, right, bottom, left] = self.look.padding;
        let edges = self.edge() * 2;
        let (across, down) = (left + right + edges, top + bottom + edges);
        let height = if height == 0 {
            0
        } else {
            height.saturating_sub(down)
        };
        cx.measure(&self.child, axis, width.saturating_sub(across), height)
            .saturating_add(match axis {
                Axis::Vertical => down,
                Axis::Horizontal => across,
            })
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let inner = rect.inner(self.edge());
        let [top, right, bottom, left] = self.look.padding;
        vec![Rect::new(
            inner.x.saturating_add(left),
            inner.y.saturating_add(top),
            inner.width.saturating_sub(left + right),
            inner.height.saturating_sub(top + bottom),
        )]
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        // The focus is all a panel reads: when it moves elsewhere in the
        // app, the panel stays as it is, and only its edges change colour
        // when it comes or goes.
        let focused = cx.focus_within();
        if self.look.no_border {
            return;
        }
        if cx.repaint() || self.shown_focus != Some(focused) {
            // A stylesheet's border sets the box, and the colour while the
            // focus is elsewhere.
            let (kind, style) = match &self.look.border {
                Some((kind, style)) if !focused && !style.is_null() => (*kind, style.clone()),
                Some((kind, _)) => (*kind, cx.theme().border(focused)),
                None => (crate::sheet::BoxKind::Round, cx.theme().border(focused)),
            };
            let title_style = style.combine(&cx.theme().title);
            let title = self.title().to_string();
            canvas.border_box(kind, &title, &style, &title_style);
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
    /// The first column in view.
    pub x_offset: Signal<u16>,
    /// Which ways it scrolls: down (the content is as wide as the view and
    /// as tall as it needs) and across (as wide as it needs).
    vertical: bool,
    horizontal: bool,
    /// The content's width and height.
    content: (u16, u16),
    /// The view's columns and rows, less the scrollbars.
    view: (u16, u16),
    /// The focused node inside when it was last scrolled into view.
    focus: Option<NodeId>,
}

impl ScrollView {
    pub fn new(child: Node, offset: Signal<u16>) -> ScrollView {
        ScrollView::both(child, crate::reactive::signal(0), offset, false, true)
    }

    pub fn both(
        child: Node,
        x_offset: Signal<u16>,
        offset: Signal<u16>,
        horizontal: bool,
        vertical: bool,
    ) -> ScrollView {
        ScrollView {
            child,
            offset,
            x_offset,
            vertical,
            horizontal,
            content: (0, 0),
            view: (1, 1),
            focus: None,
        }
    }

    fn by(&self, rows: i32) {
        let max = self.content.1.saturating_sub(self.view.1) as i32;
        self.offset
            .update(|o| *o = (*o as i32 + rows).clamp(0, max.max(0)) as u16);
    }

    fn across(&self, columns: i32) {
        let max = self.content.0.saturating_sub(self.view.0) as i32;
        self.x_offset
            .update(|o| *o = (*o as i32 + columns).clamp(0, max.max(0)) as u16);
    }

    fn top(&self) -> u16 {
        self.offset
            .get_untracked()
            .min(self.content.1.saturating_sub(self.view.1))
    }

    fn left(&self) -> u16 {
        self.x_offset
            .get_untracked()
            .min(self.content.0.saturating_sub(self.view.0))
    }
}

/// The first of `start..start + len` to show so that `from..to` is in
/// view: unchanged if it already is.
fn into_view(start: u16, len: u16, from: u16, to: u16) -> u16 {
    if from < start {
        from
    } else if to > start.saturating_add(len) {
        to.saturating_sub(len)
    } else {
        start
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
        // Across, the content is as wide as it asks (at most
        // MAX_SCROLL_COLUMNS); down, as tall as it needs at that width.
        // Each scrollbar takes a line when the content overflows that way:
        // the last column for down, the last row for across.
        let wide = |width: u16| {
            if self.horizontal {
                // A viewport wider than the cap is the content's width.
                cx.measure(&self.child, Axis::Horizontal, MAX_SCROLL_COLUMNS, 0)
                    .min(MAX_SCROLL_COLUMNS)
                    .max(width)
            } else {
                width
            }
        };
        let tall = |width: u16| {
            if self.vertical {
                cx.measure(&self.child, Axis::Vertical, width, 0)
            } else {
                rect.height
            }
        };
        let (mut view_w, mut view_h) = (rect.width, rect.height);
        let mut w = wide(view_w);
        let mut h = tall(w);
        // The two bars can each make room the other needs: settle in two
        // passes.
        for _ in 0..2 {
            if self.vertical && h > view_h && rect.width > 1 {
                view_w = rect.width - 1;
            }
            if self.horizontal && w > view_w && rect.height > 1 {
                view_h = rect.height - 1;
            }
            w = wide(view_w);
            h = tall(w);
        }
        self.offset.get();
        self.x_offset.get();
        self.view = (view_w, view_h);
        self.content = (
            w.max(view_w),
            if self.vertical {
                h.max(view_h).min(MAX_SCROLL_ROWS)
            } else {
                view_h
            },
        );
        vec![Rect::new(0, 0, self.content.0, self.content.1)]
    }

    fn scroll(&mut self, cx: &ScrollCx) -> (Rect, (u16, u16)) {
        // Keep a newly focused node inside in view, once it is laid out.
        let focused = cx.focused();
        if focused.map(|(id, _)| id) != self.focus {
            self.focus = focused.map(|(id, _)| id);
            if let Some((_, area)) = focused.filter(|(_, area)| !area.is_empty()) {
                let (left, top) = (self.left(), self.top());
                let next_top = into_view(top, self.view.1, area.y, area.bottom());
                let next_left = into_view(left, self.view.0, area.x, area.right());
                if self.vertical && next_top != top {
                    Runtime::current().untracked(|| self.offset.set(next_top));
                }
                if self.horizontal && next_left != left {
                    Runtime::current().untracked(|| self.x_offset.set(next_left));
                }
            }
        }
        (
            Rect::new(0, 0, self.view.0, self.view.1),
            (self.left(), self.top()),
        )
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        // The focus moving inside may scroll the view.
        cx.focus_within();
        let (content_w, content_h) = self.content;
        let (view_w, view_h) = self.view;
        if view_w < canvas.width() && content_h > view_h {
            let lines = crate::node::scrollbar(cx.console(), view_h, self.top(), content_h);
            canvas.lines_at(canvas.width() - 1, 0, 1, view_h, &lines);
        }
        if view_h < canvas.height() && content_w > view_w {
            let line = crate::node::scrollbar_across(cx.console(), view_w, self.left(), content_w);
            canvas.lines_at(0, canvas.height() - 1, view_w, 1, &[line]);
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let page = self.view.1.max(2) as i32 - 1;
        let page_across = self.view.0.max(2) as i32 - 1;
        match event {
            WidgetEvent::Key(key) if key.modifiers == rich_interact::Modifiers::NONE => {
                match key.code {
                    KeyCode::Up if self.vertical => self.by(-1),
                    KeyCode::Down if self.vertical => self.by(1),
                    KeyCode::PageUp if self.vertical => self.by(-page),
                    KeyCode::PageDown if self.vertical => self.by(page),
                    KeyCode::Left if self.horizontal => self.across(-1),
                    KeyCode::Right if self.horizontal => self.across(1),
                    KeyCode::Home => {
                        self.offset.set(0);
                        self.x_offset.set(0);
                    }
                    KeyCode::End if self.vertical => self.offset.set(MAX_SCROLL_ROWS),
                    KeyCode::End => self.x_offset.set(MAX_SCROLL_COLUMNS),
                    _ => return Used::No,
                }
            }
            WidgetEvent::Key(key)
                if key.modifiers.shift
                    && !key.modifiers.ctrl
                    && !key.modifiers.alt
                    && self.horizontal =>
            {
                match key.code {
                    KeyCode::PageUp => self.across(-page_across),
                    KeyCode::PageDown => self.across(page_across),
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => {
                // Shift with the wheel, or the wheel in a view that only
                // scrolls across, scrolls across.
                let across = self.horizontal && (mouse.modifiers.shift || !self.vertical);
                match (mouse.kind, across) {
                    (MouseKind::ScrollUp, false) => self.by(-3),
                    (MouseKind::ScrollDown, false) => self.by(3),
                    (MouseKind::ScrollUp, true) => self.across(-6),
                    (MouseKind::ScrollDown, true) => self.across(6),
                    _ => return Used::No,
                }
            }
            _ => return Used::No,
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}
