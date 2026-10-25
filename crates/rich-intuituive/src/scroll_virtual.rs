//! A scroll view over items that builds only those near the view:
//! [`scroll_virtual`].
//!
//! Items have an estimated height, corrected once an item is drawn. The
//! view keeps the items in view plus one screen of overscan either side,
//! by key, and drops the rest. Its offset is a `u64` of rows, so it has no
//! row cap; the scrollbar comes from the heights. When items above the
//! view change height or are inserted, the top item in view keeps its row.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use rich_interact::{KeyCode, MouseKind};

use crate::app::Ctx;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::reactive::{Runtime, Signal};
use crate::screen::Rect;
use crate::scrollbar::{Bar, Scrollbar};
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, ScrollCx, Used, Widget, WidgetEvent};

/// Items per block of summed heights: finding a row walks the blocks, then
/// at most this many items.
const CHUNK: usize = 256;
/// How long the offset rests before stand-ins give way to full items.
const REST: Duration = Duration::from_millis(100);
/// The most rows the built items span: the content's coordinates are `u16`.
const MAX_SPAN: u32 = u16::MAX as u32;
/// How far either side of where an anchored item was it is looked for,
/// after its index stops holding its key.
const SEARCH: usize = 4096;

type NearEndFn = Rc<RefCell<dyn FnMut(&mut Ctx)>>;

/// [`Node::on_near_end`]'s hook.
struct NearEnd {
    threshold: u64,
    call: NearEndFn,
    /// `len` when it last fired: it fires again only once `len` grows.
    fired: Option<usize>,
}

/// A built item: its key and whether it is a stand-in.
struct Built {
    key: u64,
    stand_in: bool,
}

/// The widget behind [`scroll_virtual`].
pub(crate) struct VirtualScroll {
    len: Box<dyn Fn() -> usize>,
    estimate: Box<dyn Fn(usize) -> u16>,
    build: Box<dyn Fn(usize) -> Node>,
    key: Option<Box<dyn Fn(usize) -> u64>>,
    stand_in: Option<Box<dyn Fn(usize) -> Node>>,
    near_end: Option<NearEnd>,
    /// The first row in view, of all the items' rows.
    offset: Signal<u64>,
    /// Bumped when the offset has rested, so stand-ins are replaced.
    wake: Signal<u64>,
    /// The summed heights of each block of [`CHUNK`] items.
    chunks: Vec<u64>,
    /// How many items `chunks` covers.
    counted: usize,
    /// Heights of items drawn, by key, where they differ from the estimate.
    measured: HashMap<u64, u16>,
    /// The width items were measured at.
    width: u16,
    /// The top item in view, its key, and the rows of it above the view.
    anchor: usize,
    anchor_key: Option<u64>,
    within: u64,
    /// The offset as last laid out: one that differs was set from outside.
    derived: Option<u64>,
    /// The first row in view and all the rows, as last laid out.
    top: u64,
    total: u64,
    /// The first built item's index, the built items and their nodes.
    first: usize,
    built: Vec<Built>,
    children: Vec<Node>,
    /// The first row in view, in the built items' own rows.
    local_top: u16,
    /// The view's columns and rows, less the bar, and its own size.
    view: (u16, u16),
    pub(crate) bar: Bar,
    bar_rect: Option<Rect>,
    /// Level of detail: whether the offset is moving fast, the rows it
    /// moved since it last rested, and when it last moved.
    fast: bool,
    moved: u64,
    last_move: Option<Duration>,
    /// Whether the last layout asked for another, for the bar.
    rebar: bool,
}

impl VirtualScroll {
    fn key_of(&self, index: usize) -> u64 {
        match &self.key {
            Some(key) => key(index),
            None => index as u64,
        }
    }

    /// Item `index`'s height: as drawn, else its estimate; at least a row.
    fn height(&self, index: usize) -> u64 {
        let measured = if self.measured.is_empty() {
            None
        } else {
            self.measured.get(&self.key_of(index)).copied()
        };
        measured.unwrap_or_else(|| (self.estimate)(index)).max(1) as u64
    }

    /// Sum the blocks again from block `from` to `len` items.
    fn recount(&mut self, from: usize, len: usize) {
        let from = from.min(self.chunks.len());
        self.chunks.truncate(from);
        let mut start = from * CHUNK;
        while start < len {
            let end = (start + CHUNK).min(len);
            let sum = (start..end).map(|i| self.height(i)).sum();
            self.chunks.push(sum);
            start = end;
        }
        self.counted = len;
    }

    /// Record that item `index` is `rows` tall.
    fn set_height(&mut self, index: usize, rows: u16) {
        let rows = rows.max(1);
        let old = self.height(index);
        if old != rows as u64 {
            let key = self.key_of(index);
            if rows == (self.estimate)(index).max(1) {
                self.measured.remove(&key);
            } else {
                self.measured.insert(key, rows);
            }
            if let Some(sum) = self.chunks.get_mut(index / CHUNK) {
                *sum = *sum + rows as u64 - old;
            }
        }
    }

    /// The row item `index` starts at.
    fn top_of(&self, index: usize) -> u64 {
        let chunk = index / CHUNK;
        let before: u64 = self.chunks[..chunk.min(self.chunks.len())].iter().sum();
        before + (chunk * CHUNK..index).map(|i| self.height(i)).sum::<u64>()
    }

    /// The item at `row`, and the rows of it above `row`.
    fn index_at(&self, row: u64) -> (usize, u64) {
        let len = self.counted;
        if len == 0 {
            return (0, 0);
        }
        let mut at = 0u64;
        for (chunk, &sum) in self.chunks.iter().enumerate() {
            if row < at + sum {
                let mut i = chunk * CHUNK;
                let end = ((chunk + 1) * CHUNK).min(len);
                while i < end {
                    let h = self.height(i);
                    if row < at + h {
                        return (i, row - at);
                    }
                    at += h;
                    i += 1;
                }
                break;
            }
            at += sum;
        }
        let last = len - 1;
        (last, self.height(last) - 1)
    }

    /// Where the item keyed `key` went, near `from`, now `len` items are
    /// there instead of `old`.
    fn find(&self, key: u64, from: usize, old: usize, len: usize) -> Option<usize> {
        let shifted = if len >= old {
            from.checked_add(len - old)
        } else {
            from.checked_sub(old - len)
        };
        if let Some(i) = shifted.filter(|&i| i < len && self.key_of(i) == key) {
            return Some(i);
        }
        (1..=SEARCH).find_map(|d| {
            [from.checked_add(d), from.checked_sub(d)]
                .into_iter()
                .flatten()
                .find(|&i| i < len && self.key_of(i) == key)
        })
    }

    /// The rows the view can show at most from the top.
    fn max_top(&self) -> u64 {
        self.total.saturating_sub(self.view.1 as u64)
    }

    /// Move the view by `rows`.
    fn by(&self, rows: i64) {
        let to = (self.top as i64).saturating_add(rows).max(0) as u64;
        self.offset.set(to.min(self.max_top()));
    }

    /// Keep the built items in line with the view: those within one screen
    /// of it, reused by key. Whether anything changed.
    fn rebuild(&mut self, len: usize, top: u64, view_h: u64) -> bool {
        let over = view_h.max(1);
        // Kept while the items built cover half a screen either side of
        // the view, are the keys they were built for, and no stand-in
        // waits for a rest.
        let (need_lo, _) = self.index_at(top.saturating_sub(over / 2));
        let (need_hi, _) = self.index_at((top + view_h + over / 2).min(self.total.max(1) - 1));
        let end = self.first + self.built.len();
        let covered = !self.built.is_empty() && self.first <= need_lo && need_hi < end;
        let current = covered
            && self
                .built
                .iter()
                .enumerate()
                .all(|(k, b)| self.first + k < len && b.key == self.key_of(self.first + k));
        let waiting = !self.fast && self.built.iter().any(|b| b.stand_in);
        if current && !waiting {
            return false;
        }
        let (lo, _) = self.index_at(top.saturating_sub(over));
        let (hi, _) = self.index_at((top + view_h + over).min(self.total.max(1) - 1));
        let mut old: HashMap<u64, (Built, Node)> = self
            .built
            .drain(..)
            .zip(self.children.drain(..))
            .map(|(b, n)| (b.key, (b, n)))
            .collect();
        let runtime = Runtime::current();
        let lod = self.fast && self.stand_in.is_some();
        let mut span = 0u32;
        for i in lo..=hi.min(len - 1) {
            let key = self.key_of(i);
            let node = match old.remove(&key) {
                Some((built, node)) if !built.stand_in || lod => {
                    self.built.push(built);
                    node
                }
                stale => {
                    if let Some((_, node)) = stale {
                        node.forget(&runtime);
                    }
                    let stand_in = lod;
                    // Built untracked: what an item reads is its own.
                    let node = runtime.untracked(|| match (&self.stand_in, stand_in) {
                        (Some(make), true) => make(i),
                        _ => (self.build)(i),
                    });
                    self.built.push(Built { key, stand_in });
                    node
                }
            };
            self.children.push(node);
            span += self.height(i) as u32;
            if span >= MAX_SPAN && i >= self.anchor {
                break;
            }
        }
        self.first = lo;
        for (_, (_, node)) in old {
            node.forget(&runtime);
        }
        true
    }
}

/// An item's rows at `width`: its fixed size, else its content.
fn item_height(cx: &MeasureCx, child: &Node, width: u16) -> u16 {
    if child.hidden() {
        return 0;
    }
    let rows = match child.own_size() {
        Size::Fixed(n) => n,
        _ => cx.measure(child, Axis::Vertical, width, 0),
    };
    rows.min(child.max).max(child.min)
}

impl Widget for VirtualScroll {
    fn name(&self) -> &'static str {
        "scroll_virtual"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("{} built of {}", self.built.len(), self.counted))
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn viewport(&self) -> bool {
        true
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        self.wake.get();
        let len = (self.len)();
        let offset = self.offset.get();
        let runtime = Runtime::current();
        let now = runtime.now.get();
        let view_h = rect.height as u64;
        // Heights: summed for new items; estimates do not depend on the
        // width, so the bar can be decided from them.
        let old = self.counted;
        if self.chunks.is_empty() || len != old {
            self.recount(old.min(len) / CHUNK, len);
        }
        // The bar takes the last column once the rows overflow the view;
        // items are measured afresh at a new width.
        let look = self.bar.look.clone();
        let overflows = move |total: u64| {
            look.as_ref()
                .is_some_and(|look| look.wanted(total as usize, view_h as usize))
        };
        let wanted = overflows(self.chunks.iter().sum());
        let width = if wanted && rect.width > 1 {
            rect.width - 1
        } else {
            rect.width
        };
        self.view = (width, rect.height);
        if width != self.width {
            self.width = width;
            if !self.measured.is_empty() {
                self.measured.clear();
                self.recount(0, len);
            }
        }
        if len == 0 {
            let runtime = Runtime::current();
            for node in self.children.drain(..) {
                node.forget(&runtime);
            }
            self.built.clear();
            (
                self.top,
                self.total,
                self.local_top,
                self.anchor,
                self.within,
            ) = (0, 0, 0, 0, 0);
            self.anchor_key = None;
            self.derived = Some(0);
            if offset != 0 {
                runtime.untracked(|| self.offset.set(0));
            }
            self.near(len, 0);
            return Vec::new();
        }
        // The anchored item moved (items inserted or removed above it):
        // follow it by its key.
        if let Some(key) = self.anchor_key {
            let at = self.anchor.min(len - 1);
            if self.key.is_some() && self.key_of(at) != key {
                match self.find(key, self.anchor, old, len) {
                    Some(found) => {
                        self.anchor = found;
                        self.recount(0, len);
                    }
                    None => self.within = 0,
                }
            }
            self.anchor = self.anchor.min(len - 1);
        }
        self.total = self.chunks.iter().sum();
        // An offset set from outside (a key, the wheel, code) moves the
        // view; otherwise the anchored item keeps its row.
        let moved_by = match self.derived {
            Some(derived) if derived != offset => Some(derived.abs_diff(offset)),
            _ => None,
        };
        // Set to the end or past it, the view stays at the end as the
        // heights there are corrected.
        let to_end = moved_by.is_some() && offset >= self.max_top();
        if self.derived != Some(offset) {
            (self.anchor, self.within) = self.index_at(offset.min(self.max_top()));
        }
        // Level of detail: fast moves build stand-ins; a rest, full items.
        if self.stand_in.is_some() {
            let rested = |since: Option<Duration>| since.is_none_or(|t| now >= t + REST);
            if let Some(rows) = moved_by {
                if rested(self.last_move) {
                    self.moved = 0;
                }
                self.moved = self.moved.saturating_add(rows);
                self.last_move = Some(now);
                self.fast = self.moved >= 2 * view_h.max(1);
                let wake = self.wake;
                crate::app::after(REST, move |_| wake.update(|w| *w += 1));
            } else if rested(self.last_move) {
                self.fast = false;
                self.moved = 0;
            }
        }
        // Build, measure, and settle the view against the measured
        // heights: a few rounds, as corrections can move the end.
        let mut top = 0;
        for _ in 0..4 {
            top = self.top_of(self.anchor) + self.within;
            if top > self.max_top() || (to_end && top != self.max_top()) {
                (self.anchor, self.within) = self.index_at(self.max_top());
                top = self.max_top();
            }
            let rebuilt = self.rebuild(len, top, view_h);
            let mut changed = false;
            for k in 0..self.children.len() {
                if self.built[k].stand_in {
                    continue;
                }
                let rows = item_height(cx, &self.children[k], width);
                let index = self.first + k;
                if self.height(index) != rows.max(1) as u64 {
                    self.set_height(index, rows);
                    changed = true;
                }
            }
            self.total = self.chunks.iter().sum();
            if !rebuilt && !changed {
                break;
            }
        }
        let top_now = self.top_of(self.anchor) + self.within;
        if top_now != top {
            top = top_now.min(self.max_top());
            (self.anchor, self.within) = self.index_at(top);
        }
        self.top = top;
        self.anchor_key = Some(self.key_of(self.anchor));
        self.derived = Some(top);
        if offset != top {
            runtime.untracked(|| self.offset.set(top));
        }
        // The built items, one under the other, from the first's top.
        let mut y = 0u32;
        let mut rects = Vec::with_capacity(self.children.len());
        for k in 0..self.children.len() {
            let index = self.first + k;
            let rows = self.height(index) as u32;
            if index == self.anchor {
                self.local_top = (y + self.within as u32).min(MAX_SPAN) as u16;
            }
            if y + rows > MAX_SPAN {
                rects.push(Rect::default());
                continue;
            }
            rects.push(Rect::new(0, y as u16, width, rows as u16));
            y += rows;
        }
        self.near(len, top);
        // Corrections that made the rows overflow (or fit) change the bar:
        // lay out again, once.
        if overflows(self.total) != wanted && !self.rebar {
            self.rebar = true;
            let wake = self.wake;
            runtime.untracked(|| wake.update(|w| *w += 1));
        } else {
            self.rebar = false;
        }
        rects
    }

    fn scroll(&mut self, _cx: &ScrollCx) -> (Rect, (u16, u16)) {
        (
            Rect::new(0, 0, self.view.0, self.view.1),
            (0, self.local_top),
        )
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        self.bar_rect = None;
        let (view_w, view_h) = self.view;
        if view_w < canvas.width() {
            if let Some(look) = &self.bar.look {
                let rect = Rect::new(canvas.width() - 1, 0, 1, view_h);
                look.paint(
                    cx.console(),
                    canvas,
                    rect,
                    self.top as usize,
                    self.total as usize,
                    view_h as usize,
                );
                self.bar_rect = Some(rect);
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if let (WidgetEvent::Mouse(mouse), Some(rect)) = (event, self.bar_rect) {
            let view = self.view.1 as usize;
            match self.bar.mouse(
                cx,
                mouse,
                rect,
                self.top as usize,
                self.total as usize,
                view,
            ) {
                Some(Some(to)) => {
                    self.offset.set((to as u64).min(self.max_top()));
                    return Used::Yes;
                }
                Some(None) => return Used::Yes,
                None => {}
            }
        }
        let page = self.view.1.max(2) as i64 - 1;
        match event {
            WidgetEvent::Key(key) if key.modifiers == rich_interact::Modifiers::NONE => {
                match key.code {
                    KeyCode::Up => self.by(-1),
                    KeyCode::Down => self.by(1),
                    KeyCode::PageUp => self.by(-page),
                    KeyCode::PageDown => self.by(page),
                    KeyCode::Home => self.offset.set(0),
                    KeyCode::End => self.offset.set(u64::MAX),
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

impl VirtualScroll {
    /// Call the near-end hook when the view is within its threshold of the
    /// end, once until `len` grows.
    fn near(&mut self, len: usize, top: u64) {
        let Some(near) = &mut self.near_end else {
            return;
        };
        let left = self.total.saturating_sub(top + self.view.1 as u64);
        if left <= near.threshold && near.fired.is_none_or(|fired| len > fired) {
            near.fired = Some(len);
            let call = near.call.clone();
            crate::app::after(Duration::ZERO, move |cx| (call.borrow_mut())(cx));
        }
    }
}

/// A scroll view over `len()` items that builds only those in view, plus a
/// screen either side: item `i` is `row(i)`, estimated `height_of(i)` rows
/// tall (at least one) until it is drawn and measured (its
/// [fixed](Node::fixed) size, else its content's height). A million items
/// cost a screenful. Items are kept while they stay near the view, and
/// dropped (their state with them) once they leave it.
///
/// The offset is a `u64` of rows, so it has no row cap, and the scrollbar
/// is computed from the heights. ↑/↓, PgUp/PgDn, Home/End, the wheel and
/// the bar scroll it. `len` may read signals: when it grows, the new items
/// are counted. With [`item_key`](Node::item_key), the top item in view
/// keeps its row when items are inserted above it or change height;
/// [`on_near_end`](Node::on_near_end) loads more as the view nears the end;
/// [`stand_in`](Node::stand_in) draws cheap stand-ins while it scrolls fast.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     scroll_virtual(|| 1_000_000, |_| 1, |i| label(format!("item {i}")))
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["end", "q"], 16, 3).unwrap();
/// assert!(screen[2].starts_with("item 999999 "));
/// ```
pub fn scroll_virtual(
    len: impl Fn() -> usize + 'static,
    height_of: impl Fn(usize) -> u16 + 'static,
    row: impl Fn(usize) -> Node + 'static,
) -> Node {
    scroll_virtual_with(len, height_of, row, crate::reactive::signal(0))
}

/// [`scroll_virtual`], with the first row in view in `offset`: read it, or
/// set it to scroll from code (`u64::MAX` for the end).
pub fn scroll_virtual_with(
    len: impl Fn() -> usize + 'static,
    height_of: impl Fn(usize) -> u16 + 'static,
    row: impl Fn(usize) -> Node + 'static,
    offset: Signal<u64>,
) -> Node {
    let view = VirtualScroll {
        len: Box::new(len),
        estimate: Box::new(height_of),
        build: Box::new(row),
        key: None,
        stand_in: None,
        near_end: None,
        offset,
        wake: crate::reactive::signal(0),
        chunks: Vec::new(),
        counted: 0,
        measured: HashMap::new(),
        width: 0,
        anchor: 0,
        anchor_key: None,
        within: 0,
        derived: None,
        top: 0,
        total: 0,
        first: 0,
        built: Vec::new(),
        children: Vec::new(),
        local_top: 0,
        view: (1, 1),
        bar: Bar::new(Some(Scrollbar::new())),
        bar_rect: None,
        fast: false,
        moved: 0,
        last_move: None,
        rebar: false,
    };
    Node::from_widget(Box::new(view), "scroll_virtual").focusable()
}

impl Node {
    /// On a [`scroll_virtual`]: call `load` once the view comes within
    /// `threshold` rows of the end, and not again until its `len` grows,
    /// so one load is in flight at a time. The forever-scroll hook: `load`
    /// starts fetching the next page, and the view counts the new items
    /// when they arrive. It runs on the next turn of the app's loop, as
    /// [`after`](crate::after) does.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let len = signal(20usize);
    ///     scroll_virtual(move || len.get(), |_| 1, |i| label(format!("item {i}")))
    ///         .on_near_end(5, move |_| len.update(|n| *n += 20))
    ///         .on_key("q", |cx| cx.quit())
    /// });
    /// // The first End reaches item 19 and loads twenty more; the next
    /// // reaches those.
    /// let screen = app.render_with(&["end", "end", "q"], 16, 3).unwrap();
    /// assert!(screen[2].starts_with("item 39 "));
    /// ```
    pub fn on_near_end(self, threshold: u64, load: impl FnMut(&mut Ctx) + 'static) -> Node {
        let call: NearEndFn = Rc::new(RefCell::new(load));
        self.widget_mut(|view: &mut VirtualScroll| {
            view.near_end = Some(NearEnd {
                threshold,
                call,
                fired: None,
            })
        });
        self
    }

    /// On a [`scroll_virtual`]: item `i`'s stable key, so an item keeps
    /// its node and its measured height when items are inserted or removed
    /// around it, and the top item in view keeps its row. Without it, an
    /// item is keyed by its index.
    pub fn item_key(self, key: impl Fn(usize) -> u64 + 'static) -> Node {
        self.widget_mut(|view: &mut VirtualScroll| view.key = Some(Box::new(key)));
        self
    }

    /// On a [`scroll_virtual`]: while the view scrolls fast (two screens
    /// or more before it rests for 100 ms), items not built yet are built
    /// by `make`, a cheap stand-in (a skeleton row, text without its
    /// images), drawn at the item's estimated height. Full items replace
    /// them once the view rests. A game's level of detail.
    pub fn stand_in(self, make: impl Fn(usize) -> Node + 'static) -> Node {
        self.widget_mut(|view: &mut VirtualScroll| view.stand_in = Some(Box::new(make)));
        self
    }
}
