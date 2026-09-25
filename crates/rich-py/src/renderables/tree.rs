//! `rich.tree.Tree`. Port of upstream `rich/tree.py`.
//!
//! Core's `Tree` draws only the default guides with no styles; upstream's
//! `style`, `guide_style` (and the heavy and double guides it selects),
//! `expanded`, `hide_root` and the ASCII guides are ported here. Like
//! upstream (and core), the render walks an explicit stack, so a deep tree
//! does not overflow the native stack.

use std::collections::HashSet;

use pyo3::exceptions::PyRecursionError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};
use pyo3::{PyTraverseError, PyVisit};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions, Justify};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::style::{Style as CoreStyle, StyleType};

use crate::renderable::{self, AsRenderable};
use crate::style::style_type;

use super::constrain::StyledRender;
use super::{ascii_only, get_style, render_lines};

const ASCII_GUIDES: [&str; 4] = ["    ", "|   ", "+-- ", "`-- "];
const TREE_GUIDES: [[&str; 4]; 3] = [
    ["    ", "│   ", "├── ", "└── "],
    ["    ", "┃   ", "┣━━ ", "┗━━ "],
    ["    ", "║   ", "╠══ ", "╚══ "],
];
const SPACE: usize = 0;
const CONTINUE: usize = 1;
const FORK: usize = 2;
const END: usize = 3;

/// Bold and underline2, the attributes that pick the heavy and double guides.
const BOLD: usize = 0;
const UNDERLINE2: usize = 9;

struct Node {
    label: Box<dyn Renderable>,
    style: StyleType,
    guide_style: StyleType,
    expanded: bool,
    children: Vec<usize>,
}

/// A tree, flattened: `nodes[0]` is the root.
struct TreeRender {
    nodes: Vec<Node>,
    hide_root: bool,
}

/// `Segment.apply_style(prefix, style.background_style, post_style=...)`.
fn guide_prefix(prefix: &[CoreSegment], style: &CoreStyle) -> Vec<CoreSegment> {
    let background = CoreStyle::from_color(None, style.bgcolor().cloned());
    let remove = CoreStyle::parse("not bold not underline2").unwrap_or_default();
    prefix
        .iter()
        .map(|segment| {
            if segment.control {
                return segment.clone();
            }
            let own = segment.style.clone().unwrap_or_default();
            CoreSegment::new(
                segment.text.clone(),
                Some(background.combine(&own).combine(&remove)),
            )
        })
        .collect()
}

impl Renderable for TreeRender {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        let ascii = ascii_only(console);
        let legacy = console.legacy_windows();
        let make_guide = |index: usize, style: CoreStyle| -> CoreSegment {
            let line = if ascii {
                ASCII_GUIDES[index]
            } else {
                let guide = if style.attr(BOLD) == Some(true) {
                    1
                } else if style.attr(UNDERLINE2) == Some(true) {
                    2
                } else {
                    0
                };
                TREE_GUIDES[if legacy { 0 } else { guide }][index]
            };
            CoreSegment::new(line, Some(style))
        };
        let style_of = |segment: &CoreSegment| segment.style.clone().unwrap_or_default();
        let root = &self.nodes[0];

        let mut rows: Vec<Vec<CoreSegment>> = Vec::new();
        let mut levels = vec![make_guide(CONTINUE, get_style(console, &root.guide_style))];
        // Each entry: the child indices being walked and the next position.
        let mut stack: Vec<(Vec<usize>, usize)> = vec![(vec![0], 0)];
        let mut guide_styles = vec![get_style(console, &root.guide_style)];
        let mut styles = vec![get_style(console, &root.style)];
        let mut depth = 0usize;
        let pad = options.justify != Justify::Default;

        while let Some((children, position)) = stack.pop() {
            let Some(&index) = children.get(position) else {
                levels.pop();
                if let Some(last) = levels.last_mut() {
                    *last = make_guide(FORK, style_of(last));
                    guide_styles.pop();
                    styles.pop();
                }
                continue;
            };
            let last = position + 1 == children.len();
            stack.push((children, position + 1));
            let node = &self.nodes[index];
            if last {
                let level = levels.last_mut().expect("levels is never empty here");
                *level = make_guide(END, style_of(level));
            }
            let guide_style = guide_styles
                .last()
                .expect("a style per level")
                .combine(&get_style(console, &node.guide_style));
            let style = styles
                .last()
                .expect("a style per level")
                .combine(&get_style(console, &node.style));
            let skip = if self.hide_root { 2 } else { 1 };
            let mut prefix: Vec<CoreSegment> = levels.iter().skip(skip).cloned().collect();
            let prefix_width: usize = prefix.iter().map(CoreSegment::cell_length).sum();
            let mut label_options = options.update_width(options.max_width.saturating_sub(prefix_width));
            label_options.height = None;
            let label = StyledRender {
                child: LabelRef(node.label.as_ref()),
                style: StyleType::Style(style.clone()),
            };
            let lines = render_lines(console, &label, &label_options, None, pad);
            if !(depth == 0 && self.hide_root) {
                for (line_no, line) in lines.into_iter().enumerate() {
                    let mut row = Vec::new();
                    if !prefix.is_empty() {
                        row.extend(guide_prefix(&prefix, &style));
                    }
                    row.extend(line);
                    rows.push(row);
                    if line_no == 0 && !prefix.is_empty() {
                        let end = prefix.last_mut().expect("prefix is not empty");
                        *end = make_guide(if last { SPACE } else { CONTINUE }, style_of(end));
                    }
                }
            }
            if node.expanded && !node.children.is_empty() {
                let level = levels.last_mut().expect("levels is never empty here");
                *level = make_guide(if last { SPACE } else { CONTINUE }, style_of(level));
                levels.push(make_guide(
                    if node.children.len() == 1 { END } else { FORK },
                    guide_style,
                ));
                let pushed = styles.last().expect("a style").combine(&get_style(console, &node.style));
                styles.push(pushed);
                let pushed = guide_styles
                    .last()
                    .expect("a style")
                    .combine(&get_style(console, &node.guide_style));
                guide_styles.push(pushed);
                stack.push((node.children.clone(), 0));
                depth += 1;
            }
        }
        super::join_lines(rows)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        let (mut minimum, mut maximum) = (0, 0);
        let mut pending: Vec<(usize, usize)> = vec![(0, 0)];
        while let Some((index, level)) = pending.pop() {
            let node = &self.nodes[index];
            let label = CoreMeasurement::get(console, options, node.label.as_ref());
            let indent = level * 4;
            minimum = minimum.max(label.minimum + indent);
            maximum = maximum.max(label.maximum + indent);
            if node.expanded {
                pending.extend(node.children.iter().rev().map(|child| (*child, level + 1)));
            }
        }
        CoreMeasurement::new(minimum, maximum)
    }
}

/// A borrowed label, as a [`super::Child`].
struct LabelRef<'a>(&'a dyn Renderable);

impl super::Child for LabelRef<'_> {
    fn get(&self) -> &dyn Renderable {
        self.0
    }
}

/// `rich.tree.Tree`: a renderable tree structure.
#[pyclass(name = "Tree", module = "rs_rich.tree")]
pub(crate) struct Tree {
    #[pyo3(get, set)]
    label: Py<PyAny>,
    style: Py<PyAny>,
    guide_style: Py<PyAny>,
    #[pyo3(get, set)]
    children: Py<PyList>,
    #[pyo3(get, set)]
    expanded: bool,
    #[pyo3(get, set)]
    highlight: bool,
    #[pyo3(get, set)]
    hide_root: bool,
}

impl AsRenderable for Tree {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        // Flatten depth first from an explicit stack; a tree that contains
        // itself would never finish upstream, so it is refused.
        let highlight = Some(self.highlight);
        let root = node(py, self, highlight)?;
        let mut nodes = vec![root.0];
        let mut pending: Vec<(usize, Py<PyList>, usize)> = vec![(0, root.1, 0)];
        let mut path: Vec<usize> = Vec::new();
        let mut on_path: HashSet<usize> = HashSet::new();
        let self_ptr = self.children.as_ptr() as usize;
        on_path.insert(self_ptr);
        path.push(self_ptr);
        while let Some((parent, children, position)) = pending.pop() {
            let list = children.bind(py);
            let Ok(child) = list.get_item(position) else {
                if let Some(done) = path.pop() {
                    on_path.remove(&done);
                }
                continue;
            };
            pending.push((parent, children.clone_ref(py), position + 1));
            let child = child.cast_into::<Tree>().map_err(PyErr::from)?;
            let child = child.borrow();
            let key = child.children.as_ptr() as usize;
            if !on_path.insert(key) {
                return Err(PyRecursionError::new_err("a Tree cannot contain itself"));
            }
            path.push(key);
            let (built, grandchildren) = node(py, &child, highlight)?;
            let index = nodes.len();
            nodes.push(built);
            nodes[parent].children.push(index);
            pending.push((index, grandchildren, 0));
        }
        Ok(Box::new(TreeRender {
            nodes,
            hide_root: self.hide_root,
        }))
    }
}

/// One node's own fields, and its children list to walk.
fn node(py: Python<'_>, tree: &Tree, highlight: Option<bool>) -> PyResult<(Node, Py<PyList>)> {
    Ok((
        Node {
            label: renderable::to_renderable(tree.label.bind(py), highlight)?,
            style: style_type(Some(tree.style.bind(py)))?.unwrap_or_default(),
            guide_style: style_type(Some(tree.guide_style.bind(py)))?.unwrap_or_default(),
            expanded: tree.expanded,
            children: Vec::new(),
        },
        tree.children.clone_ref(py),
    ))
}

#[pymethods]
impl Tree {
    #[classattr]
    #[pyo3(name = "ASCII_GUIDES")]
    fn ascii_guides() -> (&'static str, &'static str, &'static str, &'static str) {
        let [a, b, c, d] = ASCII_GUIDES;
        (a, b, c, d)
    }

    #[classattr]
    #[pyo3(name = "TREE_GUIDES")]
    fn tree_guides() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
        TREE_GUIDES.iter().map(|[a, b, c, d]| (*a, *b, *c, *d)).collect()
    }

    #[new]
    #[pyo3(signature = (
        label, *, style=None, guide_style=None, expanded=true, highlight=false, hide_root=false
    ))]
    fn new(
        py: Python<'_>,
        label: Py<PyAny>,
        style: Option<Py<PyAny>>,
        guide_style: Option<Py<PyAny>>,
        expanded: bool,
        highlight: bool,
        hide_root: bool,
    ) -> PyResult<Self> {
        let style = style.unwrap_or_else(|| PyString::new(py, "tree").into_any().unbind());
        let guide_style =
            guide_style.unwrap_or_else(|| PyString::new(py, "tree.line").into_any().unbind());
        style_type(Some(style.bind(py)))?;
        style_type(Some(guide_style.bind(py)))?;
        Ok(Tree {
            label,
            style,
            guide_style,
            children: PyList::empty(py).unbind(),
            expanded,
            highlight,
            hide_root,
        })
    }

    /// Add a child tree, returning it so it can be added to in turn.
    #[pyo3(signature = (label, *, style=None, guide_style=None, expanded=true, highlight=Some(false)))]
    fn add(
        &self,
        py: Python<'_>,
        label: Py<PyAny>,
        style: Option<Py<PyAny>>,
        guide_style: Option<Py<PyAny>>,
        expanded: bool,
        highlight: Option<bool>,
    ) -> PyResult<Py<Tree>> {
        let node = Tree::new(
            py,
            label,
            Some(style.unwrap_or_else(|| self.style.clone_ref(py))),
            Some(guide_style.unwrap_or_else(|| self.guide_style.clone_ref(py))),
            expanded,
            highlight.unwrap_or(self.highlight),
            false,
        )?;
        let node = Py::new(py, node)?;
        self.children.bind(py).append(node.clone_ref(py))?;
        Ok(node)
    }

    #[getter]
    fn style(&self, py: Python<'_>) -> Py<PyAny> {
        self.style.clone_ref(py)
    }

    #[setter]
    fn set_style(&mut self, style: Bound<'_, PyAny>) -> PyResult<()> {
        style_type(Some(&style))?;
        self.style = style.unbind();
        Ok(())
    }

    #[getter]
    fn guide_style(&self, py: Python<'_>) -> Py<PyAny> {
        self.guide_style.clone_ref(py)
    }

    #[setter]
    fn set_guide_style(&mut self, style: Bound<'_, PyAny>) -> PyResult<()> {
        style_type(Some(&style))?;
        self.guide_style = style.unbind();
        Ok(())
    }

    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.label)?;
        visit.call(&self.style)?;
        visit.call(&self.guide_style)?;
        visit.call(&self.children)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Tree>(m)
}
