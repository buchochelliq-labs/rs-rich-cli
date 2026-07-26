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

    /// Render into visual lines, wrapping each hard line to `width` cells when
    /// `Some`. Port of the line-producing half of `Text.render`/`Text.wrap`.
    pub fn render_lines(&self, base_style: &Style, width: Option<usize>) -> Vec<Vec<Segment>> {
        let effective_base = base_style.combine(&self.style);
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        for (start, end) in self.wrapped_ranges(width) {
            lines.push(self.line_segments(start, end, &effective_base));
        }
        // Justify each line to the render width, if requested. (Note: this pads
        // to `width` unconditionally; upstream first shrinks `width` to the
        // content via measurement for a *bare* top-level Text — see
        // docs/DIVERGENCES.md.)
        if let Some(width) = width {
            if self.justify != Justify::Default {
                for line in &mut lines {
                    *line = justify_line(line, width, self.justify, &effective_base);
                }
            }
        }
        lines
    }

    /// Render into a flat segment stream with [`Segment::line`] between visual
    /// lines (wrapping when `width` is `Some`).
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

/// Pad `line` to `width` cells according to `justify`, using `style` for the pad.
fn justify_line(line: &[Segment], width: usize, justify: Justify, style: &Style) -> Vec<Segment> {
    let line_width: usize = line.iter().map(Segment::cell_length).sum();
    let excess = width.saturating_sub(line_width);
    let (left, right) = match justify {
        Justify::Right => (excess, 0),
        Justify::Center => (excess / 2, excess - excess / 2),
        // Left and (for now) Full pad on the right; Default never reaches here.
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
