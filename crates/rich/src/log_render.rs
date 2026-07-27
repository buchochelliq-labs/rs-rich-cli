//! Rendering log records.
//!
//! Rust-native reimagining of `rich/_log_render.py` + `rich/logging.py`.
//! Upstream integrates with Python's `logging`; [`LogRender`] instead formats a
//! single log record — an optional time, a severity-colored level, the message,
//! and an optional source path — into a styled line, using the same column
//! styles (`log.time`, `logging.level.*`, `log.path`).
//!
//! The formatter takes a [`LogLevel`] enum + strings rather than depending on the
//! `log`/`tracing` crates, keeping the core dependency-light. Wiring it into a
//! `log::Log` handler is a `rich-ext` follow-up. See docs/DIVERGENCES.md #19.

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;
use crate::text::Text;

/// Log severity, mirroring the `log` crate's five levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// The uppercase name and its style spec (from `logging.level.*`).
    fn styled(self) -> (&'static str, &'static str) {
        match self {
            LogLevel::Trace => ("TRACE", "dim"),
            LogLevel::Debug => ("DEBUG", "green"),
            LogLevel::Info => ("INFO", "blue"),
            LogLevel::Warn => ("WARN", "yellow"),
            LogLevel::Error => ("ERROR", "bold red"),
        }
    }
}

/// A single formatted log record. Mirrors the role of
/// `rich._log_render.LogRender` for one row.
pub struct LogRender {
    time: Option<String>,
    level: LogLevel,
    message: String,
    path: Option<String>,
}

impl LogRender {
    /// A record at `level` with `message`.
    pub fn new(level: LogLevel, message: impl Into<String>) -> Self {
        LogRender {
            time: None,
            level,
            message: message.into(),
            path: None,
        }
    }

    /// Add a leading time column (styled `log.time` = dim cyan).
    pub fn time(mut self, time: impl Into<String>) -> Self {
        self.time = Some(time.into());
        self
    }

    /// Add a trailing source path (styled `log.path` = dim).
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    fn content(&self) -> Text {
        let mut text = Text::new("");
        if let Some(time) = &self.time {
            text.append(time, Style::parse("dim cyan").ok());
            text.append(" ", None);
        }
        let (name, spec) = self.level.styled();
        // Pad the level name so messages line up (level names are ≤ 5 cells).
        text.append(&format!("{name:<5}"), Style::parse(spec).ok());
        text.append(" ", None);
        text.append(&self.message, None);
        if let Some(path) = &self.path {
            text.append(&format!("  {path}"), Style::parse("dim").ok());
        }
        text
    }
}

impl Renderable for LogRender {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.content().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.content().measure(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(record: &LogRender) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .build()
            .render_to_string(record)
    }

    #[test]
    fn info_record_is_blue_level() {
        let out = render(&LogRender::new(LogLevel::Info, "server started"));
        // "INFO " padded, styled blue (34).
        assert!(out.contains("\x1b[34mINFO \x1b[0m"), "got {out:?}");
        assert!(out.contains("server started"));
    }

    #[test]
    fn error_level_is_bold_red() {
        let out = render(&LogRender::new(LogLevel::Error, "boom"));
        assert!(out.contains("\x1b[1;31mERROR\x1b[0m"), "got {out:?}");
    }

    #[test]
    fn time_and_path_columns() {
        let out = render(
            &LogRender::new(LogLevel::Warn, "low disk")
                .time("12:00:00")
                .path("main.rs:42"),
        );
        // Time is dim cyan (2;36), path is dim (2).
        assert!(out.contains("\x1b[2;36m12:00:00\x1b[0m"), "got {out:?}");
        assert!(out.contains("main.rs:42"));
        assert!(out.contains("\x1b[33mWARN \x1b[0m"));
    }
}
