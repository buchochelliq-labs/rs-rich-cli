//! Snapshot relationships. Addresses refer to the final Page, never copied text or pixels.
use super::*;
use std::ops::Range;

/// A page and its immutable source relationships, captured together.
/// Cloning preserves the exact page allocation. Only model extraction and the
/// tab's guarded publication path can create or replace this pair.
#[derive(Clone, Debug)]
pub struct SourcePage {
    pub(super) page: Arc<Page>,
    pub(crate) relations: Arc<SourceRelations>,
}
impl SourcePage {
    pub fn page(&self) -> &Arc<Page> {
        &self.page
    }

    pub(crate) fn same_relations(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.relations, &other.relations) || self.relations == other.relations
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceNode {
    pub node: NodeId,
    pub parent: Option<usize>,
    pub rect: Option<Rect>,
    /// Position in the captured DOM's source order is this record's index.
    pub ordinary: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BlockPiece {
    pub node: usize,
    pub block: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RunLocation {
    Block { block: usize, run: usize },
    Cell { block: usize, row: usize, cell: usize, run: usize },
    Caption { block: usize },
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RunPiece {
    pub node: usize,
    pub location: RunLocation,
    /// UTF-8 byte range in the final cleaned run (or table caption).
    pub bytes: Range<usize>,
}
impl RunPiece {
    pub(crate) fn text<'a>(&self, page: &'a Page) -> &'a str {
        let text = match self.location {
            RunLocation::Block { block, run } => &page.blocks[block].runs[run].text,
            RunLocation::Cell { block, row, cell, run } => {
                let BlockKind::Table(table) = &page.blocks[block].kind else { unreachable!() };
                &table.rows[row][cell].runs[run].text
            }
            RunLocation::Caption { block } => {
                let BlockKind::Table(table) = &page.blocks[block].kind else { unreachable!() };
                &table.caption
            }
        };
        &text[self.bytes.clone()]
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceGrid {
    pub node: usize,
    /// Source roots, including boxless roots. Descendants retain their own boxes.
    pub items: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceFigure {
    pub node: usize,
    pub parent: Option<usize>,
    pub captions: Vec<usize>,
    /// All descendant pieces; nested figures remain identifiable by parent.
    pub blocks: Vec<usize>,
    pub runs: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceRelations {
    pub nodes: Vec<SourceNode>,
    pub blocks: Vec<BlockPiece>,
    pub runs: Vec<RunPiece>,
    pub grids: Vec<SourceGrid>,
    pub figures: Vec<SourceFigure>,
    /// Index into Page.controls, proven by DOM ancestry, for each Page.media.
    pub media_controls: Vec<Option<usize>>,
    /// The existing semantic matcher selected these exact (Spoken, Shown) roots.
    pub twins: Vec<(usize, usize)>,
}
impl SourceRelations {
    pub(crate) fn contains(&self, root: usize, mut node: usize) -> bool {
        loop {
            if node == root { return true; }
            let Some(parent) = self.nodes.get(node).and_then(|n| n.parent) else { return false; };
            node = parent;
        }
    }
}

// Transient character origins follow the Walker's actual merges and whitespace
// edits. They are compressed to byte ranges only after final stack assignment.
pub(super) type Origins = Vec<Vec<Option<usize>>>;
#[derive(Default)]
pub(super) struct Extraction {
    pub runs: Origins,
    pub blocks: HashMap<usize, Origins>,
    pub cells: Vec<(RunLocation, Vec<Option<usize>>)>,
}
impl Extraction {
    pub fn finish(self, dom: &Dom, page: &Page) -> SourceRelations {
        let by_node: HashMap<_, _> = dom.backend.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        let nodes = dom.backend.iter().enumerate().map(|(i, &node)| SourceNode {
            node,
            parent: usize::try_from(dom.parent[i]).ok().filter(|&p| p < i),
            rect: dom.rect(i),
            ordinary: !matches!(dom.style(i, 9), "absolute" | "fixed" | "sticky") && !dom.concealed[i],
        }).collect();
        let blocks = page.blocks.iter().enumerate().filter_map(|(block, b)| {
            Some(BlockPiece { node: *by_node.get(&b.node)?, block })
        }).collect();
        let mut runs = Vec::new();
        let add = |out: &mut Vec<RunPiece>, location: RunLocation, text: &str, origins: &[Option<usize>]| {
            for ((start, c), &node) in text.char_indices().zip(origins) {
                let Some(node) = node else { continue; };
                let end = start + c.len_utf8();
                if let Some(last) = out.last_mut().filter(|p| p.node == node && p.location == location && p.bytes.end == start) {
                    last.bytes.end = end;
                } else {
                    out.push(RunPiece { node, location: location.clone(), bytes: start..end });
                }
            }
        };
        // HashMap iteration must never turn equal snapshots into unequal graphs.
        for (block, b) in page.blocks.iter().enumerate() {
            if let Some(origins) = self.blocks.get(&block) {
                for (run, (r, o)) in b.runs.iter().zip(origins).enumerate() {
                    add(&mut runs, RunLocation::Block { block, run }, &r.text, o);
                }
            }
        }
        for (location, origins) in self.cells {
            let text = match location {
                RunLocation::Cell { block, row, cell, run } => {
                    let BlockKind::Table(t) = &page.blocks[block].kind else { continue; };
                    &t.rows[row][cell].runs[run].text
                }
                RunLocation::Caption { block } => {
                    let BlockKind::Table(t) = &page.blocks[block].kind else { continue; };
                    &t.caption
                }
                _ => continue,
            };
            add(&mut runs, location, text, &origins);
        }
        let grids = (0..dom.name.len()).filter(|&i| {
            matches!(dom.style(i, 0), "grid" | "inline-grid") || page.grids.iter().any(|g| g.node == dom.backend[i])
        }).map(|node| SourceGrid { node, items: dom.children[node].iter().copied().filter(|&i| dom.is_element(i)).collect() }).collect();
        let mut relations = SourceRelations { nodes, blocks, runs, grids, figures: Vec::new(), media_controls: Vec::new(), twins: Vec::new() };
        for i in 0..dom.name.len() {
            if dom.name[i] != "FIGURE" { continue; }
            let mut parent = relations.nodes[i].parent;
            while parent.is_some_and(|p| dom.name[p] != "FIGURE") {
                parent = relations.nodes[parent.unwrap()].parent;
            }
            let captions = (0..dom.name.len()).filter(|&c| {
                if dom.name[c] != "FIGCAPTION" { return false; }
                let mut ancestor = relations.nodes[c].parent;
                while let Some(a) = ancestor {
                    if dom.name[a] == "FIGURE" { return a == i; }
                    ancestor = relations.nodes[a].parent;
                }
                false
            }).collect();
            relations.figures.push(SourceFigure {
                node: i, parent, captions,
                blocks: relations.blocks.iter().filter(|b| relations.contains(i, b.node)).map(|b| b.block).collect(),
                runs: relations.runs.iter().enumerate().filter(|(_, r)| relations.contains(i, r.node)).map(|(r, _)| r).collect(),
            });
        }
        relations.media_controls = page.media.iter().map(|m| {
            let mut node = *by_node.get(&m.node)?;
            loop {
                if let Some(control) = page.controls.iter().position(|c| c.node == dom.backend[node]) { return Some(control); }
                node = relations.nodes[node].parent?;
            }
        }).collect();
        relations.twins = dom.twins.iter().filter_map(|(&i, &(kind, other))| (kind == Twin::Spoken).then_some((i, other))).collect();
        relations.twins.sort_unstable();
        relations
    }
}
