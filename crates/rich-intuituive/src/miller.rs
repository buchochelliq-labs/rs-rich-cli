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
///   [`leave`](Self::leave), [`cd`](Self::cd), or any [`Move`] with
///   [`go`](Self::go); bind the usual keys with
///   [`navigation`](Self::navigation), or your own keys and wording with
///   [`navigation_with`](Self::navigation_with).
/// - Reading: each signal has a method, [`at`](Self::at),
///   [`cursor`](Self::cursor), [`hovered`](Self::hovered),
///   [`items`](Self::items), [`first`](Self::first), the parent's and the
///   preview's, and [`visited`](Self::visited), the levels shown.
/// - The columns' lists take any list option through
///   [`view_with`](Self::view_with)'s [`MillerOptions`].
/// - Each level remembers the item last hovered in it: going back up, or
///   into it again, hovers that item.
/// - The mouse: a click in the current column hovers an item and the
///   wheel moves the cursor; a click in the parent column goes to that
///   item's level (below the items, up a level)
///   ([`click_parent`](Self::click_parent)), and
///   [`enter_at`](Self::enter_at) enters the hovered item at a name, for
///   a click in its preview.
/// - Other items at a level: [`overlay`](Self::overlay) lists search
///   results, bookmarks or a "recent" list at a level, each opening a
///   level of its own, until the columns leave it.
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
    /// The levels shown, the most recent last.
    visited: Signal<Vec<S::Key>>,
    /// The preview column's hovered item and first row in view.
    preview_cursor: Signal<usize>,
    preview_first: Signal<usize>,
    rows: Signal<RowRects>,
    column: Signal<Option<RectHandle>>,
    /// The panes' shares of the width, which the rails drag.
    outer: Signal<f64>,
    inner: Signal<f64>,
    /// Items listed at a level in place of its own ([`overlay`](Miller::overlay)).
    overlay: Signal<Option<Overlay<S::Key>>>,
}

/// What a [`Miller::overlay`] lists at a level: each item's name, and the
/// level it opens (`None`, a leaf).
#[derive(Clone, PartialEq)]
struct Overlay<K> {
    at: K,
    names: Vec<String>,
    opens: Vec<Option<K>>,
}

impl<K: PartialEq + Clone> Overlay<K> {
    /// The level item `name` opens, if the overlay is at `at` and lists it.
    fn child(&self, at: &K, name: &str) -> Option<Option<K>> {
        if self.at != *at {
            return None;
        }
        let i = self.names.iter().position(|n| n == name)?;
        Some(self.opens[i].clone())
    }
}

/// The level `name` at `at` opens: the overlay's, else the source's.
/// Subscribes to the overlay.
fn child_of<S: MillerSource>(
    source: &S,
    overlay: Signal<Option<Overlay<S::Key>>>,
    at: &S::Key,
    name: &str,
) -> Option<S::Key> {
    let listed = overlay.with(|o| o.as_ref().and_then(|o| o.child(at, name)));
    match listed {
        Some(opens) => opens,
        None => source.child(at, name),
    }
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
    let overlay: Signal<Option<Overlay<S::Key>>> = signal(None);
    let items = {
        let source = source.clone();
        memo(move || {
            let listed = overlay.with(|o| {
                o.as_ref()
                    .filter(|o| at.with(|key| *key == o.at))
                    .map(|o| o.names.clone())
            });
            listed.unwrap_or_else(|| at.with(|key| source.items(key)))
        })
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
        visited: signal(Vec::new()),
        preview_cursor: signal(0),
        preview_first: signal(0),
        rows: signal(RowRects::new()),
        column: signal(None),
        outer: signal(1.0 / 8.0),
        inner: signal(4.0 / 7.0),
        overlay,
    };
    // Remember the item hovered at each level, for coming back (not an
    // overlay's: the level's own is remembered).
    watch(
        move || (at.get(), hovered.get(), overlay.with(Option::is_some)),
        move |(key, name, overlaid), _| {
            if let (Some(name), false) = (name, overlaid) {
                miller.remember.update(|r| {
                    r.insert(key, name);
                });
            }
        },
    );
    // The levels shown, the most recent last.
    watch(
        move || at.get(),
        move |key, _| {
            miller.visited.update(|visited| {
                visited.retain(|k| *k != key);
                visited.push(key);
            })
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

    /// The current level, a signal (the `at` field).
    pub fn at(&self) -> Signal<S::Key> {
        self.at
    }

    /// The current level's items' names (the `items` field).
    pub fn items(&self) -> Memo<Vec<String>> {
        self.items
    }

    /// The level above's items' names (the `parent_items` field).
    pub fn parent_items(&self) -> Memo<Vec<String>> {
        self.parent_items
    }

    /// The hovered item's index in [`items`](Self::items), a signal (the
    /// `cursor` field).
    pub fn cursor(&self) -> Signal<usize> {
        self.cursor
    }

    /// The current level's index in the level above (the `parent_cursor`
    /// field).
    pub fn parent_cursor(&self) -> Signal<usize> {
        self.parent_cursor
    }

    /// The hovered item's name (the `hovered` field).
    pub fn hovered(&self) -> Memo<Option<String>> {
        self.hovered
    }

    /// The current column's first row in view, a signal (the `first`
    /// field).
    pub fn first(&self) -> Signal<usize> {
        self.first
    }

    /// The parent column's first row in view (the `parent_first` field).
    pub fn parent_first(&self) -> Signal<usize> {
        self.parent_first
    }

    /// The hovered item in the default preview column (the level the
    /// hovered item opens), and its first row in view.
    pub fn preview_cursor(&self) -> (Signal<usize>, Signal<usize>) {
        (self.preview_cursor, self.preview_first)
    }

    /// The item last hovered at each level shown, a signal: what
    /// [`remembered`](Self::remembered) looks up.
    pub fn remembered_all(&self) -> Signal<HashMap<S::Key, String>> {
        self.remember
    }

    /// The levels shown so far, the most recent last (the current level
    /// among them), a signal: a jump list, a "recent" menu, a frecency
    /// store's visits.
    ///
    /// ```
    /// # use intuituive::miller::MillerSource;
    /// # use intuituive::miller_of;
    /// # use intuituive::prelude::*;
    /// # struct Menu;
    /// # impl MillerSource for Menu {
    /// #     type Key = String;
    /// #     fn items(&self, at: &String) -> Vec<String> {
    /// #         if at.is_empty() { vec!["fruit".into(), "veg".into()] } else { vec!["x".into()] }
    /// #     }
    /// #     fn child(&self, at: &String, name: &str) -> Option<String> {
    /// #         at.is_empty().then(|| name.to_string())
    /// #     }
    /// #     fn parent(&self, at: &String) -> Option<(String, String)> {
    /// #         (!at.is_empty()).then(|| (String::new(), at.clone()))
    /// #     }
    /// # }
    /// let app = App::new(|| {
    ///     let columns = miller_of(Menu, String::new());
    ///     column([
    ///         columns.view().flex(1),
    ///         text(move || format!("{:?}", columns.visited().get())).fixed(1),
    ///     ])
    ///     .on_key("l", move |_| columns.enter())
    ///     .on_key("h", move |_| columns.leave())
    /// });
    /// let mut driver = app.driver(40, 3);
    /// driver.press("l");
    /// driver.press("h");
    /// assert_eq!(driver.screen().plain()[2].trim_end(), r#"["fruit", ""]"#);
    /// ```
    pub fn visited(&self) -> Signal<Vec<S::Key>> {
        self.visited
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
        let at = self.at.get_untracked();
        crate::reactive::untracked(|| child_of(&*self.source(), self.overlay, &at, &name))
    }

    /// List `items` at level `at` in place of its own, and go there: each
    /// item a name and the level it opens (`None` for a leaf), as a search's
    /// results, bookmarks or a "recent" list. The cursor starts on the
    /// first. It lasts until the columns leave the level: entering an item
    /// goes to its level, and [`leave`](Self::leave) (or
    /// [`end_overlay`](Self::end_overlay)) goes back to the level's own
    /// items, hovering the one hovered before, as Yazi leaves a search.
    ///
    /// ```
    /// # use intuituive::miller::MillerSource;
    /// # use intuituive::miller_of;
    /// # use intuituive::prelude::*;
    /// # struct Menu;
    /// # impl MillerSource for Menu {
    /// #     type Key = String;
    /// #     fn items(&self, at: &String) -> Vec<String> {
    /// #         match at.as_str() {
    /// #             "" => vec!["fruit".into(), "veg".into()],
    /// #             "fruit" => vec!["apple".into(), "pear".into()],
    /// #             _ => vec!["leek".into()],
    /// #         }
    /// #     }
    /// #     fn child(&self, at: &String, name: &str) -> Option<String> {
    /// #         at.is_empty().then(|| name.to_string())
    /// #     }
    /// #     fn parent(&self, at: &String) -> Option<(String, String)> {
    /// #         (!at.is_empty()).then(|| (String::new(), at.clone()))
    /// #     }
    /// # }
    /// let app = App::new(|| {
    ///     let m = miller_of(Menu, String::new());
    ///     m.navigation(m.view()).on_key("b", move |_| {
    ///         // Bookmarks at the top: one opens a level, one is a leaf.
    ///         m.overlay(String::new(), vec![("★ veg".into(), Some("veg".into())), ("★ note".into(), None)]);
    ///     })
    /// });
    /// let mut driver = app.driver(40, 2);
    /// driver.press("b");
    /// assert!(driver.screen().plain()[0].contains("★ veg"));
    /// driver.press("l");
    /// assert!(driver.screen().plain()[0].contains("leek"));
    /// driver.press("h b h");
    /// // Leaving the overlay shows the level's own items again.
    /// assert!(driver.screen().plain()[0].contains("fruit"));
    /// ```
    pub fn overlay(&self, at: S::Key, items: Vec<(String, Option<S::Key>)>) {
        if self.at.get_untracked() != at {
            self.cd(at.clone(), None);
        }
        let (names, opens) = items.into_iter().unzip();
        self.overlay.set(Some(Overlay { at, names, opens }));
        self.cursor.set(0);
        self.first.set(0);
    }

    /// The level an [`overlay`](Self::overlay) is listed at, while one
    /// is, subscribing: a title saying "search results", say.
    pub fn overlay_at(&self) -> Option<S::Key> {
        self.overlay.with(|o| o.as_ref().map(|o| o.at.clone()))
    }

    /// End the [`overlay`](Self::overlay): the level's own items again,
    /// hovering the one hovered before it. Nothing without one.
    pub fn end_overlay(&self) {
        if self.overlay.with_untracked(Option::is_none) {
            return;
        }
        self.overlay.set(None);
        let at = self.at.get_untracked();
        let to = self
            .remembered(&at)
            .and_then(|n| self.position(&n))
            .unwrap_or(0);
        self.cursor.set(to);
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
        // Going anywhere ends an overlay.
        if self.overlay.with_untracked(Option::is_some) {
            self.overlay.set(None);
        }
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

    /// Go up a level, hovering the level left; in an
    /// [`overlay`](Self::overlay), back to the level's own items.
    pub fn leave(&self) {
        if self.overlay.with_untracked(Option::is_some) {
            self.end_overlay();
            return;
        }
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

    /// Make move `to`: what [`navigation`](Self::navigation) binds each
    /// key to, for a binding of your own (a key sequence:
    /// `.bind_sequence("g g", "Top", move |_| m.go(Move::Top))`).
    pub fn go(&self, to: Move) {
        match to {
            Move::Up => self.arrow(-1),
            Move::Down => self.arrow(1),
            Move::Leave => self.leave(),
            Move::Enter => self.enter(),
            Move::Top => self.move_to(0),
            Move::Bottom => self.move_to(isize::MAX),
            Move::HalfPageUp => self.page(-0.5),
            Move::HalfPageDown => self.page(0.5),
            Move::PageUp => self.page(-1.0),
            Move::PageDown => self.page(1.0),
        }
    }

    /// Bind the moves of a table on `node` (which should hold the view):
    /// for each row, a move, its keys (space-separated, either of them,
    /// as [`Node::bind`] takes them) and the description the help and the
    /// which-key pop-up show. A clone keeps its own keys and wording where
    /// [`navigation`](Self::navigation)'s are fixed; a move left out is
    /// not bound, and one may have several rows. A key sequence is bound
    /// with [`go`](Self::go). The node sees the keys before the focused
    /// list.
    ///
    /// ```
    /// # use intuituive::miller::MillerSource;
    /// # use intuituive::miller_of;
    /// # use intuituive::prelude::*;
    /// # struct Menu;
    /// # impl MillerSource for Menu {
    /// #     type Key = String;
    /// #     fn items(&self, at: &String) -> Vec<String> {
    /// #         if at.is_empty() { vec!["fruit".into(), "veg".into()] } else { vec!["x".into()] }
    /// #     }
    /// #     fn child(&self, at: &String, name: &str) -> Option<String> {
    /// #         at.is_empty().then(|| name.to_string())
    /// #     }
    /// #     fn parent(&self, at: &String) -> Option<(String, String)> {
    /// #         (!at.is_empty()).then(|| (String::new(), at.clone()))
    /// #     }
    /// # }
    /// use intuituive::miller::Move;
    ///
    /// let app = App::new(|| {
    ///     let m = miller_of(Menu, String::new());
    ///     m.navigation_with(
    ///         m.view(),
    ///         &[
    ///             (Move::Up, "k up", "Previous file"),
    ///             (Move::Down, "j down", "Next file"),
    ///             (Move::PageDown, "shift+pagedown", "Next page"),
    ///         ],
    ///     )
    ///     .bind_sequence("g g", "Top", move |_| m.go(Move::Top))
    ///     .on_key("?", |cx| cx.help())
    /// });
    /// let mut driver = app.driver(60, 12);
    /// driver.press("?");
    /// assert!(driver.screen().plain().join("\n").contains("Previous file"));
    /// ```
    pub fn navigation_with(self, node: Node, moves: &[(Move, &str, &str)]) -> Node {
        let m = self;
        moves
            .iter()
            .fold(node.previews_keys(), |node, &(to, keys, description)| {
                node.bind(keys, description, move |_| m.go(to))
            })
    }

    /// Bind the usual movement keys on `node` (which should hold the
    /// view): `j`/`k` (and the arrows) move, `h`/`l` (and ←/→) leave and
    /// enter, `g g`/`G` (and Home/End) go to the top and the bottom,
    /// Ctrl+D/U and PgDn/PgUp move by half and whole columns. The node
    /// sees the keys before the focused list.
    pub fn navigation(self, node: Node) -> Node {
        let m = self;
        self.navigation_with(
            node,
            &[
                (Move::Up, "k up", "Move up"),
                (Move::Down, "j down", "Move down"),
                (Move::Leave, "h left", "Go up a level"),
                (Move::Enter, "l right", "Enter"),
                (Move::Top, "home", "Go to the top"),
                (Move::Bottom, "G end", "Go to the bottom"),
                (Move::HalfPageDown, "ctrl+d", "Half a page down"),
                (Move::HalfPageUp, "ctrl+u", "Half a page up"),
                (Move::PageDown, "pagedown ctrl+f", "A page down"),
                (Move::PageUp, "pageup ctrl+b", "A page up"),
            ],
        )
        .bind_sequence("g g", "Go to the top", move |_| m.go(Move::Top))
    }

    /// The three panes side by side, with draggable rails between them:
    /// the columns of your own (a richer row, a preview by type).
    pub fn layout(self, parent: Node, current: Node, preview: Node) -> Node {
        hsplit(parent, hsplit(current, preview, self.inner), self.outer)
    }

    /// The three columns with the default ones: names in lists, and the
    /// hovered item's items as its preview.
    pub fn view(self) -> Node {
        self.view_with(MillerOptions::new())
    }

    /// The default columns, each list given what `options` adds to it:
    /// any list option ([`highlight_layer`](Node::highlight_layer),
    /// [`on_context`](Node::on_context), [`marker`](Node::marker), …), a
    /// handler, a name.
    ///
    /// ```
    /// # use intuituive::miller::MillerSource;
    /// # use intuituive::miller_of;
    /// # use intuituive::prelude::*;
    /// # struct Menu;
    /// # impl MillerSource for Menu {
    /// #     type Key = String;
    /// #     fn items(&self, at: &String) -> Vec<String> {
    /// #         if at.is_empty() { vec!["fruit".into(), "veg".into()] } else { vec!["x".into()] }
    /// #     }
    /// #     fn child(&self, at: &String, name: &str) -> Option<String> {
    /// #         at.is_empty().then(|| name.to_string())
    /// #     }
    /// #     fn parent(&self, at: &String) -> Option<(String, String)> {
    /// #         (!at.is_empty()).then(|| (String::new(), at.clone()))
    /// #     }
    /// # }
    /// use intuituive::miller::MillerOptions;
    ///
    /// let app = App::new(|| {
    ///     let m = miller_of(Menu, String::new());
    ///     m.view_with(MillerOptions::new().current(|list| list.symbol("> ")))
    /// });
    /// let mut driver = app.driver(40, 2);
    /// driver.settle();
    /// assert!(driver.screen().plain()[0].contains("> fruit"));
    /// ```
    pub fn view_with(self, options: MillerOptions) -> Node {
        self.layout(
            options.wrap_parent(self.parent_column()),
            options.wrap_current(self.current_column()),
            options.wrap_preview(self.preview_column()),
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
        let (at, overlay) = (self.at, self.overlay);
        let shown = memo(move || {
            let name = hovered.get()?;
            let child = at.with(|key| child_of(&*source, overlay, key, &name))?;
            let items = source.items(&child);
            Some((child, items))
        });
        let (first, cursor) = (self.preview_first, self.preview_cursor);
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

/// A move of the [`Miller`] columns, for [`Miller::go`] and the table of
/// [`Miller::navigation_with`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Move {
    /// The item above.
    Up,
    /// The item below.
    Down,
    /// Up a level, hovering the level left.
    Leave,
    /// Into the hovered item, if it opens a level.
    Enter,
    /// The first item.
    Top,
    /// The last item.
    Bottom,
    /// Half a column up.
    HalfPageUp,
    /// Half a column down.
    HalfPageDown,
    /// A column up.
    PageUp,
    /// A column down.
    PageDown,
}

/// What [`MillerOptions`] does to a column's node.
type Hook = Rc<dyn Fn(Node) -> Node>;

/// What [`Miller::view_with`] adds to each column's list: a hook given
/// the list as made, returning it with options of yours. A column view of
/// your own (a plugin's, over [`Miller::layout`]) gives its lists to the
/// hooks with [`wrap_current`](Self::wrap_current) and the like.
#[derive(Clone, Default)]
pub struct MillerOptions {
    current: Option<Hook>,
    parent: Option<Hook>,
    preview: Option<Hook>,
}

impl MillerOptions {
    /// No hooks: the columns as they come.
    pub fn new() -> MillerOptions {
        MillerOptions::default()
    }

    /// Give the current column's list to `hook`:
    /// `.current(|list| list.highlight_layer(HighlightLayer::Under))`.
    pub fn current(mut self, hook: impl Fn(Node) -> Node + 'static) -> MillerOptions {
        self.current = Some(Rc::new(hook));
        self
    }

    /// Give the parent column's list to `hook`.
    pub fn parent(mut self, hook: impl Fn(Node) -> Node + 'static) -> MillerOptions {
        self.parent = Some(Rc::new(hook));
        self
    }

    /// Give the preview column's node (the default view's: a list) to
    /// `hook`.
    pub fn preview(mut self, hook: impl Fn(Node) -> Node + 'static) -> MillerOptions {
        self.preview = Some(Rc::new(hook));
        self
    }

    /// `column` through the [`current`](Self::current) hook, if there is
    /// one.
    pub fn wrap_current(&self, column: Node) -> Node {
        wrap(&self.current, column)
    }

    /// `column` through the [`parent`](Self::parent) hook, if there is one.
    pub fn wrap_parent(&self, column: Node) -> Node {
        wrap(&self.parent, column)
    }

    /// `column` through the [`preview`](Self::preview) hook, if there is
    /// one.
    pub fn wrap_preview(&self, column: Node) -> Node {
        wrap(&self.preview, column)
    }
}

fn wrap(hook: &Option<Hook>, column: Node) -> Node {
    match hook {
        Some(hook) => hook(column),
        None => column,
    }
}
