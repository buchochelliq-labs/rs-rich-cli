//! Rendering log records.
//!
//! Port of upstream `rich/_log_render.py`. A [`LogRender`] lays one log record
//! out as a borderless [`Table::grid`] row: an optional time (blanked when it
//! repeats the previous record's), an optional fixed-width level, the message
//! (which takes the remaining width and folds), and an optional `path:line`
//! linked to the source file. It remembers the last time it showed, as the
//! upstream callable does.
//!
//! Upstream formats a `datetime` with `strftime`; this port takes the time
//! already formatted, keeping the core free of a date-time dependency. The
//! `rich-ext` log and tracing handlers format record times for it.
//! [`LogRecord`] is a one-record convenience over it.

use std::cell::RefCell;
use std::sync::Arc;

use crate::console::{Console, ConsoleOptions, Overflow};
use crate::containers::Renderables;
use crate::measure::Measurement;
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::{Style, StyleType};
use crate::table::{Cell, Table};
use crate::text::Text;

/// Lays out log records. Port of `rich._log_render.LogRender`.
pub struct LogRender {
    show_time: bool,
    show_level: bool,
    show_path: bool,
    omit_repeated_times: bool,
    level_width: Option<usize>,
    last_time: RefCell<Option<Text>>,
}

impl Default for LogRender {
    fn default() -> Self {
        LogRender {
            show_time: true,
            show_level: false,
            show_path: true,
            omit_repeated_times: true,
            level_width: Some(8),
            last_time: RefCell::new(None),
        }
    }
}

impl LogRender {
    /// Upstream's defaults: time and path shown, level hidden, repeated times
    /// omitted, level column 8 cells wide.
    pub fn new() -> Self {
        LogRender::default()
    }

    /// Show the time column (upstream `show_time`).
    pub fn show_time(mut self, show: bool) -> Self {
        self.show_time = show;
        self
    }

    /// Show the level column (upstream `show_level`).
    pub fn show_level(mut self, show: bool) -> Self {
        self.show_level = show;
        self
    }

    /// Show the path column (upstream `show_path`).
    pub fn show_path(mut self, show: bool) -> Self {
        self.show_path = show;
        self
    }

    /// Blank a time equal to the previous record's (upstream
    /// `omit_repeated_times`).
    pub fn omit_repeated_times(mut self, omit: bool) -> Self {
        self.omit_repeated_times = omit;
        self
    }

    /// The level column's width, or `None` to fit its content (upstream
    /// `level_width`).
    pub fn level_width(mut self, width: Option<usize>) -> Self {
        self.level_width = width;
        self
    }

    /// Lay out one record. Port of `LogRender.__call__`: `time` is the
    /// formatted log time, `level` the styled level text (see [`level_text`]),
    /// and `link_path` makes the path a `file://` hyperlink.
    ///
    /// The message is a single `Text`; [`render_renderables`](Self::render_renderables)
    /// takes any renderables, as upstream's `renderables` argument does.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        console: &Console,
        message: Text,
        time: Option<Text>,
        level: Text,
        path: Option<&str>,
        line_no: Option<u32>,
        link_path: Option<&str>,
    ) -> Table {
        self.render_cell(
            console,
            Cell::Text(message),
            time,
            level,
            path,
            line_no,
            link_path,
        )
    }

    /// Lay out one record whose message is any sequence of renderables. Port
    /// of `LogRender.__call__` with its `renderables` argument: they render
    /// one after another in the message column, as upstream's
    /// `Renderables(renderables)` cell does. `Console.log` of a table, a panel
    /// or `log_locals`' scope goes through here.
    #[allow(clippy::too_many_arguments)]
    pub fn render_renderables(
        &self,
        console: &Console,
        renderables: Vec<Arc<dyn Renderable + Send + Sync>>,
        time: Option<Text>,
        level: Text,
        path: Option<&str>,
        line_no: Option<u32>,
        link_path: Option<&str>,
    ) -> Table {
        self.render_cell(
            console,
            Cell::Renderable(Arc::new(Renderables::new(renderables))),
            time,
            level,
            path,
            line_no,
            link_path,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_cell(
        &self,
        console: &Console,
        message: Cell,
        time: Option<Text>,
        level: Text,
        path: Option<&str>,
        line_no: Option<u32>,
        link_path: Option<&str>,
    ) -> Table {
        let style = |name: &str| {
            console
                .get_style(&StyleType::from(name))
                .unwrap_or_default()
        };
        let mut output = Table::grid().padding(0, 1, 0, 1).expand(true);
        if self.show_time {
            output.add_column("").column_style(style("log.time"));
        }
        if self.show_level {
            output.add_column("").column_style(style("log.level"));
            if let Some(width) = self.level_width {
                output.column_width(width);
            }
        }
        output
            .add_column("")
            .column_ratio(1)
            .column_style(style("log.message"))
            .column_overflow(Overflow::Fold);
        let path = path
            .filter(|_| self.show_path)
            .filter(|path| !path.is_empty());
        if path.is_some() {
            output.add_column("").column_style(style("log.path"));
        }

        let mut row: Vec<Cell> = Vec::new();
        if self.show_time {
            let display = time.unwrap_or_default();
            let mut last = self.last_time.borrow_mut();
            let repeated = last.as_ref().is_some_and(|last| same_text(last, &display));
            if repeated && self.omit_repeated_times {
                row.push(Cell::Text(Text::new(
                    " ".repeat(display.plain().chars().count()),
                )));
            } else {
                row.push(Cell::Text(display.clone()));
                *last = Some(display);
            }
        }
        if self.show_level {
            row.push(Cell::Text(level));
        }
        row.push(message);
        if let Some(path) = path {
            let link = |target: String| Style::new().with_link(target);
            let mut path_text = Text::new("");
            path_text.append(
                path,
                link_path.map(|link_path| link(format!("file://{link_path}")).into()),
            );
            if let Some(line_no) = line_no.filter(|line| *line > 0) {
                path_text.append(":", None);
                path_text.append(
                    &line_no.to_string(),
                    link_path.map(|link_path| link(format!("file://{link_path}#{line_no}")).into()),
                );
            }
            row.push(Cell::Text(path_text));
        }
        output.add_row_cells(row);
        output
    }
}

/// `Text.__eq__`: the same characters and the same spans.
fn same_text(a: &Text, b: &Text) -> bool {
    a.plain() == b.plain() && a.spans() == b.spans()
}

/// A level name as upstream's `RichHandler.get_level_text` styles it: padded
/// to 8 cells, in `logging.level.<name>`.
pub fn level_text(name: &str) -> Text {
    Text::styled(
        format!("{name:<8}"),
        format!("logging.level.{}", name.to_lowercase()),
    )
}

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
    /// The level name as Python's `logging` spells it (`WARNING`, not `WARN`);
    /// `TRACE`, which Python lacks, uses `logging.level.notset`.
    pub fn name(self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARNING",
            LogLevel::Error => "ERROR",
        }
    }

    /// The styled level column for this level.
    pub fn text(self) -> Text {
        match self {
            LogLevel::Trace => Text::styled(format!("{:<8}", "TRACE"), "logging.level.notset"),
            level => level_text(level.name()),
        }
    }
}

/// One log record, rendered through a fresh [`LogRender`] with the level
/// shown. A convenience for printing a single record; a stream of records
/// should share one [`LogRender`] so repeated times are omitted.
pub struct LogRecord {
    level: LogLevel,
    message: String,
    time: Option<String>,
    path: Option<String>,
    line_no: Option<u32>,
}

impl LogRecord {
    /// A record at `level` with `message`, which is plain text.
    pub fn new(level: LogLevel, message: impl Into<String>) -> Self {
        LogRecord {
            level,
            message: message.into(),
            time: None,
            path: None,
            line_no: None,
        }
    }

    /// The formatted time for the time column.
    pub fn time(mut self, time: impl Into<String>) -> Self {
        self.time = Some(time.into());
        self
    }

    /// The source path for the path column.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The source line, shown after the path.
    pub fn line(mut self, line: u32) -> Self {
        self.line_no = Some(line);
        self
    }

    fn table(&self, console: &Console) -> Table {
        LogRender::new()
            .show_level(true)
            .show_time(self.time.is_some())
            .render(
                console,
                Text::new(self.message.clone()),
                self.time.as_deref().map(Text::new),
                self.level.text(),
                self.path.as_deref(),
                self.line_no,
                None,
            )
    }
}

impl Renderable for LogRecord {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.table(console).rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.table(console).measure(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .highlight(false)
            .build()
    }

    #[test]
    fn a_repeated_time_is_blanked() {
        let console = console();
        let render = LogRender::new();
        let first = console.render_to_string(&render.render(
            &console,
            Text::new("one"),
            Some(Text::new("[12:00]")),
            Text::new(""),
            None,
            None,
            None,
        ));
        let second = console.render_to_string(&render.render(
            &console,
            Text::new("two"),
            Some(Text::new("[12:00]")),
            Text::new(""),
            None,
            None,
            None,
        ));
        assert!(first.contains("[12:00]"), "{first:?}");
        assert!(!second.contains("[12:00]"), "{second:?}");
        assert!(second.contains("two"));
    }

    #[test]
    fn warn_uses_pythons_level_name() {
        let out = console().render_to_string(&LogRecord::new(LogLevel::Warn, "low disk"));
        assert!(out.contains("\x1b[33mWARNING \x1b[0m"), "{out:?}");
        assert!(out.contains("low disk"));
    }
}
