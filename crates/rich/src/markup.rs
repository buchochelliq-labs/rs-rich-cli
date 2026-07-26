//! Console markup parsing.
//!
//! Port of upstream `rich/markup.py`. Turns `"[bold]Hello[/] [red]World[/]"`
//! into a [`Text`] with spans. Tags are resolved against a [`Theme`] first (so
//! named styles like `[warning]` work) and otherwise parsed as inline style
//! definitions via [`Style::parse`]. `\[` is an escaped literal bracket.
//!
//! Slice scope: the parser is lenient about unbalanced tags (it auto-closes at
//! end of input) rather than raising `MarkupError`. See docs/DIVERGENCES.md.

use crate::errors::Result;
use crate::style::Style;
use crate::text::{Span, Text};
use crate::theme::Theme;

struct RawSpan {
    start: usize,
    end: usize,
    tag: String,
}

/// Parse `markup` into styled [`Text`].
pub fn render(markup: &str, theme: &Theme) -> Result<Text> {
    let mut plain = String::new();
    let mut raw_spans: Vec<RawSpan> = Vec::new();
    let mut stack: Vec<(String, usize)> = Vec::new();

    let bytes = markup.as_bytes();
    let mut chars = markup.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if bytes.get(i + 1) == Some(&b'[') => {
                // Escaped bracket: emit a literal '[' and skip it.
                plain.push('[');
                chars.next();
            }
            '[' => {
                // Read up to the closing ']'.
                let mut tag = String::new();
                let mut closed = false;
                for (_, tc) in chars.by_ref() {
                    if tc == ']' {
                        closed = true;
                        break;
                    }
                    tag.push(tc);
                }
                if !closed {
                    // No closing bracket — treat the '[' as literal text.
                    plain.push('[');
                    plain.push_str(&tag);
                    continue;
                }
                if let Some(name) = tag.strip_prefix('/') {
                    let name = name.trim();
                    let end = plain.len();
                    if name.is_empty() {
                        if let Some((open_tag, start)) = stack.pop() {
                            raw_spans.push(RawSpan {
                                start,
                                end,
                                tag: open_tag,
                            });
                        }
                    } else if let Some(pos) = stack.iter().rposition(|(t, _)| t == name) {
                        let (open_tag, start) = stack.remove(pos);
                        raw_spans.push(RawSpan {
                            start,
                            end,
                            tag: open_tag,
                        });
                    }
                } else {
                    stack.push((tag, plain.len()));
                }
            }
            _ => plain.push(c),
        }
    }

    // Auto-close anything still open (lenient — see module docs).
    let end = plain.len();
    while let Some((open_tag, start)) = stack.pop() {
        raw_spans.push(RawSpan {
            start,
            end,
            tag: open_tag,
        });
    }

    // Resolve tag strings to styles.
    let mut spans: Vec<Span> = Vec::with_capacity(raw_spans.len());
    for raw in raw_spans {
        if raw.start >= raw.end {
            continue;
        }
        let style = resolve(&raw.tag, theme)?;
        spans.push(Span {
            start: raw.start,
            end: raw.end,
            style,
        });
    }
    // Outer spans first so that inner (more nested) spans win when combined.
    spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));

    let mut text = Text::new(plain);
    for span in spans {
        text.push_span(span);
    }
    Ok(text)
}

/// Resolve a tag to a [`Style`]: a theme name if known, else an inline spec.
fn resolve(tag: &str, theme: &Theme) -> Result<Style> {
    let trimmed = tag.trim();
    if let Some(style) = theme.get(trimmed) {
        return Ok(style.clone());
    }
    Style::parse(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render_to_ansi(markup: &str) -> String {
        let theme = Theme::default_theme();
        let text = render(markup, &theme).unwrap();
        let segments = text.render(&Style::new(), Some(ColorSystem::Truecolor));
        segments
            .iter()
            .map(|s| {
                s.style
                    .clone()
                    .unwrap_or_default()
                    .render(&s.text, Some(ColorSystem::Truecolor))
            })
            .collect()
    }

    #[test]
    fn simple_tags() {
        assert_eq!(
            render_to_ansi("[bold]Hello[/] [red]World[/]"),
            "\x1b[1mHello\x1b[0m \x1b[31mWorld\x1b[0m"
        );
    }

    #[test]
    fn nested_inner_wins() {
        // outer red, inner blue -> the inner color should apply to 'x'
        let out = render_to_ansi("[red]a[blue]x[/]b[/]");
        assert_eq!(out, "\x1b[31ma\x1b[0m\x1b[34mx\x1b[0m\x1b[31mb\x1b[0m");
    }

    #[test]
    fn escaped_bracket_is_literal() {
        let theme = Theme::default_theme();
        let text = render("\\[not a tag]", &theme).unwrap();
        assert_eq!(text.plain(), "[not a tag]");
    }

    #[test]
    fn theme_name_resolves() {
        // "error" -> "bold red" -> 1;31
        assert_eq!(render_to_ansi("[error]boom[/]"), "\x1b[1;31mboom\x1b[0m");
    }
}
