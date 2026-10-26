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

/// Format `time` as Python's `datetime.strftime(format)` formats a naive
/// datetime holding the same UTC wall-clock time, in the C locale: the time
/// column of `Console.log` (upstream's default `"[%X]"`).
///
/// The C89 directives and glibc's common extensions are supported (`%a %A
/// %b %B %c %C %d %D %e %f %F %h %H %I %j %m %M %n %p %r %R %S %t %T %u %U
/// %w %W %x %X %y %Y %z %Z %%`); `%z` and `%Z` are empty, as for a naive
/// datetime, and any other directive is written as it is, as glibc does.
pub(crate) fn strftime(time: std::time::SystemTime, format: &str) -> String {
    let (seconds, micros) = match time.duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => (since.as_secs() as i64, since.subsec_micros()),
        Err(before) => {
            let before = before.duration();
            let mut seconds = -(before.as_secs() as i64);
            let mut micros = before.subsec_micros();
            if micros > 0 {
                seconds -= 1;
                micros = 1_000_000 - micros;
            }
            (seconds, micros)
        }
    };
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (of_day / 3600, of_day / 60 % 60, of_day % 60);
    // 1970-01-01 was a Thursday; 0 is Sunday, as `%w`.
    let weekday = (days + 4).rem_euclid(7);
    let year_day = days - days_from_civil(year, 1, 1);
    let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
    const DAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let day_name = DAYS[weekday as usize];
    let month_name = MONTHS[(month - 1) as usize];
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        let Some(directive) = chars.next() else {
            out.push('%');
            break;
        };
        let piece = match directive {
            'a' => day_name[..3].to_string(),
            'A' => day_name.to_string(),
            'b' | 'h' => month_name[..3].to_string(),
            'B' => month_name.to_string(),
            'c' => format!(
                "{} {} {day:>2} {hour:02}:{minute:02}:{second:02} {year}",
                &day_name[..3],
                &month_name[..3]
            ),
            'C' => format!("{:02}", year.div_euclid(100)),
            'd' => format!("{day:02}"),
            'D' | 'x' => format!("{month:02}/{day:02}/{:02}", year.rem_euclid(100)),
            'e' => format!("{day:>2}"),
            'f' => format!("{micros:06}"),
            'F' => format!("{year}-{month:02}-{day:02}"),
            'H' => format!("{hour:02}"),
            'I' => format!("{hour12:02}"),
            'j' => format!("{:03}", year_day + 1),
            'm' => format!("{month:02}"),
            'M' => format!("{minute:02}"),
            'n' => "\n".to_string(),
            'p' => if hour < 12 { "AM" } else { "PM" }.to_string(),
            'r' => format!(
                "{hour12:02}:{minute:02}:{second:02} {}",
                if hour < 12 { "AM" } else { "PM" }
            ),
            'R' => format!("{hour:02}:{minute:02}"),
            'S' => format!("{second:02}"),
            't' => "\t".to_string(),
            'T' | 'X' => format!("{hour:02}:{minute:02}:{second:02}"),
            'u' => (if weekday == 0 { 7 } else { weekday }).to_string(),
            'U' => format!("{:02}", (year_day + 7 - weekday) / 7),
            'w' => weekday.to_string(),
            'W' => format!("{:02}", (year_day + 7 - (weekday + 6) % 7) / 7),
            'y' => format!("{:02}", year.rem_euclid(100)),
            'Y' => year.to_string(),
            'z' | 'Z' => String::new(),
            '%' => "%".to_string(),
            other => format!("%{other}"),
        };
        out.push_str(&piece);
    }
    out
}

/// The proleptic Gregorian `(year, month, day)` of a day count from
/// 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The day count from 1970-01-01 of a proleptic Gregorian date (Howard
/// Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
