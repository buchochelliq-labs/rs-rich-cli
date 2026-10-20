//! Log streams: records read from JSON Lines or plain-text logs, then
//! filtered, collapsed and grouped (#724).
//!
//! A [`Record`] is one entry of a log: its fields under the names the input
//! gave them, in input order, and any lines that followed it that no parser
//! read (a stack trace). [`Record::to_event`] reads it as a
//! [`StructuredEvent`]: the message, the severity and the time from their
//! usual keys ([`MESSAGE_KEYS`], [`LEVEL_KEYS`], [`TIME_KEYS`]), the rest as
//! fields. Around it:
//!
//! - **Parsing:** a [`LineParser`] reads one format; [`Parsers`] holds the
//!   built-in ones (logfmt, `LEVEL message`, timestamped lines and
//!   nginx/apache access logs, and JSON with the `data` feature) and takes
//!   more. [`Assembler`] turns lines into records: a line no parser reads
//!   joins the record before it, and before any record it is a text record,
//!   so nothing is dropped.
//! - **Filtering:** [`LevelFilter`] (`warn+`, `error`), [`Predicate`]
//!   (`service=api`, `latency_ms>500`, `msg~timeout`) and [`Filter`], which
//!   holds both and ANDs the predicates.
//! - **Collapsing:** [`Collapser`] folds a run of identical records (the
//!   time aside) into the first and a count.
//! - **Grouping:** [`GroupBy`] finds a record's trace, span or correlation
//!   id; [`Groups`] gathers records by it, [`GroupHeader`] draws a group's
//!   header and [`Summary`] counts levels and names the groups with errors.
//! - **From `tracing` and `log`:** [`Record::from_event`] reads the
//!   adapters' events as records, and [`ChannelSink`] sends them down a
//!   bounded channel to a log view on another thread, dropping (and
//!   counting) what a full channel cannot take.
//! - **Time:** [`parse_timestamp`] reads RFC 3339, the access log's format
//!   and epoch numbers, and [`Clock`] shows a time as written, relative to
//!   the first record, or not at all.
//!
//! ```
//! use rich_ext::log_stream::{Assembled, Assembler, Filter, LevelFilter, Parsers, Predicate};
//!
//! let mut lines = Assembler::new(Parsers::new());
//! let filter = Filter::new()
//!     .levels("warn+".parse::<LevelFilter>().unwrap())
//!     .predicate(Predicate::parse("latency_ms>500").unwrap());
//! let mut kept = Vec::new();
//! for line in [
//!     "level=info msg=ok latency_ms=12",
//!     "level=warn msg=slow latency_ms=900",
//!     "level=error msg=failed latency_ms=40",
//! ] {
//!     if let Some(Assembled::Record(record)) = lines.push(line) {
//!         if filter.matches(&record) {
//!             kept.push(record);
//!         }
//!     }
//! }
//! kept.extend(lines.flush().filter(|record| filter.matches(record)));
//! assert_eq!(kept.len(), 1);
//! assert_eq!(kept[0].message().as_deref(), Some("slow"));
//! ```

use std::borrow::Cow;

use crate::event::{EventContext, Message, Severity, StructuredEvent, Value};
use crate::redact::Redactor;

mod channel;
mod collapse;
mod filter;
mod group;
mod parse;
mod time;

pub use channel::ChannelSink;
pub use collapse::{Collapser, Push, Repeat};
pub use filter::{severity_of, Filter, FilterError, LevelFilter, Op, Predicate};
pub use group::{Group, GroupBy, GroupHeader, Groups, LevelCounts, Summary};
#[cfg(feature = "data")]
pub use parse::JsonParser;
pub use parse::{
    logfmt_pairs, AccessLog, Assembled, Assembler, LevelLine, LineParser, Logfmt, Parsers,
    Timestamped,
};
pub use time::{format_offset, parse_timestamp, Clock, TimeMode};

/// The keys a record's message is read from, in order.
pub const MESSAGE_KEYS: &[&str] = &["message", "msg"];
/// The keys a record's level is read from, in order.
pub const LEVEL_KEYS: &[&str] = &["level", "severity"];
/// The keys a record's time is read from, in order.
pub const TIME_KEYS: &[&str] = &["timestamp", "time", "@timestamp"];

/// The theme keys log streams draw with, and their default styles.
pub const STYLES: &[(&str, &str)] = &[
    ("log.group", "bold"),
    ("log.group.key", "bold magenta"),
    ("log.group.count", "dim"),
    ("log.repeat", "dim"),
    ("log.continuation", "dim"),
    ("log.summary", "bold"),
    ("log.summary.error", "bold red"),
];

/// One entry of a log stream. See the [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// The fields under the names the input gave them, in input order. A
    /// line no parser read has one, `message`.
    pub fields: Vec<(String, Value)>,
    /// The parser that read it: `json`, `logfmt`, `level`, `timestamp`,
    /// `access`, a parser of your own, or `text` for a line none read.
    pub format: Cow<'static, str>,
    /// Lines that followed it that no parser read, such as a stack trace.
    pub continuation: Vec<String>,
}

impl Record {
    /// A record with no fields, read by the parser named `format`.
    pub fn new(format: impl Into<Cow<'static, str>>) -> Record {
        Record {
            fields: Vec::new(),
            format: format.into(),
            continuation: Vec::new(),
        }
    }

    /// A line no parser read: its `message` is the line.
    pub fn text(line: impl Into<String>) -> Record {
        Record::new("text").field("message", Value::String(line.into()))
    }

    /// Set a field, replacing one of the same name. Returns the record.
    #[must_use]
    pub fn field(mut self, key: impl Into<String>, value: Value) -> Record {
        self.set(key, value);
        self
    }

    /// Set a field, replacing one of the same name.
    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.fields.iter_mut().find(|(name, _)| *name == key) {
            Some((_, old)) => *old = value,
            None => self.fields.push((key, value)),
        }
    }

    /// Whether no parser read it.
    pub fn is_text(&self) -> bool {
        self.format == "text"
    }

    /// The field named exactly `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// The value at `path`: the field named `path`; for a name in
    /// [`MESSAGE_KEYS`], [`LEVEL_KEYS`] or [`TIME_KEYS`], the first of its
    /// group the record has (so `message` finds `msg`); or keys joined by
    /// dots into nested maps, and list indices (`http.status`,
    /// `tags.0`).
    pub fn lookup(&self, path: &str) -> Option<&Value> {
        lookup_fields(&self.fields, path)
    }

    /// The first of `keys` the record has, with its value.
    pub fn first_of<'a>(&'a self, keys: &[&str]) -> Option<(&'a str, &'a Value)> {
        keys.iter().find_map(|key| {
            self.fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(name, value)| (name.as_str(), value))
        })
    }

    /// The message, from the first of [`MESSAGE_KEYS`]; a value that is
    /// not a string as JSON-like text.
    pub fn message(&self) -> Option<Cow<'_, str>> {
        self.first_of(MESSAGE_KEYS).map(|(_, value)| display(value))
    }

    /// The level as written, from the first of [`LEVEL_KEYS`].
    pub fn level(&self) -> Option<&Value> {
        self.first_of(LEVEL_KEYS).map(|(_, value)| value)
    }

    /// The severity the level names (see [`severity_of`]).
    pub fn severity(&self) -> Option<Severity> {
        self.level().and_then(severity_of)
    }

    /// The time as written, from the first of [`TIME_KEYS`].
    pub fn time(&self) -> Option<Cow<'_, str>> {
        self.first_of(TIME_KEYS).map(|(_, value)| display(value))
    }

    /// The time as microseconds since the Unix epoch, when it parses (see
    /// [`parse_timestamp`]; a number is read as epoch seconds,
    /// milliseconds, microseconds or nanoseconds by its size).
    pub fn instant(&self) -> Option<i64> {
        let (_, value) = self.first_of(TIME_KEYS)?;
        match value {
            Value::String(text) => parse_timestamp(text),
            other => time::epoch(other),
        }
    }

    /// Whether lines no parser reads may join it: it was read by a parser
    /// and has a field besides its message. A bare JSON number, or a
    /// `msg=…` line alone, takes none.
    pub fn takes_continuation(&self) -> bool {
        !self.is_text()
            && self
                .fields
                .iter()
                .any(|(key, _)| !MESSAGE_KEYS.contains(&key.as_str()))
    }

    /// The record as an event: the message, severity and time from their
    /// keys, every other field in order. A level that names no severity
    /// stays a field.
    pub fn to_event(&self) -> StructuredEvent {
        let message_key = self.first_of(MESSAGE_KEYS).map(|(key, _)| key);
        let time_key = self.first_of(TIME_KEYS).map(|(key, _)| key);
        let severity = self.severity();
        let level_key = severity.and(self.first_of(LEVEL_KEYS).map(|(key, _)| key));
        let mut event =
            StructuredEvent::new(Message::Literal(self.message().unwrap_or_default().into()))
                .context(EventContext {
                    severity,
                    timestamp: self.time().map(Cow::into_owned),
                    ..Default::default()
                });
        for (key, value) in &self.fields {
            let name = Some(key.as_str());
            if name == message_key || name == time_key || name == level_key {
                continue;
            }
            event.fields.push((key.clone(), value.clone()));
        }
        event
    }

    /// An event as a record, the reverse of [`to_event`](Self::to_event):
    /// a `tracing` event from `EventLayer` or a `log` record from
    /// `LogAdapter` (see [`adapters`](crate::adapters)), ready for a log
    /// view. The fields, in order: `timestamp`, `level` (the severity's
    /// name), `target` and `message`, then the event's own fields, then
    /// `thread`, `task`, `correlation_id` and `spans` (the span names,
    /// outermost first, joined by `:`), each only when the event has it.
    /// An event marking a span opening or closing gets `span` (`open` or
    /// `close`) and, on a close, `elapsed_us`. Its format is `event`.
    ///
    /// ```
    /// use rich_ext::event::{EventContext, Message, Severity, StructuredEvent, Value};
    /// use rich_ext::log_stream::Record;
    ///
    /// let event = StructuredEvent::new(Message::Literal("connected".into()))
    ///     .field("port", Value::Unsigned(5432))
    ///     .context(EventContext {
    ///         severity: Some(Severity::Warn),
    ///         target: Some("db::pool".into()),
    ///         ..Default::default()
    ///     });
    /// let record = Record::from_event(&event);
    /// assert_eq!(record.message().as_deref(), Some("connected"));
    /// assert_eq!(record.severity(), Some(Severity::Warn));
    /// assert_eq!(record.get("target"), Some(&Value::String("db::pool".into())));
    /// assert_eq!(record.get("port"), Some(&Value::Unsigned(5432)));
    /// ```
    pub fn from_event(event: &StructuredEvent) -> Record {
        let context = &event.context;
        let text = |value: &str| Value::String(value.to_string());
        let mut record = Record::new("event");
        if let Some(time) = &context.timestamp {
            record.fields.push(("timestamp".into(), text(time)));
        }
        if let Some(severity) = context.severity {
            record
                .fields
                .push(("level".into(), text(severity.as_str())));
        }
        if let Some(target) = &context.target {
            record.fields.push(("target".into(), text(target)));
        }
        let message = match &event.message {
            Message::Literal(message) | Message::Markup(message) => message,
        };
        record.fields.push(("message".into(), text(message)));
        for (key, value) in &event.fields {
            record.set(key.clone(), value.clone());
        }
        for (key, value) in [
            ("thread", &context.thread),
            ("task", &context.task),
            ("correlation_id", &context.correlation_id),
        ] {
            if let Some(value) = value {
                record.set(key, text(value));
            }
        }
        let spans = event.span_context();
        if !spans.is_empty() {
            let names: Vec<&str> = spans.iter().map(|span| span.name.as_str()).collect();
            record.set("spans", text(&names.join(":")));
        }
        match event.span_marker() {
            Some(crate::event::SpanEvent::Open) => record.set("span", text("open")),
            Some(crate::event::SpanEvent::Close { elapsed }) => {
                record.set("span", text("close"));
                let micros = elapsed.as_micros().min(u64::MAX as u128) as u64;
                record.set("elapsed_us", Value::Unsigned(micros));
            }
            None => {}
        }
        record
    }

    /// Whether `other` is the same record but for its time: the same
    /// fields in the same order, the [`TIME_KEYS`] aside, and the same
    /// continuation lines.
    pub fn same_as(&self, other: &Record) -> bool {
        fn untimed(record: &Record) -> impl Iterator<Item = &(String, Value)> {
            record
                .fields
                .iter()
                .filter(|(key, _)| !TIME_KEYS.contains(&key.as_str()))
        }
        self.continuation == other.continuation && untimed(self).eq(untimed(other))
    }

    /// Mask secrets in the record's values with `redactor`, as
    /// [`Redactor::redact_field`] masks a `key=value` pair: a value under a
    /// secret key whole, and what the detectors and patterns match in any
    /// other text, in the message and in the continuation lines. A number
    /// or a flag that has to be masked becomes the mask's text.
    pub fn redact(&mut self, redactor: &Redactor) {
        if redactor.is_empty() {
            return;
        }
        for (key, value) in &mut self.fields {
            redact_value(redactor, key, value);
        }
        for line in &mut self.continuation {
            *line = redactor.redact_str(line);
        }
    }

    /// Show terminal controls in its text as symbols (see
    /// [`sanitize_terminal_controls`](crate::sanitize_terminal_controls)):
    /// every key, nested ones included, every string value, and the
    /// continuation lines.
    pub fn sanitize(&mut self) {
        fn key(key: &mut String) {
            if key.chars().any(char::is_control) {
                *key = crate::sanitize_terminal_controls(key);
            }
        }
        fn walk(value: &mut Value) {
            match value {
                Value::String(text) => *text = crate::sanitize_terminal_controls(text),
                Value::List(items) => items.iter_mut().for_each(walk),
                Value::Map(entries) => entries.iter_mut().for_each(|(name, value)| {
                    key(name);
                    walk(value)
                }),
                _ => {}
            }
        }
        for (name, value) in &mut self.fields {
            key(name);
            walk(value);
        }
        for line in &mut self.continuation {
            *line = crate::sanitize_terminal_controls(line);
        }
    }
}

/// [`Record::lookup`] over any fields (an event's, a span's).
pub(crate) fn lookup_fields<'a>(fields: &'a [(String, Value)], path: &str) -> Option<&'a Value> {
    let get = |key: &str| {
        fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    };
    if let Some(value) = get(path) {
        return Some(value);
    }
    for group in [MESSAGE_KEYS, LEVEL_KEYS, TIME_KEYS] {
        if group.contains(&path) {
            if let Some(value) = group.iter().find_map(|key| get(key)) {
                return Some(value);
            }
        }
    }
    if !path.contains('.') {
        return None;
    }
    let parts: Vec<&str> = path.split('.').collect();
    nested(fields, &parts)
}

/// `value` as text: a string as it is, anything else in its compact form
/// (`[1, 2]`, `{a: 1}`).
pub(crate) fn display(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(text) => Cow::Borrowed(text),
        other => Cow::Owned(other.format(false, 0)),
    }
}

/// The value at `parts` (keys and list indices) under `fields`, trying the
/// longest run of parts that names a key first, so a key holding a dot
/// (`http.status` written flat) is found as well as a nested one.
fn nested<'a>(fields: &'a [(String, Value)], parts: &[&str]) -> Option<&'a Value> {
    for take in (1..=parts.len()).rev() {
        let key = parts[..take].join(".");
        let Some((_, value)) = fields.iter().find(|(name, _)| *name == key) else {
            continue;
        };
        if take == parts.len() {
            return Some(value);
        }
        if let Some(found) = within(value, &parts[take..]) {
            return Some(found);
        }
    }
    None
}

fn within<'a>(value: &'a Value, parts: &[&str]) -> Option<&'a Value> {
    match value {
        Value::Map(entries) => nested(entries, parts),
        Value::List(items) => {
            let index: usize = parts[0].parse().ok()?;
            let item = items.get(index)?;
            if parts.len() == 1 {
                Some(item)
            } else {
                within(item, &parts[1..])
            }
        }
        _ => None,
    }
}

fn redact_value(redactor: &Redactor, key: &str, value: &mut Value) {
    match value {
        Value::String(text) => {
            let redacted = redactor.redact_field(key, text);
            if redacted != *text {
                *text = redacted;
            }
        }
        Value::List(items) => {
            for item in items {
                redact_value(redactor, key, item);
            }
        }
        Value::Map(entries) => {
            for (key, value) in entries {
                redact_value(redactor, key, value);
            }
        }
        Value::Null => {}
        other => {
            let text = other.format(false, 0);
            let redacted = redactor.redact_field(key, &text);
            if redacted != text {
                *other = Value::String(redacted);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Record {
        Record::new("json")
            .field("time", Value::String("2026-10-18T10:00:00Z".into()))
            .field("level", Value::String("WARN".into()))
            .field("msg", Value::String("slow".into()))
            .field(
                "http",
                Value::Map(vec![
                    ("status".into(), Value::Integer(503)),
                    (
                        "headers".into(),
                        Value::List(vec![Value::String("a".into())]),
                    ),
                ]),
            )
            .field("db.table", Value::String("users".into()))
    }

    #[test]
    fn lookup_finds_aliases_dotted_keys_and_nested_values() {
        let record = record();
        assert_eq!(
            record.lookup("message"),
            Some(&Value::String("slow".into()))
        );
        assert_eq!(record.lookup("timestamp"), record.get("time"));
        assert_eq!(record.lookup("severity"), record.get("level"));
        assert_eq!(record.lookup("http.status"), Some(&Value::Integer(503)));
        assert_eq!(
            record.lookup("http.headers.0"),
            Some(&Value::String("a".into()))
        );
        assert_eq!(
            record.lookup("db.table"),
            Some(&Value::String("users".into()))
        );
        assert_eq!(record.lookup("http.headers.9"), None);
        assert_eq!(record.lookup("http.nope"), None);
        assert_eq!(record.lookup("nope"), None);
    }

    #[test]
    fn reads_message_severity_and_time() {
        let record = record();
        assert_eq!(record.message().as_deref(), Some("slow"));
        assert_eq!(record.severity(), Some(Severity::Warn));
        assert_eq!(record.time().as_deref(), Some("2026-10-18T10:00:00Z"));
        assert_eq!(record.instant(), Some(1_792_317_600_000_000));
        let event = record.to_event();
        assert_eq!(event.context.severity, Some(Severity::Warn));
        assert_eq!(
            event.context.timestamp.as_deref(),
            Some("2026-10-18T10:00:00Z")
        );
        let keys: Vec<&str> = event.fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["http", "db.table"]);
        // A level naming no severity stays a field.
        let odd = Record::new("json")
            .field("level", Value::String("loud".into()))
            .to_event();
        assert_eq!(odd.context.severity, None);
        assert_eq!(odd.fields[0].0, "level");
    }

    #[test]
    fn same_as_ignores_only_the_time() {
        let a = record();
        let mut b = record().field("time", Value::String("2026-10-18T10:00:05Z".into()));
        assert!(a.same_as(&b));
        b.continuation.push("  at main".into());
        assert!(!a.same_as(&b));
        let c = record().field("msg", Value::String("fast".into()));
        assert!(!a.same_as(&c));
    }

    #[test]
    fn text_records_take_no_continuation() {
        assert!(!Record::text("hello").takes_continuation());
        assert!(record().takes_continuation());
        let bare = Record::new("json").field("message", Value::String("42".into()));
        assert!(!bare.takes_continuation());
    }

    #[test]
    fn redaction_masks_secret_keys_and_secrets_in_text() {
        let mut record = Record::new("json")
            .field("password", Value::String("hunter 2".into()))
            .field("api_key", Value::Integer(1234))
            .field(
                "msg",
                Value::String("login token=ghp_0123456789abcdefghijklmnopqrstuvwxyzAB ok".into()),
            )
            .field(
                "nested",
                Value::Map(vec![("secret".into(), Value::String("x".into()))]),
            )
            .field("user", Value::String("ann".into()));
        record
            .continuation
            .push("Authorization: Bearer abcdef0123456789".into());
        record.redact(&Redactor::secrets());
        assert_eq!(
            record.get("password"),
            Some(&Value::String("********".into()))
        );
        assert_eq!(
            record.get("api_key"),
            Some(&Value::String("********".into()))
        );
        assert_eq!(record.message().as_deref(), Some("login token=******** ok"));
        assert_eq!(
            record.lookup("nested.secret"),
            Some(&Value::String("********".into()))
        );
        assert_eq!(record.get("user"), Some(&Value::String("ann".into())));
        assert_eq!(record.continuation[0], "Authorization: Bearer ********");
        // Patterns alone mask what they match, and not secret keys.
        let mut record = Record::new("json")
            .field("password", Value::String("hunter2".into()))
            .field("msg", Value::String("card 4111-1111".into()));
        record.redact(&Redactor::new().pattern(r"\d{4}-\d{4}").unwrap());
        assert_eq!(
            record.get("password"),
            Some(&Value::String("hunter2".into()))
        );
        assert_eq!(record.message().as_deref(), Some("card ********"));
    }

    #[test]
    fn sanitize_shows_controls_in_values_and_continuations() {
        let mut record = Record::text("a\x1b[2Jb");
        record.continuation.push("\x07".into());
        record.sanitize();
        assert!(!record.message().unwrap().contains('\x1b'));
        assert!(!record.continuation[0].contains('\x07'));
    }

    #[test]
    fn sanitize_shows_controls_in_keys_nested_ones_too() {
        let mut record = Record::new("json")
            .field("\x1b]52;c;eA==\x07", Value::Integer(1))
            .field(
                "outer",
                Value::Map(vec![(
                    "in\x1b[2Jner".into(),
                    Value::List(vec![Value::Map(vec![(
                        "\u{9b}31m".into(),
                        Value::String("ok".into()),
                    )])]),
                )]),
            );
        record.sanitize();
        fn controls(value: &Value) -> bool {
            match value {
                Value::Map(entries) => entries
                    .iter()
                    .any(|(key, value)| key.chars().any(char::is_control) || controls(value)),
                Value::List(items) => items.iter().any(controls),
                Value::String(text) => text.chars().any(char::is_control),
                _ => false,
            }
        }
        assert!(controls(&Value::Map(vec![("\x07".into(), Value::Null)])));
        assert!(!controls(&Value::Map(record.fields.clone())), "{record:?}");
        assert_eq!(record.fields[1].0, "outer");
    }
}
