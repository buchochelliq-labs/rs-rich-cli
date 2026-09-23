//! Segments — the atoms of rendering.
//!
//! Port of upstream `rich/segment.py`. A [`Segment`] is a piece of text with an
//! optional [`Style`]. Everything renderable ultimately becomes a stream of
//! segments, which the [`Console`](crate::console::Console) turns into bytes.
//!
//! Control-code segments carry a `control` flag; the typed control sequences
//! that populate them live in [`control`](crate::control).

use crate::cells::cell_len;
use crate::style::Style;

/// A span of text with an optional style. Mirrors `rich.segment.Segment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub style: Option<Style>,
    /// Whether this segment carries terminal control codes rather than content.
    pub control: bool,
}

impl Segment {
    /// A plain content segment.
    pub fn new(text: impl Into<String>, style: Option<Style>) -> Self {
        Segment {
            text: text.into(),
            style,
            control: false,
        }
    }

    /// A newline segment (`Segment.line()` upstream).
    pub fn line() -> Self {
        Segment {
            text: "\n".to_string(),
            style: None,
            control: false,
        }
    }

    /// A control segment (carries no visible width).
    pub fn control(text: impl Into<String>) -> Self {
        Segment {
            text: text.into(),
            style: None,
            control: true,
        }
    }

    /// The number of terminal cells this segment occupies (0 for control).
    pub fn cell_length(&self) -> usize {
        if self.control {
            0
        } else {
            cell_len(&self.text)
        }
    }

    /// Merge adjacent segments that share the same style and control flag.
    /// Port of `Segment.simplify`.
    pub fn simplify(segments: &[Segment]) -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::with_capacity(segments.len());
        for segment in segments {
            match out.last_mut() {
                Some(last) if last.style == segment.style && last.control == segment.control => {
                    last.text.push_str(&segment.text);
                }
                _ => out.push(segment.clone()),
            }
        }
        out
    }

    /// Apply `style` as a base *under* each segment's own style (that segment's
    /// style wins on top). Control segments are left untouched. Port of
    /// `Segment.apply_style` (the `style`-only path).
    ///
    /// Line-break segments (`"\n"`) are also left unstyled: upstream's
    /// line-oriented print pipeline re-emits row separators plain, so styling
    /// them would add stray SGR runs around every newline.
    pub fn apply_style(segments: &[Segment], style: &Style) -> Vec<Segment> {
        segments
            .iter()
            .map(|segment| {
                if segment.control || segment.text == "\n" {
                    segment.clone()
                } else {
                    let combined = match &segment.style {
                        Some(own) => style.combine(own),
                        None => style.clone(),
                    };
                    Segment {
                        text: segment.text.clone(),
                        style: Some(combined),
                        control: false,
                    }
                }
            })
            .collect()
    }

    /// Split a flat segment stream into lines, breaking on `\n`.
    ///
    /// Port of `Segment.split_lines`. Newline characters are consumed (not kept
    /// in the output); a trailing newline yields a final empty line only if
    /// there was content after the last break.
    pub fn split_lines(segments: &[Segment]) -> Vec<Vec<Segment>> {
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        let mut current: Vec<Segment> = Vec::new();
        for segment in segments {
            if segment.control || !segment.text.contains('\n') {
                if !segment.text.is_empty() {
                    current.push(segment.clone());
                }
                continue;
            }
            let mut parts = segment.text.split('\n').peekable();
            while let Some(part) = parts.next() {
                if !part.is_empty() {
                    current.push(Segment::new(part, segment.style.clone()));
                }
                if parts.peek().is_some() {
                    // The break between parts closes the current line.
                    lines.push(std::mem::take(&mut current));
                }
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }

    /// Shape a set of lines into exactly `height` rows of `width` cells: crop
    /// extra rows, pad each row to `width`, and append blank rows to reach
    /// `height`. Port of `Segment.set_shape` (`style=None`, `new_lines=False`).
    pub fn set_shape(lines: Vec<Vec<Segment>>, width: usize, height: usize) -> Vec<Vec<Segment>> {
        let mut shaped: Vec<Vec<Segment>> = lines
            .into_iter()
            .take(height)
            .map(|line| Segment::adjust_line_length(&line, width, None))
            .collect();
        while shaped.len() < height {
            shaped.push(vec![Segment::new(" ".repeat(width), None)]);
        }
        shaped
    }

    /// Fold every line to at most `width` cells, breaking at **word boundaries**
    /// the way upstream's word wrapping does.
    ///
    /// [`fold_lines`](Self::fold_lines) breaks wherever the row happens to fill
    /// up, which splits identifiers and words mid-character-run
    /// (`epsilon, z` / `eta, eta, theta)`). Upstream's `word_wrap=True` routes
    /// through `_wrap.divide_line`, which we already port for `Text` — this
    /// applies the same break offsets to a styled segment run, so styles survive
    /// the split.
    ///
    /// A word longer than `width` is still folded mid-word; there is nowhere
    /// else to break it.
    pub fn fold_lines_words(segments: &[Segment], width: usize) -> Vec<Segment> {
        if width == 0 {
            return segments.to_vec();
        }
        let mut out = Vec::new();
        let lines = Self::split_lines(segments);
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            let plain: String = line
                .iter()
                .filter(|segment| !segment.control)
                .map(|segment| segment.text.as_str())
                .collect();
            let breaks = crate::wrap::divide_line(&plain, width, true);

            let mut char_pos = 0usize;
            let mut next_break = 0usize;
            for segment in line {
                if segment.control {
                    out.push(segment);
                    continue;
                }
                let mut buf = String::new();
                for ch in segment.text.chars() {
                    while next_break < breaks.len() && char_pos == breaks[next_break] {
                        if !buf.is_empty() {
                            out.push(Segment::new(buf.clone(), segment.style.clone()));
                            buf.clear();
                        }
                        out.push(Segment::line());
                        next_break += 1;
                    }
                    buf.push(ch);
                    char_pos += 1;
                }
                if !buf.is_empty() {
                    out.push(Segment::new(buf, segment.style.clone()));
                }
            }
            if index != last {
                out.push(Segment::line());
            }
        }
        out
    }

    /// Fold every line to at most `width` cells, carrying the overflow onto
    /// continuation lines instead of discarding it.
    ///
    /// [`crop_lines`](Self::crop_lines) is the display backstop and **throws the
    /// remainder away** — correct for a renderable that has already wrapped
    /// itself, and data loss for one that emits a long line verbatim. Styles are
    /// preserved across the split.
    ///
    /// This breaks wherever the row happens to fill up, so it splits words. That
    /// is right only for content upstream folds *without* word wrapping. A
    /// renderable whose upstream counterpart goes through `Text.wrap` wants
    /// [`fold_lines_words`](Self::fold_lines_words) instead: reaching for this
    /// one is what made `--json` print `over t` / `he lazy` where rich prints
    /// `over ` / `the lazy`.
    pub fn fold_lines(segments: &[Segment], width: usize) -> Vec<Segment> {
        if width == 0 {
            return segments.to_vec();
        }
        let mut out = Vec::new();
        let lines = Self::split_lines(segments);
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            let mut used = 0usize;
            for segment in line {
                if segment.control {
                    out.push(segment);
                    continue;
                }
                // Walk the segment in cell-sized pieces, breaking whenever the
                // current row is full.
                let mut remaining = segment.text.as_str();
                while !remaining.is_empty() {
                    let room = width.saturating_sub(used);
                    if room == 0 {
                        out.push(Segment::line());
                        used = 0;
                        continue;
                    }
                    let chunks = crate::cells::chop_cells(remaining, room);
                    let mut head = chunks.first().cloned().unwrap_or_default();
                    if head.is_empty() {
                        if used > 0 {
                            // The row has content but no space for this
                            // character; start a fresh one and try again.
                            out.push(Segment::line());
                            used = 0;
                            continue;
                        }
                        // Already at the start of a row and the glyph STILL does
                        // not fit — a 2-cell glyph at width 1. Emit it anyway,
                        // overflowing by a cell.
                        //
                        // A whole *grapheme*, not a single code point: taking one
                        // code point off `"❤️"` emits a bare `❤` and leaves a
                        // stranded variation selector to be emitted on the next
                        // row, where it silently re-widens whatever character
                        // precedes it.
                        //
                        // Retrying here instead was an infinite loop that
                        // allocated a line break per iteration: ~400 MB/s until
                        // the process was killed. Every branch of this loop must
                        // consume input.
                        let (spans, _) = crate::cells::split_graphemes(remaining);
                        let take = spans.first().map_or(remaining.len(), |span| span.1);
                        head = remaining[..take].to_string();
                    }
                    used += crate::cells::cell_len(&head);
                    remaining = &remaining[head.len()..];
                    out.push(Segment::new(head, segment.style.clone()));
                    if !remaining.is_empty() {
                        out.push(Segment::line());
                        used = 0;
                    }
                }
            }
            if index != last {
                out.push(Segment::line());
            }
        }
        out
    }

    /// Crop every line in a segment stream to at most `width` cells, discarding
    /// the excess and leaving short lines alone.
    ///
    /// Port of `Segment.split_and_crop_lines` with `pad=False`, which is what
    /// `Console.print(crop=True)` applies to the finished stream. It is the only
    /// thing standing between an [`Overflow::Ignore`](crate::console::Overflow)
    /// text and a line that runs off the side of the terminal.
    ///
    /// Control segments occupy no cells and are always kept, so cursor moves and
    /// hyperlink codes survive a crop.
    pub fn crop_lines(segments: &[Segment], width: usize) -> Vec<Segment> {
        let mut result: Vec<Segment> = Vec::with_capacity(segments.len());
        let mut used = 0usize;
        for segment in segments {
            if segment.control {
                result.push(segment.clone());
                continue;
            }
            if segment.text == "\n" {
                used = 0;
                result.push(segment.clone());
                continue;
            }
            let length = segment.cell_length();
            if used + length <= width {
                used += length;
                result.push(segment.clone());
            } else if used < width {
                // Straddles the crop: keep the part that fits. A wide character
                // across the boundary is dropped and the gap padded, as
                // `set_cell_size` does everywhere else.
                result.push(Segment::new(
                    crate::cells::set_cell_size(&segment.text, width - used),
                    segment.style.clone(),
                ));
                used = width;
            }
            // Anything else is wholly past the crop, so it is dropped.
        }
        result
    }

    /// Pad (with a styled space run) or crop a single line to exactly `length`
    /// cells. Port of `Segment.adjust_line_length`.
    pub fn adjust_line_length(
        line: &[Segment],
        length: usize,
        style: Option<Style>,
    ) -> Vec<Segment> {
        let line_length: usize = line.iter().map(Segment::cell_length).sum();
        if line_length == length {
            line.to_vec()
        } else if line_length < length {
            let mut new_line = line.to_vec();
            new_line.push(Segment::new(" ".repeat(length - line_length), style));
            new_line
        } else {
            // Crop from the left, honoring cell widths.
            let mut new_line: Vec<Segment> = Vec::new();
            let mut remaining = length;
            for segment in line {
                let seg_len = segment.cell_length();
                if seg_len <= remaining {
                    new_line.push(segment.clone());
                    remaining -= seg_len;
                } else {
                    let cropped = crate::cells::set_cell_size(&segment.text, remaining);
                    new_line.push(Segment::new(cropped, segment.style.clone()));
                    break;
                }
            }
            new_line
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cropping is per line, leaves short lines alone, and keeps zero-width
    /// control segments so cursor moves survive.
    #[test]
    fn crop_lines_cuts_each_line_independently() {
        let segments = vec![
            Segment::new("hello world", None),
            Segment::line(),
            Segment::new("hi", None),
            Segment::line(),
            Segment::control("\x1b[2A"),
            Segment::new("abcdefgh", None),
        ];
        let cropped = Segment::crop_lines(&segments, 5);
        let texts: Vec<&str> = cropped.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["hello", "\n", "hi", "\n", "\x1b[2A", "abcde"]);
    }

    /// A wide character straddling the crop is dropped whole and its cell padded,
    /// so the line still occupies exactly the requested width.
    #[test]
    fn crop_lines_pads_a_split_wide_character() {
        let segments = vec![Segment::new("aa你好", None)];
        let cropped = Segment::crop_lines(&segments, 5);
        assert_eq!(cropped[0].text, "aa你 ");
    }

    /// A crop boundary falling between segments keeps the styles of the ones it
    /// kept and drops the rest entirely.
    #[test]
    fn crop_lines_preserves_styles_and_drops_the_tail() {
        let bold = Style::parse("bold").unwrap();
        let segments = vec![
            Segment::new("abc", Some(bold.clone())),
            Segment::new("defgh", None),
        ];
        let cropped = Segment::crop_lines(&segments, 3);
        assert_eq!(cropped.len(), 1);
        assert_eq!(cropped[0].text, "abc");
        assert_eq!(cropped[0].style, Some(bold));
    }

    #[test]
    fn cell_length_ignores_control() {
        assert_eq!(Segment::new("abc", None).cell_length(), 3);
        assert_eq!(Segment::control("\x1b[2J").cell_length(), 0);
    }

    #[test]
    fn fold_lines_carries_the_overflow_instead_of_dropping_it() {
        let segments = vec![Segment::new("abcdefghij", None)];
        let folded = Segment::fold_lines(&segments, 4);
        let text: String = folded.iter().map(|s| s.text.as_str()).collect();
        // Every character survives; only line breaks are added.
        assert_eq!(text.replace('\n', ""), "abcdefghij");
        assert_eq!(Segment::split_lines(&folded).len(), 3);
    }

    #[test]
    fn fold_lines_preserves_styles_across_a_break() {
        let style = Style::parse("bold").expect("valid style");
        let segments = vec![Segment::new("abcdef", Some(style.clone()))];
        let folded = Segment::fold_lines(&segments, 3);
        for segment in folded.iter().filter(|s| !s.text.contains('\n')) {
            assert_eq!(segment.style.as_ref(), Some(&style), "style lost on fold");
        }
    }

    /// A glyph wider than the whole row is emitted anyway, overflowing — but as
    /// a whole grapheme. Taking a single code point off `"❤️"` put the bare `❤`
    /// on one row and stranded the variation selector at the start of the next,
    /// where it silently re-widens whatever character follows it.
    #[test]
    fn fold_lines_never_splits_a_grapheme() {
        let heart = "\u{2764}\u{fe0f}";
        let segments = vec![Segment::new(heart.repeat(3), None)];
        let folded = Segment::fold_lines(&segments, 1);
        let rows: Vec<String> = Segment::split_lines(&folded)
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(rows, vec![heart, heart, heart]);
    }

    #[test]
    fn crop_lines_still_drops_the_overflow() {
        // fold_lines is the alternative, not a replacement: crop stays the
        // display backstop for renderables that already wrapped themselves.
        let segments = vec![Segment::new("abcdefghij", None)];
        let cropped = Segment::crop_lines(&segments, 4);
        let text: String = cropped.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "abcd");
    }

    #[test]
    fn fold_lines_terminates_when_a_glyph_is_wider_than_the_width() {
        // A 2-cell character with 1 column available used to loop forever,
        // pushing a line break per iteration (~400 MB/s until killed). Every
        // branch of the fold loop must consume input.
        let segments = vec![Segment::new("\u{4f60}\u{4f60}", None)];
        let folded = Segment::fold_lines(&segments, 1);
        let text: String = folded.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            text.matches('\u{4f60}').count(),
            2,
            "both characters should survive, overflowing rather than looping"
        );
    }

    /// The word-wrapping fold breaks *between* words, leaving the space that
    /// separated them at the end of the finished row — exactly where
    /// `_wrap.divide_line` puts the offset.
    #[test]
    fn fold_lines_words_breaks_between_words() {
        let segments = vec![Segment::new("the quick brown fox", None)];
        // 12, not 10: at 10 a character fold would land on the same boundary by
        // luck and the test would pass either way.
        let folded = Segment::fold_lines_words(&segments, 12);
        let lines: Vec<String> = Segment::split_lines(&folded)
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(lines, vec!["the quick ", "brown fox"]);
    }

    /// A break landing inside a styled run must not drop the style, or a wrapped
    /// JSON string would lose its colour halfway down.
    #[test]
    fn fold_lines_words_preserves_styles_across_a_break() {
        let green = Style::parse("green").expect("valid style");
        let segments = vec![
            Segment::new("key: ", None),
            Segment::new("alpha beta gamma", Some(green.clone())),
        ];
        let folded = Segment::fold_lines_words(&segments, 12);
        let styled: String = folded
            .iter()
            .filter(|s| s.style.as_ref() == Some(&green))
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(styled, "alpha beta gamma", "style lost across the break");
    }

    /// Nothing may be dropped: a word wider than the row still has to fold, and
    /// the offsets have to line up with the segments they cut.
    #[test]
    fn fold_lines_words_keeps_every_character() {
        let segments = vec![
            Segment::new("short ", None),
            Segment::new("z".repeat(25), None),
            Segment::new(" tail", None),
        ];
        for width in 1..=30 {
            let folded = Segment::fold_lines_words(&segments, width);
            let text: String = folded.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(
                text.replace('\n', ""),
                format!("short {} tail", "z".repeat(25)),
                "width {width} lost or reordered characters"
            );
        }
    }

    /// Control segments carry no cells, so they must ride through untouched
    /// rather than count against the width or vanish.
    #[test]
    fn fold_lines_words_keeps_control_segments() {
        let segments = vec![
            Segment::control("\x1b]8;;http://x\x1b\\"),
            Segment::new("alpha beta", None),
        ];
        let folded = Segment::fold_lines_words(&segments, 8);
        assert_eq!(folded.iter().filter(|s| s.control).count(), 1);
        let text: String = folded
            .iter()
            .filter(|s| !s.control)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(text, "alpha \nbeta");
    }

    #[test]
    fn fold_lines_terminates_at_every_narrow_width() {
        // Mixed widths: ASCII, CJK, and an emoji, folded at each width from 1.
        let sample = "a\u{4f60}b\u{1f600}c";
        for width in 1..=6 {
            let folded = Segment::fold_lines(&[Segment::new(sample, None)], width);
            let text: String = folded.iter().map(|s| s.text.as_str()).collect();
            assert!(
                text.contains('c'),
                "width {width} lost the tail, or did not terminate"
            );
        }
    }
}
