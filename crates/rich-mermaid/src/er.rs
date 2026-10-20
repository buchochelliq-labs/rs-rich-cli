//! Parse Mermaid entity-relationship diagrams (`erDiagram`) and draw them as
//! text, through `rs-rich-diagram`'s layered layout.
//!
//! What is drawn:
//!
//! - each entity as a table box: its name (or its `["alias"]`) as a centred
//!   header, ruled off from one row per attribute, in Mermaid's order:
//!   type, name, keys (`PK`, `FK`, `UK`) and comment, in aligned columns;
//! - each relationship as a line between the two entities, solid for an
//!   identifying one (`--`, `to`) and dotted for a non-identifying one
//!   (`..`, `optionally to`), its label beside the line;
//! - the cardinality at each end as text next to the line where it meets
//!   that entity: `1` (`||`, exactly one), `0..1` (`|o` / `o|`, zero or
//!   one), `0..N` (`}o` / `o{`, zero or more) and `1..N` (`}|` / `|{`, one
//!   or more). Text rather than crow's-foot glyphs: it reads the same in
//!   ASCII, and needs no legend;
//! - `direction TB|BT|LR|RL` (top to bottom by default, as in Mermaid), and
//!   a front-matter `title:` above the drawing.
//!
//! The word forms (`only one`, `zero or one`, `one or more`, `zero or many`,
//! `many(0)`, `1+`, … with `to` / `optionally to`) read the same as the
//! symbols. A relationship from an entity to itself is marked `↻` beside its
//! box, with its label and cardinalities in a note. `style`, `classDef` and
//! `class` statements are accepted and ignored, as are `accTitle` and
//! `accDescr`. Anything else is a [`ParseError`] naming it, and the diagram
//! falls back to its source.
//!
//! ```
//! use rich_mermaid::er;
//!
//! let diagram = er::parse("erDiagram\n  CUSTOMER ||--o{ ORDER : places").unwrap();
//! let drawing = er::draw(&diagram, false).unwrap();
//! assert!(drawing.lines.iter().any(|line| line.contains("0..N")), "{:#?}", drawing.lines);
//! ```

use std::collections::HashMap;

use rich::cells::cell_len;
use rich_diagram::{Direction, Drawing, Edge, Graph, Head, Node, Shape, Stroke};

use crate::flowchart::{clean_label, printable, ParseError};
use crate::sequence::statements;

/// The most entities a diagram may have ([`rich_diagram::MAX_NODES`]).
pub const MAX_ENTITIES: usize = rich_diagram::MAX_NODES;
/// The most relationships a diagram may have ([`rich_diagram::MAX_EDGES`]).
pub const MAX_RELATIONSHIPS: usize = rich_diagram::MAX_EDGES;

/// A key an attribute is part of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// `PK`.
    Primary,
    /// `FK`.
    Foreign,
    /// `UK`.
    Unique,
}

impl Key {
    /// `PK`, `FK` or `UK`.
    pub fn code(self) -> &'static str {
        match self {
            Key::Primary => "PK",
            Key::Foreign => "FK",
            Key::Unique => "UK",
        }
    }
}

/// An entity's attribute: `type name [keys] ["comment"]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attribute {
    pub data_type: String,
    pub name: String,
    pub keys: Vec<Key>,
    pub comment: Option<String>,
}

/// An entity and its attributes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entity {
    /// What relationships refer to it by.
    pub id: String,
    /// Its header: the alias, or the id.
    pub label: String,
    pub attributes: Vec<Attribute>,
}

/// How many of an entity relate, at one end of a relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cardinality {
    /// `||`: exactly one.
    ExactlyOne,
    /// `|o` / `o|`: zero or one.
    ZeroOrOne,
    /// `}o` / `o{`: zero or more.
    ZeroOrMore,
    /// `}|` / `|{`: one or more.
    OneOrMore,
}

impl Cardinality {
    /// `1`, `0..1`, `0..N` or `1..N`: what the drawing shows at that end.
    pub fn notation(self) -> &'static str {
        match self {
            Cardinality::ExactlyOne => "1",
            Cardinality::ZeroOrOne => "0..1",
            Cardinality::ZeroOrMore => "0..N",
            Cardinality::OneOrMore => "1..N",
        }
    }
}

/// A relationship between two entities, by index into
/// [`EntityDiagram::entities`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    /// The first entity named.
    pub from: usize,
    /// The second.
    pub to: usize,
    /// How many of `from` relate (the symbol beside it, `||` in `A ||--o{ B`).
    pub from_cardinality: Cardinality,
    /// How many of `to` relate (`o{` in `A ||--o{ B`).
    pub to_cardinality: Cardinality,
    /// `--` (`true`, a solid line) or `..` (`false`, dotted).
    pub identifying: bool,
    /// Empty for none.
    pub label: String,
}

/// A parsed entity-relationship diagram.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityDiagram {
    pub title: Option<String>,
    pub direction: Direction,
    /// In order of first appearance.
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    /// Things the text renderer simplified, to show under the drawing.
    pub notes: Vec<String>,
}

impl EntityDiagram {
    /// The diagram as a [`Graph`]: [`Shape::Table`] nodes, and an edge per
    /// relationship with its label and its cardinalities as end texts
    /// ([`Edge::start_label`], [`Edge::end_label`]). With the notes on what
    /// the graph cannot show: a relationship from an entity to itself is
    /// drawn as a `↻` beside its box, so its texts are a note.
    pub fn to_graph(&self) -> (Graph, Vec<String>) {
        let nodes = self
            .entities
            .iter()
            .map(|entity| Node::new(entity.id.clone(), table(entity)).shape(Shape::Table))
            .collect();
        let mut notes = Vec::new();
        let edges = self
            .relationships
            .iter()
            .map(|relationship| {
                let mut edge = Edge::new(relationship.from, relationship.to);
                edge.start = Head::None;
                edge.end = Head::None;
                edge.stroke = if relationship.identifying {
                    Stroke::Solid
                } else {
                    Stroke::Dotted
                };
                edge.label =
                    Some(relationship.label.replace('\n', " ")).filter(|label| !label.is_empty());
                edge.start_label = Some(relationship.from_cardinality.notation().into());
                edge.end_label = Some(relationship.to_cardinality.notation().into());
                if relationship.from == relationship.to {
                    let entity = &self.entities[relationship.from];
                    let label = match &edge.label {
                        Some(label) => format!("`{label}`, "),
                        None => String::new(),
                    };
                    notes.push(format!(
                        "{} relates to itself: {label}{} to {}",
                        entity.label.replace('\n', " "),
                        relationship.from_cardinality.notation(),
                        relationship.to_cardinality.notation()
                    ));
                }
                edge
            })
            .collect();
        (Graph::from_parts(self.direction, nodes, edges), notes)
    }
}

/// An entity's box text: its header, then one aligned row per attribute.
fn table(entity: &Entity) -> String {
    let flat = |text: &str| text.replace(['\r', '\n'], " ");
    let rows: Vec<[String; 4]> = entity
        .attributes
        .iter()
        .map(|a| {
            let keys: Vec<&str> = a.keys.iter().map(|k| k.code()).collect();
            [
                flat(&a.data_type),
                flat(&a.name),
                keys.join(", "),
                a.comment.as_deref().map(flat).unwrap_or_default(),
            ]
        })
        .collect();
    let width = |i: usize| rows.iter().map(|row| cell_len(&row[i])).max().unwrap_or(0);
    let widths = [width(0), width(1), width(2), width(3)];
    let mut lines = vec![flat(&entity.label)];
    for row in &rows {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            if widths[i] == 0 {
                continue;
            }
            if !line.is_empty() {
                line.push_str("  ");
            }
            line.push_str(cell);
            line.push_str(&" ".repeat(widths[i] - cell_len(cell)));
        }
        lines.push(line.trim_end().to_string());
    }
    // Rows pad to one width, so the box's rows line up under the header.
    let widest = lines[1..].iter().map(|l| cell_len(l)).max().unwrap_or(0);
    for line in &mut lines[1..] {
        let pad = widest - cell_len(line);
        line.push_str(&" ".repeat(pad));
    }
    lines.join("\n")
}

/// Lay out and draw `diagram`, its notes (and the layout's) in
/// [`Drawing::notes`]. `ascii` restricts the output to ASCII.
pub fn draw(diagram: &EntityDiagram, ascii: bool) -> Result<Drawing, String> {
    let (graph, notes) = diagram.to_graph();
    let mut drawing = rich_diagram::draw(&graph, ascii).map_err(|error| error.to_string())?;
    let mut all = notes;
    all.append(&mut drawing.notes);
    drawing.notes = all;
    if let Some(title) = &diagram.title {
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>();
        let width = cell_len(&title);
        let pad = drawing.width.saturating_sub(width) / 2;
        let mut lines = vec![format!("{}{title}", " ".repeat(pad)), String::new()];
        lines.append(&mut drawing.lines);
        drawing.lines = lines;
        drawing.width = drawing.width.max(width);
    }
    Ok(drawing)
}

/// Whether `source` is an ER diagram: its first statement, after front
/// matter and comments, is `erDiagram`.
pub fn is_er(source: &str) -> bool {
    crate::sequence::header_keyword(source) == Some("erDiagram")
}

/// Parse a Mermaid ER diagram.
///
/// ```
/// use rich_mermaid::er::{parse, Cardinality, Key};
///
/// let diagram = parse(
///     "erDiagram
///       CUSTOMER |o..|{ ADDRESS : \"lives at\"
///       CUSTOMER {
///         string id PK \"the customer number\"
///       }",
/// )
/// .unwrap();
/// let relationship = &diagram.relationships[0];
/// assert_eq!(relationship.to_cardinality, Cardinality::OneOrMore);
/// assert!(!relationship.identifying);
/// assert_eq!(diagram.entities[0].attributes[0].keys, [Key::Primary]);
/// ```
pub fn parse(source: &str) -> Result<EntityDiagram, ParseError> {
    let (title, header, statements) = statements(source)?;
    if header != "erDiagram" {
        return Err(ParseError::Syntax {
            line: 1,
            message: format!("expected `erDiagram`, found `{}`", printable(&header, 40)),
        });
    }
    let mut parser = Parser {
        diagram: EntityDiagram {
            title,
            direction: Direction::TopDown,
            ..EntityDiagram::default()
        },
        ids: HashMap::new(),
    };
    // An entity's `{ … }` block may span statements.
    let mut block: Option<(usize, usize, String)> = None;
    let mut skipping_description = false;
    for (number, statement) in &statements {
        if skipping_description {
            skipping_description = !statement.contains('}');
            continue;
        }
        if let Some((entity, line, body)) = &mut block {
            match statement.find('}') {
                Some(close) => {
                    body.push('\n');
                    body.push_str(&statement[..close]);
                    let rest = statement[close + 1..].trim();
                    let (entity, line, body) = (*entity, *line, std::mem::take(body));
                    block = None;
                    parser.attributes(entity, &body, line)?;
                    if !rest.is_empty() {
                        return Err(parser.error(
                            *number,
                            format!("unexpected `{}` after `}}`", printable(rest, 20)),
                        ));
                    }
                }
                None => {
                    body.push('\n');
                    body.push_str(statement);
                }
            }
            continue;
        }
        let lower = statement.to_ascii_lowercase();
        if lower.starts_with("accdescr") && statement.contains('{') {
            skipping_description = !statement.contains('}');
            continue;
        }
        if let Some(open) = block_start(statement) {
            let entity = parser.entity_declaration(statement[..open].trim(), *number)?;
            let after = &statement[open + 1..];
            match after.find('}') {
                Some(close) => parser.attributes(entity, &after[..close], *number)?,
                None => block = Some((entity, *number, after.to_string())),
            }
            continue;
        }
        parser.statement(statement, *number)?;
    }
    if let Some((entity, line, _)) = block {
        let name = printable(&parser.diagram.entities[entity].id, 40);
        return Err(parser.error(line, format!("the `{{` after `{name}` is never closed")));
    }
    Ok(parser.diagram)
}

/// Where an entity's `{` opens: after nothing but an entity name (a `{` in
/// a relationship, `o{`, follows more).
fn block_start(statement: &str) -> Option<usize> {
    let (name, rest) = split_name(statement.trim_start());
    let rest = rest.trim_start();
    if name.is_empty() || !rest.starts_with('{') {
        return None;
    }
    Some(statement.len() - rest.len())
}

struct Parser {
    diagram: EntityDiagram,
    ids: HashMap<String, usize>,
}

/// The left cardinality symbols, then the right ones.
const LEFT: &[(&str, Cardinality)] = &[
    ("|o", Cardinality::ZeroOrOne),
    ("||", Cardinality::ExactlyOne),
    ("}o", Cardinality::ZeroOrMore),
    ("}|", Cardinality::OneOrMore),
];
const RIGHT: &[(&str, Cardinality)] = &[
    ("o|", Cardinality::ZeroOrOne),
    ("||", Cardinality::ExactlyOne),
    ("o{", Cardinality::ZeroOrMore),
    ("|{", Cardinality::OneOrMore),
];
/// The word forms, longest first.
const WORDS: &[(&str, Cardinality)] = &[
    ("one or zero", Cardinality::ZeroOrOne),
    ("zero or one", Cardinality::ZeroOrOne),
    ("one or more", Cardinality::OneOrMore),
    ("one or many", Cardinality::OneOrMore),
    ("zero or more", Cardinality::ZeroOrMore),
    ("zero or many", Cardinality::ZeroOrMore),
    ("only one", Cardinality::ExactlyOne),
    ("many(1)", Cardinality::OneOrMore),
    ("many(0)", Cardinality::ZeroOrMore),
    ("1+", Cardinality::OneOrMore),
    ("0+", Cardinality::ZeroOrMore),
    ("1", Cardinality::ExactlyOne),
];

impl Parser {
    fn error(&self, line: usize, message: String) -> ParseError {
        ParseError::Syntax { line, message }
    }

    /// The entity `id`, added if new; `label` renames it.
    fn entity(
        &mut self,
        id: &str,
        label: Option<String>,
        line: usize,
    ) -> Result<usize, ParseError> {
        if let Some(&index) = self.ids.get(id) {
            if let Some(label) = label {
                self.diagram.entities[index].label = label;
            }
            return Ok(index);
        }
        if self.diagram.entities.len() >= MAX_ENTITIES {
            return Err(ParseError::TooLarge(format!(
                "more than {MAX_ENTITIES} entities"
            )));
        }
        let valid = id.starts_with('"')
            || id
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
        if id.is_empty() || !valid {
            return Err(self.error(
                line,
                format!("`{}` is not an entity name", printable(id, 30)),
            ));
        }
        let index = self.diagram.entities.len();
        self.ids.insert(id.to_string(), index);
        self.diagram.entities.push(Entity {
            id: id.to_string(),
            label: label.unwrap_or_else(|| clean_label(id)),
            attributes: Vec::new(),
        });
        Ok(index)
    }

    /// `NAME`, `NAME["alias"]` or `"quoted name"`.
    fn entity_declaration(&mut self, text: &str, line: usize) -> Result<usize, ParseError> {
        match text.find('[') {
            Some(open) if text.ends_with(']') => {
                let alias = clean_label(&text[open + 1..text.len() - 1]);
                self.entity(text[..open].trim(), Some(alias), line)
            }
            _ => self.entity(text.trim(), None, line),
        }
    }

    fn statement(&mut self, statement: &str, line: usize) -> Result<(), ParseError> {
        let word = statement
            .split(|c: char| c.is_whitespace() || c == ':')
            .next()
            .unwrap_or("");
        match word.to_ascii_lowercase().as_str() {
            "direction" => {
                let value = statement[word.len()..].trim();
                self.diagram.direction = match value {
                    "TB" | "TD" => Direction::TopDown,
                    "BT" => Direction::BottomUp,
                    "LR" => Direction::LeftRight,
                    "RL" => Direction::RightLeft,
                    other => {
                        return Err(self.error(
                            line,
                            format!(
                                "unknown direction `{}`: use TB, BT, LR or RL",
                                printable(other, 20)
                            ),
                        ))
                    }
                };
                return Ok(());
            }
            "style" | "classdef" | "class" | "acctitle" | "accdescr" => return Ok(()),
            _ => {}
        }
        // `A <relationship> B : label`, or a lone entity.
        let (head, label) = match find_label_colon(statement) {
            Some(colon) => (
                statement[..colon].trim(),
                Some(clean_label(&statement[colon + 1..])),
            ),
            None => (statement.trim(), None),
        };
        let (first, rest) = split_name(head);
        let rest = rest.trim();
        if rest.is_empty() {
            if label.is_some() {
                return Err(self.error(line, "a label needs a relationship before it".into()));
            }
            self.entity_declaration(first, line)?;
            return Ok(());
        }
        let Some((from_cardinality, identifying, to_cardinality, second)) = relation(rest) else {
            return Err(self.error(
                line,
                format!("`{}` is not drawn as text", printable(statement, 40)),
            ));
        };
        let (second, extra) = split_name(second.trim());
        if !extra.trim().is_empty() {
            return Err(self.error(
                line,
                format!("unexpected `{}`", printable(extra.trim(), 20)),
            ));
        }
        if self.diagram.relationships.len() >= MAX_RELATIONSHIPS {
            return Err(ParseError::TooLarge(format!(
                "more than {MAX_RELATIONSHIPS} relationships"
            )));
        }
        let from = self.entity_declaration(first, line)?;
        let to = self.entity_declaration(second, line)?;
        self.diagram.relationships.push(Relationship {
            from,
            to,
            from_cardinality,
            to_cardinality,
            identifying,
            label: label.unwrap_or_default(),
        });
        Ok(())
    }

    /// The attributes in an entity's `{ … }`.
    fn attributes(&mut self, entity: usize, body: &str, line: usize) -> Result<(), ParseError> {
        let tokens = tokens(body);
        let mut at = 0;
        while at < tokens.len() {
            let data_type = &tokens[at];
            let Some(name) = tokens
                .get(at + 1)
                .filter(|t| !t.starts_with('"') && *t != ",")
            else {
                return Err(self.error(
                    line,
                    format!(
                        "the attribute `{}` needs a type and a name",
                        printable(data_type, 30)
                    ),
                ));
            };
            if data_type.starts_with('"') || data_type == "," {
                return Err(self.error(
                    line,
                    format!("unexpected `{}` in an entity", printable(data_type, 30)),
                ));
            }
            let mut attribute = Attribute {
                data_type: data_type.clone(),
                name: name.clone(),
                ..Attribute::default()
            };
            at += 2;
            loop {
                match tokens.get(at).map(String::as_str) {
                    Some("PK") => attribute.keys.push(Key::Primary),
                    Some("FK") => attribute.keys.push(Key::Foreign),
                    Some("UK") => attribute.keys.push(Key::Unique),
                    Some(",") if !attribute.keys.is_empty() => {}
                    _ => break,
                }
                at += 1;
            }
            if let Some(comment) = tokens.get(at).filter(|t| t.starts_with('"')) {
                attribute.comment = Some(clean_label(comment)).filter(|c| !c.is_empty());
                at += 1;
            }
            self.diagram.entities[entity].attributes.push(attribute);
        }
        Ok(())
    }
}

/// Words, `"quoted strings"` (quotes kept) and commas outside parentheses.
fn tokens(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut depth = 0usize;
    for c in body.chars() {
        match c {
            '"' => {
                current.push(c);
                if quoted {
                    out.push(std::mem::take(&mut current));
                }
                quoted = !quoted;
            }
            _ if quoted => current.push(c),
            '(' | '[' => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if depth == 0 => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                out.push(",".into());
            }
            c if c.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// The `:` before a relationship's label: the first outside quotes.
fn find_label_colon(statement: &str) -> Option<usize> {
    let mut quoted = false;
    for (at, c) in statement.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some(at),
            _ => {}
        }
    }
    None
}

/// An entity name at the start of `text` (a word, a `"quoted name"`, either
/// with a `["alias"]`), and the rest.
fn split_name(text: &str) -> (&str, &str) {
    let mut end = 0;
    let mut chars = text.char_indices().peekable();
    if text.starts_with('"') {
        end = text
            .strip_prefix('"')
            .and_then(|rest| rest.find('"'))
            .map_or(text.len(), |close| close + 2);
    } else {
        while let Some(&(at, c)) = chars.peek() {
            if c.is_alphanumeric() || c == '_' || (c == '-' && !text[at..].starts_with("--")) {
                end = at + c.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
    }
    if text[end..].starts_with('[') {
        if let Some(close) = text[end..].find(']') {
            end += close + 1;
        }
    }
    (&text[..end], &text[end..])
}

/// `||--o{ B` or `only one to zero or more B`: the cardinalities, whether it
/// is identifying, and what follows (the second entity).
fn relation(text: &str) -> Option<(Cardinality, bool, Cardinality, &str)> {
    if let Some((symbol, left)) = LEFT.iter().find(|(s, _)| text.starts_with(s)) {
        let rest = &text[symbol.len()..];
        let identifying = match rest.get(..2)? {
            "--" => true,
            ".." => false,
            _ => return None,
        };
        let rest = &rest[2..];
        let (symbol, right) = RIGHT.iter().find(|(s, _)| rest.starts_with(s))?;
        return Some((*left, identifying, *right, &rest[symbol.len()..]));
    }
    let lower = text.to_ascii_lowercase();
    let (word, left) = WORDS.iter().find(|(w, _)| lower.starts_with(w))?;
    let rest = lower[word.len()..].trim_start();
    let (identifying, after) = if let Some(after) = rest.strip_prefix("optionally to ") {
        (false, after)
    } else {
        (true, rest.strip_prefix("to ")?)
    };
    let after = after.trim_start();
    let (word, right) = WORDS
        .iter()
        .find(|(w, _)| after.starts_with(w) && after[w.len()..].starts_with(char::is_whitespace))?;
    // Back to the original text, for the entity's case.
    let offset = text.len() - after.len() + word.len();
    Some((*left, identifying, *right, &text[offset..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ends(diagram: &EntityDiagram) -> Vec<(String, &str, &str, bool, String, &str)> {
        diagram
            .relationships
            .iter()
            .map(|r| {
                (
                    diagram.entities[r.from].id.clone(),
                    r.from_cardinality.notation(),
                    r.to_cardinality.notation(),
                    r.identifying,
                    diagram.entities[r.to].id.clone(),
                    r.label.as_str(),
                )
            })
            .collect()
    }

    fn syntax(source: &str) -> (usize, String) {
        match parse(source) {
            Err(ParseError::Syntax { line, message }) => (line, message),
            other => panic!("expected a syntax error, got {other:?}"),
        }
    }

    #[test]
    fn every_cardinality_and_line() {
        let diagram = parse(
            "erDiagram
              A ||--|| B : one
              A |o..o| C : \"zero or one\"
              A }o--o{ D : many
              A }|..|{ E-F : \"one or more\"",
        )
        .unwrap();
        let s = |x: &str| x.to_string();
        assert_eq!(
            ends(&diagram),
            [
                (s("A"), "1", "1", true, s("B"), "one"),
                (s("A"), "0..1", "0..1", false, s("C"), "zero or one"),
                (s("A"), "0..N", "0..N", true, s("D"), "many"),
                (s("A"), "1..N", "1..N", false, s("E-F"), "one or more"),
            ]
        );
    }

    #[test]
    fn word_forms() {
        let diagram = parse(
            "erDiagram
              CAR only one to zero or more NAMED-DRIVER : allows
              PERSON one or more optionally to zero or one CAR : drives
              X many(0) to 1+ Y : z
              X 1 to 0+ Y : w",
        )
        .unwrap();
        let s = |x: &str| x.to_string();
        assert_eq!(
            ends(&diagram),
            [
                (s("CAR"), "1", "0..N", true, s("NAMED-DRIVER"), "allows"),
                (s("PERSON"), "1..N", "0..1", false, s("CAR"), "drives"),
                (s("X"), "0..N", "1..N", true, s("Y"), "z"),
                (s("X"), "1", "0..N", true, s("Y"), "w"),
            ]
        );
    }

    #[test]
    fn attributes_keys_and_comments() {
        let diagram = parse(
            "erDiagram
              CUSTOMER[\"Customer Account\"] {
                string name PK \"their full name\"
                varchar(255,2) custNumber PK, FK
                string[] tags UK
                int *age
              }
              ORDER { int id PK string note }",
        )
        .unwrap();
        let customer = &diagram.entities[0];
        assert_eq!(customer.label, "Customer Account");
        let rows: Vec<(&str, &str, Vec<Key>, Option<&str>)> = customer
            .attributes
            .iter()
            .map(|a| {
                (
                    a.data_type.as_str(),
                    a.name.as_str(),
                    a.keys.clone(),
                    a.comment.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "string",
                    "name",
                    vec![Key::Primary],
                    Some("their full name")
                ),
                (
                    "varchar(255,2)",
                    "custNumber",
                    vec![Key::Primary, Key::Foreign],
                    None
                ),
                ("string[]", "tags", vec![Key::Unique], None),
                ("int", "*age", vec![], None),
            ]
        );
        assert_eq!(diagram.entities[1].attributes.len(), 2);
        assert_eq!(diagram.entities[1].attributes[1].name, "note");
        assert_eq!(
            syntax("erDiagram\n A {\n string\n }").1,
            "the attribute `string` needs a type and a name"
        );
        assert_eq!(
            syntax("erDiagram\n A {\n string x"),
            (2, "the `{` after `A` is never closed".into())
        );
    }

    #[test]
    fn entities_direction_and_title() {
        let diagram = parse(
            "---\ntitle: Shop\n---\nerDiagram\n  direction LR\n  LONELY\n  \"Two Words\" ||--o{ LONELY : has\n  style LONELY fill:#f9f\n  classDef x fill:#f00",
        )
        .unwrap();
        assert_eq!(diagram.title.as_deref(), Some("Shop"));
        assert_eq!(diagram.direction, Direction::LeftRight);
        let ids: Vec<&str> = diagram.entities.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["LONELY", "\"Two Words\""]);
        assert_eq!(diagram.entities[1].label, "Two Words");
        assert_eq!(parse("erDiagram").unwrap().direction, Direction::TopDown);
        assert_eq!(
            syntax("erDiagram\n direction XY").1,
            "unknown direction `XY`: use TB, BT, LR or RL"
        );
    }

    #[test]
    fn unsupported_syntax_names_itself() {
        assert_eq!(
            syntax("erDiagram\n A ||-->o{ B : x"),
            (2, "`A ||-->o{ B : x` is not drawn as text".into())
        );
        assert_eq!(
            syntax("erDiagram\n A <|-- B"),
            (2, "`A <|-- B` is not drawn as text".into())
        );
        assert_eq!(
            syntax("erDiagram\n A : label").1,
            "a label needs a relationship before it"
        );
        assert!(is_er("%% hi\nerDiagram\n A"));
        assert!(!is_er("graph TD"));
    }

    #[test]
    fn graphs_carry_cardinalities_at_their_ends() {
        let diagram = parse("erDiagram\n A ||..o{ B : has\n A |o--|| A : parent").unwrap();
        let (graph, notes) = diagram.to_graph();
        let edge = &graph.edges()[0];
        assert_eq!(
            (
                edge.start_label.as_deref(),
                edge.end_label.as_deref(),
                edge.label.as_deref(),
                edge.stroke,
            ),
            (Some("1"), Some("0..N"), Some("has"), Stroke::Dotted)
        );
        assert_eq!(notes, ["A relates to itself: `parent`, 0..1 to 1"]);
    }
}
