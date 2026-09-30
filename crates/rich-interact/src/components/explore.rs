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
use rich_ext::data::{copy_text, display_value, Explorer, Node, Path, PathSegment, Value};

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

/// The subtree at a path, drawn by [`Explorer`] when the preview renders.
/// Holding the document and a path, not a copy, keeps a large document's
/// previews cheap.
struct Subtree {
    root: Arc<Node>,
    path: Path,
    depth: usize,
}

impl Subtree {
    fn view(&self) -> Option<Explorer<'_>> {
        let node = self.root.at(&self.path)?;
        let label = match self.path.last() {
            None => "$".to_string(),
            Some(PathSegment::Key(key)) => key.clone(),
            Some(PathSegment::Index(index)) => format!("[{index}]"),
        };
        Some(Explorer::new(node).max_depth(self.depth).root_label(label))
    }
}

impl Renderable for Subtree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.view()
            .map(|view| view.rich_render(console, options))
            .unwrap_or_default()
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.view().map_or_else(
            || Measurement::new(0, 0),
            |view| view.measure(console, options),
        )
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
    root: Arc<Node>,
    paths: Vec<Path>,
    keymap: Keymap,
}

impl DataExplorer {
    /// Explore `node` under `prompt` (a file name, say). Only the root's
    /// children show at first.
    pub fn new(prompt: impl Into<String>, node: Node) -> DataExplorer {
        let root = Arc::new(node);
        let mut nodes: Vec<(usize, Item<usize>)> = Vec::new();
        let mut paths = Vec::new();
        let mut crumbs = Vec::new();
        // Parents before children, in document order.
        let mut stack: Vec<(Path, usize)> = vec![(Path::root(), 0)];
        while let Some((path, depth)) = stack.pop() {
            let Some(node) = root.at(&path) else { continue };
            let name = match path.last() {
                None => "$".to_string(),
                Some(PathSegment::Key(key)) => key.clone(),
                Some(PathSegment::Index(index)) => format!("[{index}]"),
            };
            let value = display_value(node);
            let label = if value.is_empty() {
                name.clone()
            } else if path.is_root() {
                format!("{name} {value}")
            } else {
                format!("{name}: {value}")
            };
            let index = nodes.len();
            let preview = Preview::Renderable(Arc::new(Subtree {
                root: Arc::clone(&root),
                path: path.clone(),
                depth: 3,
            }));
            let mut item = Item::new(index, label);
            item.preview = Some(preview);
            nodes.push((depth, item));
            crumbs.push(name);
            match &node.value {
                Value::Map(entries) => {
                    for (key, _) in entries.iter().rev() {
                        stack.push((path.child_key(key), depth + 1));
                    }
                }
                Value::Seq(items) => {
                    for i in (0..items.len()).rev() {
                        stack.push((path.child_index(i), depth + 1));
                    }
                }
                _ => {}
            }
            paths.push(path);
        }
        let jsonpaths = paths.iter().map(json_path).collect();
        let tree = TreeSelect::new(prompt, nodes)
            .crumbs(crumbs)
            .paths(jsonpaths)
            .breadcrumbs(true)
            .fold_below(1);
        DataExplorer {
            tree,
            root,
            paths,
            keymap: explore_keymap(),
        }
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
        &self.root
    }

    /// The focused node's path.
    pub fn focused(&self) -> Option<&Path> {
        self.tree.focused().map(|index| &self.paths[index])
    }

    /// The tree underneath.
    pub fn tree(&self) -> &TreeSelect<usize> {
        &self.tree
    }

    /// Alt+Y: copy the focused node's value.
    fn copy_value(&mut self) {
        let Some(node) = self.focused().and_then(|path| self.root.at(path)) else {
            return;
        };
        let result = clipboard::copy(copy_text(node));
        self.tree
            .set_status(Some(clipboard::report("value", &result)));
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
            Flow::Done(index) => Flow::Done(self.paths[index].clone()),
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
        Ok(self.tree.prompt(io)?.map(|index| self.paths[index].clone()))
    }
}
