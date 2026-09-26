//! `rich.tree.Tree`. Port of upstream `rich/tree.py`.
//!
//! The Python class keeps its label, styles and children (a list of `Tree`s);
//! printing builds core's `Tree`, which renders the guides, styles,
//! `expanded`, `hide_root` and the ASCII guides. The tree is built from an
//! explicit stack (and core walks and drops it with one), so a deep tree does
//! not overflow the native stack.

use std::collections::HashSet;

use pyo3::exceptions::PyRecursionError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyString};
use pyo3::{PyTraverseError, PyVisit};

use rich::protocol::Renderable;
use rich::style::StyleType;
use rich::tree::{ASCII_GUIDES, TREE_GUIDES};
use rich::{Cell, Tree as CoreTree};

use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::style::style_type;
use crate::text::Text;

/// One node's own fields, flattened depth first (`nodes[0]` is the root).
struct Node {
    label: Cell,
    style: StyleType,
    guide_style: StyleType,
    expanded: bool,
    children: Vec<usize>,
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
        // Build core's trees bottom up: in depth-first order every child
        // comes after its parent.
        let mut built: Vec<Option<CoreTree>> = Vec::with_capacity(nodes.len());
        built.resize_with(nodes.len(), || None);
        for (index, node) in nodes.into_iter().enumerate().rev() {
            let mut tree = CoreTree::new(node.label)
                .style(node.style)
                .guide_style(node.guide_style)
                .expanded(node.expanded)
                .highlight(self.highlight);
            for child in node.children {
                if let Some(child) = built[child].take() {
                    tree.add_tree(child);
                }
            }
            built[index] = Some(tree);
        }
        let root = built[0].take().expect("the root is built last");
        Ok(Box::new(root.hide_root(self.hide_root)))
    }
}

/// One node's own fields, and its children list to walk. Every label
/// renders with the root's `highlight`, as upstream's do.
fn node(py: Python<'_>, tree: &Tree, highlight: Option<bool>) -> PyResult<(Node, Py<PyList>)> {
    let label = tree.label.bind(py);
    let label = match label.extract::<PyRef<'_, Text>>() {
        Ok(text) => Cell::Text(text.inner.clone()),
        Err(_) => {
            // Check now what printing needs: bad markup, a non-renderable.
            renderable::to_renderable(label, highlight)?;
            Cell::Renderable(PyRenderable::shared(label.clone().unbind(), highlight))
        }
    };
    Ok((
        Node {
            label,
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
        TREE_GUIDES
            .iter()
            .map(|[a, b, c, d]| (*a, *b, *c, *d))
            .collect()
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
