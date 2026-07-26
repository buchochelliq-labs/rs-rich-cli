//! Console markup parsing.
//!
//! Port of upstream `rich/markup.py`. Turns `"[bold]Hello[/] [red]World[/]"`
//! into a [`Text`] with spans. Tags are resolved against a [`Theme`] first (so
//! named styles like `[warning]` work) and otherwise parsed as inline style
//! definitions via [`Style::parse`].
//!
//! A `[…]` is only a tag when it starts with `[a-z#/@]` (matching upstream's
//! `RE_TAGS`), so `[Hello]` and `[42]` are literal text. `\[` escapes a bracket,
//! and an unmatched closing tag raises [`RichError::Markup`](crate::RichError).
//! `@`-tags (meta/handlers) apply no visible style.

use crate::errors::{Result, RichError};
use crate::style::Style;
use crate::text::{Span, Text};
use crate::theme::Theme;

struct RawSpan {
    start: usize,
    end: usize,
    tag: String,
}

/// Whether `c` may start a markup tag (upstream's `[a-z#/@]` class).
fn is_tag_start(c: char) -> bool {
    c.is_ascii_lowercase() || c == '#' || c == '/' || c == '@'
}

/// Escape `markup` so it renders literally (no tags interpreted). Port of
/// `rich.markup.escape`: a `\` is inserted before every tag-opening `[`, any
/// pre-existing backslashes are doubled, and a lone trailing `\` is doubled.
pub fn escape(markup: &str) -> String {
    let bytes = markup.as_bytes();
    let mut out = String::with_capacity(markup.len() + 2);
    let mut i = 0;
    while i < bytes.len() {
        // Count a run of backslashes.
        let run_start = i;
        while i < bytes.len() && bytes[i] == b'\\' {
            i += 1;
        }
        let backslashes = i - run_start;
        // A tag opener is `[` followed by a tag-start char.
        let is_opener =
            bytes.get(i) == Some(&b'[') && markup[i + 1..].chars().next().is_some_and(is_tag_start);
        if is_opener {
            // Double the run and add one more before the bracket.
            for _ in 0..backslashes * 2 + 1 {
                out.push('\\');
            }
            out.push('[');
            i += 1;
        } else {
            for _ in 0..backslashes {
                out.push('\\');
            }
            if let Some(c) = markup[i..].chars().next() {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
    // A single trailing backslash is doubled so it can't escape appended text.
    if out.ends_with('\\') && !out.ends_with("\\\\") {
        out.push('\\');
    }
    out
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
            // A '[' only opens a tag when the next char is a tag-start; a tag
            // must then close with ']'. Otherwise the '[' is literal text.
            '[' if chars.peek().is_some_and(|&(_, nc)| is_tag_start(nc)) => {
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
                        // Auto-close: pop the most recent open tag, or error.
                        let (open_tag, start) = stack.pop().ok_or_else(|| {
                            RichError::Markup(format!(
                                "closing tag '[/]' at position {i} has nothing to close"
                            ))
                        })?;
                        raw_spans.push(RawSpan {
                            start,
                            end,
                            tag: open_tag,
                        });
                    } else {
                        // Explicit close: must match an open tag of the same name.
                        let pos = stack.iter().rposition(|(t, _)| t == name).ok_or_else(|| {
                            RichError::Markup(format!(
                                "closing tag '[/{name}]' at position {i} doesn't match any open tag"
                            ))
                        })?;
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
///
/// `@`-prefixed tags are meta/handler tags (links, spans of app data); we don't
/// model those, so they contribute a null style — matching upstream, where they
/// carry no visible styling.
fn resolve(tag: &str, theme: &Theme) -> Result<Style> {
    let trimmed = tag.trim();
    if trimmed.starts_with('@') {
        return Ok(Style::new());
    }
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

    #[test]
    fn bracket_is_literal_unless_tag_start() {
        // Upstream only treats `[a-z#/@]`-led brackets as tags.
        let theme = Theme::default_theme();
        assert_eq!(
            render("[Hello] world", &theme).unwrap().plain(),
            "[Hello] world"
        );
        assert_eq!(render("[42] x", &theme).unwrap().plain(), "[42] x");
    }

    #[test]
    fn hex_tag_and_meta_tag() {
        // `#` starts a tag; `@` tags carry no visible style.
        assert_eq!(
            render_to_ansi("[#ff0000]x[/]"),
            "\x1b[38;2;255;0;0mx\x1b[0m"
        );
        assert_eq!(render_to_ansi("[@foo]y[/]"), "y");
    }

    #[test]
    fn unmatched_closing_tags_error() {
        let theme = Theme::default_theme();
        assert!(render("a[/]b", &theme).is_err());
        assert!(render("[bold]a[/red]", &theme).is_err());
        assert!(render("x[/red]y", &theme).is_err());
        // Unclosed *opening* tags are auto-closed, not an error.
        assert!(render("[bold]hi", &theme).is_ok());
    }

    #[test]
    fn escape_matches_upstream() {
        assert_eq!(escape("[bold]"), "\\[bold]");
        assert_eq!(escape("a[b]c"), "a\\[b]c");
        assert_eq!(escape("back\\slash"), "back\\slash");
        assert_eq!(escape("trailing\\"), "trailing\\\\");
        assert_eq!(escape("[Hello]"), "[Hello]"); // not a tag → unchanged
                                                  // Escaped markup round-trips to the literal text.
        let theme = Theme::default_theme();
        assert_eq!(render(&escape("[bold]"), &theme).unwrap().plain(), "[bold]");
    }
}
