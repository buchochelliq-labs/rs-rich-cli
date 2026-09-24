//! A byte and hex inspector in the style of `hexdump -C`.
//!
//! [`HexView`] shows bytes as lines of an offset, the bytes in hex (in groups,
//! with an extra space between groups) and an ASCII panel. Bytes are coloured
//! by class, matches of a needle are highlighted, and runs of identical full
//! lines collapse to a single `*` line. The last line holds the end offset,
//! as `hexdump -C` prints it.
//!
//! ```
//! use rich::Console;
//! use rich_ext::hex::{parse_needle, HexView};
//!
//! let console = Console::builder().width(80).color_system(None).build();
//! let needle = parse_needle("\"PNG\"").unwrap();
//! let view = HexView::new(*b"\x89PNG\r\n\x1a\n").highlight(&needle);
//! let out = console.segments_to_string(&rich::Renderable::rich_render(&view, &console, &console.options()));
//! assert_eq!(
//!     out.lines().next().unwrap(),
//!     "00000000  89 50 4e 47 0d 0a 1a 0a                           │.PNG....│"
//! );
//! ```
//!
//! Styles come from the theme keys `hex.offset`, `hex.null`, `hex.printable`,
//! `hex.whitespace`, `hex.control`, `hex.high`, `hex.match` and `hex.border`,
//! with built-in fallbacks when the theme lacks them.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Overflow, Renderable, Segment, Style, Text};

use crate::event::theme_style;

/// Every start position of `needle` in `haystack`, overlapping matches
/// included. An empty needle matches nowhere.
pub fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at)
        .collect()
}

/// Parse a search needle: hex (`"de ad be ef"`, `"deadbeef"`, `"0xDEAD"`,
/// separators ` `, `,`, `:` allowed) or quoted text (`"\"PNG\""` or
/// `"'PNG'"`). Quoted text understands `\\`, `\"`, `\'`, `\n`, `\r`, `\t`,
/// `\0` and `\xNN`.
pub fn parse_needle(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty needle".into());
    }
    let quoted = s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')));
    if quoted {
        let bytes = unescape(&s[1..s.len() - 1])?;
        if bytes.is_empty() {
            return Err("empty needle".into());
        }
        return Ok(bytes);
    }
    let mut out = Vec::new();
    for token in s.split(|c: char| c.is_whitespace() || c == ',' || c == ':') {
        if token.is_empty() {
            continue;
        }
        let digits = token
            .strip_prefix("0x")
            .or_else(|| token.strip_prefix("0X"))
            .unwrap_or(token);
        if digits.is_empty() {
            return Err(format!("{token:?} has no hex digits"));
        }
        if let Some(bad) = digits.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(format!(
                "{bad:?} is not a hex digit (quote text to search for it, e.g. \"\\\"{token}\\\"\")"
            ));
        }
        if digits.len() % 2 != 0 {
            return Err(format!("{token:?} has an odd number of hex digits"));
        }
        for pair in digits.as_bytes().chunks(2) {
            let pair = std::str::from_utf8(pair).expect("ASCII hex digits");
            out.push(u8::from_str_radix(pair, 16).expect("validated hex"));
        }
    }
    if out.is_empty() {
        return Err("empty needle".into());
    }
    Ok(out)
}

fn unescape(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        match chars.next() {
            Some('\\') => out.push(b'\\'),
            Some('"') => out.push(b'"'),
            Some('\'') => out.push(b'\''),
            Some('n') => out.push(b'\n'),
            Some('r') => out.push(b'\r'),
            Some('t') => out.push(b'\t'),
            Some('0') => out.push(0),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                match u8::from_str_radix(&hex, 16) {
                    Ok(b) if hex.len() == 2 => out.push(b),
                    _ => return Err(format!("\\x{hex} is not a two-digit hex escape")),
                }
            }
            Some(other) => return Err(format!("unknown escape \\{other}")),
            None => return Err("trailing backslash".into()),
        }
    }
    Ok(out)
}

/// The class a byte is coloured by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteClass {
    /// `0x00`.
    Null,
    /// `0x21..=0x7e`.
    Printable,
    /// Space, `\t`, `\n`, `\r`, VT and FF.
    Whitespace,
    /// Other bytes below `0x20`, and `0x7f`.
    Control,
    /// `0x80` and above.
    High,
}

impl ByteClass {
    /// The class of `byte`.
    pub fn of(byte: u8) -> Self {
        match byte {
            0 => ByteClass::Null,
            b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c => ByteClass::Whitespace,
            0x21..=0x7e => ByteClass::Printable,
            0x80..=0xff => ByteClass::High,
            _ => ByteClass::Control,
        }
    }

    fn style(self, console: &Console) -> Style {
        match self {
            ByteClass::Null => theme_style(console, "hex.null", "dim"),
            ByteClass::Printable => theme_style(console, "hex.printable", ""),
            ByteClass::Whitespace => theme_style(console, "hex.whitespace", "green"),
            ByteClass::Control => theme_style(console, "hex.control", "yellow"),
            ByteClass::High => theme_style(console, "hex.high", "magenta"),
        }
    }
}

/// A renderable hex dump of some bytes.
#[derive(Clone, Debug)]
pub struct HexView {
    bytes: Vec<u8>,
    offset: u64,
    bytes_per_line: Option<usize>,
    group: usize,
    needle: Vec<u8>,
    ascii_panel: bool,
    collapse: bool,
}

/// The most bytes per line [`HexView::bytes_per_line`] accepts; larger
/// values are clamped to it.
pub const MAX_BYTES_PER_LINE: usize = 4096;

/// The bytes per line [`HexView`] prefers when fitting the width.
const PREFERRED_PER_LINE: usize = 16;

impl HexView {
    /// Dump `bytes`, starting at offset 0, fitting the width.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        HexView {
            bytes: bytes.into(),
            offset: 0,
            bytes_per_line: None,
            group: 8,
            needle: Vec::new(),
            ascii_panel: true,
            collapse: true,
        }
    }

    /// The address of the first byte (for a slice of a larger file).
    pub fn offset(mut self, base: u64) -> Self {
        self.offset = base;
        self
    }

    /// Bytes per line. `None` (the default) fits the width: 16 when it fits,
    /// otherwise the largest multiple of the group size between 8 and 32 that
    /// fits, and fewer than 8 only when nothing else does. `Some(n)` is used
    /// as given, between 1 and [`MAX_BYTES_PER_LINE`]; lines wider than the
    /// console fold.
    pub fn bytes_per_line(mut self, n: Option<usize>) -> Self {
        self.bytes_per_line = n.map(|n| n.clamp(1, MAX_BYTES_PER_LINE));
        self
    }

    /// Bytes per group (default 8, at least 1).
    pub fn group(mut self, n: usize) -> Self {
        self.group = n.max(1);
        self
    }

    /// Highlight every match of `needle` (an empty needle highlights nothing).
    pub fn highlight(mut self, needle: &[u8]) -> Self {
        self.needle = needle.to_vec();
        self
    }

    /// Show the ASCII panel (default on).
    pub fn ascii_panel(mut self, on: bool) -> Self {
        self.ascii_panel = on;
        self
    }

    /// Collapse runs of identical full lines to `*` (default on).
    pub fn collapse(mut self, on: bool) -> Self {
        self.collapse = on;
        self
    }

    /// The bytes being shown.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Hex digits in the offset column: enough for the end offset, at least 8.
    pub fn offset_digits(&self) -> usize {
        let end = self.offset.saturating_add(self.bytes.len() as u64);
        let digits = if end == 0 {
            1
        } else {
            (64 - end.leading_zeros() as usize).div_ceil(4)
        };
        digits.max(8)
    }

    /// The cell width of a full line with `n` bytes per line.
    /// Saturates rather than overflowing for absurd `n`.
    pub fn line_width(&self, n: usize) -> usize {
        let n = n.max(1);
        let groups = n.div_ceil(self.group);
        let hex = n
            .saturating_mul(3)
            .saturating_sub(1)
            .saturating_add(groups - 1);
        let panel = if self.ascii_panel {
            n.saturating_add(4)
        } else {
            0
        };
        self.offset_digits()
            .saturating_add(2)
            .saturating_add(hex)
            .saturating_add(panel)
    }

    /// The bytes per line used at `width` cells.
    pub fn resolved_bytes_per_line(&self, width: usize) -> usize {
        if let Some(n) = self.bytes_per_line {
            return n.clamp(1, MAX_BYTES_PER_LINE);
        }
        let fits = |n: usize| self.line_width(n) <= width;
        let candidates: Vec<usize> = (1..=32 / self.group.min(32))
            .map(|k| k * self.group)
            .filter(|n| (8..=32).contains(n))
            .collect();
        if candidates.contains(&PREFERRED_PER_LINE) && fits(PREFERRED_PER_LINE) {
            return PREFERRED_PER_LINE;
        }
        // Largest candidate not above 16, then the smallest above it.
        let below = candidates
            .iter()
            .rev()
            .find(|&&n| n <= PREFERRED_PER_LINE && fits(n));
        let above = candidates
            .iter()
            .find(|&&n| n > PREFERRED_PER_LINE && fits(n));
        if let Some(&n) = below.or(above) {
            return n;
        }
        (1..8).rev().find(|&n| fits(n)).unwrap_or(1)
    }

    fn lines(&self, console: &Console, per_line: usize) -> Vec<Text> {
        let ascii = console.ascii_only();
        let bar = if ascii { "|" } else { "│" };
        let digits = self.offset_digits();
        let offset_style = theme_style(console, "hex.offset", "cyan");
        let border_style = theme_style(console, "hex.border", "dim");
        let match_style = theme_style(console, "hex.match", "reverse");

        let mut matched = vec![false; self.bytes.len()];
        for at in find_all(&self.bytes, &self.needle) {
            matched[at..at + self.needle.len()].fill(true);
        }
        let byte_style = |i: usize| {
            let base = ByteClass::of(self.bytes[i]).style(console);
            if matched[i] {
                base.combine(&match_style)
            } else {
                base
            }
        };

        // The ASCII panel lines up after the hex column of a full line. When
        // the data is shorter than a line, the column is only as wide as the
        // data needs (but never narrower than the usual 16 bytes), so a huge
        // `bytes_per_line` does not pad a small file with thousands of spaces.
        let pad_bytes = per_line.min(self.bytes.len().max(PREFERRED_PER_LINE));
        let groups = pad_bytes.div_ceil(self.group);
        let hex_width = 3 * pad_bytes - 1 + (groups - 1);
        let mut out = Vec::new();
        let mut previous: Option<&[u8]> = None;
        let mut starred = false;
        for (index, chunk) in self.bytes.chunks(per_line).enumerate() {
            let start = index * per_line;
            let full = chunk.len() == per_line;
            let has_match = matched[start..start + chunk.len()].iter().any(|m| *m);
            if self.collapse && full && !has_match && previous == Some(chunk) {
                if !starred {
                    out.push(Text::new("*"));
                    starred = true;
                }
                continue;
            }
            starred = false;
            previous = if full && !has_match {
                Some(chunk)
            } else {
                None
            };

            let mut line = Text::new("");
            let address = self.offset.saturating_add(start as u64);
            line.append(
                &format!("{address:0digits$x}"),
                Some(offset_style.clone().into()),
            );
            line.append("  ", None);
            let mut used = 0;
            for (k, &byte) in chunk.iter().enumerate() {
                if k > 0 {
                    let gap = if k % self.group == 0 { "  " } else { " " };
                    line.append(gap, None);
                    used += gap.len();
                }
                line.append(&format!("{byte:02x}"), Some(byte_style(start + k).into()));
                used += 2;
            }
            if self.ascii_panel {
                line.append(&" ".repeat(hex_width.saturating_sub(used) + 2), None);
                line.append(bar, Some(border_style.clone().into()));
                for (k, &byte) in chunk.iter().enumerate() {
                    let shown = if (0x20..=0x7e).contains(&byte) {
                        byte as char
                    } else {
                        '.'
                    };
                    line.append(&shown.to_string(), Some(byte_style(start + k).into()));
                }
                line.append(bar, Some(border_style.clone().into()));
            }
            out.push(line);
        }
        let end = self.offset.saturating_add(self.bytes.len() as u64);
        out.push(Text::styled(format!("{end:0digits$x}"), offset_style));
        out
    }
}

impl Renderable for HexView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let per_line = self.resolved_bytes_per_line(options.max_width);
        let mut out = Vec::new();
        for line in self.lines(console, per_line) {
            if out
                .last()
                .is_some_and(|s: &Segment| !s.text.ends_with('\n'))
            {
                out.push(Segment::line());
            }
            let line = line.overflow(Overflow::Fold);
            out.extend(line.rich_render(console, options));
        }
        // The console ends the output with a newline; don't add a blank line.
        while out.last().is_some_and(|s| s.text == "\n") {
            out.pop();
        }
        out
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let (min, max) = match self.bytes_per_line {
            Some(n) => (self.line_width(n), self.line_width(n)),
            None => (self.line_width(1), self.line_width(PREFERRED_PER_LINE)),
        };
        Measurement::new(min, max).clamp(None, Some(options.max_width))
    }
}
