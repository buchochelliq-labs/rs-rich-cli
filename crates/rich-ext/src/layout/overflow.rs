//! Shared cell-aware overflow, preserving resolved segment styles.
use rich::{Justify, Overflow, Segment, Style, Text, Theme};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverflowPolicy {
    Wrap,
    #[default]
    Fold,
    Crop,
    Ellipsis,
    Visible,
}
pub fn fit_segments(
    segments: &[Segment],
    width: usize,
    policy: OverflowPolicy,
) -> Vec<Vec<Segment>> {
    if width == 0 {
        return Vec::new();
    }
    let mut text = Text::new("");
    for segment in segments.iter().filter(|s| !s.control) {
        text.append(&segment.text, segment.style.clone().map(Into::into));
    }
    let (overflow, no_wrap) = match policy {
        OverflowPolicy::Wrap => (Overflow::Crop, false),
        OverflowPolicy::Fold => (Overflow::Fold, false),
        OverflowPolicy::Crop => (Overflow::Crop, true),
        OverflowPolicy::Ellipsis => (Overflow::Ellipsis, true),
        OverflowPolicy::Visible => (Overflow::Ignore, true),
    };
    text.render_lines_wrapped(
        &Theme::default_theme(),
        &Style::new(),
        Some(width),
        Justify::Default,
        overflow,
        no_wrap,
    )
}
