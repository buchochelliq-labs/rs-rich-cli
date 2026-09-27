//! The VT emulator, with every grapheme cluster as wide as rich measures it.
//!
//! vt100 gives each character its own width, so a ZWJ sequence such as 👩‍👧
//! (woman, ZWJ, girl) takes four cells, ❤️ (heart, VS16) one, and a flag's
//! two regional indicators land in separate cells, where rich
//! ([`rich::cells::cell_len`]), and the terminals it targets, give each of them
//! two. Every column after such a cluster would shift. [`Terminal`] sits in
//! front of the parser and keeps each cluster in one cell of rich's width:
//!
//! - zero-width parts that change nothing (a combining accent, a ZWJ) are
//!   passed through, and vt100 appends them to the cell;
//! - otherwise the cluster's cell is reprinted as its first character and one
//!   zero-width marker (an unassigned code point vt100 appends), followed by
//!   filler cells up to rich's width. The marker names the whole cluster,
//!   interned by its text, and cells and screen text are read back through it.
//!
//! The reprint is sent to the emulator only; recorded output is untouched.

use std::borrow::Cow;
use std::collections::HashMap;

use unicode_width::UnicodeWidthChar;

use crate::screen::{Snapshot, Theme};

const ZWJ: char = '\u{200d}';
/// Markers: unassigned code points in the default-ignorable block
/// U+E0000–U+E0FFF, which vt100 treats as zero width. Tags (U+E0020–E007F)
/// and variation selectors (U+E0100–E01EF) are real text and stay clear.
const MARKER_FIRST: u32 = 0xE0200;
const MARKERS: usize = 0xE0FFF - 0xE0200 + 1;
/// Pads a cluster to rich's width: a private-use character one cell wide,
/// read back as a continuation of the cluster's cell.
const FILLER: char = '\u{10fffd}';

fn marker(c: char) -> Option<usize> {
    let index = (c as u32).checked_sub(MARKER_FIRST)? as usize;
    (index < MARKERS).then_some(index)
}

fn modifier(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

fn regional(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Ground,
    Escape,
    Csi,
    /// OSC, DCS and the other string sequences, up to BEL or ST.
    Text,
    TextEscape,
}

/// The cluster in the cell just printed, while it can still grow.
#[derive(Debug)]
struct Cluster {
    text: String,
    /// Cells its first character took in vt100.
    native: usize,
    /// Cells it takes now: `native` plus fillers.
    cells: usize,
    /// Its row and first column, once it has been reprinted.
    at: Option<(u16, u16)>,
}

/// A vt100 parser that keeps each grapheme cluster in one cell of the width
/// rich gives it.
pub struct Terminal {
    parser: vt100::Parser,
    mode: Mode,
    /// Bytes of a character split across reads.
    partial: Vec<u8>,
    /// Bytes for the parser, not yet sent.
    out: Vec<u8>,
    cluster: Option<Cluster>,
    /// Interned clusters: a marker's index into this names its cluster.
    /// Never reused, so a cell still on screen keeps its text.
    clusters: Vec<String>,
    ids: HashMap<String, usize>,
    capacity: usize,
}

impl Terminal {
    pub fn new(rows: u16, columns: u16) -> Terminal {
        Terminal {
            parser: vt100::Parser::new(rows, columns, 0),
            mode: Mode::Ground,
            partial: Vec::new(),
            out: Vec::new(),
            cluster: None,
            clusters: Vec::new(),
            ids: HashMap::new(),
            capacity: MARKERS,
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match self.mode {
                Mode::Ground => self.ground(byte),
                Mode::Escape => {
                    self.mode = match byte {
                        b'[' => Mode::Csi,
                        b']' | b'P' | b'X' | b'^' | b'_' => Mode::Text,
                        _ => Mode::Ground,
                    };
                    self.out.push(byte);
                }
                Mode::Csi => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.mode = Mode::Ground;
                    }
                    self.out.push(byte);
                }
                Mode::Text => {
                    match byte {
                        0x07 => self.mode = Mode::Ground,
                        0x1b => self.mode = Mode::TextEscape,
                        _ => {}
                    }
                    self.out.push(byte);
                }
                Mode::TextEscape => {
                    self.mode = if byte == b'\\' {
                        Mode::Ground
                    } else {
                        Mode::Text
                    };
                    self.out.push(byte);
                }
            }
        }
        self.flush();
    }

    fn flush(&mut self) {
        if !self.out.is_empty() {
            self.parser.process(&self.out);
            self.out.clear();
        }
    }

    fn ground(&mut self, byte: u8) {
        if self.partial.is_empty() && byte < 0x80 && !(0x20..0x7f).contains(&byte) {
            // Controls and escapes move the cursor or print elsewhere:
            // nothing after them joins the cell before.
            self.cluster = None;
            if byte == 0x1b {
                self.mode = Mode::Escape;
            }
            self.out.push(byte);
            return;
        }
        self.partial.push(byte);
        match std::str::from_utf8(&self.partial) {
            Ok(text) => {
                let c = text.chars().next().expect("one character");
                self.partial.clear();
                self.char(c);
            }
            Err(e) if e.error_len().is_none() && self.partial.len() < 4 => {}
            Err(_) => {
                self.cluster = None;
                self.out.append(&mut self.partial);
            }
        }
    }

    fn char(&mut self, c: char) {
        let width = c.width();
        if let Some(cluster) = &self.cluster {
            let first = cluster.text.chars().next();
            let continues = width == Some(0)
                || cluster.text.ends_with(ZWJ)
                || modifier(c)
                || (regional(c)
                    && first.is_some_and(regional)
                    && cluster.text.chars().count() == 1);
            if continues && self.extend(c) {
                return;
            }
        }
        push(&mut self.out, c);
        self.cluster = match width {
            Some(native) if native > 0 => Some(Cluster {
                text: c.to_string(),
                native,
                cells: native,
                at: None,
            }),
            _ => None,
        };
    }

    /// Add `c` to the current cluster. False when it cannot be (the cursor
    /// moved, the line is too short, or the markers are all taken); the
    /// caller then prints `c` as vt100 would.
    fn extend(&mut self, c: char) -> bool {
        let cluster = self.cluster.as_ref().expect("cluster");
        let mut text = cluster.text.clone();
        text.push(c);
        let cells = rich::cells::cell_len(&text).max(cluster.native);
        if cluster.at.is_none() && c.width() == Some(0) && cells == cluster.cells {
            push(&mut self.out, c);
            self.cluster.as_mut().expect("cluster").text = text;
            return true;
        }
        self.flush();
        let (row, column) = self.parser.screen().cursor_position();
        let columns = self.parser.screen().size().1;
        let cluster = self.cluster.as_ref().expect("cluster");
        let start = match cluster.at {
            Some((at_row, start))
                if at_row == row && start as usize + cluster.cells == column as usize =>
            {
                start
            }
            Some(_) => {
                self.cluster = None;
                return false;
            }
            None => match column.checked_sub(cluster.cells as u16) {
                Some(start) => start,
                None => {
                    self.cluster = None;
                    return false;
                }
            },
        };
        if start as usize + cells > columns as usize {
            self.cluster = None;
            return false;
        }
        let Some(id) = self.intern(&text) else {
            self.cluster = None;
            return false;
        };
        let back = column - start;
        if back > 0 {
            self.out
                .extend_from_slice(format!("\x1b[{back}D").as_bytes());
        }
        let cluster = self.cluster.as_mut().expect("cluster");
        let first = cluster.text.chars().next().expect("a first character");
        push(&mut self.out, first);
        push(
            &mut self.out,
            char::from_u32(MARKER_FIRST + id as u32).expect("marker"),
        );
        for _ in cluster.native..cells {
            push(&mut self.out, FILLER);
        }
        cluster.text = text;
        cluster.cells = cells;
        cluster.at = Some((row, start));
        true
    }

    fn intern(&mut self, text: &str) -> Option<usize> {
        if let Some(&id) = self.ids.get(text) {
            return Some(id);
        }
        if self.clusters.len() >= self.capacity {
            return None;
        }
        self.clusters.push(text.to_string());
        self.ids.insert(text.to_string(), self.clusters.len() - 1);
        Some(self.clusters.len() - 1)
    }

    pub fn set_size(&mut self, rows: u16, columns: u16) {
        self.cluster = None;
        self.parser.screen_mut().set_size(rows, columns);
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// The screen's text, rows joined by line breaks.
    pub fn contents(&self) -> String {
        let text = self.parser.screen().contents();
        if !text.chars().any(|c| c == FILLER || marker(c).is_some()) {
            return text;
        }
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            match marker(c) {
                // The first character is already there, before the marker.
                Some(id) => out.extend(
                    self.clusters
                        .get(id)
                        .into_iter()
                        .flat_map(|s| s.chars().skip(1)),
                ),
                None if c == FILLER => {}
                None => out.push(c),
            }
        }
        out
    }

    pub fn snapshot(&self, theme: &Theme) -> Snapshot {
        let mut snapshot = Snapshot::read(self.parser.screen(), theme, Cow::Borrowed);
        for row in &mut snapshot.rows {
            let mut x = 0;
            while x < row.len() {
                if let Some(id) = row[x].text.chars().find_map(marker) {
                    row[x].text = self.clusters.get(id).cloned().unwrap_or_default();
                    let mut next = x + row[x].width.max(1) as usize;
                    while next < row.len() && row[next].text == FILLER.to_string() {
                        row[next].text.clear();
                        row[next].width = 0;
                        row[x].width += 1;
                        next += 1;
                    }
                    x = next;
                    continue;
                }
                if row[x].text.contains(FILLER) {
                    // Its cluster was overwritten: a blank cell.
                    row[x].text = " ".into();
                }
                x += 1;
            }
        }
        snapshot
    }
}

fn push(out: &mut Vec<u8>, c: char) {
    let mut buffer = [0; 4];
    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(bytes: &str, columns: u16) -> (Terminal, Snapshot) {
        let mut terminal = Terminal::new(2, columns);
        terminal.process(bytes.as_bytes());
        let snapshot = terminal.snapshot(&Theme::default());
        (terminal, snapshot)
    }

    /// The row's cells, without continuations.
    fn cells(snapshot: &Snapshot) -> Vec<(&str, u8)> {
        snapshot.rows[0]
            .iter()
            .filter(|cell| !cell.is_continuation())
            .map(|cell| (cell.text.as_str(), cell.width))
            .collect()
    }

    #[test]
    fn markers_and_filler_have_the_widths_they_need() {
        for index in [0, MARKERS - 1] {
            let c = char::from_u32(MARKER_FIRST + index as u32).unwrap();
            assert_eq!(c.width(), Some(0));
        }
        assert_eq!(FILLER.width(), Some(1));
    }

    #[test]
    fn a_zwj_sequence_takes_one_wide_cell() {
        let (terminal, snapshot) = screen("a👩\u{200d}👧b", 10);
        let row = &snapshot.rows[0];
        assert_eq!(row[1].text, "👩\u{200d}👧");
        assert_eq!(row[1].width, 2);
        assert!(row[2].is_continuation());
        assert_eq!(row[3].text, "b");
        assert_eq!(terminal.contents(), "a👩\u{200d}👧b");
    }

    #[test]
    fn every_cluster_takes_the_cells_rich_gives_it() {
        let clusters = [
            "👩\u{200d}👧",
            "👨\u{200d}👩\u{200d}👧\u{200d}👦",
            "👩🏽\u{200d}❤\u{fe0f}\u{200d}💋\u{200d}👨🏿",
            "👍🏽",
            "❤\u{fe0f}",
            "1\u{fe0f}\u{20e3}",
            "🇺🇸",
            "🏳\u{fe0f}\u{200d}🌈",
            "🏳\u{fe0f}\u{200d}⚧\u{fe0f}",
            "e\u{301}",
        ];
        for cluster in clusters {
            let (terminal, snapshot) = screen(&format!("{cluster}|"), 20);
            let width = rich::cells::cell_len(cluster);
            assert_eq!(
                cells(&snapshot)[..2],
                [(cluster, width as u8), ("|", 1)],
                "{cluster:?}"
            );
            assert_eq!(snapshot.rows[0][width].text, "|", "{cluster:?}");
            assert_eq!(terminal.contents(), format!("{cluster}|"));
        }
    }

    #[test]
    fn flags_pair_up() {
        let (_, snapshot) = screen("🇺🇸🇬🇧🇫", 20);
        assert_eq!(cells(&snapshot)[..3], [("🇺🇸", 2), ("🇬🇧", 2), ("🇫", 1)]);
    }

    #[test]
    fn text_zwj_and_escapes_pass_through() {
        // A sequence split by a cursor move does not join, and an OSC title
        // is left alone.
        let (_, snapshot) = screen("b👩\x1b[C\u{200d}👧\x1b]0;t👩\u{200d}👧\x07", 10);
        let row = &snapshot.rows[0];
        assert_eq!(row[0].text, "b");
        assert_eq!(row[1].text, "👩");
        assert_eq!(row[4].text, "👧");
        assert_eq!(row[4].width, 2);
    }

    #[test]
    fn clusters_are_interned_and_never_reused() {
        let mut terminal = Terminal::new(3, 20);
        terminal.process("👩\u{200d}👧\r\n👩\u{200d}👧".as_bytes());
        assert_eq!(terminal.clusters.len(), 1);
        // With every marker taken, a new cluster is printed as vt100 would,
        // and the cells already on screen keep their text.
        terminal.capacity = 1;
        terminal.process("\r\n👍🏽".as_bytes());
        let snapshot = terminal.snapshot(&Theme::default());
        assert_eq!(snapshot.rows[0][0].text, "👩\u{200d}👧");
        assert_eq!(snapshot.rows[2][0].text, "👍");
        assert_eq!(snapshot.rows[2][2].text, "🏽");
    }

    #[test]
    fn a_cluster_that_would_pass_the_margin_is_left_alone() {
        let (_, snapshot) = screen("abc❤\u{fe0f}", 4);
        assert_eq!(snapshot.rows[0][3].text, "❤\u{fe0f}");
        assert_eq!(snapshot.rows[0][3].width, 1);
    }

    #[test]
    fn characters_split_across_reads() {
        let bytes = "x👩\u{200d}👧❤\u{fe0f}y".as_bytes();
        let mut terminal = Terminal::new(1, 10);
        for byte in bytes {
            terminal.process(std::slice::from_ref(byte));
        }
        assert_eq!(terminal.contents(), "x👩\u{200d}👧❤\u{fe0f}y");
        assert_eq!(terminal.snapshot(&Theme::default()).rows[0][5].text, "y");
    }
}
