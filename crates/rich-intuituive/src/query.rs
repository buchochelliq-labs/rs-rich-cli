//! A query language over records: what a filter box, a search line or a
//! command bar reads. [`parse`] reads a text by a [`Grammar`] (the field
//! names, their aliases and value types, how bare words match) into a
//! [`Query`], and [`Query::matches`] tests any [`Record`]. Generic: a
//! todo list's `project:home +next due.before:eow`, a process list's
//! `cpu > 5 and name:fire`, a log's `level:error or "timed out"` are each
//! a grammar, not core code.
//!
//! The syntax:
//!
//! - **Bare words** match the record's [`text`](Record::text), ignoring
//!   case; `"quoted words"` match as one.
//! - **`field:value`** compares a field (`project:home`, `project:"my
//!   home"`); `field:` matches a record without it. How it compares is the
//!   field's [`Kind`]: text by its [`TextMatch`] (exact by default),
//!   numbers equal, dates on the same day, booleans by `yes`/`true`/`1`.
//! - **Modifiers**, `field.modifier:value`: `before`/`below` and
//!   `after`/`above` (less, greater), `is`/`equals` and `not`/`isnt`,
//!   `has`/`contains` and `hasnt`, `startswith` and `endswith`, `none`
//!   and `any` (absent, present).
//! - **Comparisons**, `field > value` (with or without spaces): `<`, `<=`,
//!   `>`, `>=`, `=` (or `==`), `!=`.
//! - **Tags**: `+tag` and `-tag` test the record's [`tags`](Record::tags).
//! - **`and`**, **`or`** (`and` binds tighter, and two terms side by side
//!   are an `and`), **`not`** (or `!` before a term), and parentheses.
//!
//! ```
//! use intuituive::query::{self, Grammar, Kind, Record, Value};
//!
//! struct Task { project: &'static str, urgency: f64, tags: Vec<&'static str>, text: &'static str }
//!
//! impl Record for Task {
//!     fn field(&self, name: &str) -> Option<Value> {
//!         match name {
//!             "project" => Some(Value::from(self.project)),
//!             "urgency" => Some(Value::Number(self.urgency)),
//!             _ => None,
//!         }
//!     }
//!     fn text(&self) -> String {
//!         self.text.to_string()
//!     }
//!     fn tags(&self) -> Vec<String> {
//!         self.tags.iter().map(|t| t.to_string()).collect()
//!     }
//! }
//!
//! let grammar = Grammar::new()
//!     .field("project", Kind::Text)
//!     .alias("proj", "project")
//!     .field("urgency", Kind::Number);
//! let task = Task { project: "home", urgency: 7.5, tags: vec!["next"], text: "Paint the fence" };
//! let matches = |text: &str| query::parse(text, &grammar).unwrap().matches(&task);
//! assert!(matches("proj:home +next"));
//! assert!(matches("urgency.above:5 fence"));
//! assert!(matches("urgency > 9 or -work"));
//! assert!(!matches("not (project:home and paint)"));
//! // Errors say where.
//! let error = query::parse("urgency.above:soon", &grammar).unwrap_err();
//! assert_eq!(error.at, 14..18);
//! ```

use std::fmt;
use std::ops::Range;
use std::rc::Rc;

use rich_interact::datetime::{Date, DateTime};

/// A field's value, as a [`Record`] gives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
    Date(DateTime),
    Bool(bool),
}

impl From<&str> for Value {
    fn from(text: &str) -> Value {
        Value::Text(text.to_string())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Value {
        Value::Text(text)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Value {
        Value::Number(n)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Value {
        Value::Number(n as f64)
    }
}

impl From<DateTime> for Value {
    fn from(at: DateTime) -> Value {
        Value::Date(at)
    }
}

impl From<Date> for Value {
    fn from(date: Date) -> Value {
        Value::Date(date.into())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl Value {
    /// The value as text: what `has`, `startswith` and a text match read.
    pub fn as_text(&self) -> String {
        match self {
            Value::Text(text) => text.clone(),
            Value::Number(n) => n.to_string(),
            Value::Date(at) => at.to_string(),
            Value::Bool(b) => b.to_string(),
        }
    }
}

/// What a query is tested against: anything with fields, a text for
/// bare words, and tags.
pub trait Record {
    /// The value of field `name` (the grammar's name, after aliases);
    /// `None` when the record has none.
    fn field(&self, name: &str) -> Option<Value>;

    /// What bare words are looked for in.
    fn text(&self) -> String {
        String::new()
    }

    /// The record's tags, for `+tag` and `-tag`.
    fn tags(&self) -> Vec<String> {
        Vec::new()
    }

    /// Whether it has `tag` (ignoring case unless the grammar does not):
    /// by default, one of [`tags`](Self::tags). A record with virtual tags
    /// (Taskwarrior's `+OVERDUE`) answers for them here.
    fn has_tag(&self, tag: &str, case_sensitive: bool) -> bool {
        self.tags().iter().any(|t| {
            if case_sensitive {
                t == tag
            } else {
                t.eq_ignore_ascii_case(tag)
            }
        })
    }
}

/// A field's type: how a query's value for it is read and compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Number,
    /// Read by the grammar's [`dates`](Grammar::dates) (ISO by default).
    Date,
    Bool,
}

/// How `field:value` compares a text field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextMatch {
    /// The whole value.
    #[default]
    Exact,
    /// The start of it: `project:home` matches `home` and `home.garden`.
    Prefix,
    /// Anywhere in it.
    Contains,
}

/// Reads a query's value for one field.
type Reader = Rc<dyn Fn(&str) -> Option<Value>>;

/// A field of a [`Grammar`].
#[derive(Clone)]
struct Field {
    name: String,
    kind: Kind,
    matching: TextMatch,
    reader: Option<Reader>,
}

/// A bare word's own test, from [`Grammar::words`].
pub type Matcher = Rc<dyn Fn(&dyn Record) -> bool>;

/// Gives bare words their own matchers.
type WordsFn = Rc<dyn Fn(&str) -> Option<Matcher>>;

/// What a [`parse`] reads: the fields (names, aliases, kinds), how dates
/// read, whether case counts, and how bare words match. A syntax
/// (Taskwarrior's, a process list's) is a grammar a plugin builds.
#[derive(Clone, Default)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct Grammar {
    fields: Vec<Field>,
    aliases: Vec<(String, String)>,
    dates: Option<Reader>,
    words: Option<WordsFn>,
    case_sensitive: bool,
    /// An unknown `name:value` is a bare word rather than an error.
    lenient: bool,
    no_tags: bool,
}

impl fmt::Debug for Grammar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grammar")
            .field("fields", &self.field_names())
            .field("aliases", &self.aliases)
            .field("case_sensitive", &self.case_sensitive)
            .finish()
    }
}

impl Grammar {
    pub fn new() -> Grammar {
        Grammar::default()
    }

    /// A field `name` of `kind`.
    pub fn field(mut self, name: &str, kind: Kind) -> Grammar {
        self.fields.retain(|f| f.name != name);
        self.fields.push(Field {
            name: name.to_string(),
            kind,
            matching: TextMatch::Exact,
            reader: None,
        });
        self
    }

    /// The last field added compares `field:value` by `matching` (text
    /// fields only).
    pub fn matching(mut self, matching: TextMatch) -> Grammar {
        if let Some(field) = self.fields.last_mut() {
            field.matching = matching;
        }
        self
    }

    /// The last field added reads its values with `read`, rather than by
    /// its kind: sizes (`1GiB`), durations, names of levels.
    pub fn read(mut self, read: impl Fn(&str) -> Option<Value> + 'static) -> Grammar {
        if let Some(field) = self.fields.last_mut() {
            field.reader = Some(Rc::new(read));
        }
        self
    }

    /// `alias` names field `field` too (`proj` for `project`).
    pub fn alias(mut self, alias: &str, field: &str) -> Grammar {
        self.aliases.push((alias.to_string(), field.to_string()));
        self
    }

    /// Read date values with `read`: [`dates::parse`](crate::dates::parse)
    /// relative to a now, for `due.before:eow`. By default dates are ISO
    /// (`2026-11-01`, `2026-11-01 14:30`).
    pub fn dates(mut self, read: impl Fn(&str) -> Option<DateTime> + 'static) -> Grammar {
        self.dates = Some(Rc::new(move |text| read(text).map(Value::Date)));
        self
    }

    /// Give some bare words a test of their own: `words` gets each bare
    /// word and returns a matcher for those it knows (Taskwarrior's ids,
    /// `2,4-5`; a regular expression), `None` for the rest, which match
    /// the record's text.
    pub fn words(mut self, words: impl Fn(&str) -> Option<Matcher> + 'static) -> Grammar {
        self.words = Some(Rc::new(words));
        self
    }

    /// Let case count in text matches and tags (it does not by default).
    pub fn case_sensitive(mut self, on: bool) -> Grammar {
        self.case_sensitive = on;
        self
    }

    /// Read `name:value` with an unknown name as a bare word rather than
    /// an error (a URL, `a:b` in a description).
    pub fn lenient(mut self) -> Grammar {
        self.lenient = true;
        self
    }

    /// Read `+word` and `-word` as words, not tags.
    pub fn without_tags(mut self) -> Grammar {
        self.no_tags = true;
        self
    }

    /// The fields' names and aliases, for completion.
    pub fn field_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.fields.iter().map(|f| f.name.clone()).collect();
        names.extend(self.aliases.iter().map(|(a, _)| a.clone()));
        names
    }

    fn field_named(&self, name: &str) -> Option<&Field> {
        let lower = name.to_ascii_lowercase();
        let name = self
            .aliases
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(&lower))
            .map_or(lower.as_str(), |(_, field)| field.as_str());
        self.fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
    }
}

/// Why a text is not a query, and where: a byte range of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    pub message: String,
    pub at: Range<usize>,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at {})", self.message, self.at.start + 1)
    }
}

impl std::error::Error for QueryError {}

/// How a field is compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    /// `field:value`, by the field's kind.
    Matches,
    Is,
    Not,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Has,
    Hasnt,
    StartsWith,
    EndsWith,
    Missing,
    Present,
}

#[derive(Clone)]
enum Term {
    Field {
        name: String,
        kind: Kind,
        matching: TextMatch,
        op: Op,
        value: Option<Value>,
    },
    Tag(String),
    Word(String),
    Custom(Matcher),
}

#[derive(Clone)]
enum Expr {
    All(Vec<Expr>),
    Any(Vec<Expr>),
    Not(Box<Expr>),
    Term(Term),
}

/// A parsed query: [`matches`](Self::matches) tests a record. The empty
/// query matches everything.
#[derive(Clone)]
pub struct Query {
    expr: Expr,
    case_sensitive: bool,
}

impl fmt::Debug for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Query")
    }
}

impl Default for Query {
    fn default() -> Query {
        Query::all()
    }
}

impl Query {
    /// The query that matches everything.
    pub fn all() -> Query {
        Query {
            expr: Expr::All(Vec::new()),
            case_sensitive: false,
        }
    }

    /// Whether it has no terms (it matches everything).
    pub fn is_empty(&self) -> bool {
        matches!(&self.expr, Expr::All(terms) if terms.is_empty())
    }

    /// Whether `record` matches.
    pub fn matches(&self, record: &dyn Record) -> bool {
        self.eval(&self.expr, record)
    }

    /// The records of `records` that match.
    pub fn filter<'a, R: Record>(&'a self, records: &'a [R]) -> impl Iterator<Item = &'a R> + 'a {
        records.iter().filter(move |r| self.matches(*r))
    }

    fn eval(&self, expr: &Expr, record: &dyn Record) -> bool {
        match expr {
            Expr::All(all) => all.iter().all(|e| self.eval(e, record)),
            Expr::Any(any) => any.iter().any(|e| self.eval(e, record)),
            Expr::Not(e) => !self.eval(e, record),
            Expr::Term(term) => self.term(term, record),
        }
    }

    fn fold(&self, text: &str) -> String {
        if self.case_sensitive {
            text.to_string()
        } else {
            text.to_lowercase()
        }
    }

    fn term(&self, term: &Term, record: &dyn Record) -> bool {
        match term {
            Term::Word(word) => self.fold(&record.text()).contains(&self.fold(word)),
            Term::Tag(tag) => record.has_tag(tag, self.case_sensitive),
            Term::Custom(matcher) => matcher(record),
            Term::Field {
                name,
                kind,
                matching,
                op,
                value,
            } => {
                let have = record.field(name);
                match op {
                    Op::Missing => return have.is_none_or(|v| v.as_text().is_empty()),
                    Op::Present => return have.is_some_and(|v| !v.as_text().is_empty()),
                    _ => {}
                }
                let Some(want) = value else {
                    // `field:` with nothing: the field is absent or empty.
                    let missing = have.is_none_or(|v| v.as_text().is_empty());
                    return match op {
                        Op::Not | Op::Hasnt => !missing,
                        _ => missing,
                    };
                };
                let Some(have) = have else {
                    // Nothing is not anything, nor does it hold anything.
                    return matches!(op, Op::Not | Op::Hasnt);
                };
                let text = |v: &Value| self.fold(&v.as_text());
                match op {
                    Op::Matches => match (kind, &have, want) {
                        (Kind::Text, _, _) => {
                            let (have, want) = (text(&have), text(want));
                            match matching {
                                TextMatch::Exact => have == want,
                                TextMatch::Prefix => have.starts_with(&want),
                                TextMatch::Contains => have.contains(&want),
                            }
                        }
                        (Kind::Date, Value::Date(have), Value::Date(want)) => {
                            have.date == want.date
                        }
                        _ => compare(&have, want) == Some(std::cmp::Ordering::Equal),
                    },
                    Op::Is => match (&have, want) {
                        (Value::Text(_), _) => text(&have) == text(want),
                        _ => compare(&have, want) == Some(std::cmp::Ordering::Equal),
                    },
                    Op::Not => match (&have, want) {
                        (Value::Text(_), _) => text(&have) != text(want),
                        _ => compare(&have, want) != Some(std::cmp::Ordering::Equal),
                    },
                    Op::Less => compare(&have, want).is_some_and(|o| o.is_lt()),
                    Op::LessOrEqual => compare(&have, want).is_some_and(|o| o.is_le()),
                    Op::Greater => compare(&have, want).is_some_and(|o| o.is_gt()),
                    Op::GreaterOrEqual => compare(&have, want).is_some_and(|o| o.is_ge()),
                    Op::Has => text(&have).contains(&text(want)),
                    Op::Hasnt => !text(&have).contains(&text(want)),
                    Op::StartsWith => text(&have).starts_with(&text(want)),
                    Op::EndsWith => text(&have).ends_with(&text(want)),
                    Op::Missing | Op::Present => unreachable!("answered above"),
                }
            }
        }
    }
}

/// How two values order: numbers and dates by value, texts by their
/// characters (ignoring case), booleans false first; `None` for two
/// kinds.
fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
        (Value::Date(a), Value::Date(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        (Value::Text(a), Value::Text(b)) => Some(a.to_lowercase().cmp(&b.to_lowercase())),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Open,
    Close,
    /// A word, quoted or not (quotes taken off), and whether any of it was
    /// quoted (a quoted `and` is a word).
    Word(String, bool),
    Compare(&'static str),
}

/// The tokens of `text`, each with its byte range.
fn tokens(text: &str) -> Result<Vec<(Token, Range<usize>)>, QueryError> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        match c {
            b'(' => {
                out.push((Token::Open, i..i + 1));
                i += 1;
            }
            b')' => {
                out.push((Token::Close, i..i + 1));
                i += 1;
            }
            b'<' | b'>' | b'=' => {
                let two = bytes.get(i + 1) == Some(&b'=');
                let op = match (c, two) {
                    (b'<', true) => "<=",
                    (b'<', false) => "<",
                    (b'>', true) => ">=",
                    (b'>', false) => ">",
                    _ => "=",
                };
                i += 1 + two as usize;
                out.push((Token::Compare(op), start..i));
            }
            b'!' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push((Token::Compare("!="), start..i));
            }
            _ => {
                // A word: up to a space, a parenthesis or a comparison;
                // quotes anywhere in it hold spaces (`project:"a b"`).
                let mut word = String::new();
                let mut quoted = false;
                while i < bytes.len() {
                    let c = bytes[i];
                    if c == b'"' || c == b'\'' {
                        let close = text[i + 1..].find(c as char).ok_or_else(|| QueryError {
                            message: "a quote is not closed".into(),
                            at: i..text.len(),
                        })?;
                        word.push_str(&text[i + 1..i + 1 + close]);
                        quoted = true;
                        i += close + 2;
                        continue;
                    }
                    let compare = matches!(c, b'<' | b'>' | b'=')
                        || (c == b'!' && bytes.get(i + 1) == Some(&b'='));
                    if c.is_ascii_whitespace() || c == b'(' || c == b')' || compare {
                        break;
                    }
                    let ch = text[i..].chars().next().expect("a character");
                    word.push(ch);
                    i += ch.len_utf8();
                }
                out.push((Token::Word(word, quoted), start..i));
            }
        }
    }
    Ok(out)
}

/// Read `text` by `grammar`. An empty text is the query that matches
/// everything.
pub fn parse(text: &str, grammar: &Grammar) -> Result<Query, QueryError> {
    let tokens = tokens(text)?;
    let mut parser = Parser {
        tokens,
        at: 0,
        grammar,
        len: text.len(),
        depth: 0,
    };
    let expr = parser.any()?;
    if let Some((_, span)) = parser.tokens.get(parser.at) {
        return Err(QueryError {
            message: "a closing parenthesis has no opening one".into(),
            at: span.clone(),
        });
    }
    Ok(Query {
        expr,
        case_sensitive: grammar.case_sensitive,
    })
}

struct Parser<'a> {
    tokens: Vec<(Token, Range<usize>)>,
    at: usize,
    grammar: &'a Grammar,
    len: usize,
    /// How many parentheses are open.
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|(t, _)| t)
    }

    fn keyword(&self, name: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(w, false)) if w.eq_ignore_ascii_case(name))
    }

    /// Terms joined by `or`.
    fn any(&mut self) -> Result<Expr, QueryError> {
        let mut parts = vec![self.all()?];
        while self.keyword("or") {
            let span = self.tokens[self.at].1.clone();
            self.at += 1;
            if self.peek().is_none() || self.keyword("or") || self.peek() == Some(&Token::Close) {
                return Err(QueryError {
                    message: "nothing after `or`".into(),
                    at: span,
                });
            }
            parts.push(self.all()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().expect("a part")
        } else {
            Expr::Any(parts)
        })
    }

    /// Terms side by side, or joined by `and`.
    fn all(&mut self) -> Result<Expr, QueryError> {
        let mut parts = Vec::new();
        loop {
            match self.peek() {
                None | Some(Token::Close) => break,
                _ if self.keyword("or") => break,
                _ if self.keyword("and") => {
                    let span = self.tokens[self.at].1.clone();
                    self.at += 1;
                    if parts.is_empty()
                        || matches!(self.peek(), None | Some(Token::Close))
                        || self.keyword("or")
                        || self.keyword("and")
                    {
                        return Err(QueryError {
                            message: "`and` needs a term on each side".into(),
                            at: span,
                        });
                    }
                }
                _ => parts.push(self.unary()?),
            }
        }
        if parts.is_empty() {
            let message = match self.peek() {
                Some(Token::Close) if self.depth == 0 => "a closing parenthesis has no opening one",
                Some(Token::Close) => "nothing in the parentheses",
                Some(_) => "nothing before `or`",
                None => return Ok(Expr::All(parts)),
            };
            return Err(QueryError {
                message: message.into(),
                at: self.tokens[self.at].1.clone(),
            });
        }
        Ok(if parts.len() == 1 {
            parts.pop().expect("a part")
        } else {
            Expr::All(parts)
        })
    }

    fn unary(&mut self) -> Result<Expr, QueryError> {
        let (token, span) = self.tokens[self.at].clone();
        match token {
            Token::Word(w, false) if w.eq_ignore_ascii_case("not") => {
                self.at += 1;
                if matches!(self.peek(), None | Some(Token::Close)) {
                    return Err(QueryError {
                        message: "nothing after `not`".into(),
                        at: span,
                    });
                }
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Token::Open => {
                self.at += 1;
                self.depth += 1;
                let inner = self.any()?;
                self.depth -= 1;
                match self.peek() {
                    Some(Token::Close) => {
                        self.at += 1;
                        Ok(inner)
                    }
                    _ => Err(QueryError {
                        message: "a parenthesis is not closed".into(),
                        at: span.start..self.len,
                    }),
                }
            }
            Token::Close => unreachable!("handled by the caller"),
            Token::Compare(op) => Err(QueryError {
                message: format!("`{op}` needs a field before it"),
                at: span,
            }),
            Token::Word(word, quoted) => {
                self.at += 1;
                if let Some(Token::Compare(op)) = self.peek().cloned() {
                    let op_span = self.tokens[self.at].1.clone();
                    self.at += 1;
                    let Some((Token::Word(value, _), value_span)) =
                        self.tokens.get(self.at).cloned()
                    else {
                        return Err(QueryError {
                            message: format!("`{op}` needs a value after it"),
                            at: op_span,
                        });
                    };
                    self.at += 1;
                    let op = match op {
                        "<" => Op::Less,
                        "<=" => Op::LessOrEqual,
                        ">" => Op::Greater,
                        ">=" => Op::GreaterOrEqual,
                        "!=" => Op::Not,
                        _ => Op::Is,
                    };
                    let field = self.grammar.field_named(&word).ok_or_else(|| QueryError {
                        message: format!("no field `{word}`"),
                        at: span.clone(),
                    })?;
                    let value = self.value(field, &value, value_span)?;
                    return Ok(Expr::Term(field_term(field, op, Some(value))));
                }
                if quoted {
                    return Ok(Expr::Term(Term::Word(word)));
                }
                self.word(word, span)
            }
        }
    }

    /// An unquoted word: a negation, a tag, a field, or a bare word.
    fn word(&mut self, word: String, span: Range<usize>) -> Result<Expr, QueryError> {
        if let Some(rest) = word.strip_prefix('!').filter(|r| !r.is_empty()) {
            let inner = self.word(rest.to_string(), span.start + 1..span.end)?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        if !self.grammar.no_tags && word.len() > 1 {
            if let Some(tag) = word.strip_prefix('+') {
                return Ok(Expr::Term(Term::Tag(tag.to_string())));
            }
            if let Some(tag) = word.strip_prefix('-') {
                // `-2d` and `-5` are words, not tags.
                if !tag.starts_with(|c: char| c.is_ascii_digit()) {
                    return Ok(Expr::Not(Box::new(Expr::Term(Term::Tag(tag.to_string())))));
                }
            }
        }
        if let Some(colon) = word.find(':') {
            let (head, value) = (&word[..colon], &word[colon + 1..]);
            let (name, modifier) = match head.split_once('.') {
                Some((name, modifier)) => (name, Some(modifier)),
                None => (head, None),
            };
            if let Some(field) = self.grammar.field_named(name) {
                let op = match modifier.map(str::to_ascii_lowercase).as_deref() {
                    None => Op::Matches,
                    Some("before" | "below" | "under" | "lt") => Op::Less,
                    Some("after" | "above" | "over" | "gt") => Op::Greater,
                    Some("is" | "equals" | "eq") => Op::Is,
                    Some("not" | "isnt" | "ne") => Op::Not,
                    Some("has" | "contains") => Op::Has,
                    Some("hasnt") => Op::Hasnt,
                    Some("startswith" | "left") => Op::StartsWith,
                    Some("endswith" | "right") => Op::EndsWith,
                    Some("none") => Op::Missing,
                    Some("any") => Op::Present,
                    Some(other) => {
                        let at = span.start + name.len() + 1;
                        return Err(QueryError {
                            message: format!("no modifier `{other}`"),
                            at: at..at + other.len(),
                        });
                    }
                };
                let at = span.start + colon + 1;
                let value = match op {
                    Op::Missing | Op::Present => None,
                    _ if value.is_empty() => None,
                    _ => Some(self.value(field, value, at..span.end)?),
                };
                if value.is_none()
                    && matches!(op, Op::Less | Op::Greater | Op::StartsWith | Op::EndsWith)
                {
                    return Err(QueryError {
                        message: format!("`{head}:` needs a value"),
                        at: span,
                    });
                }
                return Ok(Expr::Term(field_term(field, op, value)));
            }
            if !self.grammar.lenient
                && !name.is_empty()
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                return Err(QueryError {
                    message: format!("no field `{name}`"),
                    at: span.start..span.start + name.len(),
                });
            }
        }
        if let Some(matcher) = self.grammar.words.as_ref().and_then(|words| words(&word)) {
            return Ok(Expr::Term(Term::Custom(matcher)));
        }
        Ok(Expr::Term(Term::Word(word)))
    }

    /// `text` as a value of `field`.
    fn value(&self, field: &Field, text: &str, at: Range<usize>) -> Result<Value, QueryError> {
        let read = if let Some(reader) = &field.reader {
            reader(text)
        } else {
            match field.kind {
                Kind::Text => Some(Value::Text(text.to_string())),
                Kind::Number => text.parse::<f64>().ok().map(Value::Number),
                Kind::Bool => match text.to_ascii_lowercase().as_str() {
                    "yes" | "true" | "1" | "on" | "y" => Some(Value::Bool(true)),
                    "no" | "false" | "0" | "off" | "n" => Some(Value::Bool(false)),
                    _ => None,
                },
                Kind::Date => match &self.grammar.dates {
                    Some(dates) => dates(text),
                    None => text.parse::<DateTime>().ok().map(Value::Date),
                },
            }
        };
        read.ok_or_else(|| QueryError {
            message: format!(
                "`{text}` is not {} for `{}`",
                match field.kind {
                    Kind::Text => "a value",
                    Kind::Number => "a number",
                    Kind::Date => "a date",
                    Kind::Bool => "yes or no",
                },
                field.name
            ),
            at,
        })
    }
}

fn field_term(field: &Field, op: Op, value: Option<Value>) -> Term {
    Term::Field {
        name: field.name.clone(),
        kind: field.kind,
        matching: field.matching,
        op,
        value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich_interact::datetime::Time;

    struct Row {
        project: Option<&'static str>,
        cpu: f64,
        due: Option<Date>,
        done: bool,
        tags: Vec<&'static str>,
        text: &'static str,
    }

    impl Record for Row {
        fn field(&self, name: &str) -> Option<Value> {
            match name {
                "project" => self.project.map(Value::from),
                "cpu" => Some(Value::Number(self.cpu)),
                "due" => self.due.map(Value::from),
                "done" => Some(Value::Bool(self.done)),
                _ => None,
            }
        }
        fn text(&self) -> String {
            self.text.into()
        }
        fn tags(&self) -> Vec<String> {
            self.tags.iter().map(|t| t.to_string()).collect()
        }
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                project: Some("home.garden"),
                cpu: 12.5,
                due: Some(Date::new(2026, 10, 21)),
                done: false,
                tags: vec!["next"],
                text: "Plant the bulbs",
            },
            Row {
                project: Some("work"),
                cpu: 80.0,
                due: Some(Date::new(2026, 11, 2)),
                done: true,
                tags: vec![],
                text: "Write the report",
            },
            Row {
                project: None,
                cpu: 0.0,
                due: None,
                done: false,
                tags: vec!["Next", "call"],
                text: "Call the plumber about the leak",
            },
        ]
    }

    fn grammar() -> Grammar {
        let now = DateTime::new(Date::new(2026, 10, 19), Time::new(12, 0, 0));
        Grammar::new()
            .field("project", Kind::Text)
            .matching(TextMatch::Prefix)
            .alias("proj", "project")
            .field("cpu", Kind::Number)
            .field("due", Kind::Date)
            .field("done", Kind::Bool)
            .dates(move |text| crate::dates::parse(text, now).ok())
    }

    /// The indexes of the rows `text` matches.
    fn hits(text: &str) -> Vec<usize> {
        let query = parse(text, &grammar()).unwrap_or_else(|e| panic!("{text:?}: {e}"));
        rows()
            .iter()
            .enumerate()
            .filter(|(_, r)| query.matches(*r))
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn words_fields_and_tags() {
        assert_eq!(hits(""), vec![0, 1, 2]);
        assert_eq!(hits("the"), vec![0, 1, 2]);
        assert_eq!(hits("PLANT"), vec![0]);
        assert_eq!(hits("\"the report\""), vec![1]);
        assert_eq!(hits("project:home"), vec![0]);
        assert_eq!(hits("proj:work"), vec![1]);
        assert_eq!(hits("project:"), vec![2]);
        assert_eq!(hits("project.none:"), vec![2]);
        assert_eq!(hits("project.any:"), vec![0, 1]);
        assert_eq!(hits("project.not:work"), vec![0, 2]);
        assert_eq!(hits("project.has:gard"), vec![0]);
        assert_eq!(hits("+next"), vec![0, 2]);
        assert_eq!(hits("-next"), vec![1]);
        assert_eq!(hits("+call +next"), vec![2]);
        assert_eq!(hits("done:yes"), vec![1]);
    }

    #[test]
    fn comparisons_numbers_and_dates() {
        assert_eq!(hits("cpu.above:10"), vec![0, 1]);
        assert_eq!(hits("cpu > 50"), vec![1]);
        assert_eq!(hits("cpu<=12.5"), vec![0, 2]);
        assert_eq!(hits("cpu != 0"), vec![0, 1]);
        assert_eq!(hits("cpu=80"), vec![1]);
        assert_eq!(hits("due.before:eow"), vec![0]);
        assert_eq!(hits("due.after:eom"), vec![1]);
        assert_eq!(hits("due:2026-10-21"), vec![0]);
        assert_eq!(hits("due:wednesday"), vec![0]);
    }

    #[test]
    fn and_or_not_and_parentheses() {
        assert_eq!(hits("plant or report"), vec![0, 1]);
        assert_eq!(hits("plant or report and +next"), vec![0]);
        assert_eq!(hits("(plant or report) and -next"), vec![1]);
        assert_eq!(hits("not plant"), vec![1, 2]);
        assert_eq!(hits("!plant"), vec![1, 2]);
        assert_eq!(hits("NOT (project:home OR project:work)"), vec![2]);
        assert_eq!(hits("\"and\""), Vec::<usize>::new());
        assert_eq!(hits("the and leak"), vec![2]);
    }

    #[test]
    fn errors_have_positions() {
        let error = |text: &str| parse(text, &grammar()).unwrap_err();
        assert_eq!(error("cpu.above:lots").at, 10..14);
        assert_eq!(
            error("cpu.above:lots").message,
            "`lots` is not a number for `cpu`"
        );
        assert_eq!(error("due.before:whenever").at, 11..19);
        assert_eq!(error("size:3").at, 0..4);
        assert_eq!(error("cpu.around:3").message, "no modifier `around`");
        assert_eq!(error("cpu.around:3").at, 4..10);
        assert_eq!(error("(plant or report").at, 0..16);
        assert_eq!(error("plant)").at, 5..6);
        assert_eq!(error("plant or").at, 6..8);
        assert_eq!(error("and plant").at, 0..3);
        assert_eq!(error("\"plant").at, 0..6);
        assert_eq!(error("> 3").at, 0..1);
        assert_eq!(error("size > 3").message, "no field `size`");
        assert_eq!(error("cpu >").at, 4..5);
        assert_eq!(error("not").at, 0..3);
        assert_eq!(error("or plant").message, "nothing before `or`");
        assert_eq!(error("()").message, "nothing in the parentheses");
        assert_eq!(error(")").at, 0..1);
        // A lenient grammar reads an unknown field as a word.
        let lenient = grammar().lenient();
        let url = parse("http://example.com", &lenient).unwrap();
        assert!(!url.matches(&rows()[0]));
    }

    #[test]
    fn words_of_their_own() {
        // Ids, as Taskwarrior's `2,3`: the grammar's words.
        let grammar = grammar().words(|word| {
            let ids: Option<Vec<usize>> = word.split(',').map(|n| n.parse().ok()).collect();
            let ids = ids?;
            Some(Rc::new(move |r: &dyn Record| {
                let text = r.text();
                let id = ["Plant the bulbs", "Write the report"]
                    .iter()
                    .position(|t| *t == text)
                    .map_or(3, |i| i + 1);
                ids.contains(&id)
            }) as Matcher)
        });
        let query = parse("1,3", &grammar).unwrap();
        let hit: Vec<bool> = rows().iter().map(|r| query.matches(r)).collect();
        assert_eq!(hit, vec![true, false, true]);
        assert_eq!(
            grammar.field_names(),
            vec!["project", "cpu", "due", "done", "proj"]
        );
    }
}
