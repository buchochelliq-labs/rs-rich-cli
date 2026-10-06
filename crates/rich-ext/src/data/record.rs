//! [`RecordView`]: one record as a table of fields, with nested values
//! folded into drill-down trees.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use rich::measure::Measurement;
use rich::table::Cell;
use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Table, Text};

use super::table::cell_text;
use super::{escape_controls, quote_str, style, summary, Explorer, Node, Path, PathSegment};
use super::{Redactor, Value};

/// A view that opens and closes branches of a document by path: the hook
/// an interactive explorer drives to drill into a nested value.
///
/// [`RecordView`] implements it, and so does `rich_interact`'s
/// `DataExplorer`, so a caller that tracks "the branch the user opened" can
/// apply it to either.
pub trait OpenBranch {
    /// Open the container at `path` and every container above it, so its
    /// children show. Returns whether `path` names a container.
    fn open_branch(&mut self, path: &Path) -> bool;

    /// Fold the container at `path` to its summary. Returns whether `path`
    /// names a container.
    fn close_branch(&mut self, path: &Path) -> bool;
}

/// One record shown as a table of fields: `field | type | value`.
///
/// A map's entries (or a sequence's items) are the rows. Scalars show as
/// they are, strings unquoted and cut to [`max_string`](Self::max_string)
/// characters with the full length in a dim `(N chars)`. A nested map or
/// sequence shows as a tree opened [`depth`](Self::depth) levels deep, with
/// deeper containers folded to `{…} 3 keys` and long containers cut to
/// [`max_items`](Self::max_items) children and `… N more`.
/// [`expand`](Self::expand) opens one branch by path whatever the depth,
/// [`collapse`](Self::collapse) folds one, and [`branch`](Self::branch)
/// gives the tree of any branch on its own, for a pane that drills in.
///
/// Redact before display with [`redact`](Self::redact): masking applies to
/// the whole record, so a secret never reaches a cell or a tree.
///
/// ```
/// use rich::Console;
/// use rich_ext::data::{from_serialize, RecordView};
/// use serde_json::json;
///
/// let record = from_serialize(&json!({
///     "id": 7,
///     "name": "web",
///     "ports": [80, 443],
///     "owner": {"team": "infra", "oncall": {"primary": "ana"}}
/// }))
/// .unwrap();
/// let view = RecordView::new(&record).expand("owner.oncall".parse().unwrap());
/// let out = Console::builder().width(44).build().render_export(&view);
/// assert!(out.contains("│ id    │ int  │ 7"), "{out}");
/// assert!(out.contains("primary: \"ana\""), "{out}");
/// ```
#[derive(Clone, Debug)]
pub struct RecordView<'a> {
    node: Cow<'a, Node>,
    depth: usize,
    expanded: HashSet<Path>,
    collapsed: HashSet<Path>,
    max_items: Option<usize>,
    max_string: Option<usize>,
    show_types: bool,
    title: Option<String>,
}

impl<'a> RecordView<'a> {
    /// View an owned or borrowed record.
    pub fn new(node: impl Into<Cow<'a, Node>>) -> Self {
        RecordView {
            node: node.into(),
            depth: 1,
            expanded: HashSet::new(),
            collapsed: HashSet::new(),
            max_items: Some(20),
            max_string: Some(200),
            show_types: true,
            title: None,
        }
    }

    /// Open nested values this many levels (default 1: a nested value's own
    /// children show, folded). `0` folds every nested value to its summary.
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Open the branch at `path` (relative to the record, such as
    /// `owner.oncall` or `ports[2]`) and the containers above it, however
    /// deep it is.
    pub fn expand(mut self, path: Path) -> Self {
        self.open_branch(&path);
        self
    }

    /// Fold the branch at `path`, even within [`depth`](Self::depth).
    pub fn collapse(mut self, path: Path) -> Self {
        self.close_branch(&path);
        self
    }

    /// Show at most this many children of a nested container, then
    /// `… N more` (default 20). `None` shows them all.
    pub fn max_items(mut self, items: impl Into<Option<usize>>) -> Self {
        self.max_items = items.into();
        self
    }

    /// Cut strings to this many characters (default 200), with the full
    /// length shown beside a cut field. `None` keeps them whole.
    pub fn max_string(mut self, length: impl Into<Option<usize>>) -> Self {
        self.max_string = length.into();
        self
    }

    /// Show the `type` column (default on).
    pub fn show_types(mut self, show: bool) -> Self {
        self.show_types = show;
        self
    }

    /// A title above the table (plain text, not markup).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Mask the record through `redactor` (such as
    /// [`Redaction::secrets`](super::Redaction::secrets)) before display.
    pub fn redact(mut self, redactor: &dyn Redactor) -> Self {
        self.node = Cow::Owned(self.node.redacted(redactor));
        self
    }

    /// The record being shown.
    pub fn node(&self) -> &Node {
        &self.node
    }

    /// The paths of the record's fields that hold a non-empty container:
    /// the branches there are to open.
    pub fn branches(&self) -> Vec<Path> {
        let root = Path::root();
        fields(&self.node)
            .into_iter()
            .filter(|(_, node)| node.is_container() && !node.is_empty())
            .map(|(segment, _)| child(&root, segment))
            .collect()
    }

    /// Whether the container at `path` shows its children.
    pub fn is_open(&self, path: &Path) -> bool {
        if path.is_root() {
            return true;
        }
        if self.collapsed.contains(path) {
            return false;
        }
        // The record's fields are depth 0; a field's children depth 1.
        path.len() <= self.depth
            || self
                .expanded
                .iter()
                .any(|open| open.segments().starts_with(path.segments()))
    }

    /// The tree for the branch at `path`, folded as this view folds it: what
    /// a drill-down pane shows when the branch is opened. `None` when `path`
    /// is not a container in the record.
    pub fn branch(&self, path: &Path) -> Option<Explorer<'static>> {
        let node = self.node.at(path)?;
        node.is_container().then(|| self.tree(path, node))
    }

    /// The tree of the container `node` at `path`, with the folds this
    /// view's settings call for. Only open containers are walked, and only
    /// the children that will show.
    fn tree(&self, path: &Path, node: &Node) -> Explorer<'static> {
        let limit = self.max_items.unwrap_or(usize::MAX);
        let mut explorer = Explorer::new(node.clone());
        if let Some(items) = self.max_items {
            explorer = explorer.max_length(items);
        }
        if let Some(length) = self.max_string {
            explorer = explorer.max_string(length);
        }
        let mut stack: Vec<(Path, Path, &Node)> = vec![(path.clone(), Path::root(), node)];
        while let Some((absolute, relative, node)) = stack.pop() {
            if !node.is_container() || node.is_empty() {
                continue;
            }
            if !self.is_open(&absolute) && !absolute.is_root() {
                explorer = explorer.fold(relative);
                continue;
            }
            for (segment, child_node) in fields(node).into_iter().take(limit) {
                stack.push((
                    child(&absolute, segment.clone()),
                    child(&relative, segment),
                    child_node,
                ));
            }
        }
        explorer
    }

    /// The core table.
    pub fn to_table(&self, console: &Console) -> Table {
        let mut table = Table::new();
        if let Some(title) = &self.title {
            table = table.title(rich::markup::escape(title));
        }
        table.add_column_text(Text::new("field"), Justify::Left);
        if self.show_types {
            table.add_column_text(Text::new("type"), Justify::Left);
        }
        table.add_column_text(Text::new("value"), Justify::Left);

        let type_cell =
            |node: &Node| Cell::Text(Text::styled(node.type_name(), style(console, "data.type")));
        if !self.node.is_container() {
            let mut row = vec![Cell::Text(Text::styled(
                "(value)",
                style(console, "data.summary"),
            ))];
            if self.show_types {
                row.push(type_cell(&self.node));
            }
            row.push(Cell::Text(self.scalar(console, &self.node)));
            table.add_row_cells(row);
            return table;
        }

        let root = Path::root();
        let entries = fields(&self.node);
        let total = entries.len();
        // A record that is a sequence is cut like any other; a map's fields
        // are the record and all show.
        let shown = match &self.node.value {
            Value::Seq(_) => self.max_items.unwrap_or(usize::MAX).min(total),
            _ => total,
        };
        for (segment, node) in entries.into_iter().take(shown) {
            let name = match &segment {
                PathSegment::Key(key) => {
                    Text::styled(escape_controls(key), style(console, "json.key"))
                }
                PathSegment::Index(index) => {
                    Text::styled(format!("[{index}]"), style(console, "data.index"))
                }
            };
            let path = child(&root, segment);
            let value = if node.is_container() && !node.is_empty() && self.is_open(&path) {
                Cell::Renderable(Arc::new(self.tree(&path, node)))
            } else if node.is_container() {
                Cell::Text(Text::styled(summary(node), style(console, "data.summary")))
            } else {
                Cell::Text(self.scalar(console, node))
            };
            let mut row = vec![Cell::Text(name)];
            if self.show_types {
                row.push(type_cell(node));
            }
            row.push(value);
            table.add_row_cells(row);
        }
        if shown < total {
            table = table.caption(format!("… {} more", total - shown));
        }
        table
    }

    /// A scalar cell: strings unquoted (controls escaped) and cut with their
    /// length beside them; other scalars as the table view shows them.
    fn scalar(&self, console: &Console, node: &Node) -> Text {
        let Value::String(s) = &node.value else {
            return cell_text(console, node, None);
        };
        let count = s.chars().count();
        match self.max_string.filter(|&max| count > max) {
            Some(max) => {
                let cut: String = s.chars().take(max).collect();
                let mut text = Text::new(format!("{}…", escape_controls(&cut)));
                text.append(
                    &format!(" ({count} chars)"),
                    Some(style(console, "data.summary").into()),
                );
                text
            }
            // A string of spaces would vanish: quote it so it shows.
            None if !s.is_empty() && s.trim().is_empty() => {
                Text::styled(quote_str(s), style(console, "json.str"))
            }
            None => Text::new(escape_controls(s)),
        }
    }
}

impl OpenBranch for RecordView<'_> {
    fn open_branch(&mut self, path: &Path) -> bool {
        let container = self.node.at(path).is_some_and(Node::is_container);
        if container {
            // Opening a branch reopens anything folded on the way to it.
            let mut at = Some(path.clone());
            while let Some(p) = at {
                self.collapsed.remove(&p);
                at = p.parent();
            }
            self.expanded.insert(path.clone());
        }
        container
    }

    fn close_branch(&mut self, path: &Path) -> bool {
        let container = self.node.at(path).is_some_and(Node::is_container);
        if container {
            self.expanded
                .retain(|open| !open.segments().starts_with(path.segments()));
            self.collapsed.insert(path.clone());
        }
        container
    }
}

/// `path` plus one segment.
fn child(path: &Path, segment: PathSegment) -> Path {
    let mut path = path.clone();
    path.push(segment);
    path
}

/// A container's children with the segment that reaches each.
fn fields(node: &Node) -> Vec<(PathSegment, &Node)> {
    match &node.value {
        Value::Map(entries) => entries
            .iter()
            .map(|(key, value)| (PathSegment::Key(key.clone()), value))
            .collect(),
        Value::Seq(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| (PathSegment::Index(index), item))
            .collect(),
        _ => Vec::new(),
    }
}

impl Renderable for RecordView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.to_table(console).rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.to_table(console).measure(console, options)
    }
}
