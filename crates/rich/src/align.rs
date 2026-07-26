//! Horizontal alignment.
//!
//! Port of upstream `rich/align.py` (horizontal axis). [`Align`] pads a child
//! renderable to fill the available width, positioning it left, center, or right.
//!
//! Slice scope: horizontal alignment. Vertical alignment (`VerticalAlign`) and
//! explicit `width`/`pad` options are deferred with the rest of `align.py`.

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// Where to position content within an available width. Shared by [`Align`],
/// `Rule` titles, and `Panel` titles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HorizontalAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// Aligns a child renderable within the available width. Mirrors `rich.align.Align`.
pub struct Align {
    child: Box<dyn Renderable>,
    align: HorizontalAlign,
}

impl Align {
    /// Left-align the child (pads on the right).
    pub fn left(child: Box<dyn Renderable>) -> Self {
        Align {
            child,
            align: HorizontalAlign::Left,
        }
    }

    /// Center the child (pads both sides, extra cell on the right).
    pub fn center(child: Box<dyn Renderable>) -> Self {
        Align {
            child,
            align: HorizontalAlign::Center,
        }
    }

    /// Right-align the child (pads on the left).
    pub fn right(child: Box<dyn Renderable>) -> Self {
        Align {
            child,
            align: HorizontalAlign::Right,
        }
    }
}

impl Renderable for Align {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let lines = console.render_lines(self.child.as_ref(), options, false);
        let style = Some(Style::new());

        let mut rows: Vec<Vec<Segment>> = Vec::with_capacity(lines.len());
        for line in lines {
            let line_width: usize = line.iter().map(Segment::cell_length).sum();
            let excess = width.saturating_sub(line_width);
            let (left_pad, right_pad) = match self.align {
                HorizontalAlign::Left => (0, excess),
                HorizontalAlign::Right => (excess, 0),
                HorizontalAlign::Center => (excess / 2, excess - excess / 2),
            };
            let mut row = Vec::new();
            if left_pad > 0 {
                row.push(Segment::new(" ".repeat(left_pad), style.clone()));
            }
            row.extend(line);
            if right_pad > 0 {
                row.push(Segment::new(" ".repeat(right_pad), style.clone()));
            }
            rows.push(row);
        }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::text::Text;

    fn console(width: usize) -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .build()
    }

    #[test]
    fn center_pads_both_sides() {
        let out = console(20).render_export(&Align::center(Box::new(Text::new("hi"))));
        assert_eq!(out, "         hi         \n");
    }

    #[test]
    fn right_pads_left() {
        let out = console(20).render_export(&Align::right(Box::new(Text::new("hi"))));
        assert_eq!(out, "                  hi\n");
    }

    #[test]
    fn center_odd_remainder_floors_left() {
        let out = console(21).render_export(&Align::center(Box::new(Text::new("hi"))));
        assert_eq!(out, "         hi          \n");
    }
}
