//! XML through quick-xml's streaming reader. The document becomes
//! `{root: …}`; see [`super::parse_xml`] for the element mapping. Each
//! element is one stack frame and children are grouped through a name index,
//! so large documents stay linear.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::{DataError, Format, LineIndex, Meta, Node, Position, Value, XmlKind};

struct Element {
    name: String,
    position: Position,
    entries: Vec<(String, Node)>,
    /// Child name → slot in `entries`.
    index: HashMap<String, usize>,
    text: String,
}

impl Element {
    fn add_child(&mut self, name: String, node: Node) {
        match self.index.get(&name) {
            Some(&slot) => {
                let entry = &mut self.entries[slot].1;
                match &mut entry.value {
                    // Already grouped into a sequence.
                    Value::Seq(items) if entry.meta.xml.is_none() => items.push(node),
                    _ => {
                        let first = std::mem::replace(entry, Node::new(Value::Null));
                        let meta = Meta {
                            position: first.meta.position,
                            ..Meta::default()
                        };
                        *entry = Node::with_meta(Value::Seq(vec![first, node]), meta);
                    }
                }
            }
            None => {
                self.index.insert(name.clone(), self.entries.len());
                self.entries.push((name, node));
            }
        }
    }

    fn finish(mut self) -> (String, Node) {
        let text = self.text.trim();
        let meta = Meta {
            position: Some(self.position),
            xml: Some(XmlKind::Element),
            ..Meta::default()
        };
        let value = if self.entries.is_empty() {
            if text.is_empty() {
                Value::Null
            } else {
                Value::String(text.to_string())
            }
        } else {
            if !text.is_empty() {
                let text_meta = Meta {
                    xml: Some(XmlKind::Text),
                    ..Meta::default()
                };
                self.entries.push((
                    "#text".into(),
                    Node::with_meta(Value::String(text.to_string()), text_meta),
                ));
            }
            Value::Map(self.entries)
        };
        (self.name, Node::with_meta(value, meta))
    }
}

fn error(message: impl Into<String>, position: Position) -> DataError {
    DataError::new(Format::Xml, message, Some(position))
}

fn open(start: &BytesStart<'_>, position: Position) -> Result<Element, DataError> {
    let mut element = Element {
        name: start.name().as_ref().to_string(),
        position,
        entries: Vec::new(),
        index: HashMap::new(),
        text: String::new(),
    };
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| error(e.to_string(), position))?;
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| error(e.to_string(), position))?;
        let meta = Meta {
            position: Some(position),
            xml: Some(XmlKind::Attribute),
            ..Meta::default()
        };
        let key = format!("@{}", attribute.key.as_ref());
        element.index.insert(key.clone(), element.entries.len());
        element.entries.push((
            key,
            Node::with_meta(Value::String(value.into_owned()), meta),
        ));
    }
    Ok(element)
}

pub(crate) fn parse(content: &str) -> Result<Node, DataError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let lines = LineIndex::new(content);
    let mut reader = Reader::from_str(content);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<(String, Node)> = None;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|e| {
            error(
                e.to_string(),
                lines.position(reader.error_position() as usize),
            )
        })?;
        // The event's first byte, past any whitespace the reader skipped.
        let at = |start: usize| {
            let skipped = content[start.min(content.len())..]
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(0);
            lines.position(start + skipped)
        };
        let finished = match event {
            Event::Start(tag) => {
                if root.is_some() && stack.is_empty() {
                    return Err(error("more than one root element", at(start)));
                }
                if stack.len() >= super::MAX_DEPTH {
                    return Err(error(
                        format!("nesting deeper than {} levels", super::MAX_DEPTH),
                        at(start),
                    ));
                }
                stack.push(open(&tag, at(start))?);
                None
            }
            Event::Empty(tag) => {
                if root.is_some() && stack.is_empty() {
                    return Err(error("more than one root element", at(start)));
                }
                Some(open(&tag, at(start))?.finish())
            }
            Event::End(_) => stack.pop().map(Element::finish),
            Event::Text(text) => {
                let content = text.xml10_content();
                match stack.last_mut() {
                    Some(element) => element.text.push_str(&content),
                    None if content.trim().is_empty() => {}
                    None => return Err(error("text outside the root element", at(start))),
                }
                None
            }
            Event::CData(data) => {
                match stack.last_mut() {
                    Some(element) => element.text.push_str(&data.xml10_content()),
                    None => return Err(error("text outside the root element", at(start))),
                }
                None
            }
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    Ok(None) => quick_xml::escape::unescape(&format!("&{};", &*reference))
                        .map_err(|e| error(e.to_string(), at(start)))?
                        .into_owned(),
                    Err(e) => return Err(error(e.to_string(), at(start))),
                };
                match stack.last_mut() {
                    Some(element) => element.text.push_str(&resolved),
                    None => return Err(error("text outside the root element", at(start))),
                }
                None
            }
            Event::Eof => {
                if let Some(element) = stack.last() {
                    return Err(error(
                        format!("unclosed element <{}>", element.name),
                        element.position,
                    ));
                }
                break;
            }
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) | Event::DocType(_) => None,
        };
        if let Some((name, node)) = finished {
            match stack.last_mut() {
                Some(parent) => parent.add_child(name, node),
                None => root = Some((name, node)),
            }
        }
    }
    match root {
        Some((name, node)) => Ok(Node::new(Value::Map(vec![(name, node)]))),
        None => Err(DataError::new(
            Format::Xml,
            "no root element",
            Some(lines.position(content.len())),
        )),
    }
}

/// Detection prefilter: `<?xml`, or `<` followed by a name start character.
pub(crate) fn looks_like(trimmed: &str) -> bool {
    trimmed.starts_with("<?xml")
        || trimmed
            .strip_prefix('<')
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_alphabetic() || c == '_')
}
