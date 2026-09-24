//! INI files, hand-written. See [`super::parse_ini`] for the rules.

use std::collections::HashMap;

use super::{DataError, Format, Meta, Node, Position, Value};

/// Entries of one map, with a key → slot index so repeated keys stay linear.
#[derive(Default)]
pub(crate) struct Entries {
    pub(crate) entries: Vec<(String, Node)>,
    index: HashMap<String, usize>,
}

impl Entries {
    /// Insert; a repeated key keeps its slot but takes the new node.
    pub(crate) fn insert(&mut self, key: String, node: Node) {
        match self.index.get(&key) {
            Some(&slot) => self.entries[slot].1 = node,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, node));
            }
        }
    }

    pub(crate) fn contains(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        let slot = *self.index.get(key)?;
        Some(&mut self.entries[slot].1)
    }
}

fn error(message: impl Into<String>, line: usize, column: usize) -> DataError {
    DataError::new(Format::Ini, message, Some(Position::new(line, column)))
}

/// Leading whitespace in characters, for 1-based columns.
fn indent(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

struct Section {
    name: String,
    meta: Meta,
    entries: Entries,
}

pub(crate) fn parse(content: &str) -> Result<Node, DataError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut root = Entries::default();
    let mut sections: Vec<Section> = Vec::new();
    let mut section_index: HashMap<String, usize> = HashMap::new();
    let mut current: Option<usize> = None;
    let mut comment: Vec<String> = Vec::new();
    // The key a continuation line extends: (section, key).
    let mut last_key: Option<(Option<usize>, String)> = None;

    for (number, raw) in content.split('\n').enumerate() {
        let number = number + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            comment.clear();
            last_key = None;
            continue;
        }
        if let Some(text) = trimmed.strip_prefix([';', '#']) {
            comment.push(
                text.strip_prefix(' ')
                    .unwrap_or(text)
                    .trim_end()
                    .to_string(),
            );
            continue;
        }
        let column = indent(line) + 1;
        if column > 1 {
            if let Some((section, key)) = &last_key {
                let entries = match section {
                    Some(i) => &mut sections[*i].entries,
                    None => &mut root,
                };
                if let Some(Node {
                    value: Value::String(value),
                    ..
                }) = entries.get_mut(key)
                {
                    value.push('\n');
                    value.push_str(trimmed);
                    continue;
                }
            }
        }
        if trimmed.starts_with('[') {
            let Some(name) = trimmed.strip_prefix('[').and_then(|t| t.strip_suffix(']')) else {
                return Err(error(
                    "expected `]` to close the section header",
                    number,
                    column,
                ));
            };
            let name = name.trim();
            if name.is_empty() {
                return Err(error("empty section name", number, column));
            }
            // Sections share the root map with the keys before the first
            // section; one would silently replace the other.
            if root.contains(name) {
                return Err(error(
                    format!("section `[{name}]` has the same name as the key `{name}` before the first section"),
                    number,
                    column,
                ));
            }
            let meta = Meta {
                position: Some(Position::new(number, column)),
                comment: (!comment.is_empty()).then(|| comment.join("\n")),
                ..Meta::default()
            };
            comment.clear();
            last_key = None;
            // A repeated section reopens the first one.
            current = Some(*section_index.entry(name.to_string()).or_insert_with(|| {
                sections.push(Section {
                    name: name.to_string(),
                    meta,
                    entries: Entries::default(),
                });
                sections.len() - 1
            }));
            continue;
        }
        let Some(split) = trimmed.find(['=', ':']) else {
            return Err(error(
                "expected `key = value`, `key: value` or a `[section]`",
                number,
                column,
            ));
        };
        let key = trimmed[..split].trim_end();
        if key.is_empty() {
            return Err(error("empty key", number, column));
        }
        let value = trimmed[split + 1..].trim_start();
        let meta = Meta {
            position: Some(Position::new(number, column)),
            comment: (!comment.is_empty()).then(|| comment.join("\n")),
            ..Meta::default()
        };
        comment.clear();
        let entries = match current {
            Some(i) => &mut sections[i].entries,
            None => &mut root,
        };
        entries.insert(
            key.to_string(),
            Node::with_meta(Value::String(value.to_string()), meta),
        );
        last_key = Some((current, key.to_string()));
    }

    for section in sections {
        root.insert(
            section.name,
            Node::with_meta(Value::Map(section.entries.entries), section.meta),
        );
    }
    Ok(Node::new(Value::Map(root.entries)))
}

/// Detection: a `[section]` header, at least one key line, and nothing that
/// is neither (see [`super::Format::detect`]).
pub(crate) fn looks_like(content: &str) -> bool {
    let mut header = false;
    let mut keys = false;
    for raw in content.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with([';', '#']) {
            continue;
        }
        if trimmed.starts_with('[') {
            let Some(name) = trimmed.strip_prefix('[').and_then(|t| t.strip_suffix(']')) else {
                return false;
            };
            if name.trim().is_empty() || name.contains(['[', ']']) {
                return false;
            }
            header = true;
            continue;
        }
        if raw.starts_with([' ', '\t']) && keys {
            continue;
        }
        match trimmed.find(['=', ':']) {
            Some(split) if !trimmed[..split].trim().is_empty() => keys = true,
            _ => return false,
        }
    }
    header && keys && parse(content).is_ok()
}
