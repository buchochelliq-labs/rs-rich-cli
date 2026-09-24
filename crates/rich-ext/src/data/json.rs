//! JSON through serde_json (with `preserve_order`, so keys keep document
//! order). serde_json reports positions, not spans, so nodes carry none.

use super::{DataError, Format, Node, Position, Value};

pub(crate) fn parse(content: &str) -> Result<Node, DataError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(value) => {
            let mut node = Node::from(&value);
            integer_negative_zero(&mut node, content);
            Ok(node)
        }
        Err(error) => {
            let position = (error.line() > 0).then(|| {
                // serde_json counts the column in bytes; ours are characters.
                let line = content.split('\n').nth(error.line() - 1).unwrap_or("");
                let mut byte = error.column().saturating_sub(1).min(line.len());
                while !line.is_char_boundary(byte) {
                    byte -= 1;
                }
                Position::new(error.line(), line[..byte].chars().count() + 1)
            });
            // serde_json appends " at line L column C"; the position is kept
            // separately, so drop it from the message.
            let message = error.to_string();
            let message = message
                .rsplit_once(" at line ")
                .map_or(message.as_str(), |(head, _)| head)
                .to_string();
            Err(DataError::new(Format::Json, message, position))
        }
    }
}

/// serde_json reads the integer `-0` as the float `-0.0`; Python's `json`
/// (and so rich) reads it as the integer `0`. Each negative zero in `node`
/// is matched, in document order, with the number token that produced it,
/// and the ones written as integers become `Int(0)`. When the counts differ
/// (a duplicate key dropped one) only an all-integer document is changed.
fn integer_negative_zero(node: &mut Node, content: &str) {
    let mut zeros: Vec<&mut Node> = Vec::new();
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if matches!(node.value, Value::Float(f) if f == 0.0 && f.is_sign_negative()) {
            zeros.push(node);
            continue;
        }
        match &mut node.value {
            Value::Seq(items) => stack.extend(items.iter_mut().rev()),
            Value::Map(entries) => stack.extend(entries.iter_mut().rev().map(|(_, v)| v)),
            _ => {}
        }
    }
    if zeros.is_empty() {
        return;
    }
    // Whether each negative-zero token is written as an integer.
    let mut integers = Vec::new();
    let mut chars = content.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        match c {
            '"' => {
                while let Some((_, c)) = chars.next() {
                    match c {
                        '\\' => {
                            chars.next();
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '-' | '0'..='9' => {
                let mut end = start + 1;
                while let Some(&(i, c)) = chars.peek() {
                    if !matches!(c, '0'..='9' | '-' | '+' | '.' | 'e' | 'E') {
                        break;
                    }
                    end = i + 1;
                    chars.next();
                }
                let token = &content[start..end];
                if token
                    .parse::<f64>()
                    .is_ok_and(|f| f == 0.0 && f.is_sign_negative())
                {
                    integers.push(!token.contains(['.', 'e', 'E']));
                }
            }
            _ => {}
        }
    }
    let matched = integers.len() == zeros.len();
    let all_integers = integers.iter().all(|i| *i);
    for (i, zero) in zeros.into_iter().enumerate() {
        let integer = if matched { integers[i] } else { all_integers };
        if integer {
            zero.value = Value::Int(0);
        }
    }
}
