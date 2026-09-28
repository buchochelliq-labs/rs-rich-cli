//! Semantic regions in frames (#226, phase 2).
//!
//! Core renderables report regions through
//! [`rich::protocol::RegionSink`] when one is installed, and tag the segments
//! they drew with the region's id in their style metadata. A
//! [`RegionRecorder`] is such a sink: it keeps each region's role, label and
//! parent. [`Frame::from_segments`] reads the tags off the styles (so styles
//! that differ only by a tag intern as one), and [`Frame::with_regions`] turns
//! them into [`Region`]s: cell spans per row, with nesting. Links need no
//! sink: [`Frame::regions`] derives them from the runs' styles.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use rich::protocol::{
    region_of, ConsoleRegions, RegionId, RegionInfo, RegionRole, RegionSink, REGION_META_KEY,
};
use rich::style::Meta;
use rich::{Console, ConsoleOptions, Renderable, Style};

use super::Frame;

/// A [`RegionSink`] that records every region it is told about: its info and
/// the region that was open when it started (its parent). Regions are
/// numbered from 0 in the order they start.
///
/// Rendering is expected to happen on one thread at a time, as `Console`
/// rendering does: the open-region stack is shared.
#[derive(Debug, Default)]
pub struct RegionRecorder {
    state: Mutex<RecorderState>,
}

#[derive(Debug, Default)]
struct RecorderState {
    regions: Vec<(RegionInfo, Option<RegionId>)>,
    open: Vec<RegionId>,
}

impl RegionRecorder {
    pub fn new() -> Self {
        RegionRecorder::default()
    }

    /// Every region recorded, in the order they started: id, info, parent.
    pub fn regions(&self) -> Vec<(RegionId, RegionInfo, Option<RegionId>)> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state
            .regions
            .iter()
            .enumerate()
            .map(|(index, (info, parent))| (RegionId(index as u64), info.clone(), *parent))
            .collect()
    }
}

impl RegionSink for RegionRecorder {
    fn enter(&self, region: RegionInfo) -> RegionId {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let id = RegionId(state.regions.len() as u64);
        let parent = state.open.last().copied();
        state.regions.push((region, parent));
        state.open.push(id);
        id
    }

    fn exit(&self, id: RegionId) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = state.open.iter().rposition(|open| *open == id) {
            state.open.truncate(at);
        }
    }
}

/// The cells `columns` of row `row`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub row: usize,
    pub columns: Range<usize>,
}

/// The smallest rectangle holding a region's spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub row: usize,
    pub column: usize,
    pub width: usize,
    pub height: usize,
}

/// A semantic region of a frame: the cells a renderable drew, with its role,
/// an optional label and link target, and its place in the nesting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub role: RegionRole,
    pub label: Option<String>,
    pub link: Option<String>,
    /// The enclosing region, as an index into the same list; `None` at the
    /// top. A parent always comes before its children.
    pub parent: Option<usize>,
    /// How many regions enclose this one.
    pub depth: usize,
    /// The cells covered, in row order, one span per contiguous stretch of a
    /// row. A region covers its children's cells too.
    pub spans: Vec<Span>,
}

impl Region {
    /// The smallest rectangle holding every span; `None` when there are none.
    pub fn bounds(&self) -> Option<Rect> {
        let first = self.spans.first()?;
        let (mut top, mut bottom) = (first.row, first.row);
        let (mut left, mut right) = (first.columns.start, first.columns.end);
        for span in &self.spans {
            top = top.min(span.row);
            bottom = bottom.max(span.row);
            left = left.min(span.columns.start);
            right = right.max(span.columns.end);
        }
        Some(Rect {
            row: top,
            column: left,
            width: right - left,
            height: bottom - top + 1,
        })
    }

    /// Whether the spans fill [`Region::bounds`] exactly: one span per row,
    /// all of one width, on consecutive rows.
    pub fn is_rectangle(&self) -> bool {
        let Some(bounds) = self.bounds() else {
            return false;
        };
        self.spans.len() == bounds.height
            && self.spans.iter().enumerate().all(|(index, span)| {
                span.row == bounds.row + index
                    && span.columns == (bounds.column..bounds.column + bounds.width)
            })
    }
}

/// A stable, lower-case name for a role, as HTML export and snapshots write
/// it: `panel`, `table`, `table-header`, `table-cell`, `table-footer`,
/// `rule`, `heading`, `code`, `link`, or an [`RegionRole::Other`] name.
pub fn role_name(role: &RegionRole) -> String {
    match role {
        RegionRole::Panel => "panel".into(),
        RegionRole::Table => "table".into(),
        RegionRole::TableHeader { .. } => "table-header".into(),
        RegionRole::TableCell { .. } => "table-cell".into(),
        RegionRole::TableFooter { .. } => "table-footer".into(),
        RegionRole::Rule => "rule".into(),
        RegionRole::Heading { .. } => "heading".into(),
        RegionRole::Code => "code".into(),
        RegionRole::Link => "link".into(),
        RegionRole::Other(name) => name.clone(),
        _ => "region".into(),
    }
}

/// `style` without its region tag, and the tag; `None` when it has none.
pub(super) fn split_region(style: &Style) -> Option<(RegionId, Style)> {
    let id = region_of(style)?;
    let rest: Meta = style
        .meta_ref()?
        .iter()
        .filter(|(key, _)| *key != REGION_META_KEY)
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect();
    let stripped = if rest.is_empty() {
        let bare = style.clear_meta_and_links();
        match style.link() {
            Some(link) => bare.with_link(link),
            None => bare,
        }
    } else {
        style.clone().with_meta(rest)
    };
    Some((id, stripped))
}

/// Render `renderable` with a [`RegionRecorder`] installed on a copy of
/// `console`, and build its frame with regions. Without regions to report,
/// the frame is what [`Frame::from_segments`] builds from a plain render.
pub fn render_frame(
    console: &Console,
    options: &ConsoleOptions,
    renderable: &dyn Renderable,
) -> Frame {
    let recorder = Arc::new(RegionRecorder::new());
    let mut observed = console.clone();
    observed.set_region_sink(Some(recorder.clone()));
    let segments = renderable.rich_render(&observed, options);
    Frame::from_segments(&segments).with_regions(&recorder)
}

impl Frame {
    /// This frame with the regions `recorder` recorded, as spans of the cells
    /// their tags cover. A region that drew nothing that is still in the
    /// frame is left out, and its children move up to the nearest ancestor
    /// that remains.
    pub fn with_regions(mut self, recorder: &RegionRecorder) -> Frame {
        let recorded = recorder.regions();
        let index: HashMap<RegionId, usize> = recorded
            .iter()
            .enumerate()
            .map(|(at, (id, _, _))| (*id, at))
            .collect();
        let parents: Vec<Option<usize>> = recorded
            .iter()
            .map(|(_, _, parent)| parent.and_then(|id| index.get(&id).copied()))
            .collect();
        let mut spans: Vec<Vec<Span>> = vec![Vec::new(); recorded.len()];
        for row in 0..self.height() {
            let mut column = 0;
            for run_index in self.row_range(row) {
                let width = self.runs[run_index].cells();
                let tag = self.run_tag(run_index);
                if width > 0 {
                    let mut at = tag.and_then(|id| index.get(&id).copied());
                    while let Some(region) = at {
                        push_span(&mut spans[region], row, column..column + width);
                        at = parents[region];
                    }
                }
                column += width;
            }
        }
        // Keep regions that cover something; renumber, re-parent.
        let mut kept: Vec<Option<usize>> = vec![None; recorded.len()];
        let mut regions: Vec<Region> = Vec::new();
        for (at, ((_, info, _), spans)) in recorded.into_iter().zip(spans).enumerate() {
            if spans.is_empty() {
                continue;
            }
            let mut parent = parents[at];
            while let Some(p) = parent {
                if kept[p].is_some() {
                    break;
                }
                parent = parents[p];
            }
            let parent = parent.and_then(|p| kept[p]);
            let depth = parent.map_or(0, |p| regions[p].depth + 1);
            kept[at] = Some(regions.len());
            regions.push(Region {
                role: info.role,
                label: info.label,
                link: info.link,
                parent,
                depth,
                spans,
            });
        }
        self.region_ids = kept
            .iter()
            .enumerate()
            .filter_map(|(at, kept)| kept.map(|k| (RegionId(at as u64), k)))
            .collect();
        self.regions = regions;
        self
    }

    /// The regions: those recorded with [`Frame::with_regions`], then one
    /// [`RegionRole::Link`] region per stretch of runs sharing a link
    /// target. A link region's parent is the innermost recorded region
    /// holding its first cell.
    pub fn regions(&self) -> Vec<Region> {
        let mut regions = self.regions.clone();
        let mut open: Option<usize> = None;
        for row in 0..self.height() {
            let mut column = 0;
            for run_index in self.row_range(row) {
                let run = self.runs[run_index];
                let width = run.cells();
                if width == 0 {
                    continue;
                }
                let link = self.styles.get(run.style).and_then(Style::link);
                match (link, open) {
                    (Some(link), Some(at)) if regions[at].link.as_deref() == Some(link) => {
                        push_span(&mut regions[at].spans, row, column..column + width);
                    }
                    (Some(link), _) => {
                        let parent = self.innermost(run_index);
                        let depth = parent.map_or(0, |p| regions[p].depth + 1);
                        regions.push(Region {
                            role: RegionRole::Link,
                            label: None,
                            link: Some(link.to_string()),
                            parent,
                            depth,
                            spans: vec![Span {
                                row,
                                columns: column..column + width,
                            }],
                        });
                        open = Some(regions.len() - 1);
                    }
                    (None, _) => open = None,
                }
                column += width;
            }
        }
        regions
    }

    /// Whether the frame has recorded regions (links aside).
    pub fn has_regions(&self) -> bool {
        !self.regions.is_empty()
    }

    /// The recorded region that drew run `run_index`, as an index into
    /// [`Frame::regions`].
    pub(super) fn innermost(&self, run_index: usize) -> Option<usize> {
        self.run_tag(run_index)
            .and_then(|id| self.region_ids.get(&id).copied())
    }
}

/// Add `columns` of `row` to `spans`, joining it to the last span when they
/// touch.
fn push_span(spans: &mut Vec<Span>, row: usize, columns: Range<usize>) {
    match spans.last_mut() {
        Some(last) if last.row == row && last.columns.end == columns.start => {
            last.columns.end = columns.end;
        }
        _ => spans.push(Span { row, columns }),
    }
}
