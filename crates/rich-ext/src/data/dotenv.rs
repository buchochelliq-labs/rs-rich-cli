//! dotenv files, hand-written. See [`super::parse_dotenv`] for the rules.

use super::ini::Entries;
use super::{DataError, Format, Meta, Node, Position, Value};

fn error(message: impl Into<String>, position: Position) -> DataError {
    DataError::new(Format::Dotenv, message, Some(position))
}

/// A cursor over the source that tracks line and column.
struct Cursor<'a> {
    rest: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
    column: usize,
}

impl Cursor<'_> {
    fn peek(&mut self) -> Option<char> {
        self.rest.peek().copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.rest.next()?;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }
    fn position(&self) -> Position {
        Position::new(self.line, self.column)
    }
    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
    }
    /// The rest of the line, without the newline (consumed) or a `\r`.
    fn rest_of_line(&mut self) -> String {
        let mut text = String::new();
        while let Some(c) = self.bump() {
            if c == '\n' {
                break;
            }
            text.push(c);
        }
        if text.ends_with('\r') {
            text.pop();
        }
        text
    }
}

fn is_key_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// What follows a closing quote: blanks, then an optional `# comment`.
fn trailing_comment(cursor: &mut Cursor<'_>) -> Result<Option<String>, DataError> {
    cursor.skip_blanks();
    let position = cursor.position();
    let rest = cursor.rest_of_line();
    let rest = rest.trim_end();
    if rest.is_empty() {
        return Ok(None);
    }
    match rest.strip_prefix('#') {
        Some(text) => Ok(Some(text.trim().to_string())),
        None => Err(error("unexpected text after the closing quote", position)),
    }
}

pub(crate) fn parse(content: &str) -> Result<Node, DataError> {
    parse_with(content, false)
}

/// Parse; `strict` (for detection) rejects spaces around `=` and unquoted
/// values containing whitespace.
fn parse_with(content: &str, strict: bool) -> Result<Node, DataError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut cursor = Cursor {
        rest: content.chars().peekable(),
        line: 1,
        column: 1,
    };
    let mut entries = Entries::default();
    let mut comment: Vec<String> = Vec::new();
    loop {
        cursor.skip_blanks();
        match cursor.peek() {
            None => break,
            Some('\n' | '\r') => {
                cursor.rest_of_line();
                comment.clear();
                continue;
            }
            Some('#') => {
                cursor.bump();
                let text = cursor.rest_of_line();
                comment.push(
                    text.strip_prefix(' ')
                        .unwrap_or(&text)
                        .trim_end()
                        .to_string(),
                );
                continue;
            }
            Some(_) => {}
        }
        let mut position = cursor.position();
        let mut key = String::new();
        while let Some(c) = cursor.peek().filter(|c| is_key_char(*c)) {
            key.push(c);
            cursor.bump();
        }
        if key == "export" && matches!(cursor.peek(), Some(' ' | '\t')) {
            cursor.skip_blanks();
            position = cursor.position();
            key.clear();
            while let Some(c) = cursor.peek().filter(|c| is_key_char(*c)) {
                key.push(c);
                cursor.bump();
            }
        }
        if !key.starts_with(is_key_start) {
            return Err(error(
                "expected a variable name (letters, digits, `_` and `.`, not starting with a digit)",
                position,
            ));
        }
        if !strict {
            cursor.skip_blanks();
        }
        if cursor.peek() != Some('=') {
            return Err(error(
                format!("expected `=` after `{key}`"),
                cursor.position(),
            ));
        }
        cursor.bump();
        if !strict {
            cursor.skip_blanks();
        }
        let mut inline_comment = None;
        let value = match cursor.peek() {
            Some(quote @ ('\'' | '"')) => {
                let open = cursor.position();
                cursor.bump();
                let mut value = String::new();
                loop {
                    match cursor.bump() {
                        None => return Err(error(format!("unterminated {quote} quote"), open)),
                        Some(c) if c == quote => break,
                        Some('\\') if quote == '"' => match cursor.bump() {
                            Some('n') => value.push('\n'),
                            Some('t') => value.push('\t'),
                            Some('r') => value.push('\r'),
                            Some('"') => value.push('"'),
                            Some('\\') => value.push('\\'),
                            Some(other) => {
                                value.push('\\');
                                value.push(other);
                            }
                            None => return Err(error("unterminated \" quote", open)),
                        },
                        Some(c) => value.push(c),
                    }
                }
                inline_comment = trailing_comment(&mut cursor)?;
                value
            }
            _ => {
                let line = cursor.rest_of_line();
                // ` #` starts a comment; a `#` inside a word does not.
                let cut = line
                    .char_indices()
                    .find(|&(i, c)| c == '#' && (i == 0 || line[..i].ends_with([' ', '\t'])))
                    .map(|(i, _)| i);
                if let Some(i) = cut {
                    inline_comment = Some(line[i + 1..].trim().to_string());
                }
                let value = line[..cut.unwrap_or(line.len())].trim();
                if strict && value.contains(char::is_whitespace) {
                    return Err(error("unquoted value with spaces", position));
                }
                value.to_string()
            }
        };
        comment.extend(inline_comment);
        let meta = Meta {
            position: Some(position),
            comment: (!comment.is_empty()).then(|| comment.join("\n")),
            ..Meta::default()
        };
        comment.clear();
        entries.insert(key, Node::with_meta(Value::String(value), meta));
    }
    Ok(Node::new(Value::Map(entries.entries)))
}

/// Detection: parses strictly and defines at least one variable.
pub(crate) fn looks_like(content: &str) -> bool {
    parse_with(content, true).is_ok_and(|node| !node.is_empty())
}
