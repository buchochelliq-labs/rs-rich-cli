//! Parse Mermaid sequence diagrams (`sequenceDiagram`) and draw them as text.
//!
//! What is drawn:
//!
//! - participants and actors (`participant A as Alice`, `actor B`, or
//!   implied by a message), left to right in the order they appear, each a
//!   box (an actor a stick figure) heading its lifeline;
//! - messages with every Mermaid arrow: `->>` and `-->>` (arrowhead), `->`
//!   and `-->` (none), `-x` and `--x` (cross), `-)` and `--)` (open, for
//!   async), `<<->>` and `<<-->>` (both ends); one dash is a solid line, two
//!   a dotted one. A message to its sender loops back to its own lifeline;
//! - `Note left of A`, `Note right of A`, `Note over A` and `Note over A,B`;
//! - `activate A` / `deactivate A`, and the `+` / `-` shorthands on a message
//!   (`A->>+B` activates B; `B-->>-A` deactivates B), drawn as a heavy
//!   lifeline while active;
//! - `loop`, `alt` / `else`, `opt`, `par` / `and`, `critical` / `option`,
//!   `break` and `rect`, framing their messages with their kind and label in
//!   the top border, a dotted divider for each `else`, `and` or `option`;
//! - `autonumber` (with a start and step, and `autonumber off`), numbering
//!   each message's text; and `title` (or a front-matter `title:`) above.
//!
//! Simplified, with a note under the drawing: `box` groups are drawn without
//! their frames, `create` participants are drawn from the top, `destroy` does
//! not end a lifeline, a participant's `@{ … }` type is drawn as a box, and
//! nested activations share one bar. `links`, `link`, `properties`,
//! `details`, `accTitle` and `accDescr` are accepted and ignored. Anything
//! else is a [`ParseError`] naming it, and the diagram falls back to its
//! source.
//!
//! ```
//! use rich_mermaid::sequence;
//!
//! let diagram = sequence::parse("sequenceDiagram\n  Alice->>+Bob: Hi\n  Bob-->>-Alice: Hello").unwrap();
//! assert_eq!(diagram.participants.len(), 2);
//! let drawing = sequence::draw(&diagram, false).unwrap();
//! assert_eq!(drawing.lines[0], "┌───────┐   ┌─────┐");
//! ```

use std::collections::HashMap;

use crate::flowchart::{clean_label, printable, strip_comment, ParseError};

mod draw;

pub use draw::draw;

/// The largest source the parser accepts, in bytes ([`rich_diagram::MAX_SOURCE`]).
pub const MAX_SOURCE: usize = rich_diagram::MAX_SOURCE;
/// The most participants a diagram may have ([`rich_diagram::MAX_NODES`]).
pub const MAX_PARTICIPANTS: usize = rich_diagram::MAX_NODES;
/// The deepest blocks (`loop`, `alt`, …, `box`) may nest.
pub const MAX_NESTING: usize = 32;
/// The most statements that draw something (messages, notes and block
/// lines) a diagram may have ([`rich_diagram::MAX_EDGES`]).
pub const MAX_EVENTS: usize = rich_diagram::MAX_EDGES;

/// How a participant heads its lifeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ParticipantKind {
    /// A box (`participant`).
    #[default]
    Participant,
    /// A stick figure (`actor`).
    Actor,
}

/// A participant or actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Participant {
    /// What messages refer to it by.
    pub id: String,
    /// The text in its head (its alias, or its id); `\n` breaks a line.
    pub label: String,
    pub kind: ParticipantKind,
}

/// A message's line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineStyle {
    /// One dash (`->>`).
    #[default]
    Solid,
    /// Two dashes (`-->>`).
    Dotted,
}

/// The end of a message's line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tip {
    /// Nothing (`->`).
    #[default]
    None,
    /// An arrowhead (`->>`).
    Arrow,
    /// A cross (`-x`).
    Cross,
    /// An open arrowhead, for an async message (`-)`).
    Open,
}

/// A message from one participant to another (or to itself).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    /// The sender, by index into [`Sequence::participants`].
    pub from: usize,
    /// The receiver.
    pub to: usize,
    /// The text, `\n` breaking lines; empty for none.
    pub text: String,
    pub line: LineStyle,
    /// The tip at the sender (only `<<->>` has one).
    pub start: Tip,
    /// The tip at the receiver.
    pub end: Tip,
    /// Its `autonumber` number, when numbering is on.
    pub number: Option<u64>,
}

/// Where a note sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotePlacement {
    LeftOf(usize),
    RightOf(usize),
    /// Over one participant, or spanning from the first to the second (left
    /// to right; both the same for one).
    Over(usize, usize),
}

/// The kind of a block framing messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Loop,
    Alt,
    Opt,
    Par,
    Critical,
    Break,
    /// A highlighted region (`rect`): a frame with no label.
    Rect,
}

impl BlockKind {
    /// The keyword that opens it.
    pub fn keyword(self) -> &'static str {
        match self {
            BlockKind::Loop => "loop",
            BlockKind::Alt => "alt",
            BlockKind::Opt => "opt",
            BlockKind::Par => "par",
            BlockKind::Critical => "critical",
            BlockKind::Break => "break",
            BlockKind::Rect => "rect",
        }
    }

    /// The keyword that divides it (`else`, `and`, `option`), if it has one.
    pub fn divider(self) -> Option<&'static str> {
        match self {
            BlockKind::Alt => Some("else"),
            BlockKind::Par => Some("and"),
            BlockKind::Critical => Some("option"),
            _ => None,
        }
    }
}

/// One statement that draws something, or changes how what follows is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Message(Message),
    Note {
        placement: NotePlacement,
        text: String,
    },
    /// The participant's lifeline turns active from its last message on (or
    /// from here, when the last statement was not a message).
    Activate(usize),
    /// It turns inactive after its last message (or here).
    Deactivate(usize),
    /// A block opens: its kind and label (empty for none).
    Start {
        kind: BlockKind,
        label: String,
    },
    /// An `else`, `and` or `option` in the innermost open block.
    Divider {
        label: String,
    },
    /// The innermost open block closes.
    End,
}

/// A parsed sequence diagram.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sequence {
    pub title: Option<String>,
    /// Left to right, in order of first appearance.
    pub participants: Vec<Participant>,
    /// In order; every block is closed.
    pub events: Vec<Event>,
    /// Things the text renderer simplified, to show under the drawing.
    pub notes: Vec<String>,
}

/// Whether `source` is a sequence diagram: its first statement, after front
/// matter and comments, is `sequenceDiagram`.
pub fn is_sequence(source: &str) -> bool {
    header_keyword(source) == Some("sequenceDiagram")
}

/// The first word of the first statement after front matter and comments.
pub(crate) fn header_keyword(source: &str) -> Option<&str> {
    let mut in_front_matter = false;
    for raw in source.lines() {
        let line = strip_comment(raw).trim();
        if line == "---" {
            in_front_matter = !in_front_matter;
            continue;
        }
        if in_front_matter || line.is_empty() {
            continue;
        }
        return line
            .split(|c: char| c.is_whitespace() || c == ';' || c == '{')
            .next();
    }
    None
}

/// A source's front-matter title, its header keyword, and its statements
/// with their line numbers.
pub(crate) type Statements = (Option<String>, String, Vec<(usize, String)>);

/// The statements after the header line, with their line numbers, and the
/// front matter's `title:`. Statements split at line breaks and at `;`
/// outside quotes and `#…;` entity codes. Fails when the source is too large
/// or has no header.
pub(crate) fn statements(source: &str) -> Result<Statements, ParseError> {
    if source.len() > MAX_SOURCE {
        return Err(ParseError::TooLarge(format!(
            "{} bytes, more than {MAX_SOURCE}",
            source.len()
        )));
    }
    let mut title = None;
    let mut header: Option<String> = None;
    let mut in_front_matter = false;
    let mut out = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(raw).trim();
        if header.is_none() && line == "---" {
            in_front_matter = !in_front_matter;
            continue;
        }
        if in_front_matter {
            if let Some(rest) = line.strip_prefix("title:") {
                title = Some(clean_label(rest)).filter(|t| !t.is_empty());
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let mut parts = split(line).into_iter();
        if header.is_none() {
            let first = parts.next().unwrap_or_default();
            let keyword_end = first
                .find(|c: char| c.is_whitespace() || c == '{')
                .unwrap_or(first.len());
            header = Some(first[..keyword_end].to_string());
            let rest = first[keyword_end..].trim();
            if !rest.is_empty() {
                out.push((number, rest.to_string()));
            }
        }
        for part in parts {
            let part = part.trim();
            if !part.is_empty() {
                out.push((number, part.to_string()));
            }
        }
    }
    let header = header.ok_or(ParseError::Empty)?;
    Ok((title, header, out))
}

/// Split a line at `;` outside double quotes and `#name;` entity codes.
fn split(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let chars: Vec<char> = line.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        match c {
            '"' => quoted = !quoted,
            '#' => {
                let name = chars[at + 1..]
                    .iter()
                    .take(9)
                    .position(|c| !c.is_ascii_alphanumeric());
                if let Some(len) = name.filter(|&len| len > 0) {
                    if chars.get(at + 1 + len) == Some(&';') {
                        current.extend(&chars[at..at + len + 2]);
                        at += len + 2;
                        continue;
                    }
                }
            }
            ';' if !quoted => {
                parts.push(std::mem::take(&mut current));
                at += 1;
                continue;
            }
            _ => {}
        }
        current.push(c);
        at += 1;
    }
    parts.push(current);
    parts
}

/// Parse a Mermaid sequence diagram.
///
/// ```
/// use rich_mermaid::sequence::{parse, Event, LineStyle, Tip};
///
/// let diagram = parse("sequenceDiagram\n  participant A as Alice\n  A--)B: later").unwrap();
/// assert_eq!(diagram.participants[0].label, "Alice");
/// let Event::Message(message) = &diagram.events[0] else { unreachable!() };
/// assert_eq!((message.line, message.end), (LineStyle::Dotted, Tip::Open));
/// ```
pub fn parse(source: &str) -> Result<Sequence, ParseError> {
    let (title, header, statements) = statements(source)?;
    if header != "sequenceDiagram" {
        return Err(ParseError::Syntax {
            line: 1,
            message: format!(
                "expected `sequenceDiagram`, found `{}`",
                printable(&header, 40)
            ),
        });
    }
    let mut parser = Parser {
        diagram: Sequence {
            title,
            ..Sequence::default()
        },
        ..Parser::default()
    };
    let mut skipping_description = false;
    for (number, statement) in &statements {
        if skipping_description {
            skipping_description = !statement.contains('}');
            continue;
        }
        let lower = statement.to_ascii_lowercase();
        if lower.starts_with("accdescr") && statement.contains('{') {
            skipping_description = !statement.contains('}');
            continue;
        }
        parser.statement(statement, *number)?;
    }
    if let Some(open) = parser.blocks.last() {
        return Err(ParseError::Syntax {
            line: open.line,
            message: format!("`{}` is never closed with `end`", open.keyword),
        });
    }
    Ok(parser.diagram)
}

/// An open block, for matching `else`, `and`, `option` and `end`.
#[derive(Debug)]
struct Open {
    /// `None` for a `box`, which draws nothing.
    kind: Option<BlockKind>,
    keyword: &'static str,
    line: usize,
}

#[derive(Default)]
struct Parser {
    diagram: Sequence,
    ids: HashMap<String, usize>,
    blocks: Vec<Open>,
    /// The next `autonumber` number and the step, while numbering is on.
    numbering: Option<(u64, u64)>,
    /// How many times each participant is active.
    active: Vec<usize>,
    noted: Vec<&'static str>,
}

/// The arrows, longest first so a prefix never wins.
const ARROWS: &[(&str, LineStyle, Tip, Tip)] = &[
    ("<<-->>", LineStyle::Dotted, Tip::Arrow, Tip::Arrow),
    ("<<->>", LineStyle::Solid, Tip::Arrow, Tip::Arrow),
    ("-->>", LineStyle::Dotted, Tip::None, Tip::Arrow),
    ("->>", LineStyle::Solid, Tip::None, Tip::Arrow),
    ("-->", LineStyle::Dotted, Tip::None, Tip::None),
    ("--x", LineStyle::Dotted, Tip::None, Tip::Cross),
    ("--)", LineStyle::Dotted, Tip::None, Tip::Open),
    ("->", LineStyle::Solid, Tip::None, Tip::None),
    ("-x", LineStyle::Solid, Tip::None, Tip::Cross),
    ("-)", LineStyle::Solid, Tip::None, Tip::Open),
];

impl Parser {
    fn error(&self, line: usize, message: String) -> ParseError {
        ParseError::Syntax { line, message }
    }

    fn note_once(&mut self, key: &'static str, text: &str) {
        if !self.noted.contains(&key) {
            self.noted.push(key);
            self.diagram.notes.push(text.to_string());
        }
    }

    fn push(&mut self, event: Event) -> Result<(), ParseError> {
        if self.diagram.events.len() >= MAX_EVENTS {
            return Err(ParseError::TooLarge(format!(
                "more than {MAX_EVENTS} statements"
            )));
        }
        self.diagram.events.push(event);
        Ok(())
    }

    /// The index of participant `id`, adding it if it is new.
    fn participant(&mut self, id: &str, line: usize) -> Result<usize, ParseError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(self.error(line, "expected a participant".into()));
        }
        if let Some(&index) = self.ids.get(id) {
            return Ok(index);
        }
        self.declare(id, None, ParticipantKind::Participant, line)
    }

    fn declare(
        &mut self,
        id: &str,
        label: Option<String>,
        kind: ParticipantKind,
        line: usize,
    ) -> Result<usize, ParseError> {
        if let Some(&index) = self.ids.get(id) {
            let participant = &mut self.diagram.participants[index];
            if let Some(label) = label {
                participant.label = label;
            }
            participant.kind = kind;
            return Ok(index);
        }
        if self.diagram.participants.len() >= MAX_PARTICIPANTS {
            return Err(ParseError::TooLarge(format!(
                "more than {MAX_PARTICIPANTS} participants"
            )));
        }
        if id.contains(['-', '>', ':', ',']) {
            return Err(self.error(
                line,
                format!("`{}` is not a participant name", printable(id, 40)),
            ));
        }
        let index = self.diagram.participants.len();
        self.ids.insert(id.to_string(), index);
        self.diagram.participants.push(Participant {
            id: id.to_string(),
            label: label.unwrap_or_else(|| clean_label(id)),
            kind,
        });
        self.active.push(0);
        Ok(index)
    }

    fn statement(&mut self, statement: &str, line: usize) -> Result<(), ParseError> {
        let (word, rest) = match statement.find(char::is_whitespace) {
            Some(at) => (&statement[..at], statement[at..].trim()),
            None => (statement, ""),
        };
        let keyword = word.to_ascii_lowercase();
        let keyword = keyword.split(':').next().unwrap_or_default();
        match keyword {
            "participant" | "actor" => {
                let kind = if keyword == "actor" {
                    ParticipantKind::Actor
                } else {
                    ParticipantKind::Participant
                };
                self.declaration(rest, kind, line)
            }
            "create" => {
                self.note_once(
                    "create",
                    "created participants are drawn from the top of the diagram",
                );
                let (word, rest) = match rest.find(char::is_whitespace) {
                    Some(at) => (&rest[..at], rest[at..].trim()),
                    None => (rest, ""),
                };
                let kind = match word.to_ascii_lowercase().as_str() {
                    "participant" => ParticipantKind::Participant,
                    "actor" => ParticipantKind::Actor,
                    _ => {
                        return Err(self.error(
                            line,
                            "`create` needs `participant` or `actor` after it".into(),
                        ))
                    }
                };
                self.declaration(rest, kind, line)
            }
            "destroy" => {
                self.participant(rest, line)?;
                self.note_once(
                    "destroy",
                    "destroyed participants' lifelines are drawn to the end",
                );
                Ok(())
            }
            "box" => {
                self.note_once("box", "boxes are drawn without their frames");
                self.blocks.push(Open {
                    kind: None,
                    keyword: "box",
                    line,
                });
                Ok(())
            }
            "loop" | "alt" | "opt" | "par" | "par_over" | "critical" | "break" | "rect" => {
                let kind = match keyword {
                    "loop" => BlockKind::Loop,
                    "alt" => BlockKind::Alt,
                    "opt" => BlockKind::Opt,
                    "par" | "par_over" => BlockKind::Par,
                    "critical" => BlockKind::Critical,
                    "break" => BlockKind::Break,
                    _ => BlockKind::Rect,
                };
                // A `rect`'s argument is its colour, which text cannot show.
                let label = if kind == BlockKind::Rect {
                    String::new()
                } else {
                    clean_label(rest)
                };
                if self.blocks.len() >= MAX_NESTING {
                    return Err(ParseError::TooLarge(format!(
                        "blocks nested more than {MAX_NESTING} deep"
                    )));
                }
                self.blocks.push(Open {
                    kind: Some(kind),
                    keyword: kind.keyword(),
                    line,
                });
                self.push(Event::Start { kind, label })
            }
            "else" | "and" | "option" => {
                let open = self.blocks.last().and_then(|open| open.kind);
                if open.and_then(BlockKind::divider) != Some(keyword) {
                    let needs = match keyword {
                        "else" => "an `alt`",
                        "and" => "a `par`",
                        _ => "a `critical`",
                    };
                    return Err(self.error(line, format!("`{keyword}` outside {needs} block")));
                }
                self.push(Event::Divider {
                    label: clean_label(rest),
                })
            }
            "end" if rest.is_empty() => match self.blocks.pop() {
                Some(Open { kind: None, .. }) => Ok(()),
                Some(_) => self.push(Event::End),
                None => Err(self.error(line, "`end` without a block to close".into())),
            },
            "note" => self.note(rest, line),
            "activate" | "deactivate" => {
                let index = self.participant(rest, line)?;
                if keyword == "activate" {
                    self.active[index] += 1;
                    self.push(Event::Activate(index))
                } else {
                    self.deactivate(index, line)?;
                    self.push(Event::Deactivate(index))
                }
            }
            "autonumber" => {
                let words: Vec<&str> = rest.split_whitespace().collect();
                self.numbering = match words.as_slice() {
                    [] => Some((1, 1)),
                    ["off"] => None,
                    [start] => Some((self.number(start, line)?, 1)),
                    [start, step] => Some((self.number(start, line)?, self.number(step, line)?)),
                    _ => {
                        return Err(self.error(line, "`autonumber` takes a start and a step".into()))
                    }
                };
                Ok(())
            }
            "title" => {
                // `title text`, `title: text` or `title:text`.
                let text = statement["title".len()..]
                    .trim_start()
                    .trim_start_matches(':');
                self.diagram.title = Some(clean_label(text)).filter(|t| !t.is_empty());
                Ok(())
            }
            "acctitle" | "accdescr" | "links" | "link" | "properties" | "details" => Ok(()),
            _ => self.message(statement, line),
        }
    }

    fn number(&self, text: &str, line: usize) -> Result<u64, ParseError> {
        text.parse().map_err(|_| {
            self.error(
                line,
                format!("`autonumber` takes numbers, not `{}`", printable(text, 20)),
            )
        })
    }

    fn deactivate(&mut self, index: usize, line: usize) -> Result<(), ParseError> {
        if self.active[index] == 0 {
            let id = printable(&self.diagram.participants[index].id, 40);
            return Err(self.error(line, format!("`{id}` is deactivated but is not active")));
        }
        self.active[index] -= 1;
        Ok(())
    }

    /// `A`, `A as Alias`, `A@{ … }`.
    fn declaration(
        &mut self,
        rest: &str,
        kind: ParticipantKind,
        line: usize,
    ) -> Result<(), ParseError> {
        let mut rest = rest;
        if let Some(at) = rest.find("@{") {
            self.note_once(
                "metadata",
                "participant types (`@{ … }`) are drawn as plain participants",
            );
            if !rest.trim_end().ends_with('}') {
                return Err(self.error(line, "`@{` is never closed with `}`".into()));
            }
            rest = &rest[..at];
        }
        let (id, alias) = match split_as(rest) {
            Some((id, alias)) => (id, Some(clean_label(alias))),
            None => (rest.trim(), None),
        };
        if id.is_empty() {
            return Err(self.error(line, "expected a participant name".into()));
        }
        self.declare(id, alias, kind, line).map(|_| ())
    }

    /// `Note left of A: text`, `Note right of A`, `Note over A[,B]`.
    fn note(&mut self, rest: &str, line: usize) -> Result<(), ParseError> {
        let Some(colon) = rest.find(':') else {
            return Err(self.error(line, "a note needs `:` and its text".into()));
        };
        let (place, text) = (rest[..colon].trim(), clean_label(&rest[colon + 1..]));
        let lower = place.to_ascii_lowercase();
        let after = |prefix: &str| -> Option<&str> {
            lower
                .strip_prefix(prefix)
                .filter(|r| r.starts_with(char::is_whitespace))
                .map(|r| place[place.len() - r.len()..].trim())
        };
        let placement = if let Some(who) = after("left of") {
            NotePlacement::LeftOf(self.participant(who, line)?)
        } else if let Some(who) = after("right of") {
            NotePlacement::RightOf(self.participant(who, line)?)
        } else if let Some(who) = after("over") {
            let names: Vec<&str> = who.split(',').collect();
            match names.as_slice() {
                [one] => {
                    let a = self.participant(one, line)?;
                    NotePlacement::Over(a, a)
                }
                [one, two] => {
                    let (a, b) = (self.participant(one, line)?, self.participant(two, line)?);
                    NotePlacement::Over(a.min(b), a.max(b))
                }
                _ => return Err(self.error(line, "a note spans at most two participants".into())),
            }
        } else {
            return Err(self.error(
                line,
                format!(
                    "a note is `left of`, `right of` or `over`, not `{}`",
                    printable(place, 30)
                ),
            ));
        };
        self.push(Event::Note { placement, text })
    }

    /// `A->>B: text`, with `+` or `-` before the receiver.
    fn message(&mut self, statement: &str, line: usize) -> Result<(), ParseError> {
        // Participant names cannot hold `-` or `<`: the arrow starts at the
        // first of either.
        let Some(at) = statement.find(['-', '<']) else {
            let word = statement.split_whitespace().next().unwrap_or("");
            return Err(self.error(
                line,
                format!("`{}` is not drawn as text", printable(word, 30)),
            ));
        };
        let after = &statement[at..];
        let Some(&(arrow, style, start, end)) =
            ARROWS.iter().find(|(arrow, ..)| after.starts_with(arrow))
        else {
            return Err(self.error(line, format!("unknown arrow at `{}`", printable(after, 20))));
        };
        let from = statement[..at].trim();
        let mut rest = after[arrow.len()..].trim_start();
        let (activate, deactivate) = match rest.chars().next() {
            Some('+') => {
                rest = &rest[1..];
                (true, false)
            }
            Some('-') => {
                rest = &rest[1..];
                (false, true)
            }
            _ => (false, false),
        };
        let (to, text) = match rest.find(':') {
            Some(colon) => (&rest[..colon], clean_label(&rest[colon + 1..])),
            None => (rest, String::new()),
        };
        let from = self.participant(from, line)?;
        let to = self.participant(to, line)?;
        let number = self.numbering.map(|(next, step)| {
            self.numbering = Some((next.saturating_add(step), step));
            next
        });
        self.push(Event::Message(Message {
            from,
            to,
            text,
            line: style,
            start,
            end,
            number,
        }))?;
        if activate {
            self.active[to] += 1;
            self.push(Event::Activate(to))?;
        }
        if deactivate {
            self.deactivate(from, line)?;
            self.push(Event::Deactivate(from))?;
        }
        Ok(())
    }
}

/// `id as alias`, split at the first ` as `.
fn split_as(text: &str) -> Option<(&str, &str)> {
    let lower = text.to_ascii_lowercase();
    let at = lower.find(" as ")?;
    Some((text[..at].trim(), text[at + 4..].trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(diagram: &Sequence) -> Vec<&Message> {
        diagram
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Message(message) => Some(message),
                _ => None,
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
    fn participants_actors_and_aliases() {
        let diagram = parse(
            "sequenceDiagram
              participant A as Alice
              actor B as Bob<br>Smith
              participant Carol
              A->>D: hi",
        )
        .unwrap();
        let heads: Vec<(&str, &str, ParticipantKind)> = diagram
            .participants
            .iter()
            .map(|p| (p.id.as_str(), p.label.as_str(), p.kind))
            .collect();
        assert_eq!(
            heads,
            [
                ("A", "Alice", ParticipantKind::Participant),
                ("B", "Bob\nSmith", ParticipantKind::Actor),
                ("Carol", "Carol", ParticipantKind::Participant),
                ("D", "D", ParticipantKind::Participant),
            ]
        );
        // Names may hold spaces.
        let diagram = parse("sequenceDiagram\n  Alice Smith->>Bob Jones: hi").unwrap();
        assert_eq!(diagram.participants[0].id, "Alice Smith");
        assert_eq!(diagram.participants[1].id, "Bob Jones");
    }

    #[test]
    fn every_arrow() {
        let source = "sequenceDiagram
            A->>B: a
            A-->>B: b
            A->B: c
            A-->B: d
            A-xB: e
            A--xB: f
            A-)B: g
            A--)B: h
            A<<->>B: i
            A<<-->>B: j
            B->>B";
        let diagram = parse(source).unwrap();
        let found: Vec<(LineStyle, Tip, Tip, &str)> = messages(&diagram)
            .iter()
            .map(|m| (m.line, m.start, m.end, m.text.as_str()))
            .collect();
        use LineStyle::{Dotted, Solid};
        assert_eq!(
            found,
            [
                (Solid, Tip::None, Tip::Arrow, "a"),
                (Dotted, Tip::None, Tip::Arrow, "b"),
                (Solid, Tip::None, Tip::None, "c"),
                (Dotted, Tip::None, Tip::None, "d"),
                (Solid, Tip::None, Tip::Cross, "e"),
                (Dotted, Tip::None, Tip::Cross, "f"),
                (Solid, Tip::None, Tip::Open, "g"),
                (Dotted, Tip::None, Tip::Open, "h"),
                (Solid, Tip::Arrow, Tip::Arrow, "i"),
                (Dotted, Tip::Arrow, Tip::Arrow, "j"),
                (Solid, Tip::None, Tip::Arrow, ""),
            ]
        );
        let last = messages(&diagram)[10];
        assert_eq!((last.from, last.to), (1, 1));
        assert_eq!(
            syntax("sequenceDiagram\n A=>B: x").1,
            "`A=>B:` is not drawn as text"
        );
        assert_eq!(
            syntax("sequenceDiagram\n A-|B: x").1,
            "unknown arrow at `-|B: x`"
        );
    }

    #[test]
    fn notes() {
        let diagram = parse(
            "sequenceDiagram
              participant A
              participant B
              Note left of A: one
              note right of B: two
              Note over A: three
              Note over B,A: four<br/>lines",
        )
        .unwrap();
        let notes: Vec<(NotePlacement, &str)> = diagram
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Note { placement, text } => Some((*placement, text.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(
            notes,
            [
                (NotePlacement::LeftOf(0), "one"),
                (NotePlacement::RightOf(1), "two"),
                (NotePlacement::Over(0, 0), "three"),
                (NotePlacement::Over(0, 1), "four\nlines"),
            ]
        );
        assert_eq!(syntax("sequenceDiagram\n Note A: x").0, 2);
        assert_eq!(
            syntax("sequenceDiagram\n Note over A").1,
            "a note needs `:` and its text"
        );
    }

    #[test]
    fn activations() {
        let diagram = parse(
            "sequenceDiagram
              A->>+B: go
              activate A
              B-->>-A: done
              deactivate A",
        )
        .unwrap();
        let kinds: Vec<String> = diagram
            .events
            .iter()
            .map(|event| match event {
                Event::Message(m) => format!("{}>{}", m.from, m.to),
                Event::Activate(p) => format!("+{p}"),
                Event::Deactivate(p) => format!("-{p}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(kinds, ["0>1", "+1", "+0", "1>0", "-1", "-0"]);
        assert_eq!(
            syntax("sequenceDiagram\n A->>B: x\n deactivate B"),
            (3, "`B` is deactivated but is not active".into())
        );
        assert_eq!(
            syntax("sequenceDiagram\n A->>-B: x").1,
            "`A` is deactivated but is not active"
        );
    }

    #[test]
    fn blocks() {
        let diagram = parse(
            "sequenceDiagram
              loop Every minute
                A->>B: ping
              end
              alt is sick
                B->>A: not so good
              else is well
                B->>A: fine
              end
              opt extra
              end
              par A to B
                A->>B: x
              and A to C
                A->>C: y
              end
              critical connect
                A->>B: c
              option timeout
                A->>B: t
              end
              break failed
                A->>B: b
              end
              rect rgb(0, 0, 255)
                A->>B: r
              end",
        )
        .unwrap();
        let shape: Vec<String> = diagram
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Start { kind, label } => Some(format!("{}[{label}]", kind.keyword())),
                Event::Divider { label } => Some(format!("|{label}")),
                Event::End => Some("end".into()),
                _ => None,
            })
            .collect();
        assert_eq!(
            shape,
            [
                "loop[Every minute]",
                "end",
                "alt[is sick]",
                "|is well",
                "end",
                "opt[extra]",
                "end",
                "par[A to B]",
                "|A to C",
                "end",
                "critical[connect]",
                "|timeout",
                "end",
                "break[failed]",
                "end",
                "rect[]",
                "end",
            ]
        );
        assert_eq!(
            syntax("sequenceDiagram\n loop x\n A->>B: y"),
            (2, "`loop` is never closed with `end`".into())
        );
        assert_eq!(
            syntax("sequenceDiagram\n A->>B: y\n end"),
            (3, "`end` without a block to close".into())
        );
        assert_eq!(
            syntax("sequenceDiagram\n loop x\n else y\n end").1,
            "`else` outside an `alt` block"
        );
        assert_eq!(
            syntax("sequenceDiagram\n alt x\n and y\n end").1,
            "`and` outside a `par` block"
        );
    }

    #[test]
    fn autonumber_and_title() {
        let diagram = parse(
            "sequenceDiagram
              title: Checkout
              A->>B: one
              autonumber
              A->>B: two
              A->>B: three
              autonumber 10 5
              A->>B: four
              A->>B: five
              autonumber off
              A->>B: six",
        )
        .unwrap();
        assert_eq!(diagram.title.as_deref(), Some("Checkout"));
        let numbers: Vec<Option<u64>> = messages(&diagram).iter().map(|m| m.number).collect();
        assert_eq!(numbers, [None, Some(1), Some(2), Some(10), Some(15), None]);
        let titled =
            parse("---\ntitle: From front matter\n---\nsequenceDiagram\n A->>B: x").unwrap();
        assert_eq!(titled.title.as_deref(), Some("From front matter"));
        let titled = parse("sequenceDiagram\n title Plain title\n A->>B: x").unwrap();
        assert_eq!(titled.title.as_deref(), Some("Plain title"));
    }

    #[test]
    fn simplified_constructs_are_noted() {
        let diagram = parse(
            "sequenceDiagram
              box Aqua Group
                participant A
                participant B
              end
              create participant C
              A->>C: make
              destroy C
              participant D@{ \"type\": \"database\" }
              links A: {\"Docs\": \"https://example.com\"}
              accTitle: Hidden
              accDescr {
                many lines
              }",
        )
        .unwrap();
        assert_eq!(diagram.participants.len(), 4);
        assert_eq!(
            diagram.notes,
            [
                "boxes are drawn without their frames",
                "created participants are drawn from the top of the diagram",
                "destroyed participants' lifelines are drawn to the end",
                "participant types (`@{ … }`) are drawn as plain participants",
            ]
        );
    }

    #[test]
    fn statements_split_at_semicolons_but_not_entities() {
        let diagram = parse("sequenceDiagram; A->>B: one#59; two; B->>A: back").unwrap();
        let texts: Vec<&str> = messages(&diagram).iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["one; two", "back"]);
    }

    #[test]
    fn other_diagrams_and_limits() {
        assert!(is_sequence(
            "%% c\n---\ntitle: x\n---\nsequenceDiagram\n A->>B: x"
        ));
        assert!(!is_sequence("graph TD\n A-->B"));
        assert_eq!(parse(""), Err(ParseError::Empty));
        assert!(matches!(parse("graph TD"), Err(ParseError::Syntax { .. })));
        let big = format!("sequenceDiagram\n{}", "A->>B: x\n".repeat(MAX_EVENTS + 1));
        assert!(matches!(parse(&big), Err(ParseError::TooLarge(_))));
        let wide: String = (0..=MAX_PARTICIPANTS)
            .map(|i| format!("participant p{i}\n"))
            .collect();
        assert!(matches!(
            parse(&format!("sequenceDiagram\n{wide}")),
            Err(ParseError::TooLarge(_))
        ));
    }
}
