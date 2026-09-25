//! Screen layout — split a region into ratioed rows and columns.
//!
//! Port of `rich/layout.py`. A [`Layout`] is a tree: a leaf holds a
//! renderable, a branch splits its region among its *visible* children
//! either into rows (side by side, [`Splitter::Row`]) or columns (stacked,
//! [`Splitter::Column`]). Region sizes come from [`ratio_resolve`], and each
//! leaf is rendered to an exact `(width, height)` block, then tiled. A leaf
//! with no renderable shows upstream's placeholder panel; the last render's
//! regions are kept in [`Layout::map`].

use std::sync::{Arc, Mutex};

use crate::align::{Align, VerticalAlign};
use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions};
use crate::highlighter::ReprHighlighter;
use crate::measure::Measurement;
use crate::panel::Panel;
use crate::protocol::{Highlighter, Renderable};
use crate::ratio::{ratio_resolve, Edge};
use crate::region::Region;
use crate::segment::Segment;
use crate::style::Style;
use crate::table::{Cell, Table};
use crate::text::Text;
use crate::tree::Tree;

/// How a layout divides its region among its children. Mirrors upstream's
/// `RowSplitter` / `ColumnSplitter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Splitter {
    /// Children are placed side by side (`split_row`).
    Row,
    /// Children are stacked vertically (`split_column`, the default).
    #[default]
    Column,
}

impl Splitter {
    /// The splitter's name (`Splitter.name`): `"row"` or `"column"`.
    pub fn name(self) -> &'static str {
        match self {
            Splitter::Row => "row",
            Splitter::Column => "column",
        }
    }

    /// The icon markup `Layout.tree` shows (`Splitter.get_tree_icon`).
    pub fn tree_icon(self) -> &'static str {
        match self {
            Splitter::Row => "[layout.tree.row]⬌",
            Splitter::Column => "[layout.tree.column]⬍",
        }
    }

    /// A splitter by name (`Layout.splitters[name]`).
    pub fn from_name(name: &str) -> Option<Splitter> {
        match name {
            "row" => Some(Splitter::Row),
            "column" => Some(Splitter::Column),
            _ => None,
        }
    }

    /// `Splitter.divide`: `region` shared among `children`, in order.
    fn divide(self, children: &[&Layout], region: Region) -> Vec<Region> {
        let edges: Vec<Edge> = children.iter().map(|child| child.edge()).collect();
        let Region {
            x,
            y,
            width,
            height,
        } = region;
        let mut offset = 0;
        match self {
            Splitter::Row => ratio_resolve(width, &edges)
                .into_iter()
                .map(|child_width| {
                    let region = Region::new(x + offset, y, child_width, height);
                    offset += child_width;
                    region
                })
                .collect(),
            Splitter::Column => ratio_resolve(height, &edges)
                .into_iter()
                .map(|child_height| {
                    let region = Region::new(x, y + offset, width, child_height);
                    offset += child_height;
                    region
                })
                .collect(),
        }
    }
}

/// One leaf of a render. Mirrors upstream's `LayoutRender`, keyed by the
/// layout's `name` and its `path` (child indexes from the root, counting
/// hidden children) since the port has no object identity to key by.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutRender {
    /// The leaf's name, if it has one.
    pub name: Option<String>,
    /// Child indexes from the root layout to the leaf.
    pub path: Vec<usize>,
    /// Where the leaf was drawn.
    pub region: Region,
    /// Its rendered lines.
    pub render: Vec<Vec<Segment>>,
}

/// A node in a layout tree. Mirrors `rich.layout.Layout`.
pub struct Layout {
    renderable: Option<Box<dyn Renderable>>,
    name: Option<String>,
    visible: bool,
    children: Vec<Layout>,
    splitter: Splitter,
    /// A fixed size along the parent's split axis, if pinned.
    size: Option<usize>,
    /// Flex weight when unsized (defaults to 1).
    ratio: usize,
    /// The smallest size this region may shrink to.
    minimum_size: usize,
    /// The last render (upstream `_render_map`).
    render_map: Mutex<Vec<LayoutRender>>,
}

impl Default for Layout {
    fn default() -> Self {
        Layout::new()
    }
}

impl Layout {
    /// An empty layout (no renderable, so it shows the placeholder; no
    /// children).
    pub fn new() -> Self {
        Layout {
            renderable: None,
            name: None,
            visible: true,
            children: Vec::new(),
            splitter: Splitter::Column,
            size: None,
            ratio: 1,
            minimum_size: 1,
            render_map: Mutex::new(Vec::new()),
        }
    }

    /// A leaf layout wrapping `renderable`.
    pub fn with_renderable(renderable: Box<dyn Renderable>) -> Self {
        let mut layout = Layout::new();
        layout.renderable = Some(renderable);
        layout
    }

    /// Name this layout, for [`get`](Self::get) (upstream `name`).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Show or hide this layout (upstream `visible`, default on). A hidden
    /// child takes no space in its parent's split.
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
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

    /// Set (or clear) the name in place.
    pub fn set_name(&mut self, name: Option<String>) -> &mut Self {
        self.name = name;
        self
    }

    /// Set the visibility in place.
    pub fn set_visible(&mut self, visible: bool) -> &mut Self {
        self.visible = visible;
        self
    }

    /// Set (or clear) the fixed size in place.
    pub fn set_size(&mut self, size: Option<usize>) -> &mut Self {
        self.size = size;
        self
    }

    /// Set the ratio in place.
    pub fn set_ratio(&mut self, ratio: usize) -> &mut Self {
        self.ratio = ratio;
        self
    }

    /// Set the minimum size in place.
    pub fn set_minimum_size(&mut self, minimum_size: usize) -> &mut Self {
        self.minimum_size = minimum_size;
        self
    }

    /// The name, if any.
    pub fn get_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Whether this layout is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// The fixed size, if any.
    pub fn get_size(&self) -> Option<usize> {
        self.size
    }

    /// The ratio.
    pub fn get_ratio(&self) -> usize {
        self.ratio
    }

    /// The minimum size.
    pub fn get_minimum_size(&self) -> usize {
        self.minimum_size
    }

    /// The splitter dividing this layout among its children.
    pub fn splitter(&self) -> Splitter {
        self.splitter
    }

    /// The leaf renderable, if one is set (none shows the placeholder).
    pub fn renderable(&self) -> Option<&dyn Renderable> {
        self.renderable.as_deref()
    }

    /// Replace the leaf renderable. Port of `Layout.update`.
    pub fn update(&mut self, renderable: Box<dyn Renderable>) {
        self.renderable = Some(renderable);
    }

    /// The visible children (upstream's `children` property).
    pub fn children(&self) -> Vec<&Layout> {
        self.children.iter().filter(|child| child.visible).collect()
    }

    /// Every child, hidden ones included (upstream `_children`).
    pub fn all_children(&self) -> &[Layout] {
        &self.children
    }

    /// Every child, mutably.
    pub fn all_children_mut(&mut self) -> &mut Vec<Layout> {
        &mut self.children
    }

    /// Split into `children` with `splitter`. Port of `Layout.split`.
    pub fn split(&mut self, children: Vec<Layout>, splitter: Splitter) {
        self.splitter = splitter;
        self.children = children;
    }

    /// Split into children stacked vertically. Port of `Layout.split_column`.
    pub fn split_column(&mut self, children: Vec<Layout>) {
        self.split(children, Splitter::Column);
    }

    /// Split into children placed side by side. Port of `Layout.split_row`.
    pub fn split_row(&mut self, children: Vec<Layout>) {
        self.split(children, Splitter::Row);
    }

    /// Add children to the existing split. Port of `Layout.add_split`.
    pub fn add_split(&mut self, children: Vec<Layout>) {
        self.children.extend(children);
    }

    /// Remove every child. Port of `Layout.unsplit`.
    pub fn unsplit(&mut self) {
        self.children.clear();
    }

    /// The first layout named `name`, depth first from this one. Port of
    /// `Layout.get`.
    pub fn get(&self, name: &str) -> Option<&Layout> {
        if self.name.as_deref() == Some(name) {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.get(name))
    }

    /// [`get`](Self::get), mutably.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Layout> {
        if self.name.as_deref() == Some(name) {
            return Some(self);
        }
        self.children
            .iter_mut()
            .find_map(|child| child.get_mut(name))
    }

    /// The layout at `path` (child indexes, hidden children counted), as
    /// [`LayoutRender::path`] records it.
    pub fn at_path(&self, path: &[usize]) -> Option<&Layout> {
        path.iter()
            .try_fold(self, |layout, &index| layout.children.get(index))
    }

    fn edge(&self) -> Edge {
        Edge::new(self.size, self.ratio, self.minimum_size)
    }

    /// Upstream's `__rich_repr__` fields that differ from their defaults, as
    /// `key=value` reprs.
    fn repr_fields(&self) -> Vec<String> {
        let mut fields = Vec::new();
        if let Some(name) = &self.name {
            fields.push(format!("name={}", py_repr_str(name)));
        }
        if let Some(size) = self.size {
            fields.push(format!("size={size}"));
        }
        if self.minimum_size != 1 {
            fields.push(format!("minimum_size={}", self.minimum_size));
        }
        if self.ratio != 1 {
            fields.push(format!("ratio={}", self.ratio));
        }
        fields
    }

    /// A tree renderable showing the layout's structure. Port of
    /// `Layout.tree`.
    pub fn tree(&self) -> Tree {
        fn summary(layout: &Layout) -> Cell {
            let mut table = Table::grid().padding(0, 1, 0, 0);
            table.add_column("").add_column("");
            table.add_row_cells(vec![
                Cell::Markup(layout.splitter.tree_icon().to_string()),
                Cell::Renderable(Arc::new(LayoutRepr {
                    fields: layout.repr_fields(),
                    dim: !layout.visible,
                })),
            ]);
            Cell::Renderable(Arc::new(table))
        }
        fn recurse(tree: &mut Tree, layout: &Layout) {
            for child in &layout.children {
                let node = tree.add(summary(child));
                node.set_guide_style(format!("layout.tree.{}", child.splitter.name()));
                recurse(node, child);
            }
        }
        let mut tree = Tree::new(summary(self))
            .guide_style(format!("layout.tree.{}", self.splitter.name()))
            .highlight(true);
        recurse(&mut tree, self);
        tree
    }

    /// Every layout's region for a `width` by `height` render, as `(path,
    /// region)` sorted by region. Port of `Layout._make_region_map`.
    pub fn region_map(&self, width: usize, height: usize) -> Vec<(Vec<usize>, Region)> {
        let mut stack: Vec<(Vec<usize>, &Layout, Region)> =
            vec![(Vec::new(), self, Region::new(0, 0, width, height))];
        let mut regions: Vec<(Vec<usize>, &Layout, Region)> = Vec::new();
        while let Some(entry) = stack.pop() {
            let (path, layout, region) = &entry;
            let visible: Vec<(usize, &Layout)> = layout
                .children
                .iter()
                .enumerate()
                .filter(|(_, child)| child.visible)
                .collect();
            if !visible.is_empty() {
                let children: Vec<&Layout> = visible.iter().map(|(_, child)| *child).collect();
                for ((index, child), child_region) in visible
                    .iter()
                    .zip(layout.splitter.divide(&children, *region))
                {
                    let mut child_path = path.clone();
                    child_path.push(*index);
                    stack.push((child_path, child, child_region));
                }
            }
            regions.push(entry);
        }
        regions.sort_by_key(|(_, _, region)| *region);
        regions
            .into_iter()
            .map(|(path, _, region)| (path, region))
            .collect()
    }

    /// Render every leaf into its region. Port of `Layout.render`: `options`
    /// give the width and (else the console's) height.
    pub fn render_regions(&self, console: &Console, options: &ConsoleOptions) -> Vec<LayoutRender> {
        let width = options.max_width;
        let height = options
            .height
            .filter(|&height| height > 0)
            .unwrap_or_else(|| console.height());
        self.region_map(width, height)
            .into_iter()
            .filter_map(|(path, region)| {
                let layout = self.at_path(&path)?;
                if !layout.children().is_empty() {
                    return None;
                }
                let leaf_options = options.update_dimensions(region.width, region.height);
                let render = match &layout.renderable {
                    Some(renderable) => {
                        console.render_lines(renderable.as_ref(), &leaf_options, true)
                    }
                    None => console.render_lines(&Placeholder { layout }, &leaf_options, true),
                };
                Some(LayoutRender {
                    name: layout.name.clone(),
                    path,
                    region,
                    render,
                })
            })
            .collect()
    }

    /// The leaves of the last render. Port of `Layout.map`.
    pub fn map(&self) -> Vec<LayoutRender> {
        self.render_map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl std::ops::Index<&str> for Layout {
    type Output = Layout;

    /// `layout[name]`; panics as upstream raises `KeyError` when there is no
    /// such layout.
    fn index(&self, name: &str) -> &Layout {
        self.get(name)
            .unwrap_or_else(|| panic!("No layout with name {name:?}"))
    }
}

impl std::ops::IndexMut<&str> for Layout {
    fn index_mut(&mut self, name: &str) -> &mut Layout {
        self.get_mut(name)
            .unwrap_or_else(|| panic!("No layout with name {name:?}"))
    }
}

/// Python's `repr()` of a string.
fn py_repr_str(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

/// `Pretty(layout)`: the layout's repr, on one line when it fits the width,
/// else one field per line; highlighted with the `ReprHighlighter`, and dim
/// for a hidden layout (`Styled(Pretty(layout), "dim")`).
struct LayoutRepr {
    fields: Vec<String>,
    dim: bool,
}

impl LayoutRepr {
    /// `pretty_repr(layout, max_width=…)`.
    fn repr(&self, max_width: usize) -> String {
        let one_line = format!("Layout({})", self.fields.join(", "));
        if self.fields.is_empty() || cell_len(&one_line) <= max_width {
            return one_line;
        }
        let last = self.fields.len() - 1;
        let mut out = String::from("Layout(\n");
        for (index, field) in self.fields.iter().enumerate() {
            out.push_str("    ");
            out.push_str(field);
            if index != last {
                out.push(',');
            }
            out.push('\n');
        }
        out.push(')');
        out
    }
}

impl Renderable for LayoutRepr {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut text = Text::new(self.repr(options.max_width));
        ReprHighlighter::new().highlight(&mut text);
        let segments = text.rich_render(console, options);
        if self.dim {
            Segment::apply_style(&segments, &Style::parse("dim").unwrap_or_default())
        } else {
            segments
        }
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let width = self
            .repr(options.max_width)
            .lines()
            .map(cell_len)
            .max()
            .unwrap_or(0);
        Measurement::new(width, width)
    }
}

/// Upstream's `_Placeholder`: a blue panel titled with the layout's name and
/// size, around its centred repr.
struct Placeholder<'a> {
    layout: &'a Layout,
}

impl Renderable for Placeholder<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let height = options
            .height
            .filter(|&height| height > 0)
            .unwrap_or(options.size.height);
        let title = match &self.layout.name {
            Some(name) => format!("{} ({width} x {height})", py_repr_str(name)),
            None => format!("({width} x {height})"),
        };
        let mut title = Text::new(title);
        ReprHighlighter::new().highlight(&mut title);
        let repr = LayoutRepr {
            fields: self.layout.repr_fields(),
            dim: false,
        };
        Panel::new(Box::new(
            Align::center(Box::new(repr)).vertical(VerticalAlign::Middle),
        ))
        .title_as_text(title)
        .border_style("blue")
        .height(height)
        .rich_render(console, options)
    }
}

impl Renderable for Layout {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = if options.max_width > 0 {
            options.max_width
        } else {
            console.width()
        };
        let height = options
            .height
            .filter(|&height| height > 0)
            .unwrap_or_else(|| console.height());
        let render_map = self.render_regions(console, &options.update_dimensions(width, height));
        let mut lines: Vec<Vec<Segment>> = vec![Vec::new(); height];
        for leaf in &render_map {
            let Region { y, height, .. } = leaf.region;
            for (row, line) in lines.iter_mut().skip(y).take(height).zip(&leaf.render) {
                row.extend(line.iter().cloned());
            }
        }
        *self
            .render_map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = render_map;

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
