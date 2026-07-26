//! Segments — the atoms of rendering.
//!
//! Port of upstream `rich/segment.py`. A [`Segment`] is a piece of text with an
//! optional [`Style`]. Everything renderable ultimately becomes a stream of
//! segments, which the [`Console`](crate::console::Console) turns into bytes.
//!
//! Slice scope: control-code segments are modeled as a simple `control` flag;
//! the full `ControlType` enum is deferred to the Console-completeness issue.

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
