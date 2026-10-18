//! Which records to keep: by level ([`LevelFilter`]), by field
//! ([`Predicate`]), or both ([`Filter`]).
//!
//! # The predicate grammar
//!
//! A predicate is `FIELD OP VALUE`, or a bare `FIELD`:
//!
//! | form | keeps a record whose field… |
//! |------|-----------------------------|
//! | `service=api` | equals the value (`==` works too) |
//! | `service!=api` | is missing, or differs |
//! | `latency_ms>500`, `>=`, `<`, `<=` | orders after (or before) the value |
//! | `msg~time(d )?out` | matches the regular expression |
//! | `msg!~retry` | is missing, or does not match |
//! | `trace_id` | is there |
//! | `!trace_id` | is missing |
//!
//! `FIELD` is a key, keys joined by dots into nested maps (`http.status`),
//! or a list index (`tags.0`); `message`, `level` and `time` also find
//! `msg`, `severity` and `timestamp` (see [`Record::lookup`]). The value is
//! the rest of the predicate, spaces around it trimmed, and may be quoted
//! with `'…'` or `"…"`.
//!
//! Comparisons take the field's kind into account:
//!
//! - **levels** (a `level` or `severity` field, against a level name)
//!   compare by severity, so `level>=warn` keeps warn, error and fatal, and
//!   `level=error` matches `ERROR`, `err` and pino's `50`;
//! - **numbers** compare as numbers: a JSON number with `=`, and also text
//!   that is a number with `>`, `>=`, `<` and `<=` (logfmt values are
//!   text);
//! - **times** (RFC 3339, against a value that starts with a date) compare
//!   as instants, whatever their offsets;
//! - anything else compares as text, case-sensitively, `=` exactly.
//!
//! A missing field fails every test but `!=`, `!~` and `!FIELD`. A regular
//! expression is fancy-regex syntax (`(?i)` makes it case-insensitive) and
//! searches the field's text; one that gives up (too much backtracking)
//! does not match.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use super::{display, Record, LEVEL_KEYS};
use crate::event::{Severity, Value};

/// Why a [`LevelFilter`] or a [`Predicate`] did not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError(pub String);

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FilterError {}

/// The severity a level name means, in any case: `trace` (and `verbose`),
/// `debug`, `info` (`information`, `notice`), `warn` (`warning`), `error`
/// (`err`) and `fatal` (`critical`, `crit`, `panic`, `alert`, `emerg`,
/// `emergency`).
pub(crate) fn severity_named(name: &str) -> Option<Severity> {
    let name = name.trim();
    if name.len() > 11 {
        return None;
    }
    Some(match name.to_ascii_lowercase().as_str() {
        "trace" | "verbose" => Severity::Trace,
        "debug" => Severity::Debug,
        "info" | "information" | "informational" | "notice" => Severity::Info,
        "warn" | "warning" => Severity::Warn,
        "error" | "err" => Severity::Error,
        "fatal" | "critical" | "crit" | "panic" | "alert" | "emerg" | "emergency" => {
            Severity::Fatal
        }
        _ => return None,
    })
}

/// The severity a level value means: a name (see [`LevelFilter`]), or a
/// number as pino and bunyan write them (10 trace, 20 debug, 30 info, 40
/// warn, 50 error, 60 fatal; a number between two is the lower one, and
/// one under 10 or from 100 names none).
///
/// ```
/// use rich_ext::event::{Severity, Value};
/// use rich_ext::log_stream::severity_of;
///
/// assert_eq!(severity_of(&Value::String("WARNING".into())), Some(Severity::Warn));
/// assert_eq!(severity_of(&Value::Integer(50)), Some(Severity::Error));
/// assert_eq!(severity_of(&Value::String("loud".into())), None);
/// ```
pub fn severity_of(value: &Value) -> Option<Severity> {
    let number = match value {
        Value::String(name) => {
            return severity_named(name).or_else(|| {
                is_number(name)
                    .then(|| name.parse::<f64>().ok())
                    .flatten()
                    .and_then(numbered)
            })
        }
        Value::Integer(n) => *n as f64,
        Value::Unsigned(n) => *n as f64,
        Value::Integer128(n) => *n as f64,
        Value::Unsigned128(n) => *n as f64,
        Value::Float(n) => *n,
        _ => return None,
    };
    numbered(number)
}

fn numbered(number: f64) -> Option<Severity> {
    if !(10.0..100.0).contains(&number) {
        return None;
    }
    Some(match number {
        n if n < 20.0 => Severity::Trace,
        n if n < 30.0 => Severity::Debug,
        n if n < 40.0 => Severity::Info,
        n if n < 50.0 => Severity::Warn,
        n if n < 60.0 => Severity::Error,
        _ => Severity::Fatal,
    })
}

/// Whether `text` is a plain decimal number: an optional sign, digits, an
/// optional fraction and exponent. Not `inf`, `0x10` or `1_000`.
pub(crate) fn is_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = usize::from(matches!(bytes.first(), Some(b'-' | b'+')));
    let start = at;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let mut integral = at - start;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let fraction = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        integral += at - fraction;
    }
    if integral == 0 {
        return false;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        at += usize::from(matches!(bytes.get(at), Some(b'-' | b'+')));
        let exponent = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == exponent {
            return false;
        }
    }
    at == bytes.len()
}

/// Which levels to keep: `error` (only errors), `warn+` (warn and above),
/// or several, `info,error+`. A record with no level that names a
/// severity is not kept, unless the filter is empty, which keeps
/// everything.
///
/// ```
/// use rich_ext::event::Severity;
/// use rich_ext::log_stream::LevelFilter;
///
/// let filter: LevelFilter = "warn+".parse().unwrap();
/// assert!(filter.allows(Some(Severity::Fatal)));
/// assert!(!filter.allows(Some(Severity::Info)));
/// assert!(!filter.allows(None));
/// assert_eq!(filter.to_string(), "warn+");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LevelFilter {
    /// Each level, and whether the ones above it count too.
    levels: Vec<(Severity, bool)>,
}

impl LevelFilter {
    /// Keep everything.
    pub fn new() -> LevelFilter {
        LevelFilter::default()
    }

    /// Keep `severity` and every level above it.
    pub fn at_least(severity: Severity) -> LevelFilter {
        LevelFilter {
            levels: vec![(severity, true)],
        }
    }

    /// Keep `severity` only.
    pub fn only(severity: Severity) -> LevelFilter {
        LevelFilter {
            levels: vec![(severity, false)],
        }
    }

    /// Also keep what `other` keeps.
    pub fn or(mut self, other: LevelFilter) -> LevelFilter {
        self.levels.extend(other.levels);
        self
    }

    /// Whether it keeps everything.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Whether a record at `severity` is kept.
    pub fn allows(&self, severity: Option<Severity>) -> bool {
        if self.levels.is_empty() {
            return true;
        }
        let Some(severity) = severity else {
            return false;
        };
        self.levels
            .iter()
            .any(|&(level, above)| severity == level || (above && severity > level))
    }
}

impl FromStr for LevelFilter {
    type Err = FilterError;

    fn from_str(spec: &str) -> Result<LevelFilter, FilterError> {
        let mut filter = LevelFilter::new();
        for part in spec.split(',').map(str::trim) {
            let (name, above) = match part.strip_suffix('+') {
                Some(name) => (name.trim_end(), true),
                None => (part, false),
            };
            let severity = severity_named(name).ok_or_else(|| {
                FilterError(format!(
                    "unknown level {part:?} (trace, debug, info, warn, error or fatal, with + \
                     for that level and above)"
                ))
            })?;
            filter.levels.push((severity, above));
        }
        Ok(filter)
    }
}

impl fmt::Display for LevelFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (severity, above)) in self.levels.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            f.write_str(severity.as_str())?;
            if *above {
                f.write_str("+")?;
            }
        }
        Ok(())
    }
}

/// A predicate's test. See the [module docs](self) for the grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `=` (or `==`).
    Eq,
    /// `!=`.
    Ne,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `~`: the regular expression matches.
    Match,
    /// `!~`: it does not.
    NotMatch,
    /// A bare `FIELD`: the field is there.
    Present,
    /// `!FIELD`: the field is missing.
    Absent,
}

impl Op {
    /// How the operator is written (empty for [`Op::Present`], `!` for
    /// [`Op::Absent`]).
    pub fn symbol(self) -> &'static str {
        match self {
            Op::Eq => "=",
            Op::Ne => "!=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Match => "~",
            Op::NotMatch => "!~",
            Op::Present => "",
            Op::Absent => "!",
        }
    }
}

/// One test on a record's field: `service=api`, `latency_ms>500`,
/// `msg~timeout`. See the [module docs](self) for the grammar.
///
/// ```
/// use rich_ext::event::Value;
/// use rich_ext::log_stream::{Predicate, Record};
///
/// let record = Record::new("logfmt")
///     .field("level", Value::String("error".into()))
///     .field("latency_ms", Value::String("812".into()));
/// assert!(Predicate::parse("latency_ms>500").unwrap().matches(&record));
/// assert!(Predicate::parse("level>=warn").unwrap().matches(&record));
/// assert!(!Predicate::parse("service=api").unwrap().matches(&record));
/// assert!(Predicate::parse("service!=api").unwrap().matches(&record));
/// ```
#[derive(Clone, Debug)]
pub struct Predicate {
    source: String,
    path: String,
    op: Op,
    value: String,
    regex: Option<fancy_regex::Regex>,
}

impl PartialEq for Predicate {
    fn eq(&self, other: &Predicate) -> bool {
        self.source == other.source
    }
}

impl Predicate {
    /// Parse `expression`. An empty one, one with no field name, a lone
    /// `!` and an invalid regular expression are errors.
    pub fn parse(expression: &str) -> Result<Predicate, FilterError> {
        let source = expression.trim();
        if source.is_empty() {
            return Err(FilterError("an empty --where expression".into()));
        }
        let Some(at) = source.find(['=', '!', '<', '>', '~']) else {
            return Ok(Predicate::new(source, source, Op::Present, ""));
        };
        let path = source[..at].trim();
        let rest = &source[at..];
        if path.is_empty() && rest.starts_with('!') {
            let name = rest[1..].trim();
            let bare = !name.is_empty() && !name.contains(['=', '!', '<', '>', '~']);
            if bare {
                return Ok(Predicate::new(source, name, Op::Absent, ""));
            }
        }
        if path.is_empty() {
            return Err(FilterError(format!(
                "{source:?} has no field name before its operator (FIELD=VALUE, FIELD>VALUE, \
                 FIELD~REGEX, ...)"
            )));
        }
        let (op, length) = [
            ("==", Op::Eq),
            ("!=", Op::Ne),
            ("!~", Op::NotMatch),
            (">=", Op::Ge),
            ("<=", Op::Le),
            ("=", Op::Eq),
            (">", Op::Gt),
            ("<", Op::Lt),
            ("~", Op::Match),
        ]
        .into_iter()
        .find(|(symbol, _)| rest.starts_with(symbol))
        .map(|(symbol, op)| (op, symbol.len()))
        .ok_or_else(|| {
            FilterError(format!(
                "{source:?}: `!` must be followed by `=` or `~` (FIELD!=VALUE, FIELD!~REGEX)"
            ))
        })?;
        let value = unquote(rest[length..].trim());
        let mut predicate = Predicate::new(source, path, op, value);
        if matches!(op, Op::Match | Op::NotMatch) {
            let regex = fancy_regex::Regex::new(value)
                .map_err(|error| FilterError(format!("{source:?}: invalid pattern: {error}")))?;
            predicate.regex = Some(regex);
        }
        Ok(predicate)
    }

    fn new(source: &str, path: &str, op: Op, value: &str) -> Predicate {
        Predicate {
            source: source.to_string(),
            path: path.to_string(),
            op,
            value: value.to_string(),
            regex: None,
        }
    }

    /// The field it tests.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn op(&self) -> Op {
        self.op
    }

    /// The value it compares with (the pattern, for `~`).
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Whether `record` passes.
    pub fn matches(&self, record: &Record) -> bool {
        let Some(field) = record.lookup(&self.path) else {
            return matches!(self.op, Op::Ne | Op::NotMatch | Op::Absent);
        };
        match self.op {
            Op::Present => true,
            Op::Absent => false,
            Op::Match | Op::NotMatch => {
                let found = self
                    .regex
                    .as_ref()
                    .is_some_and(|regex| regex.is_match(display(field).as_ref()).unwrap_or(false));
                found == (self.op == Op::Match)
            }
            Op::Eq => self.equal(field),
            Op::Ne => !self.equal(field),
            Op::Gt | Op::Ge | Op::Lt | Op::Le => {
                let Some(order) = self.order(field) else {
                    return false;
                };
                match self.op {
                    Op::Gt => order == Ordering::Greater,
                    Op::Ge => order != Ordering::Less,
                    Op::Lt => order == Ordering::Less,
                    _ => order != Ordering::Greater,
                }
            }
        }
    }

    /// Whether the field is a level, compared by severity.
    fn levels(&self, field: &Value) -> Option<(Severity, Severity)> {
        if !LEVEL_KEYS.contains(&self.path.as_str()) {
            return None;
        }
        Some((severity_of(field)?, severity_named(&self.value)?))
    }

    fn equal(&self, field: &Value) -> bool {
        if let Some((have, want)) = self.levels(field) {
            return have == want;
        }
        if let Some(number) = number_value(field) {
            if let Some(order) = number.compare(&self.value) {
                return order == Ordering::Equal;
            }
        }
        display(field) == self.value
    }

    fn order(&self, field: &Value) -> Option<Ordering> {
        if let Some((have, want)) = self.levels(field) {
            return Some(have.cmp(&want));
        }
        let text = display(field);
        let number = number_value(field).or_else(|| {
            matches!(field, Value::String(_))
                .then(|| Number::parse(&text))
                .flatten()
        });
        if let Some(order) = number.and_then(|number| number.compare(&self.value)) {
            return Some(order);
        }
        let value = self.value.as_bytes();
        let dated = value
            .get(..4)
            .is_some_and(|year| year.iter().all(u8::is_ascii_digit))
            && matches!(value.get(4), Some(b'-' | b'/'));
        if dated {
            if let (Some(have), Some(want)) = (
                super::parse_timestamp(&text),
                super::parse_timestamp(&self.value),
            ) {
                return Some(have.cmp(&want));
            }
        }
        match field {
            Value::List(_) | Value::Map(_) => None,
            _ => Some(text.as_ref().cmp(self.value.as_str())),
        }
    }
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl FromStr for Predicate {
    type Err = FilterError;

    fn from_str(expression: &str) -> Result<Predicate, FilterError> {
        Predicate::parse(expression)
    }
}

/// `text` without one pair of matching quotes around it.
fn unquote(text: &str) -> &str {
    for quote in ['"', '\''] {
        if text.len() >= 2 && text.starts_with(quote) && text.ends_with(quote) {
            return &text[1..text.len() - 1];
        }
    }
    text
}

/// A number from a field, compared exactly when both sides are whole.
#[derive(Clone, Copy, Debug)]
enum Number {
    Whole(i128),
    Real(f64),
}

impl Number {
    fn parse(text: &str) -> Option<Number> {
        if !is_number(text) {
            return None;
        }
        if let Ok(whole) = text.parse::<i128>() {
            return Some(Number::Whole(whole));
        }
        text.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(Number::Real)
    }

    fn compare(self, other: &str) -> Option<Ordering> {
        match (self, Number::parse(other)?) {
            (Number::Whole(a), Number::Whole(b)) => Some(a.cmp(&b)),
            (a, b) => a.real().partial_cmp(&b.real()),
        }
    }

    fn real(self) -> f64 {
        match self {
            Number::Whole(n) => n as f64,
            Number::Real(n) => n,
        }
    }
}

/// The number a JSON number field holds.
fn number_value(value: &Value) -> Option<Number> {
    Some(match value {
        Value::Integer(n) => Number::Whole(i128::from(*n)),
        Value::Unsigned(n) => Number::Whole(i128::from(*n)),
        Value::Integer128(n) => Number::Whole(*n),
        Value::Unsigned128(n) => i128::try_from(*n)
            .map(Number::Whole)
            .unwrap_or(Number::Real(*n as f64)),
        Value::Float(n) if n.is_finite() => Number::Real(*n),
        _ => return None,
    })
}

/// A [`LevelFilter`] and predicates, all of which a record must pass.
///
/// ```
/// use rich_ext::event::Value;
/// use rich_ext::log_stream::{Filter, LevelFilter, Predicate, Record};
///
/// let filter = Filter::new()
///     .levels(LevelFilter::at_least(rich_ext::event::Severity::Warn))
///     .predicate(Predicate::parse("service=api").unwrap());
/// let record = Record::new("json")
///     .field("level", Value::String("error".into()))
///     .field("service", Value::String("api".into()));
/// assert!(filter.matches(&record));
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    levels: LevelFilter,
    predicates: Vec<Predicate>,
}

impl Filter {
    /// Keep everything.
    pub fn new() -> Filter {
        Filter::default()
    }

    /// Keep only the levels `levels` allows.
    pub fn levels(mut self, levels: LevelFilter) -> Filter {
        self.levels = levels;
        self
    }

    /// Also require `predicate`.
    pub fn predicate(mut self, predicate: Predicate) -> Filter {
        self.predicates.push(predicate);
        self
    }

    /// The level filter.
    pub fn level_filter(&self) -> &LevelFilter {
        &self.levels
    }

    /// The predicates, in the order given.
    pub fn predicates(&self) -> &[Predicate] {
        &self.predicates
    }

    /// Whether it keeps everything.
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty() && self.predicates.is_empty()
    }

    /// Whether `record` is kept.
    pub fn matches(&self, record: &Record) -> bool {
        (self.levels.is_empty() || self.levels.allows(record.severity()))
            && self.predicates.iter().all(|p| p.matches(record))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Record {
        Record::new("json")
            .field("time", Value::String("2026-10-18T12:00:00+02:00".into()))
            .field("level", Value::String("WARN".into()))
            .field("msg", Value::String("upstream timed out".into()))
            .field("service", Value::String("api".into()))
            .field("latency_ms", Value::Integer(812))
            .field("ratio", Value::Float(0.25))
            .field("ok", Value::Bool(false))
            .field("note", Value::Null)
            .field("code", Value::String("007".into()))
            .field(
                "http",
                Value::Map(vec![("status".into(), Value::Unsigned(503))]),
            )
            .field("tags", Value::List(vec![Value::String("edge".into())]))
    }

    fn keeps(expression: &str) -> bool {
        Predicate::parse(expression)
            .unwrap_or_else(|e| panic!("{expression}: {e}"))
            .matches(&record())
    }

    #[test]
    fn equality_and_inequality() {
        assert!(keeps("service=api"));
        assert!(keeps("service == api"));
        assert!(keeps("service='api'"));
        assert!(keeps("service=\"api\""));
        assert!(!keeps("service=API"));
        assert!(keeps("service!=web"));
        assert!(!keeps("service!=api"));
        assert!(keeps("latency_ms=812"));
        assert!(keeps("latency_ms=812.0"));
        assert!(keeps("ratio=0.25"));
        assert!(keeps("ok=false"));
        assert!(keeps("note=null"));
        // Text is compared as text with `=`: "007" is not 7.
        assert!(!keeps("code=7"));
        assert!(keeps("code=007"));
        assert!(keeps("http.status=503"));
        assert!(keeps("tags.0=edge"));
    }

    #[test]
    fn ordering_by_number_text_and_time() {
        assert!(keeps("latency_ms>500"));
        assert!(keeps("latency_ms>=812"));
        assert!(!keeps("latency_ms<812"));
        assert!(keeps("latency_ms<=812"));
        assert!(keeps("ratio<1"));
        // Numeric text orders as a number (logfmt values are text).
        assert!(keeps("code>6"));
        assert!(keeps("service<b"));
        assert!(keeps("time>=2026-10-18T09:59:59Z"));
        assert!(keeps("time<2026-10-18T10:00:01Z"));
        assert!(keeps("time>2026-10-18"));
        // A list or map does not order.
        assert!(!keeps("tags>a"));
        assert!(!keeps("http<z"));
    }

    #[test]
    fn levels_compare_by_severity() {
        assert!(keeps("level>=warn"));
        assert!(keeps("level=warning"));
        assert!(keeps("severity=warn"));
        assert!(!keeps("level>=error"));
        assert!(keeps("level<error"));
        let pino = Record::new("json").field("level", Value::Integer(50));
        assert!(Predicate::parse("level=error").unwrap().matches(&pino));
        assert!(Predicate::parse("level>warn").unwrap().matches(&pino));
    }

    #[test]
    fn patterns_presence_and_missing_fields() {
        assert!(keeps("msg~timed out"));
        assert!(keeps("message~^upstream"));
        assert!(keeps("msg~(?i)TIMED"));
        assert!(!keeps("msg~TIMED"));
        assert!(keeps("msg!~retry"));
        assert!(keeps("http~503"));
        assert!(!keeps("trace"));
        assert!(keeps("service"));
        assert!(!keeps("!service"));
        assert!(keeps("!trace_id"));
        // A missing field fails all but the negative tests.
        for expression in ["user=ann", "user>1", "user~a", "user"] {
            assert!(!keeps(expression), "{expression}");
        }
        for expression in ["user!=ann", "user!~a", "!user"] {
            assert!(keeps(expression), "{expression}");
        }
    }

    #[test]
    fn malformed_expressions_are_errors() {
        for bad in ["", "   ", "=api", ">5", "a!b", "msg~(", "msg!~[", "!", "! "] {
            assert!(Predicate::parse(bad).is_err(), "{bad:?} parsed");
        }
        let error = Predicate::parse("=api").unwrap_err();
        assert!(error.0.contains("no field name"), "{error}");
        assert_eq!(Predicate::parse("a = b").unwrap().value(), "b");
        assert_eq!(Predicate::parse("a=").unwrap().value(), "");
        assert_eq!(Predicate::parse("a>=1").unwrap().op(), Op::Ge);
        assert_eq!(Predicate::parse("a b=c").unwrap().path(), "a b");
    }

    #[test]
    fn level_filters_parse_and_combine() {
        let filter: LevelFilter = "info, error+".parse().unwrap();
        assert!(filter.allows(Some(Severity::Info)));
        assert!(!filter.allows(Some(Severity::Warn)));
        assert!(filter.allows(Some(Severity::Fatal)));
        assert_eq!(filter.to_string(), "info,error+");
        assert_eq!(
            "WARNING+".parse::<LevelFilter>().unwrap(),
            LevelFilter::at_least(Severity::Warn)
        );
        assert_eq!(
            "err".parse::<LevelFilter>().unwrap(),
            LevelFilter::only(Severity::Error)
        );
        for bad in ["", "loud", "warn++", "+", "info,"] {
            assert!(bad.parse::<LevelFilter>().is_err(), "{bad:?}");
        }
        assert!(LevelFilter::new().allows(None));
        let combined = LevelFilter::only(Severity::Debug).or(LevelFilter::only(Severity::Fatal));
        assert!(combined.allows(Some(Severity::Fatal)));
        assert!(!combined.allows(Some(Severity::Error)));
    }

    #[test]
    fn severities_from_names_and_numbers() {
        let named = |name: &str| severity_of(&Value::String(name.into()));
        assert_eq!(named("Verbose"), Some(Severity::Trace));
        assert_eq!(named("notice"), Some(Severity::Info));
        assert_eq!(named("CRIT"), Some(Severity::Fatal));
        assert_eq!(named("30"), Some(Severity::Info));
        assert_eq!(named("I"), None);
        assert_eq!(severity_of(&Value::Integer(10)), Some(Severity::Trace));
        assert_eq!(severity_of(&Value::Float(45.0)), Some(Severity::Warn));
        assert_eq!(severity_of(&Value::Integer(60)), Some(Severity::Fatal));
        assert_eq!(severity_of(&Value::Integer(3)), None);
        assert_eq!(severity_of(&Value::Integer(-1)), None);
        assert_eq!(severity_of(&Value::Integer(5000)), None);
        assert_eq!(severity_of(&Value::Bool(true)), None);
    }

    #[test]
    fn numbers_are_plain_decimals() {
        for good in [
            "0", "-1", "+2", "1.5", ".5", "5.", "1e3", "1E-3", "-0.25e+2",
        ] {
            assert!(is_number(good), "{good}");
        }
        for bad in [
            "", "-", ".", "e5", "1e", "inf", "NaN", "0x10", "1_000", " 1", "1 ",
        ] {
            assert!(!is_number(bad), "{bad}");
        }
    }

    #[test]
    fn a_filter_ands_its_parts() {
        let filter = Filter::new()
            .levels("warn+".parse().unwrap())
            .predicate(Predicate::parse("service=api").unwrap())
            .predicate(Predicate::parse("latency_ms>500").unwrap());
        assert!(filter.matches(&record()));
        let filter = filter.predicate(Predicate::parse("latency_ms>900").unwrap());
        assert!(!filter.matches(&record()));
        assert!(Filter::new().matches(&Record::text("anything")));
        let levels = Filter::new().levels("error".parse().unwrap());
        assert!(!levels.matches(&Record::text("no level")));
    }
}
