//! Tables and trees to pick from (#491): a [`Select`] whose items are a
//! table's rows or a tree's nodes, so they filter, preview and take
//! [`Actions`] the same way, with each action told what it is done to (a
//! [`TargetKind::Row`] or a [`TargetKind::Node`]).

use std::time::Duration;

use rich::cells::cell_len;

use crate::component::{Component, Context, Flow, View};
use crate::components::{shown, text, PreviewLayout, Select, Theme};
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
    };
}

/// Pick a row of a table: columns aligned under their headings, rows
/// filtered by any cell. Returns the row's value.
pub struct TableSelect<T> {
    select: Select<T>,
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
        let items: Vec<Item<T>> = rows
            .into_iter()
            .map(|(mut item, cells)| {
                item.label = line(&cells);
                values.push(cells.join("\t"));
                item
            })
            .collect();
        let mut select = Select::new(prompt, items);
        let heading = line(&headers);
        let style = Theme::default().prompt;
        select.heading = Some(vec![text(heading, &style)]);
        select.set_kind(TargetKind::Row);
        select.set_values(values);
        TableSelect { select }
    }

    select_builders!();
}

impl<T: Clone> Component for TableSelect<T> {
    type Output = T;

    fn keymap(&self) -> Keymap {
        self.select.visible_keymap()
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<T> {
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

/// Pick a node of a tree. Nodes are given in order, each with its depth
/// (0 for a root); Right expands the focused node and Left collapses it,
/// or moves to its parent. Typing searches every node, collapsed or not.
pub struct TreeSelect<T> {
    select: Select<T>,
    parents: Vec<Option<usize>>,
    /// Whether each node is the last of its parent's children.
    last: Vec<bool>,
    parent_of_any: Vec<bool>,
    collapsed: Vec<bool>,
}

impl<T> TreeSelect<T> {
    pub fn new<I, V>(prompt: impl Into<String>, nodes: I) -> TreeSelect<T>
    where
        I: IntoIterator<Item = (usize, V)>,
        V: Into<Item<T>>,
    {
        let mut items: Vec<Item<T>> = Vec::new();
        let mut depths: Vec<usize> = Vec::new();
        let mut parents: Vec<Option<usize>> = Vec::new();
        for (depth, node) in nodes {
            // No deeper than one below the node before.
            let depth = depth.min(depths.last().map_or(0, |d| d + 1));
            let parent = (0..items.len()).rev().find(|&i| depths[i] + 1 == depth);
            let parent = if depth == 0 { None } else { parent };
            let mut item: Item<T> = node.into();
            item.label = shown(&item.label);
            items.push(item);
            depths.push(depth);
            parents.push(parent);
        }
        let count = items.len();
        let mut parent_of_any = vec![false; count];
        for parent in parents.iter().flatten() {
            parent_of_any[*parent] = true;
        }
        let last: Vec<bool> = (0..count)
            .map(|i| !(i + 1..count).any(|j| parents[j] == parents[i]))
            .collect();
        let values: Vec<String> = (0..count)
            .map(|i| {
                let mut path = vec![items[i].label.as_str()];
                let mut at = parents[i];
                while let Some(parent) = at {
                    path.push(items[parent].label.as_str());
                    at = parents[parent];
                }
                path.reverse();
                path.join("/")
            })
            .collect();
        let mut select = Select::new(prompt, items);
        select.set_kind(TargetKind::Node);
        select.set_values(values);
        select.hints = Some("←→ fold".into());
        let mut tree = TreeSelect {
            select,
            parents,
            last,
            parent_of_any,
            collapsed: vec![false; count],
        };
        tree.update();
        tree
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

    /// The guides and fold markers, and which nodes a collapse hides.
    fn update(&mut self) {
        let count = self.parents.len();
        let mut prefixes = Vec::with_capacity(count);
        let mut hidden = Vec::with_capacity(count);
        for index in 0..count {
            let mut guides = String::new();
            let mut at = self.parents[index];
            let mut up = Vec::new();
            while let Some(parent) = at {
                up.push(parent);
                at = self.parents[parent];
            }
            hidden.push(up.iter().any(|&ancestor| self.collapsed[ancestor]));
            // Each ancestor below the root draws a bar if more of its
            // siblings follow.
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
            guides.push_str(match (self.parent_of_any[index], self.collapsed[index]) {
                (true, true) => "▸ ",
                (true, false) => "▾ ",
                _ => "",
            });
            prefixes.push(guides);
        }
        self.select.prefixes = prefixes;
        self.select.hidden = hidden;
        self.select.refilter();
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

impl<T: Clone> Component for TreeSelect<T> {
    type Output = T;

    fn keymap(&self) -> Keymap {
        let mut keymap = Keymap::new("tree")
            .bind("expand", keys("right"), "expand")
            .bind("collapse", keys("left"), "collapse, or go to the parent");
        keymap.extend(self.select.visible_keymap());
        keymap
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<T> {
        if let Some(key) = event.key() {
            if !self.select.menu_open() && self.fold(key) {
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
