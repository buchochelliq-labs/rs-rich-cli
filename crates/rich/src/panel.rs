//! Panels — a box drawn around a renderable.
//!
//! Port of upstream `rich/panel.py`. A [`Panel`] frames a child renderable with
//! a box border, inner padding, and an optional centered title.
//!
//! Slice scope: centered title, box + border style + padding, expand-to-width.
//! Subtitle, `title_align`/`subtitle_align`, and `fit` sizing are deferred.

use crate::cells::{cell_len, truncate};
use crate::console::{Console, ConsoleOptions};
use crate::padding::join_rows;
use crate::protocol::Renderable;
use crate::r#box::{Box as BoxSet, ROUNDED};
use crate::segment::Segment;
use crate::style::Style;

/// A bordered box around a renderable. Mirrors `rich.panel.Panel`.
pub struct Panel {
    child: Box<dyn Renderable>,
    box_set: BoxSet,
    title: Option<String>,
    padding: (usize, usize, usize, usize),
    border_style: Style,
    style: Style,
}

impl Panel {
    /// A panel around `child` with default box (`ROUNDED`) and padding `(0,1)`.
    pub fn new(child: Box<dyn Renderable>) -> Self {
        Panel {
            child,
            box_set: ROUNDED,
            title: None,
            padding: (0, 1, 0, 1),
            border_style: Style::new(),
            style: Style::new(),
        }
    }

    /// Set a centered title (drawn into the top border).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Choose the box-drawing set.
    pub fn box_set(mut self, box_set: BoxSet) -> Self {
        self.box_set = box_set;
        self
    }

    /// Set the inner padding `(top, right, bottom, left)`.
    pub fn padding(mut self, padding: (usize, usize, usize, usize)) -> Self {
        self.padding = padding;
        self
    }

    /// Set the border style.
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Build the (possibly titled) top border string for a given inner width.
    fn top_border(&self, inner_width: usize) -> String {
        let Some(title) = &self.title else {
            return self.box_set.get_top(&[inner_width]);
        };
        // Truncate the title so it (plus its flanking spaces) fits.
        let title = truncate(title, inner_width.saturating_sub(2));
        let padded = format!(" {title} ");
        let padded_len = cell_len(&padded);
        let fill = inner_width.saturating_sub(padded_len);
        let left = fill / 2;
        let right = fill - left;
        let top = self.box_set.top;
        let mut border = String::new();
        border.push(self.box_set.top_left);
        border.extend(std::iter::repeat(top).take(left));
        border.push_str(&padded);
        border.extend(std::iter::repeat(top).take(right));
        border.push(self.box_set.top_right);
        border
    }
}

impl Renderable for Panel {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let inner_width = width.saturating_sub(2);
        let (pt, pr, pb, pl) = self.padding;
        let child_width = inner_width.saturating_sub(pl).saturating_sub(pr);

        let child_options = options.update_width(child_width);
        let child_lines = console.render_lines(self.child.as_ref(), &child_options, true);

        let border = Some(self.border_style.clone());
        let inner_style = Some(self.style.clone());
        let left_border = || Segment::new(self.box_set.mid_left.to_string(), border.clone());
        let right_border = || Segment::new(self.box_set.mid_right.to_string(), border.clone());
        let blank_inner = || Segment::new(" ".repeat(inner_width), inner_style.clone());

        let mut rows: Vec<Vec<Segment>> = Vec::new();

        // Top border (with title if present).
        rows.push(vec![Segment::new(
            self.top_border(inner_width),
            border.clone(),
        )]);

        // Top padding rows.
        for _ in 0..pt {
            rows.push(vec![left_border(), blank_inner(), right_border()]);
        }

        // Content rows: border + left pad + content + right pad + border.
        for line in child_lines {
            let mut row = vec![left_border()];
            if pl > 0 {
                row.push(Segment::new(" ".repeat(pl), inner_style.clone()));
            }
            row.extend(line);
            if pr > 0 {
                row.push(Segment::new(" ".repeat(pr), inner_style.clone()));
            }
            row.push(right_border());
            rows.push(row);
        }

        // Bottom padding rows.
        for _ in 0..pb {
            rows.push(vec![left_border(), blank_inner(), right_border()]);
        }

        // Bottom border.
        rows.push(vec![Segment::new(
            self.box_set.get_bottom(&[inner_width]),
            border.clone(),
        )]);

        join_rows(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#box::SQUARE;
    use crate::text::Text;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(20)
            .build()
    }

    #[test]
    fn plain_panel() {
        let out = console().render_export(&Panel::new(Box::new(Text::new("hello"))));
        assert_eq!(
            out,
            "╭──────────────────╮\n│ hello            │\n╰──────────────────╯\n"
        );
    }

    #[test]
    fn titled_panel() {
        let out = console().render_export(&Panel::new(Box::new(Text::new("hello"))).title("T"));
        assert_eq!(
            out,
            "╭─────── T ────────╮\n│ hello            │\n╰──────────────────╯\n"
        );
    }

    #[test]
    fn square_box() {
        let out = console().render_export(&Panel::new(Box::new(Text::new("hi"))).box_set(SQUARE));
        assert_eq!(
            out,
            "┌──────────────────┐\n│ hi               │\n└──────────────────┘\n"
        );
    }
}
