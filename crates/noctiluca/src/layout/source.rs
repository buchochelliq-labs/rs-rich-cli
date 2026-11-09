//! Source interpretation only; rectangular allocation uses the shared layout solver.
use super::*;
use crate::engine::Rect;
use crate::model::source::{RunLocation, SourcePosition, SourceRelations};
use rich_intuituive::Size;
use rich_intuituive::layout::{StackLayout, Track, adaptive_columns, split};
use rich_intuituive::node::Axis;
use rich_intuituive::screen::Rect as Cells;

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

fn alias(source: &SourcePage, node: usize, block: usize) -> Option<(usize, Rect)> {
    let rel = &source.relations;
    let &(_, shown) = rel
        .twins
        .iter()
        .find(|(spoken, _)| rel.contains(*spoken, node))?;
    // The known counterpart must still have a retained visible piece in the same layer.
    let layer = source.page().blocks[block].stack.layer;
    rel.runs.iter().find(|piece| {
        let b = &source.page().blocks[block_of(&piece.location)];
        rel.contains(shown, piece.node) && !b.stack.covered && b.stack.layer == layer
    })?;
    Some((shown, rel.nodes[shown].rect?))
}

pub(super) fn has_alias(source: &SourcePage, block: usize, marks: &Marks) -> bool {
    marks.adapt.on(Rule::ScreenReaderTwins)
        && source.relations.runs.iter().any(|piece| {
            matches!(piece.location, RunLocation::Block { block: b, .. } if b == block)
                && alias(source, piece.node, block).is_some()
        })
}

fn boundary(rel: &SourceRelations, node: usize) -> Option<usize> {
    rel.grids
        .iter()
        .flat_map(|g| &g.items)
        .copied()
        .chain(rel.figures.iter().map(|f| f.node))
        .filter(|&root| rel.contains(root, node))
        .max()
}

pub(super) fn text_groups(source: &SourcePage, block: usize, marks: &Marks) -> Vec<TextGroup> {
    groups_at(
        source,
        block,
        &source.page().blocks[block].runs,
        marks,
        |run| RunLocation::Block { block, run },
    )
}

pub(super) fn cell_groups(
    source: &SourcePage,
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
    source: &SourcePage,
    block: usize,
    runs: &[Run],
    marks: &Marks,
    location: impl Fn(usize) -> RunLocation,
) -> Vec<TextGroup> {
    let rel = &source.relations;
    let owner = rel.blocks.iter().find(|p| p.block == block).map(|p| p.node);
    let pieces: Vec<_> = rel
        .runs
        .iter()
        .filter(|p| (0..runs.len()).any(|run| p.location == location(run)))
        .collect();
    let mixed_alias = marks.adapt.on(Rule::ScreenReaderTwins)
        && pieces
            .iter()
            .any(|p| alias(source, p.node, block).is_some());
    let own_boundary = owner.and_then(|n| boundary(rel, n));
    let mixed_owner = pieces.iter().any(|p| boundary(rel, p.node) != own_boundary);
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
            let key = boundary(rel, at).unwrap_or(at);
            if let Some(last) = groups.last_mut().filter(|g| {
                g.rect == rect && g.owner.map(|n| boundary(rel, n).unwrap_or(n)) == Some(key)
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
        !all.iter()
            .any(|&other| other != r && rel.contains(other, r))
    });
    roots
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
    source: &SourcePage,
    items: &mut [Item],
    columns: usize,
    cell: (f64, f64),
    marks: &Marks,
) {
    let rel = &source.relations;
    let (cw, ch) = cell;
    let mut allocations = Vec::<Allocation>::new();
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
            let owns = |root, video| {
                rel.blocks.iter().any(|piece| {
                    rel.contains(root, piece.node)
                        && match source.page().blocks[piece.block].kind {
                            BlockKind::Media(m) => {
                                video && source.page().media[m].kind == MediaKind::Video
                            }
                            BlockKind::Log(_) => !video,
                            _ => false,
                        }
                })
            };
            let paired = grid.items.len() == 2
                && ((owns(grid.items[0], true) && owns(grid.items[1], false))
                    || (owns(grid.items[1], true) && owns(grid.items[0], false)));
            // Unequal spans, overlap and unknown/positioned items are not equal cards.
            if boxes.len() < 2
                || first.width <= 0.0
                || first.height <= 0.0
                || (!paired
                    && boxes.iter().any(|r| {
                        (r.width - first.width).abs() > 1.0 || (r.height - first.height).abs() > 1.0
                    }))
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
            let target = inherited(rel, &allocations, grid.node, rect);
            let x0 = ((target.x / cw).round().max(0.0) as usize).min(columns - 1);
            let x1 = ((target.right() / cw).round().max(0.0) as usize).clamp(x0 + 1, columns);
            let fit = adaptive_columns(
                (x1 - x0) as u16,
                if paired {
                    32
                } else {
                    crate::adapt::MIN_GRID_CELLS as u16
                },
                1,
                count,
            );
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
        for root in containers(rel) {
            let Some(rect) = rel.nodes[root].rect else {
                continue;
            };
            let rect = inherited(rel, &allocations, root, rect);
            let x0 = ((rect.x / cw).round().max(0.0) as usize).min(columns - 1);
            let x1 = ((rect.right() / cw).round().max(0.0) as usize).clamp(x0 + 1, columns);
            for i in members(rel, root, items) {
                let item = &mut items[i];
                if item.row.is_some() {
                    let original = item.source.and_then(|n| rel.nodes[n].rect).unwrap_or(rect);
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
}

pub(super) fn stack_figures(
    source: &SourcePage,
    items: &mut [Item],
    _columns: usize,
    cell: (f64, f64),
    marks: &Marks,
) {
    if !marks.adapt.on(Rule::FigureFlow) {
        return;
    }
    let rel = &source.relations;
    for root in containers(rel) {
        let mut group = members(rel, root, items);
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|&i| items[i].source);
        let top = group
            .iter()
            .map(|&i| items[i].top)
            .fold(f64::INFINITY, f64::min);
        // Cells of one source table row occupy one measured vertical track.
        let mut units: Vec<Vec<usize>> = Vec::new();
        for i in group {
            if let Some(row) = items[i].row {
                if let Some(unit) = units
                    .iter_mut()
                    .find(|unit| items[unit[0]].row == Some(row))
                {
                    unit.push(i);
                    continue;
                }
            }
            units.push(vec![i]);
        }
        let heights: Vec<_> = units
            .iter()
            .map(|unit| {
                unit.iter()
                    .map(|&i| items[i].lines.len())
                    .max()
                    .unwrap_or(0)
                    .min(u16::MAX as usize) as u16
            })
            .collect();
        let total: usize = heights.iter().map(|&h| h as usize).sum();
        // Unrepresentably large groups keep their source placement.
        if total > u16::MAX as usize {
            continue;
        }
        let tracks: Vec<_> = heights
            .iter()
            .map(|&h| Track::new(Size::Fixed(h)))
            .collect();
        let slots = split(
            Cells::new(0, 0, 1, total as u16),
            Axis::Vertical,
            &tracks,
            StackLayout::default(),
        );
        for (unit, slot) in units.iter().zip(slots) {
            for &i in unit {
                items[i].top = top + slot.y as f64 * cell.1;
                items[i].bottom = items[i].top + slot.height as f64 * cell.1;
            }
        }
    }
}
