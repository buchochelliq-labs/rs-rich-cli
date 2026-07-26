//! Markdown rendering.
//!
//! Port of upstream `rich/markdown.py` (core block/inline elements). Parses
//! CommonMark with `pulldown-cmark` and renders each block as a justified,
//! full-width [`Text`], separated by blank lines.
//!
//! Scope: paragraphs, ATX headings (h1–h6), and inline strong/emphasis/code.
//! Lists, code blocks, block quotes, links, rules, and tables are deferred (see
//! docs/DIVERGENCES.md and the Markdown issue).

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::console::{Console, ConsoleOptions, Justify};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;

const CODE_STYLE: &str = "bold cyan on black"; // markdown.code

/// A rendered Markdown document. Mirrors `rich.markdown.Markdown`.
pub struct Markdown {
    blocks: Vec<Text>,
}

impl Markdown {
    /// Parse CommonMark `source` into renderable blocks.
    pub fn new(source: &str) -> Self {
        Markdown {
            blocks: parse(source),
        }
    }
}

fn heading_level(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// The `(base style, justify)` for a heading level, from `default_styles.py`
/// and `Heading.LEVEL_ALIGN`.
fn heading_format(level: usize) -> (Style, Justify) {
    let (spec, justify) = match level {
        1 => ("bold underline", Justify::Center),
        2 => ("underline magenta", Justify::Left),
        3 => ("bold magenta", Justify::Left),
        4 => ("italic magenta", Justify::Left),
        5 => ("italic", Justify::Left),
        _ => ("dim", Justify::Left),
    };
    (Style::parse(spec).unwrap_or_default(), justify)
}

fn inline_style(strong: usize, emphasis: usize) -> Option<Style> {
    if strong == 0 && emphasis == 0 {
        return None;
    }
    let mut style = Style::new();
    if strong > 0 {
        style = style.combine(&Style::parse("bold").expect("valid style"));
    }
    if emphasis > 0 {
        style = style.combine(&Style::parse("italic").expect("valid style"));
    }
    Some(style)
}

fn parse(source: &str) -> Vec<Text> {
    let mut blocks: Vec<Text> = Vec::new();
    let mut current: Option<Text> = None;
    // A heading's style is applied as a span over its content (so the justify
    // padding stays plain, matching upstream); paragraphs have no such style.
    let mut heading_style: Option<Style> = None;
    let mut justify = Justify::Left;
    let mut strong = 0usize;
    let mut emphasis = 0usize;

    for event in Parser::new(source) {
        match event {
            Event::Start(Tag::Paragraph) => {
                current = Some(Text::new(""));
                heading_style = None;
                justify = Justify::Left;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let (style, heading_justify) = heading_format(heading_level(level));
                current = Some(Text::new(""));
                heading_style = Some(style);
                justify = heading_justify;
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Heading(_)) => {
                if let Some(mut text) = current.take() {
                    if let Some(style) = &heading_style {
                        // Style the whole heading content (leaving pad plain).
                        let end = text.plain().len();
                        text.stylize(0, end, style.clone());
                    }
                    text.set_justify(justify);
                    blocks.push(text);
                }
                strong = 0;
                emphasis = 0;
            }
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => emphasis += 1,
            Event::End(TagEnd::Emphasis) => emphasis = emphasis.saturating_sub(1),
            Event::Text(text) => {
                if let Some(block) = current.as_mut() {
                    block.append(&text, inline_style(strong, emphasis));
                }
            }
            Event::Code(text) => {
                if let Some(block) = current.as_mut() {
                    block.append(&text, Style::parse(CODE_STYLE).ok());
                }
            }
            Event::SoftBreak => {
                if let Some(block) = current.as_mut() {
                    block.append(" ", None);
                }
            }
            Event::HardBreak => {
                if let Some(block) = current.as_mut() {
                    block.append("\n", None);
                }
            }
            _ => {} // Other elements are deferred.
        }
    }
    blocks
}

impl Renderable for Markdown {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                lines.push(Vec::new()); // blank separator line between blocks
            }
            lines.extend(block.render_lines(console.base_style(), Some(width)));
        }

        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
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

    fn render(source: &str) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        console.render_to_string(&Markdown::new(source))
    }

    #[test]
    fn paragraph_inline_styles() {
        // Captured from real rich 15.0.0.
        assert_eq!(
            render("a `x` b"),
            "a \x1b[1;36;40mx\x1b[0m b               "
        );
    }

    #[test]
    fn headings() {
        assert_eq!(render("# Head"), "        \x1b[1;4mHead\x1b[0m        ");
        assert_eq!(render("## Sub"), "\x1b[4;35mSub\x1b[0m                 ");
    }

    #[test]
    fn two_paragraphs_separated_by_blank_line() {
        assert_eq!(
            render("First para.\n\nSecond para."),
            "First para.         \n\nSecond para.        "
        );
    }
}
