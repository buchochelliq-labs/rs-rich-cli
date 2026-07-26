//! Padding around a renderable.
//!
//! Port of upstream `rich/padding.py`. [`Padding`] surrounds a child renderable
//! with blank space on any of its four sides.

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// Blank space around a child renderable. Mirrors `rich.padding.Padding`.
///
/// The pad is `(top, right, bottom, left)` — the same order as CSS and upstream.
pub struct Padding {
    child: Box<dyn Renderable>,
    pad: (usize, usize, usize, usize),
    style: Style,
}

impl Padding {
    /// Pad `child` by an explicit `(top, right, bottom, left)`.
    pub fn new(child: Box<dyn Renderable>, pad: (usize, usize, usize, usize)) -> Self {
        Padding {
            child,
            pad,
            style: Style::new(),
        }
    }

    /// Equal padding on all four sides.
    pub fn uniform(child: Box<dyn Renderable>, amount: usize) -> Self {
        Padding::new(child, (amount, amount, amount, amount))
    }

    /// `(vertical, horizontal)` padding (top==bottom, left==right).
    pub fn symmetric(child: Box<dyn Renderable>, vertical: usize, horizontal: usize) -> Self {
        Padding::new(child, (vertical, horizontal, vertical, horizontal))
    }

    /// Set the style applied to the padding (and blank lines).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl Renderable for Padding {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (top, right, bottom, left) = self.pad;
        let width = options.max_width;
        let child_width = width.saturating_sub(left).saturating_sub(right);

        let child_options = options.update_width(child_width);
        let lines = console.render_lines(self.child.as_ref(), &child_options, true);

        let style = Some(self.style.clone());
        let blank = || Segment::new(" ".repeat(width), style.clone());
        let left_pad = |row: &mut Vec<Segment>| {
            if left > 0 {
                row.push(Segment::new(" ".repeat(left), style.clone()));
            }
        };
        let right_pad = |row: &mut Vec<Segment>| {
            if right > 0 {
                row.push(Segment::new(" ".repeat(right), style.clone()));
            }
        };

        let mut rows: Vec<Vec<Segment>> = Vec::new();
        for _ in 0..top {
            rows.push(vec![blank()]);
        }
        for line in lines {
            let mut row = Vec::new();
            left_pad(&mut row);
            row.extend(line);
            right_pad(&mut row);
            rows.push(row);
        }
        for _ in 0..bottom {
            rows.push(vec![blank()]);
        }

        join_rows(rows)
    }
}

/// Flatten rows into a segment stream separated by newline segments (no trailing
/// newline — the console adds one on export/print).
pub(crate) fn join_rows(rows: Vec<Vec<Segment>>) -> Vec<Segment> {
    let mut segments = Vec::new();
    let last = rows.len().saturating_sub(1);
    for (index, row) in rows.into_iter().enumerate() {
        segments.extend(row);
        if index != last {
            segments.push(Segment::line());
        }
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Text;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(10)
            .build()
    }

    #[test]
    fn pads_all_sides() {
        let padding = Padding::new(Box::new(Text::new("hi")), (1, 2, 1, 2));
        let out = console().render_export(&padding);
        assert_eq!(out, "          \n  hi      \n          \n");
    }

    #[test]
    fn horizontal_only() {
        let padding = Padding::new(Box::new(Text::new("hi")), (0, 1, 0, 1));
        let out = console().render_export(&padding);
        assert_eq!(out, " hi       \n");
    }
}
