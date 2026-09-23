//! JSON through serde_json (with `preserve_order`, so keys keep document
//! order). serde_json reports positions, not spans, so nodes carry none.

use super::{DataError, Format, Node, Position};

pub(crate) fn parse(content: &str) -> Result<Node, DataError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(value) => Ok(Node::from(&value)),
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
