//! Small text-scanning helpers the parsers share.
//!
//! Everything here is linear, or `O(log n)` per query on a precomputed index,
//! so a parser that asks "where does this span close?" for every opener stays
//! fast on a megabyte of unbalanced markers.

use super::model::Inline;

/// How deep the parsers nest lists and inline spans. Deeper content is
/// kept, as text, without further nesting. A block inside a block (a quote,
/// an example, an admonition) counts [`BLOCK_WEIGHT`] levels: each draws a
/// frame or bar around everything inside it, which costs more to draw.
pub(crate) const MAX_NESTING: usize = 16;

/// How many levels of [`MAX_NESTING`] a nested block uses.
pub(crate) const BLOCK_WEIGHT: usize = 2;

/// How much of a document the sniffers read.
pub(crate) const SNIFF_BYTES: usize = 64 * 1024;

/// The source with line endings made `\n`, a leading byte-order mark
/// dropped, and NUL bytes replaced, so a parser sees one kind of line.
pub(crate) fn normalize(source: &str) -> String {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\0' => out.push('\u{fffd}'),
            _ => out.push(ch),
        }
    }
    out
}

/// The start of `text` that the sniffers read: at most [`SNIFF_BYTES`],
/// cut at a character boundary.
pub(crate) fn sample(text: &str) -> &str {
    if text.len() <= SNIFF_BYTES {
        return text;
    }
    let mut end = SNIFF_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// At most the first `max` bytes of `text`, cut at a character boundary.
pub(crate) fn prefix(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Tabs to spaces at `Syntax`'s default tab stop (4), counted in
/// characters, as `Syntax` expands them.
pub(crate) fn expand_tabs(code: &str) -> String {
    if !code.contains('\t') {
        return code.to_string();
    }
    let mut out = String::with_capacity(code.len());
    let mut column = 0;
    for ch in code.chars() {
        match ch {
            '\t' => {
                let spaces = 4 - column % 4;
                out.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            }
            '\n' | '\r' => {
                out.push(ch);
                column = 0;
            }
            _ => {
                out.push(ch);
                column += 1;
            }
        }
    }
    out
}

/// The number of leading spaces of `line`, a tab counting to the next
/// multiple of 8 (as Org and roff count them).
pub(crate) fn indent_of(line: &str) -> usize {
    let mut column = 0;
    for ch in line.chars() {
        match ch {
            ' ' => column += 1,
            '\t' => column = (column / 8 + 1) * 8,
            _ => break,
        }
    }
    column
}

/// `line` without its first `columns` columns of indentation (tabs
/// expanded as [`indent_of`] counts them).
pub(crate) fn dedent(line: &str, columns: usize) -> String {
    let mut column = 0;
    for (index, ch) in line.char_indices() {
        if column >= columns {
            let pad = column - columns;
            return format!("{}{}", " ".repeat(pad), &line[index..]);
        }
        match ch {
            ' ' => column += 1,
            '\t' => column = (column / 8 + 1) * 8,
            _ => {
                return format!(
                    "{}{}",
                    " ".repeat(column.saturating_sub(columns)),
                    &line[index..]
                )
            }
        }
    }
    String::new()
}

/// Sorted positions of something in a text, answering "the first one
/// after `position`" by binary search.
pub(crate) struct Positions(Vec<usize>);

impl Positions {
    /// Every byte offset where `pattern` starts in `text`.
    pub(crate) fn of(text: &str, pattern: &str) -> Self {
        Positions(text.match_indices(pattern).map(|(at, _)| at).collect())
    }

    /// The given offsets, which must be sorted.
    pub(crate) fn from_sorted(positions: Vec<usize>) -> Self {
        Positions(positions)
    }

    /// The first position at or after `position`.
    pub(crate) fn at_or_after(&self, position: usize) -> Option<usize> {
        let index = self.0.partition_point(|&at| at < position);
        self.0.get(index).copied()
    }
}

/// The character before byte offset `at`, if any.
pub(crate) fn char_before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

/// The character at byte offset `at`, if any.
pub(crate) fn char_at(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

/// URL schemes a bare link may start with.
const SCHEMES: &[&str] = &[
    "https://", "http://", "ftp://", "ftps://", "sftp://", "file://", "mailto:", "irc://",
    "ircs://", "news:", "ssh://", "git://",
];

/// The end of a bare URL starting at `at`, if one starts there: up to the
/// first space, `<>"` or `[` (which starts AsciiDoc's link text), with
/// trailing punctuation left out (and a closing parenthesis kept only when
/// the URL opened one).
pub(crate) fn url_end(text: &str, at: usize) -> Option<usize> {
    // Every scheme starts with one of these letters: most positions are
    // ruled out by one comparison.
    let first = text.as_bytes().get(at)?.to_ascii_lowercase();
    if !matches!(first, b'h' | b'f' | b's' | b'm' | b'i' | b'n' | b'g') {
        return None;
    }
    let rest = &text[at..];
    let scheme = SCHEMES
        .iter()
        .find(|scheme| starts_with_ignore_case(rest, scheme))?;
    if char_before(text, at).is_some_and(|c| c.is_alphanumeric() || c == '/' || c == '.') {
        return None;
    }
    let mut end = at + scheme.len();
    for (offset, ch) in text[end..].char_indices() {
        // `[` right after the scheme is an IPv6 host, `http://[::1]/`.
        let bracket = ch == '[' && offset > 0;
        if ch.is_whitespace() || bracket || matches!(ch, '<' | '>' | '"' | '`' | '\u{0}') {
            end += offset;
            return trim_url(text, at, end, scheme.len());
        }
    }
    trim_url(text, at, text.len(), scheme.len())
}

fn trim_url(text: &str, at: usize, mut end: usize, scheme: usize) -> Option<usize> {
    while let Some(last) = char_before(text, end) {
        let unbalanced_close =
            last == ')' && text[at..end].matches('(').count() < text[at..end].matches(')').count();
        let unbalanced_bracket =
            last == ']' && text[at..end].matches('[').count() < text[at..end].matches(']').count();
        if matches!(
            last,
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_' | '~'
        ) || unbalanced_close
            || unbalanced_bracket
        {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    (end > at + scheme).then_some(end)
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.is_char_boundary(prefix.len())
        && text[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// Appends text to a run of inlines, merging it into the last text inline.
#[derive(Default)]
pub(crate) struct InlineBuf {
    pub(crate) inlines: Vec<Inline>,
    text: String,
}

impl InlineBuf {
    pub(crate) fn push_str(&mut self, text: &str) {
        self.text.push_str(text);
    }

    pub(crate) fn push(&mut self, ch: char) {
        self.text.push(ch);
    }

    pub(crate) fn inline(&mut self, inline: Inline) {
        self.flush();
        self.inlines.push(inline);
    }

    fn flush(&mut self) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            match self.inlines.last_mut() {
                Some(Inline::Text(last)) => last.push_str(&text),
                _ => self.inlines.push(Inline::Text(text)),
            }
        }
    }

    pub(crate) fn finish(mut self) -> Vec<Inline> {
        self.flush();
        self.inlines
    }
}

/// Paragraph text from source lines: trimmed lines joined by `\n`, which
/// the inline parsers read as a space (a renderer wraps the text anyway)
/// while still knowing where the source lines broke.
pub(crate) fn join_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// Trims trailing whitespace from a run of inlines' last text, and leading
/// whitespace from its first.
pub(crate) fn trim_inlines(mut inlines: Vec<Inline>) -> Vec<Inline> {
    if let Some(Inline::Text(first)) = inlines.first_mut() {
        let trimmed = first.trim_start().to_string();
        *first = trimmed;
    }
    if let Some(Inline::Text(last)) = inlines.last_mut() {
        let trimmed = last.trim_end().to_string();
        *last = trimmed;
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
    inlines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_end_before_trailing_punctuation() {
        let text = "see https://example.com/a_(b), then.";
        let start = text.find("https").unwrap();
        let end = url_end(text, start).unwrap();
        assert_eq!(&text[start..end], "https://example.com/a_(b)");
        let text = "(https://example.com/x).";
        let start = text.find("https").unwrap();
        assert_eq!(
            &text[start..url_end(text, start).unwrap()],
            "https://example.com/x"
        );
        assert_eq!(url_end("xhttps://a.b", 1), None);
        assert_eq!(url_end("https://", 0), None);
    }

    #[test]
    fn indentation_counts_tabs_to_eight() {
        assert_eq!(indent_of("\t- a"), 8);
        assert_eq!(indent_of("  \t- a"), 8);
        assert_eq!(dedent("\t- a", 2), "      - a");
        assert_eq!(dedent("    x", 2), "  x");
    }

    #[test]
    fn line_endings_are_normalised() {
        assert_eq!(normalize("\u{feff}a\r\nb\rc\0"), "a\nb\nc\u{fffd}");
    }
}
