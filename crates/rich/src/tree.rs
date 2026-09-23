//! Trees.
//!
//! Port of upstream `rich/tree.py`. A [`Tree`] renders a hierarchy with the
//! familiar `├──`/`└──` guide lines.
//!
//! Slice scope: the default (thin) guides, with string (markup), `Text` or
//! renderable labels. Custom `guide_style`/label styles, `hide_root`,
//! `expanded` and the ASCII/heavy guide sets are deferred with the rest of
//! `tree.py`.

use crate::cells::cell_len;
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

    /// Recursively render into `lines`. `prefix_first` precedes the label's first
    /// line; `prefix_rest` precedes wrapped continuation lines and is the base
    /// for this node's children.
    #[allow(clippy::too_many_arguments)]
    fn render_into(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        highlight: bool,
        lines: &mut Vec<Vec<Segment>>,
        prefix_first: &str,
        prefix_rest: &str,
        width: usize,
    ) {
        let guide_style = Some(Style::new());
        // Upstream renders the label at `options.max_width - sum(guide widths)`;
        // with no room left, `Console.render` yields nothing, so neither the
        // label nor its guides are emitted (its children still recurse).
        let available = width.saturating_sub(cell_len(prefix_first));
        // The label renders with `options.update(highlight=self.highlight)`,
        // so a string goes through `render_str` with the root's setting.
        let mut label_lines = if available < 1 {
            Vec::new()
        } else if let Cell::Renderable(renderable) = &self.label {
            let mut label_options = options.update_width(available);
            label_options.height = None;
            console.render_lines(renderable.as_ref(), &label_options, false)
        } else {
            self.label
                .to_text(console, Some(highlight))
                .unwrap_or_default()
                .render_lines(console.theme(), &Style::new(), Some(available))
        };
        if available >= 1 && label_lines.is_empty() {
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

        let last_index = self.children.len().saturating_sub(1);
        for (index, child) in self.children.iter().enumerate() {
            let last = index == last_index;
            let child_first = format!("{prefix_rest}{}", if last { END } else { FORK });
            let child_rest = format!("{prefix_rest}{}", if last { SPACE } else { CONTINUE });
            child.render_into(
                console,
                options,
                highlight,
                lines,
                &child_first,
                &child_rest,
                width,
            );
        }
    }
}

impl Renderable for Tree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        self.render_into(
            console,
            options,
            self.highlight,
            &mut lines,
            "",
            "",
            options.max_width,
        );

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
        fn walk(
            tree: &Tree,
            console: &Console,
            options: &ConsoleOptions,
            level: usize,
            width: &mut (usize, usize),
        ) {
            let label = tree.label.measure_cell(console, options);
            let indent = level * 4;
            width.0 = width.0.max(label.minimum + indent);
            width.1 = width.1.max(label.maximum + indent);
            for child in &tree.children {
                walk(child, console, options, level + 1, width);
            }
        }
        let mut width = (0, 0);
        walk(self, console, options, 0, &mut width);
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
}
