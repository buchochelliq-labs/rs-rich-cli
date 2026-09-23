//! Leaf-level differences between two documents.

use std::collections::{HashMap, HashSet};

use rich::{Console, ConsoleOptions, Overflow, Renderable, Segment, Style, Text};

use super::explorer::fit_quoted;
use super::{flatten, scalar_text, style, summary, value_eq, Node, Path, Value};
use crate::event::flatten as join_lines;

/// What happened to a leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
}

/// One difference.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: Path,
    pub kind: ChangeKind,
    /// The old leaf (removed or changed).
    pub old: Option<Node>,
    /// The new leaf (added or changed).
    pub new: Option<Node>,
}

/// The differences from `old` to `new`, compared leaf by leaf (see
/// [`flatten`]) and ignoring metadata. Changes come in document order:
/// the new document's order, with removed leaves where they stood in the
/// old one.
///
/// ```
/// use rich_ext::data::{diff, parse, ChangeKind, Format};
///
/// let old = parse(Format::Json, r#"{"a": 1, "b": 2}"#).unwrap();
/// let new = parse(Format::Json, r#"{"a": 1, "b": 3, "c": 4}"#).unwrap();
/// let kinds: Vec<_> = diff(&old, &new).iter().map(|c| c.kind).collect();
/// assert_eq!(kinds, [ChangeKind::Changed, ChangeKind::Added]);
/// ```
pub fn diff(old: &Node, new: &Node) -> Vec<Change> {
    let old_leaves = flatten(old);
    let new_leaves = flatten(new);
    let old_index: HashMap<&Path, usize> = old_leaves
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (p, i))
        .collect();
    let new_index: HashMap<&Path, usize> = new_leaves
        .iter()
        .enumerate()
        .map(|(i, (p, _))| (p, i))
        .collect();
    let mut changes = Vec::new();
    let mut done: HashSet<usize> = HashSet::new();
    let (mut i, mut j) = (0, 0);
    while i < old_leaves.len() || j < new_leaves.len() {
        if i < old_leaves.len() {
            let (path, node) = &old_leaves[i];
            if done.contains(&i) {
                i += 1;
                continue;
            }
            if !new_index.contains_key(path) {
                changes.push(Change {
                    path: path.clone(),
                    kind: ChangeKind::Removed,
                    old: Some(node.clone()),
                    new: None,
                });
                i += 1;
                continue;
            }
        }
        let Some((path, node)) = new_leaves.get(j) else {
            i += 1;
            continue;
        };
        j += 1;
        match old_index.get(path) {
            None => changes.push(Change {
                path: path.clone(),
                kind: ChangeKind::Added,
                old: None,
                new: Some(node.clone()),
            }),
            Some(&at) => {
                done.insert(at);
                let before = &old_leaves[at].1;
                if !value_eq(before, node) {
                    changes.push(Change {
                        path: path.clone(),
                        kind: ChangeKind::Changed,
                        old: Some(before.clone()),
                        new: Some(node.clone()),
                    });
                }
            }
        }
    }
    changes
}

/// Changes as `+ path: value` (added, `data.added`), `- path: value`
/// (removed, `data.removed`) and `~ path: old → new` (changed,
/// `data.changed`) lines.
///
/// ```
/// use rich::Console;
/// use rich_ext::data::{parse, DiffView, Format};
///
/// let old = parse(Format::Json, r#"{"a": 1, "b": "x"}"#).unwrap();
/// let new = parse(Format::Json, r#"{"a": 2, "c": true}"#).unwrap();
/// let out = Console::builder().width(40).build().render_export(&DiffView::new(&old, &new));
/// assert_eq!(out, "~ a: 1 → 2\n- b: \"x\"\n+ c: true\n");
/// ```
#[derive(Clone, Debug)]
pub struct DiffView {
    changes: Vec<Change>,
}

impl DiffView {
    /// The differences from `old` to `new`.
    pub fn new(old: &Node, new: &Node) -> Self {
        DiffView {
            changes: diff(old, new),
        }
    }
    /// Show precomputed changes.
    pub fn from_changes(changes: Vec<Change>) -> Self {
        DiffView { changes }
    }
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }
}

fn leaf(node: &Node) -> String {
    match &node.value {
        Value::Seq(_) | Value::Map(_) => summary(node),
        Value::String(s) => fit_quoted(s, Some(80), None),
        other => scalar_text(other, true).0,
    }
}

impl Renderable for DiffView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut rows = Vec::new();
        if self.changes.is_empty() {
            rows.push(vec![Segment::new(
                "No differences",
                Some(style(console, "data.summary")),
            )]);
        }
        for change in &self.changes {
            let path = if change.path.is_root() {
                "(root)".to_string()
            } else {
                change.path.to_string()
            };
            let (mark, key, body) = match (change.kind, &change.old, &change.new) {
                (ChangeKind::Added, _, Some(new)) => ("+", "data.added", leaf(new)),
                (ChangeKind::Removed, Some(old), _) => ("-", "data.removed", leaf(old)),
                (_, Some(old), Some(new)) => (
                    "~",
                    "data.changed",
                    format!("{} → {}", leaf(old), leaf(new)),
                ),
                _ => continue,
            };
            let mut text = Text::styled(
                super::escape_controls(&format!("{mark} {path}: {body}")),
                style(console, key),
            );
            text.truncate(options.max_width, Some(Overflow::Ellipsis), false);
            rows.push(text.render(console.theme(), &Style::new()));
        }
        join_lines(rows)
    }
}
