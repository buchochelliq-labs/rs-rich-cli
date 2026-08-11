//! Terminal cell measurement.
//!
//! Port of upstream `rich/cells.py`. Upstream ships its own width table
//! (`_cell_widths.py`); we delegate to the `unicode-width` crate, which
//! implements the same Unicode East Asian Width rules. Any observed divergence
//! on exotic codepoints is tracked in docs/DIVERGENCES.md.

use crate::cell_widths::NARROW_TO_WIDE;

/// The number of terminal cells `text` occupies. Port of `cell_len`.
pub fn cell_len(text: &str) -> usize {
    // Fast path, matching upstream's: without a zero-width joiner or a
    // variation selector nothing can change a character's measured width, so
    // the sum of the per-character widths is the answer.
    if !text.contains(ZERO_WIDTH_JOINER) && !text.contains(VARIATION_SELECTOR_16) {
        return text.chars().map(char_cell_width).sum();
    }

    // Port of upstream `cells._cell_len`'s cluster pass. Two rules matter:
    // a ZWJ consumes the character after it (so a family emoji measures as one
    // emoji, not as its parts), and a variation selector promotes the preceding
    // narrow character to two cells.
    let chars: Vec<char> = text.chars().collect();
    let mut total = 0usize;
    let mut last_measured: Option<char> = None;
    let mut index = 0usize;
    while index < chars.len() {
        let c = chars[index];
        if c == ZERO_WIDTH_JOINER {
            index += 1; // skip the joined character entirely
        } else if c == VARIATION_SELECTOR_16 {
            if let Some(previous) = last_measured.take() {
                if NARROW_TO_WIDE.contains(&previous) {
                    total += 1;
                }
            }
        } else {
            let width = char_cell_width(c);
            if width > 0 {
                last_measured = Some(c);
                total += width;
            }
        }
        index += 1;
    }
    total
}

/// Zero-width joiner: binds emoji into a single cluster.
const ZERO_WIDTH_JOINER: char = '\u{200d}';
/// Variation selector 16: renders the preceding character as emoji (2 cells).
const VARIATION_SELECTOR_16: char = '\u{fe0f}';

/// The cell width of a single character (control chars count as 0).
///
/// Uses upstream's vendored table rather than the `unicode-width` crate: the two
/// disagree on 348 code points (spacing marks, format characters, modifier
/// symbols), and every disagreement misaligns a table, panel or wrap point.
/// Port of `cells.get_character_cell_size`.
pub fn char_cell_width(c: char) -> usize {
    let codepoint = c as u32;
    if (codepoint > 0 && codepoint < 32) || (0x7F..0xA0).contains(&codepoint) {
        return 0;
    }
    let table = &crate::cell_widths::CELL_WIDTH_RANGES;
    // Beyond the table's last range upstream assumes a single cell.
    if codepoint > table[table.len() - 1].1 {
        return 1;
    }
    let mut lower = 0usize;
    let mut upper = table.len() - 1;
    while lower <= upper {
        let mid = (lower + upper) / 2;
        let (start, end, width) = table[mid];
        if codepoint > end {
            lower = mid + 1;
        } else if codepoint < start {
            if mid == 0 {
                break;
            }
            upper = mid - 1;
        } else {
            return width as usize;
        }
    }
    1
}

/// Split `text` into chunks, each at most `width` cells wide. Port of
/// `cells.chop_cells` — char-based, so 0-width combining marks stay attached to
/// their base character (no grapheme table needed). Used to fold over-long words
/// during wrapping. Matches upstream byte-for-byte, including its quirk that a
/// leading character *wider* than `width` (e.g. a 2-cell CJK char folded to
/// width 1) yields an empty leading chunk (`["", "宽", …]`).
pub fn chop_cells(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut size = 0usize;
    for c in text.chars() {
        let cw = char_cell_width(c);
        // No `!line.is_empty()` guard: when the first char of a fresh line is
        // already wider than `width`, upstream appends the empty prefix (and so
        // do we). For normal text (`cw <= width`) this branch never fires on an
        // empty line, so ordinary wrapping is unaffected.
        if size + cw > width {
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
    fn chop_char_wider_than_width_emits_empty_leading_chunk() {
        // Upstream quirk: folding a 2-cell CJK char to width 1 yields an empty
        // leading chunk. Captured from real rich 15.0.0 `chop_cells`:
        //   chop_cells("宽宽", 1) == ["", "宽", "宽"]
        //   chop_cells("宽", 1)   == ["", "宽"]
        //   chop_cells("a宽b", 2) == ["a", "宽", "b"]
        assert_eq!(chop_cells("宽宽", 1), vec!["", "宽", "宽"]);
        assert_eq!(chop_cells("宽", 1), vec!["", "宽"]);
        assert_eq!(chop_cells("a宽b", 2), vec!["a", "宽", "b"]);
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

    /// Upstream vendors its own width table and measures emoji *clusters*: a ZWJ
    /// consumes the character after it and U+FE0F promotes a narrow character to
    /// two cells. Summing per code point from the `unicode-width` crate gave 8
    /// cells for a family emoji and 1 for a heart, misaligning every table and
    /// panel that contained one.
    #[test]
    fn emoji_clusters_measure_as_one_glyph() {
        for (text, expected, what) in [
            ("\u{2764}\u{fe0f}", 2, "heart + VS16"),
            ("\u{26a0}\u{fe0f}", 2, "warning + VS16"),
            (
                "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}",
                2,
                "ZWJ family",
            ),
            ("\u{1f44d}\u{1f3fb}", 2, "thumbs up + skin tone"),
            (
                "\u{1f3f3}\u{fe0f}\u{200d}\u{1f308}",
                2,
                "rainbow flag (ZWJ)",
            ),
            ("1\u{fe0f}\u{20e3}", 2, "keycap"),
        ] {
            assert_eq!(cell_len(text), expected, "{what} measured wrongly");
        }
    }

    /// Spacing marks, format characters and modifier symbols are where the
    /// `unicode-width` crate and upstream disagreed most (312 of 348 mismatches
    /// were Mc spacing marks).
    #[test]
    fn combining_and_modifier_characters_take_no_cells() {
        assert_eq!(
            cell_len("\u{915}\u{93f}"),
            1,
            "Devanagari vowel sign should be zero-width"
        );
        assert_eq!(
            cell_len("\u{1f3fb}"),
            0,
            "a lone skin-tone modifier is zero-width"
        );
        assert_eq!(cell_len("\u{4f60}\u{597d}"), 4, "CJK stayed wide");
        assert_eq!(cell_len("ascii"), 5, "ASCII unaffected");
    }
}
