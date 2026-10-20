//! A tree of items that expand and collapse, with one selected.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use rich::Segment;
use rich_interact::{Button, Key, KeyCode, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::{signal, Signal};
use crate::screen::Rect;
use crate::scrollbar::Bar;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};
use crate::widgets::{cell_line, over};

/// An item of a [`tree`]: a line of markup and the items under it, with
/// a key for a [`tree_keyed`] and markup drawn before its indent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct TreeItem {
    pub label: String,
    pub children: Vec<TreeItem>,
    /// What names it in a [`tree_keyed`] (a path, an id); empty for none.
    pub key: String,
    /// Console markup drawn before the indent ([`prefix`](Self::prefix)).
    pub prefix: String,
}

impl TreeItem {
    /// An item with no children.
    pub fn new(label: impl Into<String>) -> TreeItem {
        TreeItem {
            label: label.into(),
            ..TreeItem::default()
        }
    }

    /// An item named `key`, for a [`tree_keyed`].
    pub fn keyed(key: impl Into<String>, label: impl Into<String>) -> TreeItem {
        TreeItem {
            key: key.into(),
            ..TreeItem::new(label)
        }
    }

    /// The item named `key`.
    pub fn key(mut self, key: impl Into<String>) -> TreeItem {
        self.key = key.into();
        self
    }

    /// Console markup drawn at the start of its row, before the indent
    /// and the arrow: gitui's status letter first (`M   lib.rs` under a
    /// folder). Give every row a prefix of the same width, so the indents
    /// line up.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::widgets::{tree_with, TreeItem};
    ///
    /// let app = App::new(|| {
    ///     let items = || {
    ///         vec![TreeItem::new("src")
    ///             .prefix("  ")
    ///             .child(TreeItem::new("lib.rs").prefix("[yellow]M[/] "))]
    ///     };
    ///     tree_with(items, signal(vec![0]), signal([vec![0]].into()))
    /// });
    /// let mut driver = app.driver(16, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "  ▾ src");
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "M     lib.rs");
    /// ```
    pub fn prefix(mut self, markup: impl Into<String>) -> TreeItem {
        self.prefix = markup.into();
        self
    }

    /// The item with `child` added under it.
    pub fn child(mut self, child: TreeItem) -> TreeItem {
        self.children.push(child);
        self
    }

    /// The item with `children` added under it.
    pub fn children(mut self, children: impl IntoIterator<Item = TreeItem>) -> TreeItem {
        self.children.extend(children);
        self
    }
}

/// Which keys a [`tree`] takes itself; the others go on to its ancestors.
/// All of them by default. Give one to [`Node::tree_keys`].
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{tree, TreeItem, TreeKeys};
///
/// let app = App::new(|| {
///     let staged = signal(0);
///     let items = || vec![TreeItem::new("src").child(TreeItem::new("lib.rs"))];
///     column([
///         tree(items, signal(vec![0])).tree_keys(TreeKeys::default().no_toggle()).fixed(2),
///         text!("{staged}"),
///     ])
///     .on_key("enter", move |_| staged.update(|n| *n += 1))
/// });
/// let mut driver = app.driver(12, 3);
/// // Enter is the app's (gitui stages a folder), not the tree's toggle.
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "▸ src");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "1");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct TreeKeys {
    /// Enter and Space toggle an item with children.
    pub toggle: bool,
    /// h, j, k, l, g and G, as the arrows, Home and End.
    pub letters: bool,
    /// ← and → (and h and l) collapse, expand and move to the parent or
    /// the first child.
    pub open_close: bool,
}

impl Default for TreeKeys {
    fn default() -> TreeKeys {
        TreeKeys {
            toggle: true,
            letters: true,
            open_close: true,
        }
    }
}

impl TreeKeys {
    /// Enter and Space go on to the tree's ancestors.
    pub fn no_toggle(mut self) -> TreeKeys {
        self.toggle = false;
        self
    }

    /// The letters (h j k l g G) go on to the tree's ancestors.
    pub fn no_letters(mut self) -> TreeKeys {
        self.letters = false;
        self
    }

    /// ← and → (and h and l) go on to the tree's ancestors.
    pub fn no_open_close(mut self) -> TreeKeys {
        self.open_close = false;
        self
    }
}

/// A row of the tree as it is shown.
struct Row {
    path: Vec<usize>,
    key: String,
    label: String,
    prefix: String,
    parent: bool,
}

/// How a tree keeps its selection and its open items: by index path, or
/// by the items' keys.
#[derive(Clone, Copy)]
enum Picks {
    Paths {
        selected: Signal<Vec<usize>>,
        expanded: Signal<HashSet<Vec<usize>>>,
    },
    Keys {
        selected: Signal<Option<String>>,
        expanded: Signal<HashSet<String>>,
    },
}

pub(crate) struct Tree {
    items: Rc<dyn Fn() -> Vec<TreeItem>>,
    picks: Picks,
    /// The selected row's index at the last draw or move: where a keyed
    /// tree's cursor stays when its item is gone.
    last: Rc<Cell<usize>>,
    /// The first row in view, and the rows last drawn.
    first: Cell<usize>,
    height: Cell<usize>,
    pub(crate) bar: Bar,
    /// Where the bar was drawn, and the rows; the column the rows start at.
    bar_at: Cell<Option<(Rect, usize)>>,
    left: Cell<u16>,
    /// Markup drawn before the selected row ([`Node::symbol`]).
    pub(crate) symbol: Option<String>,
    pub(crate) keys: TreeKeys,
}

/// A tree of markup items, one selected. `selected` is the path to the
/// selected item: its index at each level, from the top. Items with
/// children show `▸` while collapsed and `▾` while expanded; each level is
/// indented two cells.
///
/// ↑/↓ (or k/j), PgUp/PgDn and Home/End (g/G) move the selection. → (or l)
/// expands an item, or moves into its first child. ← (or h) collapses it,
/// or moves to its parent. Enter and Space toggle an item with children
/// ([`Node::tree_keys`] leaves any of these to the tree's ancestors). A
/// click selects a row, a click on its arrow toggles it, and the wheel
/// moves the selection. The selection is highlighted in the theme's
/// `selected` style while the tree has the focus, and `tree.selected`
/// otherwise; [`Node::symbol`] draws a highlight symbol before it.
/// `items` may read signals. For a selection that follows an item rather
/// than its place, see [`tree_keyed`].
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{tree, TreeItem};
///
/// let app = App::new(|| {
///     let selected = signal(vec![0]);
///     let items = || {
///         vec![
///             TreeItem::new("src").child(TreeItem::new("main.rs")),
///             TreeItem::new("Cargo.toml"),
///         ]
///     };
///     tree(items, selected).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["right", "q"], 16, 3).unwrap();
/// assert_eq!(screen[0].trim_end(), "▾ src");
/// assert_eq!(screen[1].trim_end(), "    main.rs");
/// assert_eq!(screen[2].trim_end(), "  Cargo.toml");
/// ```
pub fn tree(items: impl Fn() -> Vec<TreeItem> + 'static, selected: Signal<Vec<usize>>) -> Node {
    tree_with(items, selected, signal(HashSet::new()))
}

/// A [`tree`] whose expanded items (their paths) are kept in `expanded`,
/// to read, set, or share with another view.
///
/// ```
/// use std::collections::HashSet;
///
/// use intuituive::prelude::*;
/// use intuituive::widgets::{tree_with, TreeItem};
///
/// let app = App::new(|| {
///     let selected = signal(vec![0]);
///     let expanded = signal(HashSet::from([vec![0]]));
///     let items = || vec![TreeItem::new("src").child(TreeItem::new("lib.rs"))];
///     tree_with(items, selected, expanded).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 16, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "    lib.rs");
/// ```
pub fn tree_with(
    items: impl Fn() -> Vec<TreeItem> + 'static,
    selected: Signal<Vec<usize>>,
    expanded: Signal<HashSet<Vec<usize>>>,
) -> Node {
    tree_of(Rc::new(items), Picks::Paths { selected, expanded })
}

/// A [`tree`] whose selection and open items are kept by the items'
/// [keys](TreeItem::key), not their places: `selected` holds the selected
/// item's key and `expanded` the open items' keys. When the items change
/// (a file staged moves every row after it, a re-sort, a reload) the
/// selection stays on its item and the open items stay open; an item that
/// is gone leaves the cursor at its row, on the item now there (and
/// `selected` says so). tui-tree-widget's identifiers. [`tree_reveal`]
/// opens an item's ancestors and selects it.
///
/// ```
/// use std::collections::HashSet;
///
/// use intuituive::prelude::*;
/// use intuituive::widgets::{tree_keyed, TreeItem};
///
/// let app = App::new(|| {
///     let files = signal(vec!["a.rs", "b.rs", "c.rs"]);
///     let selected = signal(Some("src/c.rs".to_string()));
///     let expanded = signal(HashSet::from(["src".to_string()]));
///     let items = move || {
///         let kids = files.get().into_iter().map(|f| TreeItem::keyed(format!("src/{f}"), f));
///         vec![TreeItem::keyed("src", "src").children(kids)]
///     };
///     tree_keyed(items, selected, expanded).on_key("s", move |_| files.update(|f| {
///         f.remove(0);
///     }))
/// });
/// let mut driver = app.driver(12, 4);
/// driver.press("s");
/// // `a.rs` is gone: `c.rs` is still selected, a row up.
/// assert_eq!(driver.screen().plain()[2].trim_end(), "    c.rs");
/// let styled = intuituive::testing::styled(&driver);
/// assert!(styled.lines().nth(2).unwrap().starts_with("[reverse]"), "{styled}");
/// ```
pub fn tree_keyed(
    items: impl Fn() -> Vec<TreeItem> + 'static,
    selected: Signal<Option<String>>,
    expanded: Signal<HashSet<String>>,
) -> Node {
    let items: Rc<dyn Fn() -> Vec<TreeItem>> = Rc::new(items);
    let node = tree_of(items.clone(), Picks::Keys { selected, expanded });
    let last = node
        .widget_mut(|tree: &mut Tree| tree.last.clone())
        .expect("a tree");
    // An item that is gone hands the selection to the row now at its
    // place.
    crate::reactive::watch(
        move || {
            let items = items();
            let rows = expanded.with(|open| flatten(&items, &|_, item| open.contains(&item.key)));
            let key = selected.get();
            let found = key
                .as_ref()
                .is_some_and(|key| path_of(&items, key).is_some());
            (!found && !rows.is_empty()).then(|| {
                let at = last.get().min(rows.len() - 1);
                rows[at].key.clone()
            })
        },
        move |to: Option<String>, _| {
            if let Some(to) = to.filter(|to| !to.is_empty()) {
                selected.set(Some(to));
            }
        },
    );
    node
}

/// Open the ancestors of the item `key` names among `items` and select
/// it, for a [`tree_keyed`] on `selected` and `expanded`: locating a
/// search result. `false` when no item has the key.
///
/// ```
/// use std::collections::HashSet;
///
/// use intuituive::prelude::*;
/// use intuituive::widgets::{tree_keyed, tree_reveal, TreeItem};
///
/// fn items() -> Vec<TreeItem> {
///     vec![TreeItem::keyed("a", "a").child(TreeItem::keyed("a/b", "b").child(TreeItem::keyed("a/b/c", "c")))]
/// }
/// let app = App::new(|| {
///     let (selected, expanded) = (signal(None), signal(HashSet::new()));
///     tree_keyed(items, selected, expanded)
///         .on_key("f", move |_| { tree_reveal(&items(), "a/b/c", selected, expanded); })
/// });
/// let mut driver = app.driver(12, 3);
/// driver.press("f");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "      c");
/// let styled = intuituive::testing::styled(&driver);
/// assert!(styled.lines().nth(2).unwrap().starts_with("[reverse]"), "{styled}");
/// ```
pub fn tree_reveal(
    items: &[TreeItem],
    key: &str,
    selected: Signal<Option<String>>,
    expanded: Signal<HashSet<String>>,
) -> bool {
    let Some(path) = path_of(items, key) else {
        return false;
    };
    let mut level = items;
    let mut ancestors = Vec::new();
    for &i in &path[..path.len() - 1] {
        ancestors.push(level[i].key.clone());
        level = &level[i].children;
    }
    if ancestors
        .iter()
        .any(|key| !expanded.with_untracked(|open| open.contains(key)))
    {
        expanded.update(|open| open.extend(ancestors));
    }
    selected.set(Some(key.to_string()));
    true
}

fn tree_of(items: Rc<dyn Fn() -> Vec<TreeItem>>, picks: Picks) -> Node {
    widget(Tree {
        items,
        picks,
        last: Rc::new(Cell::new(0)),
        first: Cell::new(0),
        height: Cell::new(1),
        bar: Bar::default(),
        bar_at: Cell::new(None),
        left: Cell::new(0),
        symbol: None,
        keys: TreeKeys::default(),
    })
}

/// The path to the item `key` names among `items`, if one does.
fn path_of(items: &[TreeItem], key: &str) -> Option<Vec<usize>> {
    for (i, item) in items.iter().enumerate() {
        if item.key == key {
            return Some(vec![i]);
        }
        if let Some(mut path) = path_of(&item.children, key) {
            path.insert(0, i);
            return Some(path);
        }
    }
    None
}

/// The rows of `items` in view, given which are open.
fn flatten(items: &[TreeItem], open: &dyn Fn(&[usize], &TreeItem) -> bool) -> Vec<Row> {
    fn walk(
        items: &[TreeItem],
        open: &dyn Fn(&[usize], &TreeItem) -> bool,
        path: &mut Vec<usize>,
        out: &mut Vec<Row>,
    ) {
        for (i, item) in items.iter().enumerate() {
            path.push(i);
            out.push(Row {
                path: path.clone(),
                key: item.key.clone(),
                label: item.label.clone(),
                prefix: item.prefix.clone(),
                parent: !item.children.is_empty(),
            });
            if !item.children.is_empty() && open(path, item) {
                walk(&item.children, open, path, out);
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(items, open, &mut Vec::new(), &mut out);
    out
}

/// The row of `selected`: its own, or that of the nearest ancestor shown
/// (the first row if none is).
fn index_of(rows: &[Row], selected: &[usize]) -> usize {
    (0..=selected.len())
        .rev()
        .find_map(|n| rows.iter().position(|row| row.path == selected[..n]))
        .unwrap_or(0)
}

impl Tree {
    fn rows(&self) -> Vec<Row> {
        let items = (self.items)();
        match self.picks {
            Picks::Paths { expanded, .. } => {
                expanded.with(|open| flatten(&items, &|path, _| open.contains(path)))
            }
            Picks::Keys { expanded, .. } => {
                expanded.with(|open| flatten(&items, &|_, item| open.contains(&item.key)))
            }
        }
    }

    /// The path of the selected item, if it is among the items.
    fn selected_path(&self, tracked: bool) -> Option<Vec<usize>> {
        match self.picks {
            Picks::Paths { selected, .. } if tracked => Some(selected.get()),
            Picks::Paths { selected, .. } => Some(selected.get_untracked()),
            Picks::Keys { selected, .. } => {
                let key = if tracked {
                    selected.get()
                } else {
                    selected.get_untracked()
                }?;
                path_of(&(self.items)(), &key)
            }
        }
    }

    /// The selected row: the selected item's, its nearest ancestor shown,
    /// or (for an item that is gone) the row at its last place.
    fn selected_row(&self, rows: &[Row], tracked: bool) -> usize {
        match self.selected_path(tracked) {
            Some(path) => index_of(rows, &path),
            None if matches!(self.picks, Picks::Keys { .. }) => {
                self.last.get().min(rows.len().saturating_sub(1))
            }
            None => 0,
        }
    }

    fn select(&self, rows: &[Row], at: usize) {
        let Some(row) = rows.get(at) else { return };
        self.last.set(at);
        match self.picks {
            Picks::Paths { selected, .. } => selected.set(row.path.clone()),
            Picks::Keys { selected, .. } => {
                let key = Some(row.key.clone()).filter(|key| !key.is_empty());
                if selected.with_untracked(|s| *s != key) {
                    selected.set(key);
                }
            }
        }
    }

    /// Expand or collapse `row`. A selection inside an item it collapses
    /// moves up to the item.
    fn set_expanded(&self, rows: &[Row], at: usize, open: bool) {
        let row = &rows[at];
        let inside = !open
            && self
                .selected_path(false)
                .is_some_and(|s| s.len() > row.path.len() && s.starts_with(&row.path));
        match self.picks {
            Picks::Paths { expanded, .. } => expanded.update(|set| {
                if open {
                    set.insert(row.path.clone());
                } else {
                    set.remove(&row.path);
                }
            }),
            Picks::Keys { expanded, .. } => expanded.update(|set| {
                if open {
                    set.insert(row.key.clone());
                } else {
                    set.remove(&row.key);
                }
            }),
        }
        if inside {
            self.select(rows, at);
        }
    }

    fn is_expanded(&self, row: &Row) -> bool {
        match self.picks {
            Picks::Paths { expanded, .. } => expanded.with_untracked(|set| set.contains(&row.path)),
            Picks::Keys { expanded, .. } => expanded.with_untracked(|set| set.contains(&row.key)),
        }
    }

    /// The cells before a row's indent: the text mode's marker, the
    /// symbol's column and its prefix.
    fn lead(&self, row: &Row) -> u16 {
        let marker = if crate::a11y::text_mode() { 2 } else { 0 };
        let symbol = self
            .symbol
            .as_deref()
            .map_or(0, crate::widget::markup_width);
        marker + symbol + crate::widget::markup_width(&row.prefix)
    }

    fn key(&self, key: &Key) -> Used {
        if key.modifiers.ctrl || key.modifiers.alt {
            return Used::No;
        }
        let rows = self.rows();
        if rows.is_empty() {
            return Used::No;
        }
        let at = self.selected_row(&rows, false);
        let row = &rows[at];
        let last = rows.len() - 1;
        let page = self.height.get().max(1);
        let to = |i: usize| self.select(&rows, i.min(last));
        let TreeKeys {
            toggle,
            letters,
            open_close,
        } = self.keys;
        let letter = |c: char| letters && key.code == KeyCode::Char(c);
        let right = open_close && (key.code == KeyCode::Right || letter('l'));
        let left = open_close && (key.code == KeyCode::Left || letter('h'));
        match key.code {
            KeyCode::Up => to(at.saturating_sub(1)),
            _ if letter('k') => to(at.saturating_sub(1)),
            KeyCode::Down => to(at + 1),
            _ if letter('j') => to(at + 1),
            KeyCode::PageUp => to(at.saturating_sub(page)),
            KeyCode::PageDown => to(at + page),
            KeyCode::Home => to(0),
            _ if letter('g') => to(0),
            KeyCode::End => to(last),
            _ if letter('G') => to(last),
            _ if right && row.parent => {
                if self.is_expanded(row) {
                    to(at + 1);
                } else {
                    self.set_expanded(&rows, at, true);
                }
            }
            _ if left => {
                if row.parent && self.is_expanded(row) {
                    self.set_expanded(&rows, at, false);
                } else if row.path.len() > 1 {
                    let parent = &row.path[..row.path.len() - 1];
                    let up = rows.iter().position(|r| r.path == parent).unwrap_or(at);
                    to(up);
                } else {
                    return Used::No;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') if toggle && row.parent => {
                let open = !self.is_expanded(row);
                self.set_expanded(&rows, at, open);
            }
            _ => return Used::No,
        }
        Used::Yes
    }
}

impl Widget for Tree {
    fn name(&self) -> &'static str {
        "tree"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Tree
    }

    fn cursor(&self) -> Option<crate::screen::Rect> {
        let rows = self.rows();
        let at = self.selected_row(&rows, false);
        let row = at.checked_sub(self.first.get())?;
        // The label, after the indent and the arrow (and the marker, in
        // text mode): what is selected is the item, not its arrow.
        let (depth, lead) = rows
            .get(at)
            .map_or((1, 0), |row| (row.path.len(), self.lead(row)));
        let x = (lead as usize + 2 * depth).min(u16::MAX as usize) as u16 + self.left.get();
        (row < self.height.get().max(1)).then_some(crate::screen::Rect::new(
            x,
            row as u16,
            u16::MAX,
            1,
        ))
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        let rows = self.rows();
        let at = self.selected_row(&rows, false);
        let mut state = crate::a11y::AccessState::item(at, rows.len());
        state.expanded = rows
            .get(at)
            .filter(|row| row.parent)
            .map(|row| self.is_expanded(row));
        state
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => self.rows().len().min(u16::MAX as usize) as u16,
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (mut width, height) = (canvas.width(), canvas.height() as usize);
        self.height.set(height.max(1));
        let rows = self.rows();
        // The bar keeps a column once the rows do not all fit.
        let look = self
            .bar
            .look
            .clone()
            .filter(|look| look.wanted(rows.len(), height));
        let mut left = 0;
        let mut bar = None;
        if let Some(look) = &look {
            let (rest, at) = look.split(width, canvas.height());
            (left, width) = (rest.x, rest.width);
            bar = Some(at);
        }
        self.left.set(left);
        let selected = self.selected_row(&rows, true);
        self.last.set(selected);
        // Keep the selection in view.
        let mut first = self
            .first
            .get()
            .min(rows.len().saturating_sub(height.max(1)));
        if selected < first {
            first = selected;
        } else if height > 0 && selected >= first + height {
            first = selected + 1 - height;
        }
        self.first.set(first);
        let highlight = if cx.focused() {
            cx.style("selected", "reverse")
        } else {
            cx.style("tree.selected", "underline")
        };
        let console = cx.console();
        let symbol = self
            .symbol
            .as_deref()
            .map(|markup| (markup, crate::widget::markup_width(markup)));
        for (i, row) in rows.iter().enumerate().skip(first).take(height) {
            let marker = match (row.parent, self.is_expanded(row)) {
                (false, _) => "  ",
                (true, true) => "▾ ",
                (true, false) => "▸ ",
            };
            let mut line = Vec::new();
            if crate::a11y::text_mode() {
                // A marker on the selected row, not only a style.
                line.push(Segment::new(if i == selected { "> " } else { "  " }, None));
            }
            if let Some((markup, cells)) = symbol {
                match i == selected {
                    true => line.extend(crate::widget::markup_line(console, markup, cells)),
                    false => line.push(Segment::new(" ".repeat(cells as usize), None)),
                }
            }
            if !row.prefix.is_empty() {
                let cells = crate::widget::markup_width(&row.prefix);
                line.extend(crate::widget::markup_line(console, &row.prefix, cells));
            }
            let indent = format!("{}{marker}", "  ".repeat(row.path.len() - 1));
            line.push(Segment::new(indent, None));
            let used: usize = line.iter().map(Segment::cell_length).sum();
            let used = used.min(width as usize) as u16;
            line.extend(cell_line(console, &row.label, width - used));
            if i == selected {
                line = over(line, &highlight);
            }
            canvas.lines_at(left, (i - first) as u16, width, 1, &[line]);
        }
        match (look, bar) {
            (Some(look), Some(at)) => {
                look.paint(console, canvas, at, first, rows.len(), height);
                self.bar_at.set(Some((at, rows.len())));
            }
            _ => self.bar_at.set(None),
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        // A press or a drag on the bar scrolls by it, carrying the
        // selection into view.
        if let (WidgetEvent::Mouse(mouse), Some((rect, len))) = (event, self.bar_at.get()) {
            let view = self.height.get();
            match self.bar.mouse(cx, mouse, rect, self.first.get(), len, view) {
                Some(Some(to)) => {
                    self.first.set(to);
                    let rows = self.rows();
                    let at = self.selected_row(&rows, false);
                    let inside = at.clamp(to, (to + view).saturating_sub(1).max(to));
                    if inside != at {
                        self.select(&rows, inside);
                    }
                    cx.redraw();
                    return Used::Yes;
                }
                Some(None) => return Used::Yes,
                None => {}
            }
        }
        match event {
            WidgetEvent::Key(key) => self.key(key),
            WidgetEvent::Mouse(mouse) => {
                let rows = self.rows();
                let mouse = rich_interact::Mouse {
                    column: mouse.column.saturating_sub(self.left.get()),
                    ..*mouse
                };
                let mouse = &mouse;
                match mouse.kind {
                    MouseKind::Down(button) => {
                        let at = self.first.get() + mouse.row as usize;
                        let Some(row) = rows.get(at) else {
                            // Below the rows: another button is used up, so
                            // no menu speaks for an old selection.
                            return if button == Button::Left {
                                Used::No
                            } else {
                                Used::Yes
                            };
                        };
                        let arrow = self.lead(row) + 2 * (row.path.len() as u16 - 1);
                        let left = button == Button::Left;
                        if left && row.parent && (arrow..arrow + 2).contains(&mouse.column) {
                            let open = !self.is_expanded(row);
                            self.set_expanded(&rows, at, open);
                        }
                        // Collapsing may have moved the selection to the
                        // item already.
                        self.select(&rows, at);
                        // Another button selects and leaves the press to
                        // the node's own handler (a context menu).
                        if !left {
                            return Used::No;
                        }
                    }
                    MouseKind::ScrollUp | MouseKind::ScrollDown if !rows.is_empty() => {
                        let at = self.selected_row(&rows, false);
                        let at = if mouse.kind == MouseKind::ScrollUp {
                            at.saturating_sub(3)
                        } else {
                            (at + 3).min(rows.len() - 1)
                        };
                        self.select(&rows, at);
                    }
                    _ => return Used::No,
                }
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Node {
    /// On a [`tree`]: take only the keys `keys` names, leaving the others
    /// to the tree's ancestors (an app's Enter on a folder, its own `h`).
    /// See [`TreeKeys`]. On any other node it does nothing.
    pub fn tree_keys(self, keys: TreeKeys) -> Node {
        self.widget_mut(|tree: &mut Tree| tree.keys = keys);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_follow_the_expanded_items() {
        let items = vec![
            TreeItem::new("a").children([TreeItem::new("a0"), TreeItem::new("a1")]),
            TreeItem::new("b"),
        ];
        let paths = |expanded: HashSet<Vec<usize>>| -> Vec<Vec<usize>> {
            flatten(&items, &|path, _| expanded.contains(path))
                .into_iter()
                .map(|r| r.path)
                .collect()
        };
        assert_eq!(paths(HashSet::new()), [vec![0], vec![1]]);
        assert_eq!(
            paths(HashSet::from([vec![0]])),
            [vec![0], vec![0, 0], vec![0, 1], vec![1]]
        );
        let rows = flatten(&items, &|_, _| false);
        // A hidden selection shows on its nearest shown ancestor.
        assert_eq!(index_of(&rows, &[0, 1]), 0);
        assert_eq!(index_of(&rows, &[1]), 1);
        assert_eq!(index_of(&rows, &[7]), 0);
    }

    #[test]
    fn a_key_is_found_at_any_depth() {
        let items = vec![
            TreeItem::keyed("a", "a").child(TreeItem::keyed("a/b", "b")),
            TreeItem::keyed("c", "c"),
        ];
        assert_eq!(path_of(&items, "a/b"), Some(vec![0, 0]));
        assert_eq!(path_of(&items, "c"), Some(vec![1]));
        assert_eq!(path_of(&items, "z"), None);
    }
}

/// An item of a [`tree_lazy`]: what it shows, the key its children are
/// loaded by (a path, an id), and whether it has children to load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LazyItem {
    pub key: String,
    pub label: String,
    pub has_children: bool,
}

impl LazyItem {
    /// An item with no children.
    pub fn leaf(key: impl Into<String>, label: impl Into<String>) -> LazyItem {
        LazyItem {
            key: key.into(),
            label: label.into(),
            has_children: false,
        }
    }

    /// An item whose children are loaded when it is first opened.
    pub fn branch(key: impl Into<String>, label: impl Into<String>) -> LazyItem {
        LazyItem {
            key: key.into(),
            label: label.into(),
            has_children: true,
        }
    }
}

/// A [`tree`] whose children are loaded when an item is first opened:
/// `children(key)` runs on a worker thread, so it may read a disk or a
/// network, and the item shows `loading…` until it returns (or its error,
/// if it fails). Each level is loaded once and kept. `roots` may read
/// signals. `selected` holds the selected item's key.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{tree_lazy, LazyItem};
///
/// let app = App::new(|| {
///     let selected = signal(None);
///     let roots = || vec![LazyItem::branch("/src", "src"), LazyItem::leaf("/a.txt", "a.txt")];
///     let children = |key: String| -> Result<Vec<LazyItem>, String> {
///         Ok(vec![LazyItem::leaf(format!("{key}/main.rs"), "main.rs")])
///     };
///     let shown = text(move || format!("{:?}", selected.get()));
///     column([tree_lazy(roots, children, selected).fixed(3), shown])
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.wait_for_tasks(true).render_with(&["right", "down", "q"], 24, 4).unwrap();
/// assert_eq!(screen[1].trim_end(), "    main.rs");
/// assert_eq!(screen[3].trim_end(), "Some(\"/src/main.rs\")");
/// ```
pub fn tree_lazy<E: std::fmt::Display>(
    roots: impl Fn() -> Vec<LazyItem> + 'static,
    children: impl Fn(String) -> Result<Vec<LazyItem>, E> + Send + Sync + 'static,
    selected: Signal<Option<String>>,
) -> Node {
    tree_lazy_with(roots, children, selected, LazyTree::new())
}

/// What a [`tree_lazy_with`] has loaded, to load again: a level whose
/// items changed (a file deleted, a folder renamed) is
/// [reloaded](Self::reload) and its open items with it. `Copy`: any
/// handler takes one. Made while the app is built.
#[derive(Clone, Copy)]
pub struct LazyTree {
    loaded: Signal<std::collections::HashMap<String, crate::task::Load<Vec<LazyItem>>>>,
    /// Bumped by a reload, so the levels open are looked at again.
    generation: Signal<u64>,
}

impl LazyTree {
    /// A handle with nothing loaded. Call it while the app is built.
    pub fn new() -> LazyTree {
        LazyTree {
            loaded: signal(std::collections::HashMap::new()),
            generation: signal(0),
        }
    }

    /// Load the children of the item `key` names again, if it is open (it
    /// shows `loading…` until they come); else when it is next opened.
    pub fn reload(&self, key: &str) {
        let had = self
            .loaded
            .with_untracked(|loaded| loaded.contains_key(key));
        if had {
            self.loaded.update(|loaded| {
                loaded.remove(key);
            });
        }
        self.generation.update(|g| *g += 1);
    }

    /// Load every level again.
    pub fn reload_all(&self) {
        self.loaded.update(|loaded| loaded.clear());
        self.generation.update(|g| *g += 1);
    }

    /// Whether the children of `key` have loaded (and not failed).
    pub fn is_loaded(&self, key: &str) -> bool {
        self.loaded
            .with(|loaded| matches!(loaded.get(key), Some(crate::task::Load::Ready(_))))
    }
}

impl Default for LazyTree {
    fn default() -> LazyTree {
        LazyTree::new()
    }
}

/// A [`tree_lazy`] whose loaded levels are kept in `loads`, to
/// [reload](LazyTree::reload) one.
///
/// ```
/// use std::sync::atomic::{AtomicUsize, Ordering};
/// use std::sync::Arc;
///
/// use intuituive::prelude::*;
/// use intuituive::widgets::{tree_lazy_with, LazyItem, LazyTree};
///
/// let app = App::new(|| {
///     let loads = LazyTree::new();
///     let calls = Arc::new(AtomicUsize::new(0));
///     let children = move |key: String| -> Result<Vec<LazyItem>, String> {
///         let n = calls.fetch_add(1, Ordering::SeqCst);
///         Ok(vec![LazyItem::leaf(format!("{key}/{n}"), format!("load {n}"))])
///     };
///     let roots = || vec![LazyItem::branch("/src", "src")];
///     tree_lazy_with(roots, children, signal(None), loads)
///         .on_key("r", move |_| loads.reload("/src"))
/// });
/// let mut driver = app.wait_for_tasks(true).driver(16, 2);
/// driver.press("right");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "    load 0");
/// driver.press("r");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "    load 1");
/// ```
pub fn tree_lazy_with<E: std::fmt::Display>(
    roots: impl Fn() -> Vec<LazyItem> + 'static,
    children: impl Fn(String) -> Result<Vec<LazyItem>, E> + Send + Sync + 'static,
    selected: Signal<Option<String>>,
    loads: LazyTree,
) -> Node {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::reactive::watch;
    use crate::task::{spawn, Load};

    let LazyTree { loaded, generation } = loads;
    let path = signal(Vec::<usize>::new());
    let expanded = signal(HashSet::<Vec<usize>>::new());
    let roots: Rc<dyn Fn() -> Vec<LazyItem>> = Rc::new(roots);
    let children = Arc::new(children);

    // The item at `path` among `roots` and what has loaded.
    fn find(
        roots: &[LazyItem],
        loaded: &HashMap<String, Load<Vec<LazyItem>>>,
        path: &[usize],
    ) -> Option<LazyItem> {
        let (first, rest) = path.split_first()?;
        let mut item = roots.get(*first)?.clone();
        for &i in rest {
            item = match loaded.get(&item.key) {
                Some(Load::Ready(kids)) => kids.get(i)?.clone(),
                _ => return None,
            };
        }
        Some(item)
    }

    // An item with what it has loaded under it, as the tree draws it.
    // Only open items are grown further: a closed one needs just a row to
    // show it has children, and data whose keys repeat on their own
    // ancestors (a link back up a folder tree) cannot recurse forever.
    fn grow(
        item: &LazyItem,
        loaded: &HashMap<String, Load<Vec<LazyItem>>>,
        open: &HashSet<Vec<usize>>,
        path: &mut Vec<usize>,
    ) -> TreeItem {
        let label = rich::markup::escape(&item.label);
        let node = TreeItem::new(label);
        if !item.has_children {
            return node;
        }
        match loaded.get(&item.key) {
            Some(Load::Ready(kids)) if open.contains(path.as_slice()) || kids.is_empty() => node
                .children(kids.iter().enumerate().map(|(i, kid)| {
                    path.push(i);
                    let grown = grow(kid, loaded, open, path);
                    path.pop();
                    grown
                })),
            Some(Load::Ready(_)) => node.child(TreeItem::new("")),
            Some(Load::Failed(error)) => node.child(TreeItem::new(format!(
                "[red]{}[/]",
                rich::markup::escape(error)
            ))),
            // Loading, or not asked for yet: a row to show it has children.
            _ => node.child(TreeItem::new("[dim]loading…[/]")),
        }
    }

    // Load what an opened item holds, once: when an item is opened, and
    // when the roots change under items already open.
    let opened = roots.clone();
    watch(
        move || (expanded.get(), opened(), generation.get()),
        move |(open, roots, _): (HashSet<Vec<usize>>, Vec<LazyItem>, u64), _| {
            for path in open {
                let item = loaded.with_untracked(|loaded| find(&roots, loaded, &path));
                let Some(item) = item.filter(|item| item.has_children) else {
                    continue;
                };
                if loaded.with_untracked(|loaded| loaded.contains_key(&item.key)) {
                    continue;
                }
                loaded.update(|loaded| {
                    loaded.insert(item.key.clone(), Load::Loading);
                });
                let (key, children) = (item.key.clone(), children.clone());
                spawn(
                    move || children(key).map_err(|error| error.to_string()),
                    move |result, _| {
                        loaded.update(|loaded| {
                            loaded.insert(
                                item.key.clone(),
                                match result {
                                    Ok(kids) => Load::Ready(kids),
                                    Err(error) => Load::Failed(error),
                                },
                            );
                        })
                    },
                );
            }
        },
    );
    // The selected key follows the selected path.
    let named = roots.clone();
    watch(
        move || {
            let roots = named();
            path.with(|path| loaded.with(|loaded| find(&roots, loaded, path)))
                .map(|item| item.key)
        },
        move |key, _| selected.set(key),
    );
    let items = move || {
        let roots = roots();
        let open = expanded.get();
        loaded.with(|loaded| {
            let mut path = Vec::new();
            roots
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    path.push(i);
                    let grown = grow(item, loaded, &open, &mut path);
                    path.pop();
                    grown
                })
                .collect()
        })
    };
    // Busy, for assistive technology, while a level loads.
    let loading = move || {
        loaded.with_untracked(|loaded| loaded.values().any(|load| matches!(load, Load::Loading)))
    };
    tree_with(items, path, expanded).busy_when(loading)
}
