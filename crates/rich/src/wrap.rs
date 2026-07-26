//! Word wrapping.
//!
//! Port of upstream `rich/_wrap.py`. [`divide_line`] returns the offsets (in
//! **char** indices, matching upstream) at which a string should be split to fit
//! within a cell width. [`Text`](crate::text::Text) uses it to wrap.

use crate::cells::{cell_len, chop_cells};

/// Yield each word as `(start_char, end_char, word)` where a "word" is any
/// leading whitespace + a non-whitespace run + its trailing whitespace.
/// Port of `_wrap.words` (regex `\s*\S+\s*`).
fn words(text: &str) -> Vec<(usize, usize, String)> {
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
            // Only whitespace remained: the `\S+` part can't match, so no word.
            break;
        }
        while pos < n && !chars[pos].is_whitespace() {
            pos += 1;
        }
        while pos < n && chars[pos].is_whitespace() {
            pos += 1;
        }
        let word: String = chars[start..pos].iter().collect();
        result.push((start, pos, word));
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
        let word_length = cell_len(word.trim_end());
        let remaining_space = width.saturating_sub(cell_offset);
        let word_fits_remaining_space = remaining_space >= word_length;

        if word_fits_remaining_space {
            cell_offset += cell_len(&word);
        } else if word_length > width {
            // The word doesn't fit on any line.
            if fold {
                let folded_word = chop_cells(&word, width);
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
                cell_offset = cell_len(&word);
            }
        } else if cell_offset != 0 && start != 0 {
            // Fits on the next (empty) line.
            break_positions.push(start);
            cell_offset = cell_len(&word);
        }
    }

    break_positions
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
