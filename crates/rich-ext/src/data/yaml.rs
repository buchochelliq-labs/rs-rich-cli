//! YAML through saphyr-parser's event stream, so anchors, aliases and
//! positions are visible (a serde route would resolve them away). Plain
//! scalars resolve per the YAML 1.2 core schema; quoted and block scalars are
//! always strings.

use std::collections::{HashMap, HashSet};

use saphyr_parser::{Event, Parser, ScalarStyle, ScanError, Span};

use super::ini::Entries;
use super::{DataError, Format, Meta, Node, Position, Value};

/// How many nodes aliases may copy in total before parsing stops. Guards
/// against "billion laughs" documents whose aliases expand exponentially.
const ALIAS_BUDGET: usize = 1_000_000;

/// How many bytes of strings (values and keys) aliases may copy in total:
/// a few nodes can hold a lot of text, so nodes alone are not a measure.
const ALIAS_BYTE_BUDGET: usize = 64 << 20;

/// The nodes in `node` and the bytes of its strings and keys. Iterative:
/// documents may nest deeper than the call stack allows.
fn cost(node: &Node) -> (usize, usize) {
    let (mut nodes, mut bytes) = (0, 0);
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        nodes += 1;
        match &node.value {
            Value::String(s) | Value::DateTime(s) => bytes += s.len(),
            Value::Seq(items) => stack.extend(items),
            Value::Map(entries) => {
                for (key, value) in entries {
                    bytes += key.len();
                    stack.push(value);
                }
            }
            _ => {}
        }
    }
    (nodes, bytes)
}

fn scan_error(error: &ScanError) -> DataError {
    let mark = error.marker();
    DataError::new(
        Format::Yaml,
        error.info().to_string(),
        Some(Position::new(mark.line(), mark.col() + 1)),
    )
}

fn position(span: &Span) -> Position {
    Position::new(span.start.line(), span.start.col() + 1)
}

/// Resolve a plain scalar per the YAML 1.2 core schema.
pub(crate) fn resolve_plain(text: &str) -> Value {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => {
            return Value::Float(f64::INFINITY)
        }
        "-.inf" | "-.Inf" | "-.INF" => return Value::Float(f64::NEG_INFINITY),
        ".nan" | ".NaN" | ".NAN" => return Value::Float(f64::NAN),
        _ => {}
    }
    let radix = |digits: &str, radix: u32, valid: fn(&char) -> bool| {
        (!digits.is_empty() && digits.chars().all(|c| valid(&c)))
            .then(|| {
                i64::from_str_radix(digits, radix)
                    .map(Value::Int)
                    .or_else(|_| u64::from_str_radix(digits, radix).map(Value::UInt))
                    .ok()
            })
            .flatten()
    };
    if let Some(hex) = text.strip_prefix("0x") {
        if let Some(value) = radix(hex, 16, char::is_ascii_hexdigit) {
            return value;
        }
    }
    if let Some(octal) = text.strip_prefix("0o") {
        if let Some(value) = radix(octal, 8, |c| ('0'..='7').contains(c)) {
            return value;
        }
    }
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if !unsigned.is_empty() && unsigned.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(i) = text.parse::<i64>() {
            return Value::Int(i);
        }
        if let Ok(u) = unsigned.parse::<u64>() {
            if !text.starts_with('-') {
                return Value::UInt(u);
            }
        }
        if let Ok(f) = text.parse::<f64>() {
            return Value::Float(f);
        }
    }
    if is_core_float(unsigned) {
        if let Ok(f) = text.parse::<f64>() {
            return Value::Float(f);
        }
    }
    Value::String(text.to_string())
}

/// `(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?`
fn is_core_float(text: &str) -> bool {
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(i) => (&text[..i], Some(&text[i + 1..])),
        None => (text, None),
    };
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some(("", frac)) => digits(frac),
        Some((int, frac)) => digits(int) && (frac.is_empty() || digits(frac)),
        None => digits(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|e| digits(e.strip_prefix(['-', '+']).unwrap_or(e)));
    mantissa_ok && exponent_ok
}

/// A mapping key as a string.
fn key_string(node: &Node, raw: Option<&str>) -> String {
    if let Some(raw) = raw {
        return raw.to_string();
    }
    match &node.value {
        Value::String(s) | Value::DateTime(s) => s.clone(),
        Value::Seq(_) | Value::Map(_) => node.to_json().to_string(),
        other => super::scalar_text(other, false).0,
    }
}

enum Frame {
    Seq {
        meta: Meta,
        anchor: usize,
        items: Vec<Node>,
    },
    Map {
        meta: Meta,
        anchor: usize,
        entries: Entries,
        key: Option<(String, Option<Position>)>,
    },
}

struct Builder<'a> {
    source: &'a str,
    /// Byte offset of each char, built on the first anchor.
    char_bytes: Option<Vec<usize>>,
    /// Where the previous event ended, in chars.
    previous_end: usize,
    stack: Vec<Frame>,
    documents: Vec<Node>,
    /// Anchor id → (name, value, cost). Filled when the anchored node
    /// completes, and only for anchors that some alias refers to.
    anchors: HashMap<usize, (String, Node, (usize, usize))>,
    /// Anchor ids that aliases refer to (from a first pass).
    aliased: HashSet<usize>,
    /// Names of anchors whose nodes are still open.
    open_anchors: HashMap<usize, String>,
    /// Nodes and bytes copied so far, by aliases and into the anchor table.
    copied: (usize, usize),
}

impl Builder<'_> {
    /// The anchor name written between the previous event and `span`.
    fn anchor_name(&mut self, id: usize, span: &Span) -> String {
        let source = self.source;
        let bytes = self.char_bytes.get_or_insert_with(|| {
            source
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(source.len()))
                .collect()
        });
        let byte = |char_index: usize| bytes[char_index.min(bytes.len() - 1)];
        let gap = &source[byte(self.previous_end)..byte(span.start.index())];
        let mut in_comment = false;
        let mut previous = ' ';
        for (i, c) in gap.char_indices() {
            match c {
                '\n' => in_comment = false,
                '#' if previous.is_whitespace() => in_comment = true,
                '&' if !in_comment => {
                    let name: String = gap[i + 1..]
                        .chars()
                        .take_while(|c| !c.is_whitespace() && !",[]{}".contains(*c))
                        .collect();
                    if !name.is_empty() {
                        return name;
                    }
                }
                _ => {}
            }
            previous = c;
        }
        format!("anchor{id}")
    }

    fn meta(&mut self, anchor: usize, span: &Span) -> Meta {
        let name = (anchor > 0).then(|| self.anchor_name(anchor, span));
        if let Some(name) = &name {
            self.open_anchors.insert(anchor, name.clone());
        }
        Meta {
            position: Some(position(span)),
            anchor: name,
            ..Meta::default()
        }
    }

    /// Charge a copy of `cost` (nodes, bytes) against the alias budgets.
    fn charge(&mut self, cost: (usize, usize), span: &Span) -> Result<(), DataError> {
        self.copied.0 = self.copied.0.saturating_add(cost.0);
        self.copied.1 = self.copied.1.saturating_add(cost.1);
        let over = if self.copied.0 > ALIAS_BUDGET {
            format!("{ALIAS_BUDGET} nodes")
        } else if self.copied.1 > ALIAS_BYTE_BUDGET {
            format!("{} MiB", ALIAS_BYTE_BUDGET >> 20)
        } else {
            return Ok(());
        };
        Err(DataError::new(
            Format::Yaml,
            format!("aliases expand to more than {over}"),
            Some(position(span)),
        ))
    }

    /// A node is complete: record its anchor and hand it to its parent.
    fn complete(
        &mut self,
        anchor: usize,
        node: Node,
        raw_key: Option<&str>,
        span: &Span,
    ) -> Result<(), DataError> {
        if anchor > 0 {
            let name = self.open_anchors.remove(&anchor).unwrap_or_default();
            // Only anchors that an alias uses are copied, and the copy
            // counts against the budget: nested anchors would otherwise
            // cost depth × subtree.
            if self.aliased.contains(&anchor) {
                let cost = cost(&node);
                self.charge(cost, span)?;
                self.anchors.insert(anchor, (name, node.clone(), cost));
            }
        }
        match self.stack.last_mut() {
            None => self.documents.push(node),
            Some(Frame::Seq { items, .. }) => items.push(node),
            Some(Frame::Map { entries, key, .. }) => match key.take() {
                None => *key = Some((key_string(&node, raw_key), node.meta.position)),
                Some((name, position)) => {
                    if entries.contains(&name) {
                        return Err(DataError::new(
                            Format::Yaml,
                            format!("duplicate key `{name}`"),
                            position.or(node.meta.position),
                        ));
                    }
                    let mut node = node;
                    node.meta.position = position.or(node.meta.position);
                    entries.insert(name, node);
                }
            },
        }
        Ok(())
    }

    fn event(&mut self, event: Event<'_>, span: Span) -> Result<(), DataError> {
        match event {
            Event::Scalar(text, style, anchor, tag) => {
                let meta = self.meta(anchor, &span);
                let forced_str = tag
                    .as_ref()
                    .is_some_and(|t| t.is_yaml_core_schema() && t.suffix == "str");
                let value = if style == ScalarStyle::Plain && !forced_str {
                    resolve_plain(&text)
                } else {
                    Value::String(text.to_string())
                };
                self.complete(anchor, Node::with_meta(value, meta), Some(&text), &span)?;
            }
            Event::SequenceStart(..) | Event::MappingStart(..)
                if self.stack.len() >= super::MAX_DEPTH =>
            {
                return Err(DataError::new(
                    Format::Yaml,
                    format!("nesting deeper than {} levels", super::MAX_DEPTH),
                    Some(position(&span)),
                ));
            }
            Event::SequenceStart(anchor, _) => {
                let meta = self.meta(anchor, &span);
                self.stack.push(Frame::Seq {
                    meta,
                    anchor,
                    items: Vec::new(),
                });
            }
            Event::MappingStart(anchor, _) => {
                let meta = self.meta(anchor, &span);
                self.stack.push(Frame::Map {
                    meta,
                    anchor,
                    entries: Entries::default(),
                    key: None,
                });
            }
            Event::SequenceEnd | Event::MappingEnd => match self.stack.pop() {
                Some(Frame::Seq {
                    meta,
                    anchor,
                    items,
                }) => self.complete(
                    anchor,
                    Node::with_meta(Value::Seq(items), meta),
                    None,
                    &span,
                )?,
                Some(Frame::Map {
                    meta,
                    anchor,
                    entries,
                    ..
                }) => self.complete(
                    anchor,
                    Node::with_meta(Value::Map(entries.entries), meta),
                    None,
                    &span,
                )?,
                None => {}
            },
            Event::Alias(id) => {
                let Some(cost) = self.anchors.get(&id).map(|entry| entry.2) else {
                    let name = self.open_anchors.get(&id).cloned().unwrap_or_default();
                    return Err(DataError::new(
                        Format::Yaml,
                        format!("alias `*{name}` refers to its own anchor (recursive data)"),
                        Some(position(&span)),
                    ));
                };
                self.charge(cost, &span)?;
                let (name, target, _) = &self.anchors[&id];
                let mut node = target.clone();
                node.meta.anchor = None;
                node.meta.alias = Some(name.clone());
                node.meta.position = Some(position(&span));
                self.complete(0, node, None, &span)?;
            }
            Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart(_)
            | Event::DocumentEnd
            | Event::Nothing => {}
        }
        self.previous_end = span.end.index();
        Ok(())
    }
}

/// The anchor ids that aliases in `content` refer to: a cheap first pass, so
/// that only those anchors are copied. A scan error ends the pass (the
/// second pass reports it).
fn aliased(content: &str) -> HashSet<usize> {
    Parser::new_from_str(content)
        .map_while(Result::ok)
        .filter_map(|(event, _)| match event {
            Event::Alias(id) => Some(id),
            _ => None,
        })
        .collect()
}

/// Each document of `content`.
fn documents(content: &str) -> Result<Vec<Node>, DataError> {
    let mut builder = Builder {
        source: content,
        char_bytes: None,
        previous_end: 0,
        stack: Vec::new(),
        documents: Vec::new(),
        anchors: HashMap::new(),
        aliased: aliased(content),
        open_anchors: HashMap::new(),
        copied: (0, 0),
    };
    for event in Parser::new_from_str(content) {
        let (event, span) = event.map_err(|e| scan_error(&e))?;
        builder.event(event, span)?;
    }
    Ok(builder.documents)
}

pub(crate) fn parse(content: &str) -> Result<Node, DataError> {
    let mut documents = documents(content)?;
    Ok(match documents.len() {
        0 => Node::new(Value::Null),
        1 => documents.pop().expect("one document"),
        _ => Node::new(Value::Seq(documents)),
    })
}

/// `key:` at the start of `line`, the key an identifier-like word or quoted.
fn is_key_line(line: &str) -> bool {
    let key_end = if let Some(rest) = line.strip_prefix('"') {
        rest.find('"').map(|i| i + 2)
    } else if let Some(rest) = line.strip_prefix('\'') {
        rest.find('\'').map(|i| i + 2)
    } else {
        let end = line
            .find(|c: char| !(c.is_alphanumeric() || "_-./".contains(c)))
            .unwrap_or(line.len());
        (end > 0).then_some(end)
    };
    let Some(end) = key_end else {
        return false;
    };
    let rest = line[end..].trim_start_matches([' ', '\t']);
    rest.strip_prefix(':')
        .is_some_and(|after| after.is_empty() || after.starts_with([' ', '\t']))
}

/// Detection (see [`super::Format::detect`]): structural top-level lines only,
/// at least two structural lines with one `key:`, and container documents.
pub(crate) fn looks_like(content: &str) -> bool {
    let mut structural = 0;
    let mut keys = 0;
    for raw in content.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let nested = raw.starts_with([' ', '\t']);
        let item = trimmed == "-" || trimmed.starts_with("- ");
        let body = if item {
            trimmed[1..].trim_start()
        } else {
            trimmed
        };
        let key = is_key_line(body);
        if key || item {
            structural += 1;
        }
        if key {
            keys += 1;
        }
        let marker =
            trimmed.starts_with("---") || trimmed.starts_with("...") || trimmed.starts_with('%');
        if !nested && !key && !item && !marker {
            return false;
        }
    }
    structural >= 2
        && keys >= 1
        && documents(content)
            .is_ok_and(|docs| !docs.is_empty() && docs.iter().all(Node::is_container))
}
