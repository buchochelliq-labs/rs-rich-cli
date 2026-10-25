//! Page text made safe for a terminal. Everything a page says is
//! untrusted: its text, title, addresses, alt text, option labels and
//! download names all pass through here before any of it is drawn.

/// Whether `c` must never reach the terminal: a C0 or C1 control
/// character (so no escape sequence can start), DEL, or a bidirectional
/// override, embedding or isolate, which could make text read in a false
/// order (`exe.png` shown for `gnp.exe`).
pub fn is_unsafe(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}'
        )
}

/// `text` safe for a terminal, its whitespace collapsed to single spaces
/// and trimmed: the way a browser lays out ordinary text.
pub fn clean(text: &str) -> String {
    let kept: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|&c| !is_unsafe(c))
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// [`clean`], but for preformatted text: line breaks are kept, tabs become
/// spaces up to the next multiple of four, and nothing else is collapsed.
pub fn clean_pre(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        match c {
            '\n' => {
                out.push('\n');
                column = 0;
            }
            '\t' => {
                let next = (column / 4 + 1) * 4;
                out.extend(std::iter::repeat_n(' ', next - column));
                column = next;
            }
            '\r' => {}
            c if is_unsafe(c) => {}
            c => {
                out.push(c);
                column += 1;
            }
        }
    }
    out
}

/// Inline text: like [`clean`], but a space at either end is kept as one
/// space, since it separates this run from the next.
pub fn clean_inline(text: &str) -> String {
    let body = clean(text);
    if body.is_empty() {
        return if text.chars().any(char::is_whitespace) {
            " ".into()
        } else {
            String::new()
        };
    }
    let lead = text.starts_with(char::is_whitespace);
    let trail = text.ends_with(char::is_whitespace);
    format!(
        "{}{body}{}",
        if lead { " " } else { "" },
        if trail { " " } else { "" }
    )
}

/// `text` cleaned and escaped for console markup, so `[bold]` written by a
/// page shows as text, not as a style.
pub fn markup(text: &str) -> String {
    rich::markup::escape(&clean(text))
}

/// At most `max` characters of `text`, with `…` when cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_control_character_survives() {
        assert_eq!(clean("a\x1b[31mb\u{9b}2Jc\u{202e}d\u{7f}"), "a[31mb2Jcd");
        assert_eq!(clean("  two\n\t lines  "), "two lines");
        assert_eq!(clean("x\u{2066}y\u{2069}\u{200f}z"), "xyz");
        for text in [
            clean("\x07\x1b]0;t\x07"),
            clean_pre("\x1b[2J\n\x1b"),
            clean_inline(" \x1b "),
        ] {
            assert!(!text.chars().any(|c| c != '\n' && is_unsafe(c)), "{text:?}");
        }
    }

    #[test]
    fn preformatted_text_keeps_its_lines() {
        assert_eq!(clean_pre("a\tb\r\n  c\x1b"), "a   b\n  c");
    }

    #[test]
    fn inline_runs_keep_one_space_at_their_ends() {
        assert_eq!(clean_inline(" of  the\n "), " of the ");
        assert_eq!(clean_inline("\n  "), " ");
        assert_eq!(clean_inline(""), "");
    }

    #[test]
    fn page_markup_is_escaped() {
        assert_eq!(markup("[bold]x[/]"), "\\[bold]x\\[/]");
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
    }
}
