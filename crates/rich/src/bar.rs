//! Horizontal bars.
//!
//! Port of upstream `rich/bar.py`. A [`Bar`] draws a filled span `[begin, end]`
//! within a range `[0, size]` across `width` cells, using eighth-block glyphs
//! for sub-cell resolution at each edge.

use crate::color::Color;
use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

const FULL_BLOCK: &str = "\u{2588}"; // █
/// Right-aligned partial blocks for the *begin* edge (index = eighths).
const BEGIN_BLOCK_ELEMENTS: [&str; 8] = [
    "\u{2588}", "\u{2588}", "\u{2588}", "\u{2590}", "\u{2590}", "\u{2590}", "\u{2595}", "\u{2595}",
];
/// Left-aligned partial blocks for the *end* edge (index = eighths).
const END_BLOCK_ELEMENTS: [&str; 8] = [
    " ", "\u{258f}", "\u{258e}", "\u{258d}", "\u{258c}", "\u{258b}", "\u{258a}", "\u{2589}",
];

/// A horizontal bar spanning `[begin, end]` within `[0, size]`. Mirrors `rich.bar.Bar`.
pub struct Bar {
    size: f64,
    begin: f64,
    end: f64,
    width: Option<usize>,
    style: Style,
}

impl Bar {
    /// A bar covering `[begin, end]` of a `[0, size]` range.
    pub fn new(size: f64, begin: f64, end: f64) -> Self {
        Bar {
            size,
            begin,
            end,
            width: None,
            // Upstream default: `color="default"`, `bgcolor="default"`.
            style: Style::new()
                .with_color(Color::default_color())
                .with_bgcolor(Color::default_color()),
        }
    }

    /// Fix the bar width (otherwise it fills the available width).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }
}

impl Renderable for Bar {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = self
            .width
            .unwrap_or(options.max_width)
            .min(options.max_width);
        let style = Some(self.style.clone());

        if self.begin >= self.end {
            return vec![Segment::new(" ".repeat(width), style)];
        }

        let prefix_complete_eighths = (width as f64 * 8.0 * self.begin / self.size) as usize;
        let prefix_bar_count = prefix_complete_eighths / 8;
        let prefix_eighths = prefix_complete_eighths % 8;

        let body_complete_eighths = (width as f64 * 8.0 * self.end / self.size) as usize;
        let body_bar_count = body_complete_eighths / 8;
        let body_eighths = body_complete_eighths % 8;

        let mut prefix = " ".repeat(prefix_bar_count);
        if prefix_eighths != 0 {
            prefix.push_str(BEGIN_BLOCK_ELEMENTS[prefix_eighths]);
        }

        let mut body = FULL_BLOCK.repeat(body_bar_count);
        if body_eighths != 0 {
            body.push_str(END_BLOCK_ELEMENTS[body_eighths]);
        }

        let body_len = body.chars().count();
        let suffix = " ".repeat(width.saturating_sub(body_len));

        // Overlay: prefix, then the body from `len(prefix)` onward, then the suffix.
        let prefix_len = prefix.chars().count();
        let body_tail: String = body.chars().skip(prefix_len).collect();
        let line = format!("{prefix}{body_tail}{suffix}");

        vec![Segment::new(line, style)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(begin: f64, end: f64) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        console.render_to_string(&Bar::new(100.0, begin, end).width(20))
    }

    #[test]
    fn full_bar() {
        assert_eq!(
            render(0.0, 100.0),
            format!("\x1b[39;49m{}\x1b[0m", FULL_BLOCK.repeat(20))
        );
    }

    #[test]
    fn half_bar() {
        assert_eq!(
            render(0.0, 50.0),
            format!("\x1b[39;49m{}          \x1b[0m", FULL_BLOCK.repeat(10))
        );
    }

    #[test]
    fn partial_edge_uses_eighth_block() {
        // end=33% of width 20 → 6 full blocks + a 4/8 left block (▌).
        assert_eq!(render(0.0, 33.0), "\x1b[39;49m██████▌             \x1b[0m");
    }
}
