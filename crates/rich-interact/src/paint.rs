//! Painting views into a region of the terminal, cell by cell.
//!
//! The region starts where the cursor is (inline) or at the top of the
//! alternate screen. The first paint writes every row; later paints write
//! only the cells [`Frame::diff`] reports as changed, moving the cursor with
//! relative sequences, so an unchanged view writes nothing at all. That is
//! what lets an idle event loop stay silent.
//!
//! Graphics (micro assets drawn with Kitty, iTerm2 or Sixel) come through a
//! [`PlacementSource`] ([`Painter::set_graphics`]): the placements it finds
//! on a view's cells are drawn after the cell diff, redrawn when the cells
//! under them are repainted, and released when the region finishes.

use std::sync::Arc;

use rich::{ColorSystem, Segment};
use rich_ext::frame::{plan_graphics, Change, Frame, PlacementSource};

use crate::component::View;

/// The visible stand-in for a terminal control, or `None` for any other
/// character. One character for one, in the style of
/// [`rich_ext::sanitize_terminal_controls`]: C0 controls as their control
/// pictures (`␛`, `␇`), DEL as `␡`, a tab as a space (it would move the
/// cursor), and C1 controls (8-bit CSI and OSC among them) as `�`.
pub(crate) fn visible(c: char) -> Option<char> {
    match c {
        '\t' => Some(' '),
        '\u{1b}' => Some('␛'),
        '\u{7f}' => Some('␡'),
        '\0'..='\u{1f}' => char::from_u32(0x2400 + c as u32),
        '\u{80}'..='\u{9f}' => Some('\u{fffd}'),
        _ => None,
    }
}

/// Make the terminal controls in `line`'s text visible, so no view (a
/// pager's content, a preview command's output, a label) can drive the
/// terminal: styles are already [`Style`](rich::Style)s, and whatever is
/// left in the text would otherwise reach it raw. Each control becomes one
/// character, so character offsets into the text still hold. Control
/// segments are left alone; the frame skips them.
pub fn sanitize_line(line: &mut [Segment]) {
    for segment in line.iter_mut().filter(|segment| !segment.control) {
        if segment.text.chars().any(|c| visible(c).is_some()) {
            segment.text = segment
                .text
                .chars()
                .map(|c| visible(c).unwrap_or(c))
                .collect();
        }
    }
}

/// Where `column`, a cell of `line` as rendered, lands once
/// [`sanitize_line`] has run: a control measures no cells (a tab and ESC
/// among them) until it is shown as a one-cell character, which moves every
/// cell after it. A zero-width control at `column` itself counts as before
/// it, so a caret after a prompt ending in ESC lands after its `␛`.
pub fn sanitized_column(line: &[Segment], column: usize) -> usize {
    let mut shift = 0isize;
    let mut base = 0;
    for segment in line.iter().filter(|segment| !segment.control) {
        for (index, c) in segment.text.char_indices() {
            let Some(shown) = visible(c) else { continue };
            let at = base + rich::cells::cell_len(&segment.text[..index]);
            let before = cell_len_of(c);
            if at < column || (at == column && before == 0) {
                shift += cell_len_of(shown) as isize - before as isize;
            }
        }
        base += segment.cell_length();
    }
    column.saturating_add_signed(shift)
}

fn cell_len_of(c: char) -> usize {
    let mut buffer = [0; 4];
    rich::cells::cell_len(c.encode_utf8(&mut buffer))
}

/// Paints successive views into one region.
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
    graphics: Option<Arc<dyn PlacementSource>>,
}

impl std::fmt::Debug for Painter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Painter")
            .field("system", &self.system)
            .field("no_color", &self.no_color)
            .field("previous", &self.previous)
            .field("extent", &self.extent)
            .field("height", &self.height)
            .field("row", &self.row)
            .field("cursor", &self.cursor)
            .field("cursor_shown", &self.cursor_shown)
            .field("graphics", &self.graphics.is_some())
            .finish()
    }
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
            graphics: None,
        }
    }

    /// Draw the graphics `source` places on each view's cells. Give it only
    /// when the painter writes to a terminal that showed it can draw them.
    pub fn set_graphics(&mut self, source: Option<Arc<dyn PlacementSource>>) {
        self.graphics = source;
        self.previous = None;
    }

    /// The graphics source, if any.
    pub fn graphics(&self) -> Option<&Arc<dyn PlacementSource>> {
        self.graphics.as_ref()
    }

    /// Repaint every row next time: after a resize, or after another
    /// program had the terminal.
    pub fn invalidate(&mut self) {
        self.previous = None;
    }

    /// Rows the region has taken on screen so far.
    pub fn extent(&self) -> usize {
        self.extent
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
            let start = segments.len();
            match &self.graphics {
                Some(source) => segments.extend(source.prepare(line.clone())),
                None => segments.extend(line.iter().cloned()),
            }
            // The one way to the terminal: nothing raw gets past it.
            sanitize_line(&mut segments[start..]);
            segments.push(Segment::line());
        }
        let mut frame = Frame::from_segments(&segments);
        let height = frame.height();
        if let Some(source) = &self.graphics {
            let placements = source
                .placements(&frame)
                .into_iter()
                .filter(|p| p.row + p.rows <= height)
                .collect();
            frame.set_placements(placements);
        }
        let mut out = String::new();
        let mut changes = Vec::new();
        let previous = self.previous.take();
        match &previous {
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
                    changes.push(Change {
                        row,
                        columns: 0..frame.row_width(row).max(1),
                    });
                }
                self.extent = self.extent.max(height);
            }
            Some(previous) => {
                changes = frame.diff(previous);
                for change in changes.clone() {
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
        if self.graphics.is_some() {
            let old = previous.as_ref().map_or(&[][..], Frame::placements);
            let plan = plan_graphics(old, frame.placements(), &changes);
            for (row, columns) in &plan.repaint {
                let end = columns.end.min(frame.cells(*row).len());
                if columns.start < end {
                    self.go(*row, columns.start, &mut out);
                    out.push_str(&frame.encode_span(
                        *row,
                        columns.start..end,
                        self.system,
                        self.no_color,
                    ));
                }
            }
            for placement in &plan.draw {
                self.go(placement.row, placement.column, &mut out);
                out.push_str(&placement.graphic.draw(placement.frame));
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
        if let Some(source) = &self.graphics {
            // What stays on screen keeps its images; the rest is released.
            let retained = match (&self.previous, clear) {
                (Some(frame), false) => frame.placements().to_vec(),
                _ => Vec::new(),
            };
            out.push_str(&source.release(&retained));
        }
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
    fn terminal_controls_in_a_view_are_painted_as_text() {
        let mut painter = Painter::new(None, false);
        let out = painter.paint(&view(&["a\x1bcb\u{9b}2Jc\x1b]0;t\x07d\x7fe\tf\x1b"]), 10);
        assert_eq!(out, "\ra␛cb\u{fffd}2Jc␛]0;t␇d␡e f␛");
        assert!(!out[1..].contains(['\x1b', '\u{9b}', '\x07', '\x7f', '\t']));
        // One character for one.
        let mut line = vec![Segment::new("x\x1by", None)];
        sanitize_line(&mut line);
        assert_eq!(line[0].text.chars().count(), 3);
    }

    #[test]
    fn sanitized_column_counts_the_shown_controls_before_it() {
        let line = vec![Segment::new("x\x1b", None), Segment::new("\ty z", None)];
        // Rendered, ESC and the tab take no cells: "x" then "y z".
        assert_eq!(sanitized_column(&line, 0), 0);
        assert_eq!(sanitized_column(&line, 1), 3);
        assert_eq!(sanitized_column(&line, 2), 4);
        assert_eq!(sanitized_column(&[Segment::new("ab", None)], 2), 2);
    }

    #[derive(Debug)]
    struct Star;

    impl rich_ext::frame::Graphic for Star {
        fn key(&self) -> u64 {
            7
        }
        fn draw(&self, frame: usize) -> String {
            format!("<star{frame}>")
        }
    }

    /// Places a star on every `*`, and swaps `?` for `*` first.
    struct Stars;

    impl PlacementSource for Stars {
        fn prepare(&self, line: Vec<Segment>) -> Vec<Segment> {
            line.into_iter()
                .map(|s| Segment::new(s.text.replace('?', "*"), s.style))
                .collect()
        }
        fn placements(&self, frame: &Frame) -> Vec<rich_ext::frame::Placement> {
            let mut out = Vec::new();
            for row in 0..frame.height() {
                for (column, cell) in frame.cells(row).iter().enumerate() {
                    if cell.text == "*" {
                        out.push(rich_ext::frame::Placement {
                            row,
                            column,
                            cols: 1,
                            rows: 1,
                            graphic: Arc::new(Star),
                            frame: 0,
                        });
                    }
                }
            }
            out
        }
        fn release(&self, retained: &[rich_ext::frame::Placement]) -> String {
            format!("<release {}>", retained.len())
        }
    }

    #[test]
    fn graphics_are_drawn_after_the_cells_and_redrawn_when_wiped() {
        let mut painter = Painter::new(None, false);
        painter.set_graphics(Some(Arc::new(Stars)));
        let out = painter.paint(&view(&["a ? b", "x"]), 10);
        assert_eq!(out, "\ra * b\r\n\rx\x1b[1A\r\x1b[2C<star0>");
        // Unchanged: nothing.
        assert_eq!(painter.paint(&view(&["a ? b", "x"]), 10), "");
        // A change beside it leaves it.
        assert_eq!(painter.paint(&view(&["a ? c", "x"]), 10), "\r\x1b[4Cc");
        // Gone: its cell is part of the diff; nothing is drawn.
        assert_eq!(painter.paint(&view(&["a . c", "x"]), 10), "\r\x1b[2C.");
        painter.paint(&view(&["a ? c", "x"]), 10);
        // Closing the region releases what is no longer on screen.
        assert!(painter.finish(true).starts_with("<release 0>"));
        let mut painter = Painter::new(None, false);
        painter.set_graphics(Some(Arc::new(Stars)));
        painter.paint(&view(&["?"]), 10);
        assert!(painter.finish(false).starts_with("<release 1>"));
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
