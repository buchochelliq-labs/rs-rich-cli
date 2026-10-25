//! A tree table ([`tree_table`]): a [table](crate::widgets::table_with)
//! whose rows nest. A process tree with its columns (bottom's, htop's), a
//! project list with counts (taskwarrior's `projects`), files with sizes.

use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use crate::node::Node;
use crate::reactive::{memo, signal, watch, Signal};
use crate::widgets::{table_with, Column, TableOptions};

/// One row of a [`tree_table`]: what identifies it (`key`), its cells
/// (console markup, one a column), and the rows under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow<K> {
    pub key: K,
    pub cells: Vec<String>,
    pub children: Vec<TreeRow<K>>,
}

impl<K> TreeRow<K> {
    /// A row with no children.
    pub fn new(key: K, cells: impl IntoIterator<Item = impl Into<String>>) -> TreeRow<K> {
        TreeRow {
            key,
            cells: cells.into_iter().map(Into::into).collect(),
            children: Vec::new(),
        }
    }

    /// The rows under it.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = TreeRow<K>>) -> TreeRow<K> {
        self.children = children.into_iter().collect();
        self
    }
}

/// How a [`tree_table`] draws and takes its tree: which column carries
/// it, its guides and markers, and the table's own options.
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct TreeTableOptions {
    table: TableOptions,
    column: usize,
    guides: [&'static str; 4],
    open: String,
    closed: String,
}

impl Default for TreeTableOptions {
    fn default() -> TreeTableOptions {
        TreeTableOptions {
            table: TableOptions::default(),
            column: 0,
            guides: ["├─ ", "└─ ", "│  ", "   "],
            open: String::new(),
            closed: "+ ".to_string(),
        }
    }
}

impl TreeTableOptions {
    pub fn new() -> TreeTableOptions {
        TreeTableOptions::default()
    }

    /// The table's options: column sizes from state, priorities, a footer,
    /// a highlight symbol, sorting shown in the header (the rows closure
    /// sorts each level itself: `sort_rows` would mix the levels up).
    pub fn table(mut self, options: TableOptions) -> TreeTableOptions {
        self.table = options;
        self
    }

    /// The column whose cells carry the tree's guides and markers; the
    /// first by default.
    pub fn tree_column(mut self, column: usize) -> TreeTableOptions {
        self.column = column;
        self
    }

    /// The guides before a row under another: a middle child's, the last
    /// child's, and what goes under a middle child and under the last
    /// (`├─ `, `└─ `, `│  `, `   ` by default; in text mode as well).
    pub fn guides(
        mut self,
        middle: &'static str,
        last: &'static str,
        down: &'static str,
        blank: &'static str,
    ) -> TreeTableOptions {
        self.guides = [middle, last, down, blank];
        self
    }

    /// What goes before an open row with children and a closed one
    /// (console markup): none and `+ ` by default, bottom's look;
    /// `("▾ ", "▸ ")` for a file tree's.
    pub fn markers(mut self, open: &str, closed: &str) -> TreeTableOptions {
        self.open = open.to_string();
        self.closed = closed.to_string();
        self
    }
}

/// A row of a tree as a [`tree_table`] shows it, from
/// [`tree_table_rows`]: its key, its parent's, how deep it is, whether it
/// has children and shows them, and its cells with the guides and the
/// marker in the tree column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownRow<K> {
    pub key: K,
    pub parent: Option<K>,
    /// 0 for a row at the top level.
    pub depth: usize,
    /// It has children, and they are shown.
    pub parent_of: bool,
    pub open: bool,
    pub cells: Vec<String>,
}

/// The rows a [`tree_table`] shows, in order, for a table of your own (an
/// index-based selection, rows with more than their cells): each row,
/// then, unless `closed` says it is, its children, with the guides and the
/// markers `options` says in its tree column.
///
/// ```
/// use intuituive::widgets::{tree_table_rows, TreeRow, TreeTableOptions};
///
/// let rows = [TreeRow::new("a", ["a"]).children([TreeRow::new("b", ["b"])])];
/// let shown = tree_table_rows(&rows, |_| false, &TreeTableOptions::new());
/// assert_eq!(shown[1].cells, ["└─ b"]);
/// let shut = tree_table_rows(&rows, |key| *key == "a", &TreeTableOptions::new());
/// assert_eq!(shut.len(), 1);
/// assert_eq!(shut[0].cells, ["+ a"]);
/// ```
pub fn tree_table_rows<K: Clone>(
    rows: &[TreeRow<K>],
    closed: impl Fn(&K) -> bool,
    options: &TreeTableOptions,
) -> Vec<ShownRow<K>> {
    let look = Look {
        column: options.column,
        guides: options.guides,
        open: options.open.clone(),
        closed: options.closed.clone(),
    };
    flatten(rows, &closed, &look)
}

/// The rows shown, in order: each row, then (while it is open) its
/// children, with guides and markers in the tree column.
fn flatten<K: Clone>(
    rows: &[TreeRow<K>],
    closed: &dyn Fn(&K) -> bool,
    look: &Look,
) -> Vec<ShownRow<K>> {
    struct Walk<'a, K> {
        out: Vec<ShownRow<K>>,
        guides: Vec<&'static str>,
        closed: &'a dyn Fn(&K) -> bool,
        look: &'a Look,
    }
    impl<K: Clone> Walk<'_, K> {
        fn walk(&mut self, row: &TreeRow<K>, parent: Option<&K>, last: bool, depth: usize) {
            let look = self.look;
            let parent_of = !row.children.is_empty();
            let open = parent_of && !(self.closed)(&row.key);
            let mut prefix: String = self.guides.concat();
            if parent.is_some() {
                prefix.push_str(if last { look.guides[1] } else { look.guides[0] });
            }
            prefix.push_str(match (parent_of, open) {
                (true, true) => &look.open,
                (true, false) => &look.closed,
                (false, _) => "",
            });
            let mut cells = row.cells.clone();
            if let Some(cell) = cells.get_mut(look.column) {
                *cell = format!("{prefix}{cell}");
            }
            self.out.push(ShownRow {
                key: row.key.clone(),
                parent: parent.cloned(),
                depth,
                parent_of,
                open,
                cells,
            });
            if !open {
                return;
            }
            if parent.is_some() {
                self.guides
                    .push(if last { look.guides[3] } else { look.guides[2] });
            }
            let count = row.children.len();
            for (i, child) in row.children.iter().enumerate() {
                self.walk(child, Some(&row.key), i + 1 == count, depth + 1);
            }
            if parent.is_some() {
                self.guides.pop();
            }
        }
    }
    let mut walk = Walk {
        out: Vec::new(),
        guides: Vec::new(),
        closed,
        look,
    };
    let count = rows.len();
    for (i, row) in rows.iter().enumerate() {
        walk.walk(row, None, i + 1 == count, 0);
    }
    walk.out
}

/// What [`flatten`] draws with.
struct Look {
    column: usize,
    guides: [&'static str; 4],
    open: String,
    closed: String,
}

/// A table whose rows nest: `rows` (which may read signals) are the top
/// level, each with its children, drawn in the table's `columns` with
/// guides and markers in the tree column (`├─ `, `└─ `, `+ ` for a closed
/// row). The selection and the closed rows are kept by key: `selected`
/// holds the selected row's key, and `collapsed` the keys of the rows
/// whose children are hidden (every row with children is open until it is
/// closed, as in a process tree). When the rows change (a re-sort, a
/// process gone) the selection stays on its row; a row that is gone
/// leaves the cursor at its place, on the row now there.
///
/// The table's keys move the selection; Space opens or closes the
/// selected row, `+` (or `=`) opens it and `-` closes it, ← closes an open
/// row or goes to the parent, → opens a closed row or goes to its first
/// child. Call it while an app or a screen is built: it keeps the table's
/// place and the key in step with [`watch`]es.
///
/// ```
/// use std::collections::HashSet;
///
/// use intuituive::prelude::*;
/// use intuituive::widgets::{tree_table, Column, TreeRow, TreeTableOptions};
///
/// let app = App::new(|| {
///     let rows = || {
///         vec![TreeRow::new(1, ["init", "1"]).children([
///             TreeRow::new(2, ["sshd", "2"]).children([TreeRow::new(4, ["bash", "4"])]),
///             TreeRow::new(3, ["cron", "3"]),
///         ])]
///     };
///     let columns = vec![Column::new("Name", Size::Auto), Column::new("PID", Size::Auto)];
///     tree_table(columns, rows, signal(None), signal(HashSet::new()), TreeTableOptions::new())
/// });
/// let mut driver = app.driver(20, 5);
/// driver.settle();
/// let rows = driver.screen().plain();
/// assert_eq!(rows[1].trim_end(), "init       1");
/// assert_eq!(rows[2].trim_end(), "├─ sshd    2");
/// assert_eq!(rows[3].trim_end(), "│  └─ bash 4");
/// assert_eq!(rows[4].trim_end(), "└─ cron    3");
/// // Down to sshd, and close it.
/// driver.press("down space");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "├─ + sshd 2");
/// ```
pub fn tree_table<K: Clone + Eq + Hash + 'static>(
    columns: Vec<Column>,
    rows: impl Fn() -> Vec<TreeRow<K>> + 'static,
    selected: Signal<Option<K>>,
    collapsed: Signal<HashSet<K>>,
    options: TreeTableOptions,
) -> Node {
    let TreeTableOptions {
        table,
        column,
        guides,
        open,
        closed,
    } = options;
    let look = Look {
        column,
        guides,
        open,
        closed,
    };
    let flat = memo(move || collapsed.with(|shut| flatten(&rows(), &|k| shut.contains(k), &look)));
    let index = signal(0usize);
    // The key follows the cursor, and the cursor the key: through a change
    // of the rows, or a key set from outside.
    watch(
        move || {
            let keys: Vec<K> = flat.with(|rows| rows.iter().map(|r| r.key.clone()).collect());
            (keys, selected.get())
        },
        move |(keys, key), _| {
            if keys.is_empty() {
                return;
            }
            match key.and_then(|key| keys.iter().position(|k| *k == key)) {
                Some(at) => index.set(at),
                None => {
                    let at = index.get_untracked().min(keys.len() - 1);
                    index.set(at);
                    selected.set(Some(keys[at].clone()));
                }
            }
        },
    );
    watch(
        move || index.get(),
        move |at, _| {
            let key = flat.with_untracked(|rows| rows.get(at).map(|r| r.key.clone()));
            if key.is_some() && selected.with_untracked(|now| *now != key) {
                selected.set(key);
            }
        },
    );
    let at = Rc::new(move || {
        let key = selected.get_untracked()?;
        flat.with_untracked(|rows| rows.iter().find(|r| r.key == key).cloned())
    });
    let set_open = move |key: K, open: bool| {
        collapsed.update(|shut| {
            if open {
                shut.remove(&key);
            } else {
                shut.insert(key);
            }
        })
    };
    let (toggle, open_it, close_it, left, right) =
        (at.clone(), at.clone(), at.clone(), at.clone(), at);
    table_with(
        columns,
        move || flat.with(|rows| rows.iter().map(|r| r.cells.clone()).collect()),
        index,
        table,
    )
    .bind("space", "Open or close", move |_| {
        if let Some(row) = toggle().filter(|r| r.parent_of) {
            set_open(row.key, !row.open);
        }
    })
    .bind("plus equals", "Open", move |_| {
        if let Some(row) = open_it().filter(|r| r.parent_of) {
            set_open(row.key, true);
        }
    })
    .bind("minus", "Close", move |_| {
        if let Some(row) = close_it().filter(|r| r.parent_of) {
            set_open(row.key, false);
        }
    })
    .bind("left", "Close, or go to the parent", move |_| {
        let Some(row) = left() else { return };
        match (row.open, row.parent) {
            (true, _) => set_open(row.key, false),
            (false, Some(parent)) => selected.set(Some(parent)),
            (false, None) => {}
        }
    })
    .bind("right", "Open, or go to the first child", move |_| {
        let Some(row) = right().filter(|r| r.parent_of) else {
            return;
        };
        match row.open {
            false => set_open(row.key, true),
            true => {
                let child = flat.with_untracked(|rows| {
                    rows.iter()
                        .find(|r| r.parent.as_ref() == Some(&row.key))
                        .map(|r| r.key.clone())
                });
                if child.is_some() {
                    selected.set(child);
                }
            }
        }
    })
    .what("tree table")
}
