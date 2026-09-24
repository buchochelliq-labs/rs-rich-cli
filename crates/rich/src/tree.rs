//! Trees.
//!
//! Port of upstream `rich/tree.py`. A [`Tree`] renders a hierarchy with the
//! familiar `├──`/`└──` guide lines.
//!
//! Slice scope: the default (thin) guides, with string (markup), `Text` or
//! renderable labels. Custom `guide_style`/label styles, `hide_root`,
//! `expanded` and the ASCII/heavy guide sets are deferred with the rest of
//! `tree.py`.

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;
use crate::table::Cell;

// Default (thin) guide segments, matching `TREE_GUIDES[0]`.
const SPACE: &str = "    ";
const CONTINUE: &str = "│   ";
const FORK: &str = "├── ";
const END: &str = "└── ";

/// A node in a hierarchy. Mirrors `rich.tree.Tree`.
pub struct Tree {
    label: Cell,
    children: Vec<Tree>,
    highlight: bool,
}

impl Tree {
    /// A new tree/subtree with the given label. A string label is console
    /// markup, as upstream's `Tree("[b]root")` is; pass a
    /// [`Text`](crate::text::Text) for a literal one.
    pub fn new(label: impl Into<Cell>) -> Self {
        Tree {
            label: label.into(),
            children: Vec::new(),
            highlight: false,
        }
    }

    /// Highlight string labels (upstream `Tree(highlight=…)`, default off).
    /// Upstream renders every label with the root's setting.
    pub fn highlight(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
        self
    }

    /// Add a child with `label`, returning a mutable reference to it so further
    /// descendants can be attached. Mirrors `Tree.add`.
    pub fn add(&mut self, label: impl Into<Cell>) -> &mut Tree {
        self.children.push(Tree::new(label));
        self.children.last_mut().expect("just pushed a child")
    }

    /// Render this node's label into `lines`. `prefix_first` precedes the
    /// label's first line; `prefix_rest` precedes wrapped continuation lines.
    #[allow(clippy::too_many_arguments)]
    fn render_label(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        highlight: bool,
        lines: &mut Vec<Vec<Segment>>,
        prefix_first: &str,
        prefix_rest: &str,
        available: usize,
    ) {
        let guide_style = Some(Style::new());
        // The label renders with `options.update(highlight=self.highlight)`,
        // so a string goes through `render_str` with the root's setting.
        let mut label_lines = if let Cell::Renderable(renderable) = &self.label {
            let mut label_options = options.update_width(available);
            label_options.height = None;
            console.render_lines(renderable.as_ref(), &label_options, false)
        } else {
            self.label
                .to_text(console, Some(highlight))
                .unwrap_or_default()
                .render_lines(console.theme(), &Style::new(), Some(available))
        };
        if label_lines.is_empty() {
            label_lines.push(Vec::new());
        }

        for (index, label_line) in label_lines.into_iter().enumerate() {
            let prefix = if index == 0 {
                prefix_first
            } else {
                prefix_rest
            };
            let mut line = Vec::new();
            if !prefix.is_empty() {
                line.push(Segment::new(prefix.to_string(), guide_style.clone()));
            }
            line.extend(label_line);
            lines.push(line);
        }
    }

    /// Render the whole hierarchy into `lines`, depth first.
    ///
    /// Like upstream's `__rich_console__`, this walks an explicit stack rather
    /// than recursing, so a very deep tree cannot overflow the thread stack.
    /// The guides are kept as one `is_last` flag per level and only turned
    /// into prefix strings for nodes that still have room to render: a guide
    /// is four cells per level, so materialising every prefix would cost
    /// quadratic memory on a deep chain.
    fn render_into(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        highlight: bool,
        lines: &mut Vec<Vec<Segment>>,
        width: usize,
    ) {
        // `(node, index of the next child to visit)`; `levels[d]` is whether
        // the ancestor at depth `d + 1` on the current path is a last child.
        let mut stack: Vec<(&Tree, usize)> = vec![(self, 0)];
        let mut levels: Vec<bool> = Vec::new();
        let visit = |node: &Tree, levels: &[bool], lines: &mut Vec<Vec<Segment>>| {
            // Upstream renders the label at `options.max_width - sum(guide
            // widths)`; with no room left, `Console.render` yields nothing, so
            // neither the label nor its guides are emitted (its children
            // still render, and get no room either).
            let guide_width = levels.len() * 4;
            if guide_width >= width {
                return;
            }
            let mut prefix_rest = String::new();
            for &last in levels {
                prefix_rest.push_str(if last { SPACE } else { CONTINUE });
            }
            let mut prefix_first = String::new();
            if let Some((&last, parents)) = levels.split_last() {
                for &parent_last in parents {
                    prefix_first.push_str(if parent_last { SPACE } else { CONTINUE });
                }
                prefix_first.push_str(if last { END } else { FORK });
            }
            node.render_label(
                console,
                options,
                highlight,
                lines,
                &prefix_first,
                &prefix_rest,
                width - guide_width,
            );
        };
        visit(self, &levels, lines);
        while let Some((node, next)) = stack.last_mut() {
            let node: &Tree = node;
            if let Some(child) = node.children.get(*next) {
                *next += 1;
                levels.push(*next == node.children.len());
                visit(child, &levels, lines);
                stack.push((child, 0));
            } else {
                stack.pop();
                levels.pop();
            }
        }
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
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        self.render_into(console, options, self.highlight, &mut lines, options.max_width);

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
            pending.extend(tree.children.iter().map(|child| (child, level + 1)));
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
