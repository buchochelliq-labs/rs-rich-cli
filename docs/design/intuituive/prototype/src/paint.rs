//! A painter that knows rows: the spike's answer to where the time went.
//!
//! `rich-interact`'s `Painter` builds a cell frame of the whole view every
//! paint (cloning, sanitising and segmenting every row) and then diffs it.
//! With a retained tree most rows are the same segments as last frame, so
//! this painter compares each row's segments first and builds cells only
//! for rows that differ, diffing them against that row's previous cells.
//! It positions the cursor absolutely (a full-screen region); the real
//! painter's inline mode and graphics are out of the spike's scope.

use rich::{ColorSystem, Segment};
use rich_ext::frame::Frame;
use rich_interact::paint::sanitize_line;

#[derive(Default)]
pub struct RowPainter {
    rows: Vec<(Vec<Segment>, Frame)>,
}

impl RowPainter {
    pub fn paint(&mut self, lines: &[Vec<Segment>], system: Option<ColorSystem>) -> String {
        let mut out = String::new();
        for (row, line) in lines.iter().enumerate() {
            if self.rows.get(row).is_some_and(|(old, _)| old == line) {
                continue;
            }
            let mut segments = line.clone();
            sanitize_line(&mut segments);
            segments.push(Segment::line());
            let frame = Frame::from_segments(&segments);
            let changes = match self.rows.get(row) {
                Some((_, old)) => frame.diff(old),
                None => vec![rich_ext::frame::Change {
                    row: 0,
                    columns: 0..frame.row_width(0).max(1),
                }],
            };
            let width = frame.row_width(0);
            for change in changes {
                out.push_str(&format!("\x1b[{};{}H", row + 1, change.columns.start + 1));
                let end = change.columns.end.min(width);
                if change.columns.start < end {
                    out.push_str(&frame.encode_span(0, change.columns.start..end, system, false));
                }
                if change.columns.end > width {
                    out.push_str("\x1b[K");
                }
            }
            let entry = (line.clone(), frame);
            if row < self.rows.len() {
                self.rows[row] = entry;
            } else {
                self.rows.push(entry);
            }
        }
        out
    }
}
