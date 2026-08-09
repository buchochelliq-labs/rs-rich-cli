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

use fancy_regex::Regex;

use crate::errors::{Result, RichError};
use crate::style::{Style, StyleType};
use crate::text::{Span, Text};

struct RawSpan {
    start: usize,
    end: usize,
    /// The tag's normalized name, e.g. `bold` for `[b]`.
    name: String,
    /// Anything after the first `=`, e.g. the URL of `[link=https://…]`.
    parameters: Option<String>,
}

/// Whether `c` may start a markup tag (the `[a-z#/@]` class in `RE_TAGS`).
/// Used by [`escape`], which decides where to insert backslashes and so needs
/// the same notion of "this bracket would open a tag".
fn is_tag_start(c: char) -> bool {
    c.is_ascii_lowercase() || c == '#' || c == '/' || c == '@'
}

/// Upstream's `RE_TAGS`, verbatim.
///
/// Using the same expression rather than hand-scanning is deliberate. Two of its
/// details are easy to get wrong by hand and both were wrong here before:
///
/// - `(\\*)` captures the **whole run** of preceding backslashes, so escaping
///   can be decided by parity. An even-length run is *not* an escape: it emits
///   half as many literal backslashes and the tag still fires.
/// - `[^\[]*?` forbids a `[` inside the tag body, so `[a[b]` is not a tag at
///   all — it is literal text, and scanning resumes at the inner `[`.
static RE_TAGS: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"((\\*)\[([a-z#/@][^\[]*?)\])").expect("RE_TAGS is a valid pattern")
});

/// One item from the scanner: either literal text or a tag.
enum Event<'a> {
    Text(String),
    Tag {
        name: &'a str,
        parameters: Option<String>,
        /// Byte offset in the source markup, used only for error messages.
        position: usize,
    },
}

/// Split `markup` into text and tag events. Direct port of `markup._parse`.
fn parse(markup: &str) -> Result<Vec<Event<'_>>> {
    let mut events: Vec<Event> = Vec::new();
    let mut position = 0usize;

    for captures in RE_TAGS.captures_iter(markup) {
        let captures =
            captures.map_err(|e| RichError::Markup(format!("markup scan failed: {e}")))?;
        let whole = captures.get(1).expect("group 1 always participates");
        let escapes = captures.get(2).map_or("", |m| m.as_str());
        let tag_text = captures
            .get(3)
            .expect("group 3 always participates")
            .as_str();
        let (mut start, end) = (whole.start(), whole.end());

        if start > position {
            events.push(Event::Text(unescape_brackets(&markup[position..start])));
        }

        if !escapes.is_empty() {
            // `divmod(len(escapes), 2)`: pairs collapse to one literal
            // backslash each, and only an odd remainder escapes the tag.
            let (backslashes, escaped) = (escapes.len() / 2, escapes.len() % 2 == 1);
            if backslashes > 0 {
                events.push(Event::Text("\\".repeat(backslashes)));
                start += backslashes * 2;
            }
            if escaped {
                // The tag is escaped: emit it as literal text, minus its
                // backslashes, and do not open anything.
                events.push(Event::Text(whole.as_str()[escapes.len()..].to_string()));
                position = end;
                continue;
            }
        }

        // Everything after the first `=` is the tag's parameters, not part of
        // its name — so `[link=url]` closes with `[/link]`.
        let (name, parameters) = match tag_text.split_once('=') {
            Some((name, params)) => (name, Some(params.to_string())),
            None => (tag_text, None),
        };
        events.push(Event::Tag {
            name,
            parameters,
            position: start,
        });
        position = end;
    }

    if position < markup.len() {
        events.push(Event::Text(unescape_brackets(&markup[position..])));
    }
    Ok(events)
}

/// `\[` in ordinary text is a literal bracket. Upstream applies exactly this
/// replacement to every text run it yields.
fn unescape_brackets(text: &str) -> String {
    text.replace("\\[", "[")
}

/// Append text to the plain buffer, dropping control codes as it goes.
///
/// Stripping has to happen *here*, not later in `Text::new`: span offsets are
/// computed against `plain` as it is built, so removing bytes afterwards shifts
/// the text out from under them — which panicked on any markup containing a
/// carriage return.
fn push_plain(plain: &mut String, chunk: &str) {
    if chunk.chars().any(crate::text::is_control_code) {
        plain.extend(chunk.chars().filter(|c| !crate::text::is_control_code(*c)));
    } else {
        plain.push_str(chunk);
    }
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
///
/// Tag names are stored unresolved on the spans, so the returned `Text` renders
/// in whichever theme prints it. The `Result` therefore reports only genuine
/// *syntax* errors — an unmatched or mismatched closing tag. An unknown tag
/// *name* is not an error: it renders as a no-op, as it does upstream.
pub fn render(markup: &str) -> Result<Text> {
    let mut plain = String::new();
    let mut raw_spans: Vec<RawSpan> = Vec::new();
    // Open tags, as `(normalized name, parameters, start offset)`. The name is
    // normalized on the way in so that `[b]…[/bold]` matches, as upstream does.
    let mut stack: Vec<(String, Option<String>, usize)> = Vec::new();

    for event in parse(markup)? {
        match event {
            Event::Text(chunk) => push_plain(&mut plain, &chunk),
            Event::Tag {
                name: tag_name,
                parameters,
                position: i,
            } => {
                if let Some(name) = tag_name.strip_prefix('/') {
                    let name = name.trim();
                    let end = plain.len();
                    let (open_name, open_parameters, start) = if name.is_empty() {
                        // Auto-close: pop the most recent open tag, or error.
                        stack.pop().ok_or_else(|| {
                            RichError::Markup(format!(
                                "closing tag '[/]' at position {i} has nothing to close"
                            ))
                        })?
                    } else {
                        // Explicit close. Both sides are normalized first, so
                        // `[b]…[/bold]` matches — upstream normalizes the open
                        // tag's name on push and the close name here.
                        let wanted = Style::normalize(name);
                        let pos = stack
                            .iter()
                            .rposition(|(open, _, _)| *open == wanted)
                            .ok_or_else(|| {
                                RichError::Markup(format!(
                                "closing tag '[/{name}]' at position {i} doesn't match any open tag"
                            ))
                            })?;
                        stack.remove(pos)
                    };
                    raw_spans.push(RawSpan {
                        start,
                        end,
                        name: open_name,
                        parameters: open_parameters,
                    });
                } else {
                    stack.push((Style::normalize(tag_name), parameters, plain.len()));
                }
            }
        }
    }

    // Auto-close anything still open (lenient — see module docs).
    let end = plain.len();
    while let Some((open_name, open_parameters, start)) = stack.pop() {
        raw_spans.push(RawSpan {
            start,
            end,
            name: open_name,
            parameters: open_parameters,
        });
    }

    // Carry tag strings through as names — the theme of whichever console
    // renders this text resolves them. Resolving here instead would freeze the
    // colours at parse time and make an unknown tag an error rather than the
    // no-op upstream produces.
    let mut spans: Vec<Span> = Vec::with_capacity(raw_spans.len());
    for raw in raw_spans {
        // Zero-length spans are NOT skipped. Upstream keeps them, and they
        // still contribute a boundary point when segments are cut, so
        // `[b]a[i][/i]b[/b]` emits two runs rather than one merged run. Same
        // colours either way — different bytes.
        // `@`-prefixed tags are meta/handler tags (spans of app data). We don't
        // model those, so they carry no styling — stated explicitly rather than
        // left to fall out of a failed parse.
        let style = if raw.name.starts_with('@') {
            StyleType::Style(Style::new())
        } else {
            // Upstream's `str(Tag)`: the name, or `"{name} {parameters}"`. That
            // is what turns `[link=https://x]` into the style `link https://x`,
            // which `Style::parse` then understands.
            StyleType::Name(match &raw.parameters {
                Some(parameters) => format!("{} {}", raw.name, parameters),
                None => raw.name.clone(),
            })
        };
        spans.push(Span {
            start: raw.start,
            end: raw.end,
            style,
        });
    }
    // Outer spans first, so inner (more nested) spans are combined last and win.
    //
    // Upstream is `sorted(spans[::-1], key=attrgetter("start"))`, and both halves
    // matter. Spans are pushed in *closing* order, innermost first, so the
    // reverse puts the outermost first; the sort then keys on `start` **only**,
    // and being stable it preserves that reversal for ties. Sorting by
    // `(start, end desc)` instead looks equivalent but is not: two tags covering
    // the exact same range compare Equal, the innermost stays first, and the
    // outer tag ends up winning. `[red][blue]x[/][/]` must render blue.
    spans.reverse();
    spans.sort_by_key(|span| span.start);

    let mut text = Text::new(plain);
    for span in spans {
        text.push_span(span);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::theme::Theme;

    fn render_to_ansi(markup: &str) -> String {
        let text = render(markup).unwrap();
        let segments = text.render(&Theme::default_theme(), &Style::new());
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

    /// A control code must never reach `plain`, because span offsets are
    /// computed against it as it is built. Stripping later shifts the text out
    /// from under the offsets, and a boundary landing inside a multi-byte
    /// character panics on slicing.
    ///
    /// Regression: `rich --print` on any input containing a CR — i.e. any file
    /// with CRLF line endings — panicked.
    #[test]
    fn control_codes_never_desynchronise_span_offsets() {
        let text = render("a\r[red]\u{2593}x[/]").expect("valid markup");
        assert_eq!(text.plain(), "a\u{2593}x");
        for span in text.spans() {
            assert!(
                text.plain().is_char_boundary(span.start)
                    && text.plain().is_char_boundary(span.end),
                "span {span:?} does not land on char boundaries of {:?}",
                text.plain()
            );
        }
        // Rendering is what actually panicked, so exercise it.
        let rendered = render_to_ansi("a\r[red]\u{2593}x[/]");
        assert!(rendered.contains('\u{2593}'), "got {rendered:?}");
    }

    #[test]
    fn nested_inner_wins() {
        // outer red, inner blue -> the inner color should apply to 'x'
        let out = render_to_ansi("[red]a[blue]x[/]b[/]");
        assert_eq!(out, "\x1b[31ma\x1b[0m\x1b[34mx\x1b[0m\x1b[31mb\x1b[0m");
    }

    #[test]
    fn escaped_bracket_is_literal() {
        let text = render("\\[not a tag]").unwrap();
        assert_eq!(text.plain(), "[not a tag]");
    }

    #[test]
    fn theme_name_resolves() {
        // An upstream style name resolves via the theme rather than being
        // parsed as an inline definition: "repr.number" -> bold not-italic cyan.
        // Captured from real rich 15.0.0: `[repr.number]7[/]`.
        assert_eq!(render_to_ansi("[repr.number]7[/]"), "\x1b[1;36m7\x1b[0m");
    }

    #[test]
    fn none_is_the_null_style() {
        // Upstream's `Style.parse` special-cases a bare "none" (many
        // DEFAULT_STYLES entries are exactly that); as a *word* inside a longer
        // definition it is still an error.
        assert!(Style::parse("none").unwrap().is_null());
        assert!(Style::parse("").unwrap().is_null());
        assert!(Style::parse("bold none").is_err());
    }

    #[test]
    fn bracket_is_literal_unless_tag_start() {
        // Upstream only treats `[a-z#/@]`-led brackets as tags.
        assert_eq!(render("[Hello] world").unwrap().plain(), "[Hello] world");
        assert_eq!(render("[42] x").unwrap().plain(), "[42] x");
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
        assert!(render("a[/]b").is_err());
        assert!(render("[bold]a[/red]").is_err());
        assert!(render("x[/red]y").is_err());
        // Unclosed *opening* tags are auto-closed, not an error.
        assert!(render("[bold]hi").is_ok());
    }

    #[test]
    fn escape_matches_upstream() {
        assert_eq!(escape("[bold]"), "\\[bold]");
        assert_eq!(escape("a[b]c"), "a\\[b]c");
        assert_eq!(escape("back\\slash"), "back\\slash");
        assert_eq!(escape("trailing\\"), "trailing\\\\");
        assert_eq!(escape("[Hello]"), "[Hello]"); // not a tag → unchanged
                                                  // Escaped markup round-trips to the literal text.
        assert_eq!(render(&escape("[bold]")).unwrap().plain(), "[bold]");
    }
}
