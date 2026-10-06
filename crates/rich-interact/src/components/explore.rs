//! An interactive explorer for structured data (#465): a JSON, YAML, TOML,
//! XML, INI or dotenv document, parsed by `rich_ext::data`, as a
//! [`TreeSelect`] you fold, search and copy from, with the focused node's
//! subtree drawn by [`rich_ext::data::Explorer`] in the preview pane.
//!
//! Needs the `data` feature.

use std::sync::Arc;
use std::time::Duration;

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment};
use rich_ext::data::{
    copy_text, display_value, Explorer, Node, OpenBranch, Path, PathSegment, Value,
};

use crate::clipboard;
use crate::component::{Component, Context, Flow, View};
use crate::components::{PreviewLayout, Theme, TreeSelect};
use crate::event::{Event, Key};
use crate::item::{Item, Preview};
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};

/// The keys a [`DataExplorer`] adds to its tree's, in context `explore`.
pub fn explore_keymap() -> Keymap {
    Keymap::new("explore").bind("copy-value", keys("alt+y"), "copy the value")
}

/// `path` as a JSONPath expression, as `--select` takes it: `$` for the
/// root, `$.servers[0].name`, `$["odd key"]`.
///
/// ```
/// use rich_ext::data::Path;
/// use rich_interact::components::json_path;
///
/// assert_eq!(json_path(&Path::root()), "$");
/// assert_eq!(json_path(&Path::root().child_key("a").child_index(0)), "$.a[0]");
/// assert_eq!(json_path(&Path::root().child_key("odd key")), "$[\"odd key\"]");
/// ```
pub fn json_path(path: &Path) -> String {
    let shown = path.to_string();
    if shown.is_empty() {
        "$".into()
    } else if shown.starts_with('[') {
        format!("${shown}")
    } else {
        format!("$.{shown}")
    }
}

/// Where each node of a document sits, in the order a [`DataExplorer`]
/// lists them: its parent and its position among the parent's children.
/// Paths, values and previews are worked out from it when they are needed,
/// so what the explorer keeps per node does not grow with the depth.
struct Shape {
    root: Arc<Node>,
    parents: Vec<Option<usize>>,
    positions: Vec<usize>,
}

impl Shape {
    /// The positions from the root down to node `index`.
    fn route(&self, index: usize) -> Vec<usize> {
        let mut route = vec![self.positions[index]];
        let mut at = self.parents[index];
        while let Some(parent) = at {
            route.push(self.positions[parent]);
            at = self.parents[parent];
        }
        // The root's own position is not a step.
        route.pop();
        route.reverse();
        route
    }

    /// Node `index` and its path.
    fn node(&self, index: usize) -> (Path, &Node) {
        let mut node: &Node = &self.root;
        let mut path = Path::root();
        for position in self.route(index) {
            let (segment, child) = match &node.value {
                Value::Seq(items) => (PathSegment::Index(position), &items[position]),
                Value::Map(entries) => {
                    let (key, child) = &entries[position];
                    (PathSegment::Key(key.clone()), child)
                }
                _ => unreachable!("a route only steps into containers"),
            };
            path.push(segment);
            node = child;
        }
        (path, node)
    }

    fn path(&self, index: usize) -> Path {
        self.node(index).0
    }

    /// The index of the node at `path`, stepping down from the root. A
    /// node's children follow it in the list, so each step scans forward
    /// from its parent.
    fn index_of(&self, path: &Path) -> Option<usize> {
        let mut index = 0;
        let mut node: &Node = &self.root;
        for segment in path.segments() {
            let (position, child) = match (&node.value, segment) {
                (Value::Seq(items), PathSegment::Index(i)) => (*i, items.get(*i)?),
                (Value::Map(entries), PathSegment::Key(key)) => entries
                    .iter()
                    .enumerate()
                    .find(|(_, (k, _))| k == key)
                    .map(|(i, (_, child))| (i, child))?,
                _ => return None,
            };
            index = (index + 1..self.parents.len())
                .find(|&j| self.parents[j] == Some(index) && self.positions[j] == position)?;
            node = child;
        }
        Some(index)
    }
}

/// A node's name: `$` for the root, its key, or `[index]`.
fn name(segment: Option<&PathSegment>) -> String {
    match segment {
        None => "$".to_string(),
        Some(PathSegment::Key(key)) => key.clone(),
        Some(PathSegment::Index(index)) => format!("[{index}]"),
    }
}

/// Every node under `root`, parents before children in document order:
/// `visit(path, position, node)`, `position` being its place among its
/// parent's children. One path is kept and changed as the walk goes, so
/// the walk's memory is the depth, not the width times the depth.
fn walk<'a>(root: &'a Node, mut visit: impl FnMut(&[PathSegment], usize, &'a Node)) {
    visit(&[], 0, root);
    let mut segments: Vec<PathSegment> = Vec::new();
    // Each open container and the position of its next child.
    let mut stack: Vec<(&'a Node, usize)> = vec![(root, 0)];
    while let Some((node, next)) = stack.last_mut() {
        let position = *next;
        let child = match &node.value {
            Value::Seq(items) => items
                .get(position)
                .map(|child| (PathSegment::Index(position), child)),
            Value::Map(entries) => entries
                .get(position)
                .map(|(key, child)| (PathSegment::Key(key.clone()), child)),
            _ => None,
        };
        let Some((segment, child)) = child else {
            stack.pop();
            segments.pop();
            continue;
        };
        *next += 1;
        segments.push(segment);
        visit(&segments, position, child);
        stack.push((child, 0));
    }
}

/// The subtree of a node, drawn by [`Explorer`] when the preview renders.
/// Holding the document's shape and an index, not a copy or a path, keeps
/// a large document's previews cheap.
struct Subtree {
    shape: Arc<Shape>,
    index: usize,
    depth: usize,
}

impl Subtree {
    /// The subtree, with at most `rows` children of each container: the
    /// pane is never taller than the terminal, so what it shows is the
    /// same, and a container with a million children costs a screenful.
    fn view(&self, rows: usize) -> Explorer<'_> {
        let (path, node) = self.shape.node(self.index);
        Explorer::new(node)
            .max_depth(self.depth)
            .max_length(rows.max(1))
            .root_label(name(path.last()))
    }
}

impl Renderable for Subtree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.view(options.max_height).rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.view(options.max_height).measure(console, options)
    }
}

/// Explore a document: every node is a row of a tree, a container folds
/// with Left and Right, typing searches keys and values (keeping each
/// match's ancestors), and the line under the question shows where the
/// focused node is. The preview pane draws its subtree.
///
/// Ctrl+Y copies the focused node's path as JSONPath (`$.servers[0].name`)
/// and Alt+Y its value (a string as it is, a container as JSON) to the
/// terminal's [clipboard](crate::clipboard). Enter returns the node's
/// [`Path`]; Escape cancels.
///
/// ```
/// use rich_ext::data::{parse, Format};
/// use rich_interact::components::{json_path, DataExplorer};
/// use rich_interact::headless::{self, Script};
///
/// let node = parse(Format::Json, r#"{"server": {"port": 8080}, "debug": true}"#).unwrap();
/// let explorer = DataExplorer::new("config.json", node);
/// // Down to `server`, open it, down to `port`, copy its path, pick it.
/// let script = Script::new().keys("down right down ctrl+y enter");
/// let (outcome, record) = headless::run(explorer, script, 60, 12);
/// let path = outcome.unwrap().value().unwrap();
/// assert_eq!(json_path(&path), "$.server.port");
/// assert_eq!(record.copies, ["$.server.port"]);
/// ```
pub struct DataExplorer {
    tree: TreeSelect<usize>,
    shape: Arc<Shape>,
    keymap: Keymap,
}

impl DataExplorer {
    /// Explore `node` under `prompt` (a file name, say). Only the root's
    /// children show at first.
    ///
    /// Building it is linear in the number of nodes: a node's path, its
    /// JSONPath and its preview are worked out from its place in the
    /// document when they are needed.
    pub fn new(prompt: impl Into<String>, node: Node) -> DataExplorer {
        let root = Arc::new(node);
        let mut nodes: Vec<(usize, Item<usize>)> = Vec::new();
        let mut crumbs = Vec::new();
        let mut parents = Vec::new();
        let mut positions = Vec::new();
        // The index of the last node seen at each depth.
        let mut open: Vec<usize> = Vec::new();
        walk(&root, |segments, position, node| {
            let depth = segments.len();
            let name = name(segments.last());
            let value = display_value(node);
            let label = if value.is_empty() {
                name.clone()
            } else if depth == 0 {
                format!("{name} {value}")
            } else {
                format!("{name}: {value}")
            };
            let index = nodes.len();
            open.truncate(depth);
            parents.push(open.last().copied());
            open.push(index);
            positions.push(position);
            nodes.push((depth, Item::new(index, label)));
            crumbs.push(name);
        });
        let shape = Arc::new(Shape {
            root,
            parents,
            positions,
        });
        for (index, (_, item)) in nodes.iter_mut().enumerate() {
            item.preview = Some(Preview::Renderable(Arc::new(Subtree {
                shape: Arc::clone(&shape),
                index,
                depth: 3,
            })));
        }
        let paths = Arc::clone(&shape);
        let mut tree = TreeSelect::new(prompt, nodes)
            .crumbs(crumbs)
            .paths_with(move |index| json_path(&paths.path(index)))
            .breadcrumbs(true)
            .fold_below(1);
        tree.set_hints(Some("←→ fold · ctrl+y path · alt+y value".into()));
        DataExplorer {
            tree,
            shape,
            keymap: explore_keymap(),
        }
    }

    /// An icon before each node: `icon(path, node)` returns one line of
    /// text (a micro asset's placeholder, say: a folder for a map, a list
    /// for an array) or `None`. Micro asset placeholders keep their tags,
    /// so the painter's graphics draw them where the terminal can.
    pub fn icons(mut self, icon: impl Fn(&Path, &Node) -> Option<rich::Text>) -> Self {
        let mut icons = Vec::new();
        walk(&self.shape.root, |segments, _, node| {
            icons.push(icon(&Path::from(segments.to_vec()), node));
        });
        self.tree.set_icons(icons);
        self
    }

    /// Show at most `rows` rows at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.tree = self.tree.height(rows);
        self
    }

    /// Where the subtree preview goes (default: beside the tree from 72
    /// columns, below it when narrower).
    pub fn preview(mut self, layout: PreviewLayout) -> Self {
        self.tree = self.tree.preview(layout);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.tree = self.tree.theme(theme);
        self
    }

    /// Start with this search typed.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.tree = self.tree.query(query);
        self
    }

    /// Fold every container at `depth` or deeper (default 1: the root's
    /// children show, folded).
    pub fn fold_below(mut self, depth: usize) -> Self {
        self.tree = self.tree.fold_below(depth);
        self
    }

    /// Report the mouse: see [`Select::with_mouse`](crate::Select::with_mouse).
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.tree = self.tree.with_mouse(on);
        self
    }

    /// Make `keys` do `action` (see [`explore_keymap`]) on this explorer.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    /// The document.
    pub fn document(&self) -> &Node {
        &self.shape.root
    }

    /// The focused node's path.
    pub fn focused(&self) -> Option<Path> {
        self.tree.focused().map(|index| self.shape.path(index))
    }

    /// The tree underneath.
    pub fn tree(&self) -> &TreeSelect<usize> {
        &self.tree
    }

    /// Alt+Y: copy the focused node's value.
    fn copy_value(&mut self) {
        let Some(index) = self.tree.focused() else {
            return;
        };
        let result = clipboard::copy(copy_text(self.shape.node(index).1));
        self.tree
            .set_status(Some(clipboard::report("value", &result)));
    }
}

/// The drill-down hook shared with [`rich_ext::data::RecordView`]: open or
/// fold a branch by its path, as if the user had pressed Right or Left on
/// it. Opening a branch opens every container above it too.
///
/// ```
/// use rich_ext::data::{parse, Format, OpenBranch};
/// use rich_interact::components::DataExplorer;
///
/// let node = parse(Format::Json, r#"{"a": {"b": {"c": 1}}}"#).unwrap();
/// let mut explorer = DataExplorer::new("doc", node);
/// assert!(explorer.open_branch(&"a.b".parse().unwrap()));
/// assert!(!explorer.open_branch(&"a.b.c".parse().unwrap()));
/// ```
impl OpenBranch for DataExplorer {
    fn open_branch(&mut self, path: &Path) -> bool {
        let Some(index) = self.branch_index(path) else {
            return false;
        };
        let mut at = Some(index);
        while let Some(index) = at {
            self.tree.set_collapsed(index, false);
            at = self.shape.parents[index];
        }
        true
    }

    fn close_branch(&mut self, path: &Path) -> bool {
        let Some(index) = self.branch_index(path) else {
            return false;
        };
        self.tree.set_collapsed(index, true);
        true
    }
}

impl DataExplorer {
    /// The index of the non-empty container at `path`.
    fn branch_index(&self, path: &Path) -> Option<usize> {
        self.shape
            .root
            .at(path)
            .filter(|node| node.is_container() && !node.is_empty())?;
        self.shape.index_of(path)
    }
}

impl Component for DataExplorer {
    type Output = Path;

    fn keymap(&self) -> Keymap {
        let mut keymap = self.keymap.clone();
        keymap.extend(self.tree.keymap());
        keymap
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<Path> {
        if let Some(key) = event.key() {
            if self.keymap.is(key, "copy-value") && self.tree.select().menu().is_none() {
                self.tree.set_status(None);
                self.copy_value();
                return Flow::Continue;
            }
        }
        match self.tree.handle(event, context) {
            Flow::Done(index) => Flow::Done(self.shape.path(index)),
            Flow::Continue => Flow::Continue,
            Flow::Cancel => Flow::Cancel,
            Flow::Ignored => Flow::Ignored,
            Flow::Handoff(command) => Flow::Handoff(command),
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.tree.render(context)
    }

    fn tick(&self) -> Option<Duration> {
        self.tree.tick()
    }

    fn mouse(&self) -> bool {
        self.tree.mouse()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<Path>, NotInteractive> {
        Ok(self.tree.prompt(io)?.map(|index| self.shape.path(index)))
    }
}
