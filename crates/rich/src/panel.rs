//! Panels — a box drawn around a renderable.
//!
//! Port of upstream `rich/panel.py`. A [`Panel`] frames a child renderable with
//! a box border, inner padding, and an optional centered title.
//!
//! Slice scope: title + subtitle (with alignment), box + border style +
//! padding, expand-to-width. `fit` (shrink-to-content) sizing is deferred.

use crate::align::HorizontalAlign;
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
    title_align: HorizontalAlign,
    subtitle: Option<String>,
    subtitle_align: HorizontalAlign,
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
            title_align: HorizontalAlign::Center,
            subtitle: None,
            subtitle_align: HorizontalAlign::Center,
            padding: (0, 1, 0, 1),
            border_style: Style::new(),
            style: Style::new(),
        }
    }

    /// Set a title (drawn into the top border, centered by default).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the title alignment within the top border.
    pub fn title_align(mut self, align: HorizontalAlign) -> Self {
        self.title_align = align;
        self
    }

    /// Set a subtitle (drawn into the bottom border, centered by default).
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Set the subtitle alignment within the bottom border.
    pub fn subtitle_align(mut self, align: HorizontalAlign) -> Self {
        self.subtitle_align = align;
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

    /// Build a top/bottom border, optionally embedding an aligned `label`.
    fn border_line(
        &self,
        inner_width: usize,
        left_corner: char,
        fill_char: char,
        right_corner: char,
        label: Option<&String>,
        align: HorizontalAlign,
    ) -> String {
        let mut border = String::new();
        border.push(left_corner);
        match label {
            None => border.extend(std::iter::repeat(fill_char).take(inner_width)),
            Some(label) => {
                // Truncate the label so it (plus its flanking spaces) fits.
                let label = truncate(label, inner_width.saturating_sub(2));
                let padded = format!(" {label} ");
                let fill = inner_width.saturating_sub(cell_len(&padded));
                let (left, right) = match align {
                    HorizontalAlign::Center => (fill / 2, fill - fill / 2),
                    // Left/right keep a single box-char offset on the near side.
                    HorizontalAlign::Left => (1.min(fill), fill.saturating_sub(1)),
                    HorizontalAlign::Right => (fill.saturating_sub(1), 1.min(fill)),
                };
                border.extend(std::iter::repeat(fill_char).take(left));
                border.push_str(&padded);
                border.extend(std::iter::repeat(fill_char).take(right));
            }
        }
        border.push(right_corner);
        border
    }
}

impl Renderable for Panel {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        // Fall back to a terminal-safe box on legacy Windows / non-UTF-8.
        let box_set = self.box_set.substitute(
            console.legacy_windows(),
            console.safe_box(),
            console.ascii_only(),
        );
        let inner_width = width.saturating_sub(2);
        let (pt, pr, pb, pl) = self.padding;
        let child_width = inner_width.saturating_sub(pl).saturating_sub(pr);

        let mut child_options = options.update_width(child_width);
        // When a height is imposed (e.g. as a Layout leaf), the child fills the
        // space left by the two borders and the top/bottom padding rows, so the
        // panel expands to exactly `height` rows. Port of `Panel`'s
        // `child_height = height - 2` (padding here lives outside the child).
        child_options.height = options.height.map(|h| h.saturating_sub(2 + pt + pb));
        let child_lines = console.render_lines(self.child.as_ref(), &child_options, true);

        let border = Some(self.border_style.clone());
        let inner_style = Some(self.style.clone());
        let left_border = || Segment::new(box_set.mid_left.to_string(), border.clone());
        let right_border = || Segment::new(box_set.mid_right.to_string(), border.clone());
        let blank_inner = || Segment::new(" ".repeat(inner_width), inner_style.clone());

        let mut rows: Vec<Vec<Segment>> = Vec::new();

        // Top border (with title if present).
        rows.push(vec![Segment::new(
            self.border_line(
                inner_width,
                box_set.top_left,
                box_set.top,
                box_set.top_right,
                self.title.as_ref(),
                self.title_align,
            ),
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

        // Bottom border (with subtitle if present).
        rows.push(vec![Segment::new(
            self.border_line(
                inner_width,
                box_set.bottom_left,
                box_set.bottom,
                box_set.bottom_right,
                self.subtitle.as_ref(),
                self.subtitle_align,
            ),
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

    #[test]
    fn legacy_windows_substitutes_rounded_to_square() {
        // On a legacy Windows console, ROUNDED falls back to SQUARE. Captured
        // from real rich 15.0.0 (legacy_windows=True, width 12).
        let legacy = Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(12)
            .no_color(false)
            .legacy_windows(true)
            .build();
        let out = legacy.render_export(&Panel::new(Box::new(Text::new("hi"))));
        assert_eq!(out, "┌──────────┐\n│ hi       │\n└──────────┘\n");
    }
}
