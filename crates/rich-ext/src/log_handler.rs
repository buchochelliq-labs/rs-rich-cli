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
//! Upstream formats `record.created` in local time with `[%x %X]`. Local time
//! needs a time-zone dependency, so the default here is UTC `[HH:MM:SS]`; an
//! event's own `timestamp` or a [`RichHandler::time_format`] closure replaces
//! it. Traceback rendering (`rich_tracebacks`) has no Rust counterpart.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rich::{Console, Highlighter, LogRender, ReprHighlighter, Table, Text};

use crate::event::{Message, Severity, StructuredEvent};

/// Upstream `RichHandler.KEYWORDS`: HTTP methods, styled `logging.keyword`.
pub const KEYWORDS: &[&str] = &[
    "GET", "POST", "HEAD", "PUT", "DELETE", "OPTIONS", "TRACE", "PATCH",
];

type TimeFormat = Box<dyn Fn() -> String + Send + Sync>;

/// Renders log events like upstream's `RichHandler`.
pub struct RichHandler {
    console: Mutex<Console>,
    render: Mutex<LogRender>,
    highlighter: Option<Box<dyn Highlighter + Send + Sync>>,
    markup: bool,
    keywords: Vec<String>,
    enable_link_path: bool,
    time_format: TimeFormat,
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
        }
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
        let mut text = match &event.message {
            Message::Literal(message) if !self.markup => Text::new(message.clone()),
            Message::Literal(markup) | Message::Markup(markup) => {
                Text::from_markup(markup).unwrap_or_else(|_| Text::new(markup.clone()))
            }
        };
        for (key, value) in &event.fields {
            text.append(&format!(" {key}={}", value.format(false, 0)), None);
        }
        if let Some(highlighter) = &self.highlighter {
            highlighter.highlight(&mut text);
        }
        if !self.keywords.is_empty() {
            let words: Vec<&str> = self.keywords.iter().map(String::as_str).collect();
            let _ = text.highlight_words(&words, "logging.keyword", true);
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
            self.render_message(event),
            Some(Text::new(time)),
            level,
            name,
            source.and_then(|source| u32::try_from(source.line).ok()),
            full_path.filter(|_| self.enable_link_path),
        )
    }

    /// Print one event. Port of `RichHandler.emit`.
    pub fn emit_event(&self, event: &StructuredEvent) {
        let console = self.console.lock().unwrap_or_else(|e| e.into_inner());
        let table = self.render_with(&console, event);
        console.print(&table);
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
