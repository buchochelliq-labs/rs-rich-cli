//! Terminal cell measurement.
//!
//! Port of upstream `rich/cells.py`. Upstream ships its own width table
//! (`_cell_widths.py`); we delegate to the `unicode-width` crate, which
//! implements the same Unicode East Asian Width rules. Any observed divergence
//! on exotic codepoints is tracked in docs/DIVERGENCES.md.

use unicode_width::UnicodeWidthChar;

/// The number of terminal cells `text` occupies. Port of `cell_len`.
pub fn cell_len(text: &str) -> usize {
    text.chars().map(char_cell_width).sum()
}

/// The cell width of a single character (control chars count as 0).
pub fn char_cell_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Crop `text` to at most `width` cells, never padding. Unlike [`set_cell_size`]
/// this leaves shorter text unchanged. Mirrors `Text.truncate` at the cell level.
pub fn truncate(text: &str, width: usize) -> String {
    if cell_len(text) <= width {
        text.to_string()
    } else {
        set_cell_size(text, width)
    }
}

/// Truncate or right-pad `text` (with spaces) so it occupies exactly `total`
/// cells. Port of `set_cell_size`.
pub fn set_cell_size(text: &str, total: usize) -> String {
    let mut width = 0usize;
    let mut result = String::new();
    for c in text.chars() {
        let cw = char_cell_width(c);
        if width + cw > total {
            break;
        }
        width += cw;
        result.push(c);
    }
    while width < total {
        result.push(' ');
        width += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_len() {
        assert_eq!(cell_len("hello"), 5);
    }

    #[test]
    fn wide_chars_count_double() {
        assert_eq!(cell_len("宽"), 2);
    }

    #[test]
    fn set_size_pads_and_truncates() {
        assert_eq!(set_cell_size("hi", 5), "hi   ");
        assert_eq!(set_cell_size("hello", 3), "hel");
    }
}
