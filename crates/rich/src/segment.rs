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

    #[test]
    fn cell_length_ignores_control() {
        assert_eq!(Segment::new("abc", None).cell_length(), 3);
        assert_eq!(Segment::control("\x1b[2J").cell_length(), 0);
    }
}
