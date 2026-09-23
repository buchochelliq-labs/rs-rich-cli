//! Seeded render fuzzing with invariants and minimisation.
//!
//! [`Case::generate`] builds a bounded random renderable tree from a seed
//! and a case index with a SplitMix64 [`Rng`] (no dependencies): styled
//! [`Text`] (markup with wide CJK, emoji, ZWJ sequences, combining marks,
//! zero-width characters, tabs, newlines and long words, under random
//! justify/overflow/no-wrap), [`Table`]s with random columns, justify,
//! overflow, no-wrap, width limits and boxes, and `Panel`, `Padding`,
//! `Align`, `Columns` and `Tree` nesting up to [`GenOptions::max_depth`].
//!
//! [`fuzz`] renders each case at its random width and checks the
//! [`Invariants`]: no panic, no line wider than the width, an identical
//! second render, `measure()` bounds (minimum ≤ maximum, visible width ≤
//! maximum), and any custom checks. A failure is shrunk greedily — hoisting
//! children, dropping rows, columns, chunks and children, shortening text,
//! clearing options and narrowing the width — while the same invariant keeps
//! failing, within [`GenOptions::shrink_budget`] checks.
//!
//! Every failure carries its seed and case index: `Case::generate(seed,
//! index, &options)` rebuilds it exactly, and [`Failure::minimized`] is a
//! Rust reproduction of the shrunk case.

use std::fmt::Write as _;
use std::sync::Arc;

use rich::cells::cell_len;
use rich::r#box::{
    Box as BoxSet, ASCII, DOUBLE, HEAVY, HEAVY_HEAD, MINIMAL, ROUNDED, SIMPLE, SQUARE,
};
use rich::{
    Align, ColumnOptions, Columns, Console, ConsoleOptions, Justify, Overflow, Padding, Panel,
    Renderable, Segment, Table, Text, Tree,
};

use super::{panic_message, plain_lines, plural, table_then_line, visible_width, Probe};

/// SplitMix64: small, fast and good enough for test-case generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    /// The generator for case `index` of a run seeded `seed`.
    pub fn for_case(seed: u64, index: u64) -> Self {
        let mut mixer = Rng(seed ^ index.wrapping_mul(0xA24B_AED4_963E_E407));
        Rng(mixer.next_u64())
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `0..n` (`0` when `n == 0`).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi.saturating_sub(lo) + 1)
    }
    /// True with probability `percent`/100.
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Generation bounds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenOptions {
    /// Container nesting depth (default 3).
    pub max_depth: usize,
    /// Children, rows, columns and items per node (default 4).
    pub max_children: usize,
    /// Words per text chunk (default 10).
    pub max_words: usize,
    /// Render width range (default 1..=100).
    pub min_width: usize,
    pub max_width: usize,
    /// Checks spent shrinking one failure (default 400).
    pub shrink_budget: usize,
    /// Kinds of node to generate (default all). Leaves come from `Text`,
    /// `Table`, `Columns` and `Tree`; with none of them, `Text` is used.
    pub kinds: Vec<NodeKind>,
}

/// A kind of generated node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Text,
    Table,
    Columns,
    Tree,
    Panel,
    Padding,
    Align,
}

impl NodeKind {
    pub const ALL: [NodeKind; 7] = [
        NodeKind::Text,
        NodeKind::Table,
        NodeKind::Columns,
        NodeKind::Tree,
        NodeKind::Panel,
        NodeKind::Padding,
        NodeKind::Align,
    ];
    fn is_leaf(self) -> bool {
        matches!(
            self,
            NodeKind::Text | NodeKind::Table | NodeKind::Columns | NodeKind::Tree
        )
    }
}

impl GenOptions {
    /// Generate only `kinds`.
    pub fn kinds(mut self, kinds: impl Into<Vec<NodeKind>>) -> Self {
        self.kinds = kinds.into();
        self
    }
    /// Generate every kind except `kind`.
    pub fn without(mut self, kind: NodeKind) -> Self {
        self.kinds.retain(|k| *k != kind);
        self
    }
}

impl Default for GenOptions {
    fn default() -> Self {
        GenOptions {
            max_depth: 3,
            max_children: 4,
            max_words: 10,
            min_width: 1,
            max_width: 100,
            shrink_budget: 400,
            kinds: NodeKind::ALL.to_vec(),
        }
    }
}

const WORDS: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "rich",
    "table",
    "wrap",
    "x",
    "hello",
    "world",
    "a",
    "Ω",
    "界",
    "日本語",
    "한국어",
    "中文字符",
    "🙂",
    "🚀",
    "👍🏽",
    "👩\u{200d}💻",
    "👨\u{200d}👩\u{200d}👧",
    "e\u{301}",
    "n\u{303}o",
    "a\u{308}\u{304}",
    "\u{200b}",
    "a\u{200d}b",
    "\t",
    "tab\there",
    "supercalifragilisticexpialidocious",
    "https://example.com/a/very/long/path/segment",
    "[x]",
    "…",
    "—",
    "--",
    "\n",
];
/// Words for labels and headers: no newlines or brackets.
const LABEL_WORDS: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "name",
    "id",
    "界",
    "日本語",
    "🙂",
    "e\u{301}",
    "value",
    "supercalifragilistic",
    "x",
];
const STYLES: &[&str] = &[
    "bold",
    "italic",
    "red",
    "#ff8700",
    "bold on blue",
    "underline magenta",
    "reverse",
    "dim",
    "strike",
    "link https://example.com",
];
const BOXES: &[&str] = &[
    "ASCII",
    "SQUARE",
    "ROUNDED",
    "HEAVY",
    "HEAVY_HEAD",
    "DOUBLE",
    "MINIMAL",
    "SIMPLE",
];
const JUSTIFY: &[Justify] = &[
    Justify::Default,
    Justify::Left,
    Justify::Center,
    Justify::Right,
    Justify::Full,
];
const OVERFLOW: &[Overflow] = &[
    Overflow::Fold,
    Overflow::Crop,
    Overflow::Ellipsis,
    Overflow::Ignore,
];

fn box_set(name: &str) -> BoxSet {
    match name {
        "ASCII" => ASCII,
        "SQUARE" => SQUARE,
        "ROUNDED" => ROUNDED,
        "HEAVY" => HEAVY,
        "DOUBLE" => DOUBLE,
        "MINIMAL" => MINIMAL,
        "SIMPLE" => SIMPLE,
        _ => HEAVY_HEAD,
    }
}

/// A run of text with an optional style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub text: String,
    pub style: Option<String>,
}

/// A generated text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextNode {
    pub chunks: Vec<Chunk>,
    pub justify: Justify,
    pub overflow: Option<Overflow>,
    pub no_wrap: bool,
}

impl TextNode {
    /// The markup the chunks make, with their text escaped.
    pub fn markup(&self) -> String {
        let mut out = String::new();
        for chunk in &self.chunks {
            let text = rich::markup::escape(&chunk.text);
            match &chunk.style {
                Some(style) => {
                    let _ = write!(out, "[{style}]{text}[/]");
                }
                None => out.push_str(&text),
            }
        }
        out
    }
    fn build(&self) -> Text {
        let markup = self.markup();
        let mut text = Text::from_markup(&markup).unwrap_or_else(|_| Text::new(markup));
        text = text.justify(self.justify).no_wrap(self.no_wrap);
        if let Some(overflow) = self.overflow {
            text = text.overflow(overflow);
        }
        text
    }
    fn to_rust(&self) -> String {
        let mut out = format!("Text::from_markup({:?}).unwrap()", self.markup());
        if self.justify != Justify::Default {
            let _ = write!(out, ".justify(Justify::{:?})", self.justify);
        }
        if let Some(overflow) = self.overflow {
            let _ = write!(out, ".overflow(Overflow::{overflow:?})");
        }
        if self.no_wrap {
            out.push_str(".no_wrap(true)");
        }
        out
    }
}

/// A generated table column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    pub header: String,
    pub justify: Justify,
    pub overflow: Overflow,
    pub no_wrap: bool,
    pub min_width: Option<usize>,
    pub max_width: Option<usize>,
}

/// A generated table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableNode {
    pub columns: Vec<ColumnSpec>,
    pub rows: Vec<Vec<String>>,
    pub box_name: &'static str,
    pub show_header: bool,
    pub show_lines: bool,
    pub expand: bool,
}

/// A generated tree node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub label: String,
    pub children: Vec<TreeNode>,
}

/// A generated renderable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Text(TextNode),
    Table(TableNode),
    Panel {
        title: Option<String>,
        child: Box<Node>,
    },
    Padding {
        pad: (usize, usize, usize, usize),
        child: Box<Node>,
    },
    Align {
        /// `left`, `center` or `right`.
        align: &'static str,
        child: Box<Node>,
    },
    Columns(Vec<String>),
    Tree(TreeNode),
}

fn words(rng: &mut Rng, pool: &[&str], max: usize) -> String {
    let n = rng.range(0, max);
    let mut out = String::new();
    for i in 0..n {
        if i > 0 && !rng.chance(10) {
            out.push(' ');
        }
        out.push_str(rng.pick::<&str>(pool));
    }
    out
}

fn gen_text(rng: &mut Rng, o: &GenOptions) -> TextNode {
    let chunks = (0..rng.range(1, o.max_children))
        .map(|_| Chunk {
            text: words(rng, WORDS, o.max_words),
            style: rng.chance(50).then(|| (*rng.pick(STYLES)).to_owned()),
        })
        .collect();
    TextNode {
        chunks,
        justify: *rng.pick(JUSTIFY),
        overflow: rng.chance(60).then(|| *rng.pick(OVERFLOW)),
        no_wrap: rng.chance(15),
    }
}

fn gen_table(rng: &mut Rng, o: &GenOptions) -> TableNode {
    let columns: Vec<ColumnSpec> = (0..rng.range(1, o.max_children))
        .map(|_| ColumnSpec {
            header: words(rng, LABEL_WORDS, 2),
            justify: *rng.pick(&JUSTIFY[1..]),
            overflow: *rng.pick(OVERFLOW),
            no_wrap: rng.chance(20),
            min_width: rng.chance(15).then(|| rng.range(1, 12)),
            max_width: rng.chance(15).then(|| rng.range(1, 20)),
        })
        .collect();
    let rows = (0..rng.range(0, o.max_children))
        .map(|_| {
            (0..columns.len())
                .map(|_| words(rng, WORDS, o.max_words / 2 + 1))
                .collect()
        })
        .collect();
    TableNode {
        columns,
        rows,
        box_name: rng.pick::<&str>(BOXES),
        show_header: !rng.chance(20),
        show_lines: rng.chance(25),
        expand: rng.chance(25),
    }
}

fn gen_tree(rng: &mut Rng, o: &GenOptions, depth: usize) -> TreeNode {
    let children = if depth < o.max_depth {
        (0..rng.range(0, o.max_children.min(3)))
            .map(|_| gen_tree(rng, o, depth + 1))
            .collect()
    } else {
        Vec::new()
    };
    TreeNode {
        label: words(rng, LABEL_WORDS, 4),
        children,
    }
}

fn gen_node(rng: &mut Rng, o: &GenOptions, depth: usize) -> Node {
    let leaf = depth >= o.max_depth;
    let kinds: Vec<NodeKind> = o
        .kinds
        .iter()
        .copied()
        .filter(|k| !leaf || k.is_leaf())
        .collect();
    let kind = if kinds.is_empty() {
        NodeKind::Text
    } else {
        *rng.pick(&kinds)
    };
    match kind {
        NodeKind::Text => Node::Text(gen_text(rng, o)),
        NodeKind::Table => Node::Table(gen_table(rng, o)),
        NodeKind::Columns => Node::Columns(
            (0..rng.range(0, o.max_children * 2))
                .map(|_| words(rng, LABEL_WORDS, 3))
                .collect(),
        ),
        NodeKind::Tree => Node::Tree(gen_tree(rng, o, depth)),
        NodeKind::Panel => Node::Panel {
            title: rng.chance(50).then(|| words(rng, LABEL_WORDS, 3)),
            child: Box::new(gen_node(rng, o, depth + 1)),
        },
        NodeKind::Padding => Node::Padding {
            pad: (
                rng.range(0, 2),
                rng.range(0, 4),
                rng.range(0, 2),
                rng.range(0, 4),
            ),
            child: Box::new(gen_node(rng, o, depth + 1)),
        },
        NodeKind::Align => Node::Align {
            align: rng.pick::<&str>(&["left", "center", "right"]),
            child: Box::new(gen_node(rng, o, depth + 1)),
        },
    }
}

fn build_tree(tree: &mut Tree, node: &TreeNode) {
    for child in &node.children {
        let sub = tree.add(child.label.clone());
        build_tree(sub, child);
    }
}

impl Node {
    /// The core renderable this node describes.
    pub fn build(&self) -> Box<dyn Renderable> {
        match self {
            Node::Text(t) => Box::new(t.build()),
            Node::Table(t) => {
                let mut table = Table::new().box_set(box_set(t.box_name));
                table = table
                    .show_header(t.show_header)
                    .show_lines(t.show_lines)
                    .expand(t.expand);
                for c in &t.columns {
                    table.add_column_with(
                        Text::new(c.header.clone()),
                        ColumnOptions {
                            justify: c.justify,
                            overflow: c.overflow,
                            no_wrap: c.no_wrap,
                            min_width: c.min_width,
                            max_width: c.max_width,
                            ..ColumnOptions::default()
                        },
                    );
                }
                for row in &t.rows {
                    table.add_row_text(row.iter().map(|c| Text::new(c.clone())).collect());
                }
                Box::new(table)
            }
            Node::Panel { title, child } => {
                let mut panel = Panel::new(child.build());
                if let Some(title) = title {
                    panel = panel.title(rich::markup::escape(title));
                }
                Box::new(panel)
            }
            Node::Padding { pad, child } => Box::new(Padding::new(child.build(), *pad)),
            Node::Align { align, child } => Box::new(match *align {
                "center" => Align::center(child.build()),
                "right" => Align::right(child.build()),
                _ => Align::left(child.build()),
            }),
            Node::Columns(items) => Box::new(Columns::new(items.clone())),
            Node::Tree(t) => {
                let mut tree = Tree::new(t.label.clone());
                build_tree(&mut tree, t);
                Box::new(tree)
            }
        }
    }

    /// A Rust expression that builds this node.
    pub fn to_rust(&self) -> String {
        match self {
            Node::Text(t) => t.to_rust(),
            Node::Table(t) => {
                let mut out = format!(
                    "{{ let mut t = Table::new().box_set(rich::r#box::{}).show_header({}).show_lines({}).expand({});",
                    t.box_name, t.show_header, t.show_lines, t.expand
                );
                for c in &t.columns {
                    let _ = write!(
                        out,
                        " t.add_column_with(Text::new({:?}), ColumnOptions {{ justify: Justify::{:?}, overflow: Overflow::{:?}, no_wrap: {}, min_width: {:?}, max_width: {:?}, ..Default::default() }});",
                        c.header, c.justify, c.overflow, c.no_wrap, c.min_width, c.max_width
                    );
                }
                for row in &t.rows {
                    let _ = write!(out, " t.add_row(&{row:?});");
                }
                out.push_str(" t }");
                out
            }
            Node::Panel { title, child } => {
                let mut out = format!("Panel::new(Box::new({}))", child.to_rust());
                if let Some(title) = title {
                    let _ = write!(out, ".title(rich::markup::escape({title:?}))");
                }
                out
            }
            Node::Padding { pad, child } => {
                format!("Padding::new(Box::new({}), {pad:?})", child.to_rust())
            }
            Node::Align { align, child } => {
                format!("Align::{align}(Box::new({}))", child.to_rust())
            }
            Node::Columns(items) => {
                format!(
                    "Columns::new(vec![{}])",
                    items
                        .iter()
                        .map(|i| format!("{i:?}.to_string()"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Node::Tree(t) => {
                fn adds(node: &TreeNode, parent: &str, depth: usize, out: &mut String) {
                    for child in &node.children {
                        let var = format!("n{depth}");
                        let _ = write!(out, " {{ let {var} = {parent}.add({:?});", child.label);
                        adds(child, &var, depth + 1, out);
                        out.push_str(" }");
                    }
                }
                let mut out = format!("{{ let mut t = Tree::new({:?});", t.label);
                adds(t, "t", 0, &mut out);
                out.push_str(" t }");
                out
            }
        }
    }

    /// The narrowest width this node can fit at all: table and panel
    /// borders and padding, and the widest glyph (a wide character needs two
    /// cells). Below it upstream overflows too and a top-level print crops,
    /// so [`Invariants::fits_width`] only applies from here up. Tree guides
    /// are not counted: upstream drops what no longer fits.
    pub fn floor(&self) -> usize {
        fn glyph(s: &str) -> usize {
            s.chars()
                .map(rich::cells::char_cell_width)
                .max()
                .unwrap_or(0)
                .max(1)
        }
        fn tree_floor(t: &TreeNode) -> usize {
            t.children
                .iter()
                .map(tree_floor)
                .fold(glyph(&t.label), usize::max)
        }
        match self {
            Node::Text(t) => t.chunks.iter().map(|c| glyph(&c.text)).max().unwrap_or(1),
            Node::Table(t) => {
                let cells: usize = (0..t.columns.len())
                    .map(|i| {
                        let header = glyph(&t.columns[i].header);
                        t.rows
                            .iter()
                            .filter_map(|r| r.get(i))
                            .map(|c| glyph(c))
                            .fold(header, usize::max)
                            + 2
                    })
                    .sum();
                cells + t.columns.len() + 1
            }
            Node::Panel { child, .. } => child.floor() + 4,
            Node::Padding { pad, child } => child.floor() + pad.1 + pad.3,
            Node::Align { child, .. } => child.floor(),
            Node::Columns(items) => items.iter().map(|i| glyph(i)).max().unwrap_or(1),
            Node::Tree(t) => tree_floor(t),
        }
    }

    /// Whether any part may overflow by design: `Overflow::Ignore`, or a
    /// table column `min_width` (a hard floor upstream too).
    pub fn allows_overflow(&self) -> bool {
        match self {
            Node::Text(t) => t.overflow == Some(Overflow::Ignore),
            Node::Table(t) => t
                .columns
                .iter()
                .any(|c| c.overflow == Overflow::Ignore || c.min_width.is_some()),
            Node::Panel { child, .. } | Node::Padding { child, .. } | Node::Align { child, .. } => {
                child.allows_overflow()
            }
            Node::Columns(_) | Node::Tree(_) => false,
        }
    }

    /// Whether any text holds a tab.
    pub fn has_tabs(&self) -> bool {
        fn tree_tabs(t: &TreeNode) -> bool {
            t.label.contains('\t') || t.children.iter().any(tree_tabs)
        }
        match self {
            Node::Text(t) => t.chunks.iter().any(|c| c.text.contains('\t')),
            Node::Table(t) => t
                .rows
                .iter()
                .flatten()
                .chain(t.columns.iter().map(|c| &c.header))
                .any(|c| c.contains('\t')),
            Node::Panel { child, .. } | Node::Padding { child, .. } | Node::Align { child, .. } => {
                child.has_tabs()
            }
            Node::Columns(items) => items.iter().any(|i| i.contains('\t')),
            Node::Tree(t) => tree_tabs(t),
        }
    }

    /// Nodes in this tree.
    pub fn size(&self) -> usize {
        match self {
            Node::Panel { child, .. } | Node::Padding { child, .. } | Node::Align { child, .. } => {
                1 + child.size()
            }
            Node::Tree(t) => tree_size(t),
            _ => 1,
        }
    }

    /// One-step simplifications, most aggressive first.
    pub fn shrink(&self) -> Vec<Node> {
        let mut out = Vec::new();
        match self {
            Node::Panel { title, child } => {
                out.push((**child).clone());
                if title.is_some() {
                    out.push(Node::Panel {
                        title: None,
                        child: child.clone(),
                    });
                }
                for t in title.iter().flat_map(|t| shrink_str(t)) {
                    out.push(Node::Panel {
                        title: Some(t),
                        child: child.clone(),
                    });
                }
                for c in child.shrink() {
                    out.push(Node::Panel {
                        title: title.clone(),
                        child: Box::new(c),
                    });
                }
            }
            Node::Padding { pad, child } => {
                out.push((**child).clone());
                if *pad != (0, 0, 0, 0) {
                    out.push(Node::Padding {
                        pad: (0, 0, 0, 0),
                        child: child.clone(),
                    });
                }
                for c in child.shrink() {
                    out.push(Node::Padding {
                        pad: *pad,
                        child: Box::new(c),
                    });
                }
            }
            Node::Align { align, child } => {
                out.push((**child).clone());
                for c in child.shrink() {
                    out.push(Node::Align {
                        align,
                        child: Box::new(c),
                    });
                }
            }
            Node::Columns(items) => {
                for i in 0..items.len() {
                    let mut v = items.clone();
                    v.remove(i);
                    out.push(Node::Columns(v));
                }
                for (i, item) in items.iter().enumerate() {
                    for s in shrink_str(item) {
                        let mut v = items.clone();
                        v[i] = s;
                        out.push(Node::Columns(v));
                    }
                }
            }
            Node::Tree(t) => out.extend(shrink_tree(t).into_iter().map(Node::Tree)),
            Node::Text(t) => out.extend(shrink_text(t).into_iter().map(Node::Text)),
            Node::Table(t) => out.extend(shrink_table(t).into_iter().map(Node::Table)),
        }
        out
    }
}

fn tree_size(t: &TreeNode) -> usize {
    1 + t.children.iter().map(tree_size).sum::<usize>()
}

/// Shorter versions of `s`: empty, each half, without the last or first char.
fn shrink_str(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut out = vec![String::new()];
    if chars.len() > 1 {
        let half = chars.len() / 2;
        out.push(chars[..half].iter().collect());
        out.push(chars[half..].iter().collect());
        out.push(chars[..chars.len() - 1].iter().collect());
        out.push(chars[1..].iter().collect());
    }
    out.dedup();
    out
}

fn shrink_tree(t: &TreeNode) -> Vec<TreeNode> {
    let mut out = Vec::new();
    for child in &t.children {
        out.push(child.clone());
    }
    for i in 0..t.children.len() {
        let mut n = t.clone();
        n.children.remove(i);
        out.push(n);
    }
    for s in shrink_str(&t.label) {
        let mut n = t.clone();
        n.label = s;
        out.push(n);
    }
    for (i, child) in t.children.iter().enumerate() {
        for c in shrink_tree(child) {
            let mut n = t.clone();
            n.children[i] = c;
            out.push(n);
        }
    }
    out
}

fn shrink_text(t: &TextNode) -> Vec<TextNode> {
    let mut out = Vec::new();
    for i in 0..t.chunks.len() {
        if t.chunks.len() > 1 {
            let mut n = t.clone();
            n.chunks.remove(i);
            out.push(n);
        }
    }
    let with = |f: &dyn Fn(&mut TextNode)| {
        let mut n = t.clone();
        f(&mut n);
        n
    };
    if t.justify != Justify::Default {
        out.push(with(&|n| n.justify = Justify::Default));
    }
    if t.overflow.is_some() {
        out.push(with(&|n| n.overflow = None));
    }
    if t.no_wrap {
        out.push(with(&|n| n.no_wrap = false));
    }
    for (i, chunk) in t.chunks.iter().enumerate() {
        if chunk.style.is_some() {
            let mut n = t.clone();
            n.chunks[i].style = None;
            out.push(n);
        }
        for s in shrink_str(&chunk.text) {
            let mut n = t.clone();
            n.chunks[i].text = s;
            out.push(n);
        }
    }
    out
}

fn shrink_table(t: &TableNode) -> Vec<TableNode> {
    let mut out = Vec::new();
    for i in 0..t.rows.len() {
        let mut n = t.clone();
        n.rows.remove(i);
        out.push(n);
    }
    if t.columns.len() > 1 {
        for i in 0..t.columns.len() {
            let mut n = t.clone();
            n.columns.remove(i);
            for row in &mut n.rows {
                if i < row.len() {
                    row.remove(i);
                }
            }
            out.push(n);
        }
    }
    let defaults = ColumnSpec {
        header: String::new(),
        justify: Justify::Left,
        overflow: Overflow::Ellipsis,
        no_wrap: false,
        min_width: None,
        max_width: None,
    };
    for (i, c) in t.columns.iter().enumerate() {
        let plain = ColumnSpec {
            header: c.header.clone(),
            ..defaults.clone()
        };
        if *c != plain {
            let mut n = t.clone();
            n.columns[i] = plain;
            out.push(n);
        }
        for s in shrink_str(&c.header) {
            let mut n = t.clone();
            n.columns[i].header = s;
            out.push(n);
        }
    }
    if t.show_lines || t.expand || !t.show_header || t.box_name != "HEAVY_HEAD" {
        let mut n = t.clone();
        n.show_lines = false;
        n.expand = false;
        n.show_header = true;
        n.box_name = "HEAVY_HEAD";
        out.push(n);
    }
    for (r, row) in t.rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            for s in shrink_str(cell) {
                let mut n = t.clone();
                n.rows[r][c] = s;
                out.push(n);
            }
        }
    }
    out
}

/// A generated case: rebuild it with [`Case::generate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    pub seed: u64,
    pub index: u64,
    pub width: usize,
    pub node: Node,
}

impl Case {
    /// Case `index` of a run seeded `seed`.
    pub fn generate(seed: u64, index: u64, options: &GenOptions) -> Self {
        let mut rng = Rng::for_case(seed, index);
        let width = rng.range(options.min_width.max(1), options.max_width.max(1));
        let node = gen_node(&mut rng, options, 0);
        Case {
            seed,
            index,
            width,
            node,
        }
    }

    /// A Rust reproduction: a comment with the seed, the expression and the
    /// width.
    pub fn to_rust(&self) -> String {
        format!(
            "// seed {}, case {}, width {}\nlet renderable = {};",
            self.seed,
            self.index,
            self.width,
            self.node.to_rust()
        )
    }
}

/// What a custom invariant sees.
pub struct Rendered<'a> {
    pub node: &'a Node,
    pub width: usize,
    pub lines: &'a [String],
    pub segments: &'a [Segment],
}

type Check = Arc<dyn Fn(&Rendered<'_>) -> Result<(), String> + Send + Sync>;

/// The properties every case must have.
#[derive(Clone)]
pub struct Invariants {
    /// Rendering does not panic.
    pub no_panic: bool,
    /// No line is wider than the width, from [`Node::floor`] up, unless
    /// [`Node::allows_overflow`].
    pub fits_width: bool,
    /// A second render of a fresh build is identical.
    pub deterministic: bool,
    /// `measure()` has minimum ≤ maximum and no line's content (first to
    /// last non-space cell) is wider than the maximum clamped to the width.
    /// Skipped for text with tabs, which upstream measures unexpanded.
    pub measure_bounds: bool,
    custom: Vec<(String, Check)>,
}

impl Default for Invariants {
    fn default() -> Self {
        Invariants {
            no_panic: true,
            fits_width: true,
            deterministic: true,
            measure_bounds: true,
            custom: Vec::new(),
        }
    }
}

impl std::fmt::Debug for Invariants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invariants")
            .field("no_panic", &self.no_panic)
            .field("fits_width", &self.fits_width)
            .field("deterministic", &self.deterministic)
            .field("measure_bounds", &self.measure_bounds)
            .field(
                "custom",
                &self.custom.iter().map(|(n, _)| n).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Invariants {
    /// Only the custom checks added afterwards.
    pub fn none() -> Self {
        Invariants {
            no_panic: false,
            fits_width: false,
            deterministic: false,
            measure_bounds: false,
            custom: Vec::new(),
        }
    }
    pub fn fits_width(mut self, on: bool) -> Self {
        self.fits_width = on;
        self
    }
    pub fn measure_bounds(mut self, on: bool) -> Self {
        self.measure_bounds = on;
        self
    }
    /// Add a named check.
    pub fn custom(
        mut self,
        name: impl Into<String>,
        check: impl Fn(&Rendered<'_>) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.custom.push((name.into(), Arc::new(check)));
        self
    }

    /// The first invariant `node` breaks at `width`, as `(name, description)`.
    pub fn check(&self, node: &Node, width: usize) -> Option<(String, String)> {
        let probe = Probe::new(width);
        let render = || -> Result<Vec<Segment>, String> {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                probe.segments(&*node.build())
            }))
            .map_err(panic_message)
        };
        let segments = match render() {
            Ok(s) => s,
            Err(message) => {
                return self
                    .no_panic
                    .then(|| ("no_panic".to_owned(), format!("panicked: {message}")));
            }
        };
        let lines = plain_lines(&segments);
        if self.fits_width && width >= node.floor() && !node.allows_overflow() {
            if let Some((i, l)) = lines.iter().enumerate().find(|(_, l)| cell_len(l) > width) {
                return Some((
                    "fits_width".into(),
                    format!(
                        "line {} is {} cells wide at width {width}: {:?}",
                        i + 1,
                        cell_len(l),
                        l.trim_end()
                    ),
                ));
            }
        }
        if self.deterministic {
            match render() {
                Ok(again) if again == segments => {}
                Ok(_) => {
                    return Some((
                        "deterministic".into(),
                        "a second render differs from the first".into(),
                    ))
                }
                Err(message) => {
                    return Some((
                        "deterministic".into(),
                        format!("a second render panicked: {message}"),
                    ))
                }
            }
        }
        // Upstream's `Text` measure does not expand tabs, so text with tabs
        // renders wider than it measures; that is faithful, not a finding.
        if self.measure_bounds && !node.has_tabs() {
            let console = probe.target().console();
            let options = probe.options(&console);
            let measured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                node.build().measure(&console, &options)
            }));
            match measured {
                Err(payload) => {
                    return Some((
                        "measure_bounds".into(),
                        format!("measure panicked: {}", panic_message(payload)),
                    ))
                }
                Ok(m) if m.minimum > m.maximum => {
                    return Some((
                        "measure_bounds".into(),
                        format!("measure minimum {} > maximum {}", m.minimum, m.maximum),
                    ))
                }
                Ok(m) => {
                    let visible = lines.iter().map(|l| visible_width(l)).max().unwrap_or(0);
                    if visible > m.maximum.min(width) && visible <= width {
                        return Some((
                            "measure_bounds".into(),
                            format!(
                                "rendered {visible} cells wide but measure maximum is {}",
                                m.maximum
                            ),
                        ));
                    }
                }
            }
        }
        let rendered = Rendered {
            node,
            width,
            lines: &lines,
            segments: &segments,
        };
        for (name, check) in &self.custom {
            if let Err(message) = check(&rendered) {
                return Some((name.clone(), message));
            }
        }
        None
    }
}

/// A case that broke an invariant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub seed: u64,
    pub case_index: u64,
    pub width: usize,
    /// The invariant's name (`no_panic`, `fits_width`, … or a custom name).
    pub invariant: String,
    pub description: String,
    /// The case as generated.
    pub case: Case,
    /// The shrunk case, when shrinking made progress.
    pub minimized_case: Option<Case>,
    /// A Rust reproduction of the shrunk case.
    pub minimized: Option<String>,
}

/// The result of [`fuzz`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FuzzReport {
    pub seed: u64,
    pub cases: u64,
    pub failures: Vec<Failure>,
}

impl FuzzReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Shrink `case` while `invariants` still fail with `invariant`.
pub fn minimize(case: &Case, invariant: &str, invariants: &Invariants, budget: usize) -> Case {
    let mut best = case.clone();
    let mut spent = 0;
    let fails = |node: &Node, width: usize| {
        invariants
            .check(node, width)
            .is_some_and(|(name, _)| name == invariant)
    };
    'outer: loop {
        let mut candidates: Vec<(Node, usize)> = best
            .node
            .shrink()
            .into_iter()
            .map(|n| (n, best.width))
            .collect();
        for w in [1, best.width / 2, best.width.saturating_sub(1)] {
            if w >= 1 && w < best.width {
                candidates.push((best.node.clone(), w));
            }
        }
        for (node, width) in candidates {
            if spent >= budget {
                break 'outer;
            }
            spent += 1;
            if fails(&node, width) {
                best.node = node;
                best.width = width;
                continue 'outer;
            }
        }
        break;
    }
    best
}

/// Run `cases` generated cases from `seed` with default generation bounds.
pub fn fuzz(seed: u64, cases: u64, invariants: &Invariants) -> FuzzReport {
    fuzz_with(seed, cases, &GenOptions::default(), invariants)
}

/// [`fuzz`] with explicit generation bounds.
pub fn fuzz_with(
    seed: u64,
    cases: u64,
    options: &GenOptions,
    invariants: &Invariants,
) -> FuzzReport {
    let mut report = FuzzReport {
        seed,
        cases,
        failures: Vec::new(),
    };
    for index in 0..cases {
        let case = Case::generate(seed, index, options);
        if let Some((invariant, description)) = invariants.check(&case.node, case.width) {
            let small = minimize(&case, &invariant, invariants, options.shrink_budget);
            let shrunk = small != case;
            report.failures.push(Failure {
                seed,
                case_index: index,
                width: case.width,
                invariant,
                description,
                minimized: shrunk.then(|| small.to_rust()),
                minimized_case: shrunk.then_some(small),
                case,
            });
        }
    }
    report
}

impl Renderable for FuzzReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let table = (!self.failures.is_empty()).then(|| {
            let mut table = Table::new();
            for header in ["Case", "Width", "Invariant", "Description"] {
                table.add_column(header);
            }
            for f in &self.failures {
                let width = match &f.minimized_case {
                    Some(m) => format!("{} (min {})", f.width, m.width),
                    None => f.width.to_string(),
                };
                table.add_row_text(vec![
                    Text::new(f.case_index.to_string()),
                    Text::new(width),
                    Text::new(f.invariant.clone()),
                    Text::new(f.description.clone()),
                ]);
            }
            table
        });
        let summary = format!(
            "seed {}: {}, {}",
            self.seed,
            plural(self.cases as usize, "case"),
            plural(self.failures.len(), "failure")
        );
        table_then_line(table, summary, console, options)
    }
}
