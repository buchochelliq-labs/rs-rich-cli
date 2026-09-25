//! A prototype frame: rows of cells, each cell one grapheme with an interned
//! style. Built from the segment stream core already produces, and encoded
//! back to the same ANSI a merged segment stream gives.

use std::collections::HashMap;

use rich::cells::split_graphemes;
use rich::segment::Segment;
use rich::{ColorSystem, Style};

/// Style id 0 is "no style" (`Segment::style == None`).
pub const NO_STYLE: u32 = 0;

/// Styles, stored once each. A cell holds a `u32` into this table.
#[derive(Default)]
pub struct StyleTable {
    styles: Vec<Option<Style>>,
    index: HashMap<String, u32>,
    /// The last style interned: consecutive segments usually share one.
    last: Option<(Style, u32)>,
}

impl StyleTable {
    pub fn new() -> Self {
        StyleTable {
            styles: vec![None],
            ..Default::default()
        }
    }

    pub fn intern(&mut self, style: Option<&Style>) -> u32 {
        let Some(style) = style else {
            return NO_STYLE;
        };
        if let Some((last, id)) = &self.last {
            if last == style {
                return *id;
            }
        }
        // `Style` has no `Hash`; its `Debug` text is a stand-in key. A real
        // design would derive `Hash` (or intern at parse time).
        let key = format!("{style:?}");
        let id = match self.index.get(&key) {
            Some(id) => *id,
            None => {
                let id = self.styles.len() as u32;
                self.styles.push(Some(style.clone()));
                self.index.insert(key, id);
                id
            }
        };
        self.last = Some((style.clone(), id));
        id
    }

    pub fn get(&self, id: u32) -> Option<&Style> {
        self.styles[id as usize].as_ref()
    }

    pub fn len(&self) -> usize {
        self.styles.len()
    }
}

/// One terminal cell: a grapheme (a slice of the frame's text arena), how
/// many cells it covers, and its style. The second cell of a wide grapheme is
/// a continuation with width 0 and no text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub text: u32,
    pub len: u16,
    pub width: u8,
    pub style: u32,
}

impl Cell {
    pub fn is_continuation(&self) -> bool {
        self.width == 0 && self.len == 0
    }
}

/// Rows of cells. Rows are ragged: a row holds only the cells the
/// renderable produced, as the segment stream does.
pub struct Frame {
    rows: Vec<(u32, u32)>,
    cells: Vec<Cell>,
    text: String,
    pub styles: StyleTable,
}

impl Frame {
    /// Build a frame from a cropped segment stream (what `Console::print`
    /// writes). Control segments are dropped: a frame holds content, and
    /// cursor movement belongs to whatever paints it.
    pub fn from_segments(segments: &[Segment]) -> Frame {
        let mut frame = Frame {
            rows: Vec::new(),
            cells: Vec::with_capacity(segments.len() * 4),
            text: String::with_capacity(segments.iter().map(|s| s.text.len()).sum()),
            styles: StyleTable::new(),
        };
        let mut row_start = 0u32;
        for segment in segments {
            if segment.control {
                continue;
            }
            let style = frame.styles.intern(segment.style.as_ref());
            let mut rest = segment.text.as_str();
            loop {
                let (piece, newline) = match rest.find('\n') {
                    Some(at) => (&rest[..at], Some(at)),
                    None => (rest, None),
                };
                frame.push_text(piece, style);
                match newline {
                    Some(at) => {
                        let end = frame.cells.len() as u32;
                        frame.rows.push((row_start, end - row_start));
                        row_start = end;
                        rest = &rest[at + 1..];
                    }
                    None => break,
                }
            }
        }
        let end = frame.cells.len() as u32;
        if end > row_start {
            frame.rows.push((row_start, end - row_start));
        }
        frame
    }

    fn push_text(&mut self, text: &str, style: u32) {
        if text.is_empty() {
            return;
        }
        // One cell per grapheme, with core's own width rules.
        let (graphemes, _) = split_graphemes(text);
        for (start, end, width) in graphemes {
            let offset = self.text.len() as u32;
            self.text.push_str(&text[start..end]);
            self.cells.push(Cell {
                text: offset,
                len: (end - start) as u16,
                width: width as u8,
                style,
            });
            for _ in 1..width {
                self.cells.push(Cell {
                    text: offset,
                    len: 0,
                    width: 0,
                    style,
                });
            }
        }
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, index: usize) -> &[Cell] {
        let (start, len) = self.rows[index];
        &self.cells[start as usize..(start + len) as usize]
    }

    pub fn cell_text(&self, cell: &Cell) -> &str {
        &self.text[cell.text as usize..(cell.text as usize + cell.len as usize)]
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Encode as ANSI: each run of one style becomes one `Style::render`,
    /// as a merged (`Segment::simplify`) stream renders.
    pub fn to_ansi(&self, system: Option<ColorSystem>) -> String {
        let mut out = String::with_capacity(self.text.len() * 2);
        for row in 0..self.height() {
            if row > 0 {
                out.push('\n');
            }
            self.encode_cells(self.row(row), system, &mut out);
        }
        out
    }

    fn encode_cells(&self, cells: &[Cell], system: Option<ColorSystem>, out: &mut String) {
        let mut run = String::new();
        let mut run_style = None;
        for cell in cells.iter().filter(|c| !c.is_continuation()) {
            if run_style != Some(cell.style) && !run.is_empty() {
                self.flush(&run, run_style.unwrap(), system, out);
                run.clear();
            }
            run_style = Some(cell.style);
            run.push_str(self.cell_text(cell));
        }
        if let Some(style) = run_style.filter(|_| !run.is_empty()) {
            self.flush(&run, style, system, out);
        }
    }

    fn flush(&self, text: &str, style: u32, system: Option<ColorSystem>, out: &mut String) {
        match (self.styles.get(style), system) {
            (Some(style), Some(system)) => out.push_str(&style.render(text, Some(system))),
            _ => out.push_str(text),
        }
    }

    /// Plain text, one line per row: `export_text` without re-splitting.
    pub fn to_plain(&self) -> String {
        let mut out = String::with_capacity(self.text.len() + self.height());
        for row in 0..self.height() {
            if row > 0 {
                out.push('\n');
            }
            for cell in self.row(row).iter().filter(|c| !c.is_continuation()) {
                out.push_str(self.cell_text(cell));
            }
        }
        out
    }

    /// Cells that differ from `previous`, as `(row, first, last)` column runs:
    /// what a cell-level repaint would rewrite.
    pub fn diff(&self, previous: &Frame) -> Vec<(usize, usize, usize)> {
        let mut runs = Vec::new();
        for row in 0..self.height().max(previous.height()) {
            let new = if row < self.height() {
                self.row(row)
            } else {
                &[]
            };
            let old = if row < previous.height() {
                previous.row(row)
            } else {
                &[]
            };
            let mut open: Option<usize> = None;
            for col in 0..new.len().max(old.len()) {
                let same = match (new.get(col), old.get(col)) {
                    (Some(a), Some(b)) => {
                        self.cell_text(a) == previous.cell_text(b)
                            && a.width == b.width
                            && self.styles.get(a.style) == previous.styles.get(b.style)
                    }
                    _ => false,
                };
                match (same, open) {
                    (false, None) => open = Some(col),
                    (true, Some(start)) => {
                        runs.push((row, start, col - 1));
                        open = None;
                    }
                    _ => {}
                }
            }
            if let Some(start) = open {
                runs.push((row, start, new.len().max(old.len()) - 1));
            }
        }
        runs
    }

    /// The bytes a cell-level repaint of `runs` writes: a cursor move to each
    /// run, then its cells.
    pub fn repaint_bytes(
        &self,
        runs: &[(usize, usize, usize)],
        system: Option<ColorSystem>,
    ) -> usize {
        let mut out = String::new();
        for &(row, first, last) in runs {
            out.push_str(&format!("\x1b[{};{}H", row + 1, first + 1));
            let cells = self.row(row);
            let end = (last + 1).min(cells.len());
            if first < end {
                self.encode_cells(&cells[first..end], system, &mut out);
            }
        }
        out.len()
    }
}

/// The middle option: rows of styled runs over one text arena, with interned
/// styles. What a merged segment stream is, without per-segment `String`s and
/// `Style` clones. Cells are derived on demand.
pub struct Runs {
    rows: Vec<(u32, u32)>,
    runs: Vec<Run>,
    text: String,
    pub styles: StyleTable,
}

#[derive(Clone, Copy, Debug)]
pub struct Run {
    pub text: u32,
    pub len: u32,
    pub cells: u32,
    pub style: u32,
}

impl Runs {
    /// Merge adjacent runs of one style (fewer SGR codes, but not the bytes
    /// upstream writes).
    pub fn from_segments(segments: &[Segment]) -> Runs {
        Runs::build(segments, true)
    }

    /// One run per segment piece: encodes to exactly what
    /// `Console::segments_to_string` writes today.
    pub fn from_segments_exact(segments: &[Segment]) -> Runs {
        Runs::build(segments, false)
    }

    fn build(segments: &[Segment], merge: bool) -> Runs {
        let mut runs = Runs {
            rows: Vec::new(),
            runs: Vec::with_capacity(segments.len()),
            text: String::with_capacity(segments.iter().map(|s| s.text.len()).sum()),
            styles: StyleTable::new(),
        };
        let mut row_start = 0u32;
        for segment in segments {
            if segment.control {
                continue;
            }
            let style = runs.styles.intern(segment.style.as_ref());
            let mut rest = segment.text.as_str();
            loop {
                let (piece, newline) = match rest.find('\n') {
                    Some(at) => (&rest[..at], Some(at)),
                    None => (rest, None),
                };
                runs.push(piece, style, row_start, merge);
                match newline {
                    Some(at) => {
                        let end = runs.runs.len() as u32;
                        runs.rows.push((row_start, end - row_start));
                        row_start = end;
                        rest = &rest[at + 1..];
                    }
                    None => break,
                }
            }
        }
        let end = runs.runs.len() as u32;
        if end > row_start {
            runs.rows.push((row_start, end - row_start));
        }
        runs
    }

    fn push(&mut self, text: &str, style: u32, row_start: u32, merge: bool) {
        if text.is_empty() {
            return;
        }
        let cells = rich::cells::cell_len(text) as u32;
        // Merge into the row's last run when the style matches.
        if merge && self.runs.len() as u32 > row_start {
            if let Some(last) = self.runs.last_mut() {
                if last.style == style && (last.text + last.len) as usize == self.text.len() {
                    self.text.push_str(text);
                    last.len += text.len() as u32;
                    last.cells += cells;
                    return;
                }
            }
        }
        let offset = self.text.len() as u32;
        self.text.push_str(text);
        self.runs.push(Run {
            text: offset,
            len: text.len() as u32,
            cells,
            style,
        });
    }

    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    pub fn to_ansi(&self, system: Option<ColorSystem>) -> String {
        let mut out = String::with_capacity(self.text.len() * 2);
        for (index, (start, len)) in self.rows.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            for run in &self.runs[*start as usize..(*start + *len) as usize] {
                let text = &self.text[run.text as usize..(run.text + run.len) as usize];
                match (self.styles.get(run.style), system) {
                    (Some(style), Some(system)) => out.push_str(&style.render(text, Some(system))),
                    _ => out.push_str(text),
                }
            }
        }
        out
    }
}
