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
//!
//! It also follows what vt100 does not: the images a program draws (Kitty,
//! Sixel and iTerm2, see [`crate::graphics`]), shown in its snapshots; the
//! kitty keyboard protocol's flags the program pushes, pops and sets
//! (`CSI > u`, `CSI < u`, `CSI = u`), for each screen; and whether a
//! synchronized update (mode 2026) is open.

use std::borrow::Cow;
use std::collections::HashMap;

use unicode_width::UnicodeWidthChar;

use crate::graphics::Graphics;
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
/// The most characters a cluster is followed for. Real clusters are far
/// shorter; past this, characters go to vt100 as they are, so a flood of
/// combining marks costs linear time, not quadratic.
const MAX_CLUSTER_CHARS: usize = 32;
/// The most of a CSI sequence's parameter and intermediate bytes kept.
/// The queries answered are a few bytes long, so a longer sequence is no
/// query, and one that never ends costs no more.
const MAX_CSI: usize = 32;
/// The most bytes of answers kept for the program until they are taken.
/// Past it, more are dropped, as by a terminal whose reply queue is full:
/// a program that asks and asks, with nobody taking the answers, costs no
/// more.
const MAX_REPLIES: usize = 4096;
/// The most bytes of one string sequence (OSC, DCS, APC) kept: an image a
/// program sends whole fits; a longer one is dropped.
const MAX_STRING: usize = 32 << 20;
/// How deep each screen's stack of kitty keyboard flags goes; a push past
/// it drops the oldest.
const MAX_KEYBOARD_STACK: usize = 32;

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
    /// After ESC and intermediate bytes (0x20–0x2F), as in `ESC ( B`, up to
    /// the final byte.
    EscapeIntermediate,
    Csi,
    /// OSC, DCS, APC and the other string sequences, up to BEL or ST.
    Text,
    TextEscape,
}

/// The kitty keyboard protocol's flags: a stack for each screen, the top
/// the flags in force.
#[derive(Debug)]
struct Keyboard {
    main: Vec<u8>,
    alternate: Vec<u8>,
}

impl Default for Keyboard {
    fn default() -> Self {
        Keyboard {
            main: vec![0],
            alternate: vec![0],
        }
    }
}

/// The cluster in the cell just printed, while it can still grow.
#[derive(Debug)]
struct Cluster {
    text: String,
    /// Characters in `text`.
    chars: usize,
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
    /// Why vt100 panicked, once it has: the terminal takes no more input.
    broken: Option<String>,
    /// The CSI sequence being read, after `ESC [`.
    csi: Vec<u8>,
    /// Answers to the program's queries, not yet sent to it.
    replies: Vec<u8>,
    /// The string sequence being read: its introducer (`]`, `P`, `_`,
    /// `X`, `^`) and body, unless it grew past [`MAX_STRING`].
    string: Vec<u8>,
    string_kind: u8,
    string_dropped: bool,
    graphics: Graphics,
    keyboard: Keyboard,
    /// Whether the kitty keyboard protocol is reported to the program.
    kitty_keyboard: bool,
    /// Whether a synchronized update is open.
    synchronized: bool,
}

/// The message a caught panic carried.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic".into())
}

impl Terminal {
    pub fn new(rows: u16, columns: u16) -> Terminal {
        Terminal::with_scrollback(rows, columns, 0)
    }

    /// A terminal that keeps up to `scrollback` rows that scrolled off its
    /// top, for [`set_scrollback`](Self::set_scrollback) to show again.
    pub fn with_scrollback(rows: u16, columns: u16, scrollback: usize) -> Terminal {
        Terminal {
            parser: vt100::Parser::new(rows, columns, scrollback),
            mode: Mode::Ground,
            partial: Vec::new(),
            out: Vec::new(),
            cluster: None,
            clusters: Vec::new(),
            ids: HashMap::new(),
            capacity: MARKERS,
            broken: None,
            csi: Vec::new(),
            replies: Vec::new(),
            string: Vec::new(),
            string_kind: 0,
            string_dropped: false,
            graphics: Graphics::default(),
            keyboard: Keyboard::default(),
            kitty_keyboard: false,
            synchronized: false,
        }
    }

    /// Report the kitty keyboard protocol to the program: answer its query
    /// (`CSI ? u`) with the flags in force, so a program that asks turns
    /// the protocol on. Off by default: the query goes unanswered, as by a
    /// terminal without the protocol, and [`keyboard_flags`](Self::keyboard_flags)
    /// stays 0 whatever the program pushes.
    pub fn set_kitty_keyboard(&mut self, on: bool) {
        self.kitty_keyboard = on;
    }

    /// The kitty keyboard protocol's flags the program has in force on the
    /// screen it shows (1 disambiguate, 2 report releases and repeats, 4
    /// alternate keys, 8 every key as an escape, 16 associated text); 0
    /// unless [`set_kitty_keyboard`](Self::set_kitty_keyboard) is on.
    pub fn keyboard_flags(&self) -> u8 {
        if !self.kitty_keyboard {
            return 0;
        }
        self.stack().last().copied().unwrap_or(0)
    }

    fn stack(&self) -> &Vec<u8> {
        if self.parser.screen().alternate_screen() {
            &self.keyboard.alternate
        } else {
            &self.keyboard.main
        }
    }

    fn stack_mut(&mut self) -> &mut Vec<u8> {
        if self.parser.screen().alternate_screen() {
            &mut self.keyboard.alternate
        } else {
            &mut self.keyboard.main
        }
    }

    /// Whether the program has a synchronized update (mode 2026) open: the
    /// screen is between frames.
    pub fn in_synchronized_update(&self) -> bool {
        self.synchronized
    }

    /// Run `f`, turning a panic in vt100 into an error. vt100 panics on some
    /// input (a wide character in one column; a resize that cuts one at the
    /// right edge); after one its screen cannot be trusted, so every later
    /// call fails too.
    fn guard<T>(&mut self, what: &str, f: impl FnOnce(&mut Self) -> T) -> std::io::Result<T> {
        if let Some(message) = &self.broken {
            return Err(std::io::Error::other(message.clone()));
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self))) {
            Ok(value) => Ok(value),
            Err(payload) => {
                let message = format!(
                    "the terminal emulator failed while {what}: {}",
                    panic_message(payload.as_ref())
                );
                self.broken = Some(message.clone());
                Err(std::io::Error::other(message))
            }
        }
    }

    /// Why the emulator failed, if it has.
    pub fn error(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    /// Feed the program's output to the emulator. Fails when vt100 panics.
    pub fn process(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.guard("processing output", |terminal| terminal.feed(bytes))
    }

    fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match self.mode {
                Mode::Ground => self.ground(byte),
                Mode::Escape => {
                    self.mode = match byte {
                        b'[' => {
                            self.csi.clear();
                            Mode::Csi
                        }
                        b']' | b'P' | b'X' | b'^' | b'_' => {
                            // Held back until it ends: an image goes to
                            // the graphics, anything else on to vt100.
                            self.out.pop();
                            self.string.clear();
                            self.string_kind = byte;
                            self.string_dropped = false;
                            self.mode = Mode::Text;
                            continue;
                        }
                        0x20..=0x2f => Mode::EscapeIntermediate,
                        0x1b => Mode::Escape,
                        b'c' => {
                            // RIS: a full reset.
                            self.graphics.reset();
                            self.keyboard = Keyboard::default();
                            self.synchronized = false;
                            Mode::Ground
                        }
                        _ => Mode::Ground,
                    };
                    self.out.push(byte);
                }
                Mode::EscapeIntermediate => {
                    self.mode = match byte {
                        0x20..=0x2f => Mode::EscapeIntermediate,
                        0x1b => Mode::Escape,
                        // The final byte, or CAN and SUB, which cancel.
                        0x30..=0x7e | 0x18 | 0x1a => Mode::Ground,
                        // Other controls run inside the sequence.
                        _ => Mode::EscapeIntermediate,
                    };
                    self.out.push(byte);
                }
                Mode::Csi => {
                    match byte {
                        0x40..=0x7e => {
                            self.mode = Mode::Ground;
                            self.answer(byte);
                            self.csi.clear();
                        }
                        // As in vt100's parser: ESC starts another sequence,
                        // and CAN and SUB cancel this one.
                        0x1b => {
                            self.mode = Mode::Escape;
                            self.csi.clear();
                        }
                        0x18 | 0x1a => {
                            self.mode = Mode::Ground;
                            self.csi.clear();
                        }
                        _ if self.csi.len() < MAX_CSI => self.csi.push(byte),
                        // Too long for a query: what is kept matches none.
                        _ => {}
                    }
                    self.out.push(byte);
                }
                Mode::Text => match byte {
                    0x07 => self.end_string(b"\x07"),
                    0x1b => self.mode = Mode::TextEscape,
                    _ => self.keep(&[byte]),
                },
                Mode::TextEscape => {
                    if byte == b'\\' {
                        self.end_string(b"\x1b\\");
                    } else {
                        self.keep(&[0x1b, byte]);
                        self.mode = Mode::Text;
                    }
                }
            }
        }
        self.flush();
    }

    /// Keep `bytes` of the string sequence being read, unless it is too
    /// long, when it is dropped whole.
    fn keep(&mut self, bytes: &[u8]) {
        if self.string_dropped {
            return;
        }
        if self.string.len() + bytes.len() > MAX_STRING {
            self.string_dropped = true;
            self.string = Vec::new();
            return;
        }
        self.string.extend_from_slice(bytes);
    }

    /// The string sequence ended with `terminator`: an image is drawn, and
    /// anything else passed on to vt100 as it came.
    fn end_string(&mut self, terminator: &[u8]) {
        self.mode = Mode::Ground;
        self.cluster = None;
        if self.string_dropped {
            return;
        }
        let body = std::mem::take(&mut self.string);
        // The cursor where the image goes: vt100 has had all before it.
        self.flush();
        let screen = self.parser.screen();
        let handled = match self.string_kind {
            b'_' => self.graphics.apc(&body, screen),
            b'P' => self.graphics.dcs(&body, screen),
            b']' => self.graphics.osc(&body, screen),
            _ => None,
        };
        match handled {
            Some(Some(moved)) => {
                if moved.down > 0 {
                    self.out
                        .extend_from_slice(format!("\x1b[{}B", moved.down).as_bytes());
                }
                if moved.right > 0 {
                    self.out
                        .extend_from_slice(format!("\x1b[{}C", moved.right).as_bytes());
                }
            }
            Some(None) => {}
            None => {
                self.out.push(0x1b);
                self.out.push(self.string_kind);
                self.out.extend_from_slice(&body);
                self.out.extend_from_slice(terminator);
            }
        }
        // Reuse the buffer, unless an image made it large.
        if body.capacity() <= 64 * 1024 {
            self.string = body;
            self.string.clear();
        }
    }

    /// Follow the modes and flags of the CSI sequence `last` ends: the
    /// kitty keyboard protocol's stack, synchronized updates, and what
    /// removes placed images (clearing the screen, switching screens).
    fn follow(&mut self, last: u8) {
        let csi = std::mem::take(&mut self.csi);
        let number = |text: &[u8]| -> Option<u32> { std::str::from_utf8(text).ok()?.parse().ok() };
        match (last, csi.first()) {
            (b'u', Some(b'>')) => {
                // vt100 has had what came before: the screen is current.
                self.flush();
                let flags = number(&csi[1..]).unwrap_or(0) as u8;
                let stack = self.stack_mut();
                if stack.len() >= MAX_KEYBOARD_STACK {
                    stack.remove(1);
                }
                stack.push(flags & 31);
            }
            (b'u', Some(b'<')) => {
                self.flush();
                let count = number(&csi[1..]).unwrap_or(1).max(1) as usize;
                let stack = self.stack_mut();
                let keep = stack.len().saturating_sub(count).max(1);
                stack.truncate(keep);
                if keep == 1 && count >= 1 {
                    stack[0] = 0;
                }
            }
            (b'u', Some(b'=')) => {
                self.flush();
                let text = &csi[1..];
                let mut parts = text.split(|b| *b == b';');
                let flags = parts.next().and_then(number).unwrap_or(0) as u8 & 31;
                let mode = parts.next().and_then(number).unwrap_or(1);
                let top = self.stack_mut().last_mut().expect("a base entry");
                *top = match mode {
                    2 => *top | flags,
                    3 => *top & !flags,
                    _ => flags,
                };
            }
            (b'h' | b'l', Some(b'?')) => {
                for mode in csi[1..].split(|b| *b == b';').filter_map(number) {
                    match mode {
                        2026 => self.synchronized = last == b'h',
                        47 | 1047 | 1049 => self.graphics.clear(),
                        _ => {}
                    }
                }
            }
            (b'J', _) if matches!(number(&csi), Some(2 | 3)) => self.graphics.clear(),
            _ => {}
        }
        self.csi = csi;
    }

    /// Answer the CSI query that `last` ends, if it is one a program waits
    /// on. Primary device attributes: a session asks for them after the
    /// kitty keyboard query, and would wait two seconds without an answer.
    /// Not answering the kitty query itself says it is unsupported, unless
    /// [`set_kitty_keyboard`](Self::set_kitty_keyboard) is on. And
    /// DECRQM for synchronized output (mode 2026), which the session asks
    /// first: recognised and reset, so recorded programs write each frame
    /// as one synchronized update, as on the terminals that have it. vt100
    /// passes over the mode's set and reset, and a frame arrives in one
    /// write anyway, so the screen is the same either way. Other modes'
    /// reports go unanswered, as before.
    fn answer(&mut self, last: u8) {
        self.follow(last);
        let kitty = (last == b'u' && self.csi == b"?" && self.kitty_keyboard).then(|| {
            self.flush();
            format!("\x1b[?{}u", self.keyboard_flags())
        });
        let reply: &[u8] = match (last, self.csi.as_slice(), &kitty) {
            // A VT220 with no extensions, as xterm answers by default.
            (b'c', b"" | b"0", _) => b"\x1b[?62c",
            (b'p', b"?2026$", _) => b"\x1b[?2026;2$y",
            // With the protocol reported: the flags in force.
            (_, _, Some(flags)) => flags.as_bytes(),
            _ => return,
        };
        if self.replies.len() + reply.len() <= MAX_REPLIES {
            self.replies.extend_from_slice(reply);
        }
    }

    /// The answers to queries the program made, to send back to it. Up to
    /// 4 KiB of them are kept until taken; more are dropped.
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
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
                || (regional(c) && first.is_some_and(regional) && cluster.chars == 1);
            if continues && self.extend(c) {
                return;
            }
        }
        push(&mut self.out, c);
        self.cluster = match width {
            Some(native) if native > 0 => Some(Cluster {
                text: c.to_string(),
                chars: 1,
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
        if cluster.chars >= MAX_CLUSTER_CHARS {
            self.cluster = None;
            return false;
        }
        let mut text = cluster.text.clone();
        text.push(c);
        let cells = rich::cells::cell_len(&text).max(cluster.native);
        if cluster.at.is_none() && c.width() == Some(0) && cells == cluster.cells {
            push(&mut self.out, c);
            let cluster = self.cluster.as_mut().expect("cluster");
            cluster.text = text;
            cluster.chars += 1;
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
        cluster.chars += 1;
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

    /// Resize the screen. Fails when vt100 panics.
    pub fn set_size(&mut self, rows: u16, columns: u16) -> std::io::Result<()> {
        self.guard("resizing", |terminal| {
            terminal.cluster = None;
            terminal.parser.screen_mut().set_size(rows, columns);
        })
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// Show the screen `rows` rows back into the scrollback (0: the live
    /// screen), as far as the scrollback goes: [`screen`](Self::screen)'s
    /// cells are then read from there.
    pub fn set_scrollback(&mut self, rows: usize) {
        self.parser.screen_mut().set_scrollback(rows);
    }

    /// The text of a cell of [`screen`](Self::screen) whose contents are
    /// `contents`, as rich prints it: a cluster's cell gives the whole
    /// cluster (as wide as rich measures it), and a filler cell that pads a
    /// cluster to that width gives `None`.
    pub fn cell_text<'a>(&self, contents: &'a str) -> Option<Cow<'a, str>> {
        if let Some(id) = contents.chars().find_map(marker) {
            return Some(Cow::Owned(
                self.clusters.get(id).cloned().unwrap_or_default(),
            ));
        }
        if contents.contains(FILLER) {
            return None;
        }
        Some(Cow::Borrowed(contents))
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
        let (images, placeholders) = self.graphics.tiles(self.parser.screen());
        for (x, y) in placeholders {
            // The image shows there; the text is blank.
            if let Some(cell) = snapshot
                .rows
                .get_mut(usize::from(y))
                .and_then(|row| row.get_mut(usize::from(x)))
            {
                cell.text = " ".into();
                cell.underline = false;
            }
        }
        snapshot.images = images;
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
        terminal.process(bytes.as_bytes()).unwrap();
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
    fn device_attribute_queries_are_answered_and_nothing_else() {
        let mut terminal = Terminal::new(2, 10);
        // crossterm's kitty keyboard probe: the kitty query, then DA1.
        terminal.process(b"\x1b[?u\x1b[c").unwrap();
        assert_eq!(terminal.take_replies(), b"\x1b[?62c");
        assert!(terminal.take_replies().is_empty());
        terminal.process(b"\x1b[0c\x1b[>c\x1b[6n\x1b[31mx").unwrap();
        assert_eq!(terminal.take_replies(), b"\x1b[?62c");
        // Split across reads.
        terminal.process(b"\x1b[").unwrap();
        terminal.process(b"c").unwrap();
        assert_eq!(terminal.take_replies(), b"\x1b[?62c");
    }

    #[test]
    fn answers_nobody_takes_are_capped() {
        let mut terminal = Terminal::new(2, 10);
        for _ in 0..64 {
            terminal.process(&b"\x1b[c".repeat(1024)).unwrap();
        }
        assert!(
            terminal.replies.len() <= MAX_REPLIES,
            "{}",
            terminal.replies.len()
        );
        // Whole answers only, and once taken, answers come again.
        assert!(terminal
            .take_replies()
            .chunks(6)
            .all(|reply| reply == b"\x1b[?62c"));
        terminal.process(b"\x1b[c").unwrap();
        assert_eq!(terminal.take_replies(), b"\x1b[?62c");
    }

    #[test]
    fn a_query_after_a_sequence_cut_off_is_answered() {
        // ESC starts a new sequence, and CAN and SUB cancel one, as vt100
        // reads them: the query after is the program's.
        for bytes in [
            &b"\x1b[1\x1b[c"[..],
            b"\x1b[?2026\x18\x1b[c",
            b"\x1b[1;\x1a\x1b[c",
        ] {
            let mut terminal = Terminal::new(2, 10);
            terminal.process(bytes).unwrap();
            assert_eq!(terminal.take_replies(), b"\x1b[?62c", "{bytes:?}");
        }
        // Cancelled, what follows is text, not the rest of a query.
        let mut terminal = Terminal::new(2, 10);
        terminal.process(b"\x1b[\x18c").unwrap();
        assert!(terminal.take_replies().is_empty());
        assert_eq!(terminal.contents(), "c");
    }

    #[test]
    fn a_sequence_that_never_ends_keeps_little() {
        let mut terminal = Terminal::new(2, 10);
        terminal.process(b"\x1b[").unwrap();
        for _ in 0..16 {
            terminal.process(&[b'1'; 64 * 1024]).unwrap();
        }
        assert!(
            terminal.csi.capacity() <= MAX_CSI,
            "{}",
            terminal.csi.capacity()
        );
        // It still ends at its final byte, and is no query.
        terminal.process(b"c\x1b[c").unwrap();
        assert_eq!(terminal.take_replies(), b"\x1b[?62c");
    }

    #[test]
    fn synchronized_output_is_reported_and_its_updates_change_nothing() {
        let mut terminal = Terminal::new(2, 10);
        // The session's start-up query: DECRQM for mode 2026, the kitty
        // query, then DA1, answered in that order.
        terminal.process(b"\x1b[?2026$p\x1b[?u\x1b[c").unwrap();
        assert_eq!(terminal.take_replies(), b"\x1b[?2026;2$y\x1b[?62c");
        // Other modes' reports, and an ANSI-mode DECRQM, are not answered.
        terminal.process(b"\x1b[?2004$p\x1b[2026$p").unwrap();
        assert!(terminal.take_replies().is_empty());
        // An update shows what it holds, as it would without one.
        let (plain, _) = screen("ab", 10);
        terminal.process(b"\x1b[?2026hab\x1b[?2026l").unwrap();
        assert_eq!(terminal.contents(), plain.contents());
        // Split across reads, and inside an update.
        terminal.process(b"\x1b[?2026h\x1b[?20").unwrap();
        terminal.process(b"26$p\x1b[?2026l").unwrap();
        assert_eq!(terminal.take_replies(), b"\x1b[?2026;2$y");
        assert_eq!(terminal.contents(), plain.contents());
    }

    /// A PNG of `width` x `height` pixels of `colour`, base64.
    fn png64(width: u32, height: u32, colour: [u8; 4]) -> String {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            let mut writer = encoder.write_header().unwrap();
            let pixels: Vec<u8> = (0..width * height).flat_map(|_| colour).collect();
            writer.write_image_data(&pixels).unwrap();
        }
        base64(&out)
    }

    fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
        out
    }

    /// The tiles' cells: (column, row, columns, rows).
    fn placed(snapshot: &Snapshot) -> Vec<(u16, u16, u16, u16)> {
        snapshot
            .images
            .iter()
            .map(|t| (t.column, t.row, t.columns, t.rows))
            .collect()
    }

    #[test]
    fn kitty_images_are_placed_at_the_cursor_and_deleted() {
        let mut terminal = Terminal::new(4, 10);
        let data = png64(16, 32, [255, 0, 0, 255]);
        // Sent in two chunks, as programs split a large image.
        let (first, rest) = data.split_at(10);
        terminal
            .process(
                format!(
                    "x\x1b_Ga=T,f=100,i=5,c=2,r=2,q=2,m=1;{first}\x1b\\\x1b_Gm=0;{rest}\x1b\\y"
                )
                .as_bytes(),
            )
            .unwrap();
        let snapshot = terminal.snapshot(&Theme::default());
        assert_eq!(placed(&snapshot), [(1, 0, 2, 1), (1, 1, 2, 1)]);
        // The image's halves, top and bottom.
        assert_eq!(snapshot.images[0].source, [0.0, 0.0, 16.0, 16.0]);
        assert_eq!(snapshot.images[1].source, [0.0, 16.0, 16.0, 16.0]);
        assert_eq!(
            snapshot.images[0].image.pixels().unwrap()[..4],
            [255, 0, 0, 255]
        );
        // The cursor moved past the image, on its last row.
        assert_eq!(terminal.screen().cursor_position(), (1, 4));
        // Nothing reached vt100's screen, and nothing was answered.
        assert_eq!(terminal.contents(), "x\n   y");
        assert!(terminal.take_replies().is_empty());
        // Placed again with C=1 under the same placement: moved, not added.
        terminal
            .process(b"\x1b[3;1H\x1b_Ga=p,i=5,c=1,r=1,C=1\x1b\\")
            .unwrap();
        assert_eq!(
            placed(&terminal.snapshot(&Theme::default())),
            [(0, 2, 1, 1)]
        );
        assert_eq!(terminal.screen().cursor_position(), (2, 0));
        terminal.process(b"\x1b_Ga=d,d=i,i=5\x1b\\").unwrap();
        assert!(terminal.snapshot(&Theme::default()).images.is_empty());
    }

    #[test]
    fn kitty_raw_pixels_compressed_or_not() {
        let rgb: Vec<u8> = (0..4).flat_map(|_| [0u8, 0, 255]).collect();
        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut zlib, &rgb).unwrap();
        let compressed = base64(&zlib.finish().unwrap());
        for (keys, data) in [
            ("f=24,s=2,v=2", base64(&rgb)),
            ("f=24,s=2,v=2,o=z", compressed),
        ] {
            let mut terminal = Terminal::new(2, 4);
            terminal
                .process(format!("\x1b_Ga=T,i=1,{keys};{data}\x1b\\").as_bytes())
                .unwrap();
            let snapshot = terminal.snapshot(&Theme::default());
            assert_eq!(placed(&snapshot), [(0, 0, 1, 1)], "{keys}");
            let pixels = snapshot.images[0].image.pixels().unwrap();
            assert_eq!(pixels[..4], [0, 0, 255, 255], "{keys}");
        }
        // Data that does not match its size shows nothing.
        let mut terminal = Terminal::new(2, 4);
        terminal
            .process(format!("\x1b_Ga=T,i=1,f=24,s=3,v=2;{}\x1b\\", base64(&rgb)).as_bytes())
            .unwrap();
        assert!(terminal.snapshot(&Theme::default()).images.is_empty());
    }

    #[test]
    fn kitty_placeholder_cells_show_their_image() {
        // As rs-rich-art draws one: a virtual placement of 2x2 cells, then
        // placeholder cells in the colour that names image 7.
        let mut terminal = Terminal::new(3, 6);
        let data = png64(16, 32, [0, 255, 0, 255]);
        let cell = |row: char, column: char| format!("\u{10EEEE}{row}{column}");
        let (r0, r1, c0, c1) = ('\u{0305}', '\u{030D}', '\u{0305}', '\u{030D}');
        terminal
            .process(
                format!(
                    "\x1b_Ga=T,U=1,f=100,t=d,i=7,c=2,r=2,q=2;{data}\x1b\\\
                     \x1b[38;2;0;0;7m{}{}\x1b[39m|\r\n\x1b[38;2;0;0;7m{}\u{10EEEE}\x1b[39m",
                    cell(r0, c0),
                    cell(r0, c1),
                    cell(r1, c0),
                )
                .as_bytes(),
            )
            .unwrap();
        let snapshot = terminal.snapshot(&Theme::default());
        // The second row's second cell has no diacritics: it continues the
        // cell before it.
        assert_eq!(placed(&snapshot), [(0, 0, 2, 1), (0, 1, 2, 1)]);
        assert_eq!(snapshot.images[1].source, [0.0, 16.0, 16.0, 16.0]);
        assert!(snapshot.images.iter().all(|tile| tile.below_text));
        // The cells read blank; the bar after them is text.
        assert_eq!(snapshot.text_grid(), "  |\n\n\n");
        // Without its virtual placement, or with another id, nothing shows.
        let mut other = Terminal::new(2, 4);
        other
            .process(format!("\x1b[38;2;0;0;8m{}", cell(r0, c0)).as_bytes())
            .unwrap();
        assert!(other.snapshot(&Theme::default()).images.is_empty());
    }

    #[test]
    fn sixel_shows_until_text_covers_it_and_a_clear_removes_it() {
        let mut terminal = Terminal::new(4, 6);
        // 8x32 red pixels: one cell across, two down; the cursor goes on
        // to the line after it.
        terminal
            .process(b"\x1b[1;2H\x1bP0;1;0q\"1;1;8;32#1;2;100;0;0#1!8~-!8~-!8~-!8~-!8~-!8~\x1b\\")
            .unwrap();
        let snapshot = terminal.snapshot(&Theme::default());
        assert_eq!(placed(&snapshot), [(1, 0, 1, 1), (1, 1, 1, 1)]);
        assert_eq!(terminal.screen().cursor_position(), (2, 1));
        // Text written over its second cell hides it there.
        terminal.process(b"\x1b[2;2HX").unwrap();
        assert_eq!(
            placed(&terminal.snapshot(&Theme::default())),
            [(1, 0, 1, 1)]
        );
        terminal.process(b"\x1b[2J").unwrap();
        assert!(terminal.snapshot(&Theme::default()).images.is_empty());
        // Other DCS strings still reach vt100 (which passes over them).
        terminal.process(b"\x1bP$qm\x1b\\ok").unwrap();
        assert!(terminal.contents().contains("ok"));
    }

    #[test]
    fn iterm_inline_images_take_their_cells() {
        let mut terminal = Terminal::new(3, 8);
        let data = png64(4, 2, [0, 0, 255, 255]);
        terminal
            .process(
                format!(
                    "\x1b]1337;File=inline=1;size=9;width=4;height=1;preserveAspectRatio=0:{data}\x07"
                )
                .as_bytes(),
            )
            .unwrap();
        let snapshot = terminal.snapshot(&Theme::default());
        assert_eq!(placed(&snapshot), [(0, 0, 4, 1)]);
        assert_eq!(snapshot.images[0].source, [0.0, 0.0, 4.0, 2.0]);
        assert_eq!(terminal.screen().cursor_position(), (0, 4));
        // Fitted, its aspect kept: 4x2 pixels in 4 cells of 8x16 fill the
        // width, and the part below the image is transparent.
        let mut fitted = Terminal::new(3, 8);
        fitted
            .process(format!("\x1b]1337;File=inline=1;width=4;height=1:{data}\x07").as_bytes())
            .unwrap();
        assert_eq!(
            fitted.snapshot(&Theme::default()).images[0].source,
            [0.0, 0.0, 4.0, 2.0]
        );
        // Not inline, or not an image: nothing, and the OSC title still
        // reaches vt100.
        let mut other = Terminal::new(2, 8);
        other
            .process(format!("\x1b]1337;File=inline=0:{data}\x07\x1b]0;title\x07ok").as_bytes())
            .unwrap();
        assert!(other.snapshot(&Theme::default()).images.is_empty());
        assert_eq!(other.contents(), "ok");
    }

    #[test]
    fn kitty_keyboard_flags_are_followed_for_each_screen() {
        let mut terminal = Terminal::new(2, 10);
        // Off: the query goes unanswered and no flags are reported.
        terminal.process(b"\x1b[>3u\x1b[?u").unwrap();
        assert!(terminal.take_replies().is_empty());
        assert_eq!(terminal.keyboard_flags(), 0);
        let mut terminal = Terminal::new(2, 10);
        terminal.set_kitty_keyboard(true);
        terminal.process(b"\x1b[?u").unwrap();
        assert_eq!(terminal.take_replies(), b"\x1b[?0u");
        // A session pushes on the alternate screen and pops before leaving.
        terminal.process(b"\x1b[?1049h\x1b[>3u\x1b[?u").unwrap();
        assert_eq!(terminal.keyboard_flags(), 3);
        assert_eq!(terminal.take_replies(), b"\x1b[?3u");
        terminal.process(b"\x1b[=8;2u").unwrap();
        assert_eq!(terminal.keyboard_flags(), 11);
        terminal.process(b"\x1b[=1;3u").unwrap();
        assert_eq!(terminal.keyboard_flags(), 10);
        // The main screen has its own stack.
        terminal.process(b"\x1b[?1049l").unwrap();
        assert_eq!(terminal.keyboard_flags(), 0);
        terminal.process(b"\x1b[?1049h").unwrap();
        assert_eq!(terminal.keyboard_flags(), 10);
        terminal.process(b"\x1b[<u").unwrap();
        assert_eq!(terminal.keyboard_flags(), 0);
        // Popping more than was pushed leaves none.
        terminal.process(b"\x1b[>1u\x1b[>2u\x1b[<5u").unwrap();
        assert_eq!(terminal.keyboard_flags(), 0);
    }

    #[test]
    fn synchronized_updates_are_followed() {
        let mut terminal = Terminal::new(2, 10);
        terminal.process(b"\x1b[?2026ha").unwrap();
        assert!(terminal.in_synchronized_update());
        terminal.process(b"b\x1b[?2026l").unwrap();
        assert!(!terminal.in_synchronized_update());
        // Split across reads.
        terminal.process(b"\x1b[?20").unwrap();
        terminal.process(b"26h").unwrap();
        assert!(terminal.in_synchronized_update());
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
        terminal
            .process("👩\u{200d}👧\r\n👩\u{200d}👧".as_bytes())
            .unwrap();
        assert_eq!(terminal.clusters.len(), 1);
        // With every marker taken, a new cluster is printed as vt100 would,
        // and the cells already on screen keep their text.
        terminal.capacity = 1;
        terminal.process("\r\n👍🏽".as_bytes()).unwrap();
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
            terminal.process(std::slice::from_ref(byte)).unwrap();
        }
        assert_eq!(terminal.contents(), "x👩\u{200d}👧❤\u{fe0f}y");
        assert_eq!(terminal.snapshot(&Theme::default()).rows[0][5].text, "y");
    }

    #[test]
    fn escapes_with_intermediate_bytes_print_nothing() {
        // `ESC ( B` selects a character set: the B is not printed, so a skin
        // tone or ZWJ after it does not join the cell before.
        for text in [
            "ab\x1b(B\u{1F3FD}",
            "ab\x1b(B\u{200d}\u{1F467}",
            "ab\x1b#8\x1b( B",
        ] {
            let mut terminal = Terminal::new(1, 10);
            terminal.process(text.as_bytes()).unwrap();
            let mut parser = vt100::Parser::new(1, 10, 0);
            parser.process(text.as_bytes());
            assert_eq!(terminal.contents(), parser.screen().contents(), "{text:?}");
        }
        let (_, snapshot) = screen("ab\x1b(B\u{1F3FD}", 10);
        assert_eq!(snapshot.rows[0][1].text, "b");
        assert_eq!(snapshot.rows[0][2].text, "\u{1F3FD}");
    }

    #[test]
    fn a_flood_of_combining_marks_takes_linear_time() {
        // Each mark re-measured the whole cluster: 40,000 took ten seconds.
        let mut text = String::from("a");
        text.extend(std::iter::repeat_n('\u{301}', 40_000));
        text.push('b');
        let start = std::time::Instant::now();
        let mut terminal = Terminal::new(2, 10);
        terminal.process(text.as_bytes()).unwrap();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert!(terminal.contents().starts_with("a\u{301}"));
        assert!(terminal.contents().ends_with('b'));
    }

    #[test]
    fn cells_read_back_as_rich_prints_them() {
        let mut terminal = Terminal::with_scrollback(2, 10, 10);
        terminal
            .process("a👩\u{200d}👧b\r\nl2\r\nl3".as_bytes())
            .unwrap();
        // The first line scrolled off the top; the scrollback shows it.
        terminal.set_scrollback(1);
        let text = |column| {
            let cell = terminal.screen().cell(0, column).unwrap();
            terminal.cell_text(cell.contents()).map(Cow::into_owned)
        };
        assert_eq!(text(0).as_deref(), Some("a"));
        assert_eq!(text(1).as_deref(), Some("👩\u{200d}👧"));
        assert_eq!(text(3).as_deref(), Some("b"));
        terminal.set_scrollback(0);
        assert_eq!(terminal.contents(), "l2\nl3");
        assert_eq!(terminal.cell_text(&FILLER.to_string()), None);
    }

    #[test]
    fn a_vt100_panic_is_an_error() {
        // vt100 0.16 panics on a wide character in a one-column screen, and on
        // a resize that cuts a wide character at the right edge.
        let mut terminal = Terminal::new(2, 1);
        let error = terminal.process("漢".as_bytes()).unwrap_err();
        assert!(error.to_string().contains("emulator failed"), "{error}");
        assert!(terminal.error().is_some());
        assert!(terminal.process(b"x").is_err());
        let mut terminal = Terminal::new(2, 4);
        terminal.process("漢漢".as_bytes()).unwrap();
        let resized = terminal
            .set_size(2, 3)
            .and_then(|()| terminal.process(b"x"));
        assert!(resized.is_err());
    }
}
