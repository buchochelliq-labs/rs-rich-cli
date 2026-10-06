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
//! - subgraphs; a subgraph named `cluster…` is a [`Cluster`], drawn as a
//!   labelled frame around its nodes (nested clusters nest their frames);
//! - `rank=same` in a subgraph, which draws its nodes in one rank where the
//!   layered layout can (see [`Graph::add_same_rank`]); other `rank` values
//!   (`min`, `max`, `source`, `sink`) are accepted with a note;
//! - quoted IDs (with `+` concatenation), numerals, and `//`, `/* */` and
//!   `#` comments.
//!
//! Attributes that only change how Graphviz paints (`color`, `fontname`,
//! `penwidth`, …) are accepted and ignored. What the native drawing cannot
//! represent is refused with a [`DotError`] naming the construct and its
//! line, never drawn partially: node ports (`a:p`), HTML-like labels
//! (`<…>`), `record` shapes, and more than one graph in a file.
//!
//! A source is read within fixed bounds, so a document cannot make it do
//! unbounded work: at most [`MAX_SOURCE`] bytes, [`MAX_NODES`] nodes,
//! [`MAX_EDGES`] edges (counted as `{ … }` groups expand) and
//! [`MAX_NESTING`] levels of `{ … }` and subgraphs. Past any of them the
//! source is refused with a [`DotError`], never drawn in part.
//!
//! Invisible nodes (`style=invis`) and nodes without an outline
//! (`shape=plaintext`, `plain`, `none`) are drawn as boxes, with a note
//! naming them; invisible edges are laid out but not drawn.
//!
//! [`Dot`] renders a source: the drawing, or the error and the source.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use rich::console::{Console, ConsoleOptions};
use rich::measure::Measurement;
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;
use rich::text::Text;

use crate::graph::{Direction, Edge, Graph, Head, Node, Shape, Stroke};
pub use crate::graph::{MAX_EDGES, MAX_NODES, MAX_SOURCE};
use crate::layout::{draw, DrawError, Drawing};

/// The deepest `{ … }` groups and subgraphs may nest. Each level is a
/// recursive step of the parser, so deeper sources are refused rather than
/// risk the stack.
pub const MAX_NESTING: usize = 64;

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

/// A subgraph named `cluster…`: a group of nodes framed in the drawing. Its
/// `id` is the subgraph's name, `cluster` prefix included; its `nodes` are
/// those of nested clusters too, in the order they were first mentioned; its
/// `parent` is the cluster it was first opened in.
pub use crate::graph::Cluster;

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
    /// The `cluster…` subgraphs, outermost first (as in
    /// [`Graph::clusters`]).
    pub clusters: Vec<Cluster>,
    /// What was accepted but is not drawn (`rank` constraints other than
    /// `rank=same`, invisible and outline-free nodes), one sentence each.
    /// What the layout cannot do is in [`Drawing::notes`] instead.
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

#[derive(Clone)]
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
                    Some(d) if d.is_ascii_digit() || d == '.' => {
                        Ok(Some((self.numeral(line)?, line)))
                    }
                    _ => Err(DotError::syntax(line, "unexpected `-`")),
                }
            }
            c if c.is_ascii_digit() || c == '.' => Ok(Some((self.numeral(line)?, line))),
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

    /// A numeral as Graphviz lexes one: `[-]?(.[0-9]+|[0-9]+(.[0-9]*)?)`.
    /// A second `.` starts the next token (`1.2.3` is `1.2` then `.3`), and
    /// a `.` without a digit is an error.
    fn numeral(&mut self, line: usize) -> Result<Tok, DotError> {
        let mut text = String::new();
        if self.chars.peek() == Some(&'-') {
            text.push('-');
            self.bump();
        }
        let mut digits = false;
        let mut take_digits = |lexer: &mut Self, text: &mut String| {
            while let Some(&c) = lexer.chars.peek() {
                if !c.is_ascii_digit() {
                    break;
                }
                digits = true;
                text.push(c);
                lexer.bump();
            }
        };
        take_digits(self, &mut text);
        if self.chars.peek() == Some(&'.') {
            text.push('.');
            self.bump();
            take_digits(self, &mut text);
        }
        if !digits {
            return Err(DotError::syntax(
                line,
                format!("expected a digit with the `.` in `{text}`"),
            ));
        }
        Ok(Tok::Id(text, false))
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
    /// With `strict`, each edge's index in `edges` by its ends (the lower
    /// index first when undirected), so a repeat merges without a scan.
    edge_index: HashMap<(usize, usize), usize>,
    /// `{ … }` groups and subgraphs open around the current statement.
    depth: usize,
    scopes: Vec<Scope>,
    clusters: Vec<Cluster>,
    /// Each cluster's nodes, to find one without a search.
    cluster_nodes: Vec<HashSet<usize>>,
    /// `rank=same` subgraphs' nodes.
    same_rank: Vec<Vec<usize>>,
    /// `rank` settings not applied, as `rank=…`, each once.
    rank_ignored: Vec<String>,
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
        let mut seen = HashSet::new();
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
                _ => self.statement(&mut members, &mut seen)?,
            }
        }
    }

    fn statement(
        &mut self,
        members: &mut Vec<usize>,
        seen: &mut HashSet<usize>,
    ) -> Result<(), DotError> {
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
            // The token after the peeked one, past any comments: only `=`
            // matters.
            let mut ahead = self.lexer.clone();
            if ahead.skip().is_ok() && ahead.chars.peek() == Some(&'=') {
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
        for (group, subgraph) in &groups {
            for &node in group {
                if seen.insert(node) {
                    members.push(node);
                }
                // A subgraph's nodes are already members of every cluster
                // open around it.
                if !subgraph {
                    self.note_member(node);
                }
            }
        }
        let groups: Vec<Vec<usize>> = groups.into_iter().map(|(group, _)| group).collect();
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
        // Groups expand to every pair: counted edge by edge, so a large
        // cross product is refused as it grows, not after.
        for (pair, &op_line) in groups.windows(2).zip(&lines[1..]) {
            for &from in &pair[0] {
                for &to in &pair[1] {
                    self.add_edge(from, to, &edge_attrs, op_line)?;
                }
            }
        }
        Ok(())
    }

    fn add_edge(
        &mut self,
        from: usize,
        to: usize,
        attrs: &Attrs,
        line: usize,
    ) -> Result<(), DotError> {
        let key = if self.directed {
            (from, to)
        } else {
            (from.min(to), from.max(to))
        };
        if self.strict {
            if let Some(&index) = self.edge_index.get(&key) {
                let existing = &mut self.edges[index].2;
                for (key, value) in attrs {
                    set(existing, key, value);
                }
                return Ok(());
            }
        }
        if self.edges.len() >= MAX_EDGES {
            return Err(DotError::unsupported(
                line,
                format!("a graph of more than {MAX_EDGES} edges"),
                "`{ … }` groups count each edge they make; split the graph",
            ));
        }
        if self.strict {
            self.edge_index.insert(key, self.edges.len());
        }
        self.edges.push((from, to, attrs.clone()));
        Ok(())
    }

    /// One end of an edge: a node, or a subgraph's nodes; and whether it
    /// is a subgraph.
    fn endpoint(&mut self) -> Result<(Vec<usize>, bool), DotError> {
        let line = self.here()?;
        if matches!(self.peek()?, Some(Tok::LBrace))
            || matches!(self.peek()?, Some(Tok::Id(text, false)) if text.eq_ignore_ascii_case("subgraph"))
        {
            return Ok((self.subgraph()?, true));
        }
        let id = self.id("a node, `subgraph` or `{`")?;
        if self.peek()? == Some(&Tok::Colon) {
            return Err(DotError::unsupported(
                line,
                format!("a node port (`{id}:…`)"),
                "connect the node itself",
            ));
        }
        Ok((vec![self.node(&id, line)?], false))
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
                    let parent = self.open.iter().rev().flatten().next().copied();
                    self.clusters.push(Cluster {
                        id: name.clone(),
                        label: None,
                        nodes: Vec::new(),
                        parent,
                    });
                    self.cluster_nodes.push(HashSet::new());
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
        if self.depth >= MAX_NESTING {
            return Err(DotError::unsupported(
                line,
                format!("nesting deeper than {MAX_NESTING} levels of `{{ … }}` and subgraphs"),
                "flatten the subgraphs",
            ));
        }
        self.take()?;
        let inherited = self.scopes.last().cloned().unwrap_or_default();
        self.scopes.push(Scope {
            graph: Attrs::new(),
            ..inherited
        });
        self.open.push(index);
        self.depth += 1;
        let members = self.statements();
        self.depth -= 1;
        self.open.pop();
        let scope = self.scopes.pop().unwrap_or_default();
        let members = members?;
        match get(&scope.graph, "rank") {
            Some(rank) if rank.eq_ignore_ascii_case("same") => {
                self.same_rank.push(members.clone());
            }
            Some(rank) => self.ignore_rank(&format!("rank={rank}")),
            None => {}
        }
        if let Some(index) = index {
            if let Some(label) = get(&scope.graph, "label") {
                let names = Names {
                    graph: Some(&name),
                    ..Names::default()
                };
                self.clusters[index].label = Some(label_text(label, &names));
            }
        }
        Ok(members)
    }

    /// Record `node` as a member of every cluster open around it.
    fn note_member(&mut self, node: usize) {
        for &index in self.open.iter().flatten() {
            if self.cluster_nodes[index].insert(node) {
                self.clusters[index].nodes.push(node);
            }
        }
    }

    /// The index of node `id`, adding it with the defaults in force.
    fn node(&mut self, id: &str, line: usize) -> Result<usize, DotError> {
        if let Some(&index) = self.ids.get(id) {
            return Ok(index);
        }
        if self.nodes.len() >= MAX_NODES {
            return Err(DotError::unsupported(
                line,
                format!("a graph of more than {MAX_NODES} nodes"),
                "split the graph",
            ));
        }
        let defaults = self
            .scopes
            .last()
            .map(|scope| scope.node.clone())
            .unwrap_or_default();
        self.ids.insert(id.to_string(), self.nodes.len());
        self.nodes.push((id.to_string(), defaults));
        Ok(self.nodes.len() - 1)
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

    fn ignore_rank(&mut self, setting: &str) {
        if !self.rank_ignored.iter().any(|s| s == setting) {
            self.rank_ignored.push(setting.to_string());
        }
    }

    fn graph_attr(&mut self, key: &str, value: &str) {
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

/// The names a label's escapes stand for, where the labelled object has
/// them: `\N` a node's, `\G` the graph's (or a cluster's own), and on an
/// edge `\T` its tail's, `\H` its head's and `\E` the edge's (`a->b`).
#[derive(Default)]
struct Names<'a> {
    node: Option<&'a str>,
    graph: Option<&'a str>,
    /// Tail, head, and whether the graph is directed.
    edge: Option<(&'a str, &'a str, bool)>,
}

/// A label's text, as Graphviz substitutes it: `\n`, `\l` and `\r` break
/// lines (a trailing one is dropped), `\N`, `\G`, `\T`, `\H` and `\E` are
/// the [`Names`] the object has, and any other escaped character (one of
/// those the object lacks included) stands for itself.
fn label_text(raw: &str, names: &Names<'_>) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'l' | 'r') => out.push('\n'),
            Some('N') if names.node.is_some() => out.push_str(names.node.unwrap_or_default()),
            Some('G') if names.graph.is_some() => out.push_str(names.graph.unwrap_or_default()),
            Some(escape @ ('T' | 'H' | 'E')) if names.edge.is_some() => {
                let (tail, head, directed) = names.edge.unwrap_or_default();
                match escape {
                    'T' => out.push_str(tail),
                    'H' => out.push_str(head),
                    _ => {
                        out.push_str(tail);
                        out.push_str(if directed { "->" } else { "--" });
                        out.push_str(head);
                    }
                }
            }
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
    if source.len() > MAX_SOURCE {
        return Err(DotError::unsupported(
            1,
            format!("a source over {MAX_SOURCE} bytes ({})", source.len()),
            "split the graph",
        ));
    }
    let mut parser = Parser {
        lexer: Lexer::new(source),
        peeked: None,
        line: 1,
        directed: false,
        strict: false,
        ids: HashMap::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        edge_index: HashMap::new(),
        depth: 0,
        scopes: Vec::new(),
        clusters: Vec::new(),
        cluster_nodes: Vec::new(),
        same_rank: Vec::new(),
        rank_ignored: Vec::new(),
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
                Some(raw) => {
                    let names = Names {
                        node: Some(id),
                        graph: Some(&graph_name),
                        edge: None,
                    };
                    label_text(raw, &names)
                }
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
            let names = Names {
                node: None,
                graph: Some(&graph_name),
                edge: Some((&parser.nodes[*from].0, &parser.nodes[*to].0, directed)),
            };
            edge.label = get(attrs, "label")
                .map(|raw| label_text(raw, &names))
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
    // `rank` on the graph itself ranks nothing in Graphviz either.
    if let Some(rank) = get(&graph_attrs, "rank") {
        parser.ignore_rank(&format!("rank={rank} (on the whole graph)"));
    }
    if !parser.rank_ignored.is_empty() {
        notes.push(format!(
            "only `rank=same` in a subgraph is applied; not applied: {}",
            parser.rank_ignored.join(", ")
        ));
    }
    let named = |test: &dyn Fn(&Attrs) -> bool| -> Vec<&str> {
        parser
            .nodes
            .iter()
            .filter(|(_, attrs)| test(attrs))
            .map(|(id, _)| id.as_str())
            .collect()
    };
    let invisible = named(&|attrs| {
        get(attrs, "style").is_some_and(|style| {
            style
                .split(',')
                .any(|s| matches!(s.trim(), "invis" | "invisible"))
        })
    });
    if !invisible.is_empty() {
        notes.push(format!(
            "invisible nodes (`style=invis`) are drawn: {}",
            invisible.join(", ")
        ));
    }
    let bare = named(&|attrs| {
        get(attrs, "shape").is_some_and(|shape| {
            ["plaintext", "plain", "none"]
                .iter()
                .any(|s| shape.eq_ignore_ascii_case(s))
        })
    });
    if !bare.is_empty() {
        notes.push(format!(
            "nodes without an outline (`shape=plaintext`, `plain` or `none`) are drawn boxed: {}",
            bare.join(", ")
        ));
    }
    let names = Names {
        graph: Some(&graph_name),
        ..Names::default()
    };
    let label = get(&graph_attrs, "label")
        .map(|raw| label_text(raw, &names))
        .filter(|label| !label.is_empty());
    let mut graph = Graph::from_parts(direction, nodes, edges);
    for cluster in &parser.clusters {
        graph.add_cluster(cluster.clone());
    }
    for group in parser.same_rank {
        graph.add_same_rank(group);
    }
    Ok(DotGraph {
        graph,
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
        let ascii = self.ascii.unwrap_or_else(|| console.ascii_only());
        let mut segments = note(reason, ascii, console, options);
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
        notes.extend(drawing.notes.iter().cloned());
        if drawing.width > width {
            notes.push(format!("cropped to {width} of {} columns", drawing.width));
        }
        for text in notes {
            segments.extend(note(&text, ascii, console, options));
        }
        segments
    }
}

/// A dim `DOT:` note, wrapped to the width, ending its line.
fn note(text: &str, ascii: bool, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
    prefixed_note("DOT", text, ascii, console, options)
}

/// A dim note after `prefix:`, wrapped to the width, ending its line. With
/// `ascii`, the notes' own non-ASCII characters (`…`, `×`) are spelled in
/// ASCII.
pub(crate) fn prefixed_note(
    prefix: &str,
    text: &str,
    ascii: bool,
    console: &Console,
    options: &ConsoleOptions,
) -> Vec<Segment> {
    let mut text: String = text.chars().filter(|c| !c.is_control()).collect();
    if ascii {
        text = text.replace('…', "...").replace('×', "x");
    }
    let style = Style::parse("dim italic").expect("valid style");
    let mut segments =
        Text::styled(format!("{prefix}: {text}"), style).rich_render(console, options);
    end_line(&mut segments);
    segments
}

/// Drop the newline that ends the last line: like core's renderables, the
/// drawing leaves that to `print`, so it is not followed by a blank line.
pub(crate) fn trim_final_newline(mut segments: Vec<Segment>) -> Vec<Segment> {
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
pub(crate) fn end_line(segments: &mut Vec<Segment>) {
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
        assert_eq!(parsed.graph.clusters(), parsed.clusters);
        assert!(parsed.notes.is_empty(), "{:?}", parsed.notes);
    }

    /// Every mention of a node searched each open cluster's nodes, and each
    /// enclosing subgraph searched them again for every node inside it: a
    /// small source nesting clusters deep took seconds.
    #[test]
    fn deep_clusters_note_their_nodes_in_linear_time() {
        let mut source = String::from("digraph {\n");
        for depth in 0..MAX_NESTING - 1 {
            source.push_str(&format!("subgraph cluster{depth} {{"));
        }
        for n in 0..MAX_NODES - 1 {
            source.push_str(&format!("n{n} "));
        }
        // The last node, mentioned again and again.
        while source.len() < MAX_SOURCE - 2 * MAX_NESTING {
            source.push_str("z ");
        }
        source.push_str(&"}".repeat(MAX_NESTING));
        let started = std::time::Instant::now();
        let parsed = parse(&source).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(parsed.clusters.len(), MAX_NESTING - 1);
        let all: Vec<usize> = (0..MAX_NODES).collect();
        for cluster in &parsed.clusters {
            assert_eq!(cluster.nodes, all);
        }
    }

    #[test]
    fn nested_clusters_know_their_parent() {
        let parsed = parse(
            "digraph {\n\
               subgraph cluster_outer { a; subgraph cluster_inner { b; c } }\n\
               subgraph cluster_outer { d }\n\
               subgraph cluster_other { e }\n\
             }",
        )
        .unwrap();
        let shape: Vec<(&str, Option<usize>, &[usize])> = parsed
            .clusters
            .iter()
            .map(|c| (c.id.as_str(), c.parent, c.nodes.as_slice()))
            .collect();
        assert_eq!(
            shape,
            [
                ("cluster_outer", None, &[0, 1, 2, 3][..]),
                ("cluster_inner", Some(0), &[1, 2][..]),
                ("cluster_other", None, &[4][..]),
            ]
        );
    }

    #[test]
    fn rank_same_groups_and_ignored_ranks() {
        let parsed = parse(
            "digraph {\n\
               rank=min\n\
               a -> b -> c; a -> d\n\
               { rank=same; b; d }\n\
               subgraph s { rank = max; c }\n\
               subgraph t { graph [rank=same]; c }\n\
             }",
        )
        .unwrap();
        assert_eq!(parsed.graph.same_rank_groups(), [vec![1, 3], vec![2]]);
        assert_eq!(
            parsed.notes,
            [
                "only `rank=same` in a subgraph is applied; not applied: rank=max, \
              rank=min (on the whole graph)"
            ]
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
