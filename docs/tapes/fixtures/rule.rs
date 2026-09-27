//! Horizontal rules.
//!
//! Port of upstream `rich/rule.py`. A [`Rule`] draws a horizontal line across
//! the available width, optionally with a centered title.
//!
//! Titles are console markup or a literal [`Text`], aligned left, center or
//! right; `characters`, `style` (default the theme's `rule.line`) and `end`
//! follow upstream.

use crate::align::HorizontalAlign;
use crate::cells::{cell_len, set_cell_size};
use crate::console::{Console, ConsoleOptions, Overflow};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::StyleType;
use crate::text::{Text, DEFAULT_TAB_SIZE};

/// A horizontal rule, optionally titled. Mirrors `rich.rule.Rule`.
pub struct Rule {
    title: Option<String>,
    /// A literal title (upstream `Rule(Text(…))`), in place of `title`.
    title_text: Option<Text>,
    characters: String,
    style: StyleType,
    end: String,
    align: HorizontalAlign,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            title: None,
            title_text: None,
            characters: "─".to_string(),
            // Upstream's default `style="rule.line"`, resolved per console.
            style: StyleType::Name("rule.line".to_string()),
            end: "\n".to_string(),
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

    /// A rule titled with a literal [`Text`] (upstream `Rule(Text(…))`): no
    /// markup, and no `rule.text` style beneath it.
    pub fn with_title_text(title: Text) -> Self {
        Rule {
            title_text: Some(title),
            ..Rule::default()
        }
    }

    /// What follows a *titled* rule (upstream `end`, default `"\n"`). As
    /// upstream, an untitled rule ignores it. The port's renderables separate
    /// lines rather than ending them, so one trailing newline of `end` is the
    /// line end the printer adds; an `end` without one cannot suppress it.
    pub fn end(mut self, end: impl Into<String>) -> Self {
        self.end = end.into();
        self
    }

    /// Override the fill character(s).
    pub fn characters(mut self, characters: impl Into<String>) -> Self {
        self.characters = characters.into();
        self
    }

    /// Override the rule style: a [`Style`], or a theme name / definition
    /// (default `"rule.line"`).
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = style.into();
        self
    }

    /// Set the title alignment (default center).
    pub fn align(mut self, align: HorizontalAlign) -> Self {
        self.align = align;
        self
    }

    /// Repeat `characters` to at least `width` cells, then crop to exactly `width`.
    fn fill(characters: &str, width: usize) -> String {
        if width == 0 {
            return String::new();
        }
        let chars_len = cell_len(characters).max(1);
        let repeat = width / chars_len + 1;
        let repeated = characters.repeat(repeat);
        set_cell_size(&repeated, width)
    }

    /// The rule as a `Text`, and whether it is titled (only a titled rule
    /// carries `end`).
    fn build_text(&self, console: &Console, options: &ConsoleOptions) -> (Text, bool) {
        let width = options.max_width;
        // `"-" if options.ascii_only and not characters.isascii()`: only the
        // titled layouts use the substitute; `_rule_line` keeps the original.
        let characters = if options.ascii_only() && !self.characters.is_ascii() {
            "-"
        } else {
            self.characters.as_str()
        };
        let rule_line = || Text::styled(Self::fill(&self.characters, width), self.style.clone());
        let title = match (&self.title_text, &self.title) {
            (Some(text), _) if !text.plain().is_empty() => {
                let mut title = text.blank_copy();
                title.append(&text.plain().replace('\n', " "), None);
                for span in text.spans() {
                    title.push_span(span.clone());
                }
                title
            }
            (None, Some(title)) if !title.is_empty() => {
                // Upstream uses Console.render_str, so titles retain markup, emoji,
                // the console's highlighter and the `rule.text` theme style.
                let parsed = console.build_text(title);
                let mut title = parsed.blank_copy();
                title.append(&parsed.plain().replace('\n', " "), None);
                for span in parsed.spans() {
                    title.push_span(span.clone());
                }
                title.set_base_style("rule.text");
                title
            }
            _ => return (rule_line(), false),
        };
        let mut title = title;

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
            return (rule_line(), false);
        }
        title.expand_tabs(DEFAULT_TAB_SIZE);
        title.truncate(truncate_width, Some(Overflow::Ellipsis), false);

        let mut text = match self.align {
            HorizontalAlign::Center => {
                // Title truncated (never padded) to leave room for the flanking spaces.
                let title_len = title.cell_len();

                let side_width = width.saturating_sub(title_len) / 2;
                let left = Self::fill(characters, side_width.saturating_sub(1));
                let right_length = width
                    .saturating_sub(title_len)
                    .saturating_sub(cell_len(&left))
                    .saturating_sub(2);
                let right = Self::fill(characters, right_length);

                let mut text = Text::new("");
                text.append(&format!("{left} "), Some(self.style.clone()));
                text = text.append_text(&title);
                text.append(&format!(" {right}"), Some(self.style.clone()));
                text
            }
            HorizontalAlign::Left => {
                let fill_len = width.saturating_sub(title.cell_len()).saturating_sub(1);
                let mut text = Text::new("");
                text = text.append_text(&title);
                text.append(" ", None);
                text.append(&Self::fill(characters, fill_len), Some(self.style.clone()));
                text
            }
            HorizontalAlign::Right => {
                // Upstream repeats the characters string once per remaining
                // *cell*, so a multi-cell fill overshoots the width and the
                // final crop below removes the title (#444).
                let repeat = width.saturating_sub(title.cell_len()).saturating_sub(1);
                let mut text = Text::new("");
                text.append(&characters.repeat(repeat), Some(self.style.clone()));
                text.append(" ", None);
                text = text.append_text(&title);
                text
            }
        };
        // Upstream: `rule_text.plain = set_cell_size(rule_text.plain, width)`.
        text.truncate(width, Some(Overflow::Crop), true);
        (text, true)
    }
}

impl Renderable for Rule {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (text, titled) = self.build_text(console, options);
        let mut segments = text.render(console.theme(), console.base_style());
        if titled {
            let end = self.end.strip_suffix('\n').unwrap_or(&self.end);
            if !end.is_empty() {
                segments.push(Segment::new(end, None));
            }
        }
        segments
    }

    /// Port of `Rule.__rich_measure__`: a rule fits any width, so it asks for
    /// a single cell and never widens a fitted container.
    fn measure(&self, _console: &Console, _options: &ConsoleOptions) -> Measurement {
        Measurement::new(1, 1)
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
            let console = Console::builder().width(width).color_system(None).build();
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
        let console = Console::builder().width(5).color_system(None).build();
        let out = console.render_to_string(&Rule::new("TITLE"));
        assert_eq!(out.trim_end_matches('\n'), "\u{2500} \u{2026} \u{2500}");
    }

    #[test]
    fn right_aligned_title_is_dropped_by_a_multi_cell_fill() {
        // Captured from real rich 15.0.0 (#444).
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(10)
            .highlight(false)
            .build();
        let rule = Rule::new("x")
            .characters("-~")
            .align(HorizontalAlign::Right);
        assert_eq!(console.render_export(&rule), "\x1b[92m-~-~-~-~-~\x1b[0m\n");
    }
}
