//! A log sink in the style of upstream `rich.logging.RichHandler`.
//!
//! [`RichHandler`] renders [`StructuredEvent`]s through core's
//! [`LogRender`]: the time (blanked when it repeats), the
//! level padded to 8 cells in `logging.level.<name>`, the message run through
//! a highlighter and keyword highlighting, and the source file name with its
//! line, linked to the full path. Behind the `log` and `tracing` features it
//! is an `adapters::EventSink`, so the existing `LogAdapter` and `EventLayer`
//! print through it.
//!
//! Beyond upstream, which has no spans:
//!
//! - An event's [spans](StructuredEvent::span_context) show before its message
//!   (`outer{id=7}:inner: message`), or as tree guides with
//!   [`SpanView::Tree`], where span open and close events draw the branches.
//! - [`RichHandler::hyperlinker`] links the path column through a
//!   [`Hyperlinker`], so an editor URL template or a base directory for
//!   relative paths applies.
//! - [`RichHandler::live`] prints through a [`LiveCoordinator`], above its
//!   regions, instead of writing to the console under them.
//!
//! Upstream formats `record.created` in local time with `[%x %X]`. Local time
//! needs a time-zone dependency, so the default here is UTC `[HH:MM:SS]`; an
//! event's own `timestamp` or a [`RichHandler::time_format`] closure replaces
//! it. Traceback rendering (`rich_tracebacks`) has no Rust counterpart.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rich::{
    Console, ConsoleOptions, Highlighter, LogRender, ReprHighlighter, Segment, Table, Text,
};

use crate::event::{Message, Severity, SpanContext, SpanEvent, StructuredEvent};
use crate::hyperlink::Hyperlinker;
use crate::live::{LiveCoordinator, LiveError};

/// Upstream `RichHandler.KEYWORDS`: HTTP methods, styled `logging.keyword`.
pub const KEYWORDS: &[&str] = &[
    "GET", "POST", "HEAD", "PUT", "DELETE", "OPTIONS", "TRACE", "PATCH",
];

type TimeFormat = Box<dyn Fn() -> String + Send + Sync>;
type Output = Box<dyn Fn(&[Segment]) -> std::io::Result<()> + Send + Sync>;

/// How an event's spans are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpanView {
    /// Before the message, outermost first: `outer{id=7}:inner: message`.
    #[default]
    Inline,
    /// As guides: each span indents the events inside it by one `│ `, its
    /// open event draws `┌ name field=value` and its close event
    /// `└ name 1.20ms`.
    Tree,
    /// Not shown.
    Hidden,
}

/// Renders log events like upstream's `RichHandler`.
pub struct RichHandler {
    console: Mutex<Console>,
    render: Mutex<LogRender>,
    highlighter: Option<Box<dyn Highlighter + Send + Sync>>,
    markup: bool,
    keywords: Vec<String>,
    enable_link_path: bool,
    time_format: TimeFormat,
    span_view: SpanView,
    hyperlinker: Option<Hyperlinker>,
    output: Option<Output>,
}

impl RichHandler {
    /// A handler printing to `console`, with upstream's defaults: time, level
    /// and path shown, repeated times omitted, [`ReprHighlighter`], no markup,
    /// [`KEYWORDS`] highlighted and paths linked.
    pub fn new(console: Console) -> Self {
        RichHandler {
            console: Mutex::new(console),
            render: Mutex::new(LogRender::new().show_level(true)),
            highlighter: Some(Box::new(ReprHighlighter::new())),
            markup: false,
            keywords: KEYWORDS.iter().map(|word| word.to_string()).collect(),
            enable_link_path: true,
            time_format: Box::new(utc_time),
            span_view: SpanView::Inline,
            hyperlinker: None,
            output: None,
        }
    }

    /// How an event's spans are shown (default [`SpanView::Inline`]).
    pub fn span_view(mut self, view: SpanView) -> Self {
        self.span_view = view;
        self
    }

    /// Link the path column through `hyperlinker` instead of a bare `file://`
    /// URL: its editor template, base directory for relative paths (as
    /// `tracing` reports them) and on/off switch apply.
    pub fn hyperlinker(mut self, hyperlinker: Hyperlinker) -> Self {
        self.hyperlinker = Some(hyperlinker);
        self
    }

    /// Print through `live`, above its regions, so log lines never tear a
    /// live display. Render with a console as wide as `live`'s target (see
    /// `RenderTarget::console`); longer lines fold.
    pub fn live<W: Write + Send + 'static>(mut self, live: Arc<Mutex<LiveCoordinator<W>>>) -> Self {
        self.output = Some(Box::new(move |segments| {
            live.lock()
                .unwrap_or_else(|e| e.into_inner())
                .print(segments)
                .map_err(|error| match error {
                    LiveError::Io(error) => error,
                    other => std::io::Error::other(other.to_string()),
                })
        }));
        self
    }

    fn map_render(self, f: impl FnOnce(LogRender) -> LogRender) -> Self {
        let render = self.render.into_inner().unwrap_or_else(|e| e.into_inner());
        RichHandler {
            render: Mutex::new(f(render)),
            ..self
        }
    }

    /// Show the time column (upstream `show_time`).
    pub fn show_time(self, show: bool) -> Self {
        self.map_render(|render| render.show_time(show))
    }

    /// Show the level column (upstream `show_level`).
    pub fn show_level(self, show: bool) -> Self {
        self.map_render(|render| render.show_level(show))
    }

    /// Show the path column (upstream `show_path`).
    pub fn show_path(self, show: bool) -> Self {
        self.map_render(|render| render.show_path(show))
    }

    /// Blank a time equal to the previous record's (upstream
    /// `omit_repeated_times`).
    pub fn omit_repeated_times(self, omit: bool) -> Self {
        self.map_render(|render| render.omit_repeated_times(omit))
    }

    /// The level column's width, or `None` to fit (upstream `log_time_format`'s
    /// sibling `level_width`, fixed at 8 upstream).
    pub fn level_width(self, width: Option<usize>) -> Self {
        self.map_render(|render| render.level_width(width))
    }

    /// Parse messages as console markup (upstream `markup`).
    pub fn markup(mut self, markup: bool) -> Self {
        self.markup = markup;
        self
    }

    /// The message highlighter, or `None` for none (upstream `highlighter`).
    pub fn highlighter(mut self, highlighter: Option<Box<dyn Highlighter + Send + Sync>>) -> Self {
        self.highlighter = highlighter;
        self
    }

    /// Words styled `logging.keyword` (upstream `keywords`).
    pub fn keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    /// Link the path column to the source file (upstream `enable_link_path`).
    pub fn enable_link_path(mut self, enable: bool) -> Self {
        self.enable_link_path = enable;
        self
    }

    /// Format the current time for events without a `timestamp` (upstream
    /// `log_time_format`).
    pub fn time_format(mut self, format: impl Fn() -> String + Send + Sync + 'static) -> Self {
        self.time_format = Box::new(format);
        self
    }

    /// The level column. Port of `RichHandler.get_level_text`, with Python's
    /// level names (`WARNING`, `CRITICAL`); `TRACE` uses `logging.level.notset`.
    pub fn level_text(severity: Severity) -> Text {
        let name = match severity {
            Severity::Trace => {
                return Text::styled(format!("{:<8}", "TRACE"), "logging.level.notset")
            }
            Severity::Debug => "DEBUG",
            Severity::Info => "INFO",
            Severity::Warn => "WARNING",
            Severity::Error => "ERROR",
            Severity::Fatal => "CRITICAL",
        };
        rich::level_text(name)
    }

    /// The message column. Port of `RichHandler.render_message`; structured
    /// fields follow the message as `key=value`.
    pub fn render_message(&self, event: &StructuredEvent) -> Text {
        let ascii = self
            .console
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ascii_only();
        self.message_text(event, ascii)
    }

    /// [`render_message`](Self::render_message) with tree guides in ASCII when
    /// `ascii`; callers holding the console lock pass it in.
    fn message_text(&self, event: &StructuredEvent, ascii: bool) -> Text {
        let spans = event.span_context();
        let mut text = match event.span_marker() {
            Some(marker) => self.span_line(event, marker, ascii),
            None => {
                let mut text = self.prefix(spans, ascii);
                text = text.append_text(&match &event.message {
                    Message::Literal(message) if !self.markup => Text::new(message.clone()),
                    Message::Literal(markup) | Message::Markup(markup) => {
                        Text::from_markup(markup).unwrap_or_else(|_| Text::new(markup.clone()))
                    }
                });
                for (key, value) in &event.fields {
                    text.append(&format!(" {key}={}", value.format(false, 0)), None);
                }
                text
            }
        };
        if let Some(highlighter) = &self.highlighter {
            highlighter.highlight(&mut text);
        }
        if !self.keywords.is_empty() {
            let words: Vec<&str> = self.keywords.iter().map(String::as_str).collect();
            let _ = text.highlight_words(&words, "logging.keyword", true);
        }
        text
    }

    /// What comes before a message for `spans`: the span chain, tree guides
    /// or nothing, as [`SpanView`] says.
    fn prefix(&self, spans: &[SpanContext], ascii: bool) -> Text {
        let mut text = Text::new("");
        match self.span_view {
            SpanView::Hidden => {}
            SpanView::Tree => text.append(&guide(ascii).repeat(spans.len()), Some("dim".into())),
            SpanView::Inline if spans.is_empty() => {}
            SpanView::Inline => {
                for (index, span) in spans.iter().enumerate() {
                    if index > 0 {
                        text.append(":", Some("dim".into()));
                    }
                    text = text.append_text(&span_label(span, "{", "}"));
                }
                text.append(": ", Some("dim".into()));
            }
        }
        text
    }

    /// The message of a span open or close event: `event`'s message is the
    /// span's name, its fields the span's and its spans the span's parents.
    fn span_line(&self, event: &StructuredEvent, marker: SpanEvent, ascii: bool) -> Text {
        let name = match &event.message {
            Message::Literal(name) | Message::Markup(name) => name.clone(),
        };
        let span = SpanContext {
            name,
            fields: event.fields.clone(),
        };
        let parents = event.span_context();
        let elapsed = match marker {
            SpanEvent::Open => None,
            SpanEvent::Close { elapsed } => Some(elapsed),
        };
        let mut text = Text::new("");
        match self.span_view {
            SpanView::Tree => {
                text.append(&guide(ascii).repeat(parents.len()), Some("dim".into()));
                let corner = match (elapsed.is_some(), ascii) {
                    (false, false) => "┌ ",
                    (true, false) => "└ ",
                    (false, true) => "+ ",
                    (true, true) => "` ",
                };
                text.append(corner, Some("dim".into()));
                // The open line shows the fields; the close line only times it.
                if elapsed.is_some() {
                    text = text.append_text(&Text::styled(span.name.clone(), "bold"));
                } else {
                    text = text.append_text(&span_label(&span, " ", ""));
                }
            }
            SpanView::Inline | SpanView::Hidden => {
                if self.span_view == SpanView::Inline {
                    for parent in parents {
                        text = text.append_text(&span_label(parent, "{", "}"));
                        text.append(":", Some("dim".into()));
                    }
                }
                text = text.append_text(&span_label(&span, "{", "}"));
                text.append(
                    if elapsed.is_some() {
                        " closed"
                    } else {
                        " opened"
                    },
                    Some("dim".into()),
                );
            }
        }
        if let Some(elapsed) = elapsed {
            text.append(" ", None);
            text.append(&format_elapsed(elapsed), Some("dim".into()));
        }
        text
    }

    /// Lay out one event. Port of `RichHandler.render`.
    pub fn render(&self, event: &StructuredEvent) -> Table {
        let console = self.console.lock().unwrap_or_else(|e| e.into_inner());
        self.render_with(&console, event)
    }

    fn render_with(&self, console: &Console, event: &StructuredEvent) -> Table {
        let context = &event.context;
        let time = context
            .timestamp
            .clone()
            .unwrap_or_else(|| (self.time_format)());
        let level = Self::level_text(context.severity.unwrap_or(Severity::Info));
        let source = context.source.as_ref();
        let full_path = source.map(|source| source.path.as_str());
        let name = full_path.map(|path| path.rsplit(['/', '\\']).next().unwrap_or(path));
        let render = self.render.lock().unwrap_or_else(|e| e.into_inner());
        render.render(
            console,
            self.message_text(event, console.ascii_only()),
            Some(Text::new(time)),
            level,
            name,
            source.and_then(|source| u32::try_from(source.line).ok()),
            full_path.filter(|_| self.enable_link_path),
        )
    }

    /// Print one event. Port of `RichHandler.emit`.
    pub fn emit_event(&self, event: &StructuredEvent) {
        let _ = self.try_emit(event);
    }

    fn try_emit(&self, event: &StructuredEvent) -> std::io::Result<()> {
        let console = self.console.lock().unwrap_or_else(|e| e.into_inner());
        let table = self.render_with(&console, event);
        if self.hyperlinker.is_none() && self.output.is_none() {
            console.print(&table);
            return Ok(());
        }
        let mut lines = console.render_lines(&table, &console.options(), false);
        if let (Some(hyperlinker), Some(source)) = (&self.hyperlinker, &event.context.source) {
            let line = Some(source.line).filter(|&line| line > 0);
            let url = hyperlinker.file_url(&source.path, line, None);
            relink(&mut lines, &source.path, url.as_deref());
        }
        match &self.output {
            Some(output) => output(&join_lines(lines)),
            None => {
                console.print(&Lines(lines));
                Ok(())
            }
        }
    }
}

/// One tree level: `│ ` (or `| ` in ASCII).
fn guide(ascii: bool) -> &'static str {
    if ascii {
        "| "
    } else {
        "│ "
    }
}

/// `name{a=1 b=2}` (or `name a=1 b=2`, with `open` = `" "` and `close` = `""`),
/// the name bold.
fn span_label(span: &SpanContext, open: &str, close: &str) -> Text {
    let mut text = Text::styled(span.name.clone(), "bold");
    if !span.fields.is_empty() {
        let fields: Vec<String> = span
            .fields
            .iter()
            .map(|(key, value)| format!("{key}={}", value.format(false, 0)))
            .collect();
        text.append(open, Some("dim".into()));
        text.append(&fields.join(" "), None);
        text.append(close, Some("dim".into()));
    }
    text
}

/// `850µs`, `1.20ms` or `2.50s`.
fn format_elapsed(elapsed: Duration) -> String {
    let micros = elapsed.as_secs_f64() * 1e6;
    if micros < 1000.0 {
        format!("{micros:.0}µs")
    } else if micros < 1e6 {
        format!("{:.2}ms", micros / 1e3)
    } else {
        format!("{:.2}s", micros / 1e6)
    }
}

/// Point the path column's `file://` link (from `LogRender`) at `url`, or
/// drop it when the hyperlinker is off.
fn relink(lines: &mut [Vec<Segment>], path: &str, url: Option<&str>) {
    let bare = format!("file://{path}");
    for segment in lines.iter_mut().flatten() {
        let Some(style) = &segment.style else {
            continue;
        };
        let linked = style
            .link()
            .is_some_and(|link| link == bare || link.starts_with(&format!("{bare}#")));
        if linked {
            segment.style = Some(style.update_link(url.map(str::to_owned)));
        }
    }
}

fn join_lines(lines: Vec<Vec<Segment>>) -> Vec<Segment> {
    let mut segments = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        if index > 0 {
            segments.push(Segment::line());
        }
        segments.extend(line);
    }
    segments
}

/// Lines already rendered, printed as they are.
struct Lines(Vec<Vec<Segment>>);

impl rich::Renderable for Lines {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        join_lines(self.0.clone())
    }
}

#[cfg(any(feature = "log", feature = "tracing"))]
impl crate::adapters::EventSink for RichHandler {
    fn emit(&self, event: StructuredEvent) -> std::io::Result<()> {
        self.emit_event(&event);
        Ok(())
    }
}

/// UTC `[HH:MM:SS]`.
fn utc_time() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let day = secs % 86_400;
    format!("[{:02}:{:02}:{:02}]", day / 3600, day / 60 % 60, day % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{EventContext, SourceLocation, Value};

    fn handler() -> RichHandler {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Truecolor))
            .width(60)
            .build();
        RichHandler::new(console).time_format(|| "[12:00:00]".into())
    }

    fn event(message: &str, severity: Severity) -> StructuredEvent {
        StructuredEvent::new(Message::Literal(message.into())).context(EventContext {
            severity: Some(severity),
            source: Some(SourceLocation {
                path: "src/server/main.rs".into(),
                line: 42,
                column: None,
            }),
            ..Default::default()
        })
    }

    #[test]
    fn renders_time_level_message_and_file_name() {
        let handler = handler().enable_link_path(false);
        let console = handler.console.lock().unwrap();
        let plain = console
            .render_to_string(&handler.render_with(&console, &event("GET /index", Severity::Warn)));
        assert!(plain.contains("[12:00:00]"), "{plain:?}");
        assert!(plain.contains("WARNING"), "{plain:?}");
        assert!(plain.contains("main.rs:42"), "{plain:?}");
        assert!(!plain.contains("src/server"), "{plain:?}");
        // `logging.keyword` is bold yellow.
        assert!(plain.contains("\x1b[1;33mGET\x1b[0m"), "{plain:?}");
    }

    #[test]
    fn links_the_full_path_and_appends_fields() {
        let handler = handler();
        let console = handler.console.lock().unwrap();
        let event = event("ready", Severity::Info).field("count", Value::Integer(3));
        let out = console.render_to_string(&handler.render_with(&console, &event));
        assert!(out.contains("file://src/server/main.rs#42"), "{out:?}");
        assert!(out.contains("\x1b[33mcount\x1b[0m=\x1b[1;36m3"), "{out:?}");
    }

    #[test]
    fn fatal_is_critical_and_trace_is_notset() {
        assert_eq!(RichHandler::level_text(Severity::Fatal).plain(), "CRITICAL");
        let trace = RichHandler::level_text(Severity::Trace);
        assert_eq!(trace.plain(), "TRACE   ");
    }
}
