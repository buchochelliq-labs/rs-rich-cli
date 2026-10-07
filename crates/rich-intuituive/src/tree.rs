//! A tree of items that expand and collapse, with one selected.

use std::cell::Cell;
use std::collections::HashSet;

use rich::Segment;
use rich_interact::{Key, KeyCode, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::{signal, Signal};
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};
use crate::widgets::{cell_line, over};

/// An item of a [`tree`]: a line of markup and the items under it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeItem {
    pub label: String,
    pub children: Vec<TreeItem>,
}

impl TreeItem {
    /// An item with no children.
    pub fn new(label: impl Into<String>) -> TreeItem {
        TreeItem {
            label: label.into(),
            children: Vec::new(),
        }
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

/// A row of the tree as it is shown.
struct Row {
    path: Vec<usize>,
    label: String,
    parent: bool,
}

struct Tree {
    items: Box<dyn Fn() -> Vec<TreeItem>>,
    selected: Signal<Vec<usize>>,
    expanded: Signal<HashSet<Vec<usize>>>,
    /// The first row in view, and the rows last drawn.
    first: Cell<usize>,
    height: Cell<usize>,
}

/// A tree of markup items, one selected. `selected` is the path to the
/// selected item: its index at each level, from the top. Items with
/// children show `▸` while collapsed and `▾` while expanded; each level is
/// indented two cells.
///
/// ↑/↓ (or k/j), PgUp/PgDn and Home/End (g/G) move the selection. → (or l)
/// expands an item, or moves into its first child. ← (or h) collapses it,
/// or moves to its parent. Enter and Space toggle an item with children. A
/// click selects a row, a click on its arrow toggles it, and the wheel
/// moves the selection. The selection is highlighted in the theme's
/// `selected` style while the tree has the focus, and `tree.selected`
/// otherwise. `items` may read signals.
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
    widget(Tree {
        items: Box::new(items),
        selected,
        expanded,
        first: Cell::new(0),
        height: Cell::new(1),
    })
}

/// The rows of `items` in view, given which are expanded.
fn flatten(items: &[TreeItem], expanded: &HashSet<Vec<usize>>) -> Vec<Row> {
    fn walk(
        items: &[TreeItem],
        expanded: &HashSet<Vec<usize>>,
        path: &mut Vec<usize>,
        out: &mut Vec<Row>,
    ) {
        for (i, item) in items.iter().enumerate() {
            path.push(i);
            out.push(Row {
                path: path.clone(),
                label: item.label.clone(),
                parent: !item.children.is_empty(),
            });
            if !item.children.is_empty() && expanded.contains(path) {
                walk(&item.children, expanded, path, out);
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(items, expanded, &mut Vec::new(), &mut out);
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
        self.expanded.with(|expanded| flatten(&items, expanded))
    }

    fn select(&self, row: &Row) {
        self.selected.set(row.path.clone());
    }

    /// Expand or collapse `path`. A selection inside an item it collapses
    /// moves up to the item.
    fn set_expanded(&self, path: &[usize], open: bool) {
        if open {
            self.expanded.update(|set| {
                set.insert(path.to_vec());
            });
        } else {
            self.expanded.update(|set| {
                set.remove(path);
            });
            let inside = self
                .selected
                .with_untracked(|s| s.len() > path.len() && s.starts_with(path));
            if inside {
                self.selected.set(path.to_vec());
            }
        }
    }

    fn is_expanded(&self, path: &[usize]) -> bool {
        self.expanded.with_untracked(|set| set.contains(path))
    }

    fn key(&self, key: &Key) -> Used {
        if key.modifiers.ctrl || key.modifiers.alt {
            return Used::No;
        }
        let rows = self.rows();
        if rows.is_empty() {
            return Used::No;
        }
        let at = self.selected.with_untracked(|s| index_of(&rows, s));
        let row = &rows[at];
        let last = rows.len() - 1;
        let page = self.height.get().max(1);
        let to = |i: usize| self.select(&rows[i.min(last)]);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => to(at.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => to(at + 1),
            KeyCode::PageUp => to(at.saturating_sub(page)),
            KeyCode::PageDown => to(at + page),
            KeyCode::Home | KeyCode::Char('g') => to(0),
            KeyCode::End | KeyCode::Char('G') => to(last),
            KeyCode::Right | KeyCode::Char('l') if row.parent => {
                if self.is_expanded(&row.path) {
                    let mut child = row.path.clone();
                    child.push(0);
                    self.selected.set(child);
                } else {
                    self.set_expanded(&row.path, true);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if row.parent && self.is_expanded(&row.path) {
                    self.set_expanded(&row.path, false);
                } else if row.path.len() > 1 {
                    self.selected.set(row.path[..row.path.len() - 1].to_vec());
                } else {
                    return Used::No;
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') if row.parent => {
                let open = !self.is_expanded(&row.path);
                self.set_expanded(&row.path, open);
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

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => self.rows().len().min(u16::MAX as usize) as u16,
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height() as usize);
        self.height.set(height.max(1));
        let rows = self.rows();
        let selected = self.selected.with(|s| index_of(&rows, s));
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
        for (i, row) in rows.iter().enumerate().skip(first).take(height) {
            let marker = match (row.parent, self.is_expanded(&row.path)) {
                (false, _) => "  ",
                (true, true) => "▾ ",
                (true, false) => "▸ ",
            };
            let prefix = format!("{}{marker}", "  ".repeat(row.path.len() - 1));
            let used = rich::cells::cell_len(&prefix).min(width as usize) as u16;
            let mut line = vec![Segment::new(prefix, None)];
            line.extend(cell_line(console, &row.label, width - used));
            if i == selected {
                line = over(line, &highlight);
            }
            canvas.lines_at(0, (i - first) as u16, width, 1, &[line]);
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Key(key) => self.key(key),
            WidgetEvent::Mouse(mouse) => {
                let rows = self.rows();
                match mouse.kind {
                    MouseKind::Down(_) => {
                        let Some(row) = rows.get(self.first.get() + mouse.row as usize) else {
                            return Used::No;
                        };
                        let arrow = 2 * (row.path.len() as u16 - 1);
                        if row.parent && (arrow..arrow + 2).contains(&mouse.column) {
                            let open = !self.is_expanded(&row.path);
                            self.set_expanded(&row.path, open);
                        }
                        // Collapsing may have moved the selection to the
                        // item already.
                        self.select(row);
                    }
                    MouseKind::ScrollUp | MouseKind::ScrollDown if !rows.is_empty() => {
                        let at = self.selected.with_untracked(|s| index_of(&rows, s));
                        let at = if mouse.kind == MouseKind::ScrollUp {
                            at.saturating_sub(3)
                        } else {
                            (at + 3).min(rows.len() - 1)
                        };
                        self.select(&rows[at]);
                    }
                    _ => return Used::No,
                }
                Used::Yes
            }
        }
    }

    fn focusable(&self) -> bool {
        true
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
            flatten(&items, &expanded)
                .into_iter()
                .map(|r| r.path)
                .collect()
        };
        assert_eq!(paths(HashSet::new()), [vec![0], vec![1]]);
        assert_eq!(
            paths(HashSet::from([vec![0]])),
            [vec![0], vec![0, 0], vec![0, 1], vec![1]]
        );
        let rows = flatten(&items, &HashSet::new());
        // A hidden selection shows on its nearest shown ancestor.
        assert_eq!(index_of(&rows, &[0, 1]), 0);
        assert_eq!(index_of(&rows, &[1]), 1);
        assert_eq!(index_of(&rows, &[7]), 0);
    }
}
