//! Frames: a rendered result as rows of styled runs (#226).
//!
//! A [`Frame`] is built from the segment stream core already returns, so no
//! renderable changes. It keeps that stream's shape (one [`Run`] per segment
//! piece) over one text arena, with every style stored once in a
//! [`StyleTable`]. From it:
//!
//! - [`Frame::to_ansi`] writes exactly what `Console::segments_to_string`
//!   writes for the same (control-free) stream, byte for byte;
//! - [`Frame::to_ansi_merged`] merges adjacent runs of one style first, which
//!   is smaller but not upstream's bytes (opt-in, for live repaint and our own
//!   exports);
//! - [`Frame::cells`] derives one cell per grapheme on demand, with core's
//!   widths;
//! - [`Frame::diff`] lists the cells that changed against a previous frame.
//!
//! Control segments are dropped: a frame holds content, and whatever paints
//! it owns cursor movement. See `docs/design/render-tree.md`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use rich::cells::{cell_len, split_graphemes};
use rich::{ColorSystem, Console, Segment, Style};

/// An index into a [`StyleTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StyleId(u32);

impl StyleId {
    /// "No style": a segment whose `style` is `None`.
    pub const NONE: StyleId = StyleId(0);

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Styles, stored once each. Runs refer to them by [`StyleId`].
///
/// Interning hashes a style's public parts (colours, attributes and link) and
/// confirms a match with `==`, so metadata still tells styles apart.
#[derive(Clone, Debug)]
pub struct StyleTable {
    styles: Vec<Option<Style>>,
    index: HashMap<u64, Vec<StyleId>>,
    /// The last style interned: consecutive segments usually share one.
    last: StyleId,
}

impl Default for StyleTable {
    fn default() -> Self {
        StyleTable {
            styles: vec![None],
            index: HashMap::new(),
            last: StyleId::NONE,
        }
    }
}

impl StyleTable {
    pub fn new() -> Self {
        StyleTable::default()
    }

    /// The id of `style`, adding it if it is new. `None` is [`StyleId::NONE`].
    pub fn intern(&mut self, style: Option<&Style>) -> StyleId {
        let Some(style) = style else {
            return StyleId::NONE;
        };
        if self.styles[self.last.index()].as_ref() == Some(style) {
            return self.last;
        }
        let key = style_key(style);
        let bucket = self.index.entry(key).or_default();
        let id = match bucket
            .iter()
            .find(|id| self.styles[id.index()].as_ref() == Some(style))
        {
            Some(id) => *id,
            None => {
                let id = StyleId(self.styles.len() as u32);
                self.styles.push(Some(style.clone()));
                bucket.push(id);
                id
            }
        };
        self.last = id;
        id
    }

    /// The style behind `id`; `None` for [`StyleId::NONE`].
    pub fn get(&self, id: StyleId) -> Option<&Style> {
        self.styles.get(id.index()).and_then(Option::as_ref)
    }

    /// The number of entries, counting [`StyleId::NONE`].
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Always false: the table always holds [`StyleId::NONE`].
    pub fn is_empty(&self) -> bool {
        false
    }
}

fn style_key(style: &Style) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    style.color().hash(&mut hasher);
    style.bgcolor().hash(&mut hasher);
    for index in 0..13 {
        style.attr(index).hash(&mut hasher);
    }
    style.link().hash(&mut hasher);
    hasher.finish()
}

/// The run is followed by a line break that belonged to its segment.
const NEWLINE: u8 = 1;
/// The run continues the previous run's segment after that line break.
const JOIN: u8 = 2;

/// A piece of one segment on one row: a slice of the frame's text, its width
/// in cells and its style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    text: u32,
    len: u32,
    cells: u32,
    style: StyleId,
    flags: u8,
}

impl Run {
    /// The width in terminal cells.
    pub fn cells(&self) -> usize {
        self.cells as usize
    }

    pub fn style(&self) -> StyleId {
        self.style
    }

    fn range(&self) -> Range<usize> {
        self.text as usize..(self.text + self.len) as usize
    }
}

/// One terminal cell of a row, from [`Frame::cells`]. The cell after a wide
/// grapheme is a continuation: empty text and width 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell<'a> {
    pub text: &'a str,
    pub width: u8,
    pub style: StyleId,
}

impl Cell<'_> {
    pub fn is_continuation(&self) -> bool {
        self.width == 0 && self.text.is_empty()
    }
}

/// Cells `columns` of `row` differ between two frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub row: usize,
    pub columns: Range<usize>,
}

/// Rows of styled runs, with interned styles. See the [module docs](self).
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// Each row as a range of `runs`.
    rows: Vec<Range<u32>>,
    runs: Vec<Run>,
    text: String,
    styles: StyleTable,
    /// The stream ended with a line break, so the last row is terminated.
    trailing_newline: bool,
}

impl Frame {
    /// Build a frame from a segment stream, one run per piece of a segment
    /// between line breaks. Control segments are dropped.
    pub fn from_segments(segments: &[Segment]) -> Frame {
        let mut frame = Frame {
            runs: Vec::with_capacity(segments.len()),
            text: String::with_capacity(segments.iter().map(|s| s.text.len()).sum()),
            ..Frame::default()
        };
        let mut row_start = 0u32;
        for segment in segments.iter().filter(|segment| !segment.control) {
            let style = frame.styles.intern(segment.style.as_ref());
            let mut flags = 0;
            let mut rest = segment.text.as_str();
            loop {
                match rest.find('\n') {
                    Some(at) => {
                        frame.push(&rest[..at], style, flags | NEWLINE);
                        let end = frame.runs.len() as u32;
                        frame.rows.push(row_start..end);
                        row_start = end;
                        rest = &rest[at + 1..];
                        flags = JOIN;
                    }
                    None => {
                        if !rest.is_empty() {
                            frame.push(rest, style, flags);
                        }
                        break;
                    }
                }
            }
        }
        let end = frame.runs.len() as u32;
        if end > row_start {
            frame.rows.push(row_start..end);
        } else {
            frame.trailing_newline = !frame.rows.is_empty();
        }
        frame
    }

    fn push(&mut self, text: &str, style: StyleId, flags: u8) {
        let offset = self.text.len() as u32;
        self.text.push_str(text);
        self.runs.push(Run {
            text: offset,
            len: text.len() as u32,
            cells: cell_len(text) as u32,
            style,
            flags,
        });
    }

    /// A copy with adjacent runs of one style on a row merged into one, and
    /// empty runs dropped: what shows, not how it was split. Line breaks are
    /// no longer part of any run, so [`Frame::to_ansi`] of the result writes
    /// them unstyled.
    pub fn merged(&self) -> Frame {
        let mut frame = Frame {
            runs: Vec::with_capacity(self.runs.len()),
            text: String::with_capacity(self.text.len()),
            styles: self.styles.clone(),
            trailing_newline: self.trailing_newline,
            ..Frame::default()
        };
        for row in 0..self.height() {
            let start = frame.runs.len() as u32;
            for run in self.row(row).iter().filter(|run| run.len > 0) {
                let text = &self.text[run.range()];
                match frame.runs[start as usize..].last_mut() {
                    Some(last) if last.style == run.style => {
                        last.len += run.len;
                        last.cells += run.cells;
                        frame.text.push_str(text);
                    }
                    _ => frame.push(text, run.style, 0),
                }
            }
            frame.rows.push(start..frame.runs.len() as u32);
        }
        frame
    }

    /// The number of rows. A final line break does not start a new row.
    pub fn height(&self) -> usize {
        self.rows.len()
    }

    /// The runs of row `index`. Panics when `index >= height()`.
    pub fn row(&self, index: usize) -> &[Run] {
        let range = &self.rows[index];
        &self.runs[range.start as usize..range.end as usize]
    }

    /// The width of row `index` in cells.
    pub fn row_width(&self, index: usize) -> usize {
        self.row(index).iter().map(Run::cells).sum()
    }

    /// The widest row, in cells.
    pub fn width(&self) -> usize {
        (0..self.height())
            .map(|row| self.row_width(row))
            .max()
            .unwrap_or(0)
    }

    /// The text of `run`.
    pub fn run_text(&self, run: &Run) -> &str {
        &self.text[run.range()]
    }

    pub fn styles(&self) -> &StyleTable {
        &self.styles
    }

    /// Whether the stream ended with a line break.
    pub fn ends_with_newline(&self) -> bool {
        self.trailing_newline
    }

    /// The total number of runs.
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// Plain text: rows joined by line breaks, without styles.
    pub fn plain(&self) -> String {
        let mut out = String::with_capacity(self.text.len() + self.height());
        for row in 0..self.height() {
            if row > 0 {
                out.push('\n');
            }
            for run in self.row(row) {
                out.push_str(self.run_text(run));
            }
        }
        if self.trailing_newline {
            out.push('\n');
        }
        out
    }

    /// Encode as `console` would: the same bytes as
    /// `console.segments_to_string` on the stream this frame was built from,
    /// less its control segments.
    pub fn to_ansi(&self, console: &Console) -> String {
        self.encode(console.color_system(), console.no_color())
    }

    /// Encode with an explicit colour system. `no_color` removes colours but
    /// keeps other attributes, as `Console` does when `no_color` is set.
    pub fn encode(&self, system: Option<ColorSystem>, no_color: bool) -> String {
        let styles = self.styles_for(system, no_color);
        let mut out = String::with_capacity(self.text.len() * 2);
        // A segment's pieces render as one: its line breaks sit inside the
        // same SGR pair, as upstream's `_render_buffer` writes them.
        let mut piece = String::new();
        let mut style = StyleId::NONE;
        for row in 0..self.height() {
            let runs = self.row(row);
            for run in runs {
                if run.flags & JOIN == 0 {
                    flush(&mut out, &piece, styles[style.index()].as_ref(), system);
                    piece.clear();
                    style = run.style;
                }
                piece.push_str(&self.text[run.range()]);
                if run.flags & NEWLINE != 0 {
                    piece.push('\n');
                }
            }
            // A merged frame's line breaks belong to no run: write them bare.
            let breaks = row + 1 < self.height() || self.trailing_newline;
            if breaks && runs.last().is_none_or(|run| run.flags & NEWLINE == 0) {
                flush(&mut out, &piece, styles[style.index()].as_ref(), system);
                piece.clear();
                out.push('\n');
            }
        }
        flush(&mut out, &piece, styles[style.index()].as_ref(), system);
        out
    }

    /// Encode after merging adjacent runs of one style, with line breaks
    /// unstyled. Fewer bytes than [`Frame::to_ansi`], but not upstream's.
    pub fn to_ansi_merged(&self, console: &Console) -> String {
        self.merged()
            .encode(console.color_system(), console.no_color())
    }

    /// Encode the runs covering `columns` of row `row` (clipped to the row),
    /// merging adjacent runs of one style. A run that starts before the range
    /// is cut at the grapheme boundary; a wide grapheme cut in half becomes a
    /// space. This is what a cell-level repaint writes for one [`Change`].
    pub fn encode_span(
        &self,
        row: usize,
        columns: Range<usize>,
        system: Option<ColorSystem>,
        no_color: bool,
    ) -> String {
        let styles = self.styles_for(system, no_color);
        let mut out = String::new();
        let mut piece = String::new();
        let mut style = None;
        for cell in self
            .cells(row)
            .into_iter()
            .skip(columns.start)
            .take(columns.len())
        {
            if style != Some(cell.style) {
                if let Some(id) = style {
                    flush(
                        &mut out,
                        &piece,
                        styles[StyleId::index(id)].as_ref(),
                        system,
                    );
                }
                piece.clear();
                style = Some(cell.style);
            }
            if cell.is_continuation() {
                if piece.is_empty() {
                    piece.push(' ');
                }
                continue;
            }
            piece.push_str(cell.text);
        }
        if let Some(id) = style {
            flush(&mut out, &piece, styles[id.index()].as_ref(), system);
        }
        out
    }

    fn styles_for(&self, system: Option<ColorSystem>, no_color: bool) -> Vec<Option<Style>> {
        if no_color && system.is_some() {
            self.styles
                .styles
                .iter()
                .map(|style| style.as_ref().map(Style::without_color))
                .collect()
        } else {
            self.styles.styles.clone()
        }
    }

    /// One cell per grapheme of row `index`, using core's widths. A wide
    /// grapheme is followed by continuation cells.
    pub fn cells(&self, index: usize) -> Vec<Cell<'_>> {
        let row = self.row(index);
        let mut cells = Vec::with_capacity(row.iter().map(Run::cells).sum());
        for run in row {
            let text = self.run_text(run);
            let (graphemes, _) = split_graphemes(text);
            for (start, end, width) in graphemes {
                cells.push(Cell {
                    text: &text[start..end],
                    width: width as u8,
                    style: run.style,
                });
                for _ in 1..width {
                    cells.push(Cell {
                        text: "",
                        width: 0,
                        style: run.style,
                    });
                }
            }
        }
        cells
    }

    /// The cells that differ from `previous`, as column ranges per row, in
    /// row order. Styles compare by value, so the two frames need not share a
    /// style table. A row present in only one frame changes across its width.
    pub fn diff(&self, previous: &Frame) -> Vec<Change> {
        let mut changes = Vec::new();
        for row in 0..self.height().max(previous.height()) {
            let new = if row < self.height() {
                self.cells(row)
            } else {
                Vec::new()
            };
            let old = if row < previous.height() {
                previous.cells(row)
            } else {
                Vec::new()
            };
            let width = new.len().max(old.len());
            let mut open = None;
            for column in 0..width {
                let same = match (new.get(column), old.get(column)) {
                    (Some(a), Some(b)) => {
                        a.text == b.text
                            && a.width == b.width
                            && self.styles.get(a.style) == previous.styles.get(b.style)
                    }
                    _ => false,
                };
                match (same, open) {
                    (false, None) => open = Some(column),
                    (true, Some(start)) => {
                        changes.push(Change {
                            row,
                            columns: start..column,
                        });
                        open = None;
                    }
                    _ => {}
                }
            }
            if let Some(start) = open {
                changes.push(Change {
                    row,
                    columns: start..width,
                });
            }
        }
        changes
    }
}

fn flush(out: &mut String, text: &str, style: Option<&Style>, system: Option<ColorSystem>) {
    match (style, system) {
        (Some(style), Some(system)) => out.push_str(&style.render(text, Some(system))),
        _ => out.push_str(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, style: Option<&str>) -> Segment {
        Segment::new(text, style.map(|s| Style::parse(s).unwrap()))
    }

    fn console(system: Option<ColorSystem>, no_color: bool) -> Console {
        Console::builder()
            .width(40)
            .force_terminal(true)
            .color_system(system)
            .no_color(no_color)
            .build()
    }

    #[test]
    fn exact_encoding_keeps_line_breaks_inside_a_segment() {
        let segments = vec![
            seg("a\nb", Some("bold")),
            seg("c", None),
            seg("\n", None),
            seg("d\n", Some("red link https://x.test")),
        ];
        let console = console(Some(ColorSystem::Truecolor), false);
        let frame = Frame::from_segments(&segments);
        assert_eq!(frame.height(), 3);
        assert!(frame.ends_with_newline());
        assert_eq!(frame.plain(), "a\nbc\nd\n");
        assert_eq!(
            frame.to_ansi(&console),
            console.segments_to_string(&segments)
        );
    }

    #[test]
    fn control_segments_are_dropped() {
        let segments = vec![seg("x", None), Segment::control("\x1b[2K"), seg("y", None)];
        let frame = Frame::from_segments(&segments);
        assert_eq!(frame.plain(), "xy");
        assert_eq!(frame.run_count(), 2);
    }

    #[test]
    fn styles_are_interned_once() {
        let segments = vec![
            seg("a", Some("bold")),
            seg("b", Some("italic")),
            seg("c", Some("bold")),
            seg("d", None),
        ];
        let frame = Frame::from_segments(&segments);
        assert_eq!(frame.styles().len(), 3);
        let row = frame.row(0);
        assert_eq!(row[0].style(), row[2].style());
        assert_eq!(row[3].style(), StyleId::NONE);
    }

    #[test]
    fn no_color_keeps_attributes() {
        let segments = vec![seg("hot", Some("bold red on blue"))];
        let console = console(Some(ColorSystem::Standard), true);
        let frame = Frame::from_segments(&segments);
        assert_eq!(
            frame.to_ansi(&console),
            console.segments_to_string(&segments)
        );
        assert_eq!(frame.to_ansi(&console), "\x1b[1mhot\x1b[0m");
    }

    #[test]
    fn merging_joins_runs_of_one_style() {
        let segments = vec![
            seg("ab", Some("bold")),
            seg("cd", Some("bold")),
            seg("", Some("red")),
            seg("\n", None),
            seg("e", None),
        ];
        let console = console(Some(ColorSystem::Truecolor), false);
        let frame = Frame::from_segments(&segments);
        let merged = frame.merged();
        assert_eq!(merged.row(0).len(), 1);
        assert_eq!(frame.to_ansi_merged(&console), "\x1b[1mabcd\x1b[0m\ne");
        assert_eq!(merged.plain(), frame.plain());
    }

    #[test]
    fn cells_follow_core_widths() {
        let frame = Frame::from_segments(&[seg("a漢b", None)]);
        let cells = frame.cells(0);
        let widths: Vec<u8> = cells.iter().map(|cell| cell.width).collect();
        assert_eq!(widths, [1, 2, 0, 1]);
        assert!(cells[2].is_continuation());
        assert_eq!(frame.width(), 4);
    }

    #[test]
    fn diff_reports_changed_cells() {
        let before = Frame::from_segments(&[seg("hello\n", None), seg("world", None)]);
        let after = Frame::from_segments(&[
            seg("hel", None),
            seg("l", Some("bold")),
            seg("o\n", None),
            seg("world!", None),
            seg("\nnew", None),
        ]);
        assert_eq!(
            after.diff(&before),
            [
                Change {
                    row: 0,
                    columns: 3..4
                },
                Change {
                    row: 1,
                    columns: 5..6
                },
                Change {
                    row: 2,
                    columns: 0..3
                },
            ]
        );
        assert!(before.diff(&before).is_empty());
        // Resegmented but identical output has no changes.
        let split = Frame::from_segments(&[seg("hel", None), seg("lo\nworld", None)]);
        assert!(split.diff(&before).is_empty());
    }

    #[test]
    fn encode_span_repaints_one_change() {
        let frame = Frame::from_segments(&[seg("ab", None), seg("cd", Some("bold"))]);
        let span = frame.encode_span(0, 1..3, Some(ColorSystem::Standard), false);
        assert_eq!(span, "b\x1b[1mc\x1b[0m");
        let wide = Frame::from_segments(&[seg("漢x", None)]);
        assert_eq!(wide.encode_span(0, 1..3, None, false), " x");
    }
}
