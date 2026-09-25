//! Parse Mermaid flowcharts (`graph` / `flowchart`).
//!
//! The subset the text renderer draws: a direction, nodes in the common
//! shapes, and edges (solid, thick, dotted or invisible; arrow, circle or cross
//! heads; labels; longer links; chains and `&`). Styling statements (`style`,
//! `classDef`, `class`, `linkStyle`, `click`) are accepted and ignored.
//! Subgraphs are drawn flat, with a note. Anything else is a [`ParseError`].

use std::collections::HashMap;
use std::fmt;

/// The largest source the parser accepts, in bytes.
pub const MAX_SOURCE: usize = 64 * 1024;
/// The most nodes a flowchart may have.
pub const MAX_NODES: usize = 500;
/// The most edges a flowchart may have.
pub const MAX_EDGES: usize = 2000;

/// Which way the flowchart flows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// `TD` or `TB`.
    #[default]
    TopDown,
    /// `BT`.
    BottomUp,
    /// `LR`.
    LeftRight,
    /// `RL`.
    RightLeft,
}

impl Direction {
    /// Whether ranks run horizontally (`LR` / `RL`).
    pub fn is_horizontal(self) -> bool {
        matches!(self, Direction::LeftRight | Direction::RightLeft)
    }
}

/// A node's shape, from the brackets around its label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    /// `A[text]`
    #[default]
    Rect,
    /// `A(text)`
    Round,
    /// `A([text])`
    Stadium,
    /// `A[[text]]`
    Subroutine,
    /// `A[(text)]`
    Cylinder,
    /// `A((text))`
    Circle,
    /// `A(((text)))`
    DoubleCircle,
    /// `A>text]`
    Asymmetric,
    /// `A{text}`
    Rhombus,
    /// `A{{text}}`
    Hexagon,
    /// `A[/text/]`
    Parallelogram,
    /// `A[\text\]`
    ParallelogramAlt,
    /// `A[/text\]`
    Trapezoid,
    /// `A[\text/]`
    TrapezoidAlt,
}

/// A node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    /// The label, with `<br>` turned into `\n` and control characters removed.
    pub label: String,
    pub shape: Shape,
}

/// How an edge is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stroke {
    /// `-->`
    Solid,
    /// `==>`
    Thick,
    /// `-.->`
    Dotted,
    /// `~~~`: laid out but not drawn.
    Invisible,
}

/// The end of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    None,
    /// `>`
    Arrow,
    /// `o`
    Circle,
    /// `x`
    Cross,
}

/// An edge between two nodes (indexes into [`Flowchart::nodes`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    pub stroke: Stroke,
    /// The head at `from` (`<-->` has one at both ends).
    pub start: Head,
    /// The head at `to`.
    pub end: Head,
    /// The minimum number of ranks the edge spans: 1 for `-->`, 2 for `--->`.
    pub length: usize,
}

/// A parsed flowchart.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Flowchart {
    pub direction: Direction,
    /// Nodes in order of first appearance.
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Things the text renderer simplified, to show under the diagram.
    pub notes: Vec<String>,
}

/// Why a source could not be parsed as a flowchart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Nothing but blank lines and comments.
    Empty,
    /// A Mermaid diagram of another kind, such as `"sequence"`.
    Unsupported(String),
    /// Too big to lay out as text.
    TooLarge(String),
    /// Not valid flowchart syntax.
    Syntax { line: usize, message: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => f.write_str("the diagram is empty"),
            ParseError::Unsupported(kind) => {
                write!(f, "{kind} diagrams are not drawn as text")
            }
            ParseError::TooLarge(what) => write!(f, "the diagram is too large: {what}"),
            ParseError::Syntax { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// The first keyword of other Mermaid diagram types, and what to call them.
const OTHER_KINDS: &[(&str, &str)] = &[
    ("sequenceDiagram", "sequence"),
    ("classDiagram", "class"),
    ("classDiagram-v2", "class"),
    ("stateDiagram", "state"),
    ("stateDiagram-v2", "state"),
    ("erDiagram", "entity relationship"),
    ("gantt", "Gantt"),
    ("pie", "pie chart"),
    ("journey", "user journey"),
    ("gitGraph", "Git graph"),
    ("mindmap", "mind map"),
    ("timeline", "timeline"),
    ("quadrantChart", "quadrant chart"),
    ("requirementDiagram", "requirement"),
    ("C4Context", "C4"),
    ("C4Container", "C4"),
    ("C4Component", "C4"),
    ("C4Dynamic", "C4"),
    ("C4Deployment", "C4"),
    ("sankey-beta", "Sankey"),
    ("xychart-beta", "XY chart"),
    ("block-beta", "block"),
    ("packet-beta", "packet"),
    ("architecture-beta", "architecture"),
    ("kanban", "Kanban"),
    ("radar-beta", "radar"),
    ("treemap-beta", "treemap"),
    ("zenuml", "ZenUML"),
];

/// Parse a Mermaid flowchart.
pub fn parse(source: &str) -> Result<Flowchart, ParseError> {
    if source.len() > MAX_SOURCE {
        return Err(ParseError::TooLarge(format!(
            "{} bytes, more than {MAX_SOURCE}",
            source.len()
        )));
    }
    let mut parser = Parser::default();
    let mut header_seen = false;
    let mut in_front_matter = false;
    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;
        let line = strip_comment(raw).trim();
        // YAML front matter (`---` … `---`) before the header: config and title.
        if !header_seen && line == "---" {
            in_front_matter = !in_front_matter;
            continue;
        }
        if in_front_matter || line.is_empty() {
            continue;
        }
        let rest = if header_seen {
            line
        } else {
            header_seen = true;
            parser.header(line, number)?
        };
        for statement in split_statements(rest) {
            parser.statement(statement.trim(), number)?;
        }
    }
    if !header_seen {
        return Err(ParseError::Empty);
    }
    Ok(parser.chart)
}

/// Drop a `%%` comment (including `%%{init: …}%%` directives).
fn strip_comment(line: &str) -> &str {
    match line.find("%%") {
        Some(at) => &line[..at],
        None => line,
    }
}

/// Split a line on `;` outside quotes and brackets.
fn split_statements(line: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut start = 0;
    for (at, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth = depth.saturating_sub(1),
            ';' if !quoted && depth == 0 => {
                parts.push(&line[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&line[start..]);
    parts
}

#[derive(Default)]
struct Parser {
    chart: Flowchart,
    ids: HashMap<String, usize>,
    noted_subgraph: bool,
}

/// A parsed link between two node groups.
struct Link {
    stroke: Stroke,
    start: Head,
    end: Head,
    label: Option<String>,
    length: usize,
}

impl Parser {
    /// Read the header line and return whatever follows the direction.
    fn header<'a>(&mut self, line: &'a str, number: usize) -> Result<&'a str, ParseError> {
        let keyword_end = line
            .find(|c: char| c.is_whitespace() || c == ';')
            .unwrap_or(line.len());
        let keyword = &line[..keyword_end];
        if !matches!(keyword, "graph" | "flowchart" | "flowchart-elk") {
            if let Some((_, kind)) = OTHER_KINDS.iter().find(|(k, _)| *k == keyword) {
                return Err(ParseError::Unsupported((*kind).to_string()));
            }
            return Err(ParseError::Syntax {
                line: number,
                message: format!(
                    "expected `graph` or `flowchart`, found `{}`",
                    printable(keyword, 40)
                ),
            });
        }
        let rest = line[keyword_end..].trim_start();
        let direction_end = rest
            .find(|c: char| c.is_whitespace() || c == ';')
            .unwrap_or(rest.len());
        let direction = match &rest[..direction_end] {
            "" => None,
            "TD" | "TB" => Some(Direction::TopDown),
            "BT" => Some(Direction::BottomUp),
            "LR" => Some(Direction::LeftRight),
            "RL" => Some(Direction::RightLeft),
            other => {
                return Err(ParseError::Syntax {
                    line: number,
                    message: format!(
                        "unknown direction `{}`: use TD, TB, BT, LR or RL",
                        printable(other, 20)
                    ),
                })
            }
        };
        self.chart.direction = direction.unwrap_or_default();
        Ok(if direction.is_some() {
            &rest[direction_end..]
        } else {
            rest
        })
    }

    fn statement(&mut self, statement: &str, number: usize) -> Result<(), ParseError> {
        if statement.is_empty() {
            return Ok(());
        }
        let first = statement.split_whitespace().next().unwrap_or("");
        match first {
            "subgraph" => {
                if !self.noted_subgraph {
                    self.noted_subgraph = true;
                    self.chart
                        .notes
                        .push("subgraphs are drawn without their frames".into());
                }
                return Ok(());
            }
            "end" | "classDef" | "class" | "style" | "linkStyle" | "click" | "direction"
            | "accTitle" | "accDescr" => return Ok(()),
            _ => {}
        }
        if first.starts_with("accTitle:") || first.starts_with("accDescr") {
            return Ok(());
        }
        let chars: Vec<char> = statement.chars().collect();
        let mut cursor = Cursor {
            chars: &chars,
            at: 0,
            line: number,
        };
        let mut previous = self.group(&mut cursor)?;
        loop {
            cursor.skip_space();
            if cursor.done() {
                return Ok(());
            }
            let Some(link) = cursor.link()? else {
                return Err(cursor.error(format!(
                    "expected an arrow or the end of the statement at `{}`",
                    cursor.rest(20)
                )));
            };
            cursor.skip_space();
            if cursor.done() {
                return Err(cursor.error("an arrow needs a node after it".into()));
            }
            let next = self.group(&mut cursor)?;
            for &from in &previous {
                for &to in &next {
                    if self.chart.edges.len() >= MAX_EDGES {
                        return Err(ParseError::TooLarge(format!("more than {MAX_EDGES} edges")));
                    }
                    self.chart.edges.push(Edge {
                        from,
                        to,
                        label: link.label.clone(),
                        stroke: link.stroke,
                        start: link.start,
                        end: link.end,
                        length: link.length,
                    });
                }
            }
            previous = next;
        }
    }

    /// `node ( & node )*`
    fn group(&mut self, cursor: &mut Cursor) -> Result<Vec<usize>, ParseError> {
        let mut nodes = vec![self.node(cursor)?];
        loop {
            let save = cursor.at;
            cursor.skip_space();
            if cursor.eat("&") {
                cursor.skip_space();
                nodes.push(self.node(cursor)?);
            } else {
                cursor.at = save;
                return Ok(nodes);
            }
        }
    }

    fn node(&mut self, cursor: &mut Cursor) -> Result<usize, ParseError> {
        cursor.skip_space();
        let id = cursor.id();
        if id.is_empty() {
            return Err(cursor.error(format!("expected a node at `{}`", cursor.rest(20))));
        }
        let shaped = cursor.shape()?;
        // `:::className` and `@{ … }` shape metadata are styling; skip them.
        if cursor.eat(":::") {
            cursor.id();
        }
        if cursor.eat("@{") {
            cursor.skip_past('}');
        }
        let index = match self.ids.get(&id) {
            Some(&index) => index,
            None => {
                if self.chart.nodes.len() >= MAX_NODES {
                    return Err(ParseError::TooLarge(format!("more than {MAX_NODES} nodes")));
                }
                self.chart.nodes.push(Node {
                    id: id.clone(),
                    label: clean_label(&id),
                    shape: Shape::Rect,
                });
                self.ids.insert(id, self.chart.nodes.len() - 1);
                self.chart.nodes.len() - 1
            }
        };
        if let Some((label, shape)) = shaped {
            let node = &mut self.chart.nodes[index];
            node.label = label;
            node.shape = shape;
        }
        Ok(index)
    }
}

struct Cursor<'a> {
    chars: &'a [char],
    at: usize,
    line: usize,
}

/// Shape brackets, longest opener first. `[/` and `[\` close two ways.
const SHAPES: &[(&str, &[(&str, Shape)])] = &[
    ("(((", &[(")))", Shape::DoubleCircle)]),
    ("([", &[("])", Shape::Stadium)]),
    ("[[", &[("]]", Shape::Subroutine)]),
    ("[(", &[(")]", Shape::Cylinder)]),
    ("((", &[("))", Shape::Circle)]),
    ("{{", &[("}}", Shape::Hexagon)]),
    (
        "[/",
        &[("/]", Shape::Parallelogram), ("\\]", Shape::Trapezoid)],
    ),
    (
        "[\\",
        &[
            ("\\]", Shape::ParallelogramAlt),
            ("/]", Shape::TrapezoidAlt),
        ],
    ),
    ("(", &[(")", Shape::Round)]),
    ("[", &[("]", Shape::Rect)]),
    ("{", &[("}", Shape::Rhombus)]),
    (">", &[("]", Shape::Asymmetric)]),
];

impl Cursor<'_> {
    fn done(&self) -> bool {
        self.at >= self.chars.len()
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.at + offset).copied()
    }

    fn starts_with(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(offset, c)| self.peek(offset) == Some(c))
    }

    fn eat(&mut self, text: &str) -> bool {
        if self.starts_with(text) {
            self.at += text.chars().count();
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        while self.peek(0).is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn skip_past(&mut self, end: char) {
        while let Some(c) = self.peek(0) {
            self.at += 1;
            if c == end {
                return;
            }
        }
    }

    fn rest(&self, limit: usize) -> String {
        let rest: String = self.chars[self.at.min(self.chars.len())..].iter().collect();
        printable(&rest, limit)
    }

    fn error(&self, message: String) -> ParseError {
        ParseError::Syntax {
            line: self.line,
            message,
        }
    }

    /// A node id: letters, digits and `_`, with `-` or `.` allowed between
    /// them (so `a-b` is one id, but `a-->b` is not).
    fn id(&mut self) -> String {
        let mut id = String::new();
        while let Some(c) = self.peek(0) {
            let joiner = matches!(c, '-' | '.')
                && !id.is_empty()
                && self
                    .peek(1)
                    .is_some_and(|next| next.is_alphanumeric() || next == '_');
            if c.is_alphanumeric() || c == '_' || joiner {
                id.push(c);
                self.at += 1;
            } else {
                break;
            }
        }
        id
    }

    /// The bracketed label after an id, if there is one.
    fn shape(&mut self) -> Result<Option<(String, Shape)>, ParseError> {
        for (open, closers) in SHAPES {
            if !self.starts_with(open) {
                continue;
            }
            self.at += open.chars().count();
            let text_start = self.at;
            // A quoted label may contain brackets.
            let mut quoted_end = None;
            let mut probe = self.at;
            while self.chars.get(probe).is_some_and(|c| c.is_whitespace()) {
                probe += 1;
            }
            if self.chars.get(probe) == Some(&'"') {
                if let Some(close) = self.chars[probe + 1..].iter().position(|&c| c == '"') {
                    quoted_end = Some(probe + 1 + close + 1);
                }
            }
            let search_from = quoted_end.unwrap_or(text_start);
            let mut best: Option<(usize, &str, Shape)> = None;
            for (close, shape) in closers.iter() {
                if let Some(found) = self.find(close, search_from) {
                    if best.is_none_or(|(at, _, _)| found < at) {
                        best = Some((found, close, *shape));
                    }
                }
            }
            let Some((found, close, shape)) = best else {
                return Err(self.error(format!("`{open}` is never closed")));
            };
            let text: String = self.chars[text_start..found].iter().collect();
            self.at = found + close.chars().count();
            return Ok(Some((clean_label(&text), shape)));
        }
        Ok(None)
    }

    fn find(&self, needle: &str, from: usize) -> Option<usize> {
        let needle: Vec<char> = needle.chars().collect();
        (from..self.chars.len()).find(|&at| self.chars[at..].starts_with(&needle))
    }

    /// A link such as `-->`, `-- text -->`, `-.->`, `==>|text|`, `<-->`,
    /// `--o` or `~~~`, or `None` (with the cursor unmoved) if there is none.
    fn link(&mut self) -> Result<Option<Link>, ParseError> {
        let save = self.at;
        if self.starts_with("~~~") {
            while self.peek(0) == Some('~') {
                self.at += 1;
            }
            return Ok(Some(Link {
                stroke: Stroke::Invisible,
                start: Head::None,
                end: Head::None,
                label: self.pipe_label()?,
                length: 1,
            }));
        }
        let start = match self.peek(0) {
            Some('<') => Head::Arrow,
            Some('o') if matches!(self.peek(1), Some('-' | '=')) => Head::Circle,
            Some('x') if matches!(self.peek(1), Some('-' | '=')) => Head::Cross,
            _ => Head::None,
        };
        if start != Head::None {
            self.at += 1;
        }
        let link = if self.starts_with("-.") {
            self.dotted()?
        } else if self.starts_with("--") {
            self.lined('-', Stroke::Solid)?
        } else if self.starts_with("==") {
            self.lined('=', Stroke::Thick)?
        } else {
            None
        };
        let Some((stroke, end, text, length)) = link else {
            self.at = save;
            return Ok(None);
        };
        let pipe = self.pipe_label()?;
        Ok(Some(Link {
            stroke,
            start,
            end,
            label: pipe.or(text),
            length,
        }))
    }

    /// The head after a run of `-`/`=`/`.`: `>`, `o` or `x`. As in Mermaid,
    /// `A--oB` is a circle head, so a node id after a link cannot start with
    /// `o` or `x` unless a space separates them.
    fn end_head(&mut self) -> Head {
        match self.peek(0) {
            Some('>') => {
                self.at += 1;
                Head::Arrow
            }
            Some(c @ ('o' | 'x')) => {
                self.at += 1;
                if c == 'o' {
                    Head::Circle
                } else {
                    Head::Cross
                }
            }
            _ => Head::None,
        }
    }

    /// `-->`, `---`, `--->`, `-- text -->` (and the `=` forms).
    #[allow(clippy::type_complexity)]
    fn lined(
        &mut self,
        line: char,
        stroke: Stroke,
    ) -> Result<Option<(Stroke, Head, Option<String>, usize)>, ParseError> {
        let mut run: usize = 0;
        while self.peek(0) == Some(line) {
            run += 1;
            self.at += 1;
        }
        let end = self.end_head();
        if end != Head::None {
            return Ok(Some((stroke, end, None, run.saturating_sub(1).max(1))));
        }
        if run >= 3 {
            return Ok(Some((stroke, Head::None, None, (run - 2).max(1))));
        }
        // `-- text -->`: text up to the next run of two or more.
        let pair: String = [line, line].iter().collect();
        let Some(close) = self.find(&pair, self.at) else {
            return Err(self.error(format!("`{pair}` starts a link that is never finished")));
        };
        let text: String = self.chars[self.at..close].iter().collect();
        self.at = close;
        let mut closing: usize = 0;
        while self.peek(0) == Some(line) {
            closing += 1;
            self.at += 1;
        }
        let end = self.end_head();
        let length = if end == Head::None {
            closing.saturating_sub(2).max(1)
        } else {
            closing.saturating_sub(1).max(1)
        };
        Ok(Some((stroke, end, Some(clean_label(&text)), length)))
    }

    /// `-.->`, `-.-`, `-..->`, `-. text .->`.
    #[allow(clippy::type_complexity)]
    fn dotted(&mut self) -> Result<Option<(Stroke, Head, Option<String>, usize)>, ParseError> {
        self.at += 1; // '-'
        let mut dots: usize = 0;
        while self.peek(0) == Some('.') {
            dots += 1;
            self.at += 1;
        }
        if self.peek(0) == Some('-') {
            self.at += 1;
            let end = self.end_head();
            return Ok(Some((Stroke::Dotted, end, None, dots.max(1))));
        }
        // `-. text .->`
        let Some(close) = self.find(".-", self.at) else {
            return Err(self.error("`-.` starts a link that is never finished".into()));
        };
        let text: String = self.chars[self.at..close].iter().collect();
        self.at = close + 2;
        let end = self.end_head();
        Ok(Some((Stroke::Dotted, end, Some(clean_label(&text)), 1)))
    }

    /// `|text|` after a link.
    fn pipe_label(&mut self) -> Result<Option<String>, ParseError> {
        let save = self.at;
        self.skip_space();
        if self.peek(0) != Some('|') {
            self.at = save;
            return Ok(None);
        }
        self.at += 1;
        let Some(close) = self.chars[self.at..].iter().position(|&c| c == '|') else {
            return Err(self.error("`|` starts a link label that is never closed".into()));
        };
        let text: String = self.chars[self.at..self.at + close].iter().collect();
        self.at += close + 1;
        Ok(Some(clean_label(&text)))
    }
}

/// Tidy label text: trim, drop surrounding quotes and Markdown-string
/// backticks, turn `<br>` into a line break, decode `#quot;`-style entities,
/// and remove control characters so a label cannot reach the terminal as an
/// escape sequence.
pub fn clean_label(text: &str) -> String {
    let mut text = text.trim();
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text = &text[1..text.len() - 1];
    }
    if text.len() >= 2 && text.starts_with('`') && text.ends_with('`') {
        text = &text[1..text.len() - 1];
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            let lower = rest.get(..6).unwrap_or(rest).to_ascii_lowercase();
            let brk = ["<br>", "<br/>", "<br />"]
                .iter()
                .find(|tag| lower.starts_with(**tag));
            if let Some(tag) = brk {
                out.push('\n');
                rest = &rest[tag.len()..];
                continue;
            }
        }
        if c == '#' {
            if let Some(end) = rest[1..].find(';').filter(|&end| end <= 8) {
                let name = &rest[1..1 + end];
                let decoded = match name {
                    "quot" => Some('"'),
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "nbsp" => Some(' '),
                    _ => name.parse::<u32>().ok().and_then(char::from_u32),
                };
                if let Some(decoded) = decoded {
                    out.push(decoded);
                    rest = &rest[end + 2..];
                    continue;
                }
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out.lines()
        .map(|line| {
            line.chars()
                .map(|c| if c == '\t' { ' ' } else { c })
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Source text quoted in an error: control characters removed, shortened.
fn printable(text: &str, limit: usize) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() > limit {
        let short: String = clean.chars().take(limit).collect();
        format!("{short}…")
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(chart: &Flowchart) -> Vec<(&str, &str, Shape)> {
        chart
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str(), n.shape))
            .collect()
    }

    #[test]
    fn header_and_directions() {
        for (header, direction) in [
            ("graph TD", Direction::TopDown),
            ("flowchart TB", Direction::TopDown),
            ("graph BT", Direction::BottomUp),
            ("flowchart LR", Direction::LeftRight),
            ("graph RL", Direction::RightLeft),
            ("graph", Direction::TopDown),
        ] {
            let chart = parse(&format!("{header}\n  A --> B")).unwrap();
            assert_eq!(chart.direction, direction, "{header}");
            assert_eq!(chart.edges.len(), 1);
        }
        assert_eq!(parse("graph TD; A-->B").unwrap().edges.len(), 1);
        assert!(matches!(
            parse("graph XY\nA-->B"),
            Err(ParseError::Syntax { .. })
        ));
        assert_eq!(parse("\n%% nothing\n"), Err(ParseError::Empty));
    }

    #[test]
    fn other_diagram_kinds_are_named() {
        assert_eq!(
            parse("sequenceDiagram\n  A->>B: hi")
                .unwrap_err()
                .to_string(),
            "sequence diagrams are not drawn as text"
        );
        assert_eq!(
            parse("---\ntitle: x\n---\nstateDiagram-v2\n[*] --> A"),
            Err(ParseError::Unsupported("state".into()))
        );
        assert!(matches!(
            parse("hello"),
            Err(ParseError::Syntax { line: 1, .. })
        ));
    }

    #[test]
    fn shapes() {
        let chart = parse(
            "graph TD\n a[rect] --> b(round) --> c([stadium]) --> d[[sub]] --> e[(db)]\n \
             f((circle)) --> g(((double))) --> h>flag] --> i{decide} --> j{{hex}}\n \
             k[/in/] --> l[\\out\\] --> m[/top\\] --> n[\\bottom/]",
        )
        .unwrap();
        use Shape::*;
        let shapes: Vec<Shape> = chart.nodes.iter().map(|n| n.shape).collect();
        assert_eq!(
            shapes,
            [
                Rect,
                Round,
                Stadium,
                Subroutine,
                Cylinder,
                Circle,
                DoubleCircle,
                Asymmetric,
                Rhombus,
                Hexagon,
                Parallelogram,
                ParallelogramAlt,
                Trapezoid,
                TrapezoidAlt
            ]
        );
        assert_eq!(chart.nodes[8].label, "decide");
    }

    #[test]
    fn links_and_labels() {
        let chart = parse(
            "graph LR\nA-->B\nA---C\nA-.->D\nA==>E\nA-- yes -->F\nA-->|no|G\nA<-->H\nA--oI\nA--xJ\nA~~~K\nA--->L\nA-. maybe .->M\nA== sure ==>N",
        )
        .unwrap();
        let summary: Vec<(Stroke, Head, Head, Option<&str>, usize)> = chart
            .edges
            .iter()
            .map(|e| (e.stroke, e.start, e.end, e.label.as_deref(), e.length))
            .collect();
        use Head::*;
        use Stroke::*;
        assert_eq!(
            summary,
            [
                (Solid, None, Arrow, Option::None, 1),
                (Solid, None, Head::None, Option::None, 1),
                (Dotted, None, Arrow, Option::None, 1),
                (Thick, None, Arrow, Option::None, 1),
                (Solid, None, Arrow, Some("yes"), 1),
                (Solid, None, Arrow, Some("no"), 1),
                (Solid, Arrow, Arrow, Option::None, 1),
                (Solid, None, Circle, Option::None, 1),
                (Solid, None, Cross, Option::None, 1),
                (Invisible, None, Head::None, Option::None, 1),
                (Solid, None, Arrow, Option::None, 2),
                (Dotted, None, Arrow, Some("maybe"), 1),
                (Thick, None, Arrow, Some("sure"), 1),
            ]
        );
    }

    #[test]
    fn chains_ampersands_and_redefinition() {
        let chart = parse("graph TD\nA & B --> C & D --> E\nA[Start here]").unwrap();
        assert_eq!(chart.edges.len(), 6);
        assert_eq!(labels(&chart)[0], ("A", "Start here", Shape::Rect));
        assert_eq!(chart.nodes.len(), 5);
    }

    #[test]
    fn labels_are_cleaned() {
        let chart =
            parse("graph TD\nA[\"a [quoted] #quot;label#quot;\"] --> B[one<br>two]\nC[\"x\u{1b}]0;evil\u{7}\"]")
                .unwrap();
        assert_eq!(chart.nodes[0].label, "a [quoted] \"label\"");
        assert_eq!(chart.nodes[1].label, "one\ntwo");
        assert!(!chart.nodes[2].label.contains('\u{1b}'));
    }

    #[test]
    fn styling_and_subgraphs_are_accepted() {
        let chart = parse(
            "graph TD\nclassDef hot fill:#f00\nsubgraph one [First]\n  A:::hot --> B\nend\nstyle A fill:#fff\nclick A callback\nlinkStyle 0 stroke:#f00",
        )
        .unwrap();
        assert_eq!(chart.edges.len(), 1);
        assert_eq!(chart.notes, ["subgraphs are drawn without their frames"]);
    }

    #[test]
    fn errors_name_the_line() {
        let error = parse("graph TD\nA --> B\nA -->").unwrap_err();
        assert_eq!(
            error,
            ParseError::Syntax {
                line: 3,
                message: "an arrow needs a node after it".into()
            }
        );
        assert!(parse("graph TD\nA[never closed").is_err());
        assert!(parse("graph TD\nA ?? B").is_err());
    }

    #[test]
    fn limits() {
        let big = format!("graph TD\n{}", "A-->B\n".repeat(MAX_SOURCE / 6 + 1));
        assert!(matches!(parse(&big), Err(ParseError::TooLarge(_))));
        let many: String = (0..=MAX_NODES).map(|i| format!("n{i}\n")).collect();
        assert!(matches!(
            parse(&format!("graph TD\n{many}")),
            Err(ParseError::TooLarge(_))
        ));
    }
}
