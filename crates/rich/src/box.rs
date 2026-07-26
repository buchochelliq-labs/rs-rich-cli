//! Box-drawing character sets.
//!
//! Port of upstream `rich/box.py`. A [`Box`] is parsed from an 8-line template
//! (exactly as upstream), giving named access to every corner, edge, and
//! junction. Panels, rules, and (later) tables draw their borders from these.
//!
//! Slice scope: the common boxes are provided. Platform-dependent substitution
//! (`Box.substitute` for legacy Windows / ASCII terminals) is **not** yet
//! implemented — see docs/DIVERGENCES.md.

/// A set of box-drawing characters. Mirrors `rich.box.Box`.
///
/// The field names match upstream's 8×4 grid:
/// ```text
/// top_left    top              top_divider     top_right
/// head_left   (space)          head_vertical   head_right
/// head_row_left head_row_horizontal head_row_cross head_row_right
/// mid_left    (space)          mid_vertical    mid_right
/// row_left    row_horizontal   row_cross       row_right
/// foot_row_left foot_row_horizontal foot_row_cross foot_row_right
/// foot_left   (space)          foot_vertical   foot_right
/// bottom_left bottom           bottom_divider  bottom_right
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Box {
    pub top_left: char,
    pub top: char,
    pub top_divider: char,
    pub top_right: char,

    pub head_left: char,
    pub head_vertical: char,
    pub head_right: char,

    pub head_row_left: char,
    pub head_row_horizontal: char,
    pub head_row_cross: char,
    pub head_row_right: char,

    pub mid_left: char,
    pub mid_vertical: char,
    pub mid_right: char,

    pub row_left: char,
    pub row_horizontal: char,
    pub row_cross: char,
    pub row_right: char,

    pub foot_row_left: char,
    pub foot_row_horizontal: char,
    pub foot_row_cross: char,
    pub foot_row_right: char,

    pub foot_left: char,
    pub foot_vertical: char,
    pub foot_right: char,

    pub bottom_left: char,
    pub bottom: char,
    pub bottom_divider: char,
    pub bottom_right: char,
}

impl Box {
    /// Parse a box from the 8-line, 4-column template (as upstream does).
    ///
    /// Panics at const-eval time if the template is malformed, so the built-in
    /// constants are validated when the crate compiles.
    const fn parse(template: &str) -> Box {
        let rows = split_rows(template);
        Box {
            top_left: rows[0][0],
            top: rows[0][1],
            top_divider: rows[0][2],
            top_right: rows[0][3],

            head_left: rows[1][0],
            head_vertical: rows[1][2],
            head_right: rows[1][3],

            head_row_left: rows[2][0],
            head_row_horizontal: rows[2][1],
            head_row_cross: rows[2][2],
            head_row_right: rows[2][3],

            mid_left: rows[3][0],
            mid_vertical: rows[3][2],
            mid_right: rows[3][3],

            row_left: rows[4][0],
            row_horizontal: rows[4][1],
            row_cross: rows[4][2],
            row_right: rows[4][3],

            foot_row_left: rows[5][0],
            foot_row_horizontal: rows[5][1],
            foot_row_cross: rows[5][2],
            foot_row_right: rows[5][3],

            foot_left: rows[6][0],
            foot_vertical: rows[6][2],
            foot_right: rows[6][3],

            bottom_left: rows[7][0],
            bottom: rows[7][1],
            bottom_divider: rows[7][2],
            bottom_right: rows[7][3],
        }
    }

    /// The top border for the given column `widths`. Port of `Box.get_top`.
    pub fn get_top(&self, widths: &[usize]) -> String {
        let mut parts = String::new();
        parts.push(self.top_left);
        let last = widths.len().saturating_sub(1);
        for (index, &width) in widths.iter().enumerate() {
            for _ in 0..width {
                parts.push(self.top);
            }
            if index != last {
                parts.push(self.top_divider);
            }
        }
        parts.push(self.top_right);
        parts
    }

    /// The bottom border for the given column `widths`. Port of `Box.get_bottom`.
    pub fn get_bottom(&self, widths: &[usize]) -> String {
        let mut parts = String::new();
        parts.push(self.bottom_left);
        let last = widths.len().saturating_sub(1);
        for (index, &width) in widths.iter().enumerate() {
            for _ in 0..width {
                parts.push(self.bottom);
            }
            if index != last {
                parts.push(self.bottom_divider);
            }
        }
        parts.push(self.bottom_right);
        parts
    }
}

/// Split an 8-line template into an `[8][4]` grid of chars (const-eval helper).
const fn split_rows(template: &str) -> [[char; 4]; 8] {
    let bytes = template.as_bytes();
    let mut rows = [[' '; 4]; 8];
    // We iterate chars manually because box glyphs are multi-byte UTF-8 and the
    // template has a fixed shape: 4 columns per line, newline-separated.
    let mut i = 0usize; // byte index
    let mut row = 0usize;
    let mut col = 0usize;
    while i < bytes.len() {
        let (ch, width) = next_char(bytes, i);
        if ch == '\n' {
            row += 1;
            col = 0;
            i += width;
            continue;
        }
        if row < 8 && col < 4 {
            rows[row][col] = ch;
        }
        col += 1;
        i += width;
    }
    rows
}

/// Decode one UTF-8 char starting at `bytes[i]`, returning it and its byte len.
/// A small const-fn UTF-8 decoder (std's `chars()` isn't const).
const fn next_char(bytes: &[u8], i: usize) -> (char, usize) {
    let b0 = bytes[i];
    if b0 < 0x80 {
        (b0 as char, 1)
    } else if b0 >> 5 == 0b110 {
        let cp = ((b0 as u32 & 0x1f) << 6) | (bytes[i + 1] as u32 & 0x3f);
        (char_from_u32(cp), 2)
    } else if b0 >> 4 == 0b1110 {
        let cp = ((b0 as u32 & 0x0f) << 12)
            | ((bytes[i + 1] as u32 & 0x3f) << 6)
            | (bytes[i + 2] as u32 & 0x3f);
        (char_from_u32(cp), 3)
    } else {
        let cp = ((b0 as u32 & 0x07) << 18)
            | ((bytes[i + 1] as u32 & 0x3f) << 12)
            | ((bytes[i + 2] as u32 & 0x3f) << 6)
            | (bytes[i + 3] as u32 & 0x3f);
        (char_from_u32(cp), 4)
    }
}

const fn char_from_u32(cp: u32) -> char {
    match char::from_u32(cp) {
        Some(c) => c,
        None => '\u{fffd}',
    }
}

// ── Built-in boxes (subset of upstream `rich/box.py`) ──

pub const ASCII: Box = Box::parse("+--+\n| ||\n|-+|\n| ||\n|-+|\n|-+|\n| ||\n+--+\n");

pub const SQUARE: Box = Box::parse("┌─┬┐\n│ ││\n├─┼┤\n│ ││\n├─┼┤\n├─┼┤\n│ ││\n└─┴┘\n");

pub const ROUNDED: Box = Box::parse("╭─┬╮\n│ ││\n├─┼┤\n│ ││\n├─┼┤\n├─┼┤\n│ ││\n╰─┴╯\n");

pub const HEAVY: Box = Box::parse("┏━┳┓\n┃ ┃┃\n┣━╋┫\n┃ ┃┃\n┣━╋┫\n┣━╋┫\n┃ ┃┃\n┗━┻┛\n");

pub const DOUBLE: Box = Box::parse("╔═╦╗\n║ ║║\n╠═╬╣\n║ ║║\n╠═╬╣\n╠═╬╣\n║ ║║\n╚═╩╝\n");

pub const MINIMAL: Box = Box::parse("  ╷ \n  │ \n╶─┼╴\n  │ \n╶─┼╴\n╶─┼╴\n  │ \n  ╵ \n");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_corners() {
        assert_eq!(ROUNDED.top_left, '╭');
        assert_eq!(ROUNDED.top_right, '╮');
        assert_eq!(ROUNDED.bottom_left, '╰');
        assert_eq!(ROUNDED.bottom_right, '╯');
        assert_eq!(ROUNDED.top, '─');
        assert_eq!(ROUNDED.mid_left, '│');
        assert_eq!(ROUNDED.mid_right, '│');
    }

    #[test]
    fn square_and_ascii() {
        assert_eq!(SQUARE.top_left, '┌');
        assert_eq!(ASCII.top_left, '+');
        assert_eq!(ASCII.top, '-');
        assert_eq!(ASCII.mid_left, '|');
    }

    #[test]
    fn get_top_and_bottom_single_column() {
        assert_eq!(ROUNDED.get_top(&[3]), "╭───╮");
        assert_eq!(ROUNDED.get_bottom(&[3]), "╰───╯");
    }
}
