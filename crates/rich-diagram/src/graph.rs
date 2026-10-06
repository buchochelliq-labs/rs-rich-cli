//! The graph model: nodes with labels and shapes, edges with labels, strokes
//! and heads, and the direction the graph flows.
//!
//! Build one in code with the chaining builder on [`Graph`], or from parts
//! (as a parser does) with [`Graph::from_parts`]. Two kinds of grouping sit
//! on top of the nodes: [`Cluster`]s, drawn as labelled frames, and
//! same-rank groups ([`Graph::same_rank`]), drawn side by side.

use std::collections::HashMap;

/// The longest edge, in ranks. An edge asks for a minimum number of ranks to
/// span ([`Edge::length`]); every rank it crosses costs layout work, so longer
/// requests are drawn at this length.
pub const MAX_EDGE_LENGTH: usize = 10;

/// The largest source a parser reads, in bytes (64 KB). The DOT parser and
/// Mermaid's flowcharts share these caps, so neither can be made to do
/// unbounded work by a document.
pub const MAX_SOURCE: usize = 64 * 1024;

/// The most nodes a parsed source may declare.
pub const MAX_NODES: usize = 500;

/// The most edges a parsed source may declare (after `{ … }` groups expand),
/// and the most [`draw`](crate::draw) lays out.
pub const MAX_EDGES: usize = 2000;

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
    /// A table: the label's first line is a centred header, ruled off from
    /// the lines below it, which are drawn left-aligned. What
    /// [`ErDiagram`](crate::er::ErDiagram) draws each entity as.
    Table,
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

/// A group of nodes drawn inside a labelled frame (a DOT `subgraph
/// cluster…`, an ER diagram's group).
///
/// [`draw`](crate::draw) keeps a cluster's nodes together in each rank and
/// draws a dashed frame around them, its label in the top border. Clusters
/// nest: a cluster with a [`parent`](Cluster::parent) is framed inside it, and
/// its nodes count as the parent's too, listed there or not.
///
/// A node listed in two clusters that do not nest is framed in the deeper
/// one (the first listed, at equal depth), and the drawing notes it. A
/// cluster with no nodes, nested ones included, is not drawn.
///
/// ```
/// use rich_diagram::{Cluster, Direction, Graph};
///
/// let mut graph = Graph::new(Direction::LeftRight).edge("web", "api").edge("api", "db");
/// let backend = graph.add_cluster(Cluster::new("backend").label("Backend").nodes([1, 2]));
/// graph.add_cluster(Cluster::new("storage").nodes([2]).parent(backend));
/// assert_eq!(graph.clusters()[1].parent, Some(0));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cluster {
    /// What it is known by (a DOT subgraph's name, `cluster` prefix
    /// included).
    pub id: String,
    /// The text in the frame's top border; none draws a bare frame.
    pub label: Option<String>,
    /// Its nodes, by index into [`Graph::nodes`]. Those of nested clusters
    /// may be listed too (DOT lists them); they need not be.
    pub nodes: Vec<usize>,
    /// The cluster it sits in, by index into [`Graph::clusters`]. A parent
    /// must come before its children; one that does not is ignored, and the
    /// cluster is drawn at the top level.
    pub parent: Option<usize>,
}

impl Cluster {
    /// An unlabelled cluster with no nodes.
    pub fn new(id: impl Into<String>) -> Self {
        Cluster {
            id: id.into(),
            ..Cluster::default()
        }
    }

    /// Set the label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the nodes, by index.
    pub fn nodes(mut self, nodes: impl IntoIterator<Item = usize>) -> Self {
        self.nodes = nodes.into_iter().collect();
        self
    }

    /// Nest it in the cluster at `parent`.
    pub fn parent(mut self, parent: usize) -> Self {
        self.parent = Some(parent);
        self
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
    clusters: Vec<Cluster>,
    same_rank: Vec<Vec<usize>>,
    ids: HashMap<String, usize>,
    last: Option<Last>,
    last_node: Option<usize>,
    last_edge: Option<usize>,
}

impl PartialEq for Graph {
    /// Graphs are equal when they draw the same: direction, nodes, edges,
    /// clusters and same-rank groups.
    fn eq(&self, other: &Self) -> bool {
        self.direction == other.direction
            && self.nodes == other.nodes
            && self.edges == other.edges
            && self.clusters == other.clusters
            && self.same_rank == other.same_rank
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
            clusters: Vec::new(),
            same_rank: Vec::new(),
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

    /// Clusters, in the order they were added.
    pub fn clusters(&self) -> &[Cluster] {
        &self.clusters
    }

    /// Same-rank groups, by node index, in the order they were added.
    pub fn same_rank_groups(&self) -> &[Vec<usize>] {
        &self.same_rank
    }

    /// Add `cluster` and return its index. Node indexes out of range are
    /// ignored when drawing.
    pub fn add_cluster(&mut self, cluster: Cluster) -> usize {
        self.clusters.push(cluster);
        self.clusters.len() - 1
    }

    /// Ask for these nodes, by index, to be drawn in the same rank (DOT's
    /// `rank=same`). [`draw`](crate::draw) honours a group unless an edge
    /// joins two of its nodes (directly, or through another group sharing a
    /// node): a layered drawing has no edges within a rank, so such a group
    /// is dropped and the drawing notes it. Otherwise ranks follow the edges
    /// with the group as one node; a cycle through it may reverse an edge
    /// (drawn pointing back up), as any cycle does. Indexes out of range are
    /// ignored.
    pub fn add_same_rank(&mut self, nodes: impl IntoIterator<Item = usize>) {
        self.same_rank.push(nodes.into_iter().collect());
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

    /// Frame the nodes `members` (by id, adding any that are new) in a
    /// top-level cluster labelled `label`. See [`Cluster`] for nesting.
    ///
    /// ```
    /// use rich_diagram::{draw, Direction, Graph};
    ///
    /// let graph = Graph::new(Direction::LeftRight)
    ///     .edge("web", "api")
    ///     .cluster("backend", "Backend", ["api"]);
    /// let drawing = draw(&graph, false).unwrap();
    /// assert!(drawing.lines[0].contains("Backend"), "{:#?}", drawing.lines);
    /// ```
    pub fn cluster<I, S>(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        members: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let nodes: Vec<usize> = members
            .into_iter()
            .map(|id| self.ensure(id.as_ref()))
            .collect();
        self.add_cluster(Cluster::new(id).label(label).nodes(nodes));
        self
    }

    /// Draw the nodes `members` (by id, adding any that are new) in the same
    /// rank. See [`add_same_rank`](Graph::add_same_rank) for when it cannot.
    pub fn same_rank<I, S>(mut self, members: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let nodes: Vec<usize> = members
            .into_iter()
            .map(|id| self.ensure(id.as_ref()))
            .collect();
        self.add_same_rank(nodes);
        self
    }

    /// Every node, edge and cluster label passed through `f`.
    pub(crate) fn map_text(&self, f: impl Fn(&str) -> String) -> Graph {
        let mut graph = self.clone();
        for node in &mut graph.nodes {
            node.label = f(&node.label);
        }
        for edge in &mut graph.edges {
            if let Some(label) = &mut edge.label {
                *label = f(label);
            }
        }
        for cluster in &mut graph.clusters {
            if let Some(label) = &mut cluster.label {
                *label = f(label);
            }
        }
        graph
    }

    /// The node each node is ranked with: itself, or the first node of the
    /// same-rank groups it shares (transitively). And the groups dropped,
    /// by index into [`same_rank_groups`](Graph::same_rank_groups), because
    /// an edge would join two nodes of one rank.
    pub(crate) fn rank_sets(&self) -> (Vec<usize>, Vec<usize>) {
        let n = self.nodes.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(parent: &mut [usize], mut v: usize) -> usize {
            while parent[v] != v {
                parent[v] = parent[parent[v]];
                v = parent[v];
            }
            v
        }
        let mut dropped = Vec::new();
        for (index, group) in self.same_rank.iter().enumerate() {
            let members: Vec<usize> = group.iter().copied().filter(|&v| v < n).collect();
            if members.len() < 2 {
                continue;
            }
            let before = parent.clone();
            for &v in &members[1..] {
                let (a, b) = (find(&mut parent, members[0]), find(&mut parent, v));
                if a != b {
                    // The lower index is the representative, so it is stable.
                    let (low, high) = (a.min(b), a.max(b));
                    parent[high] = low;
                }
            }
            let flat = self.edges.iter().any(|edge| {
                edge.from != edge.to
                    && edge.from < n
                    && edge.to < n
                    && find(&mut parent, edge.from) == find(&mut parent, edge.to)
            });
            if flat {
                parent = before;
                dropped.push(index);
            }
        }
        let rep = (0..n).map(|v| find(&mut parent, v)).collect();
        (rep, dropped)
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
