//! A width-aware tree (or table) over a [`Node`], with folding and limits.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use rich::cells::{cell_len, char_cell_width};
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Justify, Overflow, Renderable, Segment, Style, Table, Text};

use super::table::{cell_text, TableOptions, TableView};
use super::{quote_str, scalar_text, style, summary, Node, Path, Value, XmlKind};
use crate::event::flatten as join_lines;

// The thin guides of core's `Tree` (upstream `TREE_GUIDES[0]`).
const SPACE: &str = "    ";
const CONTINUE: &str = "│   ";
const FORK: &str = "├── ";
const END: &str = "└── ";

/// How an [`Explorer`] lays the document out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// A tree with guide lines.
    #[default]
    Tree,
    /// A table: records (a sequence of maps) as rows, anything else as
    /// `path | value` rows.
    Table,
}

/// A renderable view of a document tree.
///
/// Scalars reuse core's JSON styles, keys `json.key`, XML attributes
/// `data.attribute`; YAML anchors and aliases show as dim `&name` / `*name`
/// badges and INI/dotenv comments as a dim `# comment`. Every line is cut to
/// the available width: long strings shrink first (keeping their quotes),
/// then the line ends in `…`, so nothing wraps.
///
/// ```
/// use rich::Console;
/// use rich_ext::data::{parse, Explorer, Format};
///
/// let node = parse(Format::Json, r#"{"a": {"b": 1, "c": 2}, "d": [1, 2, 3]}"#).unwrap();
/// let explorer = Explorer::new(&node).max_depth(1).root_label("doc");
/// let out = Console::builder().width(40).build().render_export(&explorer);
/// assert_eq!(out, "doc\n├── a: {…} 2 keys\n└── d: […] 3 items\n");
/// ```
#[derive(Clone, Debug)]
pub struct Explorer<'a> {
    node: Cow<'a, Node>,
    max_depth: Option<usize>,
    max_length: Option<usize>,
    max_string: Option<usize>,
    show_paths: bool,
    show_types: bool,
    folded: HashSet<Path>,
    highlighted: HashMap<Path, Style>,
    root_label: Option<String>,
    view: View,
}

enum Key<'n> {
    Root,
    Name(&'n str),
    Index(usize),
}

enum Item<'n> {
    Node {
        key: Key<'n>,
        node: &'n Node,
        path: Path,
        depth: usize,
    },
    More(usize),
}

/// One rendered tree line: guide prefix, label, and the label's minimum.
struct Line {
    prefix: String,
    label: Text,
    minimum: usize,
}

impl<'a> Explorer<'a> {
    /// Explore an owned or borrowed node.
    pub fn new(node: impl Into<Cow<'a, Node>>) -> Self {
        Explorer {
            node: node.into(),
            max_depth: None,
            max_length: None,
            max_string: None,
            show_paths: false,
            show_types: false,
            folded: HashSet::new(),
            highlighted: HashMap::new(),
            root_label: None,
            view: View::Tree,
        }
    }

    /// Fold containers this deep (the root is depth 0) to a summary like
    /// `{…} 3 keys`.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Show at most this many children per container, then `… N more`.
    pub fn max_length(mut self, length: usize) -> Self {
        self.max_length = Some(length);
        self
    }

    /// Cut strings to this many characters (with `…`). Strings are also cut
    /// to the available width regardless.
    pub fn max_string(mut self, length: usize) -> Self {
        self.max_string = Some(length);
        self
    }

    /// Append each leaf's path, dim.
    pub fn show_paths(mut self, show: bool) -> Self {
        self.show_paths = show;
        self
    }

    /// Append each node's type (`str`, `int`, `map`, …), dim.
    pub fn show_types(mut self, show: bool) -> Self {
        self.show_types = show;
        self
    }

    /// Fold the container at `path`.
    pub fn fold(mut self, path: Path) -> Self {
        self.folded.insert(path);
        self
    }

    /// Style the tree line of the node at `path`, such as the selection of a
    /// `data::transform::Highlight`. The table view ignores it.
    pub fn highlight(mut self, path: Path, style: Style) -> Self {
        self.highlighted.insert(path, style);
        self
    }

    /// The root line's label (default: the root's summary).
    pub fn root_label(mut self, label: impl Into<String>) -> Self {
        self.root_label = Some(label.into());
        self
    }

    /// Tree or table.
    pub fn view(mut self, view: View) -> Self {
        self.view = view;
        self
    }

    /// The node being explored.
    pub fn node(&self) -> &Node {
        &self.node
    }

    fn is_folded(&self, path: &Path, depth: usize) -> bool {
        self.max_depth.is_some_and(|max| depth >= max) || self.folded.contains(path)
    }

    fn expanded(&self, node: &Node, path: &Path, depth: usize) -> bool {
        node.is_container() && !node.is_empty() && !self.is_folded(path, depth)
    }

    /// The label of one tree line, cut to `available` cells. Returns the
    /// label and its minimum useful width (its head plus an ellipsis).
    fn label(
        &self,
        console: &Console,
        key: &Key<'_>,
        node: &Node,
        path: &Path,
        expanded: bool,
        available: usize,
    ) -> (Text, usize) {
        let mut head = Text::new("");
        match key {
            Key::Root => {
                if let Some(label) = &self.root_label {
                    head.append(&super::escape_controls(label), None);
                }
            }
            Key::Name(name) => {
                let key_style = match node.meta.xml {
                    Some(XmlKind::Attribute) => "data.attribute",
                    Some(XmlKind::Text) => "data.comment",
                    _ => "json.key",
                };
                head.append(
                    &super::escape_controls(name),
                    Some(style(console, key_style).into()),
                );
            }
            Key::Index(index) => {
                head.append(
                    &format!("[{index}]"),
                    Some(style(console, "data.index").into()),
                );
            }
        }

        // The value: a scalar, or a summary for a folded, empty or unlabelled
        // root container.
        let root_summary = matches!(key, Key::Root) && self.root_label.is_none();
        let value: Option<(String, &'static str, bool)> = if node.is_container() {
            (!expanded || root_summary).then(|| (summary(node), "data.summary", false))
        } else {
            let is_string = matches!(node.value, Value::String(_));
            let text = match &node.value {
                Value::String(s) => fit_quoted(s, self.max_string, None),
                other => scalar_text(other, true).0,
            };
            Some((text, scalar_text(&node.value, true).1, is_string))
        };
        let separator = if value.is_some() && !head.is_empty() {
            ": "
        } else {
            ""
        };

        let mut badges = Text::new("");
        if let Some(anchor) = &node.meta.anchor {
            badges.append(
                &format!(" &{}", super::escape_controls(anchor)),
                Some(style(console, "data.anchor").into()),
            );
        }
        if let Some(alias) = &node.meta.alias {
            badges.append(
                &format!(" *{}", super::escape_controls(alias)),
                Some(style(console, "data.alias").into()),
            );
        }
        if self.show_types {
            badges.append(
                &format!(" ({})", node.type_name()),
                Some(style(console, "data.type").into()),
            );
        }

        let mut extras = Text::new("");
        if let Some(comment) = &node.meta.comment {
            let comment = super::escape_controls(&comment.replace('\n', " "));
            extras.append(
                &format!("  # {comment}"),
                Some(style(console, "data.comment").into()),
            );
        }
        if self.show_paths && !expanded && !path.is_root() {
            extras.append(
                &format!("  {path}"),
                Some(style(console, "data.path").into()),
            );
        }

        let fixed = head.cell_len() + cell_len(separator) + badges.cell_len();
        let minimum = head.cell_len() + 1;
        let mut text = head;
        text.append(separator, None);
        if let Some((mut value, value_style, is_string)) = value {
            if is_string && fixed + cell_len(&value) > available {
                if let (Some(budget), Value::String(s)) =
                    (available.checked_sub(fixed), &node.value)
                {
                    value = fit_quoted(s, self.max_string, Some(budget));
                }
            }
            text.append(&value, Some(style(console, value_style).into()));
        }
        let text = text.append_text(&badges).append_text(&extras);
        let mut text = text;
        text.truncate(available, Some(Overflow::Ellipsis), false);
        if let Some(style) = self.highlighted.get(path) {
            let end = text.plain().len();
            text.stylize(style.clone(), 0, end);
        }
        (text, minimum)
    }

    fn tree_lines(&self, console: &Console, width: usize) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut stack: Vec<(Item<'_>, String, String)> = vec![(
            Item::Node {
                key: Key::Root,
                node: &self.node,
                path: Path::root(),
                depth: 0,
            },
            String::new(),
            String::new(),
        )];
        while let Some((item, first, rest)) = stack.pop() {
            let available = width.saturating_sub(cell_len(&first));
            let (key, node, path, depth) = match item {
                Item::More(count) => {
                    let mut label =
                        Text::styled(format!("… {count} more"), style(console, "data.summary"));
                    label.truncate(available, Some(Overflow::Ellipsis), false);
                    lines.push(Line {
                        prefix: first,
                        label,
                        minimum: 1,
                    });
                    continue;
                }
                Item::Node {
                    key,
                    node,
                    path,
                    depth,
                } => (key, node, path, depth),
            };
            let expanded = self.expanded(node, &path, depth);
            let (label, minimum) = self.label(console, &key, node, &path, expanded, available);
            lines.push(Line {
                prefix: first,
                label,
                minimum,
            });
            if !expanded {
                continue;
            }
            let limit = self.max_length.unwrap_or(usize::MAX);
            let mut children: Vec<Item<'_>> = match &node.value {
                Value::Seq(items) => items
                    .iter()
                    .enumerate()
                    .take(limit)
                    .map(|(i, child)| Item::Node {
                        key: Key::Index(i),
                        node: child,
                        path: path.child_index(i),
                        depth: depth + 1,
                    })
                    .collect(),
                Value::Map(entries) => entries
                    .iter()
                    .take(limit)
                    .map(|(k, child)| Item::Node {
                        key: Key::Name(k),
                        node: child,
                        path: path.child_key(k),
                        depth: depth + 1,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            if node.len() > limit {
                children.push(Item::More(node.len() - limit));
            }
            let last = children.len().saturating_sub(1);
            for (index, child) in children.into_iter().enumerate().rev() {
                let (fork, space) = if index == last {
                    (END, SPACE)
                } else {
                    (FORK, CONTINUE)
                };
                stack.push((child, format!("{rest}{fork}"), format!("{rest}{space}")));
            }
        }
        lines
    }

    fn table_view(&self, console: &Console) -> Table {
        let records = matches!(&self.node.value, Value::Seq(items)
            if !items.is_empty() && items.iter().all(|i| matches!(i.value, Value::Map(_))));
        if records {
            let options = TableOptions {
                max_rows: self.max_length,
                max_string: self.max_string,
                ..TableOptions::default()
            };
            return TableView::new(&*self.node)
                .options(options)
                .to_table(console);
        }
        let mut table = Table::new();
        table.add_column_text(Text::new("path"), Justify::Left);
        table.add_column_text(Text::new("value"), Justify::Left);
        if self.show_types {
            table.add_column_text(Text::new("type"), Justify::Left);
        }
        let dim = |text: String| Text::styled(text, style(console, "data.path"));
        let mut stack: Vec<(&Node, Path, usize)> = vec![(&self.node, Path::root(), 0)];
        while let Some((node, path, depth)) = stack.pop() {
            if depth != usize::MAX && self.expanded(node, &path, depth) {
                let limit = self.max_length.unwrap_or(usize::MAX);
                if node.len() > limit {
                    // Pushed first, so it pops after the shown children.
                    stack.push((&self.node, path.clone(), usize::MAX));
                }
                match &node.value {
                    Value::Seq(items) => {
                        for (i, item) in items.iter().enumerate().take(limit).rev() {
                            stack.push((item, path.child_index(i), depth + 1));
                        }
                    }
                    Value::Map(entries) => {
                        for (k, v) in entries.iter().take(limit).rev() {
                            stack.push((v, path.child_key(k), depth + 1));
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let shown_path = if path.is_root() {
                "(root)".to_string()
            } else {
                path.to_string()
            };
            let mut row = vec![Text::new(super::escape_controls(&shown_path))];
            if depth == usize::MAX {
                // The `… N more` marker for `path`'s container.
                let container = self.node.at(&path).map_or(0, Node::len);
                let limit = self.max_length.unwrap_or(usize::MAX);
                row.push(dim(format!("… {} more", container.saturating_sub(limit))));
            } else if node.is_container() {
                row.push(Text::styled(summary(node), style(console, "data.summary")));
            } else {
                row.push(cell_text(console, node, self.max_string));
            }
            if self.show_types {
                row.push(dim(if depth == usize::MAX {
                    String::new()
                } else {
                    node.type_name().to_string()
                }));
            }
            table.add_row_text(row);
        }
        table
    }
}

/// `raw` as a quoted string, cut to `max_chars` characters and then to
/// `budget` cells, ending `…"` when cut.
pub(crate) fn fit_quoted(raw: &str, max_chars: Option<usize>, budget: Option<usize>) -> String {
    let (content, cut) = match max_chars.and_then(|n| raw.char_indices().nth(n)) {
        Some((i, _)) => (&raw[..i], true),
        None => (raw, false),
    };
    let quoted = quote_str(content);
    let escaped = &quoted[1..quoted.len() - 1];
    let whole = cell_len(escaped) + 2 + usize::from(cut);
    match budget {
        Some(budget) if whole > budget => {
            let allowed = budget.saturating_sub(3);
            let mut kept = String::new();
            let mut used = 0;
            for c in escaped.chars() {
                let w = char_cell_width(c);
                if used + w > allowed {
                    break;
                }
                used += w;
                kept.push(c);
            }
            // Never leave half an escape sequence.
            let trailing = kept.chars().rev().take_while(|c| *c == '\\').count();
            if trailing % 2 == 1 {
                kept.pop();
            }
            format!("\"{kept}…\"")
        }
        _ if cut => format!("\"{escaped}…\""),
        _ => quoted,
    }
}

impl Renderable for Explorer<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if options.max_width == 0 {
            return Vec::new();
        }
        if self.view == View::Table {
            return self.table_view(console).rich_render(console, options);
        }
        let guide = style(console, "tree.line");
        let rows = self
            .tree_lines(console, options.max_width)
            .into_iter()
            .map(|line| {
                let mut row = Vec::new();
                if !line.prefix.is_empty() {
                    row.push(Segment::new(line.prefix, Some(guide.clone())));
                }
                row.extend(line.label.render(console.theme(), &Style::new()));
                row
            })
            .collect();
        join_lines(rows)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        if self.view == View::Table {
            return Measurement::new(options.max_width, options.max_width);
        }
        let lines = self.tree_lines(console, usize::MAX / 2);
        let (mut minimum, mut maximum) = (0, 0);
        for line in &lines {
            let prefix = cell_len(&line.prefix);
            minimum = minimum.max(prefix + line.minimum);
            maximum = maximum.max(prefix + line.label.cell_len());
        }
        Measurement::new(minimum.min(maximum), maximum).with_maximum(options.max_width)
    }
}
