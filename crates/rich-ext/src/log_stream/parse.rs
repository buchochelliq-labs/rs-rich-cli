//! Reading log lines: a [`LineParser`] per format, a [`Parsers`] registry,
//! and the [`Assembler`] that turns lines into records.
//!
//! The built-in parsers, in the order [`Parsers::new`] tries them:
//!
//! | name | reads |
//! |------|-------|
//! | `json` (`data` feature) | a JSON value: an object's keys are the fields; any other value is the message |
//! | `access` (`nginx`, `apache`, `clf`, `combined`) | the common and combined access log formats, the level from the status (5xx error, 4xx warn) |
//! | `timestamp` (`rfc3339`) | a line that starts with an ISO 8601 time, bracketed or not, then logfmt pairs, a level and message, or a message |
//! | `logfmt` | `key=value` pairs, values quoted with `"…"` where they hold spaces |
//! | `level` | a line that starts with a level (`ERROR …`, `[warn] …`, `INFO:app:…`) |
//!
//! A message's trailing `key=value` words become fields (`ERROR failed
//! host=db` has the message `failed` and the field `host`), and a target
//! before it (`INFO app::db: query`, Python's `INFO:app.db:query`) the
//! field `target`.

use std::borrow::Cow;
use std::fmt;

use super::filter::severity_named;
use super::time::{clf_prefix, iso_prefix};
use super::{Record, LEVEL_KEYS, MESSAGE_KEYS, TIME_KEYS};
use crate::event::{Severity, Value};

/// Reads one log format.
pub trait LineParser: Send + Sync {
    /// The name `--parser` and [`Parsers::only`] know it by, and the
    /// [`Record::format`] it gives.
    fn name(&self) -> &str;

    /// Other names it answers to.
    fn aliases(&self) -> &[&str] {
        &[]
    }

    /// The record `line` holds, or `None` when the line is not in this
    /// format. `line` has no line break.
    fn parse(&self, line: &str) -> Option<Record>;
}

fn answers(parser: &dyn LineParser, name: &str) -> bool {
    parser.name() == name || parser.aliases().contains(&name)
}

/// An ordered set of parsers: the first that reads a line wins. See the
/// [module docs](self) for the built-in ones.
pub struct Parsers {
    parsers: Vec<Box<dyn LineParser>>,
}

impl Default for Parsers {
    fn default() -> Parsers {
        let mut parsers = Parsers::empty();
        #[cfg(feature = "data")]
        parsers.push(JsonParser);
        parsers.push(AccessLog);
        parsers.push(Timestamped);
        parsers.push(Logfmt);
        parsers.push(LevelLine);
        parsers
    }
}

impl fmt::Debug for Parsers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.names()).finish()
    }
}

impl Parsers {
    /// The built-in parsers.
    pub fn new() -> Parsers {
        Parsers::default()
    }

    /// No parsers: every line is text.
    pub fn empty() -> Parsers {
        Parsers {
            parsers: Vec::new(),
        }
    }

    /// Try `parser` before the others.
    pub fn with_parser(mut self, parser: impl LineParser + 'static) -> Parsers {
        self.parsers.insert(0, Box::new(parser));
        self
    }

    /// Try `parser` after the others.
    pub fn push(&mut self, parser: impl LineParser + 'static) {
        self.parsers.push(Box::new(parser));
    }

    /// Keep only the parsers that answer to one of `names` (by name or
    /// alias), in their order here.
    pub fn only(mut self, names: &[&str]) -> Parsers {
        self.parsers
            .retain(|parser| names.iter().any(|name| answers(&**parser, name)));
        self
    }

    /// The parsers' names, in order.
    pub fn names(&self) -> Vec<&str> {
        self.parsers.iter().map(|parser| parser.name()).collect()
    }

    /// The parser that answers to `name`.
    pub fn get(&self, name: &str) -> Option<&dyn LineParser> {
        self.parsers
            .iter()
            .find(|parser| answers(&***parser, name))
            .map(|parser| &**parser)
    }

    /// Whether a parser answers to `name`.
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// `line` read by the first parser that reads it.
    pub fn parse(&self, line: &str) -> Option<Record> {
        self.parsers.iter().find_map(|parser| parser.parse(line))
    }
}

fn space(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'@' | b'/')
}

/// The `key=value` pairs of `text`, when it is nothing else: values may be
/// `"quoted"` (with `\"`, `\\`, `\n`, `\t` and `\r` escapes) and may be
/// empty (`key=`). A key starts with a letter, `_` or `@` and holds
/// letters, digits and `_ . - @ /`. A bare word or an unclosed quote makes
/// it `None`.
///
/// ```
/// use rich_ext::log_stream::logfmt_pairs;
///
/// assert_eq!(
///     logfmt_pairs(r#"level=info msg="hello world" n=3"#).unwrap(),
///     [("level".into(), "info".into()), ("msg".into(), "hello world".into()), ("n".into(), "3".into())]
/// );
/// assert_eq!(logfmt_pairs("hello world"), None);
/// ```
pub fn logfmt_pairs(text: &str) -> Option<Vec<(String, String)>> {
    let bytes = text.as_bytes();
    let mut pairs = Vec::new();
    let mut at = 0;
    loop {
        while at < bytes.len() && space(bytes[at]) {
            at += 1;
        }
        if at == bytes.len() {
            break;
        }
        let start = at;
        if !(bytes[at].is_ascii_alphabetic() || matches!(bytes[at], b'_' | b'@')) {
            return None;
        }
        while at < bytes.len() && key_byte(bytes[at]) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            return None;
        }
        let key = text[start..at].to_string();
        at += 1;
        let value = if bytes.get(at) == Some(&b'"') {
            let (value, used) = quoted(&text[at + 1..])?;
            at += 1 + used;
            if at < bytes.len() && !space(bytes[at]) {
                return None;
            }
            value
        } else {
            let start = at;
            while at < bytes.len() && !space(bytes[at]) {
                at += 1;
            }
            text[start..at].to_string()
        };
        pairs.push((key, value));
    }
    Some(pairs)
}

/// A quoted value's text up to its closing `"`, unescaped, and the bytes
/// taken (the closing quote included).
fn quoted(text: &str) -> Option<(String, usize)> {
    let mut value = String::new();
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((value, at + 1)),
            '\\' => match chars.next()?.1 {
                'n' => value.push('\n'),
                't' => value.push('\t'),
                'r' => value.push('\r'),
                other @ ('"' | '\\') => value.push(other),
                other => {
                    value.push('\\');
                    value.push(other);
                }
            },
            c => value.push(c),
        }
    }
    None
}

/// `text` split into a message and the `key=value` words that end it.
fn trailing_pairs(text: &str) -> (&str, Vec<(String, String)>) {
    let text = text.trim_end();
    let mut cut = text.len();
    let mut pairs = Vec::new();
    loop {
        let head = text[..cut].trim_end();
        let start = head.rfind([' ', '\t']).map_or(0, |space| space + 1);
        let word = &head[start..];
        if word.is_empty() || !word.contains('=') {
            break;
        }
        match logfmt_pairs(word) {
            Some(mut found) if found.len() == 1 => pairs.push(found.remove(0)),
            _ => break,
        }
        cut = start;
        if start == 0 {
            break;
        }
    }
    pairs.reverse();
    (text[..cut].trim_end(), pairs)
}

/// `true` for the characters a target (`app::db`, `app.db`) is made of.
fn target_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | ':' | '-' | '/')
}

fn string(text: impl Into<String>) -> Value {
    Value::String(text.into())
}

/// Reads logfmt: `time=… level=info msg="hello" user=ann`. A line needs two
/// pairs, or one whose key is a message, level or time key, so a sentence
/// holding one `a=b` is not read as logfmt.
#[derive(Clone, Copy, Debug, Default)]
pub struct Logfmt;

impl LineParser for Logfmt {
    fn name(&self) -> &str {
        "logfmt"
    }

    fn parse(&self, line: &str) -> Option<Record> {
        let pairs = logfmt_pairs(line)?;
        let known = pairs.iter().any(|(key, _)| {
            let key = key.as_str();
            MESSAGE_KEYS.contains(&key) || LEVEL_KEYS.contains(&key) || TIME_KEYS.contains(&key)
        });
        if pairs.is_empty() || (pairs.len() < 2 && !known) {
            return None;
        }
        let mut record = Record::new("logfmt");
        for (key, value) in pairs {
            record.set(key, string(value));
        }
        Some(record)
    }
}

/// The level word at the start of `line` (bracketed or not), its severity,
/// and what follows it. At the start of a line (`strict`) a bare word must
/// be upper case (`ERROR`) or end with a colon (`Warning:`), so prose that
/// opens with one (`Error handling is hard`) is not read as a level; after
/// a time any case will do.
fn level_prefix(line: &str, strict: bool) -> Option<(&str, Severity, &str)> {
    let text = line.trim_start();
    let (close, inner) = match text.as_bytes().first() {
        Some(b'[') => (Some(']'), &text[1..]),
        Some(b'<') => (Some('>'), &text[1..]),
        Some(b'(') => (Some(')'), &text[1..]),
        _ => (None, text),
    };
    let inner = inner.trim_start();
    let length = inner
        .bytes()
        .take_while(u8::is_ascii_alphabetic)
        .take(12)
        .count();
    let word = &inner[..length];
    let severity = severity_named(word)?;
    let mut after = &inner[length..];
    match close {
        Some(close) => after = after.trim_start().strip_prefix(close)?,
        None => {
            if after
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                return None;
            }
            let shouted = word.bytes().all(|b| b.is_ascii_uppercase());
            if strict && !shouted && !after.starts_with(':') {
                return None;
            }
        }
    }
    Some((word, severity, after))
}

/// Python's `%(name)s - %(levelname)s - %(message)s`: a level among the
/// first parts of `text` split at ` - ` (or ` | `), the parts before it the
/// target.
fn dashed_level(record: &mut Record, text: &str) -> bool {
    let text = text
        .strip_prefix("- ")
        .or_else(|| text.strip_prefix("| "))
        .unwrap_or(text);
    for separator in [" - ", " | "] {
        let parts: Vec<&str> = text.split(separator).collect();
        for index in 0..parts.len().saturating_sub(1).min(3) {
            let word = parts[index].trim();
            let shouted = word.bytes().all(|b| b.is_ascii_uppercase());
            if !shouted || severity_named(word).is_none() {
                continue;
            }
            record.set("level", string(word));
            if index > 0 {
                record.set("target", string(parts[..index].join(separator)));
            }
            let taken: usize = parts[..=index]
                .iter()
                .map(|part| part.len() + separator.len())
                .sum();
            let (message, pairs) = trailing_pairs(&text[taken.min(text.len())..]);
            record.set("message", string(message));
            for (key, value) in pairs {
                record.set(key, string(value));
            }
            return true;
        }
    }
    false
}

/// The fields of what follows a level: a target, the message and its
/// trailing pairs.
fn after_level(record: &mut Record, rest: &str) {
    let mut rest = rest;
    let mut target = None;
    // Python's `LEVEL:logger:message`.
    if let Some(after) = rest.strip_prefix(':') {
        rest = after;
        if let Some((name, message)) = after.split_once(':') {
            if !name.is_empty() && name.chars().all(target_char) && !name.contains("::") {
                target = Some(name);
                rest = message;
            }
        }
    }
    let mut rest = rest.trim_start();
    for separator in ["- ", "| ", ": "] {
        if let Some(after) = rest.strip_prefix(separator) {
            rest = after.trim_start();
            break;
        }
    }
    // tracing's `target: message`.
    if target.is_none() {
        let first = rest.split([' ', '\t']).next().unwrap_or_default();
        if let Some(name) = first.strip_suffix(':') {
            if name.contains("::") && name.chars().all(target_char) {
                target = Some(name);
                rest = rest[first.len()..].trim_start();
            }
        }
    }
    if let Some(target) = target {
        record.set("target", string(target));
    }
    let (message, pairs) = trailing_pairs(rest);
    record.set("message", string(message));
    for (key, value) in pairs {
        record.set(key, string(value));
    }
}

/// Reads a line that starts with a level: `ERROR request failed`, `[warn]
/// disk 91% full`, `INFO app::db: query`, Python's `INFO:root:started`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LevelLine;

impl LineParser for LevelLine {
    fn name(&self) -> &str {
        "level"
    }

    fn parse(&self, line: &str) -> Option<Record> {
        let (word, _, rest) = level_prefix(line, true)?;
        let mut record = Record::new("level").field("level", string(word));
        after_level(&mut record, rest);
        Some(record)
    }
}

/// Reads a line that starts with an ISO 8601 time, bracketed or not:
/// `2026-10-18T10:00:00Z INFO started`, `[2026-10-18 10:00:00,123] ERROR
/// …`, a time and logfmt pairs, Python's `time - name - LEVEL - message`,
/// Monolog's `[time] app.ERROR: message`, and the `2026/10/18 10:00:00`
/// of nginx's error log and Go's `log` package.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timestamped;

impl LineParser for Timestamped {
    fn name(&self) -> &str {
        "timestamp"
    }

    fn aliases(&self) -> &[&str] {
        &["rfc3339"]
    }

    fn parse(&self, line: &str) -> Option<Record> {
        let text = line.trim_start();
        let (bracketed, inner) = match text.strip_prefix('[') {
            Some(inner) => (true, inner),
            None => (false, text),
        };
        let (_, used) = iso_prefix(inner)?;
        let time = &inner[..used];
        let mut rest = &inner[used..];
        if bracketed {
            rest = rest.strip_prefix(']')?;
        } else if rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
            return None;
        }
        let rest = rest.trim();
        let mut record = Record::new("timestamp").field("timestamp", string(time));
        if rest.is_empty() {
            return Some(record);
        }
        if let Some(pairs) = logfmt_pairs(rest) {
            for (key, value) in pairs {
                record.set(key, string(value));
            }
            return Some(record);
        }
        if let Some((word, _, after)) = level_prefix(rest, false) {
            record.set("level", string(word));
            after_level(&mut record, after);
            return Some(record);
        }
        if dashed_level(&mut record, rest) {
            return Some(record);
        }
        // Monolog: `app.ERROR: message`.
        let first = rest.split([' ', '\t']).next().unwrap_or_default();
        if let Some((channel, level)) = first
            .strip_suffix(':')
            .and_then(|name| name.rsplit_once('.'))
        {
            if severity_named(level).is_some() && !channel.is_empty() {
                record.set("level", string(level));
                record.set("target", string(channel));
                let (message, pairs) = trailing_pairs(rest[first.len()..].trim_start());
                record.set("message", string(message));
                for (key, value) in pairs {
                    record.set(key, string(value));
                }
                return Some(record);
            }
        }
        let (message, pairs) = trailing_pairs(rest);
        record.set("message", string(message));
        for (key, value) in pairs {
            record.set(key, string(value));
        }
        Some(record)
    }
}

/// Reads the common and combined access log formats nginx and Apache
/// write: `127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /a.gif
/// HTTP/1.0" 200 2326 "http://example.com/" "Mozilla/4.08"`. The fields
/// are `remote_addr`, `ident` and `user` (when not `-`), `time`, `method`,
/// `path` and `protocol` (or `request`, when it is not three words),
/// `status` and `bytes` as numbers, `referer` and `user_agent`; the level
/// is `error` for a 5xx status, `warn` for 4xx and `info` otherwise, and
/// the message `GET /a.gif 200`.
#[derive(Clone, Copy, Debug, Default)]
pub struct AccessLog;

impl AccessLog {
    /// The next space-separated word of `text` and what follows it.
    fn word(text: &str) -> Option<(&str, &str)> {
        let text = text.trim_start_matches(' ');
        let end = text.find(' ').unwrap_or(text.len());
        (end > 0).then(|| (&text[..end], &text[end..]))
    }

    /// A `"…"` field (with `\"` escapes) at the start of `text`.
    fn quoted(text: &str) -> Option<(String, &str)> {
        let text = text.trim_start_matches(' ').strip_prefix('"')?;
        let (value, used) = quoted(text)?;
        Some((value, &text[used..]))
    }
}

impl LineParser for AccessLog {
    fn name(&self) -> &str {
        "access"
    }

    fn aliases(&self) -> &[&str] {
        &["nginx", "apache", "clf", "combined"]
    }

    fn parse(&self, line: &str) -> Option<Record> {
        let (remote, rest) = Self::word(line)?;
        let (ident, rest) = Self::word(rest)?;
        let (user, rest) = Self::word(rest)?;
        let rest = rest.strip_prefix(" [")?;
        let (_, used) = clf_prefix(rest)?;
        let time = &rest[..used];
        let rest = rest[used..].strip_prefix(']')?;
        let (request, rest) = Self::quoted(rest)?;
        let (status, rest) = Self::word(rest)?;
        if status.len() != 3 || !status.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let status: i64 = status.parse().ok()?;
        let (bytes, rest) = Self::word(rest)?;
        if bytes != "-" && !bytes.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let mut record = Record::new("access").field("remote_addr", string(remote));
        if ident != "-" {
            record.set("ident", string(ident));
        }
        if user != "-" {
            record.set("user", string(user));
        }
        record.set("time", string(time));
        let parts: Vec<&str> = request.split(' ').collect();
        let what = match parts.as_slice() {
            [method, path, protocol] => {
                record.set("method", string(*method));
                record.set("path", string(*path));
                record.set("protocol", string(*protocol));
                format!("{method} {path}")
            }
            _ => {
                record.set("request", string(request.clone()));
                request.clone()
            }
        };
        record.set("status", Value::Integer(status));
        if let Ok(bytes) = bytes.parse::<i64>() {
            record.set("bytes", Value::Integer(bytes));
        }
        if let Some((referer, rest)) = Self::quoted(rest) {
            if referer != "-" {
                record.set("referer", string(referer));
            }
            if let Some((agent, _)) = Self::quoted(rest) {
                if agent != "-" {
                    record.set("user_agent", string(agent));
                }
            }
        }
        let level = match status {
            500.. => "error",
            400..=499 => "warn",
            _ => "info",
        };
        record.set("level", string(level));
        record.set("message", string(format!("{what} {status}")));
        Some(record)
    }
}

/// Reads a JSON value: an object's keys are the fields, in order, and any
/// other value is the message, as compact JSON.
#[cfg(feature = "data")]
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonParser;

#[cfg(feature = "data")]
impl JsonParser {
    /// A JSON value as an event [`Value`]: numbers as integers when they
    /// are whole and fit, else as floats.
    pub fn value(value: serde_json::Value) -> Value {
        match value {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(v) => Value::Bool(v),
            serde_json::Value::Number(v) => {
                if let Some(v) = v.as_i64() {
                    Value::Integer(v)
                } else if let Some(v) = v.as_u64() {
                    Value::Unsigned(v)
                } else {
                    Value::Float(v.as_f64().unwrap_or_default())
                }
            }
            serde_json::Value::String(v) => Value::String(v),
            serde_json::Value::Array(v) => Value::List(v.into_iter().map(Self::value).collect()),
            serde_json::Value::Object(v) => Value::Map(
                v.into_iter()
                    .map(|(key, value)| (key, Self::value(value)))
                    .collect(),
            ),
        }
    }

    /// The record a parsed JSON value holds.
    pub fn record(value: serde_json::Value) -> Record {
        match value {
            serde_json::Value::Object(map) => Record {
                fields: map
                    .into_iter()
                    .map(|(key, value)| (key, Self::value(value)))
                    .collect(),
                format: Cow::Borrowed("json"),
                continuation: Vec::new(),
            },
            other => Record::new("json").field("message", Value::String(other.to_string())),
        }
    }
}

#[cfg(feature = "data")]
impl LineParser for JsonParser {
    fn name(&self) -> &str {
        "json"
    }

    fn parse(&self, line: &str) -> Option<Record> {
        let text = line.trim();
        let first = *text.as_bytes().first()?;
        if !matches!(
            first,
            b'{' | b'[' | b'"' | b'-' | b'0'..=b'9' | b't' | b'f' | b'n'
        ) {
            return None;
        }
        serde_json::from_str(text).ok().map(Self::record)
    }
}

/// What [`Assembler::push`] completed.
#[derive(Clone, Debug, PartialEq)]
pub enum Assembled {
    /// A record, with the lines that followed it that no parser read.
    Record(Record),
    /// A line no parser read, after the record it follows was already
    /// given out (by [`Assembler::flush`], or past
    /// [`Assembler::continuation_limit`]): it belongs under that record.
    Continuation(String),
}

/// Turns lines into records: each line is read by the first parser that
/// can, and a line none can read joins the record before it (when that
/// record [takes continuation lines](Record::takes_continuation)) or is a
/// text record of its own. Blank lines are skipped, and ANSI colour
/// sequences are taken out before parsing.
///
/// A record is given out when the next record starts, so a caller reading
/// a live stream calls [`flush`](Self::flush) when the input goes quiet;
/// lines that then still belong to it arrive as
/// [`Assembled::Continuation`].
///
/// ```
/// use rich_ext::log_stream::{Assembled, Assembler, Parsers};
///
/// let mut lines = Assembler::new(Parsers::new());
/// assert_eq!(lines.push("ERROR request failed"), None);
/// assert_eq!(lines.push("Traceback (most recent call last):"), None);
/// let Some(Assembled::Record(failed)) = lines.push("INFO retrying") else {
///     panic!()
/// };
/// assert_eq!(failed.continuation, ["Traceback (most recent call last):"]);
/// assert!(lines.flush().is_some());
/// ```
#[derive(Debug)]
pub struct Assembler {
    parsers: Parsers,
    pending: Option<Record>,
    /// Whether a line no parser reads joins the last record given out.
    attached: bool,
    limit: usize,
}

impl Assembler {
    /// The continuation lines a record keeps by default.
    pub const CONTINUATION_LIMIT: usize = 1_000;

    pub fn new(parsers: Parsers) -> Assembler {
        Assembler {
            parsers,
            pending: None,
            attached: false,
            limit: Assembler::CONTINUATION_LIMIT,
        }
    }

    /// Keep at most `limit` continuation lines in a record; once it has
    /// them, it is given out and the lines after it arrive as
    /// [`Assembled::Continuation`].
    pub fn continuation_limit(mut self, limit: usize) -> Assembler {
        self.limit = limit.max(1);
        self
    }

    pub fn parsers(&self) -> &Parsers {
        &self.parsers
    }

    /// Read one line (a trailing `\r` or `\n` is ignored), and give out
    /// what it completed.
    pub fn push(&mut self, line: &str) -> Option<Assembled> {
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line = strip_colours(line);
        if line.trim().is_empty() {
            return None;
        }
        if let Some(record) = self.parsers.parse(&line) {
            return self.replace(record).map(Assembled::Record);
        }
        match &mut self.pending {
            Some(pending) if pending.takes_continuation() => {
                pending.continuation.push(line.into_owned());
                if pending.continuation.len() >= self.limit {
                    self.attached = true;
                    return self.pending.take().map(Assembled::Record);
                }
                None
            }
            Some(_) => self
                .replace(Record::text(line.into_owned()))
                .map(Assembled::Record),
            None if self.attached => Some(Assembled::Continuation(line.into_owned())),
            None => {
                self.pending = Some(Record::text(line.into_owned()));
                None
            }
        }
    }

    fn replace(&mut self, record: Record) -> Option<Record> {
        let done = self.pending.replace(record);
        if let Some(done) = &done {
            self.attached = done.takes_continuation();
        }
        done
    }

    /// Give out the record being read, at the end of the input or when it
    /// goes quiet.
    pub fn flush(&mut self) -> Option<Record> {
        let done = self.pending.take();
        if let Some(done) = &done {
            self.attached = done.takes_continuation();
        }
        done
    }
}

/// `line` without ANSI CSI sequences (colours, cursor moves).
fn strip_colours(line: &str) -> Cow<'_, str> {
    if !line.contains('\x1b') {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            // Parameters and intermediates, then one final byte.
            for c in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&c) {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(record: &Record, key: &str) -> Option<String> {
        record.get(key).map(|value| match value {
            Value::String(text) => text.clone(),
            other => other.format(false, 0),
        })
    }

    fn keys(record: &Record) -> Vec<&str> {
        record.fields.iter().map(|(k, _)| k.as_str()).collect()
    }

    #[test]
    fn logfmt_reads_pairs_and_refuses_prose() {
        let record = Logfmt
            .parse(r#"time=2026-10-18T10:00:00Z level=info msg="hi \"there\"" empty= path=/a=b"#)
            .unwrap();
        assert_eq!(keys(&record), ["time", "level", "msg", "empty", "path"]);
        assert_eq!(text(&record, "msg").as_deref(), Some("hi \"there\""));
        assert_eq!(text(&record, "empty").as_deref(), Some(""));
        assert_eq!(text(&record, "path").as_deref(), Some("/a=b"));
        assert_eq!(record.format, "logfmt");
        // One pair needs a known key.
        assert!(Logfmt.parse("msg=alone").is_some());
        assert!(Logfmt.parse("x=1").is_none());
        for bad in [
            "hello world",
            "a=1 bare",
            r#"a="unclosed"#,
            r#"a="x"y b=1"#,
            "=1 b=2",
            "1a=2 b=3",
            "",
            "   ",
        ] {
            assert!(Logfmt.parse(bad).is_none(), "{bad:?}");
        }
        // Duplicate keys keep the last value.
        let record = Logfmt.parse("a=1 a=2").unwrap();
        assert_eq!(text(&record, "a").as_deref(), Some("2"));
    }

    #[test]
    fn level_lines_with_targets_and_trailing_fields() {
        let record = LevelLine
            .parse("ERROR request failed host=db port=5432")
            .unwrap();
        assert_eq!(text(&record, "level").as_deref(), Some("ERROR"));
        assert_eq!(record.message().as_deref(), Some("request failed"));
        assert_eq!(text(&record, "port").as_deref(), Some("5432"));
        let record = LevelLine.parse("[warn] disk 91% full").unwrap();
        assert_eq!(record.severity(), Some(Severity::Warn));
        assert_eq!(record.message().as_deref(), Some("disk 91% full"));
        let record = LevelLine.parse("INFO:app.db:connected").unwrap();
        assert_eq!(text(&record, "target").as_deref(), Some("app.db"));
        assert_eq!(record.message().as_deref(), Some("connected"));
        let record = LevelLine.parse("WARN app::net: retrying in 5s").unwrap();
        assert_eq!(text(&record, "target").as_deref(), Some("app::net"));
        assert_eq!(record.message().as_deref(), Some("retrying in 5s"));
        let record = LevelLine.parse("Warning: Retrying: attempt 2").unwrap();
        assert_eq!(text(&record, "target"), None);
        assert_eq!(record.message().as_deref(), Some("Retrying: attempt 2"));
        let record = LevelLine.parse("DEBUG - cache warm").unwrap();
        assert_eq!(record.message().as_deref(), Some("cache warm"));
        for bad in [
            "Information is power",
            "Error handling is hard",
            "INFOrmation",
            "[ERROR",
            "hello",
            "",
            "I think so",
        ] {
            assert!(LevelLine.parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn timestamped_lines_in_their_common_shapes() {
        let record = Timestamped
            .parse("2026-10-18T10:00:00.123Z  INFO my_app::db: query took 5ms rows=3")
            .unwrap();
        assert_eq!(
            keys(&record),
            ["timestamp", "level", "target", "message", "rows"]
        );
        assert_eq!(
            text(&record, "timestamp").as_deref(),
            Some("2026-10-18T10:00:00.123Z")
        );
        assert_eq!(record.message().as_deref(), Some("query took 5ms"));
        let record = Timestamped
            .parse("[2026-10-18 10:00:00,123] ERROR boom")
            .unwrap();
        assert_eq!(
            text(&record, "timestamp").as_deref(),
            Some("2026-10-18 10:00:00,123")
        );
        assert_eq!(record.severity(), Some(Severity::Error));
        let record = Timestamped
            .parse("2026-10-18T10:00:00Z level=warn msg=\"slow query\"")
            .unwrap();
        assert_eq!(keys(&record), ["timestamp", "level", "msg"]);
        let record = Timestamped
            .parse("[2026-10-18T10:00:00+00:00] app.ERROR: payment failed")
            .unwrap();
        assert_eq!(text(&record, "target").as_deref(), Some("app"));
        assert_eq!(record.severity(), Some(Severity::Error));
        assert_eq!(record.message().as_deref(), Some("payment failed"));
        let record = Timestamped.parse("2026-10-18 just a note").unwrap();
        assert_eq!(record.message().as_deref(), Some("just a note"));
        let record = Timestamped
            .parse("2026-10-18 10:00:00 error happened")
            .unwrap();
        assert_eq!(record.severity(), Some(Severity::Error));
        let record = Timestamped
            .parse("2026-10-18 10:00:00,123 - app.db - WARNING - pool exhausted size=10")
            .unwrap();
        assert_eq!(
            keys(&record),
            ["timestamp", "level", "target", "message", "size"]
        );
        assert_eq!(text(&record, "target").as_deref(), Some("app.db"));
        assert_eq!(record.message().as_deref(), Some("pool exhausted"));
        let record = Timestamped
            .parse("2026/10/18 10:00:00 [error] 123#0: *1 open() failed")
            .unwrap();
        assert_eq!(record.severity(), Some(Severity::Error));
        assert_eq!(record.message().as_deref(), Some("123#0: *1 open() failed"));
        assert!(record.instant().is_some());
        assert_eq!(
            Timestamped
                .parse("2026-10-18T10:00:00Z")
                .unwrap()
                .fields
                .len(),
            1
        );
        for bad in [
            "2026-10-18T10:00:00Zx",
            "[2026-10-18 x",
            "20261018 x",
            "x 2026-10-18",
        ] {
            assert!(Timestamped.parse(bad).is_none(), "{bad:?}");
        }
        assert!(Parsers::new().get("rfc3339").is_some());
    }

    #[test]
    fn access_logs_common_and_combined() {
        let line = r#"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 404 2326 "http://www.example.com/start.html" "Mozilla/4.08 [en] (Win98; I ;Nav)""#;
        let record = AccessLog.parse(line).unwrap();
        assert_eq!(
            keys(&record),
            [
                "remote_addr",
                "user",
                "time",
                "method",
                "path",
                "protocol",
                "status",
                "bytes",
                "referer",
                "user_agent",
                "level",
                "message"
            ]
        );
        assert_eq!(record.get("status"), Some(&Value::Integer(404)));
        assert_eq!(record.severity(), Some(Severity::Warn));
        assert_eq!(record.message().as_deref(), Some("GET /apache_pb.gif 404"));
        assert_eq!(record.instant(), Some(971_211_336_000_000));
        let common = r#"10.0.0.2 - - [10/Oct/2000:13:55:36 +0000] "-" 500 -"#;
        let record = AccessLog.parse(common).unwrap();
        assert_eq!(text(&record, "request").as_deref(), Some("-"));
        assert_eq!(record.get("bytes"), None);
        assert_eq!(record.severity(), Some(Severity::Error));
        let escaped = r#"::1 - - [10/Oct/2000:13:55:36 +0000] "GET /a\"b HTTP/1.1" 200 5 "-" "-""#;
        let record = AccessLog.parse(escaped).unwrap();
        assert_eq!(text(&record, "path").as_deref(), Some("/a\"b"));
        assert_eq!(text(&record, "referer"), None);
        for bad in [
            "127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] \"GET / HTTP/1.0\" 20 1",
            "127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] \"GET / HTTP/1.0 200 1",
            "127.0.0.1 - - [99/Oct/2000:13:55:36 -0700] \"GET / HTTP/1.0\" 200 1",
            "127.0.0.1 - - \"GET / HTTP/1.0\" 200 1",
            "a b c",
            "",
        ] {
            assert!(AccessLog.parse(bad).is_none(), "{bad:?}");
        }
        for alias in ["nginx", "apache", "clf", "combined"] {
            assert_eq!(Parsers::new().get(alias).map(|p| p.name()), Some("access"));
        }
    }

    #[cfg(feature = "data")]
    #[test]
    fn json_objects_and_other_values() {
        let record = JsonParser
            .parse(r#" {"level":"info","n":1,"big":18446744073709551615,"f":1.5,"tags":["a"],"o":{"k":null}} "#)
            .unwrap();
        assert_eq!(keys(&record), ["level", "n", "big", "f", "tags", "o"]);
        assert_eq!(record.get("big"), Some(&Value::Unsigned(u64::MAX)));
        assert_eq!(record.get("f"), Some(&Value::Float(1.5)));
        let bare = JsonParser.parse("[1, 2]").unwrap();
        assert_eq!(bare.message().as_deref(), Some("[1,2]"));
        assert!(!bare.takes_continuation());
        for bad in ["{", "hello", "{\"a\":}", ""] {
            assert!(JsonParser.parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_registry_tries_parsers_in_order_and_takes_more() {
        struct Upper;
        impl LineParser for Upper {
            fn name(&self) -> &str {
                "upper"
            }
            fn parse(&self, line: &str) -> Option<Record> {
                line.chars()
                    .all(|c| c.is_ascii_uppercase() || c == ' ')
                    .then(|| Record::new("upper").field("message", string(line)))
            }
        }
        let parsers = Parsers::new().with_parser(Upper);
        assert_eq!(parsers.parse("ERROR BOOM").unwrap().format, "upper");
        assert_eq!(Parsers::new().parse("ERROR BOOM").unwrap().format, "level");
        let only = Parsers::new().only(&["logfmt", "nope"]);
        assert_eq!(only.names(), ["logfmt"]);
        assert!(only.parse("ERROR BOOM").is_none());
        assert!(Parsers::empty().parse("a=1 b=2").is_none());
        assert!(format!("{:?}", Parsers::new()).contains("logfmt"));
    }

    #[test]
    fn the_assembler_attaches_continuations_and_keeps_text() {
        let mut lines = Assembler::new(Parsers::new()).continuation_limit(2);
        let mut out = Vec::new();
        for line in [
            "plain text first",
            "\x1b[32mERROR\x1b[0m failed",
            "  at one",
            "",
            "  at two\r",
            "  at three",
            "WARN next",
            "not parsed after warn",
        ] {
            out.extend(lines.push(line));
        }
        let last = lines.flush().unwrap();
        out.extend(lines.push("late line"));
        assert_eq!(out.len(), 4, "{out:#?}");
        let Assembled::Record(text) = &out[0] else {
            panic!("{out:#?}")
        };
        assert!(text.is_text());
        assert_eq!(text.message().as_deref(), Some("plain text first"));
        let Assembled::Record(failed) = &out[1] else {
            panic!("{out:#?}")
        };
        assert_eq!(failed.message().as_deref(), Some("failed"));
        assert_eq!(failed.continuation, ["  at one", "  at two"]);
        // Past the limit, and after a flush, lines arrive on their own.
        assert_eq!(out[2], Assembled::Continuation("  at three".into()));
        assert_eq!(out[3], Assembled::Continuation("late line".into()));
        assert_eq!(last.message().as_deref(), Some("next"));
        assert_eq!(last.continuation, ["not parsed after warn"]);
        // Text after text stays text: each line its own record.
        let mut lines = Assembler::new(Parsers::new());
        assert_eq!(lines.push("one"), None);
        let Some(Assembled::Record(one)) = lines.push("two") else {
            panic!()
        };
        assert_eq!(one.message().as_deref(), Some("one"));
        assert_eq!(lines.flush().unwrap().message().as_deref(), Some("two"));
        assert_eq!(lines.push("three"), None);
    }
}
