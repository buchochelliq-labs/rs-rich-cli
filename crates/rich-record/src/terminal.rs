//! The VT emulator, with emoji sequences kept in one cell.
//!
//! vt100 gives every character its own width, so a ZWJ sequence such as 👩‍👧
//! (woman, ZWJ, girl) takes four cells where rich, and the terminals it
//! targets, take two, and every column after it shifts. Skin-tone modifiers
//! split the same way. [`Terminal`] sits in front of the parser and joins
//! them: the first emoji is printed as usual, and each later part is replaced
//! by one zero-width marker character, which vt100 appends to the same cell.
//! The marker names a slot holding the whole sequence, and cells and screen
//! text are read back through it.

use std::borrow::Cow;

use unicode_width::UnicodeWidthChar;

use crate::screen::{Snapshot, Theme};

const ZWJ: char = '\u{200d}';
/// Markers: unassigned code points in the default-ignorable block
/// U+E0000–U+E0FFF, which vt100 treats as zero width. Tags (U+E0020–E007F)
/// and variation selectors (U+E0100–E01EF) are real text and stay clear.
const MARKER_FIRST: u32 = 0xE0200;
const MARKERS: usize = 0xE0FFF - 0xE0200 + 1;

/// Whether `c` is an emoji that can start or continue a sequence.
fn pictographic(c: char) -> bool {
    c.width() == Some(2)
        || matches!(
            c as u32,
            0x00A9 | 0x00AE | 0x203C | 0x2049 | 0x2122 | 0x2139 | 0x2194..=0x21AA
                | 0x231A..=0x23FF | 0x24C2 | 0x25AA..=0x25FE | 0x2600..=0x27BF
                | 0x2934 | 0x2935 | 0x2B05..=0x2B55 | 0x3030 | 0x303D | 0x3297
                | 0x3299 | 0x1F000..=0x1FAFF
        )
}

/// Skin-tone modifiers, which join the emoji before them.
fn modifier(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

/// Zero-width characters that belong to the emoji before them: variation
/// selectors, the keycap mark and tags.
fn extends(c: char) -> bool {
    matches!(c as u32, 0xFE00..=0xFE0F | 0x20E3 | 0xE0020..=0xE007F)
}

fn marker(c: char) -> Option<usize> {
    let index = (c as u32).checked_sub(MARKER_FIRST)? as usize;
    (index < MARKERS).then_some(index)
}

/// One joined sequence: the text vt100 printed before the first marker, and
/// the whole sequence.
#[derive(Clone, Debug, Default)]
struct Slot {
    prefix: String,
    full: String,
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

#[derive(Debug)]
struct Joiner {
    mode: Mode,
    /// Bytes of a character split across reads.
    partial: Vec<u8>,
    /// The emoji sequence in the cell just printed, while it can grow.
    cluster: Option<String>,
    /// The slot that cluster was given once it was joined.
    slot: Option<usize>,
    /// A ZWJ held back until the next character shows whether it joins.
    zwj: bool,
    slots: Vec<Slot>,
    next: usize,
}

impl Joiner {
    fn new() -> Joiner {
        Joiner {
            mode: Mode::Ground,
            partial: Vec::new(),
            cluster: None,
            slot: None,
            zwj: false,
            slots: Vec::new(),
            next: 0,
        }
    }

    fn filter(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len() + 8);
        for &byte in bytes {
            match self.mode {
                Mode::Ground => self.ground(byte, &mut out),
                Mode::Escape => {
                    self.mode = match byte {
                        b'[' => Mode::Csi,
                        b']' | b'P' | b'X' | b'^' | b'_' => Mode::Text,
                        _ => Mode::Ground,
                    };
                    out.push(byte);
                }
                Mode::Csi => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.mode = Mode::Ground;
                    }
                    out.push(byte);
                }
                Mode::Text => {
                    match byte {
                        0x07 => self.mode = Mode::Ground,
                        0x1b => self.mode = Mode::TextEscape,
                        _ => {}
                    }
                    out.push(byte);
                }
                Mode::TextEscape => {
                    self.mode = if byte == b'\\' {
                        Mode::Ground
                    } else {
                        Mode::Text
                    };
                    out.push(byte);
                }
            }
        }
        out
    }

    fn ground(&mut self, byte: u8, out: &mut Vec<u8>) {
        if self.partial.is_empty() && byte < 0x80 {
            // Controls and escapes move the cursor or print elsewhere:
            // nothing after them joins the cell before.
            self.end(out);
            if byte == 0x1b {
                self.mode = Mode::Escape;
            }
            out.push(byte);
            return;
        }
        self.partial.push(byte);
        match std::str::from_utf8(&self.partial) {
            Ok(text) => {
                let c = text.chars().next().expect("one character");
                self.partial.clear();
                self.char(c, out);
            }
            Err(e) if e.error_len().is_none() && self.partial.len() < 4 => {}
            Err(_) => {
                self.end(out);
                out.append(&mut self.partial);
            }
        }
    }

    /// End the current sequence, printing a held ZWJ.
    fn end(&mut self, out: &mut Vec<u8>) {
        if std::mem::take(&mut self.zwj) {
            push(out, ZWJ);
        }
        self.cluster = None;
        self.slot = None;
    }

    fn char(&mut self, c: char, out: &mut Vec<u8>) {
        if self.zwj {
            self.zwj = false;
            if pictographic(c) {
                self.join(&[ZWJ, c], out);
                return;
            }
            push(out, ZWJ);
            self.cluster = None;
            self.slot = None;
        }
        if self.cluster.is_some() {
            if c == ZWJ {
                self.zwj = true;
                return;
            }
            if modifier(c) {
                self.join(&[c], out);
                return;
            }
            if extends(c) {
                if self.slot.is_some() {
                    self.join(&[c], out);
                } else {
                    self.cluster.as_mut().expect("cluster").push(c);
                    push(out, c);
                }
                return;
            }
        }
        push(out, c);
        self.slot = None;
        self.cluster = pictographic(c).then(|| c.to_string());
    }

    /// Add `parts` to the current sequence, in its slot.
    fn join(&mut self, parts: &[char], out: &mut Vec<u8>) {
        let cluster = self.cluster.as_mut().expect("cluster");
        let prefix = cluster.clone();
        cluster.extend(parts);
        let full = cluster.clone();
        match self.slot {
            Some(slot) => self.slots[slot].full = full,
            None => {
                // Slots are reused oldest first once all are taken; by then
                // the cell that used one is long gone.
                let slot = self.next;
                self.next = (self.next + 1) % MARKERS;
                let entry = Slot { prefix, full };
                if slot < self.slots.len() {
                    self.slots[slot] = entry;
                } else {
                    self.slots.push(entry);
                }
                self.slot = Some(slot);
                push(
                    out,
                    char::from_u32(MARKER_FIRST + slot as u32).expect("marker"),
                );
            }
        }
    }

    /// The text of one cell: a joined sequence in full.
    fn cell<'a>(&self, text: &'a str) -> Cow<'a, str> {
        match text.chars().find_map(marker) {
            Some(slot) => match self.slots.get(slot) {
                Some(entry) => Cow::Owned(entry.full.clone()),
                None => Cow::Borrowed(text),
            },
            None => Cow::Borrowed(text),
        }
    }

    /// Screen text with each marker replaced by the rest of its sequence.
    fn text(&self, text: String) -> String {
        if !text.chars().any(|c| marker(c).is_some()) {
            return text;
        }
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            match marker(c).and_then(|slot| self.slots.get(slot)) {
                Some(entry) => out.push_str(entry.full.get(entry.prefix.len()..).unwrap_or("")),
                None if marker(c).is_some() => {}
                None => out.push(c),
            }
        }
        out
    }
}

fn push(out: &mut Vec<u8>, c: char) {
    let mut buffer = [0; 4];
    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
}

/// A vt100 parser that keeps emoji sequences in one cell.
pub struct Terminal {
    parser: vt100::Parser,
    joiner: Joiner,
}

impl Terminal {
    pub fn new(rows: u16, columns: u16) -> Terminal {
        Terminal {
            parser: vt100::Parser::new(rows, columns, 0),
            joiner: Joiner::new(),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        let bytes = self.joiner.filter(bytes);
        self.parser.process(&bytes);
    }

    pub fn set_size(&mut self, rows: u16, columns: u16) {
        self.parser.screen_mut().set_size(rows, columns);
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// The screen's text, rows joined by line breaks.
    pub fn contents(&self) -> String {
        self.joiner.text(self.parser.screen().contents())
    }

    pub fn snapshot(&self, theme: &Theme) -> Snapshot {
        Snapshot::read(self.parser.screen(), theme, |text| self.joiner.cell(text))
    }
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

    #[test]
    fn markers_are_zero_width() {
        for index in [0, MARKERS - 1] {
            let c = char::from_u32(MARKER_FIRST + index as u32).unwrap();
            assert_eq!(c.width(), Some(0));
        }
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
    fn long_sequences_and_modifiers_join() {
        let kiss = "👩🏽\u{200d}❤\u{fe0f}\u{200d}💋\u{200d}👨🏿";
        let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        let (terminal, snapshot) = screen(&format!("{kiss}|{family}|👍🏽|❤\u{fe0f}!"), 20);
        let row = &snapshot.rows[0];
        let cells: Vec<&str> = row
            .iter()
            .filter(|cell| !cell.is_continuation())
            .map(|cell| cell.text.as_str())
            .take(8)
            .collect();
        assert_eq!(cells, [kiss, "|", family, "|", "👍🏽", "|", "❤\u{fe0f}", "!"]);
        assert!(terminal
            .contents()
            .starts_with(&format!("{kiss}|{family}|👍🏽|❤\u{fe0f}!")));
    }

    #[test]
    fn text_zwj_and_escapes_pass_through() {
        // A ZWJ between letters stays, and a sequence split by a cursor move
        // does not join.
        let (_, snapshot) = screen("a\u{200d}b👩\x1b[C\u{200d}👧\x1b]0;t👩\u{200d}👧\x07", 10);
        let row = &snapshot.rows[0];
        assert_eq!(row[0].text, "a\u{200d}");
        assert_eq!(row[1].text, "b");
        assert_eq!(row[2].text, "👩");
        assert_eq!(row[5].text, "👧");
        assert_eq!(row[5].width, 2);
    }

    #[test]
    fn characters_split_across_reads() {
        let bytes = "x👩\u{200d}👧y".as_bytes();
        let mut terminal = Terminal::new(1, 10);
        for byte in bytes {
            terminal.process(std::slice::from_ref(byte));
        }
        assert_eq!(terminal.contents(), "x👩\u{200d}👧y");
        assert_eq!(terminal.snapshot(&Theme::default()).rows[0][3].text, "y");
    }
}
