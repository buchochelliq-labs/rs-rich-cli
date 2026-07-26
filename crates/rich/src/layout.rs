//! Screen layout — split a region into ratioed rows and columns.
//!
//! Port of `rich/layout.py` (core). A [`Layout`] is a tree: a leaf holds a
//! renderable, a branch splits its region among children either into columns
//! (stacked vertically via [`Layout::split_column`]) or rows (side by side via
//! [`Layout::split_row`]). Region sizes come from [`ratio_resolve`], and each
//! leaf is rendered to an exact `(width, height)` block, then tiled.
//!
//! Scope: sizing (`size`/`ratio`/`minimum_size`), row/column splits, and leaf
//! rendering are ported. The interactive placeholder shown for an *empty* leaf
//! (upstream's fancy `_Placeholder`) is rendered as blank space (see
//! docs/DIVERGENCES.md).

use crate::console::{Console, ConsoleOptions, Justify};
use crate::protocol::Renderable;
use crate::ratio::{ratio_resolve, Edge};
use crate::segment::Segment;

/// The axis along which a branch splits its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// Children are stacked vertically (a `split_column`).
    Column,
    /// Children are placed side by side (a `split_row`).
    Row,
}

/// A node in a layout tree. Mirrors `rich.layout.Layout`.
pub struct Layout {
    renderable: Option<Box<dyn Renderable>>,
    children: Vec<Layout>,
    direction: Direction,
    /// A fixed size along the parent's split axis, if pinned.
    size: Option<usize>,
    /// Flex weight when unsized (defaults to 1).
    ratio: usize,
    /// The smallest size this region may shrink to.
    minimum_size: usize,
}

impl Default for Layout {
    fn default() -> Self {
        Layout::new()
    }
}

impl Layout {
    /// An empty layout (no renderable, no children).
    pub fn new() -> Self {
        Layout {
            renderable: None,
            children: Vec::new(),
            direction: Direction::Column,
            size: None,
            ratio: 1,
            minimum_size: 1,
        }
    }

    /// A leaf layout wrapping `renderable`.
    pub fn with_renderable(renderable: Box<dyn Renderable>) -> Self {
        let mut layout = Layout::new();
        layout.renderable = Some(renderable);
        layout
    }

    /// Pin this region to a fixed size along its parent's split axis.
    pub fn size(mut self, size: usize) -> Self {
        self.size = Some(size);
        self
    }

    /// Set the flex weight used when this region is unsized.
    pub fn ratio(mut self, ratio: usize) -> Self {
        self.ratio = ratio;
        self
    }

    /// Set the minimum size this region may shrink to.
    pub fn minimum_size(mut self, minimum_size: usize) -> Self {
        self.minimum_size = minimum_size;
        self
    }

    /// Split into children stacked vertically. Port of `Layout.split_column`.
    pub fn split_column(&mut self, children: Vec<Layout>) {
        self.direction = Direction::Column;
        self.children = children;
    }

    /// Split into children placed side by side. Port of `Layout.split_row`.
    pub fn split_row(&mut self, children: Vec<Layout>) {
        self.direction = Direction::Row;
        self.children = children;
    }

    fn edge(&self) -> Edge {
        Edge::new(self.size, self.ratio, self.minimum_size)
    }

    /// Render this node into exactly `height` lines, each `width` cells wide.
    fn render_region(&self, console: &Console, width: usize, height: usize) -> Vec<Vec<Segment>> {
        if self.children.is_empty() {
            return self.render_leaf(console, width, height);
        }
        let edges: Vec<Edge> = self.children.iter().map(Layout::edge).collect();
        match self.direction {
            Direction::Column => {
                // Divide the height; stack the children's line blocks.
                let heights = ratio_resolve(height, &edges);
                let mut lines = Vec::with_capacity(height);
                for (child, child_height) in self.children.iter().zip(heights) {
                    lines.extend(child.render_region(console, width, child_height));
                }
                lines
            }
            Direction::Row => {
                // Divide the width; place the children's blocks side by side.
                let widths = ratio_resolve(width, &edges);
                let blocks: Vec<Vec<Vec<Segment>>> = self
                    .children
                    .iter()
                    .zip(widths)
                    .map(|(child, child_width)| child.render_region(console, child_width, height))
                    .collect();
                (0..height)
                    .map(|y| {
                        let mut row = Vec::new();
                        for block in &blocks {
                            row.extend(block[y].iter().cloned());
                        }
                        row
                    })
                    .collect()
            }
        }
    }

    /// Render a leaf renderable to a `(width, height)` block (blank if empty).
    fn render_leaf(&self, console: &Console, width: usize, height: usize) -> Vec<Vec<Segment>> {
        let lines = match &self.renderable {
            Some(renderable) => {
                let mut options = console.options().update_dimensions(width, height);
                options.justify = Justify::Default;
                console.render_lines(renderable.as_ref(), &options, true)
            }
            None => Vec::new(),
        };
        Segment::set_shape(lines, width, height)
    }
}

impl Renderable for Layout {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let height = options.height.unwrap_or_else(|| console.height());
        let lines = self.render_region(console, width, height);

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
    use crate::text::Text;

    fn console(width: usize, height: usize) -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .height(height)
            .build()
    }

    fn leaf(s: &str) -> Layout {
        Layout::with_renderable(Box::new(Text::new(s)))
    }

    /// A line of `text` left-justified into `width` cells.
    fn cell(text: &str, width: usize) -> String {
        format!("{text}{}", " ".repeat(width - text.chars().count()))
    }

    #[test]
    fn column_split_stacks() {
        let c = console(24, 4);
        let mut lay = Layout::new();
        lay.split_column(vec![leaf("top"), leaf("bottom")]);
        // Captured from real rich 15.0.0: two ratio-1 rows over height 4.
        let blank = " ".repeat(24);
        let expected = format!(
            "{}\n{blank}\n{}\n{blank}\n",
            cell("top", 24),
            cell("bottom", 24)
        );
        assert_eq!(c.capture(|con| con.print(&lay)), expected);
    }

    #[test]
    fn row_split_side_by_side() {
        let c = console(24, 4);
        let mut lay = Layout::new();
        lay.split_row(vec![leaf("L"), leaf("R")]);
        // Two ratio-1 columns of width 12; only row 0 has content.
        let blank = " ".repeat(24);
        let row0 = format!("{}{}", cell("L", 12), cell("R", 12));
        let expected = format!("{row0}\n{blank}\n{blank}\n{blank}\n");
        assert_eq!(c.capture(|con| con.print(&lay)), expected);
    }
}
