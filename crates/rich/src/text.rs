//! Styled text with spans.
//!
//! Port of upstream `rich/text.py` (core subset). [`Text`] is a plain string
//! plus a list of [`Span`]s, each applying a [`Style`] to a byte range. Spans
//! may overlap and nest; [`Text::render`] flattens them into non-overlapping
//! [`Segment`]s by combining every span covering each run.

use crate::cells::cell_len;
use crate::color::ColorSystem;
use crate::console::Justify;
use crate::errors::Result;
use crate::markup;
use crate::segment::Segment;
use crate::style::Style;
use crate::theme::Theme;

/// A style applied to a byte range `[start, end)` of a [`Text`]'s plain string.
/// Mirrors `rich.text.Span`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: Style,
}

/// Styled text. Mirrors `rich.text.Text`.
#[derive(Debug, Clone, Default)]
pub struct Text {
    plain: String,
    spans: Vec<Span>,
    /// A base style applied to the whole text.
    style: Style,
    /// How lines are justified within the render width.
    justify: Justify,
}

impl Text {
    /// Plain, unstyled text.
    pub fn new(plain: impl Into<String>) -> Self {
        Text {
            plain: plain.into(),
            spans: Vec::new(),
            style: Style::new(),
            justify: Justify::Default,
        }
    }

    /// Text with a base style.
    pub fn styled(plain: impl Into<String>, style: Style) -> Self {
        Text {
            plain: plain.into(),
            spans: Vec::new(),
            style,
            justify: Justify::Default,
        }
    }

    /// Set how lines are justified within the render width (builder form).
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Set how lines are justified within the render width.
    pub fn set_justify(&mut self, justify: Justify) {
        self.justify = justify;
    }

    /// This text's own justify method.
    pub fn get_justify(&self) -> Justify {
        self.justify
    }

    /// Build styled text from console markup, resolving tags against `theme`.
    /// Port of `Text.from_markup`.
    pub fn from_markup(markup_text: &str, theme: &Theme) -> Result<Text> {
        markup::render(markup_text, theme)
    }

    /// The unstyled string content.
    pub fn plain(&self) -> &str {
        &self.plain
    }

    /// The spans currently applied.
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Length in terminal cells.
    pub fn cell_len(&self) -> usize {
        cell_len(&self.plain)
    }

    /// True when there is no content.
    pub fn is_empty(&self) -> bool {
        self.plain.is_empty()
    }

    /// Append more text, optionally under `style`, returning the byte range added.
    pub fn append(&mut self, text: &str, style: Option<Style>) {
        let start = self.plain.len();
        self.plain.push_str(text);
        let end = self.plain.len();
        if let Some(style) = style {
            self.spans.push(Span { start, end, style });
        }
    }

    /// Append another `Text`, carrying over its base style (as a covering span)
    /// and all of its spans, shifted to their new offsets. Port of
    /// `Text.append_text`. Consumes `self` and returns it for chaining.
    pub fn append_text(mut self, other: &Text) -> Text {
        let offset = self.plain.len();
        self.plain.push_str(&other.plain);
        let end = self.plain.len();
        if !other.style.is_null() {
            self.spans.push(Span {
                start: offset,
                end,
                style: other.style.clone(),
            });
        }
        for span in &other.spans {
            self.spans.push(Span {
                start: span.start + offset,
                end: span.end + offset,
                style: span.style.clone(),
            });
        }
        self
    }

    /// Apply `style` to the byte range `[start, end)`. Port of `Text.stylize`
    /// (byte offsets; ASCII-only callers such as highlighters are unaffected by
    /// the char-vs-byte distinction).
    pub fn stylize(&mut self, start: usize, end: usize, style: Style) {
        let end = end.min(self.plain.len());
        if start >= end {
            return;
        }
        self.spans.push(Span { start, end, style });
    }

    /// Push a raw span (used by the markup parser).
    pub(crate) fn push_span(&mut self, span: Span) {
        self.spans.push(span);
    }

    /// Set the whole-text base style.
    pub fn set_base_style(&mut self, style: Style) {
        self.style = style;
    }

    /// Flatten into non-overlapping segments (newlines become [`Segment::line`]),
    /// combining `base_style`, this text's base style, and every covering span.
    /// Does **not** wrap. Port of the core of `Text.render`.
    ///
    /// `system` is accepted for API symmetry; the color system is applied later
    /// by the [`Console`](crate::console::Console).
    pub fn render(&self, base_style: &Style, system: Option<ColorSystem>) -> Vec<Segment> {
        let _ = system;
        self.render_joined(base_style, None)
    }

    /// The `(minimum, maximum)` cell width of this text: `maximum` is the widest
    /// hard line, `minimum` the widest word. Port of `Text.__rich_measure__`.
    pub fn measurement(&self) -> (usize, usize) {
        let max_line = self.plain.split('\n').map(cell_len).max().unwrap_or(0);
        let min_word = self
            .plain
            .split_whitespace()
            .map(cell_len)
            .max()
            .unwrap_or(max_line);
        (min_word, max_line)
    }

    /// Render into visual lines, wrapping each hard line to `width` cells when
    /// `Some`, and justifying per this text's own justify.
    pub fn render_lines(&self, base_style: &Style, width: Option<usize>) -> Vec<Vec<Segment>> {
        self.render_lines_justified(base_style, width, self.justify)
    }

    /// Like [`render_lines`](Self::render_lines) but with an explicit `justify`
    /// (used by the console to apply `options.justify`).
    pub fn render_lines_justified(
        &self,
        base_style: &Style,
        width: Option<usize>,
        justify: Justify,
    ) -> Vec<Vec<Segment>> {
        let effective_base = base_style.combine(&self.style);
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        for (start, end) in self.wrapped_ranges(width) {
            lines.push(self.line_segments(start, end, &effective_base));
        }
        if let Some(width) = width {
            if justify != Justify::Default {
                let last = lines.len().saturating_sub(1);
                for (index, line) in lines.iter_mut().enumerate() {
                    // Full justification leaves the final line ragged, so it
                    // needs to know where it is in the paragraph.
                    *line = justify_line(line, width, justify, &effective_base, index == last);
                }
            }
        }
        lines
    }

    /// Render into a flat segment stream (newlines between lines), justifying per
    /// `justify`.
    pub fn render_joined_justified(
        &self,
        base_style: &Style,
        width: usize,
        justify: Justify,
    ) -> Vec<Segment> {
        let lines = self.render_lines_justified(base_style, Some(width), justify);
        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }

    /// Render into a flat segment stream with [`Segment::line`] between visual
    /// lines (wrapping when `width` is `Some`), using this text's own justify.
    fn render_joined(&self, base_style: &Style, width: Option<usize>) -> Vec<Segment> {
        let lines = self.render_lines(base_style, width);
        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }

    /// The `(start_byte, end_byte)` range of each visual line: hard lines split
    /// on `\n`, then wrapped to `width` cells when `Some`.
    fn wrapped_ranges(&self, width: Option<usize>) -> Vec<(usize, usize)> {
        let mut hard: Vec<(usize, usize)> = Vec::new();
        let mut start = 0;
        for (i, byte) in self.plain.bytes().enumerate() {
            if byte == b'\n' {
                hard.push((start, i));
                start = i + 1;
            }
        }
        hard.push((start, self.plain.len()));

        let Some(width) = width else {
            return hard;
        };

        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for (a, b) in hard {
            let sub = &self.plain[a..b];
            let breaks = crate::wrap::divide_line(sub, width, true);
            let mut cuts = vec![a];
            for char_offset in breaks {
                cuts.push(a + char_to_byte(sub, char_offset));
            }
            cuts.push(b);
            for window in cuts.windows(2) {
                ranges.push((window[0], window[1]));
            }
        }
        ranges
    }

    /// Combine `effective_base` with every span covering `[start, end)`,
    /// producing non-overlapping segments for that byte range.
    fn line_segments(&self, start: usize, end: usize, effective_base: &Style) -> Vec<Segment> {
        if start >= end {
            return Vec::new();
        }
        let mut points: Vec<usize> = vec![start, end];
        for span in &self.spans {
            let span_start = span.start.clamp(start, end);
            let span_end = span.end.clamp(start, end);
            points.push(span_start);
            points.push(span_end);
        }
        points.sort_unstable();
        points.dedup();

        let mut segments = Vec::new();
        for window in points.windows(2) {
            let (a, b) = (window[0], window[1]);
            if a >= b {
                continue;
            }
            let slice = &self.plain[a..b];
            if slice.is_empty() {
                continue;
            }
            let mut style = effective_base.clone();
            for span in &self.spans {
                if span.start <= a && span.end >= b {
                    style = style.combine(&span.style);
                }
            }
            segments.push(Segment::new(slice, Some(style)));
        }
        segments
    }

    /// Render wrapped to `width`, joined into a flat segment stream. Used by the
    /// [`Console`](crate::console::Console) render path.
    pub fn render_wrapped(&self, base_style: &Style, width: usize) -> Vec<Segment> {
        self.render_joined(base_style, Some(width))
    }
}

/// Byte offset of the `char_idx`-th char in `text` (clamped to `text.len()`).
fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

/// Split a rendered line into whitespace-separated words, each word keeping its
/// own styled segments. Separator spaces are dropped — [`full_justify`] decides
/// the new gaps. Port of the `line.split(" ")` in upstream's `full` branch.
fn split_words(line: &[Segment]) -> Vec<Vec<Segment>> {
    let mut words: Vec<Vec<Segment>> = Vec::new();
    let mut current: Vec<Segment> = Vec::new();
    for segment in line {
        // A segment can straddle a space, so split within it and keep the style.
        for (index, piece) in segment.text.split(' ').enumerate() {
            if index > 0 {
                words.push(std::mem::take(&mut current));
            }
            if !piece.is_empty() {
                current.push(Segment::new(piece, segment.style.clone()));
            }
        }
    }
    words.push(current);
    // Wrapping leaves a trailing space on every line but the last, so the naive
    // split ends with an empty word. Upstream's `Text.split` drops it, and the
    // count matters: it decides how many gaps share the slack.
    if words.last().is_some_and(|w| w.is_empty()) {
        words.pop();
    }
    words
}

/// Distribute `width` across `line`'s words by widening the gaps between them.
/// Direct port of the `justify == "full"` branch of upstream's `Lines.justify`:
/// every gap starts at one space, and the extra columns are handed out from the
/// rightmost gap backwards, cycling.
fn full_justify(line: &[Segment], width: usize, style: &Style) -> Vec<Segment> {
    let words = split_words(line);
    let words_size: usize = words
        .iter()
        .map(|word| word.iter().map(Segment::cell_length).sum::<usize>())
        .sum();
    let mut num_spaces = words.len().saturating_sub(1);
    let mut spaces = vec![1usize; num_spaces];
    if !spaces.is_empty() {
        let mut index = 0;
        while words_size + num_spaces < width {
            let slot = spaces.len() - index - 1;
            spaces[slot] += 1;
            num_spaces += 1;
            index = (index + 1) % spaces.len();
        }
    }

    let mut out: Vec<Segment> = Vec::new();
    for (index, word) in words.iter().enumerate() {
        out.extend(word.iter().cloned());
        if let Some(&gap) = spaces.get(index) {
            // Upstream styles the gap with the surrounding style when the two
            // neighbours agree, else with the line's base style.
            let before = word.last().and_then(|s| s.style.clone());
            let after = words
                .get(index + 1)
                .and_then(|w| w.first())
                .and_then(|s| s.style.clone());
            let gap_style = if before == after {
                before.unwrap_or_else(|| style.clone())
            } else {
                style.clone()
            };
            out.push(Segment::new(" ".repeat(gap), Some(gap_style)));
        }
    }
    out
}

/// Pad `line` to `width` cells according to `justify`, using `style` for the
/// pad (so e.g. a styled table cell fills with its own style).
///
/// `is_last` marks the final line of the paragraph, which full justification
/// leaves ragged rather than stretching.
fn justify_line(
    line: &[Segment],
    width: usize,
    justify: Justify,
    style: &Style,
    is_last: bool,
) -> Vec<Segment> {
    // Full justification rewrites the interior gaps instead of padding an edge.
    if justify == Justify::Full {
        // Upstream `break`s before the final line, so it is left exactly as
        // wrapped — not even padded out to the width, unlike every other mode.
        return if is_last {
            line.to_vec()
        } else {
            full_justify(line, width, style)
        };
    }
    let line_width: usize = line.iter().map(Segment::cell_length).sum();
    let excess = width.saturating_sub(line_width);
    let (left, right) = match justify {
        Justify::Right => (excess, 0),
        Justify::Center => (excess / 2, excess - excess / 2),
        // Left, Default, and full justification's ragged last line pad right.
        Justify::Left | Justify::Full | Justify::Default => (0, excess),
    };
    let mut out = Vec::with_capacity(line.len() + 2);
    if left > 0 {
        out.push(Segment::new(" ".repeat(left), Some(style.clone())));
    }
    out.extend(line.iter().cloned());
    if right > 0 {
        out.push(Segment::new(" ".repeat(right), Some(style.clone())));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full justification widens the gaps between words so every line but the
    /// last fills the width exactly.
    ///
    /// Captured verbatim from real rich 15.0.0 —
    /// `Lines.justify(console, 20, justify="full")` on
    /// `"aaa bbb ccc ddddddddddddddddddd ee ff"` yields:
    ///
    /// ```text
    /// 'aaa     bbb      ccc'   <- stretched to exactly 20
    /// 'ddddddddddddddddddd'    <- one word: nothing to widen, and the
    ///                             trailing space wrapping left is dropped
    /// 'ee ff'                  <- final line untouched: NOT padded to width
    /// ```
    ///
    /// Two details worth pinning: the slack is handed out from the rightmost
    /// gap backwards (so the gaps are 5 then 6, not 6 then 5), and the last
    /// line is the one case where a justified line is left short of the width.
    #[test]
    fn full_justify_matches_upstream() {
        let text = Text::new("aaa bbb ccc ddddddddddddddddddd ee ff").justify(Justify::Full);
        let plain: Vec<String> = text
            .render_lines(&Style::new(), Some(20))
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(
            plain,
            vec!["aaa     bbb      ccc", "ddddddddddddddddddd", "ee ff"]
        );
        assert_eq!(plain[0].chars().count(), 20);
    }

    #[test]
    fn append_creates_spans() {
        let mut text = Text::new("");
        text.append("hello", Some(Style::parse("bold").unwrap()));
        text.append(" world", None);
        assert_eq!(text.plain(), "hello world");
        assert_eq!(text.spans().len(), 1);
    }

    #[test]
    fn render_flattens_overlapping_spans() {
        let mut text = Text::new("abcdef");
        text.stylize(0, 4, Style::parse("bold").unwrap());
        text.stylize(2, 6, Style::parse("red").unwrap());
        let segments = text.render(&Style::new(), Some(ColorSystem::Truecolor));
        // Boundaries at 0,2,4,6 -> "ab"(bold) "cd"(bold+red) "ef"(red)
        let rendered: Vec<_> = segments.iter().map(|s| s.text.clone()).collect();
        assert_eq!(rendered, vec!["ab", "cd", "ef"]);
    }
}
