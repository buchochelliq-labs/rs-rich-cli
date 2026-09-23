//! `path = value` leaves, and back.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Table, Text};

use super::table::cell_text;
use super::{escape_controls, style, Node, Path, PathSegment, Value};

/// Every leaf of `node` with its path, in document order. Scalars and
/// *empty* containers are leaves, so [`unflatten`] rebuilds the same shape.
/// A scalar root is one leaf at the empty path.
///
/// ```
/// use rich_ext::data::{flatten, parse, unflatten, Format};
///
/// let node = parse(Format::Json, r#"{"a": [1, {"b": null}], "c": {}}"#).unwrap();
/// let leaves = flatten(&node);
/// let paths: Vec<String> = leaves.iter().map(|(p, _)| p.to_string()).collect();
/// assert_eq!(paths, ["a[0]", "a[1].b", "c"]);
/// assert_eq!(unflatten(leaves).unwrap().to_json(), node.to_json());
/// ```
pub fn flatten(node: &Node) -> Vec<(Path, Node)> {
    let mut leaves = Vec::new();
    node.walk(|path, node| {
        if node.is_empty() {
            leaves.push((path.clone(), node.clone()));
        }
    });
    leaves
}

/// Why leaves could not be put back together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnflattenError {
    /// No leaves at all.
    Empty,
    /// The path is a leaf and also has children.
    LeafAndContainer(Path),
    /// The same leaf path twice.
    Duplicate(Path),
    /// The container at the path has both keys and indexes.
    MixedSegments(Path),
    /// The sequence at the path is missing index `missing`.
    SparseIndex { path: Path, missing: usize },
}

impl fmt::Display for UnflattenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = |p: &Path| {
            if p.is_root() {
                "the root".to_string()
            } else {
                format!("`{p}`")
            }
        };
        match self {
            UnflattenError::Empty => write!(f, "no leaves to unflatten"),
            UnflattenError::LeafAndContainer(p) => {
                write!(f, "{} is both a value and a container", shown(p))
            }
            UnflattenError::Duplicate(p) => write!(f, "{} appears twice", shown(p)),
            UnflattenError::MixedSegments(p) => {
                write!(f, "{} has both keys and indexes", shown(p))
            }
            UnflattenError::SparseIndex { path, missing } => {
                write!(f, "{} is missing index {missing}", shown(path))
            }
        }
    }
}

impl std::error::Error for UnflattenError {}

/// A node under construction. `None` marks an entry a path created but has
/// not filled yet.
enum Slot {
    Leaf(Node),
    Map {
        entries: Vec<(String, Option<Slot>)>,
        index: HashMap<String, usize>,
    },
    Seq(BTreeMap<usize, Option<Slot>>),
}

fn finish(slot: Option<Slot>, path: &mut Path) -> Result<Node, UnflattenError> {
    Ok(match slot {
        None => Node::new(Value::Null),
        Some(Slot::Leaf(node)) => node,
        Some(Slot::Map { entries, .. }) => {
            let mut out = Vec::with_capacity(entries.len());
            for (key, slot) in entries {
                path.push(PathSegment::Key(key.clone()));
                out.push((key, finish(slot, path)?));
                path.0.pop();
            }
            Node::new(Value::Map(out))
        }
        Some(Slot::Seq(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for (expected, (index, slot)) in items.into_iter().enumerate() {
                if index != expected {
                    return Err(UnflattenError::SparseIndex {
                        path: path.clone(),
                        missing: expected,
                    });
                }
                path.push(PathSegment::Index(index));
                out.push(finish(slot, path)?);
                path.0.pop();
            }
            Node::new(Value::Seq(out))
        }
    })
}

/// Rebuild a tree from `(path, leaf)` pairs, in any order. Map keys keep
/// their first-seen order; sequence indexes must run `0..n` without gaps.
pub fn unflatten(leaves: Vec<(Path, Node)>) -> Result<Node, UnflattenError> {
    if leaves.is_empty() {
        return Err(UnflattenError::Empty);
    }
    let mut root: Option<Slot> = None;
    for (path, leaf) in leaves {
        let mut slot = &mut root;
        for (depth, segment) in path.segments().iter().enumerate() {
            let here = || Path::from(path.segments()[..depth].to_vec());
            let container = slot.get_or_insert_with(|| match segment {
                PathSegment::Key(_) => Slot::Map {
                    entries: Vec::new(),
                    index: HashMap::new(),
                },
                PathSegment::Index(_) => Slot::Seq(BTreeMap::new()),
            });
            slot = match (container, segment) {
                (Slot::Leaf(_), _) => return Err(UnflattenError::LeafAndContainer(here())),
                (Slot::Map { entries, index }, PathSegment::Key(key)) => {
                    let at = match index.get(key) {
                        Some(&at) => at,
                        None => {
                            index.insert(key.clone(), entries.len());
                            entries.push((key.clone(), None));
                            entries.len() - 1
                        }
                    };
                    &mut entries[at].1
                }
                (Slot::Seq(items), PathSegment::Index(i)) => items.entry(*i).or_insert(None),
                _ => return Err(UnflattenError::MixedSegments(here())),
            };
        }
        match slot {
            None => *slot = Some(Slot::Leaf(leaf)),
            Some(Slot::Leaf(_)) => return Err(UnflattenError::Duplicate(path)),
            Some(_) => return Err(UnflattenError::LeafAndContainer(path)),
        }
    }
    finish(root, &mut Path::root())
}

/// Leaves as a `path | value` table, optionally with a type column.
#[derive(Clone, Debug)]
pub struct FlatView<'a> {
    node: Cow<'a, Node>,
    show_types: bool,
    max_string: Option<usize>,
}

impl<'a> FlatView<'a> {
    pub fn new(node: impl Into<Cow<'a, Node>>) -> Self {
        FlatView {
            node: node.into(),
            show_types: false,
            max_string: None,
        }
    }
    /// Add a `type` column.
    pub fn show_types(mut self, show: bool) -> Self {
        self.show_types = show;
        self
    }
    /// Cut strings to this many characters.
    pub fn max_string(mut self, length: usize) -> Self {
        self.max_string = Some(length);
        self
    }
}

impl Renderable for FlatView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut table = Table::new();
        table.add_column_text(Text::new("path"), Justify::Left);
        table.add_column_text(Text::new("value"), Justify::Left);
        if self.show_types {
            table.add_column_text(Text::new("type"), Justify::Left);
        }
        for (path, leaf) in flatten(&self.node) {
            let shown = if path.is_root() {
                "(root)".to_string()
            } else {
                path.to_string()
            };
            let value = if leaf.is_container() {
                Text::styled(super::summary(&leaf), style(console, "data.summary"))
            } else {
                cell_text(console, &leaf, self.max_string)
            };
            let mut row = vec![Text::new(escape_controls(&shown)), value];
            if self.show_types {
                row.push(Text::styled(leaf.type_name(), style(console, "data.type")));
            }
            table.add_row_text(row);
        }
        table.rich_render(console, options)
    }
}
