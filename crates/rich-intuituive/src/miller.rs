//! Miller columns over any tree: the level above, the level the cursor is
//! in and a preview of the hovered item, over one cursor.

use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use rich::markup::escape;

use crate::list::{list_lazy, RowRects};
use crate::node::Node;
use crate::place::RectHandle;
use crate::reactive::{memo, signal, watch, Memo, Signal};
use crate::screen::Rect;
use crate::split::hsplit;

/// A tree [`Miller`] columns browse: levels named by a key, each with
/// items named by a string. A directory tree is one (the `files`
/// plugin's), and so are a menu, a JSON document, a bucket's prefixes or
/// a category tree.
pub trait MillerSource: 'static {
    /// Where a level is: a directory's path, a node's id.
    type Key: Clone + PartialEq + Eq + Hash + 'static;

    /// The names of the items at `at`, in order. Read inside a memo: it
    /// may read signals, and the columns follow them.
    fn items(&self, at: &Self::Key) -> Vec<String>;

    /// The level item `name` at `at` opens, or `None` for a leaf.
    fn child(&self, at: &Self::Key, name: &str) -> Option<Self::Key>;

    /// The level above `at`, and the name `at` has there; `None` at the
    /// top.
    fn parent(&self, at: &Self::Key) -> Option<(Self::Key, String)>;

    /// Called when the columns arrive at a level, before its cursor is
    /// placed: clear a filter, say. Nothing by default.
    fn arrive(&self, at: &Self::Key) {
        let _ = at;
    }
}

/// Miller columns over a [`MillerSource`]: the level above (the current
/// level hovered in it), the current level (the cursor's column) and a
/// preview of the hovered item, side by side in panes whose rails the
/// mouse drags. All of it is signals, and the struct is `Copy`, so any
/// handler takes one. Made by [`miller_of`]; drawn by [`view`](Self::view)
/// or, with columns of your own, [`layout`](Self::layout).
///
/// - Moving: [`arrow`](Self::arrow), [`move_to`](Self::move_to),
///   [`page`](Self::page), [`enter`](Self::enter),
///   [`leave`](Self::leave), [`cd`](Self::cd); or bind the usual keys
///   with [`navigation`](Self::navigation).
/// - Each level remembers the item last hovered in it: going back up, or
///   into it again, hovers that item.
/// - The mouse: a click in the current column hovers an item and the
///   wheel moves the cursor; a click in the parent column goes to that
///   item's level (below the items, up a level)
///   ([`click_parent`](Self::click_parent)), and
///   [`enter_at`](Self::enter_at) enters the hovered item at a name, for
///   a click in its preview.
pub struct Miller<S: MillerSource> {
    source: Signal<Rc<S>>,
    /// The current level.
    pub at: Signal<S::Key>,
    /// Its items' names.
    pub items: Memo<Vec<String>>,
    /// The items of the level above (none at the top).
    pub parent_items: Memo<Vec<String>>,
    /// The hovered item's index in `items`.
    pub cursor: Signal<usize>,
    /// The current level's index in `parent_items`.
    pub parent_cursor: Signal<usize>,
    /// The hovered item's name.
    pub hovered: Memo<Option<String>>,
    /// The first row in view of the current column, and of the parent.
    pub first: Signal<usize>,
    pub parent_first: Signal<usize>,
    /// The item last hovered in each level visited.
    remember: Signal<HashMap<S::Key, String>>,
    rows: Signal<RowRects>,
    column: Signal<Option<RectHandle>>,
    /// The panes' shares of the width, which the rails drag.
    outer: Signal<f64>,
    inner: Signal<f64>,
}

impl<S: MillerSource> Clone for Miller<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: MillerSource> Copy for Miller<S> {}

/// [`Miller`] columns over `source`, starting at `start`, in the shares
/// 1:4:3. Call it while the app is built.
///
/// ```
/// use intuituive::miller::MillerSource;
/// use intuituive::miller_of;
/// use intuituive::prelude::*;
///
/// /// A two-level menu.
/// struct Menu;
/// impl MillerSource for Menu {
///     type Key = String;
///     fn items(&self, at: &String) -> Vec<String> {
///         match at.as_str() {
///             "" => vec!["fruit".into(), "veg".into()],
///             "fruit" => vec!["apple".into(), "pear".into()],
///             _ => vec!["leek".into()],
///         }
///     }
///     fn child(&self, at: &String, name: &str) -> Option<String> {
///         at.is_empty().then(|| name.to_string())
///     }
///     fn parent(&self, at: &String) -> Option<(String, String)> {
///         (!at.is_empty()).then(|| (String::new(), at.clone()))
///     }
/// }
///
/// let app = App::new(|| {
///     let columns = miller_of(Menu, String::new());
///     columns.navigation(columns.view())
/// });
/// // `fruit` is hovered and its items previewed; `l` enters it.
/// let screen = app.render_with(&["l"], 40, 2).unwrap();
/// assert!(screen[0].contains("fruit") && screen[0].contains("apple"), "{screen:?}");
/// ```
pub fn miller_of<S: MillerSource>(source: S, start: S::Key) -> Miller<S> {
    miller_in(source, signal(start))
}

/// [`miller_of`] on a signal of the current level made beforehand, so the
/// source can read it (a model that follows the current level).
pub fn miller_in<S: MillerSource>(source: S, at: Signal<S::Key>) -> Miller<S> {
    let source = Rc::new(source);
    let items = {
        let source = source.clone();
        memo(move || at.with(|key| source.items(key)))
    };
    let parent_items = {
        let source = source.clone();
        memo(move || {
            let up = at.with(|key| source.parent(key));
            up.map(|(up, _)| source.items(&up)).unwrap_or_default()
        })
    };
    let cursor = signal(0usize);
    let hovered = memo(move || {
        let at = cursor.get();
        items.with(|f| f.get(at.min(f.len().saturating_sub(1))).cloned())
    });
    let parent_name = {
        let source = source.clone();
        memo(move || at.with(|key| source.parent(key).map(|(_, name)| name)))
    };
    let miller = Miller {
        source: signal(source),
        at,
        items,
        parent_items,
        cursor,
        parent_cursor: signal(0),
        hovered,
        first: signal(0),
        parent_first: signal(0),
        remember: signal(HashMap::new()),
        rows: signal(RowRects::new()),
        column: signal(None),
        outer: signal(1.0 / 8.0),
        inner: signal(4.0 / 7.0),
    };
    // Remember the item hovered at each level, for coming back.
    watch(
        move || (at.get(), hovered.get()),
        move |(key, name), _| {
            if let Some(name) = name {
                miller.remember.update(|r| {
                    r.insert(key, name);
                });
            }
        },
    );
    // The cursor stays on the items when they shrink.
    watch(
        move || items.with(Vec::len),
        move |len, _| {
            if cursor.get_untracked() >= len {
                cursor.set(len.saturating_sub(1));
            }
        },
    );
    // The parent column hovers the current level.
    watch(
        move || (parent_name.get(), parent_items.with(Vec::len)),
        move |(name, _), _| {
            let at = name
                .and_then(|n| parent_items.with_untracked(|p| p.iter().position(|i| *i == n)))
                .unwrap_or(0);
            miller.parent_cursor.set(at);
        },
    );
    miller
}

impl<S: MillerSource> Miller<S> {
    fn source(&self) -> Rc<S> {
        self.source.with_untracked(Rc::clone)
    }

    /// The columns' shares of the width, `[1, 4, 3]` by default. A share
    /// of 0 hides that column's pane down to its rail.
    pub fn ratios(self, parent: u16, current: u16, preview: u16) -> Self {
        let total = f64::from(parent + current + preview).max(1.0);
        self.outer.set(f64::from(parent) / total);
        let rest = f64::from(current + preview).max(1.0);
        self.inner.set(f64::from(current) / rest);
        self
    }

    /// Where the current column's rows in view are on the screen, once a
    /// column passes them to its list ([`list`](crate::list)'s
    /// `row_rects`).
    pub fn row_rects(&self) -> RowRects {
        self.rows.get_untracked()
    }

    /// Where the current column is on the screen.
    pub fn column_rect(&self) -> Option<Rect> {
        self.column.get_untracked().and_then(|c| c.get_untracked())
    }

    /// Mark `node` as the current column, for [`page`](Self::page) and
    /// [`column_rect`](Self::column_rect). [`current_column`](Self::current_column)
    /// does; a column of your own should too.
    pub fn track_current(&self, node: Node) -> Node {
        let handle = node.rect_handle();
        // `update`, not `set`: compared by identity, a new handle is set.
        self.column.update(|c| *c = Some(handle));
        node
    }

    /// The item last hovered at `at`, if it has been shown.
    pub fn remembered(&self, at: &S::Key) -> Option<String> {
        self.remember.with_untracked(|r| r.get(at).cloned())
    }

    /// The index of item `name` at the current level, without subscribing.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.items
            .with_untracked(|items| items.iter().position(|i| i == name))
    }

    /// The level the hovered item opens, if it opens one.
    pub fn hovered_child(&self) -> Option<S::Key> {
        let name = self.hovered.get_untracked()?;
        self.source().child(&self.at.get_untracked(), &name)
    }

    /// Move the cursor to `to`, kept in the items.
    pub fn move_to(&self, to: isize) {
        let len = self.items.with_untracked(Vec::len);
        let to = to.clamp(0, len.saturating_sub(1) as isize) as usize;
        self.cursor.set(to);
    }

    /// Move the cursor by `by` items.
    pub fn arrow(&self, by: isize) {
        self.move_to(self.cursor.get_untracked() as isize + by);
    }

    /// Move by a share of the column's height (0.5 is half a page).
    pub fn page(&self, share: f64) {
        let rows = self.column_rect().map_or(1, |r| r.height.max(1)) as f64;
        let by = (rows * share).round() as isize;
        self.arrow(if by == 0 { share.signum() as isize } else { by });
    }

    /// Show level `at`, hovering `hover` there, else the item hovered when
    /// it was last shown, else the first. The source's
    /// [`arrive`](MillerSource::arrive) runs first.
    pub fn cd(&self, at: S::Key, hover: Option<String>) {
        self.at.set(at);
        let at = self.at.get_untracked();
        self.source().arrive(&at);
        let hover = hover.or_else(|| self.remembered(&at));
        let to = hover.and_then(|n| self.position(&n)).unwrap_or(0);
        self.cursor.set(to);
    }

    /// Enter the hovered item, if it opens a level.
    pub fn enter(&self) {
        if let Some(child) = self.hovered_child() {
            self.cd(child, None);
        }
    }

    /// Enter the hovered item, hovering `name` in it (a click in its
    /// preview; `None` for the remembered item).
    pub fn enter_at(&self, name: Option<String>) {
        if let Some(child) = self.hovered_child() {
            self.cd(child, name);
        }
    }

    /// Go up a level, hovering the level left.
    pub fn leave(&self) {
        let at = self.at.get_untracked();
        if let Some((up, name)) = self.source().parent(&at) {
            self.cd(up, Some(name));
        }
    }

    /// Run `change` (reading the items again, say), keeping the hovered
    /// item hovered if it is still there.
    pub fn keep(&self, change: impl FnOnce()) {
        let name = self.hovered.get_untracked();
        change();
        if let Some(at) = name.and_then(|n| self.position(&n)) {
            self.cursor.set(at);
        }
    }

    /// A click on row `row` of the parent column (counted from its first
    /// row in view): a level there is entered, a leaf hovered at its
    /// level; below the items, up a level.
    pub fn click_parent(&self, row: usize) {
        let source = self.source();
        let at = self.at.get_untracked();
        let Some((up, _)) = source.parent(&at) else {
            return;
        };
        let i = self.parent_first.get_untracked() + row;
        match self.parent_items.with_untracked(|p| p.get(i).cloned()) {
            Some(name) => match source.child(&up, &name) {
                Some(child) => self.cd(child, None),
                None => self.cd(up, Some(name)),
            },
            None => self.leave(),
        }
    }

    /// Bind the usual movement keys on `node` (which should hold the
    /// view): `j`/`k` (and the arrows) move, `h`/`l` (and ←/→) leave and
    /// enter, `g g`/`G` (and Home/End) go to the top and the bottom,
    /// Ctrl+D/U and PgDn/PgUp move by half and whole columns. The node
    /// sees the keys before the focused list.
    pub fn navigation(self, node: Node) -> Node {
        let m = self;
        node.previews_keys()
            .bind("k up", "Move up", move |_| m.arrow(-1))
            .bind("j down", "Move down", move |_| m.arrow(1))
            .bind("h left", "Go up a level", move |_| m.leave())
            .bind("l right", "Enter", move |_| m.enter())
            .bind_sequence("g g", "Go to the top", move |_| m.move_to(0))
            .bind("home", "Go to the top", move |_| m.move_to(0))
            .bind("G end", "Go to the bottom", move |_| m.move_to(isize::MAX))
            .bind("ctrl+d", "Half a page down", move |_| m.page(0.5))
            .bind("ctrl+u", "Half a page up", move |_| m.page(-0.5))
            .bind("pagedown ctrl+f", "A page down", move |_| m.page(1.0))
            .bind("pageup ctrl+b", "A page up", move |_| m.page(-1.0))
    }

    /// The three panes side by side, with draggable rails between them:
    /// the columns of your own (a richer row, a preview by type).
    pub fn layout(self, parent: Node, current: Node, preview: Node) -> Node {
        hsplit(parent, hsplit(current, preview, self.inner), self.outer)
    }

    /// The three columns with the default ones: names in lists, and the
    /// hovered item's items as its preview.
    pub fn view(self) -> Node {
        self.layout(
            self.parent_column(),
            self.current_column(),
            self.preview_column(),
        )
    }

    /// The current level's names in a list on the cursor, focused, its
    /// wheel and clicks moving the cursor.
    pub fn current_column(self) -> Node {
        let items = self.items;
        let node = list_lazy(
            move || items.with(Vec::len),
            move |i| items.with(|all| all.get(i).map(|n| escape(n)).unwrap_or_default()),
            self.cursor,
        )
        .offset(self.first)
        .row_rects(&self.rows.get_untracked())
        .name("current")
        .autofocus();
        self.track_current(node)
    }

    /// The level above's names, the current level hovered; a click goes
    /// there ([`click_parent`](Self::click_parent)).
    pub fn parent_column(self) -> Node {
        let m = self;
        let items = self.parent_items;
        list_lazy(
            move || items.with(Vec::len),
            move |i| items.with(|all| all.get(i).map(|n| escape(n)).unwrap_or_default()),
            self.parent_cursor,
        )
        .offset(self.parent_first)
        .no_focus()
        .name("parent")
        .on_mouse(move |_, mouse| {
            use rich_interact::{Button, MouseKind};
            match mouse.kind {
                MouseKind::Down(Button::Left) => {}
                MouseKind::ScrollUp | MouseKind::ScrollDown => return true,
                _ => return false,
            }
            m.click_parent(mouse.row as usize);
            true
        })
    }

    /// The hovered item's items, if it opens a level, the one last
    /// hovered there underlined; a click enters it there.
    pub fn preview_column(self) -> Node {
        let m = self;
        let source = self.source();
        let hovered = self.hovered;
        let at = self.at;
        let shown = memo(move || {
            let name = hovered.get()?;
            let child = at.with(|key| source.child(key, &name))?;
            let items = source.items(&child);
            Some((child, items))
        });
        let first = signal(0usize);
        let cursor = signal(0usize);
        watch(
            move || shown.get(),
            move |shown, _| {
                let at = shown
                    .and_then(|(child, items)| {
                        let name = m.remembered(&child)?;
                        items.iter().position(|i| *i == name)
                    })
                    .unwrap_or(0);
                cursor.set(at);
                first.set(0);
            },
        );
        list_lazy(
            move || shown.with(|s| s.as_ref().map_or(0, |(_, items)| items.len())),
            move |i| {
                shown.with(|s| {
                    s.as_ref()
                        .and_then(|(_, items)| items.get(i))
                        .map(|n| escape(n))
                        .unwrap_or_default()
                })
            },
            cursor,
        )
        .offset(first)
        .selected_style_for(|_| Some("underline".into()))
        .no_focus()
        .name("preview")
        .on_mouse(move |_, mouse| {
            use rich_interact::{Button, MouseKind};
            match mouse.kind {
                MouseKind::Down(Button::Left) => {}
                MouseKind::ScrollUp | MouseKind::ScrollDown => return true,
                _ => return false,
            }
            let i = first.get_untracked() + mouse.row as usize;
            let name =
                shown.with_untracked(|s| s.as_ref().and_then(|(_, items)| items.get(i).cloned()));
            m.enter_at(name);
            true
        })
    }
}
