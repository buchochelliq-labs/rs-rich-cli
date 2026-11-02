//! Lists whose rows are nodes: Textual's `ListView` of `ListItem`s, and
//! `OptionList`'s separators and disabled options.
//!
//! [`list_of`] puts `len()` rows in a [`scroll`](crate::scroll), each built
//! by `build_row(i)` from any nodes (a card, a row with a label and a
//! button), with a cursor the list's keys move. The selected row matches a
//! stylesheet's `:selected`, so `:selected .title { text-style: bold }`
//! styles its children.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use rich_interact::{Button, KeyCode, MouseKind};

use crate::app::Ctx;
use crate::node::{Axis, Node, with_node};
use crate::reactive::{NodeId, Signal, signal};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// A handler of an item.
pub(crate) type ItemHandler = Box<dyn FnMut(usize, &mut Ctx)>;
type OnItem = Rc<RefCell<Option<ItemHandler>>>;
type Of<T> = Rc<RefCell<Option<Box<dyn Fn(usize) -> T>>>>;

/// What [`list_of`]'s options set, shared with its rows.
#[derive(Default)]
pub(crate) struct Shared {
    key: Of<u64>,
    separator: Of<bool>,
    disabled: Of<bool>,
    on_select: OnItem,
    on_highlight: OnItem,
    on_context: OnItem,
    padding: Cell<usize>,
    /// Each row's node, by its item's key, as last built.
    rows: RefCell<HashMap<u64, NodeId>>,
    /// Each key's item now.
    index: RefCell<HashMap<u64, usize>>,
}

impl Shared {
    fn read<T>(of: &Of<T>, i: usize, or: T) -> T {
        of.borrow().as_ref().map_or(or, |f| f(i))
    }

    fn disabled(&self, i: usize) -> bool {
        Self::read(&self.disabled, i, false)
    }

    fn key(&self, i: usize) -> u64 {
        Self::read(&self.key, i, i as u64)
    }

    /// The item keyed `key` now.
    fn item(&self, key: u64) -> Option<usize> {
        self.index.borrow().get(&key).copied()
    }
}

/// The list: the scroll it shows its rows in, and the cursor's keys.
pub(crate) struct NodeList {
    child: Node,
    len: Rc<dyn Fn() -> usize>,
    selected: Signal<usize>,
    offset: Signal<u16>,
    pub(crate) shared: Rc<Shared>,
}

/// A list of `len()` rows, each the node `build_row(i)` makes (any nodes:
/// a card, a row with a label and a button), with the cursor on
/// `selected`. ↑/↓, Home/End and PgUp/PgDn move it, skipping
/// [disabled](Node::item_disabled_when) rows, and keep it in view
/// ([`scroll_padding`](Node::scroll_padding) rows either side); Enter or a
/// click on a row [selects](Node::on_select) it, unless a node inside the
/// row took the click (a button in a card runs the button). The wheel
/// scrolls the view.
///
/// The selected row matches a stylesheet's `:selected`, so its children
/// can be styled (`:selected .title { text-style: bold }`). A row is built
/// once and kept while its [key](Node::item_key) stays, with its state
/// (`build_row` gets the index it is built at; what it shows should follow
/// its item's key once items move); `len` and the options may read
/// signals. All rows are laid out, as a
/// [`scroll`](crate::scroll)'s content is: for very long lists, see
/// [`virtual_list`](crate::widgets::virtual_list).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::list_of;
///
/// let app = App::new(|| {
///     let picked = signal(0usize);
///     list_of(|| 3, |i| label(format!("card {i}")).name("card"), picked)
///         .item_disabled_when(|i| i == 1)
///         .separator_after(|i| i == 0)
/// });
/// let mut driver = app.driver(10, 4);
/// driver.press("down");
/// // The separator and the disabled card are skipped.
/// assert!(driver.screen().plain()[3].starts_with("card 2"));
/// ```
pub fn list_of(
    len: impl Fn() -> usize + 'static,
    build_row: impl Fn(usize) -> Node + 'static,
    selected: Signal<usize>,
) -> Node {
    let shared = Rc::new(Shared::default());
    let len: Rc<dyn Fn() -> usize> = Rc::new(len);
    let build_row: Rc<dyn Fn(usize) -> Node> = Rc::new(build_row);
    // Each row is keyed by its item's key and whether it is disabled or
    // followed by a separator, so a change of either builds it again.
    let keys = {
        let (shared, len) = (shared.clone(), len.clone());
        move || {
            let keys: Vec<(u64, bool, bool)> = (0..len())
                .map(|i| {
                    (
                        shared.key(i),
                        shared.disabled(i),
                        Shared::read(&shared.separator, i, false),
                    )
                })
                .collect();
            *shared.index.borrow_mut() = keys
                .iter()
                .enumerate()
                .map(|(i, (key, _, _))| (*key, i))
                .collect();
            keys
        }
    };
    let build = {
        let shared = shared.clone();
        move |(key, disabled, separated): (u64, bool, bool)| {
            let i = shared.item(key).unwrap_or(0);
            let mut parts = vec![build_row(i)];
            if separated {
                parts.push(crate::rule::rule().fixed(1));
            }
            let here = shared.clone();
            let mut row = crate::node::column(parts)
                .auto()
                .selected_when(move || here.item(key) == Some(selected.get()))
                .disabled_when(move || disabled);
            if disabled {
                row = row.style("dim");
            }
            let (pick, menu) = (shared.clone(), shared.clone());
            let row = row
                .on_click(move |cx| {
                    if let Some(i) = pick.item(key).filter(|i| !pick.disabled(*i)) {
                        selected.set(i);
                        call(&pick.on_highlight, i, cx);
                        call(&pick.on_select, i, cx);
                    }
                })
                .on_mouse(move |cx, mouse| {
                    let Some(i) = menu.item(key) else {
                        return false;
                    };
                    if mouse.kind != MouseKind::Down(Button::Right) {
                        return false;
                    }
                    let used = menu.on_context.borrow().is_some();
                    call(&menu.on_context, i, cx);
                    used
                });
            shared.rows.borrow_mut().insert(key, row.id());
            row
        }
    };
    let offset = signal(0u16);
    let content = crate::node::each(keys, build);
    let child = crate::node::scroll_with(content, offset).no_focus();
    Node::from_widget(
        Box::new(NodeList {
            child,
            len,
            selected,
            offset,
            shared,
        }),
        "list_of",
    )
    .focusable()
}

fn call(handler: &OnItem, i: usize, cx: &mut Ctx) {
    if let Some(handler) = handler.borrow_mut().as_mut() {
        handler(i, cx);
    }
}

impl NodeList {
    /// Item `i`'s rectangle within the content, as last laid out.
    fn row_rect(&self, i: usize) -> Option<Rect> {
        let id = *self.shared.rows.borrow().get(&self.shared.key(i))?;
        let mut content_top = None;
        self.child
            .each_child(&mut |content| content_top = Some(content.rect().y));
        let mut found = None;
        with_node(&self.child, id, &mut |node| found = Some(node.rect()));
        let rect = found.filter(|rect| rect.height > 0)?;
        Some(Rect::new(
            rect.x,
            rect.y.saturating_sub(content_top.unwrap_or(rect.y)),
            rect.width,
            rect.height,
        ))
    }

    /// The view's height.
    fn view_height(&self) -> u16 {
        self.child.rect().height
    }

    /// Scroll so item `i` (and the padding either side, where it fits) is
    /// in view.
    fn reveal(&self, i: usize, len: usize) {
        let pad = self.shared.padding.get();
        let span = |j: usize| self.row_rect(j);
        let (Some(at), view) = (span(i), self.view_height()) else {
            return;
        };
        if view == 0 {
            return;
        }
        let top = span(i.saturating_sub(pad)).map_or(at.y, |r| r.y.min(at.y));
        let bottom = span((i + pad).min(len.saturating_sub(1)))
            .map_or(at.bottom(), |r| r.bottom().max(at.bottom()));
        let offset = self.offset.get_untracked();
        let next = if top < offset {
            top
        } else if bottom > offset.saturating_add(view) {
            // Its top stays in view when it is taller than the view.
            bottom.saturating_sub(view).min(at.y)
        } else {
            return;
        };
        self.offset.set(next);
    }

    /// The next item from `from` that is not disabled, `step` at a time
    /// (stopping at either end); `from` itself when none is.
    fn next_enabled(&self, from: usize, forward: bool, len: usize) -> usize {
        let mut i = from;
        loop {
            let next = if forward {
                (i + 1 < len).then_some(i + 1)
            } else {
                i.checked_sub(1)
            };
            match next {
                Some(n) if self.shared.disabled(n) => i = n,
                Some(n) => return n,
                None => return from,
            }
        }
    }

    /// The first item not disabled from `from` toward the end (`forward`)
    /// or the start, `from` included.
    fn first_enabled(&self, from: usize, forward: bool, len: usize) -> Option<usize> {
        let mut i = from;
        loop {
            if !self.shared.disabled(i) {
                return Some(i);
            }
            i = if forward {
                (i + 1 < len).then_some(i + 1)?
            } else {
                i.checked_sub(1)?
            };
        }
    }
}

impl Widget for NodeList {
    fn name(&self) -> &'static str {
        "list_of"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        cx.measure(&self.child, axis, width, height)
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![rect]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }

    fn focusable(&self) -> bool {
        true
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::List
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let WidgetEvent::Key(key) = event else {
            return Used::No;
        };
        if key.modifiers.ctrl || key.modifiers.alt {
            return Used::No;
        }
        let len = (self.len)();
        if len == 0 {
            return Used::No;
        }
        let at = self.selected.get_untracked().min(len - 1);
        let page = || {
            let height = self.row_rect(at).map_or(1, |r| r.height.max(1));
            (self.view_height() / height).max(1) as usize
        };
        let to = match key.code {
            KeyCode::Down => self.next_enabled(at, true, len),
            KeyCode::Up => self.next_enabled(at, false, len),
            KeyCode::Home => self.first_enabled(0, true, len).unwrap_or(at),
            KeyCode::End => self.first_enabled(len - 1, false, len).unwrap_or(at),
            KeyCode::PageDown => {
                let target = (at + page()).min(len - 1);
                self.first_enabled(target, false, len).unwrap_or(at)
            }
            KeyCode::PageUp => {
                let target = at.saturating_sub(page());
                self.first_enabled(target, true, len).unwrap_or(at)
            }
            KeyCode::Enter => {
                if !self.shared.disabled(at) {
                    call(&self.shared.on_select, at, cx.app());
                }
                return Used::Yes;
            }
            _ => return Used::No,
        };
        if to != at {
            self.selected.set(to);
            call(&self.shared.on_highlight, to, cx.app());
        }
        self.reveal(to, len);
        Used::Yes
    }
}

impl Node {
    /// Run `handler` with the row's item as the cursor of a [`list_of`]
    /// moves to it (a key, a click).
    pub fn on_highlight(self, handler: impl FnMut(usize, &mut Ctx) + 'static) -> Node {
        self.widget_mut(|list: &mut NodeList| {
            *list.shared.on_highlight.borrow_mut() = Some(Box::new(handler));
        });
        self
    }

    /// On a [`list_of`]: draw a rule under item `i` while `after(i)` holds
    /// (OptionList's `None` between options). The cursor never stops on it.
    pub fn separator_after(self, after: impl Fn(usize) -> bool + 'static) -> Node {
        self.widget_mut(|list: &mut NodeList| {
            *list.shared.separator.borrow_mut() = Some(Box::new(after));
        });
        self
    }

    /// On a [`list_of`]: item `i` is disabled while `disabled(i)` holds:
    /// drawn dim, matching a stylesheet's `:disabled`, skipped by the
    /// cursor, and its clicks do not select it.
    pub fn item_disabled_when(self, disabled: impl Fn(usize) -> bool + 'static) -> Node {
        self.widget_mut(|list: &mut NodeList| {
            *list.shared.disabled.borrow_mut() = Some(Box::new(disabled));
        });
        self
    }

    /// A [`list_of`]'s options, for the list options that reach it too
    /// ([`on_select`](Self::on_select), [`on_context`](Self::on_context),
    /// [`scroll_padding`](Self::scroll_padding),
    /// [`item_key`](Self::item_key)). Whether it is one.
    pub(crate) fn node_list(&self, f: impl FnOnce(&Shared)) -> bool {
        self.widget_mut(|list: &mut NodeList| f(&list.shared))
            .is_some()
    }
}

impl Shared {
    pub(crate) fn set_on_select(&self, handler: ItemHandler) {
        *self.on_select.borrow_mut() = Some(handler);
    }

    pub(crate) fn set_on_context(&self, handler: ItemHandler) {
        *self.on_context.borrow_mut() = Some(handler);
    }

    pub(crate) fn set_padding(&self, items: usize) {
        self.padding.set(items);
    }

    pub(crate) fn set_key(&self, key: Box<dyn Fn(usize) -> u64>) {
        *self.key.borrow_mut() = Some(key);
    }
}
