//! Cluster frames for the layered layout: which cluster each point of the
//! layout belongs to, keeping a cluster's points together in each rank, and
//! moving them apart across the rank until no frame overlaps what is outside
//! it.
//!
//! [`layout`](crate::layout) uses this only for a graph with clusters, so a
//! graph without them is laid out exactly as before.

use std::collections::HashMap;

use crate::graph::Graph;

/// The empty cells between a frame and anything outside it.
pub(crate) const PAD_OUT: i64 = 1;

/// The most points the separation moves, and the most passes it makes,
/// before it gives up and the frames are not drawn.
const MAX_SHIFTS: usize = 20_000;
const MAX_PASSES: usize = 500;

/// The clusters a graph draws, as a tree.
pub(crate) struct Tree {
    /// For every graph cluster: its ancestors, outermost first, then itself.
    chain: Vec<Vec<usize>>,
    /// Drawn clusters (those with a node, nested ones' included), deepest
    /// first, so a pass in this order sees children before their parents.
    pub deepest_first: Vec<usize>,
    /// The drawn children of every cluster.
    pub children: Vec<Vec<usize>>,
    /// The innermost cluster of every node.
    pub node: Vec<Option<usize>>,
}

impl Tree {
    /// The tree of `graph`'s clusters, or `None` when it draws no frame. A
    /// node listed in clusters that do not nest goes in the deepest (the
    /// first, at equal depth), with a note for each such node.
    pub fn new(graph: &Graph, notes: &mut Vec<String>) -> Option<Tree> {
        let clusters = graph.clusters();
        if clusters.is_empty() {
            return None;
        }
        let n = graph.nodes().len();
        let mut chain: Vec<Vec<usize>> = Vec::with_capacity(clusters.len());
        for (index, cluster) in clusters.iter().enumerate() {
            // A parent must come first; anything else would allow a cycle.
            let mut own = match cluster.parent.filter(|&parent| parent < index) {
                Some(parent) => chain[parent].clone(),
                None => Vec::new(),
            };
            own.push(index);
            chain.push(own);
        }
        let mut listed: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (index, cluster) in clusters.iter().enumerate() {
            for &v in &cluster.nodes {
                if v < n && !listed[v].contains(&index) {
                    listed[v].push(index);
                }
            }
        }
        let name = |c: usize| {
            let cluster = &clusters[c];
            cluster.label.clone().unwrap_or_else(|| cluster.id.clone())
        };
        let mut node = vec![None; n];
        for v in 0..n {
            let mut best: Option<usize> = None;
            for &c in &listed[v] {
                if best.is_none_or(|b| chain[c].len() > chain[b].len()) {
                    best = Some(c);
                }
            }
            node[v] = best;
            if let Some(best) = best {
                if let Some(&other) = listed[v].iter().find(|c| !chain[best].contains(c)) {
                    notes.push(format!(
                        "node `{}` is in clusters that do not nest ({} and {}); it is framed in {}",
                        graph.nodes()[v].id,
                        name(best),
                        name(other),
                        name(best)
                    ));
                }
            }
        }
        let mut alive = vec![false; clusters.len()];
        for c in node.iter().flatten() {
            for &k in &chain[*c] {
                alive[k] = true;
            }
        }
        if !alive.contains(&true) {
            return None;
        }
        let mut children = vec![Vec::new(); clusters.len()];
        let mut deepest_first: Vec<usize> = (0..clusters.len()).filter(|&c| alive[c]).collect();
        for &c in &deepest_first {
            if let [.., parent, _] = chain[c][..] {
                children[parent].push(c);
            }
        }
        deepest_first.sort_by_key(|&c| (std::cmp::Reverse(chain[c].len()), c));
        Some(Tree {
            chain,
            deepest_first,
            children,
            node,
        })
    }

    /// The clusters around a point in cluster `c`, outermost first.
    pub fn chain_of(&self, c: Option<usize>) -> &[usize] {
        c.map_or(&[], |c| &self.chain[c])
    }

    /// The innermost cluster holding both `a` and `b`.
    pub fn common(&self, a: Option<usize>, b: Option<usize>) -> Option<usize> {
        let (a, b) = (self.chain_of(a), self.chain_of(b));
        a.iter()
            .zip(b)
            .take_while(|(x, y)| x == y)
            .last()
            .map(|(x, _)| *x)
    }
}

/// How the clusters of one layout sit: every point's innermost cluster, the
/// points inside each cluster, and the left-to-right order of clusters that
/// share a parent, which every rank keeps.
pub(crate) struct Grouping<'a> {
    pub tree: &'a Tree,
    /// The innermost cluster of every point.
    pub of: Vec<Option<usize>>,
    /// For every cluster: the points directly in it.
    pub own: Vec<Vec<usize>>,
    /// For every cluster: the points in it, nested clusters' included.
    pub inside: Vec<Vec<usize>>,
    /// Every cluster's place among its siblings.
    order: Vec<usize>,
}

impl<'a> Grouping<'a> {
    pub fn new(tree: &'a Tree, of: Vec<Option<usize>>) -> Self {
        let count = tree.chain.len();
        let mut own = vec![Vec::new(); count];
        let mut inside = vec![Vec::new(); count];
        for (v, c) in of.iter().enumerate() {
            if let Some(c) = *c {
                own[c].push(v);
            }
            for &k in tree.chain_of(*c) {
                inside[k].push(v);
            }
        }
        Grouping {
            tree,
            of,
            own,
            inside,
            order: (0..count).collect(),
        }
    }

    /// Order siblings by where their points sit on average in `layers`, so
    /// the constrained ordering starts close to the unconstrained one.
    pub fn order_siblings(&mut self, layers: &[Vec<usize>]) {
        let mut index = vec![0usize; self.of.len()];
        for layer in layers {
            for (i, &v) in layer.iter().enumerate() {
                index[v] = i;
            }
        }
        let mean = |c: usize| {
            let points = &self.inside[c];
            points.iter().map(|&v| index[v] as f64).sum::<f64>() / points.len().max(1) as f64
        };
        let mut sorted: Vec<(f64, usize)> = (0..self.order.len()).map(|c| (mean(c), c)).collect();
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (place, (_, c)) in sorted.into_iter().enumerate() {
            self.order[c] = place;
        }
    }

    /// The rank's points, given in the order their keys want, regrouped so
    /// each cluster's points are contiguous: a cluster's block goes where
    /// its points' mean key puts it, and sibling blocks then take those
    /// places in their fixed order.
    pub fn arrange(&self, items: &[(f64, usize)]) -> Vec<usize> {
        let mut out = Vec::with_capacity(items.len());
        self.arrange_level(items, 0, &mut out);
        out
    }

    fn arrange_level(&self, items: &[(f64, usize)], depth: usize, out: &mut Vec<usize>) {
        enum Slot {
            Point(usize),
            Block(usize, Vec<(f64, usize)>),
        }
        let mut slots: Vec<(f64, usize, Slot)> = Vec::new();
        let mut block_at: HashMap<usize, usize> = HashMap::new();
        for (i, &(key, v)) in items.iter().enumerate() {
            match self.tree.chain_of(self.of[v]).get(depth) {
                None => slots.push((key, i, Slot::Point(v))),
                Some(&c) => {
                    let at = *block_at.entry(c).or_insert_with(|| {
                        slots.push((0.0, i, Slot::Block(c, Vec::new())));
                        slots.len() - 1
                    });
                    slots[at].0 += key;
                    if let Slot::Block(_, members) = &mut slots[at].2 {
                        members.push((key, v));
                    }
                }
            }
        }
        for slot in &mut slots {
            if let Slot::Block(_, members) = &slot.2 {
                slot.0 /= members.len() as f64;
            }
        }
        slots.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let places: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| matches!(slot.2, Slot::Block(..)))
            .map(|(i, _)| i)
            .collect();
        let mut blocks: Vec<(f64, usize, Slot)> = Vec::with_capacity(places.len());
        for &i in places.iter().rev() {
            blocks.push(slots.remove(i));
        }
        blocks.sort_by_key(|slot| match slot.2 {
            Slot::Block(c, _) => self.order[c],
            Slot::Point(_) => usize::MAX,
        });
        for (&i, block) in places.iter().zip(blocks) {
            slots.insert(i, block);
        }
        for (_, _, slot) in slots {
            match slot {
                Slot::Point(v) => out.push(v),
                Slot::Block(_, members) => self.arrange_level(&members, depth + 1, out),
            }
        }
    }

    /// Every drawn cluster's frame across the ranks, `(first, last)` cell of
    /// its sides, from the points' positions and sizes. A frame clears its
    /// contents by `pad` cells and is at least `min_width[c]` cells wide.
    pub fn extents(
        &self,
        position: &[i64],
        cross: &[i64],
        pad: i64,
        min_width: &[i64],
    ) -> Vec<(i64, i64)> {
        let mut extent = vec![(0i64, -1i64); self.own.len()];
        for &c in &self.tree.deepest_first {
            let mut lo = i64::MAX;
            let mut hi = i64::MIN;
            for &v in &self.own[c] {
                lo = lo.min(position[v]);
                hi = hi.max(position[v] + cross[v] - 1);
            }
            for &k in &self.tree.children[c] {
                lo = lo.min(extent[k].0);
                hi = hi.max(extent[k].1);
            }
            let left = lo - 1 - pad;
            extent[c] = (left, (hi + 1 + pad).max(left + min_width[c] - 1));
        }
        extent
    }

    /// Push points right until every rank keeps its spacing and no frame
    /// overlaps a point or frame outside it: `gap` cells between two points,
    /// [`PAD_OUT`] between a frame and its neighbour. Every move is to the
    /// right and keeps the order, so each rank stays valid. Returns the
    /// frames, or `None` (positions untouched) when it does not settle.
    pub fn separate(
        &self,
        layers: &[Vec<usize>],
        position: &mut [i64],
        cross: &[i64],
        gap: i64,
        pad: i64,
        min_width: &[i64],
    ) -> Option<Vec<(i64, i64)>> {
        let original = position.to_vec();
        let mut extent = self.extents(position, cross, pad, min_width);
        let mut shifts = 0;
        for _ in 0..MAX_PASSES {
            let mut moved = false;
            for layer in layers {
                for pair in layer.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let (ca, cb) = (
                        self.tree.chain_of(self.of[a]),
                        self.tree.chain_of(self.of[b]),
                    );
                    let depth = ca.iter().zip(cb).take_while(|(x, y)| x == y).count();
                    let (right, framed_a) = match ca.get(depth) {
                        Some(&k) => (extent[k].1, true),
                        None => (position[a] + cross[a] - 1, false),
                    };
                    let (left, kb) = match cb.get(depth) {
                        Some(&k) => (extent[k].0, Some(k)),
                        None => (position[b], None),
                    };
                    let space = if framed_a || kb.is_some() {
                        PAD_OUT
                    } else {
                        gap
                    };
                    let delta = right + 1 + space - left;
                    if delta <= 0 {
                        continue;
                    }
                    match kb {
                        Some(k) => {
                            for &v in &self.inside[k] {
                                position[v] += delta;
                            }
                        }
                        None => position[b] += delta,
                    }
                    moved = true;
                    shifts += 1;
                    if shifts > MAX_SHIFTS {
                        position.copy_from_slice(&original);
                        return None;
                    }
                    extent = self.extents(position, cross, pad, min_width);
                }
            }
            if !moved {
                return Some(extent);
            }
        }
        position.copy_from_slice(&original);
        None
    }
}
