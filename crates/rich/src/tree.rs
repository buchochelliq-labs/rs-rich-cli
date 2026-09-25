//! Trees.
//!
//! Port of upstream `rich/tree.py`. A [`Tree`] renders a hierarchy with the
//! familiar `├──`/`└──` guide lines: string (markup), `Text` or renderable
//! labels, per-node `style`/`guide_style` (a bold guide style draws heavy
//! guides, `underline2` double ones, and an ASCII-only console ASCII ones),
//! collapsed (`expanded = false`) nodes and a hidden root.

use crate::console::Justify;
use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::{Style, StyleType};
use crate::table::Cell;

/// `Tree.ASCII_GUIDES`: the guides on a console that cannot encode Unicode.
pub const ASCII_GUIDES: [&str; 4] = ["    ", "|   ", "+-- ", "`-- "];
/// `Tree.TREE_GUIDES`: thin, heavy (bold) and double (underline2) guides.
pub const TREE_GUIDES: [[&str; 4]; 3] = [
    ["    ", "│   ", "├── ", "└── "],
    ["    ", "┃   ", "┣━━ ", "┗━━ "],
    ["    ", "║   ", "╠══ ", "╚══ "],
];

// Indexes into a guide set, as upstream's `SPACE, CONTINUE, FORK, END`.
const SPACE: usize = 0;
const CONTINUE: usize = 1;
const FORK: usize = 2;
const END: usize = 3;

/// A node in a hierarchy. Mirrors `rich.tree.Tree`.
pub struct Tree {
    label: Cell,
    style: StyleType,
    guide_style: StyleType,
    children: Vec<Tree>,
    expanded: bool,
    highlight: bool,
    hide_root: bool,
}

impl Tree {
    /// A new tree/subtree with the given label. A string label is console
    /// markup, as upstream's `Tree("[b]root")` is; pass a
    /// [`Text`](crate::text::Text) for a literal one.
    pub fn new(label: impl Into<Cell>) -> Self {
        Tree {
            label: label.into(),
            style: StyleType::Name("tree".to_string()),
            guide_style: StyleType::Name("tree.line".to_string()),
            children: Vec::new(),
            expanded: true,
            highlight: false,
            hide_root: false,
        }
    }

    /// Highlight string labels (upstream `Tree(highlight=…)`, default off).
    /// Upstream renders every label with the root's setting.
    pub fn highlight(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
        self
    }

    /// The style of this node's label and, stacked, its descendants'
    /// (upstream `Tree(style=…)`, default `"tree"`).
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = style.into();
        self
    }

    /// The style of the guide lines below this node (upstream
    /// `Tree(guide_style=…)`, default `"tree.line"`). Bold selects the heavy
    /// guides and `underline2` the double ones.
    pub fn guide_style(mut self, style: impl Into<StyleType>) -> Self {
        self.guide_style = style.into();
        self
    }

    /// Whether this node's children are shown (upstream `expanded`, default
    /// on).
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Hide the root node, rendering its children as the top level (upstream
    /// `hide_root`, default off; read from the root only).
    pub fn hide_root(mut self, hide_root: bool) -> Self {
        self.hide_root = hide_root;
        self
    }

    /// Set [`style`](Self::style) in place.
    pub fn set_style(&mut self, style: impl Into<StyleType>) -> &mut Self {
        self.style = style.into();
        self
    }

    /// Set [`guide_style`](Self::guide_style) in place.
    pub fn set_guide_style(&mut self, style: impl Into<StyleType>) -> &mut Self {
        self.guide_style = style.into();
        self
    }

    /// Set [`expanded`](Self::expanded) in place (upstream assigns
    /// `tree.expanded`).
    pub fn set_expanded(&mut self, expanded: bool) -> &mut Self {
        self.expanded = expanded;
        self
    }

    /// Set [`hide_root`](Self::hide_root) in place.
    pub fn set_hide_root(&mut self, hide_root: bool) -> &mut Self {
        self.hide_root = hide_root;
        self
    }

    /// Set [`highlight`](Self::highlight) in place.
    pub fn set_highlight(&mut self, highlight: bool) -> &mut Self {
        self.highlight = highlight;
        self
    }

    /// Replace the label in place.
    pub fn set_label(&mut self, label: impl Into<Cell>) -> &mut Self {
        self.label = label.into();
        self
    }

    /// The label.
    pub fn label(&self) -> &Cell {
        &self.label
    }

    /// The node's style (see [`style`](Self::style)).
    pub fn get_style(&self) -> &StyleType {
        &self.style
    }

    /// The node's guide style (see [`guide_style`](Self::guide_style)).
    pub fn get_guide_style(&self) -> &StyleType {
        &self.guide_style
    }

    /// Whether children are shown.
    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Whether the root is hidden.
    pub fn is_root_hidden(&self) -> bool {
        self.hide_root
    }

    /// Whether string labels are highlighted.
    pub fn is_highlighted(&self) -> bool {
        self.highlight
    }

    /// The child nodes.
    pub fn children(&self) -> &[Tree] {
        &self.children
    }

    /// The child nodes, mutably.
    pub fn children_mut(&mut self) -> &mut Vec<Tree> {
        &mut self.children
    }

    /// Add a child with `label`, returning a mutable reference to it so further
    /// descendants can be attached. Mirrors `Tree.add` with its defaults: the
    /// child inherits this node's `style` and `guide_style` and is expanded.
    pub fn add(&mut self, label: impl Into<Cell>) -> &mut Tree {
        let mut child = Tree::new(label);
        child.style = self.style.clone();
        child.guide_style = self.guide_style.clone();
        self.add_tree(child)
    }

    /// Attach an already built subtree as the last child, returning it. Use
    /// it for upstream's `Tree.add(label, style=…, guide_style=…,
    /// expanded=…)`.
    pub fn add_tree(&mut self, child: Tree) -> &mut Tree {
        self.children.push(child);
        self.children.last_mut().expect("just pushed a child")
    }

    /// `make_guide`: the guide segment at `index` for a level in `style`.
    fn make_guide(options: &ConsoleOptions, index: usize, style: Style) -> Segment {
        let line = if options.ascii_only() {
            ASCII_GUIDES[index]
        } else {
            let guide = if style.attr(BOLD) == Some(true) {
                1
            } else if style.attr(UNDERLINE2) == Some(true) {
                2
            } else {
                0
            };
            TREE_GUIDES[if options.legacy_windows { 0 } else { guide }][index]
        };
        Segment::new(line, Some(style))
    }
}

// Attribute indexes in `Style` (bold, underline2).
const BOLD: usize = 0;
const UNDERLINE2: usize = 9;

/// `Styled(node.label, style)` for the render loop: the label rendered as
/// upstream renders a `str`/`Text`/renderable, with `style` beneath it.
struct Label<'a> {
    label: &'a Cell,
    style: &'a Style,
    highlight: bool,
}

impl Renderable for Label<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let segments = match self.label {
            Cell::Renderable(renderable) => renderable.rich_render(console, options),
            cell => cell
                .to_text(console, Some(self.highlight))
                .unwrap_or_default()
                .rich_render(console, options),
        };
        if self.style.is_null() {
            segments
        } else {
            Segment::apply_style(&segments, self.style)
        }
    }
}

impl Tree {
    /// Port of `Tree.__rich_console__`, as lines.
    ///
    /// Like upstream, this walks an explicit stack rather than recursing, so
    /// a very deep tree cannot overflow the thread stack. The prefix (four
    /// cells per level) is only copied for nodes that still have room to
    /// render: materialising every prefix would cost quadratic memory on a
    /// deep chain.
    fn render_lines_into(&self, console: &Console, options: &ConsoleOptions) -> Vec<Vec<Segment>> {
        let get_style = |style: &StyleType| console.get_style(style).unwrap_or_default();
        let guide_style = get_style(&self.guide_style);
        let mut levels: Vec<Segment> =
            vec![Self::make_guide(options, CONTINUE, guide_style.clone())];
        let mut stack: Vec<(&[Tree], usize)> = vec![(std::slice::from_ref(self), 0)];
        let mut guide_style_stack = vec![guide_style];
        let mut style_stack = vec![get_style(&self.style)];
        let remove_guide_styles = Style::parse("not bold not underline2").unwrap_or_default();
        let offset = if self.hide_root { 2 } else { 1 };
        let pad = options.justify != Justify::Default;
        let mut depth = 0usize;
        let mut lines = Vec::new();
        let level_style = |segment: &Segment| segment.style.clone().unwrap_or_default();

        while let Some((siblings, next)) = stack.last_mut() {
            let siblings: &[Tree] = siblings;
            let Some(node) = siblings.get(*next) else {
                stack.pop();
                levels.pop();
                if let Some(level) = levels.last_mut() {
                    let style = level_style(level);
                    *level = Self::make_guide(options, FORK, style);
                    guide_style_stack.pop();
                    style_stack.pop();
                }
                continue;
            };
            *next += 1;
            let last = *next == siblings.len();
            if last {
                let level = levels.last_mut().expect("a level per open stack entry");
                *level = Self::make_guide(options, END, level_style(level));
            }

            let current_guide = guide_style_stack.last().cloned().unwrap_or_default();
            let current_style = style_stack.last().cloned().unwrap_or_default();
            let node_guide_style = current_guide.combine(&get_style(&node.guide_style));
            let style = current_style.combine(&get_style(&node.style));
            let prefix_width = levels.len().saturating_sub(offset) * 4;

            if !(depth == 0 && self.hide_root) && prefix_width < options.max_width {
                let mut prefix: Vec<Segment> = levels[offset.min(levels.len())..].to_vec();
                let mut label_options = options.update_width(options.max_width - prefix_width);
                label_options.highlight = Some(self.highlight);
                label_options.height = None;
                let label = Label {
                    label: &node.label,
                    style: &style,
                    highlight: self.highlight,
                };
                let background = Style::from_color(None, style.bgcolor().cloned());
                for (index, label_line) in console
                    .render_lines(&label, &label_options, pad)
                    .into_iter()
                    .enumerate()
                {
                    let mut line = Vec::new();
                    if !prefix.is_empty() {
                        line.extend(prefix.iter().map(|segment| {
                            let own = segment.style.clone().unwrap_or_default();
                            let own = if background.is_null() {
                                own
                            } else {
                                background.combine(&own)
                            };
                            let style = if own.is_null() {
                                remove_guide_styles.clone()
                            } else {
                                own.combine(&remove_guide_styles)
                            };
                            Segment::new(segment.text.clone(), Some(style))
                        }));
                    }
                    line.extend(label_line);
                    lines.push(line);
                    if index == 0 && !prefix.is_empty() {
                        let level = prefix.last_mut().expect("non-empty prefix");
                        *level = Self::make_guide(
                            options,
                            if last { SPACE } else { CONTINUE },
                            level_style(level),
                        );
                    }
                }
            }

            if node.expanded && !node.children.is_empty() {
                let level = levels.last_mut().expect("a level per open stack entry");
                *level = Self::make_guide(
                    options,
                    if last { SPACE } else { CONTINUE },
                    level_style(level),
                );
                levels.push(Self::make_guide(
                    options,
                    if node.children.len() == 1 { END } else { FORK },
                    node_guide_style,
                ));
                style_stack.push(current_style.combine(&get_style(&node.style)));
                guide_style_stack.push(current_guide.combine(&get_style(&node.guide_style)));
                stack.push((&node.children, 0));
                depth += 1;
            }
        }
        lines
    }
}

impl Drop for Tree {
    /// Drop descendants from an explicit stack: the derived drop recurses
    /// once per level and overflows the stack on a very deep tree.
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut child) = pending.pop() {
            pending.append(&mut child.children);
        }
    }
}

impl Renderable for Tree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let lines = self.render_lines_into(console, options);
        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }

    /// Port of `Tree.__rich_measure__`: the widest label plus its indent of
    /// four cells per level, for both bounds.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        // Iterative, like the render: `(node, level)` pairs to visit.
        let mut width = (0, 0);
        let mut pending: Vec<(&Tree, usize)> = vec![(self, 0)];
        while let Some((tree, level)) = pending.pop() {
            let label = tree.label.measure_cell(console, options);
            let indent = level * 4;
            width.0 = width.0.max(label.minimum + indent);
            width.1 = width.1.max(label.maximum + indent);
            if tree.expanded {
                pending.extend(tree.children.iter().map(|child| (child, level + 1)));
            }
        }
        Measurement::new(width.0, width.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .build()
    }

    #[test]
    fn nested_tree() {
        let mut tree = Tree::new("root");
        let a = tree.add("child A");
        a.add("leaf A1");
        a.add("leaf A2");
        tree.add("child B");
        let out = console().render_export(&tree);
        let expected = concat!(
            "root\n",
            "├── child A\n",
            "│   ├── leaf A1\n",
            "│   └── leaf A2\n",
            "└── child B\n",
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn deep_tree_renders_without_recursion() {
        // Upstream renders from an explicit stack, so a 20 000-level chain is
        // fine; a recursive render (or drop, or measure) overflows a normal
        // 2 MiB thread stack long before that.
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let mut tree = Tree::new("0");
                let mut node = &mut tree;
                for depth in 1..20_000 {
                    node = node.add(depth.to_string());
                }
                let console = Console::builder().width(12).build();
                let out = console.render_export(&tree);
                let measured = Measurement::get(&console, &console.options(), &tree);
                (out, measured)
            })
            .expect("spawn");
        let (out, measured) = handle.join().expect("deep tree render overflowed");
        // Labels render while the guides leave room (4 cells per level at
        // width 12: depths 0-2); deeper nodes have no room and emit nothing.
        assert_eq!(out, "0\n└── 1\n    └── 2\n");
        // `Measurement.get` clamps the 80 001-cell measure to the width.
        assert_eq!(measured, Measurement::new(12, 12));
    }
}
