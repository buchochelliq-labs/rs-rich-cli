//! DOT (Graphviz) sources, parsed natively into a [`Graph`] (#240).
//!
//! [`parse`] reads the subset of DOT people write by hand:
//!
//! - `graph` and `digraph`, optionally `strict` (repeated edges merge), with
//!   an optional name;
//! - node statements (`a [label="A", shape=box]`) and edge statements,
//!   chains included (`a -> b -> c`), with `{ … }` groups as endpoints
//!   (`a -> { b c }`);
//! - attribute lists for `label`, `shape` and `style` (plus `dir`,
//!   `arrowhead`, `arrowtail` and `minlen` on edges, and `rankdir` and
//!   `label` on the graph), and `node [ … ]`, `edge [ … ]` and `graph [ … ]`
//!   defaults, scoped to the subgraph they are set in;
//! - subgraphs; a subgraph named `cluster…` is a [`Cluster`];
//! - quoted IDs (with `+` concatenation), numerals, and `//`, `/* */` and
//!   `#` comments.
//!
//! Attributes that only change how Graphviz paints (`color`, `fontname`,
//! `penwidth`, …) are accepted and ignored. What the native drawing cannot
//! represent is refused with a [`DotError`] naming the construct and its
//! line, never drawn partially: node ports (`a:p`), HTML-like labels
//! (`<…>`), `record` shapes, and more than one graph in a file.
//!
//! [`Dot`] renders a source: the drawing, or the error and the source.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use rich::console::{Console, ConsoleOptions};
use rich::measure::Measurement;
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;
use rich::text::Text;

use crate::graph::{Direction, Edge, Graph, Head, Node, Shape, Stroke};
use crate::layout::{draw, DrawError, Drawing};

/// Why a DOT source was not read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DotError {
    /// The 1-based line the problem is on.
    pub line: usize,
    /// For a construct this parser does not support, its name (`"a node
    /// port"`, `"an HTML-like label"`, …); `None` for a syntax error.
    pub construct: Option<String>,
    message: String,
}

impl DotError {
    fn syntax(line: usize, message: impl Into<String>) -> Self {
        DotError {
            line,
            construct: None,
            message: message.into(),
        }
    }

    fn unsupported(line: usize, construct: impl Into<String>, hint: &str) -> Self {
        let construct = construct.into();
        let mut message = format!("{construct} is not supported");
        if !hint.is_empty() {
            message.push_str(" (");
            message.push_str(hint);
            message.push(')');
        }
        DotError {
            line,
            construct: Some(construct),
            message,
        }
    }

    /// What is wrong, without the line.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for DotError {}

/// A subgraph named `cluster…`: a group of nodes Graphviz frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cluster {
    /// The subgraph's name, `cluster` prefix included.
    pub id: String,
    /// Its `label`, if it has one.
    pub label: Option<String>,
    /// Its nodes (nested clusters' included), by index into
    /// [`Graph::nodes`], in the order they were first mentioned.
    pub nodes: Vec<usize>,
}

/// A parsed DOT source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DotGraph {
    /// The nodes and edges, ready to draw.
    pub graph: Graph,
    /// `digraph` (edges have arrows) rather than `graph`.
    pub directed: bool,
    /// `strict`: repeated edges were merged into one.
    pub strict: bool,
    /// The graph's name, if it has one.
    pub name: Option<String>,
    /// The graph's `label`, if it has one.
    pub label: Option<String>,
    /// The `cluster…` subgraphs, outermost first.
    pub clusters: Vec<Cluster>,
    /// What was accepted but is not drawn (cluster frames, `rank`
    /// constraints), one sentence each.
    pub notes: Vec<String>,
}

// ------------------------------------------------------------------ lexer

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    /// An ID; `true` when it was quoted (so never a keyword).
    Id(String, bool),
    Html,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Eq,
    Semi,
    Comma,
    Colon,
    /// `->`
    Arrow,
    /// `--`
    Line,
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
    /// At the start of a line, before anything but whitespace: where a `#`
    /// starts a (preprocessor output) comment line.
    line_start: bool,
}

fn id_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn id_char(c: char) -> bool {
    id_start(c) || c.is_ascii_digit()
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Lexer {
            chars: source.chars().peekable(),
            line: 1,
            line_start: true,
        }
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        if c == '\n' {
            self.line += 1;
            self.line_start = true;
        }
        Some(c)
    }

    /// Skip whitespace and comments.
    fn skip(&mut self) -> Result<(), DotError> {
        loop {
            match self.chars.peek().copied() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('#') if self.line_start => {
                    while self.chars.peek().is_some_and(|&c| c != '\n') {
                        self.bump();
                    }
                }
                Some('/') => {
                    let mut ahead = self.chars.clone();
                    ahead.next();
                    match ahead.next() {
                        Some('/') => {
                            while self.chars.peek().is_some_and(|&c| c != '\n') {
                                self.bump();
                            }
                        }
                        Some('*') => {
                            let start = self.line;
                            self.bump();
                            self.bump();
                            let mut star = false;
                            loop {
                                match self.bump() {
                                    Some('/') if star => break,
                                    Some(c) => star = c == '*',
                                    None => {
                                        return Err(DotError::syntax(
                                            start,
                                            "a /* comment is never closed",
                                        ))
                                    }
                                }
                            }
                        }
                        _ => return Ok(()),
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// The next token and the line it starts on.
    fn next(&mut self) -> Result<Option<(Tok, usize)>, DotError> {
        self.skip()?;
        let line = self.line;
        let Some(c) = self.chars.peek().copied() else {
            return Ok(None);
        };
        self.line_start = false;
        let single = |tok: Tok| Some((tok, line));
        let tok = match c {
            '{' => single(Tok::LBrace),
            '}' => single(Tok::RBrace),
            '[' => single(Tok::LBracket),
            ']' => single(Tok::RBracket),
            '=' => single(Tok::Eq),
            ';' => single(Tok::Semi),
            ',' => single(Tok::Comma),
            ':' => single(Tok::Colon),
            _ => None,
        };
        if tok.is_some() {
            self.bump();
            return Ok(tok);
        }
        match c {
            '"' => {
                self.bump();
                let mut text = self.quoted(line)?;
                // `"a" + "b"` concatenates.
                loop {
                    self.skip()?;
                    if self.chars.peek() != Some(&'+') {
                        break;
                    }
                    self.bump();
                    self.skip()?;
                    if self.chars.peek() != Some(&'"') {
                        return Err(DotError::syntax(
                            self.line,
                            "`+` must join two quoted strings",
                        ));
                    }
                    self.bump();
                    text.push_str(&self.quoted(line)?);
                }
                Ok(Some((Tok::Id(text, true), line)))
            }
            '<' => {
                // Skip the whole HTML string, so the error names it.
                let mut depth = 0usize;
                loop {
                    match self.bump() {
                        Some('<') => depth += 1,
                        Some('>') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Some(_) => {}
                        None => {
                            return Err(DotError::syntax(line, "an HTML string is never closed"))
                        }
                    }
                }
                Ok(Some((Tok::Html, line)))
            }
            '-' => {
                let mut ahead = self.chars.clone();
                ahead.next();
                match ahead.next() {
                    Some('>') => {
                        self.bump();
                        self.bump();
                        Ok(Some((Tok::Arrow, line)))
                    }
                    Some('-') => {
                        self.bump();
                        self.bump();
                        Ok(Some((Tok::Line, line)))
                    }
                    Some(d) if d.is_ascii_digit() || d == '.' => Ok(Some((self.numeral(), line))),
                    _ => Err(DotError::syntax(line, "unexpected `-`")),
                }
            }
            c if c.is_ascii_digit() || c == '.' => Ok(Some((self.numeral(), line))),
            c if id_start(c) => {
                let mut text = String::new();
                while let Some(&c) = self.chars.peek() {
                    if !id_char(c) {
                        break;
                    }
                    text.push(c);
                    self.bump();
                }
                Ok(Some((Tok::Id(text, false), line)))
            }
            other => Err(DotError::syntax(line, format!("unexpected {other:?}"))),
        }
    }

    /// The rest of a quoted string, after its opening `"`. `\"` is a quote
    /// and a backslash before a line break joins the lines; other escapes
    /// are kept for [`label_text`].
    fn quoted(&mut self, line: usize) -> Result<String, DotError> {
        let mut text = String::new();
        loop {
            match self.bump() {
                Some('"') => return Ok(text),
                Some('\\') => match self.chars.peek().copied() {
                    Some('"') => {
                        self.bump();
                        text.push('"');
                    }
                    Some('\n') => {
                        self.bump();
                    }
                    Some('\r') => {
                        self.bump();
                        if self.chars.peek() == Some(&'\n') {
                            self.bump();
                        }
                    }
                    _ => text.push('\\'),
                },
                Some(c) => text.push(c),
                None => return Err(DotError::syntax(line, "a quoted string is never closed")),
            }
        }
    }

    fn numeral(&mut self) -> Tok {
        let mut text = String::new();
        if self.chars.peek() == Some(&'-') {
            text.push('-');
            self.bump();
        }
        while let Some(&c) = self.chars.peek() {
            if !(c.is_ascii_digit() || c == '.') {
                break;
            }
            text.push(c);
            self.bump();
        }
        Tok::Id(text, false)
    }
}

// ----------------------------------------------------------------- parser

type Attrs = Vec<(String, String)>;

fn set(attrs: &mut Attrs, key: &str, value: &str) {
    match attrs.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value.to_string(),
        None => attrs.push((key.to_string(), value.to_string())),
    }
}

fn get<'a>(attrs: &'a Attrs, key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// The defaults in force in one scope (the graph, or a subgraph).
#[derive(Clone, Default)]
struct Scope {
    node: Attrs,
    edge: Attrs,
    /// Graph attributes set in this scope (for a cluster's `label`).
    graph: Attrs,
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    peeked: Option<Option<(Tok, usize)>>,
    /// The line of the last token taken, for errors at the end of input.
    line: usize,
    directed: bool,
    strict: bool,
    ids: HashMap<String, usize>,
    nodes: Vec<(String, Attrs)>,
    edges: Vec<(usize, usize, Attrs)>,
    scopes: Vec<Scope>,
    clusters: Vec<Cluster>,
    rank_noted: bool,
    /// Subgraphs open around the current statement, deepest last: the
    /// clusters' indexes in `clusters` (`None` for other subgraphs).
    open: Vec<Option<usize>>,
    anonymous: usize,
}

impl<'a> Parser<'a> {
    fn peek(&mut self) -> Result<Option<&Tok>, DotError> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lexer.next()?);
        }
        Ok(self
            .peeked
            .as_ref()
            .and_then(|t| t.as_ref())
            .map(|(t, _)| t))
    }

    fn take(&mut self) -> Result<Option<Tok>, DotError> {
        let next = match self.peeked.take() {
            Some(next) => next,
            None => self.lexer.next()?,
        };
        Ok(next.map(|(tok, line)| {
            self.line = line;
            tok
        }))
    }

    /// The line of the next token (or of the last one, at the end).
    fn here(&mut self) -> Result<usize, DotError> {
        self.peek()?;
        Ok(self
            .peeked
            .as_ref()
            .and_then(|t| t.as_ref())
            .map_or(self.line, |(_, line)| *line))
    }

    fn eat(&mut self, tok: &Tok) -> Result<bool, DotError> {
        if self.peek()? == Some(tok) {
            self.take()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn expect(&mut self, tok: Tok, what: &str) -> Result<(), DotError> {
        let line = self.here()?;
        match self.take()? {
            Some(found) if found == tok => Ok(()),
            Some(found) => Err(DotError::syntax(
                line,
                format!("expected {what}, found {}", describe(&found)),
            )),
            None => Err(DotError::syntax(
                line,
                format!("expected {what}, found the end of the file"),
            )),
        }
    }

    fn keyword(&mut self, word: &str) -> Result<bool, DotError> {
        let matches =
            matches!(self.peek()?, Some(Tok::Id(text, false)) if text.eq_ignore_ascii_case(word));
        if matches {
            self.take()?;
        }
        Ok(matches)
    }

    /// An ID (not a keyword), for a node, an attribute name or value.
    fn id(&mut self, what: &str) -> Result<String, DotError> {
        let line = self.here()?;
        match self.take()? {
            Some(Tok::Id(text, quoted)) if quoted || !is_keyword(&text) => Ok(text),
            Some(Tok::Html) => Err(DotError::unsupported(
                line,
                "an HTML-like label",
                "use a quoted string",
            )),
            Some(found) => Err(DotError::syntax(
                line,
                format!("expected {what}, found {}", describe(&found)),
            )),
            None => Err(DotError::syntax(
                line,
                format!("expected {what}, found the end of the file"),
            )),
        }
    }

    fn graph(&mut self) -> Result<(Option<String>, Attrs), DotError> {
        self.strict = self.keyword("strict")?;
        if self.keyword("digraph")? {
            self.directed = true;
        } else if !self.keyword("graph")? {
            let line = self.here()?;
            return Err(DotError::syntax(
                line,
                "a DOT file starts with `graph` or `digraph`",
            ));
        }
        let name = match self.peek()? {
            Some(Tok::Id(..)) | Some(Tok::Html) => Some(self.id("the graph's name")?),
            _ => None,
        };
        self.expect(Tok::LBrace, "`{`")?;
        self.scopes.push(Scope::default());
        self.statements()?;
        let scope = self.scopes.pop().unwrap_or_default();
        let line = self.here()?;
        if self.take()?.is_some() {
            return Err(DotError::unsupported(
                line,
                "more than one graph in a file",
                "split them into files of their own",
            ));
        }
        Ok((name, scope.graph))
    }

    /// Statements up to and including the closing `}`.
    fn statements(&mut self) -> Result<Vec<usize>, DotError> {
        let mut members = Vec::new();
        loop {
            let line = self.here()?;
            match self.peek()? {
                None => return Err(DotError::syntax(line, "a `{` is never closed")),
                Some(Tok::RBrace) => {
                    self.take()?;
                    return Ok(members);
                }
                Some(Tok::Semi) => {
                    self.take()?;
                }
                _ => self.statement(&mut members)?,
            }
        }
    }

    fn statement(&mut self, members: &mut Vec<usize>) -> Result<(), DotError> {
        let line = self.here()?;
        for (word, which) in [("node", 0), ("edge", 1), ("graph", 2)] {
            if self.keyword(word)? {
                let attrs = self.attr_lists(true)?;
                if which == 2 {
                    for (key, value) in &attrs {
                        self.graph_attr(key, value);
                    }
                    return Ok(());
                }
                let scope = self.scopes.last_mut().expect("a scope is open");
                let target = if which == 0 {
                    &mut scope.node
                } else {
                    &mut scope.edge
                };
                for (key, value) in &attrs {
                    set(target, key, value);
                }
                return Ok(());
            }
        }
        if matches!(self.peek()?, Some(Tok::Id(text, false)) if text.eq_ignore_ascii_case("strict") || text.eq_ignore_ascii_case("digraph"))
        {
            return Err(DotError::unsupported(
                line,
                "a graph inside a graph",
                "use `subgraph`",
            ));
        }
        // `key = value` sets a graph attribute.
        if matches!(self.peek()?, Some(Tok::Id(..))) {
            let mut ahead = self.lexer.chars.clone();
            // The token after the peeked one: only `=` matters.
            while ahead.peek().is_some_and(|c| c.is_whitespace()) {
                ahead.next();
            }
            if ahead.peek() == Some(&'=') {
                let key = self.id("an attribute name")?;
                self.expect(Tok::Eq, "`=`")?;
                let value = self.id("an attribute value")?;
                self.graph_attr(&key, &value);
                return Ok(());
            }
        }
        let first = self.endpoint()?;
        let mut groups = vec![first];
        let mut lines = vec![line];
        loop {
            let op_line = self.here()?;
            let directed = match self.peek()? {
                Some(Tok::Arrow) => true,
                Some(Tok::Line) => false,
                _ => break,
            };
            self.take()?;
            if directed != self.directed {
                let (op, kind) = if directed {
                    ("->", "an undirected `graph`; use `--`")
                } else {
                    ("--", "a `digraph`; use `->`")
                };
                return Err(DotError::syntax(op_line, format!("`{op}` in {kind}")));
            }
            groups.push(self.endpoint()?);
            lines.push(op_line);
        }
        let attrs = self.attr_lists(false)?;
        for group in &groups {
            for &node in group {
                if !members.contains(&node) {
                    members.push(node);
                }
                self.note_member(node);
            }
        }
        if groups.len() == 1 {
            // A node statement (or a bare subgraph): its attributes apply to
            // each node.
            if !attrs.is_empty() {
                for &node in &groups[0] {
                    for (key, value) in &attrs {
                        self.node_attr(node, key, value, line)?;
                    }
                }
            }
            return Ok(());
        }
        let mut edge_attrs = self
            .scopes
            .last()
            .map(|scope| scope.edge.clone())
            .unwrap_or_default();
        for (key, value) in &attrs {
            set(&mut edge_attrs, key, value);
        }
        for pair in groups.windows(2) {
            for &from in &pair[0] {
                for &to in &pair[1] {
                    self.add_edge(from, to, &edge_attrs);
                }
            }
        }
        Ok(())
    }

    fn add_edge(&mut self, from: usize, to: usize, attrs: &Attrs) {
        if self.strict {
            let directed = self.directed;
            if let Some((_, _, existing)) = self
                .edges
                .iter_mut()
                .find(|(a, b, _)| (*a == from && *b == to) || (!directed && *a == to && *b == from))
            {
                for (key, value) in attrs {
                    set(existing, key, value);
                }
                return;
            }
        }
        self.edges.push((from, to, attrs.clone()));
    }

    /// One end of an edge: a node, or a subgraph's nodes.
    fn endpoint(&mut self) -> Result<Vec<usize>, DotError> {
        let line = self.here()?;
        if matches!(self.peek()?, Some(Tok::LBrace))
            || matches!(self.peek()?, Some(Tok::Id(text, false)) if text.eq_ignore_ascii_case("subgraph"))
        {
            return self.subgraph();
        }
        let id = self.id("a node, `subgraph` or `{`")?;
        if self.peek()? == Some(&Tok::Colon) {
            return Err(DotError::unsupported(
                line,
                format!("a node port (`{id}:…`)"),
                "connect the node itself",
            ));
        }
        Ok(vec![self.node(&id)])
    }

    fn subgraph(&mut self) -> Result<Vec<usize>, DotError> {
        let line = self.here()?;
        let name = if self.keyword("subgraph")? {
            match self.peek()? {
                Some(Tok::Id(..)) | Some(Tok::Html) => Some(self.id("the subgraph's name")?),
                _ => None,
            }
        } else {
            None
        };
        let name = name.unwrap_or_else(|| {
            self.anonymous += 1;
            format!("%{}", self.anonymous)
        });
        let cluster = name
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cluster"));
        let index = if cluster {
            match self.clusters.iter().position(|c| c.id == name) {
                Some(index) => Some(index),
                None => {
                    self.clusters.push(Cluster {
                        id: name.clone(),
                        label: None,
                        nodes: Vec::new(),
                    });
                    Some(self.clusters.len() - 1)
                }
            }
        } else {
            None
        };
        if self.peek()? != Some(&Tok::LBrace) {
            // `subgraph name` alone refers to one defined earlier.
            return Err(DotError::unsupported(
                line,
                format!("a reference to subgraph `{name}` without a body"),
                "repeat its nodes",
            ));
        }
        self.take()?;
        let inherited = self.scopes.last().cloned().unwrap_or_default();
        self.scopes.push(Scope {
            graph: Attrs::new(),
            ..inherited
        });
        self.open.push(index);
        let members = self.statements();
        self.open.pop();
        let scope = self.scopes.pop().unwrap_or_default();
        let members = members?;
        if let Some(index) = index {
            if let Some(label) = get(&scope.graph, "label") {
                self.clusters[index].label = Some(label_text(label, &name, ""));
            }
        }
        Ok(members)
    }

    /// Record `node` as a member of every cluster open around it.
    fn note_member(&mut self, node: usize) {
        for index in self.open.iter().flatten() {
            let cluster = &mut self.clusters[*index];
            if !cluster.nodes.contains(&node) {
                cluster.nodes.push(node);
            }
        }
    }

    /// The index of node `id`, adding it with the defaults in force.
    fn node(&mut self, id: &str) -> usize {
        if let Some(&index) = self.ids.get(id) {
            return index;
        }
        let defaults = self
            .scopes
            .last()
            .map(|scope| scope.node.clone())
            .unwrap_or_default();
        self.ids.insert(id.to_string(), self.nodes.len());
        self.nodes.push((id.to_string(), defaults));
        self.nodes.len() - 1
    }

    fn node_attr(
        &mut self,
        node: usize,
        key: &str,
        value: &str,
        line: usize,
    ) -> Result<(), DotError> {
        check_attr(key, value, line)?;
        set(&mut self.nodes[node].1, key, value);
        Ok(())
    }

    fn graph_attr(&mut self, key: &str, value: &str) {
        if key == "rank" {
            self.rank_noted = true;
        }
        if let Some(scope) = self.scopes.last_mut() {
            set(&mut scope.graph, key, value);
        }
    }

    /// `[a=b, c=d] [e=f]…`: none, one or several lists. `required` for the
    /// `node`/`edge`/`graph` statements, which must have one.
    fn attr_lists(&mut self, required: bool) -> Result<Attrs, DotError> {
        let mut attrs = Attrs::new();
        if required && self.peek()? != Some(&Tok::LBracket) {
            let line = self.here()?;
            return Err(DotError::syntax(line, "expected `[` and attributes"));
        }
        while self.eat(&Tok::LBracket)? {
            loop {
                if self.eat(&Tok::RBracket)? {
                    break;
                }
                let line = self.here()?;
                let key = self.id("an attribute name")?;
                let value = if self.eat(&Tok::Eq)? {
                    self.id("an attribute value")?
                } else {
                    // Graphviz reads `[key]` as `key=true`.
                    "true".to_string()
                };
                check_attr(&key, &value, line)?;
                set(&mut attrs, &key, &value);
                if !self.eat(&Tok::Comma)? {
                    self.eat(&Tok::Semi)?;
                }
            }
        }
        Ok(attrs)
    }
}

fn is_keyword(text: &str) -> bool {
    ["node", "edge", "graph", "digraph", "subgraph", "strict"]
        .iter()
        .any(|k| text.eq_ignore_ascii_case(k))
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Id(text, true) => format!("{text:?}"),
        Tok::Id(text, false) => format!("`{text}`"),
        Tok::Html => "an HTML string".into(),
        Tok::LBrace => "`{`".into(),
        Tok::RBrace => "`}`".into(),
        Tok::LBracket => "`[`".into(),
        Tok::RBracket => "`]`".into(),
        Tok::Eq => "`=`".into(),
        Tok::Semi => "`;`".into(),
        Tok::Comma => "`,`".into(),
        Tok::Colon => "`:`".into(),
        Tok::Arrow => "`->`".into(),
        Tok::Line => "`--`".into(),
    }
}

/// Refuse attribute values the drawing cannot show.
fn check_attr(key: &str, value: &str, line: usize) -> Result<(), DotError> {
    if key == "shape" && (value.eq_ignore_ascii_case("record") || value == "Mrecord") {
        return Err(DotError::unsupported(
            line,
            format!("the `{value}` shape"),
            "use a box with a multi-line label",
        ));
    }
    Ok(())
}

/// A label's text: `\n`, `\l` and `\r` break lines (a trailing one is
/// dropped), `\N` is the node's name and `\G` the graph's, and any other
/// escaped character stands for itself.
fn label_text(raw: &str, node: &str, graph: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'l' | 'r') => out.push('\n'),
            Some('N') => out.push_str(node),
            Some('G') => out.push_str(graph),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// A Graphviz shape as the nearest [`Shape`]. Unknown shapes draw as boxes,
/// as Graphviz draws them.
fn shape(name: &str, style: &str) -> Shape {
    let rounded = style.split(',').any(|s| s.trim() == "rounded");
    match name.to_ascii_lowercase().as_str() {
        "ellipse" | "oval" | "egg" => Shape::Round,
        "circle" | "point" => Shape::Circle,
        "doublecircle" | "doubleoctagon" | "tripleoctagon" => Shape::DoubleCircle,
        "diamond" | "mdiamond" => Shape::Rhombus,
        "hexagon" | "octagon" | "septagon" => Shape::Hexagon,
        "parallelogram" => Shape::Parallelogram,
        "trapezium" => Shape::Trapezoid,
        "invtrapezium" => Shape::TrapezoidAlt,
        "cylinder" => Shape::Cylinder,
        "cds" | "rarrow" | "larrow" | "rpromoter" | "lpromoter" => Shape::Asymmetric,
        "component" | "box3d" | "tab" | "folder" => Shape::Subroutine,
        _ if rounded => Shape::Round,
        _ => Shape::Rect,
    }
}

fn arrow(name: &str) -> Head {
    let name = name.trim_start_matches('o').to_ascii_lowercase();
    match name.as_str() {
        "none" => Head::None,
        "dot" | "odot" => Head::Circle,
        "tee" | "box" | "obox" | "crow" | "diamond" | "odiamond" => Head::Cross,
        _ => Head::Arrow,
    }
}

/// Parse a DOT source. See the [module docs](self) for the subset.
///
/// ```
/// use rich_diagram::dot;
///
/// let parsed = dot::parse("digraph { rankdir=LR; a -> b -> c [label=\"go\"] }").unwrap();
/// assert_eq!(parsed.graph.nodes().len(), 3);
/// assert_eq!(parsed.graph.edges()[1].label.as_deref(), Some("go"));
///
/// let error = dot::parse("digraph {\n  a:out -> b\n}").unwrap_err();
/// assert_eq!(error.to_string(), "line 2: a node port (`a:…`) is not supported (connect the node itself)");
/// ```
pub fn parse(source: &str) -> Result<DotGraph, DotError> {
    let mut parser = Parser {
        lexer: Lexer::new(source),
        peeked: None,
        line: 1,
        directed: false,
        strict: false,
        ids: HashMap::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        scopes: Vec::new(),
        clusters: Vec::new(),
        rank_noted: false,
        open: Vec::new(),
        anonymous: 0,
    };
    let (name, graph_attrs) = parser.graph()?;
    let graph_name = name.clone().unwrap_or_default();
    let direction = match get(&graph_attrs, "rankdir")
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        Some("LR") => Direction::LeftRight,
        Some("RL") => Direction::RightLeft,
        Some("BT") => Direction::BottomUp,
        _ => Direction::TopDown,
    };
    let nodes = parser
        .nodes
        .iter()
        .map(|(id, attrs)| {
            let label = match get(attrs, "label") {
                Some(raw) => label_text(raw, id, &graph_name),
                None => id.clone(),
            };
            let style = get(attrs, "style").unwrap_or("");
            let shape = shape(get(attrs, "shape").unwrap_or("ellipse"), style);
            let label = if get(attrs, "shape").is_some_and(|s| s.eq_ignore_ascii_case("point")) {
                String::new()
            } else {
                label
            };
            Node::new(id.clone(), label).shape(shape)
        })
        .collect();
    let directed = parser.directed;
    let edges = parser
        .edges
        .iter()
        .map(|(from, to, attrs)| {
            let mut edge = Edge::new(*from, *to);
            edge.label = get(attrs, "label")
                .map(|raw| label_text(raw, "", &graph_name))
                .filter(|label| !label.is_empty());
            let style = get(attrs, "style").unwrap_or("");
            edge.stroke = style
                .split(',')
                .map(str::trim)
                .fold(Stroke::Solid, |stroke, part| match part {
                    "dashed" | "dotted" => Stroke::Dotted,
                    "bold" => Stroke::Thick,
                    "invis" | "invisible" => Stroke::Invisible,
                    _ => stroke,
                });
            let default_dir = if directed { "forward" } else { "none" };
            let head = arrow(get(attrs, "arrowhead").unwrap_or("normal"));
            let tail = arrow(get(attrs, "arrowtail").unwrap_or("normal"));
            (edge.start, edge.end) = match get(attrs, "dir").unwrap_or(default_dir) {
                "back" => (tail, Head::None),
                "both" => (tail, head),
                "none" => (Head::None, Head::None),
                _ => (Head::None, head),
            };
            if let Some(length) = get(attrs, "minlen").and_then(|v| v.parse::<f64>().ok()) {
                edge.length = (length.max(1.0) as usize).min(crate::MAX_EDGE_LENGTH);
            }
            edge
        })
        .collect();
    let mut notes = Vec::new();
    if !parser.clusters.is_empty() {
        let names: Vec<String> = parser
            .clusters
            .iter()
            .map(|cluster| {
                let members: Vec<&str> = cluster
                    .nodes
                    .iter()
                    .map(|&i| parser.nodes[i].0.as_str())
                    .collect();
                format!(
                    "{} ({})",
                    cluster.label.as_deref().unwrap_or(&cluster.id),
                    members.join(", ")
                )
            })
            .collect();
        notes.push(format!(
            "clusters are drawn without their frames: {}",
            names.join("; ")
        ));
    }
    if parser.rank_noted {
        notes.push("`rank` constraints are not applied".into());
    }
    let label = get(&graph_attrs, "label")
        .map(|raw| label_text(raw, "", &graph_name))
        .filter(|label| !label.is_empty());
    Ok(DotGraph {
        graph: Graph::from_parts(direction, nodes, edges),
        directed,
        strict: parser.strict,
        name,
        label,
        clusters: parser.clusters,
        notes,
    })
}

// ------------------------------------------------------------- renderable

/// A DOT source as a renderable: drawn natively through the layered layout,
/// or, when it cannot be, the reason under a dim `DOT:` note and the source.
///
/// Under the drawing come the graph's `label` and a note for anything
/// accepted but not drawn. Like [`Diagram`](crate::Diagram), the drawing is
/// cropped at the width it is given, never wrapped.
///
/// ```
/// use rich::Console;
/// use rich_diagram::Dot;
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let out = console.render_export(&Dot::new("digraph { rankdir=LR; a -> b }"));
/// assert!(out.contains("│ a ├─►│ b │"), "{out}");
/// ```
#[derive(Clone, Debug)]
pub struct Dot {
    source: String,
    ascii: Option<bool>,
    parsed: OnceLock<Result<DotGraph, DotError>>,
    drawn: [OnceLock<Result<Drawing, DrawError>>; 2],
}

impl Dot {
    pub fn new(source: impl Into<String>) -> Self {
        Dot {
            source: source.into(),
            ascii: None,
            parsed: OnceLock::new(),
            drawn: Default::default(),
        }
    }

    /// Draw with ASCII only (`true`) or box drawing (`false`), whatever the
    /// console's encoding.
    pub fn ascii(mut self, ascii: bool) -> Self {
        self.ascii = Some(ascii);
        self
    }

    /// Set [`Dot::ascii`] from an option: `None` follows the console.
    pub fn ascii_option(mut self, ascii: Option<bool>) -> Self {
        self.ascii = ascii;
        self
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The parsed graph, or why it could not be read.
    pub fn parsed(&self) -> Result<&DotGraph, &DotError> {
        self.parsed.get_or_init(|| parse(&self.source)).as_ref()
    }

    fn drawing(&self, graph: &Graph, ascii: bool) -> Result<&Drawing, &DrawError> {
        self.drawn[usize::from(ascii)]
            .get_or_init(|| draw(graph, ascii))
            .as_ref()
    }

    fn source_block(
        &self,
        reason: &str,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Vec<Segment> {
        let mut segments = note(reason, console, options);
        // The source comes from a document: drop control characters before
        // showing it.
        let source: String = self
            .source
            .trim_end_matches('\n')
            .chars()
            .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
            .collect();
        if source.trim().is_empty() {
            return segments;
        }
        for line in source.lines() {
            let text = Text::new(format!("  {}", line.replace('\t', "    ")));
            segments.extend(text.no_wrap(true).rich_render(console, options));
            end_line(&mut segments);
        }
        segments
    }
}

impl Renderable for Dot {
    /// The drawing's width, like [`Diagram`](crate::Diagram); the whole
    /// width when there is no drawing (the notes and source wrap).
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let ascii = self.ascii.unwrap_or_else(|| console.ascii_only());
        match self.parsed() {
            Ok(parsed) if !parsed.graph.is_empty() => match self.drawing(&parsed.graph, ascii) {
                Ok(drawing) => Measurement::new(drawing.width, drawing.width),
                Err(_) => Measurement::new(options.max_width, options.max_width),
            },
            _ => Measurement::new(options.max_width, options.max_width),
        }
    }

    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        trim_final_newline(self.render_lines(console, options))
    }
}

impl Dot {
    /// The drawing, its label and notes, each line ending in a newline.
    fn render_lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let parsed = match self.parsed() {
            Ok(parsed) => parsed,
            Err(error) => return self.source_block(&error.to_string(), console, options),
        };
        if parsed.graph.is_empty() {
            return self.source_block("the graph has no nodes", console, options);
        }
        let ascii = self.ascii.unwrap_or_else(|| console.ascii_only());
        let drawing = match self.drawing(&parsed.graph, ascii) {
            Ok(drawing) => drawing,
            Err(error) => {
                return self.source_block(&format!("too large to draw: {error}"), console, options)
            }
        };
        let width = options.max_width;
        let mut segments = Vec::new();
        for line in drawing.cropped(width) {
            segments.push(Segment::new(line, None));
            segments.push(Segment::line());
        }
        if let Some(label) = &parsed.label {
            let label: String = label
                .chars()
                .filter(|c| !c.is_control() || *c == '\n')
                .collect();
            let style = Style::parse("bold").expect("valid style");
            segments.extend(Text::styled(label, style).rich_render(console, options));
            end_line(&mut segments);
        }
        let mut notes = parsed.notes.clone();
        if drawing.width > width {
            notes.push(format!("cropped to {width} of {} columns", drawing.width));
        }
        for text in notes {
            segments.extend(note(&text, console, options));
        }
        segments
    }
}

/// A dim note, wrapped to the width, ending its line.
fn note(text: &str, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    let style = Style::parse("dim italic").expect("valid style");
    let mut segments = Text::styled(format!("DOT: {text}"), style).rich_render(console, options);
    end_line(&mut segments);
    segments
}

/// Drop the newline that ends the last line: like core's renderables, the
/// drawing leaves that to `print`, so it is not followed by a blank line.
fn trim_final_newline(mut segments: Vec<Segment>) -> Vec<Segment> {
    if let Some(index) = segments
        .iter()
        .rposition(|segment| !segment.text.is_empty())
    {
        if segments[index].text.ends_with('\n') {
            segments[index].text.pop();
            if segments[index].text.is_empty() {
                segments.remove(index);
            }
        }
    }
    segments
}

/// End the last line, so whatever follows starts on a line of its own.
fn end_line(segments: &mut Vec<Segment>) {
    if segments
        .iter()
        .rev()
        .find(|segment| !segment.text.is_empty())
        .is_some_and(|segment| !segment.text.ends_with('\n'))
    {
        segments.push(Segment::line());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(parsed: &DotGraph) -> Vec<(&str, &str)> {
        parsed
            .graph
            .edges()
            .iter()
            .map(|e| {
                (
                    parsed.graph.nodes()[e.from].id.as_str(),
                    parsed.graph.nodes()[e.to].id.as_str(),
                )
            })
            .collect()
    }

    #[test]
    fn chains_groups_and_defaults() {
        let parsed = parse(
            "strict digraph G {\n\
               node [shape=box]\n\
               a -> b -> c\n\
               a -> { d e } [label=\"fan\", style=dashed]\n\
               a -> b [label=again]\n\
             }",
        )
        .unwrap();
        assert!(parsed.strict && parsed.directed);
        assert_eq!(parsed.name.as_deref(), Some("G"));
        assert_eq!(
            ids(&parsed),
            [("a", "b"), ("b", "c"), ("a", "d"), ("a", "e")]
        );
        assert_eq!(parsed.graph.edges()[0].label.as_deref(), Some("again"));
        assert_eq!(parsed.graph.edges()[2].stroke, Stroke::Dotted);
        assert!(parsed.graph.nodes().iter().all(|n| n.shape == Shape::Rect));
    }

    #[test]
    fn defaults_are_scoped_to_their_subgraph() {
        let parsed = parse(
            "graph {\n\
               subgraph cluster_x { label=\"X\"; node [shape=circle]; a -- b }\n\
               c -- a\n\
             }",
        )
        .unwrap();
        let shapes: Vec<Shape> = parsed.graph.nodes().iter().map(|n| n.shape).collect();
        assert_eq!(shapes, [Shape::Circle, Shape::Circle, Shape::Round]);
        assert_eq!(parsed.clusters.len(), 1);
        assert_eq!(parsed.clusters[0].label.as_deref(), Some("X"));
        assert_eq!(parsed.clusters[0].nodes, [0, 1]);
        let edge = &parsed.graph.edges()[0];
        assert_eq!((edge.start, edge.end), (Head::None, Head::None));
        assert_eq!(
            parsed.notes,
            ["clusters are drawn without their frames: X (a, b)"]
        );
    }

    #[test]
    fn quoted_ids_comments_and_labels() {
        let parsed = parse(
            "# generated\n\
             digraph {\n\
               // a comment\n\
               \"first node\" [label=\"Line one\\nline \" + \"two\\l\"] /* block\n comment */\n\
               \"first node\" -> 2.5 [dir=both, arrowtail=dot]\n\
             }",
        )
        .unwrap();
        assert_eq!(parsed.graph.nodes()[0].label, "Line one\nline two");
        assert_eq!(parsed.graph.nodes()[1].id, "2.5");
        let edge = &parsed.graph.edges()[0];
        assert_eq!((edge.start, edge.end), (Head::Circle, Head::Arrow));
    }

    #[test]
    fn unsupported_constructs_name_themselves_and_their_line() {
        for (source, line, construct) in [
            ("digraph {\n a:p -> b\n}", 2, "a node port (`a:…`)"),
            (
                "digraph {\n\n a [label=<<b>x</b>>]\n}",
                3,
                "an HTML-like label",
            ),
            ("digraph {\n a [shape=record]\n}", 2, "the `record` shape"),
            ("digraph {}\ndigraph {}", 2, "more than one graph in a file"),
        ] {
            let error = parse(source).unwrap_err();
            assert_eq!(error.line, line, "{source}: {error}");
            assert_eq!(
                error.construct.as_deref(),
                Some(construct),
                "{source}: {error}"
            );
        }
    }

    #[test]
    fn syntax_errors_say_where() {
        let error = parse("graph {\n a -> b\n}").unwrap_err();
        assert_eq!(
            error.to_string(),
            "line 2: `->` in an undirected `graph`; use `--`"
        );
        assert_eq!(error.construct, None);
        let error = parse("digraph {\n a -> \n").unwrap_err();
        assert!(error.to_string().contains("end of the file"), "{error}");
        let error = parse("flowchart LR").unwrap_err();
        assert_eq!(error.line, 1);
    }
}
