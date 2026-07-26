//! Trees.
//!
//! Port of upstream `rich/tree.py`. A [`Tree`] renders a hierarchy with the
//! familiar `├──`/`└──` guide lines.
//!
//! Slice scope: the default (thin) guides and plain labels. Custom
//! `guide_style`/label styles and the ASCII/heavy guide sets are deferred with
//! the rest of `tree.py`.

use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;

// Default (thin) guide segments, matching `TREE_GUIDES[0]`.
const SPACE: &str = "    ";
const CONTINUE: &str = "│   ";
const FORK: &str = "├── ";
const END: &str = "└── ";

/// A node in a hierarchy. Mirrors `rich.tree.Tree`.
pub struct Tree {
    label: String,
    children: Vec<Tree>,
}

impl Tree {
    /// A new tree/subtree with the given label.
    pub fn new(label: impl Into<String>) -> Self {
        Tree {
            label: label.into(),
            children: Vec::new(),
        }
    }

    /// Add a child with `label`, returning a mutable reference to it so further
    /// descendants can be attached. Mirrors `Tree.add`.
    pub fn add(&mut self, label: impl Into<String>) -> &mut Tree {
        self.children.push(Tree::new(label));
        self.children.last_mut().expect("just pushed a child")
    }

    /// Recursively render into `lines`. `prefix_first` precedes the label's first
    /// line; `prefix_rest` precedes wrapped continuation lines and is the base
    /// for this node's children.
    fn render_into(
        &self,
        lines: &mut Vec<Vec<Segment>>,
        prefix_first: &str,
        prefix_rest: &str,
        width: usize,
    ) {
        let guide_style = Some(Style::new());
        let available = width.saturating_sub(cell_len(prefix_first));
        let mut label_lines = Text::new(&self.label).render_lines(&Style::new(), Some(available));
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

        let last_index = self.children.len().saturating_sub(1);
        for (index, child) in self.children.iter().enumerate() {
            let last = index == last_index;
            let child_first = format!("{prefix_rest}{}", if last { END } else { FORK });
            let child_rest = format!("{prefix_rest}{}", if last { SPACE } else { CONTINUE });
            child.render_into(lines, &child_first, &child_rest, width);
        }
    }
}

impl Renderable for Tree {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        self.render_into(&mut lines, "", "", options.max_width);

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
