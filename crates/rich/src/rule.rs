//! Horizontal rules.
//!
//! Port of upstream `rich/rule.py`. A [`Rule`] draws a horizontal line across
//! the available width, optionally with a centered title.
//!
//! Slice scope: center alignment (the default). `left`/`right` title alignment
//! is deferred with the rest of `rule.py`.

use crate::align::HorizontalAlign;
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
    align: HorizontalAlign,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            title: None,
            characters: "─".to_string(),
            // Upstream's `rule.line` default style.
            style: Style::parse("bright_green").expect("valid built-in style"),
            align: HorizontalAlign::Center,
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

    /// Set the title alignment (default center).
    pub fn align(mut self, align: HorizontalAlign) -> Self {
        self.align = align;
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

        // Upstream: `required_space = 4 if align == "center" else 2`, and when
        // no space is left for the title it falls back to an untitled rule.
        // Without this a narrow rule drew nothing at all — at width 1 and 2 the
        // whole line came out blank, so `--rule` in a narrow terminal silently
        // produced no rule.
        let required_space = if matches!(self.align, HorizontalAlign::Center) {
            4
        } else {
            2
        };
        let truncate_width = width.saturating_sub(required_space);
        if truncate_width == 0 {
            return Text::styled(self.fill(width), self.style.clone());
        }

        match self.align {
            HorizontalAlign::Center => {
                // Title truncated (never padded) to leave room for the flanking spaces.
                let title = truncate_ellipsis(title, truncate_width);
                let title_len = cell_len(&title);

                let side_width = width.saturating_sub(title_len) / 2;
                let left = self.fill(side_width.saturating_sub(1));
                let right_length = width
                    .saturating_sub(title_len)
                    .saturating_sub(cell_len(&left))
                    .saturating_sub(2);
                let right = self.fill(right_length);

                let mut text = Text::new("");
                text.append(&format!("{left} "), Some(self.style.clone().into()));
                text.append(&title, None);
                text.append(&format!(" {right}"), Some(self.style.clone().into()));
                text
            }
            HorizontalAlign::Left => {
                let title = truncate_ellipsis(title, truncate_width);
                let fill_len = width.saturating_sub(cell_len(&title)).saturating_sub(1);
                let mut text = Text::new("");
                text.append(&format!("{title} "), None);
                text.append(&self.fill(fill_len), Some(self.style.clone().into()));
                text
            }
            HorizontalAlign::Right => {
                let title = truncate_ellipsis(title, truncate_width);
                let fill_len = width.saturating_sub(cell_len(&title)).saturating_sub(1);
                let mut text = Text::new("");
                text.append(&self.fill(fill_len), Some(self.style.clone().into()));
                text.append(&format!(" {title}"), None);
                text
            }
        }
    }
}

/// Truncate to `width` cells with upstream's `overflow="ellipsis"`: an over-long
/// title loses its tail to a single `…` rather than being cut mid-word, which is
/// what `Text.truncate(..., overflow="ellipsis")` does before a rule is drawn.
fn truncate_ellipsis(text: &str, width: usize) -> String {
    if cell_len(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = truncate(text, width.saturating_sub(1));
    out.push('\u{2026}');
    out
}

impl Renderable for Rule {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let text = self.build_text(options.max_width);
        text.render(console.theme(), console.base_style())
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

    /// A title needs four cells beside it; with none left upstream falls back to
    /// an untitled rule. We drew a line of spaces instead, so `--rule` in a very
    /// narrow terminal produced no visible rule at all.
    #[test]
    fn a_title_that_cannot_fit_falls_back_to_a_plain_rule() {
        for width in [1usize, 2, 3, 4] {
            let console = Console::builder().width(width).no_color(true).build();
            let out = console.render_to_string(&Rule::new("TITLE"));
            assert_eq!(
                out.trim_end_matches('\n'),
                "\u{2500}".repeat(width),
                "width {width} did not fall back to a plain rule"
            );
        }
    }

    /// Upstream truncates an over-long title with `overflow="ellipsis"`.
    #[test]
    fn an_over_long_title_is_ellipsised() {
        let console = Console::builder().width(5).no_color(true).build();
        let out = console.render_to_string(&Rule::new("TITLE"));
        assert_eq!(out.trim_end_matches('\n'), "\u{2500} \u{2026} \u{2500}");
    }
}
