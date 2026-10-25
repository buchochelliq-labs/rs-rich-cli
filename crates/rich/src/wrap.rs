//! Word wrapping.
//!
//! Port of upstream `rich/_wrap.py`. [`divide_line`] returns the offsets (in
//! **char** indices, matching upstream) at which a string should be split to fit
//! within a cell width. [`Text`](crate::text::Text) uses it to wrap.

use std::iter::Peekable;
use std::str::CharIndices;

use crate::cells::{cell_len, chop_cells};

/// Yield each word as `(start_char, end_char, word)` where a "word" is any
/// leading whitespace + a non-whitespace run + its trailing whitespace.
/// Port of `_wrap.words` (regex `\s*\S+\s*`).
///
/// The word is a slice of `text` and the scan walks it once, rather than
/// collecting its characters and each word into new strings
/// (docs/PORTING.md, "Performance notes").
fn words(text: &str) -> Vec<(usize, usize, &str)> {
    // Step past a run of whitespace (`space`) or of anything else, counting
    // the characters into `index`.
    fn skip(chars: &mut Peekable<CharIndices>, index: &mut usize, space: bool) {
        while chars.next_if(|(_, c)| c.is_whitespace() == space).is_some() {
            *index += 1;
        }
    }
    if text.is_ascii() {
        return ascii_words(text);
    }
    let mut result = Vec::new();
    let mut chars = text.char_indices().peekable();
    // The char index of the next character `chars` yields.
    let mut index = 0usize;
    while let Some(&(start_byte, _)) = chars.peek() {
        let start = index;
        skip(&mut chars, &mut index, true);
        if chars.peek().is_none() {
            // Only whitespace remained: the `\S+` part can't match, so no word.
            break;
        }
        skip(&mut chars, &mut index, false);
        skip(&mut chars, &mut index, true);
        let end_byte = chars.peek().map_or(text.len(), |(byte, _)| *byte);
        result.push((start, index, &text[start_byte..end_byte]));
    }
    result
}

/// [`words`] of ASCII text, where a byte is a character: the same scan over
/// bytes, with `char::is_whitespace`'s ASCII members (space, tab, line
/// feed, vertical tab, form feed, carriage return).
fn ascii_words(text: &str) -> Vec<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let space = |at: usize| matches!(bytes[at], b' ' | b'\t'..=b'\r');
    let mut result = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let start = pos;
        while pos < bytes.len() && space(pos) {
            pos += 1;
        }
        if pos == bytes.len() {
            break;
        }
        while pos < bytes.len() && !space(pos) {
            pos += 1;
        }
        while pos < bytes.len() && space(pos) {
            pos += 1;
        }
        result.push((start, pos, &text[start..pos]));
    }
    result
}

/// Return the char offsets at which `text` should break to fit `width` cells.
///
/// Direct port of `_wrap.divide_line`. With `fold`, words longer than `width`
/// are folded across lines; otherwise such a word is simply moved to its own
/// line (and later cropped by the caller).
pub fn divide_line(text: &str, width: usize, fold: bool) -> Vec<usize> {
    let mut break_positions: Vec<usize> = Vec::new();
    let mut cell_offset = 0usize;

    for (start, _end, word) in words(text) {
        let trimmed = word.trim_end();
        let unjoined = crate::cells::unjoined_cell_len(trimmed);
        let word_length = unjoined.unwrap_or_else(|| cell_len(trimmed));
        // `cell_len(word)`: with no joiner or selector in the word, its
        // trailing whitespace only adds its own width, so the word is not
        // measured twice (docs/PORTING.md, "Performance notes").
        let whole_length = || match unjoined {
            Some(length) => length + cell_len(&word[trimmed.len()..]),
            None => cell_len(word),
        };
        // Signed, because upstream's `width - cell_offset` goes NEGATIVE once a
        // line has overshot (an unfoldable word wider than the whole width bumps
        // `cell_offset` past `width`). Clamping that to zero made a following
        // zero-cell word — a word joiner, a lone variation selector — "fit" in
        // the nothing that was left, so its break was never emitted and the rest
        // of the line vanished from the output.
        let remaining_space = width as isize - cell_offset as isize;
        let word_fits_remaining_space = remaining_space >= word_length as isize;

        if word_fits_remaining_space {
            cell_offset += whole_length();
        } else if word_length > width {
            // The word doesn't fit on any line.
            if fold {
                let folded_word = chop_cells(word, width);
                let last_index = folded_word.len().saturating_sub(1);
                let mut start = start;
                for (index, line) in folded_word.iter().enumerate() {
                    if start != 0 {
                        break_positions.push(start);
                    }
                    if index == last_index {
                        cell_offset = cell_len(line);
                    } else {
                        start += line.chars().count();
                    }
                }
            } else {
                if start != 0 {
                    break_positions.push(start);
                }
                cell_offset = whole_length();
            }
        } else if cell_offset != 0 && start != 0 {
            // Fits on the next (empty) line.
            break_positions.push(start);
            cell_offset = whole_length();
        }
    }

    break_positions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `words` as first ported, collecting characters: the slices the
    /// faster scan returns are its words, at its offsets.
    #[test]
    fn words_match_the_collecting_scan() {
        fn reference(text: &str) -> Vec<(usize, usize, String)> {
            let chars: Vec<char> = text.chars().collect();
            let n = chars.len();
            let mut result = Vec::new();
            let mut pos = 0;
            while pos < n {
                let start = pos;
                while pos < n && chars[pos].is_whitespace() {
                    pos += 1;
                }
                if pos >= n {
                    break;
                }
                while pos < n && !chars[pos].is_whitespace() {
                    pos += 1;
                }
                while pos < n && chars[pos].is_whitespace() {
                    pos += 1;
                }
                result.push((start, pos, chars[start..pos].iter().collect()));
            }
            result
        }
        const ALPHABET: [char; 13] = [
            'a', ' ', '\t', '\n', 'é', '界', '\u{3000}', '\u{1c}', 'x', '\u{200d}', '\u{b}',
            '\u{c}', '\r',
        ];
        let mut seed = 0x2545_f491u32;
        for length in 0..40 {
            for _ in 0..50 {
                let text: String = (0..length)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        ALPHABET[seed as usize % ALPHABET.len()]
                    })
                    .collect();
                let fast: Vec<(usize, usize, String)> = words(&text)
                    .into_iter()
                    .map(|(start, end, word)| (start, end, word.to_string()))
                    .collect();
                assert_eq!(fast, reference(&text), "{text:?}");
                // And its ASCII characters alone, which take the byte scan.
                let ascii: String = text.chars().filter(char::is_ascii).collect();
                let fast: Vec<(usize, usize, String)> = words(&ascii)
                    .into_iter()
                    .map(|(start, end, word)| (start, end, word.to_string()))
                    .collect();
                assert_eq!(fast, reference(&ascii), "{ascii:?}");
            }
        }
    }

    #[test]
    fn no_break_when_it_fits() {
        assert_eq!(divide_line("hello world", 20, true), Vec::<usize>::new());
    }

    #[test]
    fn breaks_between_words() {
        // "hello " is 6 cells; "world" doesn't fit in width 8 → break at char 6.
        assert_eq!(divide_line("hello world", 8, true), vec![6]);
    }

    #[test]
    fn folds_overlong_word() {
        // A 10-char word folded at width 4 → breaks every 4 chars.
        assert_eq!(divide_line("abcdefghij", 4, true), vec![4, 8]);
    }

    /// An unfoldable word wider than the line pushes `cell_offset` **past** the
    /// width, so upstream's `width - cell_offset` is negative and nothing fits
    /// on that line any more. Computing it with `saturating_sub` clamped it to
    /// zero, and a following word measuring zero cells — a word joiner, a lone
    /// variation selector — then "fit" in the nothing that was left, so its
    /// break was never emitted and the rest of the text disappeared.
    ///
    /// Real rich 15.0.0: `divide_line("aaaaaaaa ⁠", 2, fold=False) == [9]`.
    #[test]
    fn a_zero_cell_word_after_an_overflowing_line_still_breaks() {
        assert_eq!(divide_line("aaaaaaaa \u{2060}", 2, false), vec![9]);
    }

    /// The grapheme-aware fold, seen from `divide_line`: at width 4 a run of
    /// 2-cell VS16 hearts breaks every two hearts (four code points), not every
    /// four. Real rich 15.0.0: `divide_line("❤️" * 6, 4) == [4, 8]`.
    #[test]
    fn folds_an_emoji_run_by_cell_width() {
        assert_eq!(
            divide_line(&"\u{2764}\u{fe0f}".repeat(6), 4, true),
            vec![4, 8]
        );
    }
}
