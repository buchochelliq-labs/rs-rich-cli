//! Horizontal rules.
//!
//! Port of upstream `rich/rule.py`. A [`Rule`] draws a horizontal line across
//! the available width, optionally with a centered title.
//!
//! Slice scope: center alignment (the default). `left`/`right` title alignment
//! is deferred with the rest of `rule.py`.

use crate::cells::{cell_len, set_cell_size, truncate};
use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;

/// A horizontal rule, optionally titled. Mirrors `rich.rule.Rule`.
pub struct Rule {
    title: Option<String>,
    characters: String,
    style: Style,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            title: None,
            characters: "─".to_string(),
            // Upstream's `rule.line` default style.
            style: Style::parse("bright_green").expect("valid built-in style"),
        }
    }
}

impl Rule {
    /// A plain, untitled rule.
    pub fn line() -> Self {
        Rule::default()
    }

    /// A rule with a centered title.
    pub fn new(title: impl Into<String>) -> Self {
        Rule {
            title: Some(title.into()),
            ..Rule::default()
        }
    }

    /// Override the fill character(s).
    pub fn characters(mut self, characters: impl Into<String>) -> Self {
        self.characters = characters.into();
        self
    }

    /// Override the rule style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Repeat `characters` to at least `width` cells, then crop to exactly `width`.
    fn fill(&self, width: usize) -> String {
        if width == 0 {
            return String::new();
        }
        let chars_len = cell_len(&self.characters).max(1);
        let repeat = width / chars_len + 1;
        let repeated = self.characters.repeat(repeat);
        set_cell_size(&repeated, width)
    }

    fn build_text(&self, width: usize) -> Text {
        let Some(title) = &self.title else {
            return Text::styled(self.fill(width), self.style.clone());
        };

        // Title truncated (never padded) to leave room for the flanking spaces.
        let title = truncate(title, width.saturating_sub(4));
        let title_len = cell_len(&title);

        let side_width = width.saturating_sub(title_len) / 2;
        let left = self.fill(side_width.saturating_sub(1));
        let right_length = width
            .saturating_sub(title_len)
            .saturating_sub(cell_len(&left))
            .saturating_sub(2);
        let right = self.fill(right_length);

        let mut text = Text::new("");
        text.append(&format!("{left} "), Some(self.style.clone()));
        text.append(&title, None);
        text.append(&format!(" {right}"), Some(self.style.clone()));
        text
    }
}

impl Renderable for Rule {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let text = self.build_text(options.max_width);
        text.render(console.base_style(), console.color_system())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(20)
            .build()
    }

    #[test]
    fn plain_rule_fills_width() {
        let out = console().render_export(&Rule::line());
        assert_eq!(out, format!("\x1b[92m{}\x1b[0m\n", "─".repeat(20)));
    }

    #[test]
    fn titled_rule_centers() {
        let out = console().render_export(&Rule::new("Hi"));
        assert_eq!(out, "\x1b[92m──────── \x1b[0mHi\x1b[92m ────────\x1b[0m\n");
    }
}
