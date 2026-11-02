//! A streaming log: lines arrive at the bottom, older ones scroll up.
//!
//! Re-rendering a whole log for every new line is what made the spike's
//! log scenario slower than ratatui. A [`Log`] keeps its lines in a bounded
//! buffer (its scrollback); its node renders each line once, and on an
//! append it moves the rows already on screen up inside its own rectangle
//! and renders only the new lines into the rows that opened at the bottom.
//!
//! ```
//! use intuituive::prelude::*;
//!
//! let app = App::new(|| {
//!     let log = Log::new(1_000);
//!     log.push("[green]started[/]");
//!     log.view().panel("Log").on_key("q", |cx| cx.quit())
//! });
//! ```
//!
//! A line may carry a level and a target ([`LogLine`]): the view shows the
//! level in its theme style and the target dimmed before the message. The
//! log then filters by level ([`Log::set_level`]), by target
//! ([`Log::set_target_level`], with a panel to set them,
//! [`Log::targets_view`]) and by a [query] typed in a box
//! ([`Log::set_filter`]: words, `level:warn`, `target:db`).
//!
//! The view follows the end until it is scrolled back (Up, PageUp, Home,
//! the wheel, the scrollbar); then new lines arrive below it without moving
//! it, and End (or scrolling down to the last line) follows again. Make it
//! [`focusable`](crate::Node::focusable) to take keys while it has the
//! focus; the wheel works over it either way.
//!
//! ```
//! use intuituive::log::{LogLine, Severity};
//! use intuituive::prelude::*;
//!
//! let app = App::new(|| {
//!     let log = Log::new(1_000);
//!     log.push_line(LogLine::new("cache warmed").level(Severity::Info).target("app::cache"));
//!     log.push_line(LogLine::new("disk [b]full[/]").level(Severity::Error).target("app::store"));
//!     log.set_level(Some(Severity::Warn));
//!     log.view().focusable().on_key("q", |cx| cx.quit())
//! });
//! let screen = app.render_with(&["q"], 30, 2).unwrap();
//! assert_eq!(screen[0].trim_end(), "ERROR app::store disk full");
//! ```
//!
//! With the off-by-default `tracing` feature, `tracing_layer` feeds a
//! log from `tracing`, so an app shows its own `tracing::info!` lines.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use rich::{Renderable, Segment, Style};
use rich_interact::{Key, KeyCode};

use crate::node::{Axis, Node};
use crate::query::{self, Grammar, Kind, Query, QueryError, TextMatch};
use crate::reactive::{Signal, signal};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, MouseExt, Used, Widget, WidgetEvent};

#[cfg(feature = "tracing")]
mod from_tracing;
mod targets;

#[cfg(feature = "tracing")]
pub use from_tracing::{TracingFeed, install_tracing, tracing_layer};
/// How serious a line is, from rs-rich-ext's events: `Trace` up to
/// `Fatal`, in that order.
pub use rich_ext::event::Severity;

/// How many lines a turn of the wheel scrolls.
const WHEEL_STEP: isize = 3;

/// One line for a [`Log`]: console markup, and optionally its level and
/// its target (the module or subsystem that wrote it, as `tracing` and
/// `log` name it).
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct LogLine {
    markup: String,
    level: Option<Severity>,
    target: Option<String>,
}

impl LogLine {
    /// A line of console markup.
    pub fn new(markup: impl Into<String>) -> LogLine {
        LogLine {
            markup: markup.into(),
            level: None,
            target: None,
        }
    }

    /// A line of plain text: square brackets show as written.
    pub fn text(text: &str) -> LogLine {
        LogLine::new(rich::markup::escape(text))
    }

    pub fn level(mut self, level: Severity) -> LogLine {
        self.level = Some(level);
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> LogLine {
        self.target = Some(target.into());
        self
    }
}

impl From<&str> for LogLine {
    fn from(markup: &str) -> LogLine {
        LogLine::new(markup)
    }
}

impl From<String> for LogLine {
    fn from(markup: String) -> LogLine {
        LogLine::new(markup)
    }
}

/// Which of a target's lines a [`Log`] shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetLevel {
    /// As the log's own level ([`Log::set_level`]) says.
    Inherit,
    /// Lines at this level or above (and lines with no level).
    From(Severity),
    /// None of them.
    Off,
}

/// A kept line.
pub(crate) struct Line {
    pub markup: String,
    pub level: Option<Severity>,
    pub target: Option<String>,
    /// The plain text, for the query's bare words and for screen readers,
    /// read from the markup the first time either asks.
    pub plain: std::cell::OnceCell<String>,
    /// Whether it passes the filters.
    pub pass: bool,
    /// A renderable written with [`Log::write`], drawn below the markup
    /// (and the level and target), and its rows at the width it was last
    /// drawn at.
    pub body: Option<Rc<dyn Renderable>>,
    pub rendered: RefCell<Option<(u16, Vec<Vec<Segment>>)>>,
}

/// How wide a renderable entry is laid out for its plain text: what the
/// filter's words and a screen reader read.
const PLAIN_WIDTH: usize = 100;

impl Line {
    fn plain(&self) -> &str {
        self.plain.get_or_init(|| {
            let mut plain = match rich::Text::from_markup(&self.markup) {
                Ok(text) => text.plain().to_string(),
                Err(_) => self.markup.clone(),
            };
            if let Some(body) = &self.body {
                let console = rich::Console::builder()
                    .width(PLAIN_WIDTH)
                    .no_color(true)
                    .build();
                let text = console.render_to_string(&**body);
                if !plain.is_empty() {
                    plain.push('\n');
                }
                plain.push_str(text.trim_end());
            }
            plain
        })
    }
}

impl query::Record for Line {
    fn field(&self, name: &str) -> Option<query::Value> {
        match name {
            "level" => self.level.map(|level| query::Value::from(level.as_str())),
            "target" => self.target.as_deref().map(query::Value::from),
            _ => None,
        }
    }

    fn text(&self) -> String {
        self.plain().to_string()
    }
}

/// What a log shows of its lines.
#[derive(Default)]
pub(crate) struct Filters {
    pub level: Option<Severity>,
    /// Per target (a target and the ones under it, `a` covering `a::b`).
    pub targets: Vec<(String, TargetLevel)>,
    pub text: String,
    pub query: Option<Query>,
}

impl Filters {
    fn any(&self) -> bool {
        self.level.is_some() || !self.targets.is_empty() || self.query.is_some()
    }

    /// The setting for `target`: the longest configured target it is, or
    /// is under.
    pub fn target_level(&self, target: Option<&str>) -> TargetLevel {
        let Some(target) = target else {
            return TargetLevel::Inherit;
        };
        self.targets
            .iter()
            .filter(|(name, _)| {
                target == name
                    || (target.starts_with(name.as_str()) && target[name.len()..].starts_with("::"))
            })
            .max_by_key(|(name, _)| name.len())
            .map_or(TargetLevel::Inherit, |(_, level)| *level)
    }

    fn passes(&self, line: &Line) -> bool {
        let least = match self.target_level(line.target.as_deref()) {
            TargetLevel::Off => return false,
            TargetLevel::From(level) => Some(level),
            TargetLevel::Inherit => self.level,
        };
        let level_ok = match (least, line.level) {
            (Some(least), Some(level)) => level >= least,
            _ => true,
        };
        level_ok && self.query.as_ref().is_none_or(|query| query.matches(line))
    }
}

pub(crate) struct LogData {
    pub lines: VecDeque<Line>,
    pub capacity: usize,
    /// Lines ever pushed, so a view knows how many arrived since it drew.
    pub total: u64,
    /// The kept lines that pass the filters.
    pub passing: usize,
    pub filters: Filters,
    /// While scrolled back: the number (counting every line ever pushed)
    /// of the newest line in view. `None` while it follows the end.
    pub bottom: Option<u64>,
    /// The targets seen, in the order they first came, with their lines
    /// (ever pushed).
    pub targets: Vec<(String, u64)>,
    /// Bumped by whatever changes which lines show but the end moving (a
    /// filter, a scroll): a view draws everything again.
    pub moved: u64,
    pub show_level: bool,
    pub show_target: bool,
    /// How many kept lines hold a renderable, which may take more than a
    /// row: while any do, a view draws its rows in full.
    pub tall: usize,
}

impl LogData {
    /// The number of the line at `index`.
    fn number(&self, index: usize) -> u64 {
        self.total - self.lines.len() as u64 + index as u64
    }

    fn passing_in(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl DoubleEndedIterator<Item = usize> + '_ {
        range.filter(|&i| self.lines[i].pass)
    }

    /// The kept lines in a view `height` tall, by index, oldest first.
    pub fn window(&self, height: usize) -> Vec<usize> {
        let len = self.lines.len();
        let end = match self.bottom {
            None => len,
            Some(number) => {
                let first = self.total - len as u64;
                if number < first {
                    0
                } else {
                    ((number - first) as usize + 1).min(len)
                }
            }
        };
        let mut shown: Vec<usize> = self.passing_in(0..end).rev().take(height).collect();
        shown.reverse();
        // Scrolled back past what is kept: the view fills from the top.
        if shown.len() < height {
            let more = height - shown.len();
            shown.extend(self.passing_in(end..len).take(more));
        }
        shown
    }

    /// Where the view's newest line goes for the index `newest`: follow
    /// when nothing that passes comes after it.
    fn bottom_at(&self, newest: usize) -> Option<u64> {
        let len = self.lines.len();
        self.passing_in(newest + 1..len)
            .next()
            .map(|_| self.number(newest))
    }

    /// The view's newest line after scrolling `by` lines (back when
    /// negative) in a view `height` tall, if that moves it.
    fn scrolled(&self, by: isize, height: usize) -> Option<Option<u64>> {
        let shown = self.window(height);
        let (&top, &bottom) = (shown.first()?, shown.last()?);
        let len = self.lines.len();
        let to = if by < 0 {
            // Back, but never so far the view has room left at the top.
            let steps = self.passing_in(0..top).count().min(by.unsigned_abs());
            if steps == 0 {
                return None;
            }
            let newest = self.passing_in(0..bottom + 1).rev().nth(steps)?;
            Some(self.number(newest))
        } else {
            match self
                .passing_in(bottom + 1..len)
                .nth(by.unsigned_abs().saturating_sub(1))
            {
                Some(i) => self.bottom_at(i),
                // Down past the last line: follow it again.
                None => None,
            }
        };
        (to != self.bottom).then_some(to)
    }

    /// The view's newest line with the passing line ranked `first` at its
    /// top.
    fn at_rank(&self, first: usize, height: usize) -> Option<u64> {
        let len = self.lines.len();
        match self.passing_in(0..len).nth(first + height.max(1) - 1) {
            Some(i) => self.bottom_at(i),
            None => None,
        }
    }

    /// The view's newest line after moving `by` lines from the line at
    /// index `newest` (back when negative), counting lines, not rows: how
    /// a view of tall lines scrolls.
    fn scrolled_from(&self, newest: usize, by: isize) -> Option<Option<u64>> {
        let len = self.lines.len();
        let to = if by < 0 {
            let back = self
                .passing_in(0..newest.min(len))
                .rev()
                .take(by.unsigned_abs())
                .last()?;
            Some(self.number(back))
        } else if by > 0 {
            match self
                .passing_in((newest + 1).min(len)..len)
                .nth(by.unsigned_abs() - 1)
            {
                Some(i) => self.bottom_at(i),
                None => None,
            }
        } else {
            return None;
        };
        (to != self.bottom).then_some(to)
    }

    /// Lines that pass and came after the newest in view.
    fn unseen(&self) -> usize {
        let Some(number) = self.bottom else {
            return 0;
        };
        let first = self.total - self.lines.len() as u64;
        let from = ((number + 1).saturating_sub(first) as usize).min(self.lines.len());
        self.lines.range(from..).filter(|line| line.pass).count()
    }

    /// Test every line again.
    fn refilter(&mut self) {
        let mut passing = 0;
        let any = self.filters.any();
        for i in 0..self.lines.len() {
            let pass = !any || self.filters.passes(&self.lines[i]);
            self.lines[i].pass = pass;
            passing += usize::from(pass);
        }
        self.passing = passing;
        self.moved += 1;
    }

    fn drop_oldest(&mut self) {
        if let Some(old) = self.lines.pop_front() {
            self.passing -= usize::from(old.pass);
            self.tall -= usize::from(old.body.is_some());
        }
    }

    /// The kept lines a view `height` rows tall shows, oldest first, when
    /// line `i` takes `rows(i)` rows; and how many rows of the first are
    /// above the view (the end in view, a tall line cut at the top).
    fn window_rows(&self, height: usize, rows: impl Fn(usize) -> usize) -> (Vec<usize>, usize) {
        let len = self.lines.len();
        let end = match self.bottom {
            None => len,
            Some(number) => {
                let first = self.total - len as u64;
                if number < first {
                    0
                } else {
                    ((number - first) as usize + 1).min(len)
                }
            }
        };
        let mut shown = Vec::new();
        let mut filled = 0;
        for i in self.passing_in(0..end).rev() {
            if filled >= height {
                break;
            }
            filled += rows(i);
            shown.push(i);
        }
        shown.reverse();
        // Scrolled back past what is kept: the view fills from the top,
        // and the rows over are at the bottom.
        let mut from_top = false;
        for i in self.passing_in(end..len) {
            if filled >= height {
                break;
            }
            filled += rows(i);
            shown.push(i);
            from_top = true;
        }
        let over = filled.saturating_sub(height);
        (shown, if from_top { 0 } else { over })
    }
}

/// The grammar of [`Log::set_filter`]: `level` and `target` fields (a
/// target matching from its start, so `target:app` finds `app::db`), and
/// words in the message; an unknown `name:` is a word.
pub fn filter_grammar() -> Grammar {
    Grammar::new()
        .field("level", Kind::Text)
        .field("target", Kind::Text)
        .matching(TextMatch::Prefix)
        .lenient()
}

/// A bounded log of console-markup lines. A `Copy` handle, like a signal:
/// move it into as many closures as you like.
#[derive(Clone, Copy)]
pub struct Log {
    pub(crate) data: Signal<LogData>,
}

impl Log {
    /// A log keeping the last `capacity` lines (its scrollback). Call it
    /// while the app is being built (it makes a signal).
    pub fn new(capacity: usize) -> Log {
        Log {
            data: signal(LogData {
                lines: VecDeque::new(),
                capacity: capacity.max(1),
                total: 0,
                passing: 0,
                filters: Filters::default(),
                bottom: None,
                targets: Vec::new(),
                moved: 0,
                show_level: true,
                show_target: true,
                tall: 0,
            }),
        }
    }

    /// Add a line (console markup) at the bottom.
    pub fn push(&self, line: impl Into<String>) {
        self.push_line(LogLine::new(line));
    }

    /// Add a line, with its level and target, at the bottom.
    pub fn push_line(&self, line: impl Into<LogLine>) {
        let LogLine {
            markup,
            level,
            target,
        } = line.into();
        self.add(Line {
            markup,
            level,
            target,
            plain: std::cell::OnceCell::new(),
            pass: true,
            body: None,
            rendered: RefCell::new(None),
        });
    }

    /// Add any rich renderable at the bottom: a `Table`, highlighted code
    /// (`Syntax`), a `Panel`, a diff. It is laid out at the view's width
    /// when it is drawn, and again when the width changes, only while it
    /// is in view; the capacity counts it as one line, and the filter
    /// matches the words of its plain text.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let log = Log::new(100);
    ///     log.push("[b]ran[/] ls");
    ///     let mut table = rich::Table::new();
    ///     table.add_column("name");
    ///     table.add_row(&["Cargo.toml"]);
    ///     log.write(table);
    ///     log.view()
    /// });
    /// let mut driver = app.driver(20, 6);
    /// driver.settle();
    /// let rows = driver.screen().plain();
    /// assert_eq!(rows[0].trim_end(), "ran ls");
    /// assert!(rows[1].starts_with("┏━"), "{rows:?}");
    /// assert!(rows[4].contains("Cargo.toml"), "{rows:?}");
    /// ```
    pub fn write(&self, renderable: impl Renderable + 'static) {
        self.write_with(renderable, LogLine::new(""));
    }

    /// [`write`](Self::write), with a level and a target (and markup shown
    /// above it, when the line has any): `LogLine::new("").level(..)`.
    pub fn write_with(&self, renderable: impl Renderable + 'static, line: impl Into<LogLine>) {
        let LogLine {
            markup,
            level,
            target,
        } = line.into();
        self.add(Line {
            markup,
            level,
            target,
            plain: std::cell::OnceCell::new(),
            pass: true,
            body: Some(Rc::new(renderable)),
            rendered: RefCell::new(None),
        });
    }

    fn add(&self, mut line: Line) {
        self.data.update(|data| {
            if let Some(target) = &line.target {
                match data.targets.iter_mut().find(|(name, _)| name == target) {
                    Some((_, count)) => *count += 1,
                    None => data.targets.push((target.clone(), 1)),
                }
            }
            line.pass = !data.filters.any() || data.filters.passes(&line);
            if data.lines.len() == data.capacity {
                data.drop_oldest();
            }
            data.passing += usize::from(line.pass);
            data.tall += usize::from(line.body.is_some());
            data.lines.push_back(line);
            data.total += 1;
        });
    }

    /// Remove every line.
    pub fn clear(&self) {
        self.data.update(|data| {
            data.lines.clear();
            data.passing = 0;
            data.tall = 0;
            data.bottom = None;
            // A total that jumps past the view's makes it draw everything.
            data.total += u32::MAX as u64;
        });
    }

    /// The lines kept.
    pub fn len(&self) -> usize {
        self.data.with_untracked(|data| data.lines.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Keep the last `capacity` lines from now on, dropping the oldest
    /// past it.
    pub fn set_capacity(&self, capacity: usize) {
        self.data.update(|data| {
            data.capacity = capacity.max(1);
            while data.lines.len() > data.capacity {
                data.drop_oldest();
            }
            data.moved += 1;
        });
    }

    /// The lines kept that pass the filters.
    pub fn shown(&self) -> usize {
        self.data.with(|data| data.passing)
    }

    /// Show lines at `level` and above (lines with no level always show);
    /// `None` shows every level.
    pub fn set_level(&self, level: Option<Severity>) {
        self.data.update(|data| {
            data.filters.level = level;
            data.refilter();
        });
    }

    /// The level set by [`set_level`](Self::set_level).
    pub fn level(&self) -> Option<Severity> {
        self.data.with(|data| data.filters.level)
    }

    /// What to show of `target`'s lines, and of the targets under it
    /// (`app` covers `app::db`); the most specific setting wins.
    pub fn set_target_level(&self, target: &str, level: TargetLevel) {
        self.data.update(|data| {
            let targets = &mut data.filters.targets;
            targets.retain(|(name, _)| name != target);
            if level != TargetLevel::Inherit {
                targets.push((target.to_string(), level));
            }
            data.refilter();
        });
    }

    /// The setting that applies to `target`.
    pub fn target_level(&self, target: &str) -> TargetLevel {
        self.data
            .with(|data| data.filters.target_level(Some(target)))
    }

    /// The targets seen, in the order they first came, with how many lines
    /// each wrote.
    pub fn targets(&self) -> Vec<(String, u64)> {
        self.data.with(|data| data.targets.clone())
    }

    /// Show only the lines `text` matches, as a [query] in
    /// [`filter_grammar`]: words in the message (ignoring case),
    /// `level:error`, `target:app::db`, `and`, `or`, `not`. An empty text
    /// shows every line. On an error the filter stays as it was.
    pub fn set_filter(&self, text: &str) -> Result<(), QueryError> {
        let query = if text.trim().is_empty() {
            None
        } else {
            Some(query::parse(text, &filter_grammar())?)
        };
        self.data.update(|data| {
            data.filters.text = text.to_string();
            data.filters.query = query;
            data.refilter();
        });
        Ok(())
    }

    /// The text of the filter set by [`set_filter`](Self::set_filter).
    pub fn filter(&self) -> String {
        self.data.with(|data| data.filters.text.clone())
    }

    /// Whether the view sticks to the newest line.
    pub fn is_following(&self) -> bool {
        self.data.with(|data| data.bottom.is_none())
    }

    /// Stick to the newest line again (what End does).
    pub fn follow(&self) {
        if self.data.with_untracked(|data| data.bottom.is_some()) {
            self.data.update(|data| {
                data.bottom = None;
                data.moved += 1;
            });
        }
    }

    /// Lines that arrived (and pass the filters) below the view while it
    /// is scrolled back: what a status line shows as "12 new".
    pub fn unseen(&self) -> usize {
        self.data.with(LogData::unseen)
    }

    /// Show each line's level (on by default).
    pub fn show_level(&self, on: bool) {
        self.data.update(|data| {
            data.show_level = on;
            data.moved += 1;
        });
    }

    /// Show each line's target (on by default).
    pub fn show_target(&self, on: bool) {
        self.data.update(|data| {
            data.show_target = on;
            data.moved += 1;
        });
    }

    /// A node showing the latest lines, one per row.
    pub fn view(&self) -> Node {
        Node::from_widget(
            Box::new(LogView {
                log: *self,
                drawn_total: None,
                drawn_moved: 0,
                fast: false,
                shown: 0,
                height: 0,
                bar: crate::scrollbar::Bar::default(),
                barred: false,
                bar_at: None,
                said: None,
                newest: None,
            }),
            "log",
        )
    }

    /// A panel of the targets seen, each with what the log shows of it,
    /// like tui-logger's: Up and Down choose one, Left shows less of it
    /// and Right more (off, error, warn, info, debug, trace), Space turns
    /// it off or back on, and Backspace gives it back to the log's level.
    /// A click chooses, the wheel moves. Focusable.
    pub fn targets_view(&self) -> Node {
        targets::view(*self)
    }

    /// Move the view's newest line to `bottom`, if that is a change.
    fn move_to(&self, bottom: Option<Option<u64>>) -> Used {
        match bottom {
            Some(bottom) => {
                self.data.update(|data| {
                    data.bottom = bottom;
                    data.moved += 1;
                });
                Used::Yes
            }
            None => Used::No,
        }
    }
}

pub(crate) struct LogView {
    pub log: Log,
    /// The log's total when the view last drew.
    pub drawn_total: Option<u64>,
    /// The log's `moved` when the view last drew.
    pub drawn_moved: u64,
    /// Whether the last draw showed the last lines, every one passing, so
    /// the next may only add what arrived.
    pub fast: bool,
    /// How many lines it showed then (at most its height).
    pub shown: usize,
    pub height: usize,
    /// Its scrollbar, which shows where the lines in view are in what the
    /// log keeps; and whether it kept a column for it at the last draw.
    pub bar: crate::scrollbar::Bar,
    pub barred: bool,
    /// Where the bar was drawn, and the place and length it showed.
    pub bar_at: Option<(Rect, usize, usize)>,
    /// The number of the newest line in view at the last draw: what a
    /// screen reader reads.
    pub said: Option<u64>,
    /// With tall lines: the index of the newest line in view at the last
    /// draw, which scrolling moves from.
    pub newest: Option<usize>,
}

/// A level as the view shows it, the theme name of its style, and the
/// style without one (rich's `logging.level.*`, where it has them).
pub(crate) fn level_look(level: Severity) -> (&'static str, &'static str, &'static str) {
    match level {
        Severity::Trace => ("TRACE", "logging.level.trace", "dim"),
        Severity::Debug => ("DEBUG", "logging.level.debug", "green"),
        Severity::Info => ("INFO ", "logging.level.info", "blue"),
        Severity::Warn => ("WARN ", "logging.level.warning", "yellow"),
        Severity::Error => ("ERROR", "logging.level.error", "bold red"),
        Severity::Fatal => ("FATAL", "logging.level.critical", "bold reverse red"),
    }
}

/// The styles a view draws a line's level and target in.
struct Looks {
    levels: [Style; 6],
    target: Style,
    show_level: bool,
    show_target: bool,
}

impl Looks {
    /// Unstyled looks, for counting rows.
    fn plain(show_level: bool, show_target: bool) -> Looks {
        Looks {
            levels: Default::default(),
            target: Style::new(),
            show_level,
            show_target,
        }
    }

    fn new(cx: &DrawCx, show_level: bool, show_target: bool) -> Looks {
        let style = |level| {
            let (_, name, fallback) = level_look(level);
            cx.style(name, fallback)
        };
        Looks {
            levels: [
                style(Severity::Trace),
                style(Severity::Debug),
                style(Severity::Info),
                style(Severity::Warn),
                style(Severity::Error),
                style(Severity::Fatal),
            ],
            target: cx.style("log.target", "dim"),
            show_level,
            show_target,
        }
    }
}

impl Widget for LogView {
    fn name(&self) -> &'static str {
        "log"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Log
    }

    fn describe(&self) -> Option<String> {
        Some(
            self.log
                .data
                .with_untracked(|data| format!("{} of {} lines", data.passing, data.lines.len())),
        )
    }

    /// The newest line in view, as plain text.
    fn access_text(&self) -> Option<String> {
        let number = self.said?;
        self.log.data.with_untracked(|data| {
            let first = data.total - data.lines.len() as u64;
            let line = data.lines.get(number.checked_sub(first)? as usize)?;
            Some(line.plain().to_string())
        })
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => self
                .log
                .data
                .with(|data| {
                    if data.tall == 0 {
                        return data.passing;
                    }
                    let looks = Looks::plain(data.show_level, data.show_target);
                    data.passing_in(0..data.lines.len())
                        .map(|i| entry_rows(cx.console(), &looks, &data.lines[i], width).len())
                        .sum()
                })
                .min(u16::MAX as usize) as u16,
            Axis::Horizontal => width,
        }
    }

    /// Only what arrived since the last frame draws, moving what is on
    /// screen up, unless the log moved or was drawn over, or hides lines,
    /// or is scrolled back.
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (total, moved) = self.log.data.with(|data| (data.total, data.moved));
        let (mut width, height) = (canvas.width(), canvas.height() as usize);
        self.height = height;
        let (looks, passing, fast) = self.log.data.with_untracked(|data| {
            (
                Looks::new(cx, data.show_level, data.show_target),
                data.passing,
                data.bottom.is_none() && data.passing == data.lines.len() && data.tall == 0,
            )
        });
        // The bar keeps a column once the lines do not all fit; taking it or
        // giving it back draws everything again.
        let look = (self.bar.look.clone()).filter(|look| look.wanted(passing, height));
        let mut x = 0;
        let mut bar = None;
        if let Some(look) = &look {
            let (rest, at) = look.split(width, canvas.height());
            (x, width) = (rest.x, rest.width);
            bar = Some(at);
        }
        let full =
            cx.repaint() || self.barred != bar.is_some() || !self.fast || moved != self.drawn_moved;
        self.barred = bar.is_some();
        let arrived = self
            .drawn_total
            .map(|drawn| total.saturating_sub(drawn) as usize);
        let console = cx.console();
        let shown = self.shown;
        let mut first = 0;
        let mut said = None;
        let mut newest = None;
        self.log.data.with_untracked(|data| {
            let render = |indices: &mut dyn Iterator<Item = usize>| -> Vec<Vec<Segment>> {
                indices
                    .map(|i| render_line(console, &looks, &data.lines[i], width))
                    .collect()
            };
            let len = data.lines.len();
            if data.tall > 0 {
                // Lines that may be taller than a row: the rows of those
                // in view, laid out at this width.
                let (window, cut) = data.window_rows(height, |i| {
                    entry_rows(console, &looks, &data.lines[i], width).len()
                });
                if let (Some(_), Some(&top)) = (bar, window.first()) {
                    first = data.passing_in(0..top).count();
                }
                if let Some(&last) = window.last() {
                    said = Some(data.number(last));
                }
                newest = window.last().copied();
                let mut lines: Vec<Vec<Segment>> = window
                    .iter()
                    .flat_map(|&i| entry_rows(console, &looks, &data.lines[i], width))
                    .collect();
                lines.drain(..cut.min(lines.len()));
                lines.truncate(height);
                canvas.lines_at(x, 0, width, height as u16, &lines);
                return;
            }
            if !fast {
                // Scrolled back or filtered: the lines in view, all of them.
                let window = data.window(height);
                if let (Some(_), Some(&top)) = (bar, window.first()) {
                    first = data.passing_in(0..top).count();
                }
                if let Some(&last) = window.last() {
                    said = Some(data.number(last));
                }
                let lines = render(&mut window.into_iter());
                canvas.lines_at(x, 0, width, height as u16, &lines);
                return;
            }
            first = len.saturating_sub(height);
            if !data.lines.is_empty() {
                said = Some(data.number(len - 1));
            }
            match arrived {
                Some(0) if !full => {}
                Some(n) if !full && n < height => {
                    if len == shown + n && shown + n <= height {
                        // Room left and nothing dropped: the new lines go
                        // under the old.
                        let lines = render(&mut (shown..len));
                        canvas.lines_at(x, shown as u16, width, n as u16, &lines);
                    } else if shown == height && len >= height {
                        // Full: scroll what is on screen up, and render only
                        // the new lines into the rows that opened.
                        canvas.scroll_up(n as u16);
                        let top = (height - n) as u16;
                        let lines = render(&mut (len - n..len));
                        canvas.lines_at(x, top, width, n as u16, &lines);
                    } else {
                        // Lines were dropped from a log shorter than the
                        // view: draw what it keeps.
                        let lines = render(&mut (len.saturating_sub(height)..len));
                        canvas.lines_at(x, 0, width, height as u16, &lines);
                    }
                }
                _ if bar.is_none() => canvas.lines(&render(&mut (len.saturating_sub(height)..len))),
                _ => {
                    let lines = render(&mut (len.saturating_sub(height)..len));
                    canvas.lines_at(x, 0, width, height as u16, &lines);
                }
            }
        });
        // The bar, where the lines in view are among those that pass.
        self.bar_at = None;
        if let (Some(look), Some(at)) = (look, bar) {
            look.paint(console, canvas, at, first, passing, height);
            self.bar_at = Some((at, first, passing));
        }
        self.fast = fast;
        self.newest = newest;
        self.drawn_total = Some(total);
        self.drawn_moved = moved;
        self.said = said;
        self.shown = self
            .log
            .data
            .with_untracked(|data| data.lines.len())
            .min(height);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let height = self.height.max(1);
        match event {
            WidgetEvent::Mouse(mouse) => {
                if let Some((rect, first, len)) = self.bar_at {
                    match self.bar.mouse(cx, mouse, rect, first, len, height) {
                        Some(Some(to)) => {
                            let bottom = self.log.data.with_untracked(|data| {
                                let bottom = data.at_rank(to, height);
                                (bottom != data.bottom).then_some(bottom)
                            });
                            self.log.move_to(bottom);
                            return Used::Yes;
                        }
                        Some(None) => return Used::Yes,
                        None => {}
                    }
                }
                match mouse.wheel() {
                    0 => Used::No,
                    way => self.scroll_by(way as isize * WHEEL_STEP, height),
                }
            }
            WidgetEvent::Key(key) => self.key(key, height),
            _ => Used::No,
        }
    }

    fn retained(&self) -> bool {
        true
    }
}

impl LogView {
    fn scroll_by(&self, by: isize, height: usize) -> Used {
        let to = self
            .log
            .data
            .with_untracked(|data| match (data.tall, self.newest) {
                (0, _) | (_, None) => data.scrolled(by, height),
                (_, Some(newest)) => data.scrolled_from(newest, by),
            });
        self.log.move_to(to)
    }

    fn key(&self, key: &Key, height: usize) -> Used {
        if key.modifiers.ctrl || key.modifiers.alt {
            return Used::No;
        }
        let page = height.saturating_sub(1).max(1) as isize;
        let letter = |c: char| key.code == KeyCode::Char(c);
        match key.code {
            KeyCode::Up => self.scroll_by(-1, height),
            KeyCode::Down => self.scroll_by(1, height),
            _ if letter('k') => self.scroll_by(-1, height),
            _ if letter('j') => self.scroll_by(1, height),
            KeyCode::PageUp => self.scroll_by(-page, height),
            KeyCode::PageDown => self.scroll_by(page, height),
            KeyCode::Home => self.top(height),
            _ if letter('g') => self.top(height),
            KeyCode::End => self.end(),
            _ if letter('G') => self.end(),
            _ => Used::No,
        }
    }

    fn top(&self, height: usize) -> Used {
        let to = self.log.data.with_untracked(|data| {
            // Tall lines: the first line at the bottom, which fills the
            // view from the top.
            let height = if data.tall > 0 { 1 } else { height };
            let bottom = data.at_rank(0, height);
            (bottom != data.bottom).then_some(bottom)
        });
        self.log.move_to(to)
    }

    fn end(&self) -> Used {
        let to = self
            .log
            .data
            .with_untracked(|data| data.bottom.is_some().then_some(None));
        self.log.move_to(to)
    }
}

/// The rows of a kept line at `width`: one for a line of markup; for a
/// written renderable, a row of its level, target and markup when it has
/// any, then its own rows, laid out at `width` once and kept until the
/// width changes.
fn entry_rows(
    console: &rich::Console,
    looks: &Looks,
    line: &Line,
    width: u16,
) -> Vec<Vec<Segment>> {
    let Some(body) = &line.body else {
        return vec![render_line(console, looks, line, width)];
    };
    let mut rows = Vec::new();
    if !line.markup.is_empty()
        || (looks.show_level && line.level.is_some())
        || (looks.show_target && line.target.is_some())
    {
        rows.push(render_line(console, looks, line, width));
    }
    let mut kept = line.rendered.borrow_mut();
    if kept.as_ref().map(|(w, _)| *w) != Some(width) {
        let options = console.options().update_width(width.max(1) as usize);
        *kept = Some((width, console.render_lines(&**body, &options, false)));
    }
    rows.extend(kept.as_ref().map(|(_, r)| r.clone()).unwrap_or_default());
    rows
}

/// One log line rendered to one row at `width` (cropped, never wrapped):
/// its level and target, when it has them and the log shows them, then
/// its markup.
fn render_line(console: &rich::Console, looks: &Looks, line: &Line, width: u16) -> Vec<Segment> {
    let message =
        rich::Text::from_markup(&line.markup).unwrap_or_else(|_| rich::Text::new(&line.markup));
    let text = if (looks.show_level && line.level.is_some())
        || (looks.show_target && line.target.is_some())
    {
        let mut text = rich::Text::new("");
        if let (true, Some(level)) = (looks.show_level, line.level) {
            let (label, _, _) = level_look(level);
            text.append(label, Some(looks.levels[level as usize].clone().into()));
            text.append(" ", None);
        }
        if let (true, Some(target)) = (looks.show_target, &line.target) {
            text.append(target, Some(looks.target.clone().into()));
            text.append(" ", None);
        }
        text.append_text(&message)
    } else {
        message
    };
    let text = text.no_wrap(true).overflow(rich::Overflow::Crop);
    let options = console.options().update_width(width.max(1) as usize);
    console
        .render_lines(&text, &options, false)
        .into_iter()
        .next()
        .unwrap_or_default()
}
