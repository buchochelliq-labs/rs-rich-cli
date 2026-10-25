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
use crate::style::{Meta, MetaValue, Style, StyleType};
use crate::text::{Span, Text};

struct RawSpan {
    start: usize,
    end: usize,
    style: StyleType,
}

/// A tag in console markup. Port of `rich.markup.Tag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// The tag name, e.g. `bold`.
    pub name: String,
    /// Anything after the first `=`, e.g. the URL of `[link=https://…]`.
    pub parameters: Option<String>,
}

impl Tag {
    /// A tag named `name`, with optional `parameters`.
    pub fn new(name: impl Into<String>, parameters: Option<String>) -> Self {
        Tag {
            name: name.into(),
            parameters,
        }
    }

    /// The tag as markup: `[name]` or `[name=parameters]`. Port of
    /// `Tag.markup`.
    pub fn markup(&self) -> String {
        match &self.parameters {
            None => format!("[{}]", self.name),
            Some(parameters) => format!("[{}={}]", self.name, parameters),
        }
    }
}

/// `str(tag)`: the name, or `"{name} {parameters}"`.
impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.parameters {
            None => f.write_str(&self.name),
            Some(parameters) => write!(f, "{} {}", self.name, parameters),
        }
    }
}

/// Python's `repr()` of a string.
pub(crate) fn py_repr_str(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
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
    // Without a backslash there is nothing to replace: copy it without
    // setting up the substring search (docs/PORTING.md, "Performance notes").
    if !text.contains('\\') {
        return text.to_string();
    }
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
    render_inner(markup, false, &StyleType::default())
}

/// [`render`] with a base `style` for the whole text and, when `emoji` is
/// set, `:emoji:` codes replaced. Port of `markup.render(markup, style,
/// emoji, emoji_variant)`: a string with no `[` is replaced whole, with
/// `emoji_variant`, and not parsed.
pub fn render_with(
    markup: &str,
    style: impl Into<StyleType>,
    emoji: bool,
    emoji_variant: Option<crate::emoji::EmojiVariant>,
) -> Result<Text> {
    let style = style.into();
    if !markup.contains('[') {
        let plain = if emoji {
            crate::emoji::replace_with_variant(markup, emoji_variant)
        } else {
            markup.to_string()
        };
        return Ok(Text::styled(plain, style));
    }
    render_inner(markup, emoji, &style)
}

/// [`render`] with `:emoji:` codes replaced, as upstream's
/// `markup.render(emoji=True)` does: only in the text *between* tags, one
/// chunk at a time, so tag names and parameters are left alone and a
/// [`MarkupError`](RichError::Markup) position counts characters of the
/// original string, not of the emoji-replaced one.
///
/// Callers handle a string with no `[` themselves: upstream replaces it whole,
/// with the console's default variant, instead of parsing it.
pub fn render_emoji(markup: &str) -> Result<Text> {
    render_inner(markup, true, &StyleType::default())
}

/// `RE_HANDLER`: `^([\w.]*?)(\(.*?\))?$`, as `(handler name, parameters)`.
fn match_handler(parameters: &str) -> Option<(&str, Option<&str>)> {
    let is_name = |c: char| c.is_alphanumeric() || c == '_' || c == '.';
    // The lazy name stops at the first `(` that starts a group running to the
    // end; without one, the whole string must be a name.
    if parameters.chars().all(is_name) {
        return Some((parameters, None));
    }
    let open = parameters.find(|c: char| !is_name(c))?;
    let (name, rest) = parameters.split_at(open);
    // `\(.*?\)` then `$`: the group must start with `(` and end at the last
    // character with `)` (`.` does not match a newline).
    if rest.starts_with('(') && rest.ends_with(')') && rest.len() >= 2 && !rest.contains('\n') {
        return Some((name, Some(rest)));
    }
    None
}

/// The meta of a closed `@` tag. Port of the `@` branch of `markup.render`:
/// `{name: ()}` with no parameters, `{name: (handler, args)}` for a handler
/// call like `foo(1, 'x')`, otherwise `{name: literal_eval(parameters)}`.
fn handler_meta(name: &str, parameters: Option<&str>) -> Result<Meta> {
    let mut meta = Meta::new();
    let Some(raw) = parameters.filter(|parameters| !parameters.is_empty()) else {
        meta.insert(name, MetaValue::Tuple(Vec::new()));
        return Ok(meta);
    };
    let mut parameters = raw.trim();
    let mut handler_name = "";
    if let Some((handler, arguments)) = match_handler(parameters) {
        handler_name = handler;
        parameters = arguments.unwrap_or("()");
    }
    let value = literal_eval(parameters).map_err(|error| {
        RichError::Markup(match error {
            LiteralError::Syntax(message) => format!(
                "error parsing {} in {}; {message}",
                py_repr_str(parameters),
                py_repr_str(raw)
            ),
            LiteralError::Value(message) => {
                format!("error parsing {}; {message}", py_repr_str(raw))
            }
        })
    })?;
    let value = if handler_name.is_empty() {
        value
    } else {
        let arguments = match value {
            MetaValue::Tuple(items) => MetaValue::Tuple(items),
            other => MetaValue::Tuple(vec![other]),
        };
        MetaValue::Tuple(vec![MetaValue::Str(handler_name.to_string()), arguments])
    };
    meta.insert(name, value);
    Ok(meta)
}

/// Why [`literal_eval`] failed: upstream's `SyntaxError` (with its message)
/// or any other exception (`ValueError` for a name or an operator).
#[derive(Debug, PartialEq)]
enum LiteralError {
    Syntax(String),
    Value(String),
}

/// Python's `ast.literal_eval` for the values a style's meta can hold:
/// numbers (with one sign), strings, `True`, `False`, `None`, tuples and
/// lists. Anything else is a `ValueError` (upstream's message names an AST
/// node at a memory address, which this one leaves out).
fn literal_eval(source: &str) -> std::result::Result<MetaValue, LiteralError> {
    tokenize_check(source)?;
    let mut parser = LiteralParser {
        chars: source
            .trim_matches(|c| c == ' ' || c == '\t')
            .chars()
            .collect(),
        position: 0,
    };
    let value = parser.expression()?;
    parser.skip_space();
    if parser.position < parser.chars.len() {
        return Err(parser.trailing());
    }
    Ok(value)
}

/// The errors Python's tokenizer raises before any parsing: an unterminated
/// string, and unbalanced or mismatched brackets.
fn tokenize_check(source: &str) -> std::result::Result<(), LiteralError> {
    let syntax = |message: String| Err(LiteralError::Syntax(message));
    let mut open: Vec<char> = Vec::new();
    let mut chars = source.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' => loop {
                match chars.next() {
                    None => {
                        return syntax(
                            "unterminated string literal (detected at line 1)".to_string(),
                        )
                    }
                    Some('\\') => {
                        chars.next();
                    }
                    Some(end) if end == c => break,
                    Some(_) => {}
                }
            },
            '(' | '[' | '{' => open.push(c),
            ')' | ']' | '}' => match open.pop() {
                None => return syntax(format!("unmatched '{c}'")),
                Some(opener) => {
                    let expected = match opener {
                        '(' => ')',
                        '[' => ']',
                        _ => '}',
                    };
                    if c != expected {
                        return syntax(format!(
                            "closing parenthesis '{c}' does not match opening parenthesis '{opener}'"
                        ));
                    }
                }
            },
            _ => {}
        }
    }
    match open.last() {
        Some(opener) => syntax(format!("'{opener}' was never closed")),
        None => Ok(()),
    }
}

struct LiteralParser {
    chars: Vec<char>,
    position: usize,
}

impl LiteralParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.position += 1;
        }
    }

    fn malformed() -> LiteralError {
        LiteralError::Value("malformed node or string".to_string())
    }

    /// Something follows a complete value: a name or operator is valid
    /// Python but not a literal; anything else is a syntax error.
    fn trailing(&self) -> LiteralError {
        match self.peek() {
            Some(c) if "+-*/%&|^<>=.@[(".contains(c) => Self::malformed(),
            Some(')') => LiteralError::Syntax("unmatched ')'".to_string()),
            Some(']') => LiteralError::Syntax("unmatched ']'".to_string()),
            _ => LiteralError::Syntax("invalid syntax".to_string()),
        }
    }

    fn expression(&mut self) -> std::result::Result<MetaValue, LiteralError> {
        self.skip_space();
        let Some(c) = self.peek() else {
            return Err(LiteralError::Syntax("invalid syntax".to_string()));
        };
        match c {
            '(' | '[' => self.sequence(c),
            '\'' | '"' => {
                let mut value = self.string()?;
                // Adjacent string literals concatenate.
                loop {
                    self.skip_space();
                    match self.peek() {
                        Some('\'' | '"') => value.push_str(&self.string()?),
                        _ => break,
                    }
                }
                Ok(MetaValue::Str(value))
            }
            '+' | '-' => {
                self.position += 1;
                self.skip_space();
                match self.peek() {
                    Some(d) if d.is_ascii_digit() || d == '.' => match self.number()? {
                        MetaValue::Int(value) if c == '-' => Ok(MetaValue::Int(-value)),
                        MetaValue::Float(value) if c == '-' => Ok(MetaValue::Float(-value)),
                        value => Ok(value),
                    },
                    _ => Err(Self::malformed()),
                }
            }
            c if c.is_ascii_digit() || c == '.' => self.number(),
            c if c.is_alphabetic() || c == '_' => {
                let start = self.position;
                while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    self.position += 1;
                }
                let word: String = self.chars[start..self.position].iter().collect();
                match word.as_str() {
                    "True" => Ok(MetaValue::Bool(true)),
                    "False" => Ok(MetaValue::Bool(false)),
                    "None" => Ok(MetaValue::None),
                    _ => Err(Self::malformed()),
                }
            }
            '{' => Err(Self::malformed()),
            _ => Err(LiteralError::Syntax("invalid syntax".to_string())),
        }
    }

    fn sequence(&mut self, open: char) -> std::result::Result<MetaValue, LiteralError> {
        let close = if open == '(' { ')' } else { ']' };
        self.position += 1;
        let mut items = Vec::new();
        let mut trailing_comma = false;
        loop {
            self.skip_space();
            match self.peek() {
                None => return Err(LiteralError::Syntax(format!("'{open}' was never closed"))),
                Some(c) if c == close => {
                    self.position += 1;
                    break;
                }
                _ => {}
            }
            items.push(self.expression()?);
            self.skip_space();
            match self.peek() {
                Some(',') => {
                    self.position += 1;
                    trailing_comma = true;
                }
                Some(c) if c == close => {
                    trailing_comma = false;
                }
                None => return Err(LiteralError::Syntax(format!("'{open}' was never closed"))),
                Some(_) => return Err(self.trailing()),
            }
        }
        Ok(if open == '[' {
            MetaValue::List(items)
        } else if items.len() == 1 && !trailing_comma {
            // `(x)` is just `x`.
            items.pop().expect("one item")
        } else {
            MetaValue::Tuple(items)
        })
    }

    fn string(&mut self) -> std::result::Result<String, LiteralError> {
        let quote = self.peek().expect("a quote");
        self.position += 1;
        let mut value = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(LiteralError::Syntax(
                    "unterminated string literal (detected at line 1)".to_string(),
                ));
            };
            self.position += 1;
            if c == quote {
                return Ok(value);
            }
            if c != '\\' {
                value.push(c);
                continue;
            }
            let Some(escaped) = self.peek() else {
                return Err(LiteralError::Syntax(
                    "unterminated string literal (detected at line 1)".to_string(),
                ));
            };
            self.position += 1;
            match escaped {
                'n' => value.push('\n'),
                't' => value.push('\t'),
                'r' => value.push('\r'),
                '0' => value.push('\0'),
                '\\' | '\'' | '"' => value.push(escaped),
                other => {
                    value.push('\\');
                    value.push(other);
                }
            }
        }
    }

    fn number(&mut self) -> std::result::Result<MetaValue, LiteralError> {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            let c = self.peek().expect("checked");
            self.position += 1;
            // An exponent's sign belongs to the number.
            if matches!(c, 'e' | 'E') && matches!(self.peek(), Some('+' | '-')) {
                let radix_prefix = self.chars[start..].starts_with(&['0', 'x'])
                    || self.chars[start..].starts_with(&['0', 'X']);
                if !radix_prefix {
                    self.position += 1;
                }
            }
        }
        let word: String = self.chars[start..self.position]
            .iter()
            .filter(|&&c| c != '_')
            .collect();
        let invalid = || LiteralError::Syntax("invalid syntax".to_string());
        let lower = word.to_ascii_lowercase();
        for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
            if let Some(digits) = lower.strip_prefix(prefix) {
                return i64::from_str_radix(digits, radix)
                    .map(MetaValue::Int)
                    .map_err(|_| invalid());
            }
        }
        if word.chars().all(|c| c.is_ascii_digit()) {
            // Python rejects a leading zero on a non-zero decimal integer.
            if word.len() > 1 && word.starts_with('0') && word.chars().any(|c| c != '0') {
                return Err(invalid());
            }
            return word.parse().map(MetaValue::Int).map_err(|_| invalid());
        }
        word.parse::<f64>()
            .ok()
            .filter(|_| !lower.contains("inf") && !lower.contains("nan"))
            .map(MetaValue::Float)
            .ok_or_else(invalid)
    }
}

fn render_inner(markup: &str, emoji: bool, base_style: &StyleType) -> Result<Text> {
    let mut plain = String::new();
    let mut raw_spans: Vec<RawSpan> = Vec::new();
    // Open tags, as `(normalized name, parameters, start offset)`. The name is
    // normalized on the way in so that `[b]…[/bold]` matches, as upstream does.
    let mut stack: Vec<(String, Option<String>, usize)> = Vec::new();

    for event in parse(markup)? {
        match event {
            Event::Text(chunk) if emoji => push_plain(&mut plain, &crate::emoji::replace(&chunk)),
            Event::Text(chunk) => push_plain(&mut plain, &chunk),
            Event::Tag {
                name: tag_name,
                parameters,
                position,
            } => {
                // Upstream reports the position in characters (a Python
                // string index), not bytes. Counted only for an error: doing
                // it per tag would make long markup quadratic.
                let at = || markup[..position].chars().count();
                if let Some(name) = tag_name.strip_prefix('/') {
                    let name = name.trim();
                    let end = plain.len();
                    // `tag.markup` of the closing tag, as written.
                    let closing = Tag::new(tag_name, parameters.clone()).markup();
                    let (open_name, open_parameters, start) = if name.is_empty() {
                        // Auto-close: pop the most recent open tag, or error.
                        stack.pop().ok_or_else(|| {
                            RichError::Markup(format!(
                                "closing tag '[/]' at position {} has nothing to close",
                                at()
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
                                    "closing tag '{closing}' at position {} doesn't match any open tag",
                                    at()
                                ))
                            })?;
                        stack.remove(pos)
                    };
                    let style = if open_name.starts_with('@') {
                        StyleType::Style(Style::from_meta(handler_meta(
                            &open_name,
                            open_parameters.as_deref(),
                        )?))
                    } else {
                        StyleType::Name(Tag::new(open_name, open_parameters).to_string())
                    };
                    raw_spans.push(RawSpan { start, end, style });
                } else {
                    stack.push((Style::normalize(tag_name), parameters, plain.len()));
                }
            }
        }
    }

    // Auto-close anything still open (lenient — see module docs).
    let end = plain.len();
    // Upstream gives these the tag string as a style name, `@` tags included:
    // the name resolves to no style when rendered.
    while let Some((open_name, open_parameters, start)) = stack.pop() {
        raw_spans.push(RawSpan {
            start,
            end,
            style: StyleType::Name(Tag::new(open_name, open_parameters).to_string()),
        });
    }

    // Tag strings are carried through as names: the theme of whichever
    // console renders this text resolves them. Resolving here instead would
    // freeze the colours at parse time and make an unknown tag an error
    // rather than the no-op upstream produces. Upstream's `str(Tag)` turns
    // `[link=https://x]` into the style `link https://x`.
    //
    // Zero-length spans are NOT skipped. Upstream keeps them, and they still
    // contribute a boundary point when segments are cut, so `[b]a[i][/i]b[/b]`
    // emits two runs rather than one merged run.
    let mut spans: Vec<Span> = raw_spans
        .into_iter()
        .map(|raw| Span {
            start: raw.start,
            end: raw.end,
            style: raw.style,
        })
        .collect();
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

    let mut text = Text::styled(plain, base_style.clone());
    for span in spans {
        text.push_span(span);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Error positions are character offsets into the original markup, and
    /// emoji codes are replaced only between tags.
    #[test]
    fn error_positions_count_characters_of_the_original() {
        let message = |source: &str| match render_emoji(source) {
            Err(RichError::Markup(message)) => message,
            other => panic!("{other:?}"),
        };
        assert!(message("é中[/i]").contains("position 2 "));
        assert!(message(":smile: [/i]").contains("position 8 "));
        assert_eq!(render_emoji("[b]:smile:[/b]").unwrap().plain(), "😄");
        assert_eq!(render("[b]:smile:[/b]").unwrap().plain(), ":smile:");
    }
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
