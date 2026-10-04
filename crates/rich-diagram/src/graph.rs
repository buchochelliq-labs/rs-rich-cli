//! The graph model: nodes with labels and shapes, edges with labels, strokes
//! and heads, and the direction the graph flows.
//!
//! Build one in code with the chaining builder on [`Graph`], or from parts
//! (as a parser does) with [`Graph::from_parts`].

use std::collections::HashMap;

/// The longest edge, in ranks. An edge asks for a minimum number of ranks to
/// span ([`Edge::length`]); every rank it crosses costs layout work, so longer
/// requests are drawn at this length.
pub const MAX_EDGE_LENGTH: usize = 10;

/// Which way the graph flows: the direction from an edge's source rank to its
/// target rank.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Top to bottom (Mermaid `TD` / `TB`).
    #[default]
    TopDown,
    /// Bottom to top (`BT`).
    BottomUp,
    /// Left to right (`LR`).
    LeftRight,
    /// Right to left (`RL`).
    RightLeft,
}

impl Direction {
    /// Whether ranks run horizontally (`LR` / `RL`).
    pub fn is_horizontal(self) -> bool {
        matches!(self, Direction::LeftRight | Direction::RightLeft)
    }
}

/// A node's outline. Each is drawn as a box whose corners and sides suggest
/// the shape; the Mermaid bracket that produces it is given for reference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Shape {
    /// A plain box (`A[text]`).
    #[default]
    Rect,
    /// Rounded corners (`A(text)`).
    Round,
    /// Rounded corners with `(` `)` sides (`A([text])`).
    Stadium,
    /// A box with an inner line down each side (`A[[text]]`).
    Subroutine,
    /// A database: rounded corners (`A[(text)]`).
    Cylinder,
    /// A circle: rounded corners with `(` `)` sides (`A((text))`).
    Circle,
    /// A double circle, drawn as [`Shape::Circle`] (`A(((text)))`).
    DoubleCircle,
    /// A flag: `>` on the left (`A>text]`).
    Asymmetric,
    /// A decision diamond: slanted corners with `<` `>` sides (`A{text}`).
    Rhombus,
    /// Slanted corners (`A{{text}}`).
    Hexagon,
    /// `/` sides (`A[/text/]`).
    Parallelogram,
    /// `\` sides (`A[\text\]`).
    ParallelogramAlt,
    /// `/` then `\` (`A[/text\]`).
    Trapezoid,
    /// `\` then `/` (`A[\text/]`).
    TrapezoidAlt,
}

/// A node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// What edges refer to it by.
    pub id: String,
    /// The text inside the box; `\n` breaks a line.
    pub label: String,
    pub shape: Shape,
}

impl Node {
    /// A [`Shape::Rect`] node.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Node {
            id: id.into(),
            label: label.into(),
            shape: Shape::Rect,
        }
    }

    /// Set the shape.
    pub fn shape(mut self, shape: Shape) -> Self {
        self.shape = shape;
        self
    }
}

/// How an edge's line is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stroke {
    /// `─` (`-->`).
    #[default]
    Solid,
    /// `━` (`==>`).
    Thick,
    /// `┄` (`-.->`).
    Dotted,
    /// Laid out but not drawn (`~~~`): it only pulls its ends into ranks.
    Invisible,
}

/// The end of an edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Head {
    /// The line meets the node with nothing on it.
    #[default]
    None,
    /// `►` (`>`).
    Arrow,
    /// `●` (`o`).
    Circle,
    /// `×` (`x`).
    Cross,
}

/// An edge between two nodes, by index into [`Graph::nodes`].
///
/// Every edge has a source and a target, which decide the ranks: the target
/// goes below (or right of) the source. An undirected edge is one with no
/// heads; a two-way edge has a head at each end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    /// Text drawn beside the edge, on one line.
    pub label: Option<String>,
    pub stroke: Stroke,
    /// The head at `from`.
    pub start: Head,
    /// The head at `to`.
    pub end: Head,
    /// The minimum number of ranks the edge spans: 1 places the target in the
    /// next rank. Drawn at most [`MAX_EDGE_LENGTH`].
    pub length: usize,
}

impl Edge {
    /// A solid edge from `from` to `to` with an arrow at `to`.
    pub fn new(from: usize, to: usize) -> Self {
        Edge {
            from,
            to,
            label: None,
            stroke: Stroke::Solid,
            start: Head::None,
            end: Head::Arrow,
            length: 1,
        }
    }
}

/// What the last builder call added, for the calls that modify it.
#[derive(Clone, Copy, Debug)]
enum Last {
    Node(usize),
    Edge(usize),
}

/// A graph to lay out and draw.
///
/// The builder methods chain. [`node`](Graph::node) and
/// [`edge`](Graph::edge) add items; [`label`](Graph::label) changes whichever
/// was added last, [`shape`](Graph::shape) the last node, and
/// [`stroke`](Graph::stroke), [`heads`](Graph::heads) and
/// [`min_length`](Graph::min_length) the last edge. An edge to an id that has
/// no node yet adds one, labelled with its id.
///
/// ```
/// use rich_diagram::{Direction, Graph, Shape, Stroke};
///
/// let graph = Graph::new(Direction::LeftRight)
///     .node("api", "API")
///     .node("db", "Postgres").shape(Shape::Cylinder)
///     .edge("api", "db").label("reads")
///     .edge("api", "cache").stroke(Stroke::Dotted);
/// assert_eq!(graph.nodes().len(), 3);
/// assert_eq!(graph.edges()[0].label.as_deref(), Some("reads"));
/// assert_eq!(graph.nodes()[2].label, "cache");
/// ```
#[derive(Clone, Debug, Default)]
pub struct Graph {
    direction: Direction,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    ids: HashMap<String, usize>,
    last: Option<Last>,
    last_node: Option<usize>,
    last_edge: Option<usize>,
}

impl PartialEq for Graph {
    /// Graphs are equal when they draw the same: direction, nodes and edges.
    fn eq(&self, other: &Self) -> bool {
        self.direction == other.direction && self.nodes == other.nodes && self.edges == other.edges
    }
}

impl Eq for Graph {}

impl Graph {
    /// An empty graph flowing in `direction`.
    pub fn new(direction: Direction) -> Self {
        Graph {
            direction,
            ..Graph::default()
        }
    }

    /// A graph from finished parts, as a parser builds them. Edge endpoints
    /// are indexes into `nodes`; [`draw`](crate::draw) refuses a graph with an
    /// endpoint out of range. When two nodes share an id, the first is the one
    /// [`node_index`](Graph::node_index) and the builder find.
    pub fn from_parts(direction: Direction, nodes: Vec<Node>, edges: Vec<Edge>) -> Self {
        let mut ids = HashMap::with_capacity(nodes.len());
        for (index, node) in nodes.iter().enumerate() {
            ids.entry(node.id.clone()).or_insert(index);
        }
        Graph {
            direction,
            nodes,
            edges,
            ids,
            last: None,
            last_node: None,
            last_edge: None,
        }
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Nodes, in the order they were added.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Edges, in the order they were added.
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// The index of the node with this id.
    pub fn node_index(&self, id: &str) -> Option<usize> {
        self.ids.get(id).copied()
    }

    /// Whether there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Change the direction.
    pub fn set_direction(&mut self, direction: Direction) {
        self.direction = direction;
    }

    /// Add `node`, or replace the label and shape of the node with its id.
    /// Returns its index.
    pub fn add_node(&mut self, node: Node) -> usize {
        let index = match self.ids.get(&node.id) {
            Some(&index) => {
                self.nodes[index].label = node.label;
                self.nodes[index].shape = node.shape;
                index
            }
            None => {
                self.ids.insert(node.id.clone(), self.nodes.len());
                self.nodes.push(node);
                self.nodes.len() - 1
            }
        };
        self.last = Some(Last::Node(index));
        self.last_node = Some(index);
        index
    }

    /// Add `edge` and return its index.
    ///
    /// # Panics
    ///
    /// If an endpoint is not the index of a node.
    pub fn add_edge(&mut self, edge: Edge) -> usize {
        assert!(
            edge.from < self.nodes.len() && edge.to < self.nodes.len(),
            "edge {} -> {} names a node that does not exist ({} nodes)",
            edge.from,
            edge.to,
            self.nodes.len()
        );
        self.edges.push(edge);
        let index = self.edges.len() - 1;
        self.last = Some(Last::Edge(index));
        self.last_edge = Some(index);
        index
    }

    /// The index of the node `id`, adding it (labelled `id`) if it is new.
    fn ensure(&mut self, id: &str) -> usize {
        match self.ids.get(id) {
            Some(&index) => index,
            None => {
                self.ids.insert(id.to_string(), self.nodes.len());
                self.nodes.push(Node::new(id, id));
                self.nodes.len() - 1
            }
        }
    }

    /// Add a box labelled `label`, or relabel the node `id` if it exists.
    pub fn node(mut self, id: impl AsRef<str>, label: impl Into<String>) -> Self {
        let id = id.as_ref();
        let shape = self
            .node_index(id)
            .map(|index| self.nodes[index].shape)
            .unwrap_or_default();
        self.add_node(Node::new(id, label).shape(shape));
        self
    }

    /// Add a solid edge from `from` to `to` with an arrow at `to`.
    pub fn edge(mut self, from: impl AsRef<str>, to: impl AsRef<str>) -> Self {
        let from = self.ensure(from.as_ref());
        let to = self.ensure(to.as_ref());
        self.add_edge(Edge::new(from, to));
        self
    }

    /// Add an undirected edge: a solid line with no heads.
    pub fn link(self, from: impl AsRef<str>, to: impl AsRef<str>) -> Self {
        self.edge(from, to).heads(Head::None, Head::None)
    }

    /// Label the node or edge added last. Does nothing before either.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        match self.last {
            Some(Last::Node(index)) => self.nodes[index].label = label.into(),
            Some(Last::Edge(index)) => self.edges[index].label = Some(label.into()),
            None => {}
        }
        self
    }

    /// Set the shape of the node added last. Does nothing before any node.
    pub fn shape(mut self, shape: Shape) -> Self {
        if let Some(index) = self.last_node {
            self.nodes[index].shape = shape;
        }
        self
    }

    /// Set the stroke of the edge added last. Does nothing before any edge.
    pub fn stroke(mut self, stroke: Stroke) -> Self {
        if let Some(index) = self.last_edge {
            self.edges[index].stroke = stroke;
        }
        self
    }

    /// Set the heads of the edge added last: `start` at its source, `end` at
    /// its target. Does nothing before any edge.
    pub fn heads(mut self, start: Head, end: Head) -> Self {
        if let Some(index) = self.last_edge {
            self.edges[index].start = start;
            self.edges[index].end = end;
        }
        self
    }

    /// Make the edge added last span at least `ranks` ranks (1 is the
    /// default; at most [`MAX_EDGE_LENGTH`] is drawn). Does nothing before any
    /// edge.
    pub fn min_length(mut self, ranks: usize) -> Self {
        if let Some(index) = self.last_edge {
            self.edges[index].length = ranks;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_modifies_the_last_item() {
        let graph = Graph::new(Direction::TopDown)
            .node("a", "A")
            .shape(Shape::Round)
            .label("Alpha")
            .edge("a", "b")
            .label("to b")
            .stroke(Stroke::Thick)
            .heads(Head::Circle, Head::Cross)
            .min_length(3)
            .shape(Shape::Rhombus);
        assert_eq!(
            graph.nodes()[0],
            Node::new("a", "Alpha").shape(Shape::Rhombus)
        );
        assert_eq!(graph.nodes()[1], Node::new("b", "b"));
        let edge = &graph.edges()[0];
        assert_eq!(edge.label.as_deref(), Some("to b"));
        assert_eq!(
            (edge.stroke, edge.start, edge.end, edge.length),
            (Stroke::Thick, Head::Circle, Head::Cross, 3)
        );
    }

    #[test]
    fn renaming_a_node_keeps_its_shape_and_index() {
        let graph = Graph::new(Direction::LeftRight)
            .edge("a", "b")
            .node("b", "Bee")
            .shape(Shape::Circle)
            .node("b", "B");
        assert_eq!(graph.node_index("b"), Some(1));
        assert_eq!(graph.nodes()[1], Node::new("b", "B").shape(Shape::Circle));
    }

    #[test]
    fn modifiers_before_anything_do_nothing() {
        let graph = Graph::new(Direction::TopDown)
            .label("x")
            .shape(Shape::Round)
            .stroke(Stroke::Dotted);
        assert!(graph.is_empty());
        let graph = graph.link("a", "b");
        assert_eq!(
            (graph.edges()[0].start, graph.edges()[0].end),
            (Head::None, Head::None)
        );
    }

    #[test]
    fn from_parts_and_builder_are_equal() {
        let parts = Graph::from_parts(
            Direction::TopDown,
            vec![Node::new("a", "a"), Node::new("b", "b")],
            vec![Edge::new(0, 1)],
        );
        assert_eq!(parts, Graph::new(Direction::TopDown).edge("a", "b"));
        assert_eq!(parts.node_index("b"), Some(1));
    }

    #[test]
    #[should_panic(expected = "does not exist")]
    fn add_edge_checks_its_ends() {
        Graph::new(Direction::TopDown).add_edge(Edge::new(0, 1));
    }
}
