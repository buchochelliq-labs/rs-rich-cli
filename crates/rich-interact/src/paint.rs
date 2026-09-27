//! Painting views into a region of the terminal, cell by cell.
//!
//! The region starts where the cursor is (inline) or at the top of the
//! alternate screen. The first paint writes every row; later paints write
//! only the cells [`Frame::diff`] reports as changed, moving the cursor with
//! relative sequences, so an unchanged view writes nothing at all. That is
//! what lets an idle event loop stay silent.

use rich::{ColorSystem, Segment};
use rich_ext::frame::Frame;

use crate::component::View;

/// Paints successive views into one region.
#[derive(Debug)]
pub struct Painter {
    system: Option<ColorSystem>,
    no_color: bool,
    previous: Option<Frame>,
    /// Rows the region has taken on screen so far.
    extent: usize,
    /// Rows of the last view painted.
    height: usize,
    /// The row of the region the cursor is on.
    row: usize,
    /// Where the terminal cursor was left for the view, and whether shown.
    cursor: Option<(usize, usize)>,
    cursor_shown: bool,
}

impl Painter {
    pub fn new(system: Option<ColorSystem>, no_color: bool) -> Painter {
        Painter {
            system,
            no_color,
            previous: None,
            extent: 0,
            height: 0,
            row: 0,
            cursor: None,
            // The session hides the cursor on entry.
            cursor_shown: false,
        }
    }

    /// Repaint every row next time: after a resize, or after another
    /// program had the terminal.
    pub fn invalidate(&mut self) {
        self.previous = None;
    }

    /// Forget the region entirely, as after the alternate screen was left
    /// and entered again: the next paint starts at the cursor.
    pub fn reset(&mut self) {
        self.previous = None;
        self.extent = 0;
        self.height = 0;
        self.row = 0;
        self.cursor_shown = false;
    }

    fn up_to(&mut self, row: usize, out: &mut String) {
        if row < self.row {
            out.push_str(&format!("\x1b[{}A", self.row - row));
            self.row = row;
        }
    }

    /// Put the cursor at the start of `row`, adding rows below the region
    /// (with line breaks, which scroll at the bottom of the screen) as
    /// needed.
    fn go(&mut self, row: usize, column: usize, out: &mut String) {
        if row < self.row {
            self.up_to(row, out);
        } else if row > self.row {
            let within = row.min(self.extent.saturating_sub(1));
            if within > self.row {
                out.push_str(&format!("\x1b[{}B", within - self.row));
                self.row = within;
            }
            while self.row < row {
                out.push_str("\r\n");
                self.row += 1;
                self.extent = self.extent.max(self.row + 1);
            }
        }
        out.push('\r');
        if column > 0 {
            out.push_str(&format!("\x1b[{column}C"));
        }
    }

    /// Paint `view`, at most `max_rows` rows of it, and return what to
    /// write (empty when nothing changed).
    pub fn paint(&mut self, view: &View, max_rows: usize) -> String {
        let rows = view.lines.len().min(max_rows);
        let mut segments = Vec::new();
        // Every line ends with a newline, so a trailing (or only) empty
        // line is still a row of the frame rather than a terminator.
        for line in &view.lines[..rows] {
            segments.extend(line.iter().cloned());
            segments.push(Segment::line());
        }
        let frame = Frame::from_segments(&segments);
        let height = frame.height();
        let mut out = String::new();
        match self.previous.take() {
            None => {
                // Everything: from the top of the region down.
                if self.extent > 0 {
                    self.up_to(0, &mut out);
                    out.push('\r');
                    out.push_str("\x1b[J");
                }
                for row in 0..height {
                    self.go(row, 0, &mut out);
                    out.push_str(&frame.encode_span(
                        row,
                        0..frame.row_width(row),
                        self.system,
                        self.no_color,
                    ));
                }
                self.extent = self.extent.max(height);
            }
            Some(previous) => {
                for change in frame.diff(&previous) {
                    let row = change.row;
                    self.go(row, change.columns.start, &mut out);
                    if row < height {
                        let width = frame.cells(row).len();
                        let end = change.columns.end.min(width);
                        if change.columns.start < end {
                            out.push_str(&frame.encode_span(
                                row,
                                change.columns.start..end,
                                self.system,
                                self.no_color,
                            ));
                        }
                        if change.columns.end > width {
                            out.push_str("\x1b[K");
                        }
                    } else {
                        // A row the view no longer has.
                        out.push_str("\x1b[K");
                    }
                }
                self.extent = self.extent.max(height);
            }
        }
        // The caret, or the cursor hidden.
        let cursor = view.cursor.filter(|(row, _)| *row < height.max(1));
        match cursor {
            Some((row, column)) => {
                if self.cursor != cursor || !out.is_empty() {
                    self.go(row, column, &mut out);
                }
                if !self.cursor_shown {
                    out.push_str("\x1b[?25h");
                    self.cursor_shown = true;
                }
            }
            None => {
                if self.cursor_shown {
                    out.push_str("\x1b[?25l");
                    self.cursor_shown = false;
                }
            }
        }
        self.cursor = cursor;
        self.height = height;
        self.previous = Some(frame);
        out
    }

    /// Leave the region: below it, so later output follows, or cleared
    /// (`clear`) with the cursor where the region began. The cursor is
    /// shown again either way.
    pub fn finish(&mut self, clear: bool) -> String {
        let mut out = String::new();
        if clear {
            if self.extent > 0 {
                self.up_to(0, &mut out);
                out.push('\r');
                out.push_str("\x1b[J");
            }
        } else if self.extent > 0 {
            // Below the last view, clearing rows a taller earlier view left
            // (a picker that collapsed to its answer).
            if self.height > 0 {
                self.go(self.height - 1, 0, &mut out);
                out.push_str("\r\n");
            } else {
                self.up_to(0, &mut out);
                out.push('\r');
            }
            if self.extent > self.height {
                out.push_str("\x1b[J");
            }
        }
        if !self.cursor_shown {
            out.push_str("\x1b[?25h");
        }
        self.reset();
        self.cursor_shown = true;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(rows: &[&str]) -> View {
        View::new(
            rows.iter()
                .map(|row| vec![Segment::new(row.to_string(), None)])
                .collect(),
        )
    }

    #[test]
    fn first_paint_writes_every_row() {
        let mut painter = Painter::new(None, false);
        let out = painter.paint(&view(&["one", "two"]), 10);
        assert_eq!(out, "\rone\r\n\rtwo");
    }

    #[test]
    fn unchanged_views_write_nothing() {
        let mut painter = Painter::new(None, false);
        painter.paint(&view(&["one", "two"]), 10);
        assert_eq!(painter.paint(&view(&["one", "two"]), 10), "");
    }

    #[test]
    fn later_paints_write_only_changed_cells() {
        let mut painter = Painter::new(None, false);
        painter.paint(&view(&["alpha", "beta", "gamma"]), 10);
        // The cursor is at the end of row 2; "beta" -> "bets" changes one
        // cell of row 1.
        let out = painter.paint(&view(&["alpha", "bets", "gamma"]), 10);
        assert_eq!(out, "\x1b[1A\r\x1b[3Cs");
        // A shorter row erases what is left of the old one.
        let out = painter.paint(&view(&["alpha", "b", "gamma"]), 10);
        assert_eq!(out, "\r\x1b[1C\x1b[K");
    }

    #[test]
    fn growing_views_add_rows_and_shrinking_ones_clear_them() {
        let mut painter = Painter::new(None, false);
        painter.paint(&view(&["a"]), 10);
        let out = painter.paint(&view(&["a", "b"]), 10);
        assert_eq!(out, "\r\n\rb");
        let out = painter.paint(&view(&["a"]), 10);
        assert_eq!(out, "\r\x1b[K");
        // Rows past `max_rows` are cut.
        let mut painter = Painter::new(None, false);
        assert_eq!(painter.paint(&view(&["a", "b", "c"]), 2), "\ra\r\n\rb");
    }

    #[test]
    fn places_the_caret_and_finishes_below() {
        let mut painter = Painter::new(None, false);
        let out = painter.paint(&view(&["name: ab", "hint"]).with_cursor(0, 8), 10);
        assert_eq!(out, "\rname: ab\r\n\rhint\x1b[1A\r\x1b[8C\x1b[?25h");
        let out = painter.finish(false);
        assert_eq!(out, "\x1b[1B\r\r\n");
        let mut painter = Painter::new(None, false);
        painter.paint(&view(&["x", "y"]), 10);
        assert_eq!(painter.finish(true), "\x1b[1A\r\x1b[J\x1b[?25h");
    }

    #[test]
    fn empty_rows_at_the_end_are_rows() {
        // A trailing empty line is part of the region, so finishing lands
        // below it rather than on it.
        let mut painter = Painter::new(None, false);
        assert_eq!(painter.paint(&view(&["a", ""]), 10), "\ra\r\n\r");
        assert_eq!(painter.finish(false), "\r\r\n\x1b[?25h");
        // A view of one empty line is one row.
        let mut painter = Painter::new(None, false);
        assert_eq!(painter.paint(&view(&[""]), 10), "\r");
        assert_eq!(painter.finish(false), "\r\r\n\x1b[?25h");
    }

    #[test]
    fn invalidate_repaints_the_region() {
        let mut painter = Painter::new(None, false);
        painter.paint(&view(&["a", "b"]), 10);
        painter.invalidate();
        assert_eq!(
            painter.paint(&view(&["a", "b"]), 10),
            "\x1b[1A\r\x1b[J\ra\x1b[1B\rb"
        );
    }
}
