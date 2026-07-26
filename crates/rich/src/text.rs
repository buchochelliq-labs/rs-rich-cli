//! Styled text with spans.
//!
//! Port of upstream `rich/text.py` (core subset). [`Text`] is a plain string
//! plus a list of [`Span`]s, each applying a [`Style`] to a byte range. Spans
//! may overlap and nest; [`Text::render`] flattens them into non-overlapping
//! [`Segment`]s by combining every span covering each run.

use crate::cells::cell_len;
use crate::color::ColorSystem;
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
}

impl Text {
    /// Plain, unstyled text.
    pub fn new(plain: impl Into<String>) -> Self {
        Text {
            plain: plain.into(),
            spans: Vec::new(),
            style: Style::new(),
        }
    }

    /// Text with a base style.
    pub fn styled(plain: impl Into<String>, style: Style) -> Self {
        Text {
            plain: plain.into(),
            spans: Vec::new(),
            style,
        }
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

    /// Flatten into non-overlapping segments, combining `base_style`, this
    /// text's base style, and every covering span. Port of the core of
    /// `Text.render`.
    pub fn render(&self, base_style: &Style, system: Option<ColorSystem>) -> Vec<Segment> {
        let effective_base = base_style.combine(&self.style);

        // Collect the sorted, unique boundary offsets.
        let mut points: Vec<usize> = vec![0, self.plain.len()];
        for span in &self.spans {
            points.push(span.start.min(self.plain.len()));
            points.push(span.end.min(self.plain.len()));
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

        // If no styling applies at all, still emit the whole string once.
        if segments.is_empty() && !self.plain.is_empty() {
            segments.push(Segment::new(self.plain.clone(), Some(effective_base)));
        }

        let _ = system; // color-system application happens at the Console layer.
        segments
    }
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
