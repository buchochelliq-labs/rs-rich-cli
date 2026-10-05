//! `rs_rich.diagram`: the `rs-rich-diagram` crate from Python (0.0.15
//! workstream 5): a graph built in code (`Graph`), drawn by the layered
//! layout (`Diagram`, `draw_graph`), and DOT sources parsed and drawn
//! natively (`parse_dot`, `Dot`).
//!
//! Owner: the diagram area. `Graph` is mutable and its builder methods
//! return the graph, so they chain; `Diagram` and `Dot` are frozen
//! renderables. Node and edge classes are `DiagramNode` and `DiagramEdge`
//! because the flat native module already has a `Node`.

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;

use rich::protocol::Renderable;
use rich_diagram::{
    Diagram as CoreDiagram, Direction, Dot as CoreDot, DotError as CoreDotError,
    DotGraph as CoreDotGraph, Edge as CoreEdge, Graph as CoreGraph, Head, Node as CoreNode, Shape,
    Stroke,
};

use crate::art::{repr_opt, repr_str};
use crate::ext::common::names;
use crate::renderable::{self, AsRenderable};

create_exception!(_native, DiagramError, PyException);
create_exception!(_native, DotError, DiagramError);
create_exception!(_native, DiagramLayoutError, DiagramError);

names!(direction, direction_name, Direction, "direction", {
    "td" => Direction::TopDown,
    "tb" => Direction::TopDown,
    "bt" => Direction::BottomUp,
    "lr" => Direction::LeftRight,
    "rl" => Direction::RightLeft,
});

names!(shape, shape_name, Shape, "shape", {
    "rect" => Shape::Rect,
    "round" => Shape::Round,
    "stadium" => Shape::Stadium,
    "subroutine" => Shape::Subroutine,
    "cylinder" => Shape::Cylinder,
    "circle" => Shape::Circle,
    "double_circle" => Shape::DoubleCircle,
    "asymmetric" => Shape::Asymmetric,
    "rhombus" => Shape::Rhombus,
    "hexagon" => Shape::Hexagon,
    "parallelogram" => Shape::Parallelogram,
    "parallelogram_alt" => Shape::ParallelogramAlt,
    "trapezoid" => Shape::Trapezoid,
    "trapezoid_alt" => Shape::TrapezoidAlt,
});

names!(stroke, stroke_name, Stroke, "stroke", {
    "solid" => Stroke::Solid,
    "thick" => Stroke::Thick,
    "dotted" => Stroke::Dotted,
    "invisible" => Stroke::Invisible,
});

names!(head, head_name, Head, "head", {
    "none" => Head::None,
    "arrow" => Head::Arrow,
    "circle" => Head::Circle,
    "cross" => Head::Cross,
});

/// `"TD"`, `"BT"`, `"LR"` or `"RL"`.
fn direction_label(direction: Direction) -> String {
    direction_name(direction).to_ascii_uppercase()
}

/// A head by name, `None` meaning no head.
fn opt_head(value: Option<&str>) -> PyResult<Head> {
    value.map_or(Ok(Head::None), head)
}

fn opt_head_name(value: Head) -> Option<&'static str> {
    (value != Head::None).then(|| head_name(value))
}

// ---------------------------------------------------------------------------
// The graph model

/// A node of a `Graph` (`rich_diagram::Node`).
#[pyclass(name = "DiagramNode", module = "rs_rich.diagram", frozen)]
pub(crate) struct DiagramNode {
    #[pyo3(get)]
    id: String,
    #[pyo3(get)]
    label: String,
    #[pyo3(get)]
    shape: &'static str,
}

#[pymethods]
impl DiagramNode {
    fn __repr__(&self) -> String {
        format!(
            "DiagramNode(id={}, label={}, shape={})",
            repr_str(&self.id),
            repr_str(&self.label),
            repr_str(self.shape)
        )
    }
}

impl From<&CoreNode> for DiagramNode {
    fn from(node: &CoreNode) -> Self {
        DiagramNode {
            id: node.id.clone(),
            label: node.label.clone(),
            shape: shape_name(node.shape),
        }
    }
}

/// An edge of a `Graph` (`rich_diagram::Edge`): `source` and `target` index
/// `nodes`.
#[pyclass(name = "DiagramEdge", module = "rs_rich.diagram", frozen)]
pub(crate) struct DiagramEdge {
    #[pyo3(get)]
    source: usize,
    #[pyo3(get)]
    target: usize,
    #[pyo3(get)]
    label: Option<String>,
    #[pyo3(get)]
    stroke: &'static str,
    #[pyo3(get)]
    start: Option<&'static str>,
    #[pyo3(get)]
    end: Option<&'static str>,
    #[pyo3(get)]
    length: usize,
}

#[pymethods]
impl DiagramEdge {
    fn __repr__(&self) -> String {
        format!(
            "DiagramEdge(source={}, target={}, label={}, stroke={}, start={}, end={}, length={})",
            self.source,
            self.target,
            repr_opt(self.label.as_deref()),
            repr_str(self.stroke),
            repr_opt(self.start),
            repr_opt(self.end),
            self.length
        )
    }
}

impl From<&CoreEdge> for DiagramEdge {
    fn from(edge: &CoreEdge) -> Self {
        DiagramEdge {
            source: edge.from,
            target: edge.to,
            label: edge.label.clone(),
            stroke: stroke_name(edge.stroke),
            start: opt_head_name(edge.start),
            end: opt_head_name(edge.end),
            length: edge.length,
        }
    }
}

/// A graph to lay out and draw (`rich_diagram::Graph`). `node` and `edge`
/// add to it and return it, so calls chain; an edge to an id with no node
/// adds one labelled with its id.
#[pyclass(name = "Graph", module = "rs_rich.diagram")]
pub(crate) struct Graph {
    inner: CoreGraph,
}

#[pymethods]
impl Graph {
    /// `direction`: `"TD"` (top down, the default; `"TB"` too), `"BT"`,
    /// `"LR"` or `"RL"`.
    #[new]
    #[pyo3(signature = (direction="TD"))]
    fn new(direction: &str) -> PyResult<Self> {
        Ok(Graph {
            inner: CoreGraph::new(self::direction(direction)?),
        })
    }

    /// Add a node labelled `label` (default: its id), or relabel and
    /// reshape the node `id`.
    #[pyo3(signature = (id, label=None, *, shape="rect"))]
    fn node<'py>(
        mut slf: PyRefMut<'py, Self>,
        id: String,
        label: Option<String>,
        shape: &str,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let shape = self::shape(shape)?;
        let label = label.unwrap_or_else(|| id.clone());
        slf.inner.add_node(CoreNode::new(id, label).shape(shape));
        Ok(slf)
    }

    /// Add an edge from `source` to `target`. `start` and `end` are the
    /// heads at each end (`None`, `"arrow"`, `"circle"` or `"cross"`);
    /// `min_length` the ranks it spans at least.
    #[pyo3(signature = (source, target, *, label=None, stroke="solid", start=None, end=Some("arrow"), min_length=1))]
    #[allow(clippy::too_many_arguments)]
    fn edge<'py>(
        mut slf: PyRefMut<'py, Self>,
        source: String,
        target: String,
        label: Option<String>,
        stroke: &str,
        start: Option<&str>,
        end: Option<&str>,
        min_length: usize,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let stroke = self::stroke(stroke)?;
        let (start, end) = (opt_head(start)?, opt_head(end)?);
        if min_length == 0 {
            return Err(PyValueError::new_err("min_length must be at least 1"));
        }
        let graph = std::mem::take(&mut slf.inner)
            .edge(source, target)
            .stroke(stroke)
            .heads(start, end)
            .min_length(min_length);
        slf.inner = match label {
            Some(label) => graph.label(label),
            None => graph,
        };
        Ok(slf)
    }

    /// An undirected edge: a solid line with no heads.
    #[pyo3(signature = (source, target, *, label=None))]
    fn link<'py>(
        slf: PyRefMut<'py, Self>,
        source: String,
        target: String,
        label: Option<String>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        Graph::edge(slf, source, target, label, "solid", None, None, 1)
    }

    #[getter]
    fn get_direction(&self) -> String {
        direction_label(self.inner.direction())
    }

    #[setter]
    fn set_direction(&mut self, direction: &str) -> PyResult<()> {
        self.inner.set_direction(self::direction(direction)?);
        Ok(())
    }

    #[getter]
    fn nodes(&self) -> Vec<DiagramNode> {
        self.inner.nodes().iter().map(DiagramNode::from).collect()
    }

    #[getter]
    fn edges(&self) -> Vec<DiagramEdge> {
        self.inner.edges().iter().map(DiagramEdge::from).collect()
    }

    /// The index of the node `id`, or `None`.
    fn node_index(&self, id: &str) -> Option<usize> {
        self.inner.node_index(id)
    }

    fn __len__(&self) -> usize {
        self.inner.nodes().len()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Graph {} nodes={} edges={}>",
            direction_label(self.inner.direction()),
            self.inner.nodes().len(),
            self.inner.edges().len()
        )
    }
}

fn layout_error(error: impl std::fmt::Display) -> PyErr {
    DiagramLayoutError::new_err(format!("too large to draw: {error}"))
}

/// Lay out and draw `graph` as lines of text (`rich_diagram::draw`). Raises
/// `DiagramLayoutError` when it is too large to draw.
#[pyfunction]
#[pyo3(signature = (graph, ascii=false))]
fn draw_graph(py: Python<'_>, graph: PyRef<'_, Graph>, ascii: bool) -> PyResult<Vec<String>> {
    let graph = graph.inner.clone();
    py.detach(move || rich_diagram::draw(&graph, ascii))
        .map(|drawing| drawing.lines)
        .map_err(layout_error)
}

// ---------------------------------------------------------------------------
// Renderables

/// A `Graph` drawn with box-drawing characters, or ASCII
/// (`rich_diagram::Diagram`). It measures to its drawing and is cropped,
/// never wrapped, when given less.
#[pyclass(name = "Diagram", module = "rs_rich.diagram", frozen)]
pub(crate) struct Diagram {
    graph: CoreGraph,
    ascii: Option<bool>,
}

impl Diagram {
    fn core(&self) -> CoreDiagram {
        let diagram = CoreDiagram::new(self.graph.clone());
        match self.ascii {
            Some(ascii) => diagram.ascii(ascii),
            None => diagram,
        }
    }
}

impl AsRenderable for Diagram {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.core()))
    }
}

#[pymethods]
impl Diagram {
    /// The graph is copied: changing it afterwards does not change this.
    /// `ascii`: draw with ASCII only (`None` follows the console).
    #[new]
    #[pyo3(signature = (graph, *, ascii=None))]
    fn new(graph: PyRef<'_, Graph>, ascii: Option<bool>) -> Self {
        Diagram {
            graph: graph.inner.clone(),
            ascii,
        }
    }

    /// A copy of the graph.
    #[getter]
    fn graph(&self) -> Graph {
        Graph {
            inner: self.graph.clone(),
        }
    }

    #[getter]
    fn ascii(&self) -> Option<bool> {
        self.ascii
    }

    /// The whole drawing, uncropped, as lines. Raises `DiagramLayoutError`
    /// when the graph is too large to draw.
    #[pyo3(signature = (ascii=false))]
    fn drawing(&self, ascii: bool) -> PyResult<Vec<String>> {
        self.core()
            .drawing(ascii)
            .map(|drawing| drawing.lines.clone())
            .map_err(layout_error)
    }

    fn __repr__(&self) -> String {
        format!(
            "<Diagram nodes={} edges={}>",
            self.graph.nodes().len(),
            self.graph.edges().len()
        )
    }
}

fn dot_error(error: &CoreDotError) -> PyErr {
    let exception = DotError::new_err(error.to_string());
    Python::attach(|py| {
        let value = exception.value(py);
        let _ = value.setattr("line", error.line);
        let _ = value.setattr("construct", error.construct.clone());
    });
    exception
}

/// A cluster of a parsed DOT graph: a `subgraph cluster…` and its nodes.
#[pyclass(name = "DotCluster", module = "rs_rich.diagram", frozen)]
pub(crate) struct DotCluster {
    #[pyo3(get)]
    id: String,
    #[pyo3(get)]
    label: Option<String>,
    /// Indexes into the graph's `nodes`.
    #[pyo3(get)]
    nodes: Vec<usize>,
}

#[pymethods]
impl DotCluster {
    fn __repr__(&self) -> String {
        format!(
            "DotCluster(id={}, label={}, nodes={:?})",
            repr_str(&self.id),
            repr_opt(self.label.as_deref()),
            self.nodes
        )
    }
}

/// A parsed DOT source (`rich_diagram::DotGraph`).
#[pyclass(name = "DotGraph", module = "rs_rich.diagram", frozen)]
pub(crate) struct DotGraph {
    inner: CoreDotGraph,
}

#[pymethods]
impl DotGraph {
    /// The graph to draw (a copy).
    #[getter]
    fn graph(&self) -> Graph {
        Graph {
            inner: self.inner.graph.clone(),
        }
    }

    /// `digraph` (`True`) or `graph`.
    #[getter]
    fn directed(&self) -> bool {
        self.inner.directed
    }

    #[getter]
    fn strict(&self) -> bool {
        self.inner.strict
    }

    #[getter]
    fn name(&self) -> Option<String> {
        self.inner.name.clone()
    }

    #[getter]
    fn label(&self) -> Option<String> {
        self.inner.label.clone()
    }

    #[getter]
    fn clusters(&self) -> Vec<DotCluster> {
        self.inner
            .clusters
            .iter()
            .map(|cluster| DotCluster {
                id: cluster.id.clone(),
                label: cluster.label.clone(),
                nodes: cluster.nodes.clone(),
            })
            .collect()
    }

    /// What was accepted but is not drawn (shown under the drawing).
    #[getter]
    fn notes(&self) -> Vec<String> {
        self.inner.notes.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "<DotGraph {} nodes={} edges={}>",
            if self.inner.directed {
                "digraph"
            } else {
                "graph"
            },
            self.inner.graph.nodes().len(),
            self.inner.graph.edges().len()
        )
    }
}

/// Parse a DOT source (`rich_diagram::dot::parse`). Raises `DotError`, with
/// `line` and, for a construct this parser does not support (a node port,
/// an HTML-like label, …), `construct`; a syntax error has `construct`
/// `None`.
#[pyfunction]
fn parse_dot(source: &str) -> PyResult<DotGraph> {
    rich_diagram::dot::parse(source)
        .map(|inner| DotGraph { inner })
        .map_err(|error| dot_error(&error))
}

/// A DOT (Graphviz) source drawn natively (`rich_diagram::Dot`). What the
/// parser refuses renders as the source under a note; `parse_dot` raises
/// instead.
#[pyclass(name = "Dot", module = "rs_rich.diagram", frozen)]
pub(crate) struct Dot {
    source: String,
    ascii: Option<bool>,
}

impl AsRenderable for Dot {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(
            CoreDot::new(self.source.clone()).ascii_option(self.ascii),
        ))
    }
}

#[pymethods]
impl Dot {
    #[new]
    #[pyo3(signature = (source, *, ascii=None))]
    fn new(source: String, ascii: Option<bool>) -> Self {
        Dot { source, ascii }
    }

    #[getter]
    fn source(&self) -> &str {
        &self.source
    }

    #[getter]
    fn ascii(&self) -> Option<bool> {
        self.ascii
    }

    /// The parsed graph; raises `DotError` as `parse_dot` does.
    fn parsed(&self) -> PyResult<DotGraph> {
        parse_dot(&self.source)
    }

    fn __repr__(&self) -> String {
        format!("<Dot {} bytes>", self.source.len())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("DiagramError", py.get_type::<DiagramError>())?;
    m.add("DotError", py.get_type::<DotError>())?;
    m.add("DiagramLayoutError", py.get_type::<DiagramLayoutError>())?;
    m.add_class::<DiagramNode>()?;
    m.add_class::<DiagramEdge>()?;
    m.add_class::<Graph>()?;
    renderable::add_renderable_class::<Diagram>(m)?;
    renderable::add_renderable_class::<Dot>(m)?;
    m.add_class::<DotGraph>()?;
    m.add_class::<DotCluster>()?;
    m.add_function(pyo3::wrap_pyfunction!(draw_graph, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(parse_dot, m)?)?;
    m.add("DIAGRAM_MAX_EDGE_LENGTH", rich_diagram::MAX_EDGE_LENGTH)?;
    Ok(())
}
