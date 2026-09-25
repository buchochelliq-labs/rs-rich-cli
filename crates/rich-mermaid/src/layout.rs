//! Lay out a [`Flowchart`] in ranks and draw it with box-drawing characters.
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

use crate::flowchart::{Direction, Flowchart, Head, Shape, Stroke};
use rich::cells::{cell_len, char_cell_width};

/// A drawn diagram: lines of text without trailing spaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagram {
    pub lines: Vec<String>,
    /// The widest line, in cells.
    pub width: usize,
}

/// The most cells a diagram may occupy before it is refused as too large.
const MAX_CELLS: i64 = 2_000_000;

const N: u8 = 1;
const E: u8 = 2;
const S: u8 = 4;
const W: u8 = 8;

/// Lay out and draw `chart`. `ascii` restricts the output to ASCII.
pub fn draw(chart: &Flowchart, ascii: bool) -> Result<Diagram, String> {
    if chart.nodes.is_empty() {
        return Ok(Diagram {
            lines: Vec::new(),
            width: 0,
        });
    }
    let horizontal = chart.direction.is_horizontal();
    let mut geometry = layout(chart, horizontal);
    if matches!(chart.direction, Direction::BottomUp | Direction::RightLeft) {
        geometry.flip(horizontal);
    }
    geometry.normalize();
    let (width, height) = geometry.extent();
    if width.saturating_mul(height) > MAX_CELLS {
        return Err(format!("it would take {width} × {height} cells to draw"));
    }
    Ok(render(chart, &geometry, ascii, width, height))
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

#[derive(Clone, Debug, Default)]
struct Geometry {
    boxes: Vec<BoxGeo>,
    edges: Vec<EdgeGeo>,
    /// Self-loop markers.
    loops: Vec<(i64, i64)>,
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
    /// Index into the chart's edges.
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

fn layout(chart: &Flowchart, horizontal: bool) -> Geometry {
    let n = chart.nodes.len();

    // 1. Break cycles: reverse the edges a DFS finds going back.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (index, edge) in chart.edges.iter().enumerate() {
        if edge.from != edge.to {
            out[edge.from].push(index);
        }
    }
    let mut reversed = vec![false; chart.edges.len()];
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
                let to = chart.edges[edge].to;
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
    // Oriented edges: (upper, lower, minimum rank span, chart edge).
    let oriented: Vec<(usize, usize, usize, usize)> = chart
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| e.from != e.to)
        .map(|(i, e)| {
            let (u, v) = if reversed[i] {
                (e.to, e.from)
            } else {
                (e.from, e.to)
            };
            (u, v, e.length.max(1), i)
        })
        .collect();

    // 2. Rank by longest path, then pull sources down next to their targets.
    let mut indegree = vec![0usize; n];
    let mut successors: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for &(u, v, len, _) in &oriented {
        indegree[v] += 1;
        successors[u].push((v, len));
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

    // 3. Vertices (nodes, then dummies) and rank-to-rank pieces.
    let mut vertices: Vec<Vertex> = chart
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let (tw, th) = text_size(&node.label);
            let extra = if node.shape == Shape::Subroutine {
                2
            } else {
                0
            };
            let (mut w, h) = (tw + 4 + extra, th + 2);
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
    // For each chart edge: its pieces in order, upper to lower.
    let mut chains: Vec<Vec<usize>> = vec![Vec::new(); chart.edges.len()];
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

    // Widen nodes so every piece gets its own port.
    let mut ins = vec![0i64; vertices.len()];
    let mut outs = vec![0i64; vertices.len()];
    for piece in &pieces {
        outs[piece.from] += 1;
        ins[piece.to] += 1;
    }
    for (v, vertex) in vertices.iter_mut().enumerate() {
        if vertex.node.is_some() {
            let ports = ins[v].max(outs[v]).max(1);
            let needed = if horizontal { ports + 2 } else { 2 * ports + 1 };
            vertex.cross = vertex.cross.max(needed);
            if !horizontal && vertex.cross % 2 == 0 {
                vertex.cross += 1;
            }
        }
    }

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

    // 4. Order each rank by barycentre sweeps.
    order_layers(&mut layers, &preds, &succs, vertices.len());

    // 5. Positions across the rank.
    let gap = if horizontal { 1 } else { 2 };
    let position = place(&layers, &vertices, &preds, &succs, gap);
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
            let interior = vertices[v].cross - 2;
            let span = (k - 1) * spacing + 1;
            let start = position[v] + 1 + (interior - span).max(0) / 2;
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
        let edge = &chart.edges[piece.edge];
        let last = vertices[piece.to].node.is_some();
        match (&edge.label, last, edge.stroke) {
            (Some(text), true, stroke) if stroke != Stroke::Invisible && !text.is_empty() => {
                Some(edge_text(text))
            }
            _ => None,
        }
    };
    for r in 0..ranks.saturating_sub(1) {
        let here: Vec<usize> = (0..pieces.len())
            .filter(|&p| vertices[pieces[p].from].rank == r)
            .collect();
        gap_tracks[r] = assign_tracks(&here, &port_from, &port_to, &mut track, |p| {
            chart.edges[pieces[p].edge].stroke == Stroke::Invisible
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
                .filter(|&&p| chart.edges[pieces[p].edge].stroke != Stroke::Invisible)
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
    let mut layer_start = vec![0i64; ranks];
    for r in 1..ranks {
        layer_start[r] =
            layer_start[r - 1] + thickness[r - 1] + gap_tracks[r - 1] + gap_labels[r - 1] + 2;
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
    for (index, edge) in chart.edges.iter().enumerate() {
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
            let gap_start = layer_start[r] + thickness[r];
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
    geometry
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
    let mut before: Vec<Vec<usize>> = vec![Vec::new(); count];
    for i in 0..count {
        for j in 0..count {
            if i != j && port_from[routed[j]] == port_to[routed[i]] {
                before[i].push(j);
            }
        }
    }
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

/// Reorder every rank to reduce crossings, keeping the best order seen.
fn order_layers(
    layers: &mut [Vec<usize>],
    preds: &[Vec<usize>],
    succs: &[Vec<usize>],
    count: usize,
) {
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
            layers[r] = keyed.into_iter().map(|(_, _, v)| v).collect();
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

/// Edge crossings between adjacent ranks.
fn crossings(layers: &[Vec<usize>], succs: &[Vec<usize>], index: &[usize]) -> usize {
    let mut total = 0;
    for layer in layers {
        let mut pairs: Vec<(usize, usize)> = layer
            .iter()
            .flat_map(|&v| succs[v].iter().map(move |&w| (index[v], index[w])))
            .collect();
        pairs.sort_unstable();
        for i in 0..pairs.len() {
            for j in i + 1..pairs.len() {
                if pairs[i].0 < pairs[j].0 && pairs[i].1 > pairs[j].1 {
                    total += 1;
                }
            }
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
                Cell::Glyph(_) | Cell::Wide => {}
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
        Shape::Rect | Shape::Subroutine => Frame {
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

fn render(chart: &Flowchart, geometry: &Geometry, ascii: bool, width: i64, height: i64) -> Diagram {
    let mut canvas = Canvas::new(width, height, ascii);

    for b in &geometry.boxes {
        let node = &chart.nodes[b.node];
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

    let lines = canvas.lines();
    let width = lines.iter().map(|l| cell_len(l)).max().unwrap_or(0);
    Diagram { lines, width }
}
