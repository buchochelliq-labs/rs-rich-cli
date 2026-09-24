//! Typed, ordered events. Time and context are supplied by the caller.
use crate::layout::{fit_segments, OverflowPolicy};
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Unsigned(u64),
    Integer128(i128),
    Unsigned128(u128),
    Float(f64),
    String(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}
impl Value {
    pub(crate) fn format(&self, expanded: bool, depth: usize) -> String {
        match self {
            Self::Null => "null".into(),
            Self::Bool(v) => v.to_string(),
            Self::Integer(v) => v.to_string(),
            Self::Unsigned(v) => v.to_string(),
            Self::Integer128(v) => v.to_string(),
            Self::Unsigned128(v) => v.to_string(),
            Self::Float(v) => v.to_string(),
            Self::String(v) => v.clone(),
            Self::List(values) => {
                let items: Vec<_> = values
                    .iter()
                    .map(|v| v.format(expanded, depth + 1))
                    .collect();
                collection("[", "]", items, expanded, depth)
            }
            Self::Map(values) => {
                let items: Vec<_> = values
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.format(expanded, depth + 1)))
                    .collect();
                collection("{", "}", items, expanded, depth)
            }
        }
    }
}
fn collection(open: &str, close: &str, items: Vec<String>, expanded: bool, depth: usize) -> String {
    if !expanded || items.is_empty() {
        return format!("{open}{}{close}", items.join(", "));
    }
    let indent = "  ".repeat(depth.saturating_add(1));
    format!(
        "{open}\n{indent}{}\n{}{close}",
        items.join(&format!(",\n{indent}")),
        "  ".repeat(depth)
    )
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Literal(String),
    Markup(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}
impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Fatal => "fatal",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventView {
    #[default]
    Compact,
    Expanded,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    pub path: String,
    pub line: usize,
    pub column: Option<usize>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventContext {
    pub timestamp: Option<String>,
    pub severity: Option<Severity>,
    pub target: Option<String>,
    pub module: Option<String>,
    pub source: Option<SourceLocation>,
    pub thread: Option<String>,
    pub task: Option<String>,
    pub correlation_id: Option<String>,
}
/// A span an event happened in: its name and its fields as last recorded.
#[derive(Clone, Debug, PartialEq)]
pub struct SpanContext {
    pub name: String,
    pub fields: Vec<(String, Value)>,
}
impl SpanContext {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: Vec::new(),
        }
    }
    pub fn field(mut self, key: impl Into<String>, value: Value) -> Self {
        self.fields.push((key.into(), value));
        self
    }
}
/// Marks an event that reports a span opening or closing rather than
/// something that happened inside one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanEvent {
    Open,
    /// The span closed after this long, from opening to closing.
    Close {
        elapsed: std::time::Duration,
    },
}
#[derive(Clone, Debug)]
pub struct StructuredEvent {
    diagnostics: Vec<crate::diagnostic::Diagnostic>,
    spans: Vec<SpanContext>,
    span_event: Option<SpanEvent>,
    pub message: Message,
    pub fields: Vec<(String, Value)>,
    pub context: EventContext,
    order: Vec<String>,
    hidden: Vec<String>,
    view: EventView,
    overflow: OverflowPolicy,
}
impl StructuredEvent {
    pub fn new(message: Message) -> Self {
        Self {
            diagnostics: Vec::new(),
            spans: Vec::new(),
            span_event: None,
            message,
            fields: Vec::new(),
            context: EventContext::default(),
            order: Vec::new(),
            hidden: Vec::new(),
            view: EventView::Compact,
            overflow: OverflowPolicy::Fold,
        }
    }
    pub fn diagnostic(mut self, diagnostic: crate::diagnostic::Diagnostic) -> Self {
        self.diagnostics.push(diagnostic);
        self
    }
    pub fn field(mut self, key: impl Into<String>, value: Value) -> Self {
        let key = key.into();
        if let Some((_, old)) = self.fields.iter_mut().find(|(name, _)| name == &key) {
            *old = value;
        } else {
            self.fields.push((key, value));
        }
        self
    }
    pub fn field_order(mut self, keys: Vec<String>) -> Self {
        self.order = keys;
        self
    }
    pub fn hide_fields(mut self, keys: Vec<String>) -> Self {
        self.hidden = keys;
        self
    }
    pub fn view(mut self, view: EventView) -> Self {
        self.view = view;
        self
    }
    pub fn context(mut self, context: EventContext) -> Self {
        self.context = context;
        self
    }
    pub fn overflow(mut self, overflow: OverflowPolicy) -> Self {
        self.overflow = overflow;
        self
    }
    /// The spans the event happened in, outermost first.
    pub fn spans(mut self, spans: Vec<SpanContext>) -> Self {
        self.spans = spans;
        self
    }
    /// Mark the event as a span opening or closing. Its message is the span's
    /// name, its fields the span's, and `spans` its parents.
    pub fn span_event(mut self, event: SpanEvent) -> Self {
        self.span_event = Some(event);
        self
    }
    /// The spans the event happened in, outermost first.
    pub fn span_context(&self) -> &[SpanContext] {
        &self.spans
    }
    /// Whether the event reports a span opening or closing.
    pub fn span_marker(&self) -> Option<SpanEvent> {
        self.span_event
    }
}
pub(crate) fn theme_style(console: &Console, key: &str, fallback: &str) -> Style {
    console
        .theme()
        .get(key)
        .cloned()
        .unwrap_or_else(|| Style::parse(fallback).unwrap_or_default())
}
pub(crate) fn flatten(lines: Vec<Vec<Segment>>) -> Vec<Segment> {
    let count = lines.len();
    lines
        .into_iter()
        .enumerate()
        .flat_map(|(i, mut row)| {
            if i + 1 < count {
                row.push(Segment::line());
            }
            row
        })
        .collect()
}
impl Renderable for StructuredEvent {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if options.max_width == 0 || options.height == Some(0) {
            return Vec::new();
        }
        let mut text = Text::new("");
        if let Some(timestamp) = &self.context.timestamp {
            text.append(&format!("{timestamp} "), None);
        }
        if let Some(severity) = self.context.severity {
            let fallback = match severity {
                Severity::Error | Severity::Fatal => "bold red",
                Severity::Warn => "yellow",
                Severity::Info => "green",
                _ => "dim",
            };
            text.append(
                &format!("{} ", severity.as_str()),
                Some(
                    theme_style(
                        console,
                        &format!("event.severity.{}", severity.as_str()),
                        fallback,
                    )
                    .into(),
                ),
            );
        }
        for (label, value) in [
            ("target", &self.context.target),
            ("module", &self.context.module),
            ("thread", &self.context.thread),
            ("task", &self.context.task),
            ("correlation", &self.context.correlation_id),
        ] {
            if let Some(value) = value {
                text.append(&format!("{label}={value} "), None);
            }
        }
        if let Some(source) = &self.context.source {
            text.append(
                &format!(
                    "{}:{}{} ",
                    source.path,
                    source.line,
                    source.column.map(|c| format!(":{c}")).unwrap_or_default()
                ),
                None,
            );
        }
        let mut message = match &self.message {
            Message::Literal(s) => Text::new(s),
            Message::Markup(s) => Text::from_markup(s).unwrap_or_else(|_| Text::new(s)),
        };
        message.set_base_style(theme_style(console, "event.message", ""));
        text = text.append_text(&message);
        let mut selected = Vec::new();
        for name in &self.order {
            if let Some(index) = self.fields.iter().position(|(k, _)| k == name) {
                if !selected.contains(&index) {
                    selected.push(index);
                }
            }
        }
        for index in 0..self.fields.len() {
            if !selected.contains(&index) {
                selected.push(index);
            }
        }
        for index in selected {
            let (name, value) = &self.fields[index];
            if self.hidden.contains(name) {
                continue;
            }
            text.append(
                if self.view == EventView::Compact {
                    " "
                } else {
                    "\n"
                },
                None,
            );
            text.append(
                &format!("{name}="),
                Some(theme_style(console, "event.field", "cyan").into()),
            );
            text.append(
                &value.format(self.view == EventView::Expanded, 0),
                Some(theme_style(console, "event.value", "").into()),
            );
        }
        let mut lines = fit_segments(
            &text.render(console.theme(), &Style::new()),
            options.max_width,
            self.overflow,
        );
        for diagnostic in &self.diagnostics {
            let mut diagnostic_options = options.clone();
            diagnostic_options.height = None;
            lines.extend(Segment::split_lines(
                &diagnostic.rich_render(console, &diagnostic_options),
            ));
        }
        if let Some(height) = options.height {
            lines.truncate(height);
        }
        flatten(lines)
    }
}
