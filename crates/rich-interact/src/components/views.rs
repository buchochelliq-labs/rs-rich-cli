//! Tables and trees to pick from (#491): a [`Select`] whose items are a
//! table's rows or a tree's nodes, so they filter, preview and take
//! [`Actions`] the same way, with each action told what it is done to (a
//! [`TargetKind::Row`] or a [`TargetKind::Node`]).
//!
//! Since 0.0.14 a table copies its focused row or cell as text, CSV or
//! JSON (#434), and a tree filters as a tree, keeping each match's
//! ancestors (#428), shows the focused node's breadcrumbs and copies its
//! path (#464), all through the terminal [clipboard](crate::clipboard).

use std::sync::Arc;
use std::time::Duration;

use rich::cells::cell_len;
use rich::{Segment, Style};

use crate::clipboard::{self, CopyFormat};
use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, shown, text, PreviewLayout, Select, Theme};
use crate::event::{Event, Key, KeyCode};
use crate::item::{Actions, Item, TargetKind};
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};

/// The builder methods both views pass on to their [`Select`].
macro_rules! select_builders {
    () => {
        /// Show at most `rows` rows at once (default 10).
        pub fn height(mut self, rows: usize) -> Self {
            self.select = self.select.height(rows);
            self
        }

        pub fn preview(mut self, layout: PreviewLayout) -> Self {
            self.select = self.select.preview(layout);
            self
        }

        pub fn theme(mut self, theme: Theme) -> Self {
            self.select = self.select.theme(theme);
            self
        }

        /// Start with this filter text.
        pub fn query(mut self, query: impl Into<String>) -> Self {
            self.select = self.select.query(query);
            self
        }

        /// The row returned without a terminal, and focused first.
        pub fn default(mut self, index: usize) -> Self {
            self.select = self.select.default(index);
            self
        }

        /// Actions offered on every row, beside each row's own.
        pub fn actions(mut self, actions: Actions) -> Self {
            self.select = self.select.actions(actions);
            self
        }

        /// The key that opens the action menu (default Ctrl+K).
        pub fn menu_key(mut self, key: Key) -> Self {
            self.select = self.select.menu_key(key);
            self
        }

        /// Report the mouse: see [`Select::with_mouse`].
        pub fn with_mouse(mut self, on: bool) -> Self {
            self.select = self.select.with_mouse(on);
            self
        }

        /// The id of the action that picked the row, if one did.
        pub fn action(&self) -> Option<&str> {
            self.select.action()
        }

        pub fn items(&self) -> &[Item<T>] {
            self.select.items()
        }

        /// The underlying list.
        pub fn select(&self) -> &Select<T> {
            &self.select
        }

        /// An icon before each row's label, by item index: see
        /// [`Select::set_icons`].
        pub fn set_icons(&mut self, icons: Vec<Option<rich::Text>>) {
            self.select.set_icons(icons);
        }
    };
}

/// The keys a [`TableSelect`] adds to its list's, in context `table`.
pub fn table_keymap() -> Keymap {
    Keymap::new("table")
        .bind("copy-row", keys("ctrl+y"), "copy the row")
        .bind("copy-cell", keys("alt+y"), "copy the focused cell")
        .bind("next-column", keys("ctrl+right"), "focus the next column")
        .bind(
            "previous-column",
            keys("ctrl+left"),
            "focus the previous column",
        )
        .bind("copy-format", keys("alt+f"), "copy as text, CSV or JSON")
}

/// Pick a row of a table: columns aligned under their headings, rows
/// filtered by any cell. Returns the row's value.
///
/// Ctrl+Y copies the focused row, and Alt+Y its focused cell, to the
/// terminal's [clipboard](crate::clipboard) (#434): as text (cells
/// separated by tabs), CSV or JSON (an object keyed by the headings),
/// cycled with Alt+F or set with [`copy_format`](Self::copy_format).
/// Ctrl+Right and Ctrl+Left move the focused column, which the headings
/// underline.
pub struct TableSelect<T> {
    select: Select<T>,
    headers: Vec<String>,
    cells: Vec<Vec<String>>,
    widths: Vec<usize>,
    /// The focused column, once moved to.
    column: Option<usize>,
    format: CopyFormat,
    keymap: Keymap,
}

impl<T> TableSelect<T> {
    /// A table of `rows`, each an item (its value, actions, preview) and
    /// its cells, under `headers`.
    pub fn new<H, S, R, V>(prompt: impl Into<String>, headers: H, rows: R) -> TableSelect<T>
    where
        H: IntoIterator<Item = S>,
        S: Into<String>,
        R: IntoIterator<Item = (V, Vec<String>)>,
        V: Into<Item<T>>,
    {
        let headers: Vec<String> = headers.into_iter().map(|h| shown(&h.into())).collect();
        let rows: Vec<(Item<T>, Vec<String>)> = rows
            .into_iter()
            .map(|(item, cells)| (item.into(), cells.iter().map(|c| shown(c)).collect()))
            .collect();
        let columns = rows
            .iter()
            .map(|(_, cells)| cells.len())
            .chain([headers.len()])
            .max()
            .unwrap_or(0);
        let mut widths = vec![0; columns];
        for cells in rows.iter().map(|(_, cells)| cells).chain([&headers]) {
            for (column, cell) in cells.iter().enumerate() {
                widths[column] = widths[column].max(cell_len(cell));
            }
        }
        let line = |cells: &[String]| -> String {
            let mut out = String::new();
            for (column, width) in widths.iter().enumerate() {
                let cell = cells.get(column).map_or("", String::as_str);
                out.push_str(cell);
                if column + 1 < widths.len() {
                    out.push_str(&" ".repeat(width - cell_len(cell) + 2));
                }
            }
            out.trim_end().to_string()
        };
        let mut values = Vec::with_capacity(rows.len());
        let mut table = Vec::with_capacity(rows.len());
        let items: Vec<Item<T>> = rows
            .into_iter()
            .map(|(mut item, cells)| {
                item.label = line(&cells);
                values.push(cells.join("\t"));
                table.push(cells);
                item
            })
            .collect();
        let mut select = Select::new(prompt, items);
        select.set_kind(TargetKind::Row);
        select.set_values(values);
        let mut view = TableSelect {
            select,
            headers,
            cells: table,
            widths,
            column: None,
            format: CopyFormat::Text,
            keymap: table_keymap(),
        };
        view.heading();
        view
    }

    select_builders!();

    /// How Ctrl+Y and Alt+Y copy (default: text).
    pub fn copy_format(mut self, format: CopyFormat) -> Self {
        self.format = format;
        self
    }

    /// The focused column, once one has been moved to.
    pub fn column(&self) -> Option<usize> {
        self.column
    }

    /// Make `keys` do `action` (see [`table_keymap`]) on this table only.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    /// The row of headings, the focused column underlined.
    fn heading(&mut self) {
        let style = Theme::default().prompt;
        let Some(focused_column) = self.column else {
            // One segment, as before columns could be focused.
            let mut heading = String::new();
            for (column, width) in self.widths.iter().enumerate() {
                let cell = self.headers.get(column).map_or("", String::as_str);
                heading.push_str(cell);
                if column + 1 < self.widths.len() {
                    heading.push_str(&" ".repeat(width - cell_len(cell) + 2));
                }
            }
            self.select.heading = Some(vec![text(heading.trim_end(), &style)]);
            return;
        };
        let focused = style.combine(&Style::parse("underline").expect("a style"));
        let mut line: Vec<Segment> = Vec::new();
        let last = self.widths.len().saturating_sub(1);
        for (column, width) in self.widths.iter().enumerate() {
            let cell = self.headers.get(column).map_or("", String::as_str);
            let style = if column == focused_column {
                &focused
            } else {
                &style
            };
            if !cell.is_empty() {
                line.push(text(cell, style));
            }
            if column < last {
                line.push(plain(" ".repeat(width - cell_len(cell) + 2)));
            }
        }
        // As before a column was focused: no padding after the last heading.
        while line
            .last()
            .is_some_and(|segment| segment.style.is_none() && segment.text.trim().is_empty())
        {
            line.pop();
        }
        self.select.heading = Some(line);
    }

    /// Copy the focused row, or its focused cell.
    fn copy(&mut self, cell: bool) {
        let Some(index) = self.select.focused() else {
            return;
        };
        let row = &self.cells[index];
        let (what, copied) = if cell {
            let column = self.column.unwrap_or(0);
            let value = row.get(column).map_or("", String::as_str);
            ("cell", self.format.cell(value))
        } else {
            ("row", self.format.row(&self.headers, row))
        };
        let result = clipboard::copy(copied);
        let what = format!("{what} as {}", self.format.name());
        self.select
            .set_status(Some(clipboard::report(&what, &result)));
    }

    /// Carry out a table key; `false` when `key` is not one.
    fn table_key(&mut self, key: Key) -> bool {
        match self.keymap.action(key) {
            Some("copy-row") => self.copy(false),
            Some("copy-cell") => self.copy(true),
            Some(step @ ("next-column" | "previous-column")) => {
                let columns = self.widths.len().max(1);
                let at = self.column.unwrap_or(0);
                self.column = Some(if step == "next-column" {
                    (at + 1).min(columns - 1)
                } else {
                    at.saturating_sub(1)
                });
                self.heading();
            }
            Some("copy-format") => {
                self.format = self.format.next();
                self.select
                    .set_status(Some(format!("copy as {}", self.format.name())));
            }
            _ => return false,
        }
        true
    }
}

impl<T: Clone> Component for TableSelect<T> {
    type Output = T;

    fn keymap(&self) -> Keymap {
        let mut keymap = if self.select.menu_open() {
            Keymap::default()
        } else {
            self.keymap.clone()
        };
        keymap.extend(self.select.visible_keymap());
        keymap
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<T> {
        if let Some(key) = event.key() {
            if !self.select.menu_open() && self.table_key(key) {
                return Flow::Continue;
            }
        }
        self.select.handle(event, context)
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.select.render(context)
    }

    fn tick(&self) -> Option<Duration> {
        self.select.tick()
    }

    fn mouse(&self) -> bool {
        Component::mouse(&self.select)
    }

    fn default_value(&self) -> Option<T> {
        self.select.default_value()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<T>, NotInteractive> {
        self.select.prompt(io)
    }
}

/// The keys a [`TreeSelect`] adds to its list's, in context `tree`.
pub fn tree_keymap() -> Keymap {
    Keymap::new("tree")
        .bind("expand", keys("right"), "expand")
        .bind("collapse", keys("left"), "collapse, or go to the parent")
        .bind("copy-path", keys("ctrl+y"), "copy the path")
}

/// Pick a node of a tree. Nodes are given in order, each with its depth
/// (0 for a root); Right expands the focused node and Left collapses it,
/// or moves to its parent.
///
/// Typing searches every node, collapsed or not, and filters as a tree
/// (#428): each match keeps its ancestors, dimmed, so you see where it is,
/// and the cursor goes to the best match. With
/// [`breadcrumbs`](Self::breadcrumbs) on, a line under the question shows
/// the path to the focused node (#464). Ctrl+Y copies that path (the
/// node's [`ActionTarget::value`](crate::ActionTarget::value)) to the
/// terminal's [clipboard](crate::clipboard).
pub struct TreeSelect<T> {
    select: Select<T>,
    parents: Arc<[Option<usize>]>,
    /// Whether each node is the last of its parent's children.
    last: Vec<bool>,
    parent_of_any: Vec<bool>,
    collapsed: Vec<bool>,
    /// Each node's name in the breadcrumbs, by index, where it is not its
    /// label.
    crumbs: Vec<String>,
    breadcrumbs: bool,
    /// The width the breadcrumbs were last laid out at.
    width: usize,
    /// Whether the guides are drawn for a filtered list.
    filtered: bool,
    keymap: Keymap,
}

impl<T> TreeSelect<T> {
    /// A tree of `nodes`, in order, each with its depth.
    ///
    /// Building it is linear in the number of nodes, and so is what it
    /// keeps: each node's path (the labels joined by `/`) is worked out
    /// when it is asked for, and its guides when its row is drawn.
    pub fn new<I, V>(prompt: impl Into<String>, nodes: I) -> TreeSelect<T>
    where
        I: IntoIterator<Item = (usize, V)>,
        V: Into<Item<T>>,
    {
        let mut items: Vec<Item<T>> = Vec::new();
        let mut parents: Vec<Option<usize>> = Vec::new();
        // The open path: the last node seen at each depth down to the one
        // before.
        let mut open: Vec<usize> = Vec::new();
        for (depth, node) in nodes {
            // No deeper than one below the node before.
            let depth = depth.min(open.len());
            open.truncate(depth);
            let index = items.len();
            parents.push(open.last().copied());
            open.push(index);
            let mut item: Item<T> = node.into();
            item.label = shown(&item.label);
            items.push(item);
        }
        let count = items.len();
        let mut parent_of_any = vec![false; count];
        for parent in parents.iter().flatten() {
            parent_of_any[*parent] = true;
        }
        let last = last_children(&parents, None);
        let parents: Arc<[Option<usize>]> = parents.into();
        let mut select = Select::new(prompt, items);
        select.set_kind(TargetKind::Node);
        select.hints = Some("←→ fold".into());
        select.set_tree(Some(parents.to_vec()));
        let mut tree = TreeSelect {
            select,
            parents,
            last,
            parent_of_any,
            collapsed: vec![false; count],
            crumbs: Vec::new(),
            breadcrumbs: false,
            width: usize::MAX,
            filtered: false,
            keymap: tree_keymap(),
        };
        tree.update();
        tree
    }

    /// Show the path to the focused node on a line under the question
    /// (#464): `root › parent › node`, cut from the left when it is too
    /// wide.
    pub fn breadcrumbs(mut self, on: bool) -> Self {
        self.breadcrumbs = on;
        self.crumb_line();
        self
    }

    /// Each node's name in the breadcrumbs, by index, in place of its
    /// label: a key rather than `key: value`, say.
    pub fn crumbs(mut self, crumbs: Vec<String>) -> Self {
        let count = self.parents.len();
        self.crumbs = crumbs
            .into_iter()
            .take(count)
            .map(|crumb| shown(&crumb))
            .collect();
        self.crumb_line();
        self
    }

    /// What Ctrl+Y copies and actions see as each node's value, by index,
    /// in place of the labels joined by `/`: a JSON path, say. Nodes past
    /// the end of `paths` keep the joined labels.
    pub fn paths(mut self, paths: Vec<String>) -> Self {
        self.select.set_values(paths);
        self
    }

    /// Like [`paths`](Self::paths), with each node's value worked out from
    /// its index when it is asked for (the focused node's, on Ctrl+Y or an
    /// action), so a large tree keeps no string per node.
    pub fn paths_with(mut self, path: impl Fn(usize) -> String + Send + Sync + 'static) -> Self {
        self.select.set_values(Vec::new());
        self.select.value_of = Some(Arc::new(path));
        self
    }

    /// Collapse every node at `depth` or deeper that has children, and
    /// expand the rest: `fold_below(1)` shows the roots' children only.
    pub fn fold_below(mut self, depth: usize) -> Self {
        // Parents come before their children, so one pass finds each
        // node's depth.
        let mut depths = vec![0usize; self.parents.len()];
        for index in 0..self.parents.len() {
            depths[index] = self.parents[index].map_or(0, |parent| depths[parent] + 1);
            self.collapsed[index] = depths[index] >= depth && self.parent_of_any[index];
        }
        self.update();
        self
    }

    /// Make `keys` do `action` (see [`tree_keymap`]) on this tree only.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    /// Each node's parent, by index.
    pub fn parents(&self) -> &[Option<usize>] {
        &self.parents
    }

    /// The focused node's index.
    pub fn focused(&self) -> Option<usize> {
        self.select.focused()
    }

    /// The names on the path to node `index`, from the root.
    pub fn path_to(&self, index: usize) -> Vec<&str> {
        let mut path: Vec<&str> = crate::kit::ancestors(&self.parents, index)
            .map(|ancestor| self.crumb(ancestor))
            .collect();
        path.reverse();
        path.push(self.crumb(index));
        path
    }

    /// Node `index`'s name in the breadcrumbs.
    fn crumb(&self, index: usize) -> &str {
        match self.crumbs.get(index) {
            Some(crumb) => crumb,
            None => &self.select.items()[index].label,
        }
    }

    /// Show `status` in place of the footer until the next key.
    pub fn set_status(&mut self, status: Option<String>) {
        self.select.set_status(status);
    }

    /// The footer's extra key hints (default `←→ fold`).
    pub fn set_hints(&mut self, hints: Option<String>) {
        self.select.set_hints(hints);
    }

    /// The breadcrumbs of the focused node, at the last width seen.
    fn crumb_line(&mut self) {
        if !self.breadcrumbs {
            return;
        }
        let path = self.select.focused().map(|index| self.path_to(index));
        let line = breadcrumb_line(path.as_deref().unwrap_or(&[]), self.width);
        self.select.heading = Some(line);
    }

    /// Ctrl+Y: copy the focused node's path.
    fn copy_path(&mut self) {
        let Some(index) = self.select.focused() else {
            return;
        };
        let path = self.select.target(index).value;
        let result = clipboard::copy(path);
        self.select
            .set_status(Some(clipboard::report("path", &result)));
    }

    /// Start with every node that has children collapsed (`true`) or
    /// expanded.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        for (index, parent) in self.parent_of_any.iter().enumerate() {
            self.collapsed[index] = collapsed && *parent;
        }
        self.update();
        self
    }

    select_builders!();

    /// Whether node `index` is collapsed.
    pub fn is_collapsed(&self, index: usize) -> bool {
        self.collapsed.get(index).copied().unwrap_or(false)
    }

    /// Collapse (`true`) or expand node `index`.
    pub fn set_collapsed(&mut self, index: usize, collapsed: bool) {
        if self.parent_of_any.get(index).copied().unwrap_or(false) {
            self.collapsed[index] = collapsed;
            self.update();
        }
    }

    /// The guides and fold markers, and which nodes a collapse hides.
    fn update(&mut self) {
        // Parents come before their children: a node is hidden when its
        // parent is collapsed or hidden.
        let mut hidden = vec![false; self.parents.len()];
        for index in 0..self.parents.len() {
            hidden[index] =
                self.parents[index].is_some_and(|parent| self.collapsed[parent] || hidden[parent]);
        }
        self.select.guides = Some(self.guides(None));
        self.select.hidden = hidden;
        self.select.refilter();
        self.filtered = false;
        self.refresh_guides();
        self.crumb_line();
    }

    /// What the guides are drawn from. With `shown`, only the nodes listed
    /// count: a filtered tree draws the branches between what it lists,
    /// and a node whose children are listed is open.
    fn guides(&self, shown: Option<&[bool]>) -> Guides {
        let count = self.parents.len();
        let (last, open) = match shown {
            None => (self.last.clone(), None),
            Some(shown) => {
                let mut open = vec![false; count];
                for (index, parent) in self.parents.iter().enumerate() {
                    if let Some(parent) = parent.filter(|_| shown[index]) {
                        open[parent] = true;
                    }
                }
                (last_children(&self.parents, Some(shown)), Some(open))
            }
        };
        let markers = (0..count)
            .map(|index| {
                let collapsed = match &open {
                    Some(open) => !open[index],
                    None => self.collapsed[index],
                };
                match (self.parent_of_any[index], collapsed) {
                    (true, true) => Marker::Folded,
                    (true, false) => Marker::Open,
                    _ => Marker::Leaf,
                }
            })
            .collect();
        Guides {
            parents: Arc::clone(&self.parents),
            last,
            markers,
        }
    }

    /// Redraw the guides for what the filter lists, once a query is typed,
    /// and back when it is cleared.
    fn refresh_guides(&mut self) {
        let filtering = !self.select.query_text().is_empty();
        if !filtering && !self.filtered {
            return;
        }
        let guides = if filtering {
            let mut shown = vec![false; self.parents.len()];
            for (index, _) in self.select.filter().matches() {
                shown[*index] = true;
            }
            self.guides(Some(&shown))
        } else {
            self.guides(None)
        };
        self.select.guides = Some(guides);
        self.filtered = filtering;
    }

    /// Right and Left: fold the focused node, or go to its parent.
    fn fold(&mut self, key: Key) -> bool {
        if !self.select.query_text().is_empty() || key.modifiers != Default::default() {
            return false;
        }
        let Some(index) = self.select.focused() else {
            return false;
        };
        match key.code {
            KeyCode::Right if self.collapsed[index] => {
                self.collapsed[index] = false;
                self.update();
            }
            KeyCode::Left if self.parent_of_any[index] && !self.collapsed[index] => {
                self.collapsed[index] = true;
                self.update();
            }
            KeyCode::Left => {
                if let Some(parent) = self.parents[index] {
                    self.select.focus_item(parent);
                }
            }
            KeyCode::Right => {}
            _ => return false,
        }
        true
    }
}

/// Whether each node is the last of its parent's children, in one pass
/// from the end. With `shown`, only the nodes listed count.
fn last_children(parents: &[Option<usize>], shown: Option<&[bool]>) -> Vec<bool> {
    let count = parents.len();
    let mut last = vec![true; count];
    // Whether a later child was seen, by parent (the roots at `count`).
    let mut seen = vec![false; count + 1];
    for index in (0..count).rev() {
        if shown.is_some_and(|shown| !shown[index]) {
            continue;
        }
        let slot = parents[index].unwrap_or(count);
        last[index] = !seen[slot];
        seen[slot] = true;
    }
    last
}

/// A tree node's fold marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    /// No children.
    Leaf,
    Folded,
    Open,
}

/// What a [`TreeSelect`]'s guides are drawn from. Its [`Select`] draws a
/// row's guides when it draws the row, so a large tree keeps no string per
/// node and the work is the rows on screen times their depth.
pub(crate) struct Guides {
    parents: Arc<[Option<usize>]>,
    /// Whether each node is drawn as the last of its parent's children.
    last: Vec<bool>,
    markers: Vec<Marker>,
}

impl Guides {
    /// Node `index`'s guides and fold marker: `│   ├── ▸ `.
    pub(crate) fn prefix(&self, index: usize) -> String {
        if index >= self.parents.len() {
            return String::new();
        }
        let up: Vec<usize> = crate::kit::ancestors(&self.parents, index).collect();
        let mut guides = String::new();
        // Each ancestor below the root draws a bar if more of its siblings
        // follow.
        for &ancestor in up.iter().rev().skip(1) {
            guides.push_str(if self.last[ancestor] {
                "    "
            } else {
                "│   "
            });
        }
        if self.parents[index].is_some() {
            guides.push_str(if self.last[index] {
                "└── "
            } else {
                "├── "
            });
        }
        guides.push_str(match self.markers[index] {
            Marker::Folded => "▸ ",
            Marker::Open => "▾ ",
            Marker::Leaf => "",
        });
        guides
    }

    /// Node `index`'s path: the names `name` gives it and its ancestors,
    /// from the root, joined by `/`.
    pub(crate) fn path<'a>(&self, index: usize, name: impl Fn(usize) -> &'a str) -> String {
        let mut path: Vec<&str> = crate::kit::ancestors(&self.parents, index)
            .map(&name)
            .collect();
        path.reverse();
        path.push(name(index));
        path.join("/")
    }
}

impl<T: Clone> Component for TreeSelect<T> {
    type Output = T;

    fn keymap(&self) -> Keymap {
        let mut keymap = if self.select.menu_open() {
            Keymap::default()
        } else {
            self.keymap.clone()
        };
        keymap.extend(self.select.visible_keymap());
        keymap
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<T> {
        self.width = context.width;
        let flow = match event.key() {
            Some(key) if !self.select.menu_open() && self.keymap.is(key, "copy-path") => {
                // A key clears the status first, as the list does.
                self.select.set_status(None);
                self.copy_path();
                Flow::Continue
            }
            Some(key) if !self.select.menu_open() && self.fold(key) => {
                self.select.set_status(None);
                Flow::Continue
            }
            _ => self.select.handle(event, context),
        };
        self.refresh_guides();
        self.crumb_line();
        flow
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.select.render(context)
    }

    fn tick(&self) -> Option<Duration> {
        self.select.tick()
    }

    fn mouse(&self) -> bool {
        Component::mouse(&self.select)
    }

    fn default_value(&self) -> Option<T> {
        self.select.default_value()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<T>, NotInteractive> {
        self.select.prompt(io)
    }
}

/// The path to a node as one line, `root › parent › node`, cut from the
/// left (`… › parent › node`) to fit `width`.
///
/// A private stand-in: once the shared breadcrumbs component lands
/// (0.0.14 workstream 2, #482), the tree and the explorers draw theirs
/// with it instead.
fn breadcrumb_line(path: &[&str], width: usize) -> Vec<Segment> {
    let theme = Theme::default();
    let separator = " › ";
    let room = width.saturating_sub(2);
    let mut first = 0;
    let total = |from: usize| -> usize {
        let names: usize = path[from..].iter().map(|name| cell_len(name)).sum();
        let gaps = path.len().saturating_sub(from + 1) * cell_len(separator);
        names
            + gaps
            + if from > 0 {
                cell_len("…") + cell_len(separator)
            } else {
                0
            }
    };
    while first + 1 < path.len() && total(first) > room {
        first += 1;
    }
    let mut line = Vec::new();
    if first > 0 {
        line.push(text("…", &theme.hint));
        line.push(text(separator, &theme.hint));
    }
    for (offset, name) in path[first..].iter().enumerate() {
        if offset > 0 {
            line.push(text(separator, &theme.hint));
        }
        let last = first + offset + 1 == path.len();
        line.push(if last {
            text(*name, &theme.focused)
        } else {
            text(*name, &theme.hint)
        });
    }
    fit(line, room)
}
