//! Records by trace, span or correlation id: the groups, their headers,
//! and a summary of levels and the groups with errors.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};

use super::{display, format_offset, lookup_fields, Record};
use crate::event::{theme_style, Severity, StructuredEvent, Value};

/// What records are grouped by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupBy {
    /// A trace id: `trace_id`, `traceId`, `trace.id`, `dd.trace_id`, …
    Trace,
    /// A span id: `span_id`, `spanId`, `span.id`, `dd.span_id`, …
    Span,
    /// A correlation or request id: `correlation_id`, `correlationId`,
    /// `request_id`, `requestId`, `req_id`, `x-request-id`, … (and an
    /// event's [`EventContext::correlation_id`](crate::event::EventContext)).
    Correlation,
}

const TRACE_KEYS: &[&str] = &[
    "trace_id",
    "traceId",
    "traceID",
    "trace.id",
    "dd.trace_id",
    "otel.trace_id",
    "logging.googleapis.com/trace",
];

const SPAN_KEYS: &[&str] = &[
    "span_id",
    "spanId",
    "spanID",
    "span.id",
    "dd.span_id",
    "otel.span_id",
    "logging.googleapis.com/spanId",
];

const CORRELATION_KEYS: &[&str] = &[
    "correlation_id",
    "correlationId",
    "correlationID",
    "correlation.id",
    "request_id",
    "requestId",
    "requestID",
    "request.id",
    "req_id",
    "reqId",
    "x_request_id",
    "x-request-id",
];

impl GroupBy {
    pub const ALL: [GroupBy; 3] = [GroupBy::Trace, GroupBy::Span, GroupBy::Correlation];

    /// `trace`, `span` or `correlation`.
    pub fn name(self) -> &'static str {
        match self {
            GroupBy::Trace => "trace",
            GroupBy::Span => "span",
            GroupBy::Correlation => "correlation",
        }
    }

    /// What one group is called, in the plural: `traces`, `spans`,
    /// `correlation ids`.
    pub fn plural(self) -> &'static str {
        match self {
            GroupBy::Trace => "traces",
            GroupBy::Span => "spans",
            GroupBy::Correlation => "correlation ids",
        }
    }

    /// The keys an id is read from, in order (a dotted key is also found
    /// nested: `trace.id` in `{"trace": {"id": …}}`).
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            GroupBy::Trace => TRACE_KEYS,
            GroupBy::Span => SPAN_KEYS,
            GroupBy::Correlation => CORRELATION_KEYS,
        }
    }

    /// `record`'s id: the first of [`keys`](Self::keys) it has with a value
    /// that is not empty, a list or a map.
    pub fn key_of(self, record: &Record) -> Option<String> {
        self.key_in(&record.fields)
    }

    fn key_in(self, fields: &[(String, Value)]) -> Option<String> {
        self.keys().iter().find_map(|key| {
            let value = lookup_fields(fields, key)?;
            match value {
                Value::Null | Value::List(_) | Value::Map(_) => None,
                Value::String(text) if text.trim().is_empty() => None,
                value => Some(display(value).into_owned()),
            }
        })
    }

    /// `event`'s id: for [`GroupBy::Correlation`] its context's
    /// `correlation_id` first; then its fields, and its spans' fields from
    /// the innermost.
    pub fn key_of_event(self, event: &StructuredEvent) -> Option<String> {
        if self == GroupBy::Correlation {
            if let Some(id) = event
                .context
                .correlation_id
                .as_ref()
                .filter(|id| !id.is_empty())
            {
                return Some(id.clone());
            }
        }
        self.key_in(&event.fields).or_else(|| {
            event
                .span_context()
                .iter()
                .rev()
                .find_map(|span| self.key_in(&span.fields))
        })
    }
}

impl fmt::Display for GroupBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for GroupBy {
    type Err = String;

    /// `trace`, `span` or `correlation`.
    fn from_str(name: &str) -> Result<GroupBy, String> {
        GroupBy::ALL
            .into_iter()
            .find(|by| by.name() == name)
            .ok_or_else(|| format!("unknown grouping {name:?} (trace, span, correlation)"))
    }
}

/// How many records there were at each level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelCounts {
    counts: [usize; 6],
    none: usize,
}

const SEVERITIES: [Severity; 6] = [
    Severity::Fatal,
    Severity::Error,
    Severity::Warn,
    Severity::Info,
    Severity::Debug,
    Severity::Trace,
];

fn index(severity: Severity) -> usize {
    SEVERITIES
        .iter()
        .position(|s| *s == severity)
        .expect("every severity is listed")
}

impl LevelCounts {
    /// Count a record at `severity` (`None`: one without a level).
    pub fn add(&mut self, severity: Option<Severity>) {
        match severity {
            Some(severity) => self.counts[index(severity)] += 1,
            None => self.none += 1,
        }
    }

    /// The records at `severity`.
    pub fn get(&self, severity: Severity) -> usize {
        self.counts[index(severity)]
    }

    /// The records without a level.
    pub fn without_level(&self) -> usize {
        self.none
    }

    pub fn total(&self) -> usize {
        self.counts.iter().sum::<usize>() + self.none
    }

    /// Errors and fatal errors.
    pub fn errors(&self) -> usize {
        self.get(Severity::Error) + self.get(Severity::Fatal)
    }

    /// `error 2 · warn 3 · info 7 · no level 1`: the levels with records,
    /// most severe first, each in its severity's style.
    pub fn text(&self, console: &Console) -> Text {
        let mut text = Text::new("");
        let mut first = true;
        let mut part = |text: &mut Text, label: String, style: Option<Style>| {
            if !first {
                text.append(" · ", Some(dim()));
            }
            first = false;
            text.append(&label, style.map(Into::into));
        };
        for severity in SEVERITIES {
            let count = self.get(severity);
            if count > 0 {
                let style = severity_style(console, severity);
                part(
                    &mut text,
                    format!("{} {count}", severity.as_str()),
                    Some(style),
                );
            }
        }
        if self.none > 0 {
            part(&mut text, format!("no level {}", self.none), None);
        }
        text
    }
}

fn dim() -> rich::style::StyleType {
    "dim".into()
}

fn severity_style(console: &Console, severity: Severity) -> Style {
    let fallback = match severity {
        Severity::Error | Severity::Fatal => "bold red",
        Severity::Warn => "yellow",
        Severity::Info => "green",
        _ => "dim",
    };
    theme_style(
        console,
        &format!("event.severity.{}", severity.as_str()),
        fallback,
    )
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

/// The records that share an id (or have none).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    /// The id; `None` for the records without one.
    pub key: Option<String>,
    pub records: Vec<Record>,
}

impl Group {
    /// The records at each level.
    pub fn levels(&self) -> LevelCounts {
        let mut counts = LevelCounts::default();
        for record in &self.records {
            counts.add(record.severity());
        }
        counts
    }

    /// The first and last times among its records, when any parse.
    pub fn span(&self) -> Option<(i64, i64)> {
        let mut instants = self.records.iter().filter_map(Record::instant);
        let first = instants.next()?;
        Some(instants.fold((first, first), |(low, high), at| {
            (low.min(at), high.max(at))
        }))
    }

    /// Its header, with counts and its time span.
    pub fn header(&self, by: GroupBy) -> GroupHeader {
        GroupHeader {
            by,
            key: self.key.clone(),
            levels: Some(self.levels()),
            span: self.span(),
        }
    }
}

/// Records gathered by id, in the order each id first appeared; the records
/// without one come last.
///
/// ```
/// use rich_ext::event::Value;
/// use rich_ext::log_stream::{GroupBy, Groups, Record};
///
/// let mut groups = Groups::new(GroupBy::Trace);
/// for (trace, message) in [("a", "start"), ("b", "start"), ("a", "done")] {
///     groups.push(
///         Record::new("json")
///             .field("trace_id", Value::String(trace.into()))
///             .field("msg", Value::String(message.into())),
///     );
/// }
/// let groups = groups.into_groups();
/// assert_eq!(groups[0].key.as_deref(), Some("a"));
/// assert_eq!(groups[0].records.len(), 2);
/// ```
#[derive(Clone, Debug)]
pub struct Groups {
    by: GroupBy,
    groups: Vec<Group>,
    index: HashMap<Option<String>, usize>,
    last: Option<usize>,
}

impl Groups {
    pub fn new(by: GroupBy) -> Groups {
        Groups {
            by,
            groups: Vec::new(),
            index: HashMap::new(),
            last: None,
        }
    }

    pub fn by(&self) -> GroupBy {
        self.by
    }

    /// Add `record` to its group.
    pub fn push(&mut self, record: Record) {
        let key = self.by.key_of(&record);
        let next = self.groups.len();
        let at = *self.index.entry(key.clone()).or_insert(next);
        if at == next {
            self.groups.push(Group {
                key,
                records: Vec::new(),
            });
        }
        self.groups[at].records.push(record);
        self.last = Some(at);
    }

    /// Add a continuation line to the record pushed last.
    pub fn push_continuation(&mut self, line: String) {
        if let Some(record) = self.last.and_then(|at| self.groups[at].records.last_mut()) {
            record.continuation.push(line);
        }
    }

    /// How many groups there are.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The groups: by first appearance, the one without an id last.
    pub fn into_groups(mut self) -> Vec<Group> {
        if let Some(at) = self.groups.iter().position(|group| group.key.is_none()) {
            let ungrouped = self.groups.remove(at);
            self.groups.push(ungrouped);
        }
        self.groups
    }
}

/// A group's header: `▸ trace 4bf92f3577b34da6 · 5 records · 1 error ·
/// 2.100s`, or just the id while records are still arriving
/// ([`levels`](Self::levels) `None`).
#[derive(Clone, Debug, PartialEq)]
pub struct GroupHeader {
    pub by: GroupBy,
    pub key: Option<String>,
    /// The group's records at each level, when they are all known.
    pub levels: Option<LevelCounts>,
    /// The group's first and last times.
    pub span: Option<(i64, i64)>,
}

impl GroupHeader {
    /// A header for a group whose records are still arriving: the id only.
    pub fn streaming(by: GroupBy, key: Option<String>) -> GroupHeader {
        GroupHeader {
            by,
            key,
            levels: None,
            span: None,
        }
    }

    /// The header as one line of text.
    pub fn text(&self, console: &Console) -> Text {
        let marker = if console.ascii_only() { "> " } else { "▸ " };
        let mut text = Text::new("");
        let group = theme_style(console, "log.group", "bold");
        text.append(marker, Some(group.clone().into()));
        match &self.key {
            Some(key) => {
                text.append(&format!("{} ", self.by.name()), Some(group.into()));
                let style = theme_style(console, "log.group.key", "bold magenta");
                text.append(key, Some(style.into()));
            }
            None => {
                let label = match self.by {
                    GroupBy::Trace => "no trace id",
                    GroupBy::Span => "no span id",
                    GroupBy::Correlation => "no correlation id",
                };
                text.append(label, Some(group.into()));
            }
        }
        let count = theme_style(console, "log.group.count", "dim");
        if let Some(levels) = &self.levels {
            text.append(" · ", Some(count.clone().into()));
            text.append(
                &plural(levels.total(), "record", "records"),
                Some(count.clone().into()),
            );
            let errors = levels.errors();
            if errors > 0 {
                text.append(" · ", Some(count.clone().into()));
                let style = theme_style(console, "log.summary.error", "bold red");
                text.append(&plural(errors, "error", "errors"), Some(style.into()));
            }
            let warnings = levels.get(Severity::Warn);
            if warnings > 0 {
                text.append(" · ", Some(count.clone().into()));
                let style = severity_style(console, Severity::Warn);
                text.append(&plural(warnings, "warning", "warnings"), Some(style.into()));
            }
        }
        if let Some((first, last)) = self.span.filter(|(first, last)| last > first) {
            text.append(" · ", Some(count.clone().into()));
            let length = format_offset(last - first);
            text.append(&length[1..], Some(count.into()));
        }
        text
    }
}

impl Renderable for GroupHeader {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.text(console).rich_render(console, options)
    }
}

/// What a stream held, for the end of it: the records read and shown, the
/// levels of those shown, the repeats folded and, when grouping, the
/// groups with errors.
///
/// ```
/// use rich_ext::event::Value;
/// use rich_ext::log_stream::{GroupBy, Record, Summary};
///
/// let mut summary = Summary::new(Some(GroupBy::Trace));
/// summary.read(3);
/// for (level, trace) in [("info", "a"), ("error", "b")] {
///     summary.shown(
///         &Record::new("json")
///             .field("level", Value::String(level.into()))
///             .field("trace_id", Value::String(trace.into())),
///     );
/// }
/// assert_eq!(summary.records_shown(), 2);
/// assert_eq!(summary.failing(), [(Some("b".to_string()), 1)]);
/// ```
#[derive(Clone, Debug, Default)]
pub struct Summary {
    by: Option<GroupBy>,
    read: usize,
    levels: LevelCounts,
    folded: usize,
    failing: Vec<(Option<String>, usize)>,
    index: HashMap<Option<String>, usize>,
}

/// The groups with errors a summary names before `and N more`.
const LISTED: usize = 10;

impl Summary {
    /// A summary; with `by`, it also names the groups with errors.
    pub fn new(by: Option<GroupBy>) -> Summary {
        Summary {
            by,
            ..Summary::default()
        }
    }

    /// Count `records` more read, kept or not.
    pub fn read(&mut self, records: usize) {
        self.read += records;
    }

    /// Count a record kept (shown, or folded into the one before it).
    pub fn shown(&mut self, record: &Record) {
        let severity = record.severity();
        self.levels.add(severity);
        let failed = matches!(severity, Some(Severity::Error | Severity::Fatal));
        if let (Some(by), true) = (self.by, failed) {
            let key = by.key_of(record);
            let next = self.failing.len();
            let at = *self.index.entry(key.clone()).or_insert(next);
            if at == next {
                self.failing.push((key, 0));
            }
            self.failing[at].1 += 1;
        }
    }

    /// Count `repeats` records folded into the one before them.
    pub fn folded(&mut self, repeats: usize) {
        self.folded += repeats;
    }

    pub fn records_read(&self) -> usize {
        self.read
    }

    pub fn records_shown(&self) -> usize {
        self.levels.total()
    }

    pub fn levels(&self) -> &LevelCounts {
        &self.levels
    }

    pub fn repeats_folded(&self) -> usize {
        self.folded
    }

    /// The groups with errors and how many, in the order their first error
    /// came; `None` is the records without an id.
    pub fn failing(&self) -> &[(Option<String>, usize)] {
        &self.failing
    }

    /// The summary's lines.
    pub fn lines(&self, console: &Console) -> Vec<Text> {
        let title = theme_style(console, "log.summary", "bold");
        let mut head = Text::new("");
        head.append("Summary: ", Some(title.into()));
        let shown = self.records_shown();
        if shown == self.read {
            head.append(&plural(shown, "record", "records"), None);
        } else {
            head.append(
                &format!(
                    "{shown} of {} shown",
                    plural(self.read, "record", "records")
                ),
                None,
            );
        }
        if self.folded > 0 {
            head.append(
                &format!(", {} folded", plural(self.folded, "repeat", "repeats")),
                Some(dim()),
            );
        }
        let mut lines = vec![head];
        if shown > 0 {
            let mut levels = Text::new("  ");
            levels = levels.append_text(&self.levels.text(console));
            lines.push(levels);
        }
        if let Some(by) = self.by {
            let mut line = Text::new("  ");
            if self.failing.is_empty() {
                line.append(&format!("no {} with errors", by.plural()), Some(dim()));
            } else {
                let error = theme_style(console, "log.summary.error", "bold red");
                let label = if self.failing.len() == 1 {
                    by.name().to_string()
                        + if by == GroupBy::Correlation {
                            " id"
                        } else {
                            ""
                        }
                } else {
                    by.plural().to_string()
                };
                line.append(
                    &format!("{} {label} with errors: ", self.failing.len()),
                    Some(error.into()),
                );
                for (at, (key, errors)) in self.failing.iter().take(LISTED).enumerate() {
                    if at > 0 {
                        line.append(", ", None);
                    }
                    let style = theme_style(console, "log.group.key", "bold magenta");
                    match key {
                        Some(key) => line.append(key, Some(style.into())),
                        None => line.append(&format!("no {} id", by.name()), Some(dim())),
                    }
                    line.append(
                        &format!(" ({})", plural(*errors, "error", "errors")),
                        Some(dim()),
                    );
                }
                if self.failing.len() > LISTED {
                    line.append(
                        &format!(" and {} more", self.failing.len() - LISTED),
                        Some(dim()),
                    );
                }
            }
            lines.push(line);
        }
        lines
    }
}

impl Renderable for Summary {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        for (at, line) in self.lines(console).into_iter().enumerate() {
            if at > 0 {
                segments.push(Segment::line());
            }
            let mut rendered = line.rich_render(console, options);
            // Each line ends with its own break, which the join replaces.
            while rendered.last().is_some_and(|s| s.text == "\n") {
                rendered.pop();
            }
            segments.extend(rendered);
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(level: &str, trace: Option<&str>, second: u32) -> Record {
        let mut record = Record::new("json")
            .field(
                "time",
                Value::String(format!("2026-10-18T10:00:{second:02}Z")),
            )
            .field("level", Value::String(level.into()))
            .field("msg", Value::String("m".into()));
        if let Some(trace) = trace {
            record.set("trace_id", Value::String(trace.into()));
        }
        record
    }

    fn plain(renderable: &dyn Renderable) -> String {
        Console::builder()
            .width(100)
            .no_color(true)
            .build()
            .render_to_string(renderable)
    }

    #[test]
    fn ids_come_from_common_keys_and_nested_maps() {
        let flat = Record::new("json").field("traceId", Value::String("t1".into()));
        assert_eq!(GroupBy::Trace.key_of(&flat).as_deref(), Some("t1"));
        let nested =
            Record::new("json").field("span", Value::Map(vec![("id".into(), Value::Integer(7))]));
        assert_eq!(GroupBy::Span.key_of(&nested).as_deref(), Some("7"));
        let request = Record::new("json").field("request_id", Value::String("r".into()));
        assert_eq!(GroupBy::Correlation.key_of(&request).as_deref(), Some("r"));
        let empty = Record::new("json").field("trace_id", Value::String(" ".into()));
        assert_eq!(GroupBy::Trace.key_of(&empty), None);
        let event = StructuredEvent::new(crate::event::Message::Literal("x".into())).context(
            crate::event::EventContext {
                correlation_id: Some("c9".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            GroupBy::Correlation.key_of_event(&event).as_deref(),
            Some("c9")
        );
        let spanned = StructuredEvent::new(crate::event::Message::Literal("x".into()))
            .spans(vec![crate::event::SpanContext::new("request")
                .field("trace_id", Value::String("t2".into()))]);
        assert_eq!(GroupBy::Trace.key_of_event(&spanned).as_deref(), Some("t2"));
        assert_eq!("span".parse::<GroupBy>(), Ok(GroupBy::Span));
        assert!("host".parse::<GroupBy>().is_err());
    }

    #[test]
    fn groups_keep_first_appearance_order_and_put_the_ungrouped_last() {
        let mut groups = Groups::new(GroupBy::Trace);
        groups.push(record("info", None, 0));
        groups.push(record("info", Some("a"), 1));
        groups.push(record("error", Some("b"), 2));
        groups.push(record("warn", Some("a"), 4));
        groups.push_continuation("  at main".into());
        assert_eq!(groups.len(), 3);
        let groups = groups.into_groups();
        let keys: Vec<_> = groups.iter().map(|g| g.key.as_deref()).collect();
        assert_eq!(keys, [Some("a"), Some("b"), None]);
        assert_eq!(groups[0].records[1].continuation, ["  at main"]);
        let header = plain(&groups[0].header(GroupBy::Trace));
        assert_eq!(header, "▸ trace a · 2 records · 1 warning · 3.000s");
        let header = plain(&groups[1].header(GroupBy::Trace));
        assert_eq!(header, "▸ trace b · 1 record · 1 error");
        let header = plain(&groups[2].header(GroupBy::Trace));
        assert_eq!(header, "▸ no trace id · 1 record");
        let streaming = plain(&GroupHeader::streaming(GroupBy::Span, Some("s".into())));
        assert_eq!(streaming, "▸ span s");
    }

    #[test]
    fn the_summary_counts_levels_and_names_failing_groups() {
        let mut summary = Summary::new(Some(GroupBy::Trace));
        summary.read(6);
        for record in [
            record("info", Some("a"), 0),
            record("error", Some("b"), 1),
            record("fatal", Some("b"), 2),
            record("error", None, 3),
            Record::text("plain"),
        ] {
            summary.shown(&record);
        }
        summary.folded(1);
        assert_eq!(summary.records_shown(), 5);
        assert_eq!(summary.levels().errors(), 3);
        assert_eq!(summary.failing(), [(Some("b".into()), 2), (None, 1)]);
        assert_eq!(
            plain(&summary),
            "Summary: 5 of 6 records shown, 1 repeat folded\n  fatal 1 · error 2 · info 1 · \
             no level 1\n  2 traces with errors: b (2 errors), no trace id (1 error)"
        );
        let mut quiet = Summary::new(Some(GroupBy::Correlation));
        quiet.read(1);
        quiet.shown(&record("info", None, 0));
        assert_eq!(
            plain(&quiet),
            "Summary: 1 record\n  info 1\n  no correlation ids with errors"
        );
        let mut many = Summary::new(Some(GroupBy::Trace));
        for n in 0..12 {
            many.read(1);
            many.shown(&record("error", Some(&format!("t{n}")), 0));
        }
        assert!(plain(&many).contains("12 traces with errors: t0 (1 error)"));
        assert!(plain(&many).contains("and 2 more"));
        let ungrouped = Summary::new(None);
        assert_eq!(plain(&ungrouped), "Summary: 0 records");
    }
}
