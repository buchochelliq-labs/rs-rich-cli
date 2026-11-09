//! Source interpretation only; rectangular allocation uses the shared layout solver.
use super::*;
use crate::engine::Rect;
use crate::model::source::{RunLocation, RunPiece, SourcePosition, SourceRelations};
use rich_intuituive::Size;
use rich_intuituive::layout::{StackLayout, Track, adaptive_columns, split};
use rich_intuituive::node::Axis;
use rich_intuituive::screen::Rect as Cells;
use std::collections::HashSet;

pub(super) struct TextGroup {
    pub runs: Vec<Run>,
    pub owner: Option<usize>,
    pub rect: Option<Rect>,
}

fn block_of(location: &RunLocation) -> usize {
    match *location {
        RunLocation::Block { block, .. }
        | RunLocation::Cell { block, .. }
        | RunLocation::Caption { block } => block,
    }
}

// Borrowed, disposable indexes for one projection. The retained source graph and
// Page stay immutable; no text, pixels or replacement ownership graph is copied.
pub(super) struct Context<'a> {
    page: &'a Page,
    pub(super) relations: &'a SourceRelations,
    owners: Vec<Option<usize>>,
    pieces: Vec<Vec<&'a RunPiece>>,
    boundaries: Vec<Option<usize>>,
    node_by_id: HashMap<NodeId, usize>,
    alias_for: Vec<Option<usize>>,
    aliases: Vec<Option<Alias>>,
    owned_kinds: Vec<u8>,
}
struct Alias {
    shown: usize,
    rect: Rect,
    layers: HashSet<Option<usize>>,
}
impl<'a> Context<'a> {
    pub(super) fn new(source: &'a SourcePage, adapt: Adapt) -> Self {
        let rel = &source.relations;
        let page = source.page().as_ref();
        let mut owners = vec![None; page.blocks.len()];
        let mut pieces = vec![Vec::new(); page.blocks.len()];
        let mut owned_kinds = vec![0u8; rel.nodes.len()];
        for piece in &rel.blocks {
            owners[piece.block] = Some(piece.node);
            owned_kinds[piece.node] |= match page.blocks[piece.block].kind {
                BlockKind::Media(m) if page.media[m].kind == MediaKind::Video => 1,
                BlockKind::Log(_) => 2,
                _ => 0,
            };
        }
        for piece in &rel.runs {
            pieces[block_of(&piece.location)].push(piece);
        }
        let boundary_roots: HashSet<_> = rel
            .grids
            .iter()
            .filter(|_| adapt.on(Rule::SnapGrids))
            .flat_map(|g| g.items.iter().copied())
            .chain(
                rel.figures
                    .iter()
                    .filter(|_| adapt.on(Rule::FigureFlow))
                    .map(|f| f.node),
            )
            .collect();
        let mut aliases = Vec::new();
        let mut alias_roots = HashMap::new();
        if adapt.on(Rule::ScreenReaderTwins) {
            for &(spoken, shown) in &rel.twins {
                alias_roots.insert(spoken, aliases.len());
                aliases.push(rel.nodes[shown].rect.map(|rect| {
                    Alias {
                        shown,
                        rect,
                        // Known counterpart pieces must survive in this exact layer.
                        layers: rel
                            .runs
                            .iter()
                            .filter(|p| rel.contains(shown, p.node))
                            .map(|p| &page.blocks[block_of(&p.location)])
                            .filter(|b| !b.stack.covered)
                            .map(|b| b.stack.layer)
                            .collect(),
                    }
                }));
            }
        }
        let mut boundaries = vec![None; rel.nodes.len()];
        let mut alias_for = vec![None; rel.nodes.len()];
        for (i, node) in rel.nodes.iter().enumerate() {
            boundaries[i] = if boundary_roots.contains(&i) {
                Some(i)
            } else {
                node.parent.and_then(|p| boundaries[p])
            };
            alias_for[i] = node
                .parent
                .and_then(|p| alias_for[p])
                .or_else(|| alias_roots.get(&i).copied());
        }
        for (i, node) in rel.nodes.iter().enumerate().rev() {
            if let Some(parent) = node.parent {
                owned_kinds[parent] |= owned_kinds[i];
            }
        }
        Self {
            page,
            relations: rel,
            owners,
            pieces,
            boundaries,
            node_by_id: rel
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.node, i))
                .collect(),
            alias_for,
            aliases,
            owned_kinds,
        }
    }
    fn page(&self) -> &Page {
        self.page
    }
    pub(super) fn owner(&self, block: usize) -> Option<usize> {
        self.owners[block]
    }
    fn node_rect(&self, node: NodeId) -> Option<Rect> {
        self.relations.nodes[*self.node_by_id.get(&node)?].rect
    }
}
fn alias(source: &Context<'_>, node: usize, block: usize) -> Option<(usize, Rect)> {
    let alias = source.aliases[source.alias_for[node]?].as_ref()?;
    alias
        .layers
        .contains(&source.page.blocks[block].stack.layer)
        .then_some((alias.shown, alias.rect))
}
pub(super) fn has_alias(source: &Context<'_>, block: usize, marks: &Marks) -> bool {
    marks.adapt.on(Rule::ScreenReaderTwins)
        && source.pieces[block].iter().any(|p| {
            matches!(p.location, RunLocation::Block { .. })
                && alias(source, p.node, block).is_some()
        })
}

pub(super) fn text_groups(source: &Context<'_>, block: usize, marks: &Marks) -> Vec<TextGroup> {
    groups_at(
        source,
        block,
        &source.page().blocks[block].runs,
        marks,
        |run| RunLocation::Block { block, run },
    )
}

pub(super) fn cell_groups(
    source: &Context<'_>,
    block: usize,
    row: usize,
    cell: usize,
    runs: &[Run],
    marks: &Marks,
) -> Vec<TextGroup> {
    groups_at(source, block, runs, marks, |run| RunLocation::Cell {
        block,
        row,
        cell,
        run,
    })
}

fn groups_at(
    source: &Context<'_>,
    block: usize,
    runs: &[Run],
    marks: &Marks,
    location: impl Fn(usize) -> RunLocation,
) -> Vec<TextGroup> {
    let rel = &source.relations;
    let owner = source.owner(block);
    let first = location(0);
    let pieces: Vec<_> = source.pieces[block]
        .iter()
        .copied()
        .filter(|p| match (&p.location, &first) {
            (RunLocation::Block { .. }, RunLocation::Block { .. }) => true,
            (
                RunLocation::Cell {
                    row: a, cell: b, ..
                },
                RunLocation::Cell {
                    row: c, cell: d, ..
                },
            ) => a == c && b == d,
            (RunLocation::Caption { .. }, RunLocation::Caption { .. }) => true,
            _ => false,
        })
        .collect();
    let mixed_alias = marks.adapt.on(Rule::ScreenReaderTwins)
        && pieces
            .iter()
            .any(|p| alias(source, p.node, block).is_some());
    let own_boundary = owner.and_then(|n| source.boundaries[n]);
    let mixed_owner = pieces
        .iter()
        .any(|p| source.boundaries[p.node] != own_boundary);
    if !mixed_alias && !mixed_owner {
        return vec![TextGroup {
            runs: runs.to_vec(),
            owner,
            rect: None,
        }];
    }
    let mut groups: Vec<TextGroup> = Vec::new();
    for (run_index, run) in runs.iter().enumerate() {
        let mut ranges: Vec<_> = pieces
            .iter()
            .copied()
            .filter(|p| p.location == location(run_index))
            .collect();
        ranges.sort_by_key(|p| p.bytes.start);
        let mut end = 0;
        for piece in ranges {
            if piece.bytes.start > end {
                let mut gap = run.clone();
                gap.text = run.text[end..piece.bytes.start].to_string();
                if let Some(last) = groups.last_mut() {
                    last.runs.push(gap);
                } else {
                    groups.push(TextGroup {
                        runs: vec![gap],
                        owner,
                        rect: None,
                    });
                }
            }
            let target = marks
                .adapt
                .on(Rule::ScreenReaderTwins)
                .then(|| alias(source, piece.node, block))
                .flatten();
            let (at, rect) = target.map_or(
                (
                    piece.node,
                    mixed_alias.then_some(rel.nodes[piece.node].rect).flatten(),
                ),
                |(n, r)| (n, Some(r)),
            );
            let mut fragment = run.clone();
            fragment.text = run.text[piece.bytes.clone()].to_string();
            let key = source.boundaries[at].unwrap_or(at);
            if let Some(last) = groups.last_mut().filter(|g| {
                g.rect == rect && g.owner.map(|n| source.boundaries[n].unwrap_or(n)) == Some(key)
            }) {
                last.runs.push(fragment);
            } else {
                groups.push(TextGroup {
                    runs: vec![fragment],
                    owner: Some(at),
                    rect,
                });
            }
            end = piece.bytes.end;
        }
        if end < run.text.len() {
            let mut tail = run.clone();
            tail.text = run.text[end..].to_string();
            if let Some(last) = groups.last_mut() {
                last.runs.push(tail);
            } else {
                groups.push(TextGroup {
                    runs: vec![tail],
                    owner,
                    rect: None,
                });
            }
        }
    }
    groups
}

#[derive(Clone, Copy)]
struct Allocation {
    root: usize,
    original: Rect,
    target: Rect,
}
fn mapped(rect: Rect, a: Allocation) -> Rect {
    let scale = (a.target.width / a.original.width.max(1.0)).min(1.0);
    Rect::new(
        a.target.x + (rect.x - a.original.x).max(0.0) * scale,
        a.target.y + rect.y - a.original.y,
        rect.width * scale,
        rect.height,
    )
}
fn inherited(rel: &SourceRelations, allocations: &[Allocation], node: usize, rect: Rect) -> Rect {
    allocations
        .iter()
        .rev()
        .find(|a| rel.contains(a.root, node))
        .map_or(rect, |&a| mapped(rect, a))
}
fn ordinary_path(rel: &SourceRelations, root: usize, mut node: usize) -> bool {
    loop {
        let n = &rel.nodes[node];
        if n.position != SourcePosition::Flow && n.rect.is_some() {
            return false;
        }
        if node == root {
            return true;
        }
        let Some(parent) = n.parent else {
            return false;
        };
        node = parent;
    }
}
fn flow_context(rel: &SourceRelations, mut node: usize) -> bool {
    loop {
        let n = &rel.nodes[node];
        // The document layout object has bounds but no element style facts.
        let Some(parent) = n.parent else { return true };
        if n.rect.is_some() && n.position != SourcePosition::Flow {
            return false;
        }
        node = parent;
    }
}
fn containers(rel: &SourceRelations) -> Vec<usize> {
    let mut roots = Vec::new();
    for figure in &rel.figures {
        let mut parent = rel.nodes[figure.node].parent;
        while let Some(node) = parent {
            let n = &rel.nodes[node];
            if n.ordinary {
                if ordinary_path(rel, node, figure.node) && flow_context(rel, node) {
                    roots.push(node);
                }
                break;
            }
            // Only a boxless wrapper is transparent to ordinary flow.
            if n.rect.is_some() {
                break;
            }
            parent = n.parent;
        }
    }
    roots.sort_unstable();
    roots.dedup();
    let all = roots.clone();
    roots.retain(|&r| {
        !all.iter().any(|&other| {
            other != r && rel.contains(other, r) && grid_unit(rel, other, r).is_none()
        })
    });
    roots
}
// Outermost grid below this ordinary container: one independently allocated unit.
fn grid_unit(rel: &SourceRelations, root: usize, node: usize) -> Option<usize> {
    rel.grids
        .iter()
        .find(|g| g.node != root && rel.contains(root, g.node) && rel.contains(g.node, node))
        .map(|g| g.node)
}

fn members(rel: &SourceRelations, root: usize, items: &[Item]) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item.source
                .is_some_and(|node| rel.contains(root, node) && ordinary_path(rel, root, node))
        })
        .map(|(i, _)| i)
        .collect()
}

pub(super) fn allocate(
    source: &Context<'_>,
    items: &mut [Item],
    columns: usize,
    cell: (f64, f64),
    marks: &Marks,
) -> Vec<(usize, f64)> {
    let rel = &source.relations;
    let (cw, ch) = cell;
    let mut stacked = Vec::new();
    let mut allocations = Vec::<Allocation>::new();
    let flow_roots = if marks.adapt.on(Rule::FigureFlow) {
        containers(rel)
    } else {
        Vec::new()
    };
    if marks.adapt.on(Rule::SnapGrids) {
        for grid in &rel.grids {
            let Some(rect) = rel.nodes[grid.node].rect else {
                continue;
            };
            if rel.nodes[grid.node].position != SourcePosition::Flow
                || !flow_context(rel, grid.node)
            {
                continue;
            }
            let Some(boxes) = grid
                .items
                .iter()
                .map(|&i| rel.nodes[i].rect)
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let Some(first) = boxes.first() else { continue };
            // The established source-declared video/log pair keeps its wider minimum.
            // Membership is actual ancestry, including a log's atomic retained slot.
            let owns =
                |root: usize, video| source.owned_kinds[root] & if video { 1 } else { 2 } != 0;
            let paired = grid.items.len() == 2
                && ((owns(grid.items[0], true) && owns(grid.items[1], false))
                    || (owns(grid.items[1], true) && owns(grid.items[0], false)));
            // Unequal spans, overlap and unknown/positioned items are not equal cards.
            if boxes.len() < 2
                || first.width <= 0.0
                || first.height <= 0.0
                || grid
                    .items
                    .iter()
                    .any(|&n| !ordinary_path(rel, grid.node, n))
                || boxes.iter().enumerate().any(|(i, a)| {
                    boxes[..i].iter().any(|b| {
                        a.x < b.right() - 0.5
                            && b.x < a.right() - 0.5
                            && a.y < b.bottom() - 0.5
                            && b.y < a.bottom() - 0.5
                    })
                })
            {
                continue;
            }
            let count = boxes
                .iter()
                .map(|r| {
                    boxes
                        .iter()
                        .filter(|b| (b.y - r.y).abs() < ch / 2.0)
                        .count()
                })
                .max()
                .unwrap_or(1);
            if count < 2 {
                continue;
            }
            let mut target = inherited(rel, &allocations, grid.node, rect);
            if let Some(&root) = flow_roots
                .iter()
                .rev()
                .find(|&&root| grid_unit(rel, root, grid.node) == Some(grid.node))
                && let Some(parent) = rel.nodes[root].rect
            {
                let parent = inherited(rel, &allocations, root, parent);
                target.x = parent.x;
                target.width = parent.width;
            }
            let x0 = ((target.x / cw).round().max(0.0) as usize).min(columns - 1);
            let x1 = ((target.right() / cw).round().max(0.0) as usize).clamp(x0 + 1, columns);
            let unequal = !paired
                && boxes.iter().any(|r| {
                    (r.width - first.width).abs() > 1.0 || (r.height - first.height).abs() > 1.0
                });
            // Spanning/unequal groups retain source columns when they fit. At a
            // narrower allocation each actual item is one vertical unit.
            if unequal && target.width <= (x1 - x0) as f64 * cw + 1.0 {
                continue;
            }
            let fit = if unequal {
                1
            } else {
                adaptive_columns(
                    (x1 - x0) as u16,
                    if paired {
                        32
                    } else {
                        crate::adapt::MIN_GRID_CELLS as u16
                    },
                    1,
                    count,
                )
            };
            if fit == 1 {
                stacked.push((grid.node, target.bottom()));
            }
            let tracks = vec![Track::new(Size::Flex(1)); fit];
            let cells = split(
                Cells::new(x0 as u16, 0, (x1 - x0) as u16, 1),
                Axis::Horizontal,
                &tracks,
                StackLayout {
                    gap: 1,
                    ..Default::default()
                },
            );
            for (index, (&root, &original)) in grid.items.iter().zip(&boxes).enumerate() {
                let area = cells[index % fit];
                let target = Rect::new(
                    area.x as f64 * cw,
                    target.y + (index / fit) as f64 * first.height,
                    area.width as f64 * cw,
                    original.height,
                );
                allocations.push(Allocation {
                    root,
                    original,
                    target,
                });
            }
        }
        for item in items.iter_mut() {
            let Some(node) = item.source else { continue };
            let Some(&a) = allocations
                .iter()
                .rev()
                .find(|a| rel.contains(a.root, node) && ordinary_path(rel, a.root, node))
            else {
                continue;
            };
            let rect = Rect::new(
                item.source_x.0 as f64 * cw,
                item.top,
                (item.source_x.1 - item.source_x.0) as f64 * cw,
                item.bottom - item.top,
            );
            let target = mapped(rect, a);
            let start = ((a.target.x / cw).round() as usize).min(columns - 1);
            let end = ((a.target.right() / cw).round() as usize).clamp(start + 1, columns);
            item.top = target.y;
            item.bottom = target.bottom();
            item.x0 = ((target.x / cw).round() as usize).clamp(start, end - 1);
            item.x1 = ((target.right() / cw).round() as usize).clamp(item.x0 + 1, end);
            item.allocation = Some((start, end));
        }
    }
    if marks.adapt.on(Rule::FigureFlow) {
        for root in flow_roots {
            let Some(rect) = rel.nodes[root].rect else {
                continue;
            };
            let rect = inherited(rel, &allocations, root, rect);
            let x0 = ((rect.x / cw).round().max(0.0) as usize).min(columns - 1);
            let x1 = ((rect.right() / cw).round().max(0.0) as usize).clamp(x0 + 1, columns);
            for i in members(rel, root, items) {
                let item = &mut items[i];
                if item
                    .source
                    .is_some_and(|node| grid_unit(rel, root, node).is_some())
                {
                    continue;
                }
                if let Some((table, _)) = item.row {
                    // A cell fragment may alias a different run root; its column
                    // allocation still belongs to the exact source table row.
                    let original = source.node_rect(table).unwrap_or(rect);
                    let scale = |x: usize| {
                        x0 + (((x as f64 * cw - original.x).max(0.0) / original.width.max(1.0))
                            * (x1 - x0) as f64)
                            .round() as usize
                    };
                    item.x0 = scale(item.source_x.0).clamp(x0, x1 - 1);
                    item.x1 = scale(item.source_x.1).clamp(item.x0 + 1, x1);
                    item.allocation = Some((item.x0, item.x1));
                } else {
                    item.x0 = x0;
                    item.x1 = x1;
                    item.allocation = Some((x0, x1));
                }
            }
        }
    }
    stacked
}

// After wrapping, measure each actual item subtree as one atomic unit. Nested
// grids have already been measured. Source anchors and actions never move.
pub(super) fn stack_grids(
    source: &Context<'_>,
    items: &mut [Item],
    roots: &[(usize, f64)],
    columns: usize,
    cell: (f64, f64),
) {
    let rel = source.relations;
    for &(root, old_bottom) in roots.iter().rev() {
        let grid = rel
            .grids
            .iter()
            .find(|g| g.node == root)
            .expect("allocated grid");
        let group = members(rel, root, items);
        if group.is_empty() {
            continue;
        }
        let top = group
            .iter()
            .map(|&i| items[i].top)
            .fold(f64::INFINITY, f64::min);
        let measured: Vec<_> = grid
            .items
            .iter()
            .map(|&unit| {
                let unit: Vec<_> = group
                    .iter()
                    .copied()
                    .filter(|&i| items[i].source.is_some_and(|n| rel.contains(unit, n)))
                    .collect();
                measure_grid(items, &unit, columns, cell.1)
            })
            .collect();
        let total: usize = measured.iter().map(|(_, h)| *h).sum();
        if total > u16::MAX as usize {
            continue;
        }
        let tracks: Vec<_> = measured
            .iter()
            .map(|(_, h)| Track::new(Size::Fixed(*h as u16)))
            .collect();
        let slots = split(
            Cells::new(0, 0, 1, total as u16),
            Axis::Vertical,
            &tracks,
            StackLayout::default(),
        );
        for ((unit, _), slot) in measured.iter().zip(slots) {
            for &(i, row) in unit {
                items[i].top = top + (slot.y as usize + row) as f64 * cell.1;
                items[i].bottom = items[i].top + items[i].lines.len() as f64 * cell.1;
            }
        }
        let extra = (top + total as f64 * cell.1 - old_bottom).max(0.0);
        // Flow siblings after the expanded subtree must follow its full occupied
        // height. Ancestry/order identify them, not their source rectangles.
        let parent = roots
            .iter()
            .rev()
            .find(|(other, _)| *other != root && rel.contains(*other, root))
            .map(|(other, _)| *other);
        for item in items.iter_mut() {
            if item.source.is_some_and(|n| {
                n > root
                    && !rel.contains(root, n)
                    && flow_context(rel, n)
                    && parent.is_none_or(|parent| rel.contains(parent, n))
            }) {
                item.top += extra;
                item.bottom += extra;
            }
        }
    }
}

pub(super) fn stack_figures(
    source: &Context<'_>,
    items: &mut [Item],
    columns: usize,
    cell: (f64, f64),
    marks: &Marks,
) {
    if !marks.adapt.on(Rule::FigureFlow) {
        return;
    }
    let rel = &source.relations;
    for root in containers(rel).into_iter().rev() {
        let mut group = members(rel, root, items);
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|&i| items[i].source);
        let top = group
            .iter()
            .map(|&i| items[i].top)
            .fold(f64::INFINITY, f64::min);
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Unit {
            Grid(usize),
            Row(NodeId, usize),
            Single(usize),
        }
        let mut units: Vec<(Unit, Vec<usize>)> = Vec::new();
        for i in group {
            let key =
                if let Some(grid) = items[i].source.and_then(|node| grid_unit(rel, root, node)) {
                    Unit::Grid(grid)
                } else if let Some((node, row)) = items[i].row {
                    Unit::Row(node, row)
                } else {
                    Unit::Single(i)
                };
            if let Some((_, unit)) = units.iter_mut().find(|(k, _)| *k == key) {
                unit.push(i);
            } else {
                units.push((key, vec![i]));
            }
        }
        let measured: Vec<_> = units
            .iter()
            .map(|(key, unit)| {
                if matches!(key, Unit::Grid(_)) {
                    measure_grid(items, unit, columns, cell.1)
                } else {
                    (
                        unit.iter().map(|&i| (i, 0)).collect(),
                        unit.iter()
                            .map(|&i| items[i].lines.len())
                            .max()
                            .unwrap_or(0),
                    )
                }
            })
            .collect();
        let heights: Vec<_> = measured.iter().map(|(_, h)| *h).collect();
        let total: usize = heights.iter().sum();
        // Unrepresentably large groups keep their source placement.
        if total > u16::MAX as usize {
            continue;
        }
        let tracks: Vec<_> = heights
            .iter()
            .map(|&h| Track::new(Size::Fixed(h as u16)))
            .collect();
        let slots = split(
            Cells::new(0, 0, 1, total as u16),
            Axis::Vertical,
            &tracks,
            StackLayout::default(),
        );
        for ((unit, _), slot) in measured.iter().zip(slots) {
            for &(i, row) in unit {
                items[i].top = top + (slot.y as usize + row) as f64 * cell.1;
                items[i].bottom = items[i].top + items[i].lines.len() as f64 * cell.1;
            }
        }
    }
}

// Measure the existing grid's local rectangular placement after wrapping, without
// painting or changing its source membership, columns or internal offsets.
fn measure_grid(
    items: &[Item],
    unit: &[usize],
    columns: usize,
    ch: f64,
) -> (Vec<(usize, usize)>, usize) {
    let top = unit
        .iter()
        .map(|&i| items[i].top)
        .fold(f64::INFINITY, f64::min);
    let mut order = unit.to_vec();
    order.sort_by(|&a, &b| {
        items[a]
            .top
            .total_cmp(&items[b].top)
            .then(items[a].x0.cmp(&items[b].x0))
    });
    let mut skyline = vec![0usize; columns];
    let mut rows = HashMap::new();
    let mut placed = Vec::new();
    for i in order {
        let item = &items[i];
        let desired = ((item.top - top) / ch).round().max(0.0) as usize;
        let y = match item.row {
            Some(key) => *rows.entry(key).or_insert_with(|| {
                unit.iter()
                    .filter(|&&j| items[j].row == Some(key))
                    .flat_map(|&j| skyline[items[j].x0..items[j].x1].iter().copied())
                    .max()
                    .unwrap_or(0)
                    .max(desired)
            }),
            None => skyline[item.x0..item.x1]
                .iter()
                .copied()
                .max()
                .unwrap_or(0)
                .max(desired),
        };
        for slot in &mut skyline[item.x0..item.x1] {
            *slot = y + item.lines.len();
        }
        placed.push((i, y));
    }
    (placed, skyline.into_iter().max().unwrap_or(0))
}
