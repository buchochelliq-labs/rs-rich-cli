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

    /// Tab-only derivation: passive updates preserve every source block;
    /// a synthetic error notice may prefix them. No source can be paired
    /// with another document or different source block contents.
    pub(crate) fn republish(mut self, page: Arc<Page>) -> Self {
        assert_eq!(self.page.document, page.document);
        let prefix = page
            .blocks
            .len()
            .checked_sub(self.page.blocks.len())
            .expect("source blocks removed");
        assert_eq!(self.page.blocks, page.blocks[prefix..]);
        if prefix != 0 {
            let relations = Arc::make_mut(&mut self.relations);
            for piece in &mut relations.blocks {
                piece.block += prefix;
            }
            for piece in &mut relations.runs {
                match &mut piece.location {
                    RunLocation::Block { block, .. }
                    | RunLocation::Cell { block, .. }
                    | RunLocation::Caption { block } => *block += prefix,
                }
            }
            for figure in &mut relations.figures {
                for block in &mut figure.blocks {
                    *block += prefix;
                }
            }
        }
        self.page = page;
        self
    }

    pub(crate) fn reuse_relations(&mut self, previous: &Self) {
        if self.same_relations(previous) {
            self.relations = previous.relations.clone();
        }
    }

    pub(crate) fn same_relations(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.relations, &other.relations) || self.relations == other.relations
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourcePosition {
    Flow,
    Positioned,
    Unknown,
}

/// Records preserve DOM source order; opaque live-log contents are omitted.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceNode {
    pub node: NodeId,
    pub parent: Option<usize>,
    pub rect: Option<Rect>,
    /// Static/relative, absolute/fixed/sticky, or absent/unsupported source facts.
    pub position: SourcePosition,
    /// Proven ordinary block flow; absent/unknown styles are ineligible.
    pub ordinary: bool,
    /// Explicit computed display: contents; absence of a box alone is not proof.
    pub contents: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BlockPiece {
    pub node: usize,
    pub block: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RunLocation {
    Block {
        block: usize,
        run: usize,
    },
    Cell {
        block: usize,
        row: usize,
        cell: usize,
        run: usize,
    },
    Caption {
        block: usize,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RunPiece {
    pub node: usize,
    pub location: RunLocation,
    /// UTF-8 byte range in the final cleaned run (or table caption).
    pub bytes: Range<usize>,
}
impl RunPiece {
    #[cfg(test)]
    pub(crate) fn text<'a>(&self, page: &'a Page) -> &'a str {
        let text = match self.location {
            RunLocation::Block { block, run } => &page.blocks[block].runs[run].text,
            RunLocation::Cell {
                block,
                row,
                cell,
                run,
            } => {
                let BlockKind::Table(table) = &page.blocks[block].kind else {
                    unreachable!()
                };
                &table.rows[row][cell].runs[run].text
            }
            RunLocation::Caption { block } => {
                let BlockKind::Table(table) = &page.blocks[block].kind else {
                    unreachable!()
                };
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
            if node == root {
                return true;
            }
            let Some(parent) = self.nodes.get(node).and_then(|n| n.parent) else {
                return false;
            };
            node = parent;
        }
    }
    /// A noninteractive live log is one fixed layout slot. Its entry tree is replaced by
    /// incremental live reads with no snapshot node provenance; retaining that
    /// stale tree would both misattribute text and invalidate unrelated layout.
    fn retain_log_slots(&mut self, page: &Page) {
        if page.logs.is_empty() {
            return;
        }
        let protected: HashSet<_> = page
            .controls
            .iter()
            .map(|c| c.node)
            .chain(page.layers.iter().map(|l| l.node))
            .collect();
        let mut protected_ancestors = HashSet::new();
        for (index, node) in self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| protected.contains(&n.node))
        {
            protected_ancestors.insert(node.node);
            let mut parent = self.nodes[index].parent;
            while let Some(p) = parent {
                protected_ancestors.insert(self.nodes[p].node);
                parent = self.nodes[p].parent;
            }
        }
        let roots: HashSet<_> = page
            .logs
            .iter()
            .map(|l| l.node)
            .filter(|n| !protected_ancestors.contains(n))
            .collect();
        let mut inside = vec![false; self.nodes.len()];
        let mut indices = vec![None; self.nodes.len()];
        let mut next = 0;
        for (i, node) in self.nodes.iter().enumerate() {
            inside[i] = node
                .parent
                .is_some_and(|p| inside[p] || roots.contains(&self.nodes[p].node));
            if !inside[i] {
                indices[i] = Some(next);
                next += 1;
            }
        }
        self.nodes = std::mem::take(&mut self.nodes)
            .into_iter()
            .enumerate()
            .filter_map(|(i, mut node)| {
                indices[i]?;
                node.parent = node.parent.and_then(|p| indices[p]);
                Some(node)
            })
            .collect();
        for piece in &mut self.blocks {
            piece.node = indices[piece.node].expect("log contents have no outer blocks");
        }
        for piece in &mut self.runs {
            piece.node = indices[piece.node].expect("log contents have no outer runs");
        }
        self.grids.retain_mut(|grid| {
            let Some(node) = indices[grid.node] else {
                return false;
            };
            grid.node = node;
            grid.items = grid.items.iter().filter_map(|&i| indices[i]).collect();
            true
        });
        self.figures.retain_mut(|figure| {
            let Some(node) = indices[figure.node] else {
                return false;
            };
            figure.node = node;
            figure.parent = figure.parent.and_then(|p| indices[p]);
            figure.captions = figure.captions.iter().filter_map(|&i| indices[i]).collect();
            true
        });
        self.twins = self
            .twins
            .iter()
            .filter_map(|&(a, b)| Some((indices[a]?, indices[b]?)))
            .collect();
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
        let by_node: HashMap<_, _> = dom
            .backend
            .iter()
            .enumerate()
            .map(|(i, &n)| (n, i))
            .collect();
        let nodes: Vec<SourceNode> = dom
            .backend
            .iter()
            .enumerate()
            .map(|(i, &node)| SourceNode {
                node,
                parent: usize::try_from(dom.parent[i]).ok().filter(|&p| p < i),
                rect: dom.rect(i),
                contents: dom
                    .source_formatting
                    .get(&node)
                    .is_some_and(|(display, position)| {
                        display == "contents" && matches!(position.as_str(), "static" | "relative")
                    }),
                position: match dom
                    .source_formatting
                    .get(&node)
                    .map(|(_, p)| p.as_str())
                    .unwrap_or_else(|| dom.style(i, 14))
                {
                    "static" | "relative" => SourcePosition::Flow,
                    "absolute" | "fixed" | "sticky" => SourcePosition::Positioned,
                    _ => SourcePosition::Unknown,
                },
                ordinary: matches!(dom.style(i, 0), "block" | "flow-root" | "list-item")
                    && matches!(dom.style(i, 14), "static" | "relative")
                    && !dom.concealed[i],
            })
            .collect();
        let blocks = page
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(block, b)| {
                Some(BlockPiece {
                    node: *by_node.get(&b.node)?,
                    block,
                })
            })
            .collect();
        let mut runs = Vec::new();
        let add = |out: &mut Vec<RunPiece>,
                   location: RunLocation,
                   text: &str,
                   origins: &[Option<usize>]| {
            for ((start, c), &node) in text.char_indices().zip(origins) {
                let Some(node) = node else {
                    continue;
                };
                let end = start + c.len_utf8();
                if let Some(last) = out
                    .last_mut()
                    .filter(|p| p.node == node && p.location == location && p.bytes.end == start)
                {
                    last.bytes.end = end;
                } else {
                    out.push(RunPiece {
                        node,
                        location: location.clone(),
                        bytes: start..end,
                    });
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
                RunLocation::Cell {
                    block,
                    row,
                    cell,
                    run,
                } => {
                    let BlockKind::Table(t) = &page.blocks[block].kind else {
                        continue;
                    };
                    &t.rows[row][cell].runs[run].text
                }
                RunLocation::Caption { block } => {
                    let BlockKind::Table(t) = &page.blocks[block].kind else {
                        continue;
                    };
                    &t.caption
                }
                _ => continue,
            };
            add(&mut runs, location, text, &origins);
        }
        fn grid_items(dom: &Dom, nodes: &[SourceNode], root: usize, out: &mut Vec<usize>) {
            for &child in &dom.children[root] {
                if !dom.is_element(child) {
                    continue;
                }
                if nodes[child].contents {
                    grid_items(dom, nodes, child, out);
                } else {
                    out.push(child);
                }
            }
        }
        let grids = (0..dom.name.len())
            .filter(|&i| {
                matches!(dom.style(i, 0), "grid" | "inline-grid")
                    || page.grids.iter().any(|g| g.node == dom.backend[i])
            })
            .map(|node| SourceGrid {
                node,
                items: {
                    let mut items = Vec::new();
                    grid_items(dom, &nodes, node, &mut items);
                    items
                },
            })
            .collect();
        let mut relations = SourceRelations {
            nodes,
            blocks,
            runs,
            grids,
            figures: Vec::new(),
            media_controls: Vec::new(),
            twins: Vec::new(),
        };
        for i in 0..dom.name.len() {
            if dom.name[i] != "FIGURE" {
                continue;
            }
            let mut parent = relations.nodes[i].parent;
            while parent.is_some_and(|p| dom.name[p] != "FIGURE") {
                parent = relations.nodes[parent.unwrap()].parent;
            }
            let captions = (0..dom.name.len())
                .filter(|&c| {
                    if dom.name[c] != "FIGCAPTION" {
                        return false;
                    }
                    let mut ancestor = relations.nodes[c].parent;
                    while let Some(a) = ancestor {
                        if dom.name[a] == "FIGURE" {
                            return a == i;
                        }
                        ancestor = relations.nodes[a].parent;
                    }
                    false
                })
                .collect();
            relations.figures.push(SourceFigure {
                node: i,
                parent,
                captions,
                blocks: relations
                    .blocks
                    .iter()
                    .filter(|b| relations.contains(i, b.node))
                    .map(|b| b.block)
                    .collect(),
                runs: relations
                    .runs
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| relations.contains(i, r.node))
                    .map(|(r, _)| r)
                    .collect(),
            });
        }
        relations.media_controls = page
            .media
            .iter()
            .map(|m| {
                let mut node = *by_node.get(&m.node)?;
                loop {
                    if let Some(control) = page
                        .controls
                        .iter()
                        .position(|c| c.node == dom.backend[node])
                    {
                        return Some(control);
                    }
                    node = relations.nodes[node].parent?;
                }
            })
            .collect();
        relations.twins = dom
            .twins
            .iter()
            .filter_map(|(&i, &(kind, other))| (kind == Twin::Spoken).then_some((i, other)))
            .collect();
        relations.twins.sort_unstable();
        relations.retain_log_slots(page);
        relations
    }
}
