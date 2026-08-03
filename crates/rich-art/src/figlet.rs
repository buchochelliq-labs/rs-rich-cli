//! FIGlet-style text banners.
//!
//! Parses the FIGfont (`.flf`) format and lays characters out with FIGlet's
//! kerning/smushing rules — the algorithm shared by `figlet(6)` and `pyfiglet`.
//! Output is byte-parity with `pyfiglet` for the bundled font (see
//! `tests/figlet_parity.rs`).

use std::collections::HashMap;
use std::fmt;

/// Smushing-mode bits, lifted from figlet's `figlet222` constants (the same
/// names `pyfiglet` uses).
mod smush {
    pub const EQUAL: i64 = 1; // smush equal chars (not hardblanks)
    pub const LOWLINE: i64 = 2; // `_` smushes with any char in the hierarchy
    pub const HIERARCHY: i64 = 4; // hierarchy: | , /\ , [] , {} , () , <>
    pub const PAIR: i64 = 8; // [ + ] -> | , { + } -> | , ( + ) -> |
    pub const BIGX: i64 = 16; // / + \ -> | , \ + / -> Y , > + < -> X
    pub const HARDBLANK: i64 = 32; // hardblank + hardblank -> hardblank
    pub const KERN: i64 = 64;
    pub const SMUSH: i64 = 128;
}

/// How a rendered banner is positioned within the available width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Left,
    Center,
    Right,
}

/// Anything that can go wrong loading a font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    /// The file didn't start with the `flf2a` signature.
    BadSignature,
    /// The header line was malformed or truncated.
    BadHeader(String),
    /// The character data ended sooner than the header promised.
    Truncated,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::BadSignature => write!(f, "not a FIGfont (missing `flf2a` signature)"),
            FontError::BadHeader(detail) => write!(f, "malformed FIGfont header: {detail}"),
            FontError::Truncated => write!(f, "FIGfont ended mid-character"),
        }
    }
}

impl std::error::Error for FontError {}

/// A parsed FIGfont. Mirrors the parts of `pyfiglet`'s `FigletFont` that
/// rendering needs.
#[derive(Debug, Clone)]
pub struct FigletFont {
    /// Each character's rows, keyed by code point. Every entry has `height` rows.
    chars: HashMap<u32, Vec<String>>,
    /// Printable width of each character (its rows are all this wide).
    widths: HashMap<u32, usize>,
    height: usize,
    hard_blank: char,
    smush_mode: i64,
}

/// The FIGfont bundled with this crate (`standard.flf` from the FIGlet
/// distribution — see `fonts/README.md` for provenance and its permission
/// notice).
pub const STANDARD_FONT: &str = include_str!("../fonts/standard.flf");

impl FigletFont {
    /// Parse a FIGfont from the contents of a `.flf` file.
    pub fn parse(source: &str) -> Result<Self, FontError> {
        // Normalise line endings; `.flf` files in the wild use both.
        let lines: Vec<&str> = source
            .split('\n')
            .map(|l| l.trim_end_matches('\r'))
            .collect();
        let header = lines.first().ok_or(FontError::BadSignature)?;
        if !header.starts_with("flf2a") {
            return Err(FontError::BadSignature);
        }

        let fields: Vec<&str> = header.split_whitespace().collect();
        // fields[0] is `flf2a` + the hardblank character.
        let signature = fields.first().ok_or(FontError::BadSignature)?;
        let hard_blank = signature
            .chars()
            .nth(5)
            .ok_or_else(|| FontError::BadHeader("no hardblank character".into()))?;

        let number = |index: usize, what: &str| -> Result<i64, FontError> {
            fields
                .get(index)
                .ok_or_else(|| FontError::BadHeader(format!("missing {what}")))?
                .parse::<i64>()
                .map_err(|_| FontError::BadHeader(format!("non-numeric {what}")))
        };

        let height = number(1, "height")? as usize;
        let old_layout = number(4, "old layout")?;
        let comment_lines = number(5, "comment line count")? as usize;
        // `full_layout` (field 7) is optional — older fonts stop after
        // `print_direction`, or even before it.
        let full_layout = fields.get(7).and_then(|f| f.parse::<i64>().ok());

        // Port of figlet's smush-mode derivation: a full layout wins outright;
        // otherwise the legacy `old_layout` is promoted.
        let smush_mode = match full_layout {
            Some(layout) => layout,
            None => match old_layout {
                0 => smush::KERN,
                n if n < 0 => 0,
                n => (n & 31) | smush::SMUSH,
            },
        };

        if height == 0 {
            return Err(FontError::BadHeader("height of zero".into()));
        }

        let mut chars = HashMap::new();
        let mut widths = HashMap::new();
        let mut cursor = 1 + comment_lines;

        // Characters 32..=126 appear first, in order, followed by the required
        // German set, then any code-tagged characters.
        let required: Vec<i64> = (32..=126)
            .chain([196, 214, 220, 228, 246, 252, 223])
            .collect();

        for code in required {
            match read_char(&lines, &mut cursor, height) {
                Some((rows, width)) => {
                    chars.insert(code as u32, rows);
                    widths.insert(code as u32, width);
                }
                // Some fonts in the wild stop early; keep what we have rather
                // than failing the whole font.
                None => break,
            }
        }

        // Code-tagged characters: a line giving the code point (decimal, or
        // `0x`/`0` prefixed), optionally followed by a comment, then its rows.
        while cursor < lines.len() {
            let tag = lines[cursor].trim();
            if tag.is_empty() {
                cursor += 1;
                continue;
            }
            let Some(code) = parse_code_tag(tag) else {
                break;
            };
            cursor += 1;
            match read_char(&lines, &mut cursor, height) {
                Some((rows, width)) => {
                    chars.insert(code, rows);
                    widths.insert(code, width);
                }
                None => break,
            }
        }

        if chars.is_empty() {
            return Err(FontError::Truncated);
        }

        Ok(FigletFont {
            chars,
            widths,
            height,
            hard_blank,
            smush_mode,
        })
    }

    /// The bundled `standard` font.
    pub fn standard() -> Self {
        FigletFont::parse(STANDARD_FONT).expect("bundled standard.flf is valid")
    }

    /// Row count of every character in this font.
    pub fn height(&self) -> usize {
        self.height
    }

    /// The character this font uses as a "hard blank" (rendered as a space).
    pub fn hard_blank(&self) -> char {
        self.hard_blank
    }

    fn rows_for(&self, code: u32) -> Option<&Vec<String>> {
        self.chars.get(&code)
    }

    fn width_for(&self, code: u32) -> Option<usize> {
        self.widths.get(&code).copied()
    }
}

impl Default for FigletFont {
    fn default() -> Self {
        FigletFont::standard()
    }
}

/// Read one character's `height` rows starting at `cursor`, stripping the
/// trailing end-mark run. Returns the rows and their common width.
fn read_char(lines: &[&str], cursor: &mut usize, height: usize) -> Option<(Vec<String>, usize)> {
    if *cursor + height > lines.len() {
        return None;
    }
    let mut rows = Vec::with_capacity(height);
    for offset in 0..height {
        let raw = lines[*cursor + offset];
        rows.push(strip_end_marks(raw));
    }
    *cursor += height;
    // Every row of a character is padded to the same width by the font, but be
    // defensive: pad to the widest.
    let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    for row in &mut rows {
        let short = width - row.chars().count();
        if short > 0 {
            row.push_str(&" ".repeat(short));
        }
    }
    Some((rows, width))
}

/// Strip a row's end-mark run: the final character is the end mark (usually
/// `@`), and the last row of a character doubles it.
fn strip_end_marks(row: &str) -> String {
    let chars: Vec<char> = row.chars().collect();
    let Some(&mark) = chars.last() else {
        return String::new();
    };
    let mut end = chars.len();
    while end > 0 && chars[end - 1] == mark {
        end -= 1;
    }
    chars[..end].iter().collect()
}

/// Parse a code-tag line's leading code point (decimal, `0xNN` hex, or `0NN`
/// octal — figlet accepts all three), ignoring any trailing comment.
fn parse_code_tag(tag: &str) -> Option<u32> {
    let token = tag.split_whitespace().next()?;
    let (negative, digits) = match token.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let value = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).ok()?
    } else if digits.len() > 1 && digits.starts_with('0') {
        i64::from_str_radix(&digits[1..], 8).ok()?
    } else {
        digits.parse::<i64>().ok()?
    };
    if negative {
        return None; // negative code points aren't renderable
    }
    u32::try_from(value).ok()
}

/// Lays text out in a [`FigletFont`], applying FIGlet's kerning/smushing rules.
/// Port of `pyfiglet`'s `FigletBuilder`.
struct Builder<'a> {
    text: Vec<u32>,
    font: &'a FigletFont,
    width: usize,
    justify: Justify,

    iterator: usize,
    max_smush: usize,
    cur_char_width: usize,
    prev_char_width: usize,

    buffer: Vec<String>,
    /// Saved `(buffer, iterator)` pairs at each blank, for line wrapping.
    blank_markers: Vec<(Vec<String>, usize)>,
    product: Vec<Vec<String>>,
}

impl<'a> Builder<'a> {
    fn new(text: &str, font: &'a FigletFont, width: usize, justify: Justify) -> Self {
        Builder {
            text: text.chars().map(|c| c as u32).collect(),
            font,
            width,
            justify,
            iterator: 0,
            max_smush: 0,
            cur_char_width: 0,
            prev_char_width: 0,
            buffer: vec![String::new(); font.height],
            blank_markers: Vec::new(),
            product: Vec::new(),
        }
    }

    fn cur_code(&self) -> Option<u32> {
        self.text.get(self.iterator).copied()
    }

    fn cur_rows(&self) -> Option<&'a Vec<String>> {
        self.font.rows_for(self.cur_code()?)
    }

    fn cur_width(&self) -> Option<usize> {
        self.font.width_for(self.cur_code()?)
    }

    /// Port of `FigletBuilder.addCharToProduct`.
    fn add_char(&mut self) {
        let Some(code) = self.cur_code() else {
            return;
        };
        if code == '\n' as u32 {
            self.blank_markers
                .push((self.buffer.clone(), self.iterator));
            self.handle_new_line();
            return;
        }
        let Some(rows) = self.cur_rows() else {
            return;
        };
        let Some(char_width) = self.cur_width() else {
            return;
        };
        // A character wider than the whole line can never be laid out.
        if self.width < char_width {
            return;
        }
        self.cur_char_width = char_width;
        self.max_smush = self.smush_amount(rows);

        let current_total = self.buffer[0].chars().count() + self.cur_char_width - self.max_smush;

        if code == ' ' as u32 {
            self.blank_markers
                .push((self.buffer.clone(), self.iterator));
        }

        if current_total >= self.width {
            self.handle_new_line();
        } else {
            for row in 0..self.font.height {
                self.add_row(rows, row);
            }
        }
        self.prev_char_width = self.cur_char_width;
    }

    /// Port of `addCurCharRowToBufferRow` + `smushRow`.
    fn add_row(&mut self, rows: &[String], row: usize) {
        let mut add_left: Vec<char> = self.buffer[row].chars().collect();
        let add_right: Vec<char> = rows[row].chars().collect();

        for i in 0..self.max_smush {
            // Index into the left buffer of the character being smushed.
            let idx = add_left.len() as isize - self.max_smush as isize + i as isize;
            let left = if idx >= 0 && (idx as usize) < add_left.len() {
                Some(add_left[idx as usize])
            } else {
                None
            };
            let Some(&right) = add_right.get(i) else {
                continue;
            };
            let smushed = self.smush_chars(left, Some(right));
            if let (Some(smushed), true) = (smushed, idx >= 0) {
                if (idx as usize) < add_left.len() {
                    add_left[idx as usize] = smushed;
                }
            }
        }

        let tail: String = add_right.iter().skip(self.max_smush).collect();
        self.buffer[row] = add_left.into_iter().collect::<String>() + &tail;
    }

    /// Port of `FigletBuilder.smushAmount`.
    fn smush_amount(&self, rows: &[String]) -> usize {
        if self.font.smush_mode & (smush::SMUSH | smush::KERN) == 0 {
            return 0;
        }
        let mut max_smush = self.cur_char_width;
        for (buffered, incoming) in self.buffer.iter().zip(rows).take(self.font.height) {
            let line_left: Vec<char> = buffered.chars().collect();
            let line_right: Vec<char> = incoming.chars().collect();

            // Only ASCII spaces are stripped, matching figlet exactly.
            let trimmed_len = {
                let mut n = line_left.len();
                while n > 0 && line_left[n - 1] == ' ' {
                    n -= 1;
                }
                n
            };
            let mut linebd = trimmed_len.saturating_sub(1);
            let ch1 = if linebd < line_left.len() {
                Some(line_left[linebd])
            } else {
                linebd = 0;
                None
            };

            let mut charbd = line_right.iter().take_while(|c| **c == ' ').count();
            let ch2 = if charbd < line_right.len() {
                Some(line_right[charbd])
            } else {
                charbd = line_right.len();
                None
            };

            // `charbd + len(left) - 1 - linebd`, guarded against underflow.
            let mut amt = charbd as isize + line_left.len() as isize - 1 - linebd as isize;

            // A blank on the left always yields another column; otherwise the
            // two touching characters have to actually smush.
            let blank_left = ch1.is_none() || ch1 == Some(' ');
            if blank_left || (ch2.is_some() && self.smush_chars(ch1, ch2).is_some()) {
                amt += 1;
            }

            let amt = amt.max(0) as usize;
            if amt < max_smush {
                max_smush = amt;
            }
        }
        max_smush
    }

    /// Port of `FigletBuilder.smushChars`: can these two touching characters be
    /// merged, and into what?
    fn smush_chars(&self, left: Option<char>, right: Option<char>) -> Option<char> {
        // Deliberately not `is_whitespace`: figlet treats only ASCII space here.
        if left == Some(' ') {
            return right;
        }
        if right == Some(' ') {
            return left;
        }
        let (left, right) = (left?, right?);

        // No overlapping when either neighbour is under two columns wide.
        if self.prev_char_width < 2 || self.cur_char_width < 2 {
            return None;
        }
        if self.font.smush_mode & smush::SMUSH == 0 {
            return None; // kerning only
        }

        let hard = self.font.hard_blank;

        // Universal overlapping: no specific rule bits set.
        if self.font.smush_mode & 63 == 0 {
            if left == hard {
                return Some(right);
            }
            if right == hard {
                return Some(left);
            }
            // The later character in the user's text dominates.
            return Some(right);
        }

        if self.font.smush_mode & smush::HARDBLANK != 0 && left == hard && right == hard {
            return Some(left);
        }
        if left == hard || right == hard {
            return None;
        }

        if self.font.smush_mode & smush::EQUAL != 0 && left == right {
            return Some(left);
        }

        // Hierarchy rules: a class on the left smushes with any class to its
        // right (and symmetrically).
        let mut classes: Vec<(&str, &str)> = Vec::new();
        if self.font.smush_mode & smush::LOWLINE != 0 {
            classes.push(("_", r"|/\[]{}()<>"));
        }
        if self.font.smush_mode & smush::HIERARCHY != 0 {
            classes.push(("|", r"/\[]{}()<>"));
            classes.push((r"\/", "[]{}()<>"));
            classes.push(("[]", "{}()<>"));
            classes.push(("{}", "()<>"));
            classes.push(("()", "<>"));
        }
        for (a, b) in classes {
            if a.contains(left) && b.contains(right) {
                return Some(right);
            }
            if a.contains(right) && b.contains(left) {
                return Some(left);
            }
        }

        if self.font.smush_mode & smush::PAIR != 0 {
            for pair in [[left, right], [right, left]] {
                let pair: String = pair.iter().collect();
                if matches!(pair.as_str(), "[]" | "{}" | "()") {
                    return Some('|');
                }
            }
        }

        if self.font.smush_mode & smush::BIGX != 0 {
            if left == '/' && right == '\\' {
                return Some('|');
            }
            if right == '/' && left == '\\' {
                return Some('Y');
            }
            if left == '>' && right == '<' {
                return Some('X');
            }
        }
        None
    }

    /// Port of `handleNewLine` + `cutBufferAt*`.
    fn handle_new_line(&mut self) {
        match self.blank_markers.pop() {
            Some((saved_buffer, saved_iterator)) => {
                self.product.push(saved_buffer);
                self.iterator = saved_iterator;
            }
            None => {
                self.product.push(self.buffer.clone());
                self.iterator = self.iterator.saturating_sub(1);
            }
        }
        self.cut_buffer_common();
    }

    fn cut_buffer_common(&mut self) {
        self.buffer = vec![String::new(); self.font.height];
        self.blank_markers.clear();
        self.prev_char_width = 0;
        if let Some(rows) = self.cur_rows() {
            self.max_smush = self.smush_amount(rows);
        }
    }

    /// Port of `justifyString` + `replaceHardblanks` + `formatProduct`.
    fn finish(mut self) -> String {
        if !self.buffer[0].is_empty() {
            self.product.push(self.buffer.clone());
        }
        let mut out = String::new();
        for buffer in &self.product {
            for row in buffer {
                let pad = match self.justify {
                    Justify::Left => 0,
                    Justify::Center => (self.width.saturating_sub(row.chars().count())) / 2,
                    Justify::Right => self.width.saturating_sub(row.chars().count() + 1),
                };
                if pad > 0 {
                    out.push_str(&" ".repeat(pad));
                }
                out.push_str(row);
                out.push('\n');
            }
        }
        out.replace(self.font.hard_blank, " ")
    }
}

/// Render `text` as a FIGlet banner.
///
/// This is the free-function form; see [`Figlet`](crate::Figlet) for the
/// `rich` renderable.
pub fn render(text: &str, font: &FigletFont, width: usize, justify: Justify) -> String {
    let mut builder = Builder::new(text, font, width, justify);
    while builder.iterator < builder.text.len() {
        builder.add_char();
        builder.iterator += 1;
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_bundled_font() {
        let font = FigletFont::standard();
        assert_eq!(font.height(), 6);
        assert_eq!(font.hard_blank(), '$');
        // Space and every printable ASCII character are present.
        assert!(font.rows_for(' ' as u32).is_some());
        assert!(font.rows_for('A' as u32).is_some());
        assert!(font.rows_for('~' as u32).is_some());
    }

    #[test]
    fn rejects_a_non_font() {
        assert_eq!(
            FigletFont::parse("hello world").unwrap_err(),
            FontError::BadSignature
        );
    }

    #[test]
    fn parses_a_synthetic_font() {
        // A hand-written 2-row font with a `#` hardblank and kerning-only
        // layout, exercising the parser independently of any vendored file.
        let mut source = String::from("flf2a# 2 2 4 0 1 0\nsynthetic test font\n");
        for _ in 32..=126 {
            source.push_str("ab@\ncd@@\n");
        }
        let font = FigletFont::parse(&source).expect("valid synthetic font");
        assert_eq!(font.height(), 2);
        assert_eq!(font.hard_blank(), '#');
        assert_eq!(font.rows_for('A' as u32).unwrap(), &["ab", "cd"]);
        // old_layout == 0 means kerning.
        assert_eq!(font.smush_mode, smush::KERN);
    }

    #[test]
    fn strips_end_marks() {
        assert_eq!(strip_end_marks(" $@"), " $");
        assert_eq!(strip_end_marks(" $@@"), " $");
        assert_eq!(strip_end_marks(""), "");
    }

    #[test]
    fn parses_code_tags() {
        assert_eq!(parse_code_tag("196  LATIN CAPITAL"), Some(196));
        assert_eq!(parse_code_tag("0x2C0"), Some(0x2C0));
        assert_eq!(parse_code_tag("0101"), Some(0o101));
        assert_eq!(parse_code_tag("-1"), None);
        assert_eq!(parse_code_tag("notacode"), None);
    }
}
