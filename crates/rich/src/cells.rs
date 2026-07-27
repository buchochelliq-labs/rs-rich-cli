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

/// Split `text` into chunks, each at most `width` cells wide. Port of
/// `cells.chop_cells` — char-based, so 0-width combining marks stay attached to
/// their base character (no grapheme table needed). Used to fold over-long words
/// during wrapping. (Unlike upstream, a leading character wider than `width`
/// doesn't produce an empty leading chunk — see docs/DIVERGENCES.md #5.)
pub fn chop_cells(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut size = 0usize;
    for c in text.chars() {
        let cw = char_cell_width(c);
        if size + cw > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            size = 0;
        }
        line.push(c);
        size += cw;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
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

    #[test]
    fn chop_ascii_and_wide() {
        // Matches real rich 15.0.0 `chop_cells`.
        assert_eq!(chop_cells("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        // Each wide char (2 cells) gets its own chunk at width 3.
        assert_eq!(chop_cells("宽宽宽宽", 3), vec!["宽", "宽", "宽", "宽"]);
    }

    #[test]
    fn chop_keeps_combining_marks_attached() {
        // base char + U+0301 (combining acute): each grapheme is one cell, the
        // combining mark is 0-width, so it stays with its base — exactly as
        // upstream's char-based `chop_cells` does (no grapheme table needed).
        let decomposed: String = "abcdef".chars().flat_map(|c| [c, '\u{301}']).collect();
        let chunks = chop_cells(&decomposed, 3);
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks.iter().map(|c| cell_len(c)).collect::<Vec<_>>(),
            vec![3, 3]
        );
        // Three base chars + three combining marks per chunk.
        assert_eq!(chunks[0].chars().count(), 6);
    }
}
