//! Lay out a [`Graph`] in ranks and draw it with box-drawing characters.
//!
//! A small layered (Sugiyama-style) layout:
//! 1. break cycles by reversing DFS back edges, then rank nodes by longest path;
//! 2. split edges that span several ranks with one-cell dummy points;
//! 3. order each rank by barycentre sweeps, keeping the order with the fewest
//!    crossings;
//! 4. place nodes along the rank by averaging their neighbours' centres;
//! 5. route every edge orthogonally, giving each edge its own port on a node
//!    and its own track in the gap between two ranks.
//!
//! Layout works in top-down terms; `LR` swaps the axes, and `BT`/`RL` mirror
//! the finished geometry. Lines are drawn as direction bits per cell, so
//! crossings and joins pick the right junction character.
//!
//! Same-rank groups ([`Graph::add_same_rank`]) are ranked as one node in step
//! 1; a group that an edge runs within is dropped with a note, since a rank
//! has no room for an edge.
//!
//! Clusters ([`Cluster`](crate::Cluster)) change three steps, and only for a
//! graph that has them, so a graph without clusters draws exactly as before:
//! - every point belongs to a cluster: a node to its innermost, a dummy to the
//!   innermost holding both ends of its edge, and a cluster with no point in a
//!   rank it spans gets an invisible placeholder there;
//! - ordering keeps each cluster's points contiguous in every rank, with
//!   sibling clusters in the same left-to-right order throughout (first
//!   chosen from an unconstrained ordering), so their frames can be
//!   rectangles that do not overlap;
//! - after placement, points move right until each frame clears its
//!   neighbours, and the gaps between ranks gain rows for the frames' top and
//!   bottom borders.
//!
//! Frames are dashed so they read apart from node boxes, and are drawn last,
//! in the cells nothing else uses: an edge crossing a frame stays whole. If
//! the separation does not settle (it is bounded, and should not happen),
//! the graph is drawn without frames and [`Drawing::notes`] says so.
//!
//! The layout takes no width: a drawing is as wide as the graph needs.
//! [`Drawing::cropped`] fits it to a width by cutting each line at the right
//! edge, which is what [`Diagram`](crate::Diagram) does.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::cluster::{Grouping, Tree};
use crate::graph::{Direction, Graph, Head, Shape, Stroke, MAX_EDGES, MAX_EDGE_LENGTH};
use rich::cells::{cell_len, char_cell_width, set_cell_size};

/// A drawn graph: lines of text without trailing spaces.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Drawing {
    pub lines: Vec<String>,
    /// The widest line, in cells.
    pub width: usize,
    /// What the graph asked for but the drawing does not show, one sentence
    /// each: a same-rank group an edge runs within, a node in clusters that
    /// do not nest, cluster frames that could not be placed. Empty for a
    /// graph without clusters or same-rank groups.
    pub notes: Vec<String>,
}

impl Drawing {
    /// The lines cut to at most `width` cells, with trailing spaces and the
    /// rows left empty at either end removed. Deterministic: the left part of
    /// the drawing is kept, whatever it cuts through.
    pub fn cropped(&self, width: usize) -> Vec<String> {
        let mut lines: Vec<String> = self
            .lines
            .iter()
            .map(|line| {
                if cell_len(line) > width {
                    set_cell_size(line, width).trim_end().to_string()
                } else {
                    line.clone()
                }
            })
            .collect();
        // Cropping can leave rows empty at either end.
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let leading = lines.iter().take_while(|line| line.is_empty()).count();
        lines.drain(..leading);
        lines
    }
}

/// Why a graph was not drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawError {
    message: String,
}

impl DrawError {
    fn new(message: String) -> Self {
        DrawError { message }
    }
}

impl fmt::Display for DrawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DrawError {}

/// The most cells a drawing may occupy before it is refused as too large.
pub const MAX_CELLS: i64 = 2_000_000;

/// The most points (nodes, plus one per rank crossed by a long edge) the
/// layout may place. Checked right after ranking, before any of the work that
/// grows with them.
pub const MAX_VERTICES: usize = 5_000;

const N: u8 = 1;
const E: u8 = 2;
const S: u8 = 4;
const W: u8 = 8;

/// Lay out and draw `graph`. `ascii` restricts the output to ASCII.
///
/// Fails when the graph has more than [`MAX_EDGES`] edges or
/// [`MAX_VERTICES`] nodes (checked first, before any layout work), when the
/// layout would place more than [`MAX_VERTICES`] points or the drawing would
/// cover more than [`MAX_CELLS`] cells, and when an edge names a node that
/// does not exist.
///
/// ```
/// use rich_diagram::{draw, Direction, Graph};
///
/// let graph = Graph::new(Direction::LeftRight).edge("A", "B");
/// let drawing = draw(&graph, false).unwrap();
/// assert_eq!(drawing.lines, ["┌───┐  ┌───┐", "│ A ├─►│ B │", "└───┘  └───┘"]);
/// let ascii = draw(&graph, true).unwrap();
/// assert_eq!(ascii.lines[1], "| A +->| B |");
/// ```
pub fn draw(graph: &Graph, ascii: bool) -> Result<Drawing, DrawError> {
    // Checked first, so a graph with edges but no nodes is refused too.
    let count = graph.nodes().len();
    if let Some(edge) = graph
        .edges()
        .iter()
        .find(|edge| edge.from >= count || edge.to >= count)
    {
        return Err(DrawError::new(format!(
            "edge {} -> {} names a node that does not exist ({count} nodes)",
            edge.from, edge.to
        )));
    }
    // Checked before any layout work, which grows faster than either.
    if graph.edges().len() > MAX_EDGES {
        return Err(DrawError::new(format!(
            "it has {} edges, more than {MAX_EDGES}",
            graph.edges().len()
        )));
    }
    if count > MAX_VERTICES {
        return Err(DrawError::new(format!(
            "it has {count} nodes, more than {MAX_VERTICES}"
        )));
    }
    if graph.nodes().is_empty() {
        return Ok(Drawing {
            lines: Vec::new(),
            width: 0,
            notes: Vec::new(),
        });
    }
    let graph = printable(graph);
    let graph = graph.as_ref();
    let horizontal = graph.direction().is_horizontal();
    let mut geometry = layout(graph, horizontal).map_err(DrawError::new)?;
    if matches!(
        graph.direction(),
        Direction::BottomUp | Direction::RightLeft
    ) {
        geometry.flip(horizontal);
    }
    geometry.normalize();
    let (width, height) = geometry.extent();
    if width.saturating_mul(height) > MAX_CELLS {
        return Err(DrawError::new(format!(
            "it would take {width} {} {height} cells to draw",
            if ascii { "x" } else { "×" }
        )));
    }
    let mut drawing = render(graph, &geometry, ascii, width, height);
    drawing.notes = geometry.notes;
    Ok(drawing)
}

/// Whether `c` may be drawn: no control characters but the line break.
fn drawable(c: char) -> bool {
    c == '\n' || !c.is_control()
}

/// `graph` with control characters removed from its labels (tabs become
/// spaces), so a label cannot reach the terminal as an escape sequence.
/// Borrowed unchanged when every label is already clean.
fn printable(graph: &Graph) -> Cow<'_, Graph> {
    let clean = |text: &str| text.chars().all(drawable);
    let dirty = graph.nodes().iter().any(|node| !clean(&node.label))
        || graph
            .edges()
            .iter()
            .any(|edge| edge.label.as_deref().is_some_and(|label| !clean(label)))
        || graph
            .clusters()
            .iter()
            .any(|cluster| cluster.label.as_deref().is_some_and(|label| !clean(label)));
    if !dirty {
        return Cow::Borrowed(graph);
    }
    Cow::Owned(graph.map_text(|text| {
        text.chars()
            .map(|c| if c == '\t' { ' ' } else { c })
            .filter(|&c| drawable(c))
            .collect()
    }))
}

// ---------------------------------------------------------------- geometry

#[derive(Clone, Debug)]
struct BoxGeo {
    node: usize,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

#[derive(Clone, Debug)]
struct EdgeGeo {
    /// Corner points from the source's border cell to the target's.
    points: Vec<(i64, i64)>,
    stroke: Stroke,
    /// Heads at the first and last point.
    start: Head,
    end: Head,
    /// Text, its leftmost column and its row.
    label: Option<(String, i64, i64)>,
}

/// A cluster's frame: its outline, and the label for its top border.
#[derive(Clone, Debug)]
struct FrameGeo {
    /// The cluster it frames, by index into the graph's clusters.
    #[cfg_attr(not(test), allow(dead_code))]
    cluster: usize,
    x: i64,
    y: i64,
    w: i64,
    h: i64,
    label: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Geometry {
    boxes: Vec<BoxGeo>,
    edges: Vec<EdgeGeo>,
    /// Self-loop markers.
    loops: Vec<(i64, i64)>,
    /// Cluster frames, outermost first.
    frames: Vec<FrameGeo>,
    /// What the layout could not do, for [`Drawing::notes`].
    notes: Vec<String>,
}

impl Geometry {
    fn bounds(&self) -> (i64, i64, i64, i64) {
        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        let mut take = |x0: i64, y0: i64, x1: i64, y1: i64| {
            min_x = min_x.min(x0);
            min_y = min_y.min(y0);
            max_x = max_x.max(x1);
            max_y = max_y.max(y1);
        };
        for b in &self.boxes {
            take(b.x, b.y, b.x + b.w - 1, b.y + b.h - 1);
        }
        for edge in &self.edges {
            for &(x, y) in &edge.points {
                take(x, y, x, y);
            }
            if let Some((text, x, y)) = &edge.label {
                take(*x, *y, x + cell_len(text) as i64 - 1, *y);
            }
        }
        for &(x, y) in &self.loops {
            take(x, y, x, y);
        }
        for f in &self.frames {
            take(f.x, f.y, f.x + f.w - 1, f.y + f.h - 1);
        }
        (min_x, min_y, max_x, max_y)
    }

    fn extent(&self) -> (i64, i64) {
        let (min_x, min_y, max_x, max_y) = self.bounds();
        (max_x - min_x + 1, max_y - min_y + 1)
    }

    /// Mirror the rank axis: top-down becomes bottom-up, left-right becomes
    /// right-left. Text still reads left to right.
    fn flip(&mut self, horizontal: bool) {
        let (min_x, min_y, max_x, max_y) = self.bounds();
        let mirror = |v: i64| {
            if horizontal {
                max_x + min_x - v
            } else {
                max_y + min_y - v
            }
        };
        for b in &mut self.boxes {
            if horizontal {
                b.x = mirror(b.x + b.w - 1);
            } else {
                b.y = mirror(b.y + b.h - 1);
            }
        }
        for edge in &mut self.edges {
            for point in &mut edge.points {
                if horizontal {
                    point.0 = mirror(point.0);
                } else {
                    point.1 = mirror(point.1);
                }
            }
            if let Some((text, x, y)) = &mut edge.label {
                if horizontal {
                    *x = mirror(*x + cell_len(text) as i64 - 1);
                } else {
                    *y = mirror(*y);
                }
            }
        }
        for point in &mut self.loops {
            if horizontal {
                point.0 = mirror(point.0);
            } else {
                point.1 = mirror(point.1);
            }
        }
        for f in &mut self.frames {
            if horizontal {
                f.x = mirror(f.x + f.w - 1);
            } else {
                f.y = mirror(f.y + f.h - 1);
            }
        }
    }

    /// Shift everything so the top-left corner is (0, 0).
    fn normalize(&mut self) {
        let (min_x, min_y, _, _) = self.bounds();
        for b in &mut self.boxes {
            b.x -= min_x;
            b.y -= min_y;
        }
        for edge in &mut self.edges {
            for point in &mut edge.points {
                point.0 -= min_x;
                point.1 -= min_y;
            }
            if let Some((_, x, y)) = &mut edge.label {
                *x -= min_x;
                *y -= min_y;
            }
        }
        for point in &mut self.loops {
            point.0 -= min_x;
            point.1 -= min_y;
        }
        for f in &mut self.frames {
            f.x -= min_x;
            f.y -= min_y;
        }
    }
}

// ------------------------------------------------------------------ layout

/// A point in the layout graph: a node, or a dummy on a long edge.
struct Vertex {
    node: Option<usize>,
    rank: usize,
    /// Size across the rank (width top-down, height left-right).
    cross: i64,
    /// Size along the flow (height top-down, width left-right).
    along: i64,
}

/// One rank-to-rank piece of an edge.
struct Piece {
    from: usize,
    to: usize,
    /// Index into the graph's edges.
    edge: usize,
}

fn text_size(label: &str) -> (i64, i64) {
    let lines: Vec<&str> = label.split('\n').collect();
    let width = lines.iter().map(|l| cell_len(l)).max().unwrap_or(0).max(1);
    (width as i64, lines.len() as i64)
}

fn edge_text(label: &str) -> String {
    label.split('\n').collect::<Vec<_>>().join(" ")
}

fn layout(graph: &Graph, horizontal: bool) -> Result<Geometry, String> {
    let n = graph.nodes().len();
    let mut notes = Vec::new();

    // Same-rank groups rank as one node: `rep` maps each node to the node it
    // ranks with (itself, without groups).
    let (rep, dropped) = graph.rank_sets();
    for &group in &dropped {
        let ids: Vec<&str> = graph.same_rank_groups()[group]
            .iter()
            .filter_map(|&v| graph.nodes().get(v))
            .map(|node| node.id.as_str())
            .collect();
        notes.push(format!(
            "a same-rank group is not applied, as an edge joins two of its nodes: {}",
            ids.join(", ")
        ));
    }

    // 1. Break cycles: reverse the edges a DFS finds going back.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (index, edge) in graph.edges().iter().enumerate() {
        if edge.from != edge.to {
            out[rep[edge.from]].push(index);
        }
    }
    let mut reversed = vec![false; graph.edges().len()];
    let mut state = vec![0u8; n]; // 0 new, 1 on the stack, 2 done
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&edge) = out[v].get(*next) {
                *next += 1;
                let to = rep[graph.edges()[edge].to];
                match state[to] {
                    0 => {
                        state[to] = 1;
                        stack.push((to, 0));
                    }
                    1 => reversed[edge] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    // Oriented edges: (upper, lower, minimum rank span, graph edge).
    let oriented: Vec<(usize, usize, usize, usize)> = graph
        .edges()
        .iter()
        .enumerate()
        .filter(|(_, e)| e.from != e.to)
        .map(|(i, e)| {
            let (u, v) = if reversed[i] {
                (e.to, e.from)
            } else {
                (e.from, e.to)
            };
            (u, v, e.length.clamp(1, MAX_EDGE_LENGTH), i)
        })
        .collect();

    // 2. Rank by longest path, then pull sources down next to their targets.
    // Ranked by representative: a same-rank group is one node here.
    let mut indegree = vec![0usize; n];
    let mut successors: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for &(u, v, len, _) in &oriented {
        indegree[rep[v]] += 1;
        successors[rep[u]].push((rep[v], len));
    }
    let mut order = Vec::with_capacity(n);
    let mut remaining = indegree.clone();
    let mut queue: std::collections::VecDeque<usize> =
        (0..n).filter(|&v| remaining[v] == 0).collect();
    while let Some(v) = queue.pop_front() {
        order.push(v);
        for &(w, _) in &successors[v] {
            remaining[w] -= 1;
            if remaining[w] == 0 {
                queue.push_back(w);
            }
        }
    }
    let mut rank = vec![0usize; n];
    for &v in &order {
        for &(w, len) in &successors[v] {
            rank[w] = rank[w].max(rank[v] + len);
        }
    }
    for &v in order.iter().rev() {
        if indegree[v] == 0 && !successors[v].is_empty() {
            rank[v] = successors[v]
                .iter()
                .map(|&(w, len)| rank[w] - len)
                .min()
                .unwrap_or(0);
        }
    }
    let rank: Vec<usize> = (0..n).map(|v| rank[rep[v]]).collect();

    // Refuse before building anything that grows with the ranks crossed.
    let dummies = oriented.iter().fold(0usize, |total, &(u, v, _, _)| {
        total.saturating_add(rank[v].saturating_sub(rank[u] + 1))
    });
    let points = n.saturating_add(dummies);
    if points > MAX_VERTICES {
        return Err(format!(
            "its edges cross {points} rank positions, more than {MAX_VERTICES}"
        ));
    }

    // 3. Vertices (nodes, then dummies) and rank-to-rank pieces.
    let mut vertices: Vec<Vertex> = graph
        .nodes()
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let (tw, th) = text_size(&node.label);
            let extra = if node.shape == Shape::Subroutine {
                2
            } else {
                0
            };
            // A table's header is ruled off from its rows.
            let rule = i64::from(node.shape == Shape::Table && th > 1);
            let (mut w, h) = (tw + 4 + extra, th + 2 + rule);
            // An odd width centres the port, so straight runs line up.
            if !horizontal && w % 2 == 0 {
                w += 1;
            }
            let (cross, along) = if horizontal { (h, w) } else { (w, h) };
            Vertex {
                node: Some(i),
                rank: rank[i],
                cross,
                along,
            }
        })
        .collect();
    let mut pieces: Vec<Piece> = Vec::new();
    // For each graph edge: its pieces in order, upper to lower.
    let mut chains: Vec<Vec<usize>> = vec![Vec::new(); graph.edges().len()];
    for &(u, v, _, edge) in &oriented {
        let mut previous = u;
        for r in rank[u] + 1..rank[v] {
            vertices.push(Vertex {
                node: None,
                rank: r,
                cross: 1,
                along: 0,
            });
            let dummy = vertices.len() - 1;
            chains[edge].push(pieces.len());
            pieces.push(Piece {
                from: previous,
                to: dummy,
                edge,
            });
            previous = dummy;
        }
        chains[edge].push(pieces.len());
        pieces.push(Piece {
            from: previous,
            to: v,
            edge,
        });
    }

    // Clusters: every point's innermost cluster. A node's is its own; a
    // dummy's, the innermost cluster holding both ends of its edge.
    let tree = Tree::new(graph, &mut notes);
    let mut of: Vec<Option<usize>> = Vec::new();
    if let Some(tree) = &tree {
        of = vec![None; vertices.len()];
        of[..n].copy_from_slice(&tree.node);
        for piece in &pieces {
            if vertices[piece.to].node.is_none() {
                let edge = &graph.edges()[piece.edge];
                of[piece.to] = tree.common(tree.node[edge.from], tree.node[edge.to]);
            }
        }
    }

    // Widen nodes so every piece gets its own port.
    let mut ins = vec![0i64; vertices.len()];
    let mut outs = vec![0i64; vertices.len()];
    for piece in &pieces {
        outs[piece.from] += 1;
        ins[piece.to] += 1;
    }
    // Left-right, a table's edges keep to its rows, below the header and
    // rule (`inset` rows).
    let inset = |node: usize| {
        let node = &graph.nodes()[node];
        i64::from(horizontal && node.shape == Shape::Table && node.label.contains('\n')) * 2
    };
    for (v, vertex) in vertices.iter_mut().enumerate() {
        if let Some(node) = vertex.node {
            let ports = ins[v].max(outs[v]).max(1);
            let needed = if horizontal {
                ports + 2 + inset(node)
            } else {
                2 * ports + 1
            };
            vertex.cross = vertex.cross.max(needed);
            if !horizontal && vertex.cross % 2 == 0 {
                vertex.cross += 1;
            }
        }
    }

    // A cluster holds a point in every rank it spans, so its frame has a
    // place in each: an empty rank gets a placeholder, which draws nothing.
    if let Some(tree) = &tree {
        fill_spans(tree, &mut vertices, &mut of);
        if vertices.len() > MAX_VERTICES {
            return Err(format!(
                "its clusters and edges cross {} rank positions, more than {MAX_VERTICES}",
                vertices.len()
            ));
        }
    }
    let mut grouping = tree.as_ref().map(|tree| Grouping::new(tree, of));

    let ranks = vertices.iter().map(|v| v.rank).max().unwrap_or(0) + 1;
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); ranks];
    for (v, vertex) in vertices.iter().enumerate() {
        layers[vertex.rank].push(v);
    }
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    for piece in &pieces {
        preds[piece.to].push(piece.from);
        succs[piece.from].push(piece.to);
    }

    // 4. Order each rank by barycentre sweeps. With clusters, a second round
    // keeps each cluster's points together, siblings in one order throughout.
    order_layers(&mut layers, &preds, &succs, vertices.len(), None);
    if let Some(grouping) = &mut grouping {
        grouping.order_siblings(&layers);
        order_layers(&mut layers, &preds, &succs, vertices.len(), Some(grouping));
    }

    // 5. Positions across the rank, then, with clusters, apart until no
    // frame overlaps what is outside it.
    let gap = if horizontal { 1 } else { 2 };
    let mut position = place(&layers, &vertices, &preds, &succs, gap);
    // Across the rank, a frame clears its contents by a column top-down; by
    // no row left-right, where its top border is a row of its own anyway.
    let pad = if horizontal { 0 } else { 1 };
    // The label sits in the frame's top border: a corner and a border cell
    // each side, and a space each side of the text. Top-down the border runs
    // across the ranks, left-right along them.
    let label_room: Vec<i64> = graph
        .clusters()
        .iter()
        .map(|cluster| match cluster.label.as_deref() {
            Some(label) if !label.is_empty() => cell_len(&edge_text(label)) as i64 + 6,
            _ => 0,
        })
        .collect();
    let mut across = None;
    if let Some(grouping) = &grouping {
        let cross: Vec<i64> = vertices.iter().map(|v| v.cross).collect();
        let min_width = if horizontal {
            vec![0; label_room.len()]
        } else {
            label_room.clone()
        };
        across = grouping.separate(&layers, &mut position, &cross, gap, pad, &min_width);
        if across.is_none() {
            notes.push("cluster frames are not drawn: they could not be placed apart".into());
        }
    }
    let grouping = grouping.filter(|_| across.is_some());
    let position = position;
    let center = |v: usize| position[v] + vertices[v].cross / 2;

    // Ports: pieces leaving and entering each vertex, spread over its side in
    // the order of the vertex at the other end.
    let spacing = if horizontal { 1 } else { 2 };
    let mut port_from = vec![0i64; pieces.len()];
    let mut port_to = vec![0i64; pieces.len()];
    let mut leaving: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    let mut entering: Vec<Vec<usize>> = vec![Vec::new(); vertices.len()];
    for (p, piece) in pieces.iter().enumerate() {
        leaving[piece.from].push(p);
        entering[piece.to].push(p);
    }
    for v in 0..vertices.len() {
        let spread = |list: &mut Vec<usize>, other: &dyn Fn(usize) -> usize, ports: &mut [i64]| {
            list.sort_by_key(|&p| (center(other(p)), other(p)));
            let k = list.len() as i64;
            if vertices[v].node.is_none() {
                for &p in list.iter() {
                    ports[p] = position[v];
                }
                return;
            }
            let skip = vertices[v].node.map_or(0, inset);
            let interior = vertices[v].cross - 2 - skip;
            let span = (k - 1) * spacing + 1;
            let start = position[v] + 1 + skip + (interior - span).max(0) / 2;
            for (i, &p) in list.iter().enumerate() {
                ports[p] = start + i as i64 * spacing;
            }
        };
        spread(&mut leaving[v], &|p| pieces[p].to, &mut port_from);
        spread(&mut entering[v], &|p| pieces[p].from, &mut port_to);
    }

    // 6. Rank thickness, and the gap after each rank: tracks for pieces that
    // change position, and room for labels.
    let thickness: Vec<i64> = layers
        .iter()
        .map(|layer| {
            layer
                .iter()
                .map(|&v| vertices[v].along)
                .max()
                .unwrap_or(0)
                .max(1)
        })
        .collect();
    let mut track = vec![None::<i64>; pieces.len()];
    let mut label_slot = vec![0i64; pieces.len()];
    let mut label_left = vec![0i64; pieces.len()];
    let mut gap_tracks = vec![0i64; ranks];
    let mut gap_labels = vec![0i64; ranks];
    let label_of = |p: usize| -> Option<String> {
        let piece = &pieces[p];
        let edge = &graph.edges()[piece.edge];
        let last = vertices[piece.to].node.is_some();
        match (&edge.label, last, edge.stroke) {
            (Some(text), true, stroke) if stroke != Stroke::Invisible && !text.is_empty() => {
                Some(edge_text(text))
            }
            _ => None,
        }
    };
    // Pieces by the rank they leave, in piece order.
    let mut leaving_rank: Vec<Vec<usize>> = vec![Vec::new(); ranks];
    for (p, piece) in pieces.iter().enumerate() {
        leaving_rank[vertices[piece.from].rank].push(p);
    }
    for r in 0..ranks.saturating_sub(1) {
        let here = std::mem::take(&mut leaving_rank[r]);
        gap_tracks[r] = assign_tracks(&here, &port_from, &port_to, &mut track, |p| {
            graph.edges()[pieces[p].edge].stroke == Stroke::Invisible
        });
        if horizontal {
            // Labels sit on each piece's final run, one per row.
            gap_labels[r] = here
                .iter()
                .filter_map(|&p| label_of(p))
                .map(|text| cell_len(&text) as i64 + 2)
                .max()
                .unwrap_or(0);
        } else {
            // Every piece runs straight along its entry position through the
            // label rows. A label sits on its own line, or beside it when that
            // would cover another piece's line.
            let entries: Vec<i64> = here
                .iter()
                .filter(|&&p| graph.edges()[pieces[p].edge].stroke != Stroke::Invisible)
                .map(|&p| port_to[p])
                .collect();
            let mut label_ends: Vec<i64> = Vec::new();
            let mut labelled: Vec<(usize, i64, i64)> = here
                .iter()
                .filter_map(|&p| {
                    let text = label_of(p)?;
                    let width = cell_len(&text) as i64;
                    let own = port_to[p];
                    let clear = |left: i64| {
                        entries
                            .iter()
                            .all(|&x| x == own || x < left || x >= left + width)
                    };
                    let left = [own - width / 2, own + 2, own - 1 - width]
                        .into_iter()
                        .find(|&left| clear(left))
                        .unwrap_or(own - width / 2);
                    label_left[p] = left;
                    Some((p, left - 1, left + width))
                })
                .collect();
            labelled.sort_by_key(|&(_, lo, _)| lo);
            for (p, lo, hi) in labelled {
                let slot = match label_ends.iter().position(|&end| end < lo) {
                    Some(slot) => slot,
                    None => {
                        label_ends.push(i64::MIN);
                        label_ends.len() - 1
                    }
                };
                label_ends[slot] = hi;
                label_slot[p] = slot as i64;
            }
            gap_labels[r] = label_ends.len() as i64;
        }
    }
    // Frames along the flow: before a rank, a border row (a column
    // left-right) per frame opening there, plus one for the arrowheads; after
    // it, a row per frame closing there, and left-right as many more as a
    // label needs. `top[c]` counts the frames opening with `c` at its first
    // rank, itself and those inside it; `first[c]` and `last[c]` are its
    // border rows.
    let count = graph.clusters().len();
    let mut spans = vec![(0usize, 0usize); count];
    let mut top = vec![0i64; count];
    let mut before = vec![0i64; ranks];
    let mut after = vec![0i64; ranks];
    let mut closing: Vec<Vec<usize>> = vec![Vec::new(); ranks];
    if let Some(grouping) = &grouping {
        for &c in &grouping.tree.deepest_first {
            let points = &grouping.inside[c];
            let lo = points.iter().map(|&v| vertices[v].rank).min().unwrap_or(0);
            let hi = points.iter().map(|&v| vertices[v].rank).max().unwrap_or(0);
            spans[c] = (lo, hi);
            top[c] = 1 + grouping.tree.children[c]
                .iter()
                .filter(|&&k| spans[k].0 == lo)
                .map(|&k| top[k])
                .max()
                .unwrap_or(0);
            before[lo] = before[lo].max(1 + top[c]);
            // Deepest first, so a frame closes after those inside it.
            closing[hi].push(c);
        }
    }
    let mut first = vec![0i64; count];
    let mut last = vec![0i64; count];
    let mut layer_start = vec![0i64; ranks];
    layer_start[0] = before[0];
    for r in 0..ranks {
        if r > 0 {
            layer_start[r] = layer_start[r - 1]
                + thickness[r - 1]
                + after[r - 1]
                + gap_tracks[r - 1]
                + gap_labels[r - 1]
                + 2
                + before[r];
        }
        let end = layer_start[r] + thickness[r] - 1;
        for &c in &closing[r] {
            first[c] = layer_start[spans[c].0] - 1 - top[c];
            let mut border = end + 1;
            if let Some(grouping) = &grouping {
                for &k in &grouping.tree.children[c] {
                    if spans[k].1 == r {
                        border = border.max(last[k] + 1);
                    }
                }
            }
            if horizontal {
                border = border.max(first[c] + label_room[c] - 1);
            }
            last[c] = border;
            after[r] = after[r].max(border - end);
        }
    }

    // 7. Geometry, in (across, along) then mapped to (x, y).
    let to_xy = |across: i64, along: i64| {
        if horizontal {
            (along, across)
        } else {
            (across, along)
        }
    };
    let mut geometry = Geometry::default();
    for (v, vertex) in vertices.iter().enumerate() {
        let Some(node) = vertex.node else { continue };
        let (x, y) = to_xy(position[v], layer_start[vertex.rank]);
        let (w, h) = if horizontal {
            (vertex.along, vertex.cross)
        } else {
            (vertex.cross, vertex.along)
        };
        geometry.boxes.push(BoxGeo { node, x, y, w, h });
    }
    for (index, edge) in graph.edges().iter().enumerate() {
        if edge.from == edge.to {
            let b = geometry
                .boxes
                .iter()
                .find(|b| b.node == edge.from)
                .expect("every node has a box");
            geometry.loops.push((b.x + b.w, b.y + b.h / 2));
            continue;
        }
        let mut points: Vec<(i64, i64)> = Vec::new();
        let mut label = None;
        for &p in &chains[index] {
            let piece = &pieces[p];
            let (from, to) = (&vertices[piece.from], &vertices[piece.to]);
            let r = from.rank;
            let start_along = if from.node.is_some() {
                layer_start[r] + from.along - 1
            } else {
                layer_start[r]
            };
            let end_along = layer_start[to.rank];
            let gap_start = layer_start[r] + thickness[r] + after[r];
            let (a, b) = (port_from[p], port_to[p]);
            points.push(to_xy(a, start_along));
            if let Some(t) = track[p] {
                let along = gap_start + 1 + t;
                points.push(to_xy(a, along));
                points.push(to_xy(b, along));
            }
            points.push(to_xy(b, end_along));
            if let Some(text) = label_of(p) {
                let width = cell_len(&text) as i64;
                label = Some(if horizontal {
                    let region = gap_start + 1 + gap_tracks[r];
                    let left = region + (gap_labels[r] - width) / 2;
                    (text, left, b)
                } else {
                    let row = gap_start + 1 + gap_tracks[r] + label_slot[p];
                    (text, label_left[p], row)
                });
            }
        }
        points.dedup();
        let (start, end) = if reversed[index] {
            (edge.end, edge.start)
        } else {
            (edge.start, edge.end)
        };
        geometry.edges.push(EdgeGeo {
            points,
            stroke: edge.stroke,
            start,
            end,
            label,
        });
    }
    if let (Some(grouping), Some(across)) = (&grouping, &across) {
        // Outermost first.
        for &c in grouping.tree.deepest_first.iter().rev() {
            let (x0, y0) = to_xy(across[c].0, first[c]);
            let (x1, y1) = to_xy(across[c].1, last[c]);
            geometry.frames.push(FrameGeo {
                cluster: c,
                x: x0,
                y: y0,
                w: x1 - x0 + 1,
                h: y1 - y0 + 1,
                label: graph.clusters()[c]
                    .label
                    .as_deref()
                    .map(edge_text)
                    .filter(|label| !label.is_empty()),
            });
        }
    }
    geometry.notes = notes;
    Ok(geometry)
}

/// Give every cluster a point in each rank it spans: a placeholder where it
/// has none, which counts for the clusters around it too.
fn fill_spans(tree: &Tree, vertices: &mut Vec<Vertex>, of: &mut Vec<Option<usize>>) {
    let mut present: HashSet<(usize, usize)> = HashSet::new();
    let mut spans: HashMap<usize, (usize, usize)> = HashMap::new();
    for (v, vertex) in vertices.iter().enumerate() {
        for &c in tree.chain_of(of[v]) {
            present.insert((c, vertex.rank));
            let span = spans.entry(c).or_insert((vertex.rank, vertex.rank));
            span.0 = span.0.min(vertex.rank);
            span.1 = span.1.max(vertex.rank);
        }
    }
    for &c in &tree.deepest_first {
        let Some(&(lo, hi)) = spans.get(&c) else {
            continue;
        };
        for rank in lo..=hi {
            if present.contains(&(c, rank)) {
                continue;
            }
            vertices.push(Vertex {
                node: None,
                rank,
                cross: 1,
                along: 0,
            });
            of.push(Some(c));
            for &k in tree.chain_of(Some(c)) {
                present.insert((k, rank));
            }
        }
    }
}

/// Give each piece that changes position a track (a row, or a column
/// left-right) in the gap, and return how many tracks the gap needs.
///
/// Pieces whose spans overlap get different tracks. And where one piece leaves
/// a vertex at the position another arrives at, the leaving piece must turn
/// first (a lower track), or their runs along that position would overlap.
fn assign_tracks(
    here: &[usize],
    port_from: &[i64],
    port_to: &[i64],
    track: &mut [Option<i64>],
    skip: impl Fn(usize) -> bool,
) -> i64 {
    let mut routed: Vec<usize> = here
        .iter()
        .copied()
        .filter(|&p| port_from[p] != port_to[p] && !skip(p))
        .collect();
    routed.sort_by_key(|&p| {
        let (a, b) = (port_from[p], port_to[p]);
        (a.min(b), a.max(b), p)
    });
    // `before[i]`: pieces that must take a lower track than piece i.
    let count = routed.len();
    let mut leaving_at: std::collections::HashMap<i64, Vec<usize>> =
        std::collections::HashMap::new();
    for (j, &p) in routed.iter().enumerate() {
        leaving_at.entry(port_from[p]).or_default().push(j);
    }
    let before: Vec<Vec<usize>> = routed
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            leaving_at
                .get(&port_to[p])
                .map(|list| list.iter().copied().filter(|&j| j != i).collect())
                .unwrap_or_default()
        })
        .collect();
    let mut occupied: Vec<Vec<(i64, i64)>> = Vec::new();
    let mut assigned: Vec<Option<i64>> = vec![None; count];
    let mut done = 0;
    while done < count {
        // The first unassigned piece whose predecessors all have tracks; on a
        // cycle (two pieces swapping positions), the first unassigned one.
        let ready = (0..count)
            .filter(|&i| assigned[i].is_none())
            .find(|&i| before[i].iter().all(|&j| assigned[j].is_some()))
            .or_else(|| (0..count).find(|&i| assigned[i].is_none()))
            .expect("an unassigned piece remains");
        let p = routed[ready];
        let (lo, hi) = (port_from[p].min(port_to[p]), port_from[p].max(port_to[p]));
        let floor = before[ready]
            .iter()
            .filter_map(|&j| assigned[j])
            .map(|t| t + 1)
            .max()
            .unwrap_or(0);
        let mut slot = floor as usize;
        loop {
            if slot == occupied.len() {
                occupied.push(Vec::new());
            }
            if occupied[slot].iter().all(|&(l, h)| hi < l || h < lo) {
                break;
            }
            slot += 1;
        }
        occupied[slot].push((lo, hi));
        assigned[ready] = Some(slot as i64);
        track[p] = Some(slot as i64);
        done += 1;
    }
    occupied.len() as i64
}

/// Reorder every rank to reduce crossings, keeping the best order seen. With
/// a grouping, every order tried keeps each cluster's points together.
fn order_layers(
    layers: &mut [Vec<usize>],
    preds: &[Vec<usize>],
    succs: &[Vec<usize>],
    count: usize,
    grouping: Option<&Grouping<'_>>,
) {
    if let Some(grouping) = grouping {
        for layer in layers.iter_mut() {
            let items: Vec<(f64, usize)> = layer
                .iter()
                .enumerate()
                .map(|(i, &v)| (i as f64, v))
                .collect();
            *layer = grouping.arrange(&items);
        }
    }
    let mut index = vec![0usize; count];
    let reindex = |layers: &[Vec<usize>], index: &mut [usize]| {
        for layer in layers {
            for (i, &v) in layer.iter().enumerate() {
                index[v] = i;
            }
        }
    };
    reindex(layers, &mut index);
    let mut best = layers.to_vec();
    let mut best_crossings = crossings(layers, succs, &index);
    for sweep in 0..8 {
        if best_crossings == 0 {
            break;
        }
        let downward = sweep % 2 == 0;
        let ranks: Vec<usize> = if downward {
            (1..layers.len()).collect()
        } else {
            (0..layers.len().saturating_sub(1)).rev().collect()
        };
        for r in ranks {
            let neighbours = if downward { preds } else { succs };
            let mut keyed: Vec<(f64, usize, usize)> = layers[r]
                .iter()
                .map(|&v| {
                    let list = &neighbours[v];
                    let key = if list.is_empty() {
                        index[v] as f64
                    } else {
                        list.iter().map(|&u| index[u] as f64).sum::<f64>() / list.len() as f64
                    };
                    (key, index[v], v)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            layers[r] = match grouping {
                Some(grouping) => {
                    let items: Vec<(f64, usize)> = keyed.iter().map(|&(k, _, v)| (k, v)).collect();
                    grouping.arrange(&items)
                }
                None => keyed.into_iter().map(|(_, _, v)| v).collect(),
            };
            for (i, &v) in layers[r].iter().enumerate() {
                index[v] = i;
            }
        }
        let now = crossings(layers, succs, &index);
        if now < best_crossings {
            best_crossings = now;
            best = layers.to_vec();
        }
    }
    layers.clone_from_slice(&best);
}

/// Edge crossings between adjacent ranks: pairs of pieces whose ends are in
/// strictly opposite orders. Counted with a Fenwick tree, O(E log V) a rank.
fn crossings(layers: &[Vec<usize>], succs: &[Vec<usize>], index: &[usize]) -> usize {
    let mut total = 0;
    let mut tree: Vec<usize> = Vec::new();
    for layer in layers {
        let mut pairs: Vec<(usize, usize)> = layer
            .iter()
            .flat_map(|&v| succs[v].iter().map(move |&w| (index[v], index[w])))
            .collect();
        pairs.sort_unstable();
        let size = pairs.iter().map(|&(_, lower)| lower + 1).max().unwrap_or(0);
        tree.clear();
        tree.resize(size + 1, 0);
        // How many inserted pieces end at or before `lower`.
        let at_most = |tree: &[usize], lower: usize| {
            let mut i = lower + 1;
            let mut sum = 0;
            while i > 0 {
                sum += tree[i];
                i &= i - 1;
            }
            sum
        };
        let mut inserted = 0;
        let mut start = 0;
        while start < pairs.len() {
            // Pieces from the same upper vertex never cross each other.
            let upper = pairs[start].0;
            let end = start + pairs[start..].partition_point(|&(u, _)| u == upper);
            for &(_, lower) in &pairs[start..end] {
                total += inserted - at_most(&tree, lower);
            }
            for &(_, lower) in &pairs[start..end] {
                let mut i = lower + 1;
                while i <= size {
                    tree[i] += 1;
                    i += i & i.wrapping_neg();
                }
                inserted += 1;
            }
            start = end;
        }
    }
    total
}

/// Positions across each rank: start packed, then pull every vertex towards
/// the mean centre of its neighbours, keeping order and spacing.
fn place(
    layers: &[Vec<usize>],
    vertices: &[Vertex],
    preds: &[Vec<usize>],
    succs: &[Vec<usize>],
    gap: i64,
) -> Vec<i64> {
    let mut position = vec![0i64; vertices.len()];
    for layer in layers {
        let mut at = 0;
        for &v in layer {
            position[v] = at;
            at += vertices[v].cross + gap;
        }
    }
    for pass in 0..12 {
        let downward = pass % 2 == 0;
        let ranks: Vec<usize> = if downward {
            (0..layers.len()).collect()
        } else {
            (0..layers.len()).rev().collect()
        };
        for r in ranks {
            let layer = &layers[r];
            let neighbours = if downward { preds } else { succs };
            let desired: Vec<i64> = layer
                .iter()
                .map(|&v| {
                    let list = &neighbours[v];
                    if list.is_empty() {
                        position[v]
                    } else {
                        let sum: i64 = list
                            .iter()
                            .map(|&u| position[u] + vertices[u].cross / 2)
                            .sum();
                        let mean = (sum as f64 / list.len() as f64).round() as i64;
                        mean - vertices[v].cross / 2
                    }
                })
                .collect();
            let count = layer.len();
            let mut left = vec![0i64; count];
            for i in 0..count {
                left[i] = if i == 0 {
                    desired[0]
                } else {
                    desired[i].max(left[i - 1] + vertices[layer[i - 1]].cross + gap)
                };
            }
            let mut right = vec![0i64; count];
            for i in (0..count).rev() {
                right[i] = if i + 1 == count {
                    desired[i]
                } else {
                    desired[i].min(right[i + 1] - vertices[layer[i]].cross - gap)
                };
            }
            for i in 0..count {
                position[layer[i]] = (left[i] + right[i]).div_euclid(2);
            }
        }
    }
    // Normalise so the leftmost vertex sits at 0.
    let min = position.iter().copied().min().unwrap_or(0);
    for p in &mut position {
        *p -= min;
    }
    position
}

// ------------------------------------------------------------------ render

#[derive(Clone, Debug)]
enum Cell {
    Empty,
    Line(u8, Stroke),
    Glyph(String),
    /// The second cell of a wide character.
    Wide,
    /// Part of a cluster frame.
    Frame(char),
}

struct Canvas {
    cells: Vec<Vec<Cell>>,
    ascii: bool,
}

impl Canvas {
    fn new(width: i64, height: i64, ascii: bool) -> Self {
        Canvas {
            cells: vec![vec![Cell::Empty; width as usize]; height as usize],
            ascii,
        }
    }

    fn get(&mut self, x: i64, y: i64) -> Option<&mut Cell> {
        if x < 0 || y < 0 {
            return None;
        }
        self.cells.get_mut(y as usize)?.get_mut(x as usize)
    }

    fn bits(&mut self, x: i64, y: i64, bits: u8, stroke: Stroke) {
        if let Some(cell) = self.get(x, y) {
            match cell {
                Cell::Line(old, old_stroke) => {
                    *old |= bits;
                    if *old_stroke == Stroke::Solid && stroke != Stroke::Solid {
                        *old_stroke = stroke;
                    }
                }
                Cell::Empty => *cell = Cell::Line(bits, stroke),
                Cell::Glyph(_) | Cell::Wide | Cell::Frame(_) => {}
            }
        }
    }

    fn glyph(&mut self, x: i64, y: i64, glyph: &str) {
        if let Some(cell) = self.get(x, y) {
            *cell = Cell::Glyph(glyph.to_string());
        }
    }

    /// Write text left to right from (x, y), clearing whatever was there.
    fn text(&mut self, x: i64, y: i64, text: &str) {
        let mut at = x;
        for c in text.chars() {
            match char_cell_width(c) {
                0 => {
                    if let Some(Cell::Glyph(previous)) = self.get(at - 1, y) {
                        previous.push(c);
                    }
                }
                width => {
                    self.glyph(at, y, &c.to_string());
                    if width == 2 {
                        if let Some(cell) = self.get(at + 1, y) {
                            *cell = Cell::Wide;
                        }
                    }
                    at += width as i64;
                }
            }
        }
    }

    fn line_char(&self, bits: u8, stroke: Stroke) -> char {
        let vertical = bits & (E | W) == 0;
        let horizontal = bits & (N | S) == 0;
        if self.ascii {
            return match (vertical, horizontal, stroke) {
                (true, _, Stroke::Dotted) => ':',
                (_, true, Stroke::Dotted) => '.',
                (_, true, Stroke::Thick) => '=',
                (true, _, _) => '|',
                (_, true, _) => '-',
                _ => '+',
            };
        }
        if vertical {
            return match stroke {
                Stroke::Thick => '┃',
                Stroke::Dotted => '┆',
                _ => '│',
            };
        }
        if horizontal {
            return match stroke {
                Stroke::Thick => '━',
                Stroke::Dotted => '┄',
                _ => '─',
            };
        }
        match bits {
            b if b == E | S => '┌',
            b if b == W | S => '┐',
            b if b == N | E => '└',
            b if b == N | W => '┘',
            b if b == N | S | E => '├',
            b if b == N | S | W => '┤',
            b if b == E | W | S => '┬',
            b if b == E | W | N => '┴',
            _ => '┼',
        }
    }

    fn lines(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                let mut line = String::new();
                for cell in row {
                    match cell {
                        Cell::Empty => line.push(' '),
                        Cell::Line(bits, stroke) => line.push(self.line_char(*bits, *stroke)),
                        Cell::Glyph(text) => line.push_str(text),
                        Cell::Wide => {}
                        Cell::Frame(c) => line.push(*c),
                    }
                }
                line.trim_end().to_string()
            })
            .collect()
    }
}

/// Corner and side characters for a node shape.
struct Frame {
    /// Top-left, top-right, bottom-left, bottom-right; `None` draws a line
    /// corner that edges can join.
    corners: [Option<&'static str>; 4],
    /// Left and right sides; `None` draws a line.
    sides: [Option<&'static str>; 2],
}

fn frame(shape: Shape, ascii: bool) -> Frame {
    let round = if ascii {
        [Some("."), Some("."), Some("'"), Some("'")]
    } else {
        [Some("╭"), Some("╮"), Some("╰"), Some("╯")]
    };
    let slanted = if ascii {
        [Some("/"), Some("\\"), Some("\\"), Some("/")]
    } else {
        [Some("╱"), Some("╲"), Some("╲"), Some("╱")]
    };
    let square = [None; 4];
    match shape {
        Shape::Rect | Shape::Subroutine | Shape::Table => Frame {
            corners: square,
            sides: [None, None],
        },
        Shape::Round | Shape::Cylinder => Frame {
            corners: round,
            sides: [None, None],
        },
        Shape::Stadium | Shape::Circle | Shape::DoubleCircle => Frame {
            corners: round,
            sides: [Some("("), Some(")")],
        },
        Shape::Asymmetric => Frame {
            corners: square,
            sides: [Some(">"), None],
        },
        Shape::Rhombus => Frame {
            corners: slanted,
            sides: [Some("<"), Some(">")],
        },
        Shape::Hexagon => Frame {
            corners: slanted,
            sides: [None, None],
        },
        Shape::Parallelogram => Frame {
            corners: square,
            sides: [Some("/"), Some("/")],
        },
        Shape::ParallelogramAlt => Frame {
            corners: square,
            sides: [Some("\\"), Some("\\")],
        },
        Shape::Trapezoid => Frame {
            corners: square,
            sides: [Some("/"), Some("\\")],
        },
        Shape::TrapezoidAlt => Frame {
            corners: square,
            sides: [Some("\\"), Some("/")],
        },
    }
}

fn head_glyph(head: Head, dx: i64, dy: i64, ascii: bool) -> Option<&'static str> {
    Some(match (head, ascii) {
        (Head::None, _) => return None,
        (Head::Circle, false) => "●",
        (Head::Circle, true) => "o",
        (Head::Cross, false) => "×",
        (Head::Cross, true) => "x",
        (Head::Arrow, false) => match (dx.signum(), dy.signum()) {
            (0, 1) => "▼",
            (0, _) => "▲",
            (1, _) => "►",
            _ => "◄",
        },
        (Head::Arrow, true) => match (dx.signum(), dy.signum()) {
            (0, 1) => "v",
            (0, _) => "^",
            (1, _) => ">",
            _ => "<",
        },
    })
}

fn render(graph: &Graph, geometry: &Geometry, ascii: bool, width: i64, height: i64) -> Drawing {
    let mut canvas = Canvas::new(width, height, ascii);

    for b in &geometry.boxes {
        let node = &graph.nodes()[b.node];
        let frame = frame(node.shape, ascii);
        let (x0, y0, x1, y1) = (b.x, b.y, b.x + b.w - 1, b.y + b.h - 1);
        for x in x0 + 1..x1 {
            canvas.bits(x, y0, E | W, Stroke::Solid);
            canvas.bits(x, y1, E | W, Stroke::Solid);
        }
        for y in y0 + 1..y1 {
            for (side, x, bits) in [(frame.sides[0], x0, N | S), (frame.sides[1], x1, N | S)] {
                match side {
                    Some(glyph) => canvas.glyph(x, y, glyph),
                    None => canvas.bits(x, y, bits, Stroke::Solid),
                }
            }
        }
        let corners = [
            (x0, y0, E | S),
            (x1, y0, W | S),
            (x0, y1, N | E),
            (x1, y1, N | W),
        ];
        for (corner, (x, y, bits)) in frame.corners.iter().zip(corners) {
            match corner {
                Some(glyph) => canvas.glyph(x, y, glyph),
                None => canvas.bits(x, y, bits, Stroke::Solid),
            }
        }
        if node.shape == Shape::Subroutine {
            for x in [x0 + 1, x1 - 1] {
                canvas.bits(x, y0, S, Stroke::Solid);
                canvas.bits(x, y1, N, Stroke::Solid);
                for y in y0 + 1..y1 {
                    canvas.bits(x, y, N | S, Stroke::Solid);
                }
            }
        }
        let lines: Vec<&str> = node.label.split('\n').collect();
        if node.shape == Shape::Table && lines.len() > 1 {
            // The header centred at the top, a rule, the rows left-aligned.
            let left = x0 + (b.w - cell_len(lines[0]) as i64) / 2;
            canvas.text(left, y0 + 1, lines[0]);
            canvas.bits(x0, y0 + 2, N | S | E, Stroke::Solid);
            canvas.bits(x1, y0 + 2, N | S | W, Stroke::Solid);
            for x in x0 + 1..x1 {
                canvas.bits(x, y0 + 2, E | W, Stroke::Solid);
            }
            for (i, line) in lines[1..].iter().enumerate() {
                canvas.text(x0 + 2, y0 + 3 + i as i64, line);
            }
            continue;
        }
        let top = y0 + 1 + (b.h - 2 - lines.len() as i64) / 2;
        for (i, line) in lines.iter().enumerate() {
            let left = x0 + (b.w - cell_len(line) as i64) / 2;
            canvas.text(left, top + i as i64, line);
        }
    }

    for edge in &geometry.edges {
        if edge.stroke == Stroke::Invisible || edge.points.len() < 2 {
            continue;
        }
        let last = edge.points.len() - 1;
        for (i, pair) in edge.points.windows(2).enumerate() {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            let (dx, dy) = ((x1 - x0).signum(), (y1 - y0).signum());
            let (forward, backward) = match (dx, dy) {
                (1, _) => (E, W),
                (-1, _) => (W, E),
                (_, 1) => (S, N),
                _ => (N, S),
            };
            let (mut x, mut y) = (x0, y0);
            while (x, y) != (x1, y1) {
                let at_start = i == 0 && (x, y) == (x0, y0);
                if !(at_start && edge.start != Head::None) {
                    canvas.bits(x, y, forward, edge.stroke);
                }
                x += dx;
                y += dy;
                let at_end = i + 1 == last && (x, y) == (x1, y1);
                if !(at_end && edge.end != Head::None) {
                    canvas.bits(x, y, backward, edge.stroke);
                }
            }
        }
        // Heads sit on the cell next to each end's border.
        let (fx, fy) = edge.points[0];
        let (sx, sy) = edge.points[1];
        let (dx, dy) = ((sx - fx).signum(), (sy - fy).signum());
        if let Some(glyph) = head_glyph(edge.start, -dx, -dy, ascii) {
            canvas.glyph(fx + dx, fy + dy, glyph);
        }
        let (lx, ly) = edge.points[last];
        let (px, py) = edge.points[last - 1];
        let (dx, dy) = ((lx - px).signum(), (ly - py).signum());
        if let Some(glyph) = head_glyph(edge.end, dx, dy, ascii) {
            canvas.glyph(lx - dx, ly - dy, glyph);
        }
    }

    for edge in &geometry.edges {
        if let Some((text, x, y)) = &edge.label {
            canvas.text(*x, *y, text);
        }
    }
    for &(x, y) in &geometry.loops {
        if matches!(canvas.get(x, y), Some(Cell::Empty)) {
            canvas.glyph(x, y, if ascii { "@" } else { "↻" });
        }
    }

    for frame in &geometry.frames {
        draw_frame(&mut canvas, frame);
    }

    let lines = canvas.lines();
    let width = lines.iter().map(|l| cell_len(l)).max().unwrap_or(0);
    Drawing {
        lines,
        width,
        notes: Vec::new(),
    }
}

/// Draw a cluster frame in the cells nothing else uses, so the edges that
/// cross it stay whole, and its label in the top border where no edge
/// crosses (or the bottom border, or over the top one's edges as a last
/// resort).
fn draw_frame(canvas: &mut Canvas, frame: &FrameGeo) {
    let (x0, y0, x1, y1) = (
        frame.x,
        frame.y,
        frame.x + frame.w - 1,
        frame.y + frame.h - 1,
    );
    let [horizontal, vertical, top_left, top_right, bottom_left, bottom_right] = if canvas.ascii {
        ['-', ':', '+', '+', '+', '+']
    } else {
        ['╌', '╎', '┌', '┐', '└', '┘']
    };
    let mut put = |x: i64, y: i64, c: char| {
        if let Some(cell) = canvas.get(x, y) {
            if matches!(cell, Cell::Empty) {
                *cell = Cell::Frame(c);
            }
        }
    };
    for x in x0 + 1..x1 {
        put(x, y0, horizontal);
        put(x, y1, horizontal);
    }
    for y in y0 + 1..y1 {
        put(x0, y, vertical);
        put(x1, y, vertical);
    }
    put(x0, y0, top_left);
    put(x1, y0, top_right);
    put(x0, y1, bottom_left);
    put(x1, y1, bottom_right);
    let Some(label) = &frame.label else {
        return;
    };
    // ` label ` between the corners, after one border cell when it fits.
    let room = frame.w - 2;
    if room < 3 {
        return;
    }
    let text = format!(" {} ", fit(label, (room - 2) as usize, canvas.ascii));
    let width = cell_len(&text) as i64;
    let free = |canvas: &mut Canvas, left: i64, y: i64| {
        (left..left + width).all(|x| matches!(canvas.get(x, y), Some(Cell::Frame(_))))
    };
    let first = if width < room { x0 + 2 } else { x0 + 1 };
    // The top border; the bottom one if edges cross the top everywhere; the
    // top regardless if they cross both.
    let (left, y) = [y0, y1]
        .into_iter()
        .find_map(|y| {
            (first..=x1 - width)
                .find(|&left| free(canvas, left, y))
                .map(|left| (left, y))
        })
        .unwrap_or((first, y0));
    canvas.text(left, y, &text);
}

/// `text` cut to `width` cells, ending in an ellipsis when cut.
fn fit(text: &str, width: usize, ascii: bool) -> String {
    if cell_len(text) <= width {
        return text.to_string();
    }
    let ellipsis = if ascii { "~" } else { "…" };
    let mut out = String::new();
    for c in text.chars() {
        if cell_len(&out) + char_cell_width(c) + 1 > width {
            break;
        }
        out.push(c);
    }
    out.push_str(ellipsis);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pairwise count the Fenwick tree replaces.
    fn crossings_by_pairs(layers: &[Vec<usize>], succs: &[Vec<usize>], index: &[usize]) -> usize {
        let mut total = 0;
        for layer in layers {
            let pairs: Vec<(usize, usize)> = layer
                .iter()
                .flat_map(|&v| succs[v].iter().map(move |&w| (index[v], index[w])))
                .collect();
            for (i, a) in pairs.iter().enumerate() {
                for b in &pairs[i + 1..] {
                    if (a.0 < b.0 && a.1 > b.1) || (b.0 < a.0 && b.1 > a.1) {
                        total += 1;
                    }
                }
            }
        }
        total
    }

    #[test]
    fn crossings_match_the_pairwise_count() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        for _ in 0..200 {
            let widths: Vec<usize> = (0..1 + next(5)).map(|_| 1 + next(7)).collect();
            let mut layers: Vec<Vec<usize>> = Vec::new();
            let mut count = 0;
            for &w in &widths {
                layers.push((count..count + w).collect());
                count += w;
            }
            let mut succs = vec![Vec::new(); count];
            for r in 0..layers.len() - 1 {
                for _ in 0..next(12) {
                    let u = layers[r][next(layers[r].len())];
                    let w = layers[r + 1][next(layers[r + 1].len())];
                    succs[u].push(w);
                }
            }
            let mut index = vec![0; count];
            for layer in &mut layers {
                // Shuffle so indexes differ from vertex order.
                for i in (1..layer.len()).rev() {
                    layer.swap(i, next(i + 1));
                }
                for (i, &v) in layer.iter().enumerate() {
                    index[v] = i;
                }
            }
            assert_eq!(
                crossings(&layers, &succs, &index),
                crossings_by_pairs(&layers, &succs, &index)
            );
        }
    }

    /// Random graphs with random (nested) clusters and same-rank groups:
    /// every frame holds its cluster's boxes and clears everything else, and
    /// every honoured group shares a rank.
    #[test]
    fn frames_hold_their_clusters_and_clear_the_rest() {
        use crate::graph::{Cluster, Edge, Node};
        let mut seed = 0x51_7cc1_b727_220au64;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound as u64) as usize
        };
        let directions = [
            Direction::TopDown,
            Direction::LeftRight,
            Direction::BottomUp,
            Direction::RightLeft,
        ];
        for case in 0..400 {
            let n = 1 + next(12);
            let nodes = (0..n)
                .map(|i| Node::new(format!("n{i}"), "x".repeat(1 + next(6))))
                .collect();
            let edges = (0..next(2 * n + 1))
                .map(|_| {
                    let mut edge = Edge::new(next(n), next(n));
                    if next(4) == 0 {
                        edge.label = Some("label".into());
                    }
                    edge.length = 1 + next(2);
                    edge
                })
                .collect();
            let mut graph = Graph::from_parts(directions[next(4)], nodes, edges);
            for c in 0..next(4) {
                let mut cluster = Cluster::new(format!("c{c}"))
                    .nodes((0..1 + next(3)).map(|_| next(n)).collect::<Vec<_>>());
                if next(3) == 0 {
                    cluster = cluster.label("Group name");
                }
                if c > 0 && next(2) == 0 {
                    cluster = cluster.parent(next(c));
                }
                graph.add_cluster(cluster);
            }
            for _ in 0..next(3) {
                graph.add_same_rank((0..2).map(|_| next(n)).collect::<Vec<_>>());
            }
            let horizontal = graph.direction().is_horizontal();
            let geometry = layout(&graph, horizontal).unwrap();
            let mut notes = Vec::new();
            let Some(tree) = Tree::new(&graph, &mut notes) else {
                assert!(geometry.frames.is_empty(), "case {case}");
                continue;
            };
            if geometry
                .notes
                .iter()
                .any(|note| note.contains("placed apart"))
            {
                panic!("case {case}: {:?}", geometry.notes);
            }
            let inside = |c: usize, node: usize| tree.chain_of(tree.node[node]).contains(&c);
            let rect = |x: i64, y: i64, w: i64, h: i64| (x, y, x + w - 1, y + h - 1);
            let within = |a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)| {
                a.0 > b.0 && a.1 > b.1 && a.2 < b.2 && a.3 < b.3
            };
            let apart = |a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)| {
                a.2 < b.0 || b.2 < a.0 || a.3 < b.1 || b.3 < a.1
            };
            assert_eq!(
                geometry.frames.len(),
                tree.deepest_first.len(),
                "case {case}"
            );
            for frame in &geometry.frames {
                let f = rect(frame.x, frame.y, frame.w, frame.h);
                for b in &geometry.boxes {
                    let r = rect(b.x, b.y, b.w, b.h);
                    if inside(frame.cluster, b.node) {
                        assert!(within(r, f), "case {case}: {} outside its frame", b.node);
                    } else {
                        assert!(apart(r, f), "case {case}: {} inside a frame", b.node);
                    }
                }
                for other in &geometry.frames {
                    if other.cluster == frame.cluster {
                        continue;
                    }
                    let o = rect(other.x, other.y, other.w, other.h);
                    let nested = tree.chain_of(Some(other.cluster)).contains(&frame.cluster);
                    let outer = tree.chain_of(Some(frame.cluster)).contains(&other.cluster);
                    if nested {
                        assert!(within(o, f), "case {case}: a nested frame sticks out");
                    } else if !outer {
                        assert!(apart(o, f), "case {case}: frames overlap");
                    }
                }
            }
            let (rep, _) = graph.rank_sets();
            for a in &geometry.boxes {
                for b in &geometry.boxes {
                    if rep[a.node] == rep[b.node] {
                        let along = |b: &BoxGeo| if horizontal { b.x } else { b.y };
                        assert_eq!(along(a), along(b), "case {case}: a same-rank group split");
                    }
                }
            }
        }
    }
}
