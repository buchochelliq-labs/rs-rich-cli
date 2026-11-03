//! Agent text is untrusted ([the design note]'s security model, rule 6):
//! control sequences are neutralised before anything reaches the terminal,
//! and the characters that hide or reorder text (bidirectional controls,
//! zero-width characters, tags) are revealed where the user decides on
//! something: commands, paths, titles, option names.
//!
//! [the design note]: https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/cornacai.md

use rich_intuituive::rich::markup::escape;

/// `text` with every control character but the line break and the tab
/// replaced by `�`: an escape sequence from an agent (or a terminal's
/// output) cannot move the cursor, retitle the window or write to the
/// clipboard. A carriage return before a line break is dropped.
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' | '\t' => out.push(c),
            '\r' if chars.peek() == Some(&'\n') => {}
            c if c.is_control() => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

/// Whether `c` hides or reorders text: a bidirectional control, a
/// zero-width character, a tag, or the byte-order mark.
pub fn is_hidden(c: char) -> bool {
    matches!(c,
        '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{feff}'
        | '\u{e0000}'..='\u{e007f}')
}

/// [`clean`] `text`, with each [hidden](is_hidden) character shown as its
/// code point, `<U+202E>`: what a permission prompt, a path or a command
/// shows.
pub fn reveal(text: &str) -> String {
    let cleaned = clean(text);
    if !cleaned.chars().any(is_hidden) {
        return cleaned;
    }
    let mut out = String::with_capacity(cleaned.len() + 8);
    for c in cleaned.chars() {
        if is_hidden(c) {
            out.push_str(&format!("<U+{:04X}>", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// [`reveal`] `text`, escaped for console markup: the agent's words in a
/// label.
pub fn markup(text: &str) -> String {
    escape(&reveal(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_are_neutralised_and_line_breaks_kept() {
        assert_eq!(clean("a\x1b[2Jb\r\nc\td"), "a\u{fffd}[2Jb\nc\td");
        assert_eq!(clean("\u{9b}31m"), "\u{fffd}31m");
    }

    #[test]
    fn hidden_characters_are_revealed() {
        assert_eq!(reveal("rm -rf /\u{202e}gpj.txt"), "rm -rf /<U+202E>gpj.txt");
        assert_eq!(reveal("a\u{200b}b"), "a<U+200B>b");
        assert_eq!(reveal("plain"), "plain");
    }

    #[test]
    fn markup_is_escaped() {
        assert_eq!(markup("[red]x[/]"), "\\[red]x\\[/]");
    }
}
