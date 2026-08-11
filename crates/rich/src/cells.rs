//! Terminal cell measurement.
//!
//! Port of upstream `rich/cells.py`. The width data is upstream's own, vendored
//! into [`cell_widths`](crate::cell_widths) by `scripts/gen_cell_widths.py` —
//! all 21 Unicode versions `rich._unicode_data` ships, selected at runtime by
//! `UNICODE_VERSION` exactly as upstream's `load()` does.
//!
//! We used to delegate to the `unicode-width` crate on the premise that both
//! implement the same East Asian Width rules. They disagree on 348 code points,
//! and a `unicode-width` build can only ever be one Unicode version anyway.

use std::sync::OnceLock;

use crate::cell_widths::{NARROW_TO_WIDE, TABLES, VERSIONS};

/// Parse a Unicode version string into `(major, minor, patch)`, padding missing
/// components with zero and ignoring any beyond the third. Port of
/// `rich._unicode_data._parse_version`; `None` stands in for its `ValueError`.
fn parse_version(version: &str) -> Option<(i64, i64, i64)> {
    let mut parts = [0i64; 3];
    for (index, part) in version.split('.').enumerate() {
        // `.trim()` because Python's `int()` accepts surrounding whitespace and
        // this is a port of `map(int, version.split("."))`.
        let value: i64 = part.trim().parse().ok()?;
        if index < 3 {
            parts[index] = value;
        }
    }
    Some((parts[0], parts[1], parts[2]))
}

/// Index into [`VERSIONS`] of the table upstream would load for `requested`.
/// Port of the version selection in `rich._unicode_data.load`.
///
/// Anything unparsable — including the literal `"latest"` — takes the newest
/// table, and a version upstream does not ship falls back to the newest one
/// *not newer* than it (`bisect_left` minus one, clamped at the oldest).
fn resolve_version(requested: &str) -> usize {
    let latest = VERSIONS.len() - 1;
    let Some(wanted) = parse_version(requested) else {
        return latest;
    };
    let shipped = || {
        VERSIONS
            .iter()
            .map(|version| parse_version(version).expect("shipped versions parse"))
    };
    // An exact match wins: upstream checks the reformatted `major.minor.patch`
    // against its version set before doing anything cleverer, so `"9.0"` finds
    // `"9.0.0"` rather than bisecting to `"8.0.0"`.
    if let Some(index) = shipped().position(|version| version == wanted) {
        return index;
    }
    shipped()
        .position(|version| version >= wanted)
        .unwrap_or(VERSIONS.len())
        .saturating_sub(1)
}

/// The width table this process measures with, chosen by `UNICODE_VERSION`.
///
/// Upstream's `_unicode_data.load("auto")` is `@cache`d, so the variable is read
/// once per process however many strings get measured; the `OnceLock` matches
/// that, and keeps [`char_cell_width`] free of a per-character env lookup.
fn cell_table() -> &'static [(u32, u32, u8)] {
    static TABLE: OnceLock<&'static [(u32, u32, u8)]> = OnceLock::new();
    TABLE.get_or_init(|| {
        // Unset behaves as `"latest"`, exactly as upstream's
        // `os.environ.get("UNICODE_VERSION", "latest")` does.
        let requested = std::env::var("UNICODE_VERSION").unwrap_or_else(|_| "latest".to_string());
        table_for(&requested)
    })
}

/// The table for `requested`, ignoring the environment.
fn table_for(requested: &str) -> &'static [(u32, u32, u8)] {
    TABLES[resolve_version(requested)]
}

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

/// Upstream's `_SINGLE_CELL_UNICODE_RANGES`: code points it is willing to assume
/// occupy exactly one cell each, so a string built only from them can be sliced
/// by code point without consulting the width table at all.
const SINGLE_CELL_RANGES: [(u32, u32); 6] = [
    (0x20, 0x7E),     // Latin (excluding non-printable)
    (0xA0, 0xAC),     // NB: 0xAD (soft hyphen) is deliberately excluded
    (0xAE, 0x2FF),    //
    (0x370, 0x482),   // Greek / Cyrillic
    (0x2500, 0x25FC), // Box drawing, box elements, geometric shapes
    (0x2800, 0x28FF), // Braille
];

/// Whether every character of `text` is one of upstream's assumed-single-cell
/// code points. Port of `cells._is_single_cell_widths`.
///
/// This is not merely an optimisation: [`chop_cells`] and [`set_cell_size`]
/// take a genuinely different code path when it holds, slicing by code point
/// rather than by grapheme, and upstream's output follows whichever path it
/// took. Note that a tab is *not* single-cell (it is zero cells wide), so a
/// tabbed string always takes the grapheme path.
fn is_single_cell_widths(text: &str) -> bool {
    text.chars().all(|c| {
        let codepoint = c as u32;
        SINGLE_CELL_RANGES
            .iter()
            .any(|(start, end)| (*start..=*end).contains(&codepoint))
    })
}

/// Divide `text` into spans that each cover exactly one grapheme, and return the
/// cell length of the whole string alongside. Port of `cells.split_graphemes`.
///
/// Each span is `(start_byte, end_byte, cell_length)`; upstream indexes by code
/// point, we index by byte so the spans slice a `&str` directly. The spans cover
/// every byte with no gaps, and a span's cell length may be zero (a lone joiner,
/// a control code).
///
/// The two rules that make this more than a `char` iterator: a zero-width joiner
/// swallows the character after it, so a family emoji is *one* grapheme; and
/// U+FE0F promotes the preceding narrow character to two cells without starting
/// a new grapheme. Zero-width characters attach to the grapheme before them.
pub fn split_graphemes(text: &str) -> (Vec<(usize, usize, usize)>, usize) {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let count = chars.len();
    // The byte offset of code point `index`, with `count` meaning "the end".
    let byte_at = |index: usize| chars.get(index).map_or(text.len(), |(offset, _)| *offset);

    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    let mut total_width = 0usize;
    let mut last_measured: Option<char> = None;
    let mut index = 0usize;

    while index < count {
        let character = chars[index].1;
        if character == ZERO_WIDTH_JOINER || character == VARIATION_SELECTOR_16 {
            let Some(last) = spans.last_mut() else {
                // A joiner or selector opening the string joins nothing. It is
                // nonsense, but upstream handles it, so we must too.
                let start = byte_at(index);
                index += 1;
                spans.push((start, byte_at(index), 0));
                continue;
            };
            if character == ZERO_WIDTH_JOINER {
                // Consume the joiner *and* whatever it joins — unless it is the
                // last character, with nothing left to join.
                index += if index < count - 1 { 2 } else { 1 };
                last.1 = byte_at(index);
            } else {
                index += 1;
                if last_measured.is_some_and(|previous| NARROW_TO_WIDE.contains(&previous)) {
                    last_measured = None;
                    last.2 += 1;
                    total_width += 1;
                }
                last.1 = byte_at(index);
            }
            continue;
        }

        let start = byte_at(index);
        let width = char_cell_width(character);
        index += 1;
        if width > 0 {
            last_measured = Some(character);
            total_width += width;
            spans.push((start, byte_at(index), width));
        } else if let Some(last) = spans.last_mut() {
            // Zero-width characters belong to the grapheme before them.
            last.1 = byte_at(index);
        } else {
            spans.push((start, byte_at(index), 0));
        }
    }

    (spans, total_width)
}

/// Split `text` at `cell_position` cells. Port of `cells._split_text`.
///
/// A split that lands *inside* a double-width grapheme cannot be represented, so
/// upstream replaces that grapheme with a space on each side of the cut — which
/// is why cropping `"❤️❤️"` to one cell yields `" "`, not half a heart.
fn split_text_inner(text: &str, cell_position: usize) -> (String, String) {
    if cell_position == 0 {
        return (String::new(), text.to_string());
    }
    let (spans, cell_length) = split_graphemes(text);
    if cell_length == 0 || spans.is_empty() {
        // Upstream divides by `cell_length` here and would raise; there is
        // nothing measurable to cut, so the whole string stays on the left.
        return (text.to_string(), String::new());
    }

    // Upstream's initial guess: assume the graphemes are evenly sized, then walk
    // to the true boundary. `as usize` truncates, matching Python's `int()`.
    let mut offset = ((cell_position as f64 / cell_length as f64) * spans.len() as f64) as usize;
    offset = offset.min(spans.len());
    let mut left_size: usize = spans[..offset].iter().map(|span| span.2).sum();

    loop {
        if left_size == cell_position {
            let Some(&(split, _, _)) = spans.get(offset) else {
                return (text.to_string(), String::new());
            };
            return (text[..split].to_string(), text[split..].to_string());
        }
        if left_size < cell_position {
            let Some(&(start, end, cell_size)) = spans.get(offset) else {
                return (text.to_string(), String::new());
            };
            if left_size + cell_size > cell_position {
                return (format!("{} ", &text[..start]), format!(" {}", &text[end..]));
            }
            offset += 1;
            left_size += cell_size;
        } else {
            let Some(&(start, end, cell_size)) =
                offset.checked_sub(1).and_then(|index| spans.get(index))
            else {
                return (String::new(), text.to_string());
            };
            if left_size - cell_size < cell_position {
                return (format!("{} ", &text[..start]), format!(" {}", &text[end..]));
            }
            offset -= 1;
            left_size -= cell_size;
        }
    }
}

/// Split `text` at `cell_position` cells. Port of `cells.split_text`.
pub fn split_text(text: &str, cell_position: usize) -> (String, String) {
    if is_single_cell_widths(text) {
        let split = char_boundary(text, cell_position);
        return (text[..split].to_string(), text[split..].to_string());
    }
    split_text_inner(text, cell_position)
}

/// The byte offset of code point `index`, or the end of `text`.
fn char_boundary(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

/// The cell width of a single character (control chars count as 0).
///
/// Uses upstream's vendored table rather than the `unicode-width` crate: the two
/// disagree on 348 code points (spacing marks, format characters, modifier
/// symbols), and every disagreement misaligns a table, panel or wrap point.
/// Port of `cells.get_character_cell_size`.
pub fn char_cell_width(c: char) -> usize {
    width_in(cell_table(), c)
}

/// [`char_cell_width`] against an explicitly chosen table, so the version
/// selection can be exercised without a process-wide environment variable.
fn width_in(table: &[(u32, u32, u8)], c: char) -> usize {
    let codepoint = c as u32;
    if (codepoint > 0 && codepoint < 32) || (0x7F..0xA0).contains(&codepoint) {
        return 0;
    }
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
/// `cells.chop_cells`. Used to fold over-long words during wrapping.
///
/// The fold is **grapheme**-aware, because a code-point fold cannot see that
/// `"❤️"` is two cells: measured per code point the heart is one cell and the
/// variation selector is zero, so twenty of them fit "within" thirty cells and
/// the row silently renders forty cells wide, punching a hole through whatever
/// border was drawn around it. Upstream splits on [`split_graphemes`]; so do we.
///
/// Two upstream quirks are preserved. A leading grapheme *wider* than `width`
/// yields an empty leading chunk (`chop_cells("宽", 1) == ["", "宽"]`), and a
/// trailing run that measures zero cells is dropped entirely
/// (`chop_cells("\n", 4) == []`).
pub fn chop_cells(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        // Upstream raises `ValueError` here (its slice step is `width`). Nothing
        // in the port can usefully raise, and callers guard on zero width, so
        // hand the text back whole rather than looping forever.
        return vec![text.to_string()];
    }
    if is_single_cell_widths(text) {
        // Upstream's fast path slices by code point, `width` at a time.
        let chars: Vec<char> = text.chars().collect();
        return chars
            .chunks(width)
            .map(|chunk| chunk.iter().collect())
            .collect();
    }

    let (spans, _) = split_graphemes(text);
    let mut lines: Vec<String> = Vec::new();
    let mut line_size = 0usize;
    let mut line_offset = 0usize;
    for (start, _end, cell_size) in spans {
        if line_size + cell_size > width {
            lines.push(text[line_offset..start].to_string());
            line_offset = start;
            line_size = 0;
        }
        line_size += cell_size;
    }
    if line_size > 0 {
        lines.push(text[line_offset..].to_string());
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
///
/// Cropping goes through [`split_text`]'s grapheme walk, so a cut landing inside
/// a two-cell grapheme becomes a space rather than a half-rendered glyph — the
/// difference between upstream's `" "` and a bare `"❤"` that the terminal still
/// draws two cells wide.
pub fn set_cell_size(text: &str, total: usize) -> String {
    if is_single_cell_widths(text) {
        let size = text.chars().count();
        if size < total {
            let mut padded = text.to_string();
            padded.extend(std::iter::repeat_n(' ', total - size));
            return padded;
        }
        return text[..char_boundary(text, total)].to_string();
    }
    if total == 0 {
        return String::new();
    }
    let cell_size = cell_len(text);
    if cell_size == total {
        return text.to_string();
    }
    if cell_size < total {
        let mut padded = text.to_string();
        padded.extend(std::iter::repeat_n(' ', total - cell_size));
        return padded;
    }
    split_text_inner(text, total).0
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

    /// The fold is per **grapheme**, not per code point. Measured per code point
    /// a VS16 heart looks like one cell (heart 1 + selector 0), so twenty of them
    /// "fit" in thirty cells and the row renders forty cells wide — straight
    /// through whatever panel or table border was drawn around it.
    ///
    /// Captured from real rich 15.0.0:
    ///
    /// ```text
    /// [cell_len(c) for c in chop_cells("❤️" * 20, 30)]  == [30, 10]
    /// [cell_len(c) for c in chop_cells(FAMILY * 4, 5)]  == [4, 4]
    /// ```
    #[test]
    fn chop_cells_folds_by_grapheme_not_by_code_point() {
        let hearts = "\u{2764}\u{fe0f}".repeat(20);
        let chunks = chop_cells(&hearts, 30);
        assert_eq!(
            chunks.iter().map(|c| cell_len(c)).collect::<Vec<_>>(),
            vec![30, 10],
            "a VS16 run overflowed its width"
        );
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}".repeat(4);
        assert_eq!(
            chop_cells(&family, 5)
                .iter()
                .map(|c| cell_len(c))
                .collect::<Vec<_>>(),
            vec![4, 4],
            "a ZWJ cluster was split"
        );
    }

    /// Upstream appends the final chunk only `if line_size:` — a tail that
    /// measures zero cells is dropped, not emitted. Real rich 15.0.0:
    /// `chop_cells("\n", 4) == []`.
    #[test]
    fn chop_cells_drops_a_trailing_zero_cell_run() {
        assert_eq!(chop_cells("\n", 4), Vec::<String>::new());
    }

    /// A cut landing inside a two-cell grapheme cannot be represented, so
    /// upstream's `_split_text` swaps that grapheme for a space. Truncating per
    /// code point instead kept the bare `❤` — still two cells on the terminal,
    /// so the crop did not crop. Real rich 15.0.0:
    ///
    /// ```text
    /// set_cell_size("❤️❤️", 1) == " "
    /// set_cell_size("❤️❤️", 3) == "❤️ "
    /// set_cell_size("宽宽", 3)  == "宽 "
    /// ```
    #[test]
    fn set_cell_size_swaps_a_straddled_grapheme_for_a_space() {
        let hearts = "\u{2764}\u{fe0f}\u{2764}\u{fe0f}";
        assert_eq!(set_cell_size(hearts, 1), " ");
        assert_eq!(set_cell_size(hearts, 3), "\u{2764}\u{fe0f} ");
        assert_eq!(set_cell_size("宽宽", 3), "宽 ");
        // Unchanged where the cut is clean.
        assert_eq!(set_cell_size(hearts, 2), "\u{2764}\u{fe0f}");
        assert_eq!(set_cell_size(hearts, 4), hearts);
    }

    /// Spans cover every byte, a ZWJ swallows what follows it, and a variation
    /// selector widens the grapheme before it without starting a new one.
    /// Upstream indexes by code point where we index by byte, so the *shape* is
    /// captured from real rich 15.0.0 and the offsets converted:
    ///
    /// ```text
    /// split_graphemes("a" + FAMILY + "b") == ([(0,1,1), (1,8,2), (8,9,1)], 4)
    /// split_graphemes("❤️")               == ([(0,2,2)], 2)
    /// ```
    #[test]
    fn split_graphemes_clusters_joiners_and_selectors() {
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}";
        let text = format!("a{family}b");
        // The family is 25 bytes: four 4-byte emoji and three 3-byte joiners.
        assert_eq!(
            split_graphemes(&text),
            (vec![(0, 1, 1), (1, 26, 2), (26, 27, 1)], 4)
        );
        assert_eq!(
            split_graphemes("\u{2764}\u{fe0f}"),
            (vec![(0, 6, 2)], 2),
            "heart (3 bytes) + VS16 (3 bytes) is one 2-cell grapheme"
        );
    }

    #[test]
    fn chop_keeps_combining_marks_attached() {
        // base char + U+0301 (combining acute): each grapheme is one cell and
        // the combining mark is 0-width, so `split_graphemes` attaches it to the
        // character before it and the pair never straddles a fold.
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

    /// Upstream ships 21 width tables and picks between them with
    /// `UNICODE_VERSION` (`rich._unicode_data.load`); we only ever measured with
    /// the newest, so a terminal pinned to an older Unicode was mismeasured.
    ///
    /// Every expectation captured from real rich 15.0.0 by setting the variable
    /// and reading `load("auto").unicode_version`:
    ///
    /// ```text
    /// '9' -> 9.0.0     '9.0' -> 9.0.0      '9.0.0.7' -> 9.0.0   ' 9 ' -> 9.0.0
    /// '13.1' -> 13.0.0 '12.1' -> 12.1.0    '18.0.0' -> 17.0.0   '99' -> 17.0.0
    /// '0' -> 4.1.0     '-1' -> 4.1.0       '1.0.0' -> 4.1.0
    /// 'latest' -> 17.0.0  'auto' -> 17.0.0  'banana' -> 17.0.0  '' -> 17.0.0
    /// ```
    ///
    /// Three rules to keep straight: an exact match wins outright (so `"12.1"`
    /// finds `12.1.0` rather than bisecting past it), an unknown version falls
    /// back to the newest table *not newer* than it, and anything unparsable
    /// takes the latest.
    #[test]
    fn unicode_version_selects_upstreams_table() {
        for (requested, expected) in [
            ("9", "9.0.0"),
            ("9.0", "9.0.0"),
            ("9.0.0", "9.0.0"),
            ("9.0.0.7", "9.0.0"),
            (" 9 ", "9.0.0"),
            ("13.1", "13.0.0"),
            ("12.1", "12.1.0"),
            ("12.1.0", "12.1.0"),
            ("0", "4.1.0"),
            ("-1", "4.1.0"),
            ("1.0.0", "4.1.0"),
            ("4.1.0", "4.1.0"),
            ("17.0.0", "17.0.0"),
            ("18.0.0", "17.0.0"),
            ("99", "17.0.0"),
            ("latest", "17.0.0"),
            ("auto", "17.0.0"),
            ("banana", "17.0.0"),
            ("", "17.0.0"),
        ] {
            assert_eq!(
                VERSIONS[resolve_version(requested)],
                expected,
                "UNICODE_VERSION={requested:?} chose the wrong table"
            );
        }
    }

    /// The tables are not interchangeable, which is the whole point of honouring
    /// the variable. Widths from real rich 15.0.0's
    /// `get_character_cell_size(chr(cp), version)`:
    ///
    /// ```text
    ///          4.1.0  8.0.0  9.0.0  12.0.0  17.0.0
    /// U+1F600      1      1      2       2       2   grinning face
    /// U+231A       1      1      2       2       2   watch
    /// U+1F9E0      1      1      1       2       2   brain
    /// U+1FAF0      1      1      1       1       2   hand with index finger
    /// ```
    #[test]
    fn an_older_table_measures_emoji_narrower() {
        for (c, widths) in [
            ('\u{1F600}', [1, 1, 2, 2, 2]),
            ('\u{231A}', [1, 1, 2, 2, 2]),
            ('\u{1F9E0}', [1, 1, 1, 2, 2]),
            ('\u{1FAF0}', [1, 1, 1, 1, 2]),
        ] {
            let measured: Vec<usize> = ["4.1.0", "8.0.0", "9.0.0", "12.0.0", "17.0.0"]
                .iter()
                .map(|version| width_in(table_for(version), c))
                .collect();
            assert_eq!(measured, widths.to_vec(), "{c:?} measured wrongly");
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
