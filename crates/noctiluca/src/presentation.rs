//! Retained layout dependencies, distinct from the authoritative page's
//! playback, decoded pixels and fixed-box log contents.
use crate::config::View;
use crate::layout::{self, Line, LogSlot, Marks, Part, Projection, Region, Span};
use crate::model::{Log, Media, Page};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct Presentation {
    previous: Option<(Arc<Page>, View, u16, Marks)>,
    projection: Rc<Projection>,
    slots: Vec<LogSlot>,
}

// Borrow every layout dependency exactly. Exhaustive destructuring makes
// additions to Page/Media/Log require a conscious invalidation decision.
fn page_key(page: &Page) -> impl PartialEq + '_ {
    let Page {
        url,
        title,
        document,
        blocks,
        controls,
        media: _,
        width,
        height,
        truncated,
        has_ax,
        background,
        logs: _,
        inline_pictures: _,
        regions,
        grids,
        indicators,
        shelves,
        layers,
        overlays,
        scroll_y,
        view_height,
    } = page;
    (
        (url, title, document, blocks, controls),
        (
            width,
            height,
            truncated,
            has_ax,
            background,
            scroll_y,
            view_height,
        ),
        (regions, grids, indicators, shelves, layers, overlays),
    )
}
fn media_key(media: &Media) -> impl PartialEq + '_ {
    let Media {
        node,
        kind,
        rect,
        alt,
        src,
        still: _,
        picture: _,
        captioned,
        playback: _,
        poster,
        controls,
        autoplay,
        empty_alt,
        aria_hidden,
        in_label,
        revision,
        text,
        pending,
    } = media;
    (
        (node, kind, rect, alt, src, captioned, poster),
        (
            controls,
            autoplay,
            empty_alt,
            aria_hidden,
            in_label,
            revision,
            text,
            pending,
        ),
    )
}
fn log_key(log: &Log) -> impl PartialEq + '_ {
    let Log {
        node,
        kind,
        rect,
        label,
        entries: _,
        first_shown: _,
    } = log;
    (node, kind, rect, label)
}
fn same_dependencies(a: &Page, b: &Page, patch_logs: bool) -> bool {
    page_key(a) == page_key(b)
        && a.media
            .iter()
            .map(media_key)
            .eq(b.media.iter().map(media_key))
        && if patch_logs {
            a.logs.iter().map(log_key).eq(b.logs.iter().map(log_key))
        } else {
            a.logs == b.logs
        }
}

#[derive(Default)]
pub(crate) struct Reading {
    previous: Option<(Arc<Page>, u16, Marks)>,
    pieces: Rc<Vec<layout::Piece>>,
}
impl Reading {
    pub fn update(
        &mut self,
        page: &Arc<Page>,
        width: u16,
        marks: &Marks,
    ) -> Rc<Vec<layout::Piece>> {
        let same = self
            .previous
            .as_ref()
            .is_some_and(|(old, old_width, old_marks)| {
                *old_width == width && old_marks == marks && same_dependencies(old, page, false)
            });
        if !same {
            self.pieces = Rc::new(layout::reader(page, width, marks));
        }
        self.previous = Some((page.clone(), width, marks.clone()));
        self.pieces.clone()
    }
}

impl Presentation {
    #[cfg(test)]
    pub fn update(&mut self, page: &Page, view: View, width: u16, marks: &Marks) -> Rc<Projection> {
        self.update_shared(&Arc::new(page.clone()), view, width, marks)
    }
    pub fn update_shared(
        &mut self,
        page: &Arc<Page>,
        view: View,
        width: u16,
        marks: &Marks,
    ) -> Rc<Projection> {
        if let Some((old, old_view, old_width, old_marks)) = &self.previous {
            let mut outer_marks = marks.clone();
            outer_marks.logs = old_marks.logs.clone();
            let same = *old_view == view && *old_width == width && outer_marks == *old_marks;
            let changed: Vec<_> = page
                .logs
                .iter()
                .enumerate()
                .filter(|(i, l)| {
                    old.logs.get(*i) != Some(*l)
                        || marks.logs.get(&l.node) != old_marks.logs.get(&l.node)
                })
                .map(|(_, l)| l.node)
                .collect();
            let patch_logs = view == View::Projection
                && changed
                    .iter()
                    .all(|node| self.slots.iter().any(|s| s.node == *node));
            if same && (changed.is_empty() || patch_logs) && { same_dependencies(old, page, true) }
            {
                let playback_changed = old.media.iter().zip(&page.media).any(|(a, b)| {
                    a.playback.is_some_and(|p| p.playing())
                        != b.playback.is_some_and(|p| p.playing())
                });
                if !changed.is_empty() || (playback_changed && view != View::Reader) {
                    let output = { Rc::make_mut(&mut self.projection) };
                    for slot in &self.slots {
                        if !changed.contains(&slot.node) {
                            continue;
                        }
                        let log = page.logs.iter().find(|l| l.node == slot.node).unwrap();
                        let lines = {
                            layout::log_lines(log, slot.region.cols, slot.region.rows, page, marks)
                        };
                        let part = part_mut(output, slot.part);
                        {
                            patch(part, slot.region, &lines, width as usize);
                        }
                    }
                    if playback_changed {
                        refresh_media(output, page, width as usize);
                    }
                }
                self.previous = Some((page.clone(), view, width, marks.clone()));
                return self.projection.clone();
            }
        }
        self.slots.clear();
        self.projection = Rc::new(crate::app::lay_out_with_logs(
            page,
            view,
            width,
            marks,
            &mut self.slots,
        ));
        self.previous = Some((page.clone(), view, width, marks.clone()));
        self.projection.clone()
    }
}

fn part_mut(p: &mut Projection, part: Part) -> &mut Projection {
    match part {
        Part::Flow => p,
        Part::Pinned => p.pinned.as_deref_mut().unwrap(),
        Part::Docked => p.docked.as_deref_mut().unwrap(),
        Part::Overlay(_) => unreachable!("dialog projections use independent slots"),
    }
}

#[derive(Clone)]
enum Glyph {
    Scalar(char),
    Joined(String),
    Continuation,
}
impl Glyph {
    fn join(&mut self, ch: char) {
        match self {
            Self::Scalar(base) => {
                let mut text = String::new();
                text.push(*base);
                text.push(ch);
                *self = Self::Joined(text);
            }
            Self::Joined(text) => text.push(ch),
            Self::Continuation => unreachable!(),
        }
    }
    fn push_to(&self, text: &mut String) {
        match self {
            Self::Scalar(ch) => text.push(*ch),
            Self::Joined(s) => text.push_str(s),
            Self::Continuation => {}
        }
    }
}
type Cell = (Glyph, usize);
type CellStyle = (String, Option<usize>);
fn cells(line: &Line, width: usize, styles: &mut Vec<CellStyle>) -> Vec<Cell> {
    let mut out = Vec::new();
    for span in &line.spans {
        let style = styles
            .iter()
            .position(|(s, control)| s == &span.style && *control == span.control)
            .unwrap_or_else(|| {
                styles.push((span.style.clone(), span.control));
                styles.len() - 1
            });
        for ch in span.text.chars() {
            let w = rich::cells::cell_len(ch.encode_utf8(&mut [0; 4]));
            if w == 0 {
                if let Some(cell) = out
                    .iter_mut()
                    .rev()
                    .find(|c: &&mut Cell| !matches!(c.0, Glyph::Continuation))
                {
                    cell.0.join(ch);
                }
                continue;
            }
            if out.len() + w > width {
                break;
            }
            out.push((Glyph::Scalar(ch), style));
            for _ in 1..w {
                out.push((Glyph::Continuation, style));
            }
        }
    }
    out.resize(width, (Glyph::Scalar(' '), 0));
    out
}
fn line(cells: Vec<Cell>, styles: &[CellStyle]) -> Line {
    let mut out = Line::default();
    for (text, style) in cells {
        let (style, control) = &styles[style];
        if matches!(text, Glyph::Continuation) {
            continue;
        }
        match out.spans.last_mut() {
            Some(last) if last.style == *style && last.control == *control => {
                text.push_to(&mut last.text)
            }
            _ => out.spans.push(Span {
                text: {
                    let mut out = String::new();
                    text.push_to(&mut out);
                    out
                },
                style: style.clone(),
                control: *control,
                picture: None,
            }),
        }
    }
    if let Some(last) = out.spans.last_mut()
        && last.style.is_empty()
    {
        last.text.truncate(last.text.trim_end().len());
    }
    out.spans.retain(|s| !s.text.is_empty());
    out
}
fn patch(p: &mut Projection, r: Region, lines: &[Line], width: usize) {
    let width = width.max(10); // The projection uses the same minimum.
    p.inline
        .retain(|(at, _)| !layout::rectangles_overlap(*at, r));
    for (k, replacement) in lines.iter().enumerate().take(r.rows) {
        let row = r.row + k;
        let Some(existing) = p.lines.get_mut(row) else {
            continue;
        };
        let mut styles = vec![(String::new(), None)];
        let mut all = cells(existing, width, &mut styles);
        let replacement_cells = cells(replacement, r.cols, &mut styles);
        let end = (r.column + r.cols).min(width);
        all[r.column..end].clone_from_slice(&replacement_cells[..end - r.column]);
        *existing = line(all, &styles);
        let mut x = r.column;
        for span in &replacement.spans {
            let start = x;
            for ch in span.text.chars() {
                let w = rich::cells::cell_len(ch.encode_utf8(&mut [0; 4]));
                if x + w > end {
                    break;
                }
                x += w;
            }
            if let Some(src) = &span.picture
                && x > start
            {
                p.inline.push((
                    Region {
                        row,
                        column: start,
                        cols: x - start,
                        rows: 1,
                        media: usize::MAX,
                    },
                    src.clone(),
                ));
            }
        }
    }
    // Projection emits inline regions in box order; log patches retain
    // visual reading order, including repeated pictures in a single row.
    p.inline.sort_by_key(|(r, _)| (r.row, r.column));
}
fn refresh_media(p: &mut Projection, page: &Page, width: usize) {
    for r in p.regions.clone() {
        let media = &page.media[r.media];
        if media.kind != crate::model::MediaKind::Video {
            continue;
        }
        let replacement = layout::play_mark(media, r.cols).unwrap_or_else(|| {
            let label = if r.rows - 1 == r.rows / 2 {
                crate::clean::truncate(&media.alt, r.cols)
            } else {
                String::new()
            };
            let pad = r.cols.saturating_sub(rich::cells::cell_len(&label));
            Line {
                spans: vec![
                    Span::new(label, "italic"),
                    Span::new("░".repeat(pad), "bright_black"),
                ],
            }
        });
        patch(
            p,
            Region {
                row: r.row + r.rows - 1,
                rows: 1,
                ..r
            },
            &[replacement],
            width,
        );
    }
    for (row, column, over) in p.overlays.clone() {
        patch(
            p,
            Region {
                row,
                column,
                cols: over.width(),
                rows: 1,
                media: usize::MAX,
            },
            &[over],
            width,
        );
    }
    if let Some(s) = p.pinned.as_mut() {
        refresh_media(s, page, width);
    }
    if let Some(s) = p.docked.as_mut() {
        refresh_media(s, page, width);
    }
}

#[cfg(test)]
mod narrow_source_tests {
    use super::*;
    #[test]
    fn transformed_log_slot_keeps_readable_width_on_passive_updates() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/stream-narrow-source.json");
        let mut page = crate::model::build(
            &crate::testing::snapshot_file(&path).unwrap(),
            &mut crate::model::Revisions::new(),
        );
        let mut shown = Presentation::default();
        let mut marks = Marks::default();
        shown.update(&page, View::Projection, 40, &marks);
        let slot = shown
            .slots
            .iter()
            .find(|s| s.node == page.logs[0].node)
            .unwrap()
            .region;
        assert!(slot.cols >= 30, "actual transformed slot: {slot:?}");
        assert_eq!(slot.rows, 18);
        let key = page.logs[0].entries.last().unwrap().key;
        marks
            .logs
            .insert(page.logs[0].node, crate::layout::LogScroll::Paused(key));
        let before = shown.update(&page, View::Projection, 40, &marks);
        let before_lines = before.lines[slot.row..slot.row + slot.rows].to_vec();
        page.logs[0].entries.push(crate::model::Entry {
            key: key + 1000,
            runs: vec![crate::model::Run::plain("new message after paused anchor")],
        });
        let after = shown.update(&page, View::Projection, 40, &marks);
        assert_eq!(&after.lines[slot.row..slot.row + slot.rows], before_lines);
        assert!(
            before_lines
                .iter()
                .any(|l| l.plain().contains("tide is coming"))
        );
        marks.logs.clear();
        let resumed = shown.update(&page, View::Projection, 40, &marks);
        assert!(
            resumed
                .lines
                .iter()
                .any(|l| l.plain().contains("new message after paused anchor"))
        );
        assert_eq!(shown.slots[0].region, slot);
    }
}
