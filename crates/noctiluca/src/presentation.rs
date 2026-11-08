//! Retained layout dependencies, distinct from the authoritative page's
//! playback, decoded pixels and fixed-box log contents.
use crate::config::View;
use crate::layout::{self, Line, LogSlot, Marks, Part, Projection, Region, Span};
use crate::model::Page;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct Presentation {
    previous: Option<(Arc<Page>, View, u16, Marks)>,
    dependencies: Option<Page>,
    projection: Rc<Projection>,
    slots: Vec<LogSlot>,
}

// Exact equality, not the rounded geometry heuristic used by tab settling.
fn dependencies(page: &Page, logs: bool) -> Page {
    let mut key = page.clone();
    key.inline_pictures.clear();
    for m in &mut key.media {
        m.still = None;
        m.picture = None;
        m.playback = None;
    }
    if logs {
        for l in &mut key.logs {
            l.entries.clear();
            l.first_shown = None;
        }
    }
    key
}

#[derive(Default)]
pub(crate) struct Reading {
    previous: Option<(Page, u16, Marks)>,
    pieces: Vec<layout::Piece>,
}
impl Reading {
    pub fn update(&mut self, page: &Page, width: u16, marks: &Marks) -> Vec<layout::Piece> {
        let key = (dependencies(page, false), width, marks.clone());
        if self.previous.as_ref() != Some(&key) {
            self.pieces = layout::reader(page, width, marks);
            self.previous = Some(key);
        }
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
        let key = dependencies(page, true);
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
            if same
                && (changed.is_empty() || patch_logs)
                && self.dependencies.as_ref() == Some(&key)
            {
                let playback_changed = old.media.iter().zip(&page.media).any(|(a, b)| {
                    a.playback.is_some_and(|p| p.playing())
                        != b.playback.is_some_and(|p| p.playing())
                });
                if !changed.is_empty() || (playback_changed && view != View::Reader) {
                    let output = Rc::make_mut(&mut self.projection);
                    for slot in &self.slots {
                        if !changed.contains(&slot.node) {
                            continue;
                        }
                        let log = page.logs.iter().find(|l| l.node == slot.node).unwrap();
                        let lines =
                            layout::log_lines(log, slot.region.cols, slot.region.rows, page, marks);
                        let part = part_mut(output, slot.part);
                        patch(part, slot.region, &lines, width as usize);
                    }
                    if playback_changed {
                        refresh_media(output, page, width as usize);
                    }
                }
                self.dependencies = Some(key);
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
        self.dependencies = Some(key);
        self.previous = Some((page.clone(), view, width, marks.clone()));
        self.projection.clone()
    }
}

fn part_mut(p: &mut Projection, part: Part) -> &mut Projection {
    match part {
        Part::Flow => p,
        Part::Pinned => p.pinned.as_deref_mut().unwrap(),
        Part::Docked => p.docked.as_deref_mut().unwrap(),
    }
}

type Cell = (String, usize);
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
            let w = rich::cells::cell_len(&ch.to_string());
            if w == 0 {
                if let Some(cell) = out.iter_mut().rev().find(|c: &&mut Cell| !c.0.is_empty()) {
                    cell.0.push(ch);
                }
                continue;
            }
            if out.len() + w > width {
                break;
            }
            out.push((ch.to_string(), style));
            for _ in 1..w {
                out.push((String::new(), style));
            }
        }
    }
    out.resize(width, (" ".into(), 0));
    out
}
fn line(cells: Vec<Cell>, styles: &[CellStyle]) -> Line {
    let mut out = Line::default();
    for (text, style) in cells {
        let (style, control) = &styles[style];
        if text.is_empty() {
            continue;
        }
        match out.spans.last_mut() {
            Some(last) if last.style == *style && last.control == *control => {
                last.text.push_str(&text)
            }
            _ => out.spans.push(Span {
                text,
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
                let w = rich::cells::cell_len(&ch.to_string());
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
