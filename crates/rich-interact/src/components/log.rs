//! A live log view (#724): records in a list that follows the end as they
//! arrive, a filter line, a pause, and each record's fields on Enter.
//!
//! Records are [`rich_ext::log_stream::Record`]s, given up front
//! ([`LogView::records`]), pushed ([`LogView::push`]) or sent from another
//! thread as they are read ([`LogView::source`]). Each is one row: its
//! time, level, message and fields, cut to the width. A run of identical
//! records (the time aside) is one row with its count, `×12`. Enter shows
//! the cursor's record's fields and the lines that followed it (a stack
//! trace) under its row.
//!
//! `/` types a filter, applied as it is typed: words that parse as a
//! [`Predicate`] (`service=api`, `latency_ms>500`, `msg~timeout`) test a
//! field, and any other word must appear in the row, in any case. Every
//! word must match. Enter keeps the filter and Escape clears it.
//!
//! The view follows the end until the cursor moves up; End (or Down on the
//! last row) follows again. Space pauses it, counting what arrives, and
//! Space again jumps to the end. `q` or Escape closes it.
//!
//! ```
//! use rich_interact::components::LogView;
//! use rich_interact::{headless, Outcome};
//! use rich_ext::event::Value;
//! use rich_ext::log_stream::Record;
//!
//! let record = |level: &str, message: &str| {
//!     Record::new("json")
//!         .field("level", Value::String(level.into()))
//!         .field("msg", Value::String(message.into()))
//! };
//! let view = LogView::new().records([record("info", "started"), record("error", "failed")]);
//! let (outcome, frames) = headless::run(view, headless::Script::new().keys("q"), 60, 6);
//! assert_eq!(outcome.unwrap(), Outcome::Done(()));
//! assert!(frames.last_frame().contains("ERROR failed"));
//! ```

use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use rich::cells::cell_len;
use rich::{Segment, Style};
use rich_ext::event::{Severity, Value};
use rich_ext::log_stream::{
    format_offset, Assembler, Predicate, Record, TimeMode, LEVEL_KEYS, MESSAGE_KEYS, TIME_KEYS,
};

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, text, Theme};
use crate::event::{Event, KeyCode};
use crate::keymap::{keys, Keymap};
use crate::kit::TextBuffer;
use crate::policy::{LineIo, NotInteractive};

/// What a [`LogView`]'s source sends.
#[derive(Clone, Debug, PartialEq)]
pub enum LogEvent {
    /// A record.
    Record(Record),
    /// A line that belongs under the last record (see
    /// [`Assembled::Continuation`](rich_ext::log_stream::Assembled)).
    Continuation(String),
}

/// A record as an event, so rs-rich-ext's
/// [`ChannelSink`](rich_ext::log_stream::ChannelSink) can feed a
/// [`LogView::source`] straight from `tracing` or `log`:
///
/// ```
/// use rich_interact::components::{LogEvent, LogView};
/// use rich_ext::log_stream::{ChannelSink, Record};
///
/// let (sink, records) = ChannelSink::<LogEvent>::bounded(1_000);
/// sink.send(Record::text("started"));
/// let view = LogView::new().source(records);
/// // `EventLayer::new(Arc::new(sink))` would send `tracing`'s events.
/// # drop(view);
/// ```
impl From<Record> for LogEvent {
    fn from(record: Record) -> LogEvent {
        LogEvent::Record(record)
    }
}

/// The keys of a [`LogView`], in context `log-view`.
pub fn log_view_keymap() -> Keymap {
    Keymap::new("log-view")
        .bind("quit", keys("q escape"), "quit")
        .bind("filter", keys("/"), "filter")
        .bind("pause", keys("space"), "pause or follow again")
        .bind("expand", keys("enter"), "show the record's fields")
        .bind("up", keys("up k"), "previous record")
        .bind("down", keys("down j"), "next record")
        .bind("page-up", keys("pageup"), "page up")
        .bind("page-down", keys("pagedown"), "page down")
        .bind("top", keys("home g"), "first record")
        .bind("bottom", keys("end G"), "last record, and follow")
}

/// The keys of a [`LogView`] while its filter is typed, in context
/// `log-filter`.
pub fn log_filter_keymap() -> Keymap {
    Keymap::new("log-filter")
        .bind("apply", keys("enter"), "keep the filter")
        .bind("clear", keys("escape"), "clear the filter")
        .bind("left", keys("left"), "move left")
        .bind("right", keys("right"), "move right")
        .bind("home", keys("home ctrl+a"), "go to the start")
        .bind("end", keys("end ctrl+e"), "go to the end")
        .bind("delete-to-start", keys("ctrl+u"), "delete to the start")
        .bind("delete-word", keys("ctrl+w"), "delete a word")
        .bind("backspace", keys("backspace"), "delete back")
        .bind("delete", keys("delete"), "delete forward")
}

struct Entry {
    record: Record,
    /// The row's text, lower-cased, and the continuation lines: what a
    /// filter word is looked for in.
    search: String,
    /// Continuation lines past [`Assembler::CONTINUATION_LIMIT`], counted
    /// rather than kept.
    more: usize,
}

/// One row: a record, and how many identical ones it stands for.
#[derive(Clone, Copy, Debug)]
struct Row {
    entry: usize,
    count: usize,
}

/// One word of the filter.
enum Term {
    Field(Predicate),
    Text(String),
}

/// A live view of log records. See the [module docs](self).
#[must_use = "a component does nothing until it is run or mounted"]
pub struct LogView {
    /// The records kept; `first` is the number of the first, as older ones
    /// are dropped past the limit.
    entries: VecDeque<Entry>,
    first: usize,
    rows: Vec<Row>,
    source: Option<Receiver<LogEvent>>,
    filter: TextBuffer,
    typing: bool,
    terms: Vec<Term>,
    cursor: usize,
    offset: usize,
    following: bool,
    paused: bool,
    /// Rows that arrived while the view was not following.
    unseen: usize,
    expanded: HashSet<usize>,
    expand_all: bool,
    collapse: bool,
    time: TimeMode,
    origin: Option<i64>,
    limit: usize,
    dropped: usize,
    keymap: Keymap,
    filter_keymap: Keymap,
    theme: Theme,
    tick: Duration,
}

impl Default for LogView {
    fn default() -> LogView {
        LogView::new()
    }
}

/// The most records drained from the source on one tick, so a flood
/// still lets keys through.
const DRAIN: usize = 20_000;

impl LogView {
    /// The records a view keeps by default before dropping the oldest.
    pub const LIMIT: usize = 200_000;

    /// An empty view.
    pub fn new() -> LogView {
        LogView {
            entries: VecDeque::new(),
            first: 0,
            rows: Vec::new(),
            source: None,
            filter: TextBuffer::new(),
            typing: false,
            terms: Vec::new(),
            cursor: 0,
            offset: 0,
            following: true,
            paused: false,
            unseen: 0,
            expanded: HashSet::new(),
            expand_all: false,
            collapse: true,
            time: TimeMode::Absolute,
            origin: None,
            limit: LogView::LIMIT,
            dropped: 0,
            keymap: log_view_keymap(),
            filter_keymap: log_filter_keymap(),
            theme: Theme::default(),
            tick: Duration::from_millis(100),
        }
    }

    /// Start with `records`.
    pub fn records(mut self, records: impl IntoIterator<Item = Record>) -> LogView {
        for record in records {
            self.push(record);
        }
        self
    }

    /// Take records from `source` as they arrive, checking it every 100 ms
    /// until its sender is dropped.
    pub fn source(mut self, source: Receiver<LogEvent>) -> LogView {
        self.source = Some(source);
        self
    }

    /// Fold runs of identical records into one row (default on).
    pub fn collapse(mut self, on: bool) -> LogView {
        self.collapse = on;
        self.rebuild();
        self
    }

    /// How times show (default as written).
    pub fn time(mut self, mode: TimeMode) -> LogView {
        self.time = mode;
        self
    }

    /// Show every record's fields, as if Enter had been pressed on each.
    pub fn expand_all(mut self, on: bool) -> LogView {
        self.expand_all = on;
        self
    }

    /// Start with this filter (see the [module docs](self)).
    pub fn filter(mut self, filter: impl Into<String>) -> LogView {
        self.filter.set_text(filter);
        self.refilter();
        self
    }

    /// Keep at most `records`, dropping the oldest (default
    /// [`LIMIT`](Self::LIMIT)).
    pub fn limit(mut self, records: usize) -> LogView {
        self.limit = records.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> LogView {
        self.theme = theme;
        self
    }

    /// The records received and kept.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The rows shown with the filter, a folded run counting once.
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// The records dropped past the limit.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Whether the view follows the end.
    pub fn is_following(&self) -> bool {
        self.following
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// The filter typed.
    pub fn filter_text(&self) -> &str {
        self.filter.text()
    }

    /// Add a record.
    pub fn push(&mut self, record: Record) {
        if self.origin.is_none() {
            self.origin = record.instant();
        }
        let search = search_text(&record);
        let entry = self.first + self.entries.len();
        self.entries.push_back(Entry {
            record,
            search,
            more: 0,
        });
        if self.matches(entry) {
            self.add_row(entry);
        }
        if self.entries.len() > self.limit + self.limit / 10 {
            self.trim();
        }
    }

    /// Add a line under the last record. Past
    /// [`Assembler::CONTINUATION_LIMIT`] lines it is only counted, and shown
    /// as `… N more lines`.
    pub fn push_continuation(&mut self, line: String) {
        let Some(last) = self.entries.back_mut() else {
            return;
        };
        if last.record.continuation.len() >= Assembler::CONTINUATION_LIMIT {
            last.more += 1;
            return;
        }
        last.search.push('\n');
        last.search.push_str(&line.to_lowercase());
        last.record.continuation.push(line);
    }

    /// Drop the oldest records down to the limit.
    fn trim(&mut self) {
        let excess = self.entries.len().saturating_sub(self.limit);
        self.entries.drain(..excess);
        self.first += excess;
        self.dropped += excess;
        self.expanded.retain(|entry| *entry >= self.first);
        let cursor_entry = self.rows.get(self.cursor).map(|row| row.entry);
        self.rebuild();
        self.cursor = cursor_entry
            .and_then(|entry| self.rows.iter().position(|row| row.entry >= entry))
            .unwrap_or(self.rows.len().saturating_sub(1));
        self.offset = self.offset.min(self.cursor);
    }

    fn entry(&self, entry: usize) -> &Entry {
        &self.entries[entry - self.first]
    }

    fn add_row(&mut self, entry: usize) {
        if self.collapse {
            if let Some(last) = self.rows.last().map(|row| row.entry) {
                let repeats = self.entry(last).record.same_as(&self.entry(entry).record);
                if repeats {
                    if let Some(row) = self.rows.last_mut() {
                        row.count += 1;
                    }
                    return;
                }
            }
        }
        self.rows.push(Row { entry, count: 1 });
        if self.following {
            self.cursor = self.rows.len() - 1;
        } else {
            self.unseen += 1;
        }
    }

    fn matches(&self, entry: usize) -> bool {
        let entry = self.entry(entry);
        self.terms.iter().all(|term| match term {
            Term::Field(predicate) => predicate.matches(&entry.record),
            Term::Text(word) => entry.search.contains(word.as_str()),
        })
    }

    /// The rows again, for a new filter or collapsing.
    fn rebuild(&mut self) {
        let following = self.following;
        self.rows.clear();
        let unseen = self.unseen;
        self.following = true;
        for entry in self.first..self.first + self.entries.len() {
            if self.matches(entry) {
                self.add_row(entry);
            }
        }
        self.following = following;
        self.unseen = unseen;
    }

    /// Parse the filter and show what it keeps.
    fn refilter(&mut self) {
        let cursor_entry = self.rows.get(self.cursor).map(|row| row.entry);
        self.terms = words(self.filter.text())
            .into_iter()
            .map(|word| {
                let field = word.contains(['=', '<', '>', '~']) || word.starts_with('!');
                match Predicate::parse(&word) {
                    Ok(predicate) if field => Term::Field(predicate),
                    _ => Term::Text(word.to_lowercase()),
                }
            })
            .collect();
        self.rebuild();
        self.cursor = if self.following {
            self.rows.len().saturating_sub(1)
        } else {
            cursor_entry
                .and_then(|entry| self.rows.iter().position(|row| row.entry >= entry))
                .unwrap_or(self.rows.len().saturating_sub(1))
        };
        self.offset = self.offset.min(self.cursor);
    }

    /// Take what the source sent; drop it once it is done.
    fn drain(&mut self) {
        let Some(source) = &self.source else {
            return;
        };
        let mut events = Vec::new();
        let mut done = false;
        while events.len() < DRAIN {
            match source.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }
        if done {
            self.source = None;
        }
        for event in events {
            match event {
                LogEvent::Record(record) => self.push(record),
                LogEvent::Continuation(line) => self.push_continuation(line),
            }
        }
    }

    fn is_expanded(&self, entry: usize) -> bool {
        self.expand_all != self.expanded.contains(&entry)
    }

    /// The lines a row takes.
    fn height(&self, row: &Row) -> usize {
        1 + if self.is_expanded(row.entry) {
            detail_count(self.entry(row.entry))
        } else {
            0
        }
    }

    /// The first row shown when the last row is at the bottom.
    fn bottom_offset(&self, page: usize) -> usize {
        let mut used = 0;
        let mut offset = self.rows.len();
        while offset > 0 {
            let height = self.height(&self.rows[offset - 1]);
            if used + height > page && used > 0 {
                break;
            }
            used += height;
            offset -= 1;
        }
        offset
    }

    /// The first row shown now.
    fn top(&self, page: usize) -> usize {
        if self.following {
            self.bottom_offset(page)
        } else {
            self.offset.min(self.rows.len().saturating_sub(1))
        }
    }

    /// Scroll just enough to show the cursor's row.
    fn show_cursor(&mut self, page: usize) {
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        loop {
            let used: usize = self.rows[self.offset..=self.cursor.min(self.rows.len() - 1)]
                .iter()
                .map(|row| self.height(row))
                .sum();
            if used <= page || self.offset >= self.cursor {
                break;
            }
            self.offset += 1;
        }
    }

    /// Stop following, keeping what is shown where it is.
    fn hold(&mut self, page: usize) {
        if self.following {
            self.offset = self.bottom_offset(page);
            self.following = false;
            self.unseen = 0;
        }
    }

    fn follow(&mut self) {
        self.following = true;
        self.paused = false;
        self.unseen = 0;
        self.cursor = self.rows.len().saturating_sub(1);
    }

    fn page(context: &Context<'_>) -> usize {
        context.height.saturating_sub(1).max(1)
    }

    fn move_cursor(&mut self, delta: isize, page: usize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let target = self.cursor.saturating_add_signed(delta).min(last);
        if delta > 0 && target == last && !self.paused {
            self.follow();
            return;
        }
        self.hold(page);
        self.cursor = target;
        self.show_cursor(page);
    }

    fn handle_filter(&mut self, event: &Event) -> Flow<()> {
        if let Event::Paste(text) = event {
            self.filter.insert(&crate::kit::pasted(text, " "));
            self.refilter();
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            return Flow::Continue;
        };
        let action = self.filter_keymap.action(key).map(str::to_string);
        let changed = match action.as_deref() {
            Some("apply") => {
                self.typing = false;
                false
            }
            Some("clear") => {
                self.typing = false;
                self.filter.set_text("");
                true
            }
            Some("left") => {
                self.filter.left();
                false
            }
            Some("right") => {
                self.filter.right();
                false
            }
            Some("home") => {
                self.filter.home();
                false
            }
            Some("end") => {
                self.filter.end();
                false
            }
            Some("delete-to-start") => {
                self.filter.delete_to_start();
                true
            }
            Some("delete-word") => {
                self.filter.delete_word();
                true
            }
            Some("backspace") => self.filter.backspace(),
            Some("delete") => self.filter.delete(),
            _ => match key.code {
                KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => {
                    let mut buffer = [0; 4];
                    self.filter.insert(c.encode_utf8(&mut buffer));
                    true
                }
                _ => false,
            },
        };
        if changed {
            self.refilter();
        }
        Flow::Continue
    }

    /// A row's line.
    fn row_line(&self, row: &Row, selected: bool, width: usize) -> Vec<Segment> {
        let record = &self.entry(row.entry).record;
        let theme = &self.theme;
        let mut line = Vec::new();
        if selected {
            line.push(text(format!("{} ", theme.pointer), &theme.pointer_style));
        } else {
            line.push(plain(" ".repeat(cell_len(&theme.pointer) + 1)));
        }
        if let Some(time) = self.time_of(record) {
            line.push(text(format!("{time} "), &theme.hint));
        }
        match record.severity() {
            Some(severity) => line.push(text(
                format!("{:<5} ", severity.as_str().to_uppercase()),
                &severity_style(severity),
            )),
            None => {
                if let Some(level) = record.level() {
                    line.push(plain(format!("{} ", one_line(&value_text(level)))));
                }
            }
        }
        if row.count > 1 {
            line.push(text(format!("×{} ", row.count), &style("bold")));
        }
        let message = record.message().unwrap_or_default();
        let message_style = if selected {
            theme.focused.clone()
        } else {
            Style::default()
        };
        line.push(text(one_line(&message), &message_style));
        let field = style("cyan");
        for (key, value) in shown_fields(record) {
            line.push(plain(" "));
            line.push(text(format!("{key}="), &field));
            line.push(plain(one_line(&value_text(value))));
        }
        if !record.continuation.is_empty() && !self.is_expanded(row.entry) {
            let lines = record.continuation.len() + self.entry(row.entry).more;
            let label = if lines == 1 { "line" } else { "lines" };
            line.push(text(format!(" (+{lines} {label})"), &theme.hint));
        }
        fit(line, width)
    }

    /// The lines under an expanded row, at most `room` of them: its
    /// fields, then its continuation lines.
    fn details(&self, row: &Row, width: usize, room: usize) -> Vec<Vec<Segment>> {
        let entry = self.entry(row.entry);
        let record = &entry.record;
        let theme = &self.theme;
        let indent = cell_len(&theme.pointer) + 3;
        let mut lines = Vec::new();
        let field = style("cyan");
        for (key, value) in record.fields.iter().take(room) {
            lines.push(fit(
                vec![
                    plain(" ".repeat(indent)),
                    text(format!("{key}: "), &field),
                    plain(one_line(&value_text(value))),
                ],
                width,
            ));
        }
        let more = (entry.more > 0).then(|| format!("… {} more lines", entry.more));
        let continuations = record.continuation.iter().map(String::as_str);
        for continuation in continuations
            .chain(more.as_deref())
            .take(room.saturating_sub(lines.len()))
        {
            lines.push(fit(
                vec![
                    plain(" ".repeat(indent)),
                    text("│ ", &theme.hint),
                    plain(continuation.replace('\t', "    ")),
                ],
                width,
            ));
        }
        lines
    }

    fn time_of(&self, record: &Record) -> Option<String> {
        match self.time {
            TimeMode::Hidden => None,
            TimeMode::Absolute => record.time().map(|time| one_line(&time)),
            TimeMode::Relative => match (record.instant(), self.origin) {
                (Some(at), Some(origin)) => Some(format_offset(at.saturating_sub(origin))),
                _ => record.time().map(|time| one_line(&time)),
            },
        }
    }

    fn status(&self, page: usize, width: usize) -> Vec<Segment> {
        let theme = &self.theme;
        if self.typing {
            let (shown, _) = self.filter.window(width.saturating_sub(3), None);
            let hint = format!("  {} matching · enter keep · esc clear", self.rows.len());
            return fit(
                vec![
                    text("/ ", &theme.prompt),
                    plain(shown),
                    text(hint, &theme.hint),
                ],
                width,
            );
        }
        let records = self.entries.len() + self.dropped;
        let mut status = plural(records, "record");
        if !self.filter.is_empty() {
            status.push_str(&format!(
                " · {} matching {:?}",
                self.rows.len(),
                self.filter.text()
            ));
        }
        let top = self.top(page);
        let state = if self.paused {
            format!("paused · {} new", self.unseen)
        } else if self.following {
            if self.source.is_some() {
                "following".to_string()
            } else {
                "at the end".to_string()
            }
        } else if self.unseen > 0 {
            format!("row {} · {} new · end follows", top + 1, self.unseen)
        } else {
            format!("row {} · end follows", top + 1)
        };
        status.push_str(&format!(" · {state}"));
        status.push_str(" · / filter · space pause · enter fields · q quit");
        fit(vec![text(status, &theme.hint)], width)
    }
}

impl Component for LogView {
    type Output = ();

    fn start(&mut self, _context: &Context<'_>) -> Flow<()> {
        self.drain();
        Flow::Continue
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<()> {
        let page = Self::page(context);
        if let Event::Tick = event {
            self.drain();
            return Flow::Continue;
        }
        if self.typing {
            return self.handle_filter(event);
        }
        let Some(key) = event.key() else {
            return Flow::Continue;
        };
        let action = self.keymap.action(key).map(str::to_string);
        match action.as_deref() {
            Some("quit") => return Flow::Done(()),
            Some("filter") => self.typing = true,
            Some("pause") => {
                if self.paused {
                    self.follow();
                } else {
                    self.hold(page);
                    self.paused = true;
                }
            }
            Some("expand") => {
                if let Some(row) = self.rows.get(self.cursor) {
                    let entry = row.entry;
                    if !self.expanded.remove(&entry) {
                        self.expanded.insert(entry);
                    }
                    if !self.following {
                        self.show_cursor(page);
                    }
                }
            }
            Some("up") => self.move_cursor(-1, page),
            Some("down") => self.move_cursor(1, page),
            Some("page-up") => self.move_cursor(-(page as isize), page),
            Some("page-down") => self.move_cursor(page as isize, page),
            Some("top") => {
                self.hold(page);
                self.cursor = 0;
                self.offset = 0;
            }
            Some("bottom") => self.follow(),
            _ => return Flow::Ignored,
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let page = Self::page(context);
        let mut lines = Vec::new();
        if self.rows.is_empty() {
            let message = if self.entries.is_empty() {
                "no records yet"
            } else {
                "no record matches the filter"
            };
            lines.push(fit(vec![text(message, &self.theme.hint)], width));
        }
        let top = self.top(page);
        for (index, row) in self.rows.iter().enumerate().skip(top) {
            if lines.len() >= page {
                break;
            }
            lines.push(self.row_line(row, index == self.cursor, width));
            if self.is_expanded(row.entry) {
                let room = page.saturating_sub(lines.len());
                lines.extend(self.details(row, width, room));
            }
        }
        lines.truncate(page);
        lines.push(self.status(page, width));
        let view = View::new(lines);
        if self.typing {
            let (_, caret) = self.filter.window(width.saturating_sub(3), None);
            let row = view.lines.len() - 1;
            return view.with_cursor(row, 2 + caret);
        }
        view
    }

    fn tick(&self) -> Option<Duration> {
        self.source.as_ref().map(|_| self.tick)
    }

    fn keymap(&self) -> Keymap {
        if self.typing {
            self.filter_keymap.clone()
        } else {
            self.keymap.clone()
        }
    }

    fn default_value(&self) -> Option<()> {
        Some(())
    }

    /// Without a terminal: every row, as text.
    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<()>, NotInteractive> {
        while self.source.is_some() {
            self.drain();
            if self.source.is_some() {
                std::thread::sleep(self.tick);
            }
        }
        for row in &self.rows {
            let line: String = self
                .row_line(row, false, usize::MAX / 2)
                .iter()
                .map(|segment| segment.text.as_str())
                .collect();
            io.write(line.trim_start());
            io.write("\n");
        }
        Ok(Some(()))
    }
}

fn style(definition: &str) -> Style {
    Style::parse(definition).expect("a built-in style")
}

fn severity_style(severity: Severity) -> Style {
    style(match severity {
        Severity::Fatal | Severity::Error => "bold red",
        Severity::Warn => "yellow",
        Severity::Info => "green",
        Severity::Debug | Severity::Trace => "dim",
    })
}

fn plural(count: usize, one: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {one}s")
    }
}

/// `text` on one line: line breaks as `↵`, tabs as spaces.
fn one_line(text: &str) -> String {
    if !text.contains(['\n', '\r', '\t']) {
        return text.to_string();
    }
    text.replace("\r\n", "↵")
        .replace(['\n', '\r'], "↵")
        .replace('\t', " ")
}

/// A value as a row shows it: a string as it is, `[a, b]`, `{k: v}`.
fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".into(),
        Value::Bool(v) => v.to_string(),
        Value::Integer(v) => v.to_string(),
        Value::Unsigned(v) => v.to_string(),
        Value::Integer128(v) => v.to_string(),
        Value::Unsigned128(v) => v.to_string(),
        Value::Float(v) => v.to_string(),
        Value::List(items) => {
            let items: Vec<String> = items.iter().map(value_text).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Map(entries) => {
            let entries: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{key}: {}", value_text(value)))
                .collect();
            format!("{{{}}}", entries.join(", "))
        }
    }
}

/// The fields a row shows after the message: all but the message, level
/// and time it already shows.
fn shown_fields(record: &Record) -> impl Iterator<Item = (&str, &Value)> {
    let message = record.first_of(MESSAGE_KEYS).map(|(key, _)| key);
    let level = record
        .severity()
        .and(record.first_of(LEVEL_KEYS).map(|(key, _)| key));
    let time = record.first_of(TIME_KEYS).map(|(key, _)| key);
    record.fields.iter().filter_map(move |(key, value)| {
        let key = key.as_str();
        (Some(key) != message && Some(key) != level && Some(key) != time).then_some((key, value))
    })
}

/// The lines under an expanded record.
fn detail_count(entry: &Entry) -> usize {
    entry.record.fields.len() + entry.record.continuation.len() + usize::from(entry.more > 0)
}

/// What a filter word is looked for in: the row's text and the
/// continuation lines, lower-cased.
fn search_text(record: &Record) -> String {
    let mut search = String::new();
    if let Some(time) = record.time() {
        search.push_str(&time);
        search.push(' ');
    }
    if let Some(level) = record.level() {
        search.push_str(&value_text(level));
        search.push(' ');
    }
    search.push_str(&record.message().unwrap_or_default());
    for (key, value) in shown_fields(record) {
        search.push(' ');
        search.push_str(key);
        search.push('=');
        search.push_str(&value_text(value));
    }
    for line in &record.continuation {
        search.push('\n');
        search.push_str(line);
    }
    search.to_lowercase()
}

/// The filter's words: split at spaces, except inside quotes.
fn words(filter: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for c in filter.chars() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                word.push(c);
            }
            (Some(open), c) if c == open => {
                quote = None;
                word.push(c);
            }
            (None, c) if c.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (_, c) => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
        .into_iter()
        .map(|word| {
            // A bare quoted word is the text inside its quotes.
            let bare = word.len() >= 2
                && (word.starts_with('"') && word.ends_with('"')
                    || word.starts_with('\'') && word.ends_with('\''));
            if bare {
                word[1..word.len() - 1].to_string()
            } else {
                word
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::{self, Script};
    use crate::Outcome;
    use std::sync::mpsc;

    fn record(level: &str, message: &str, second: u32) -> Record {
        Record::new("json")
            .field(
                "time",
                Value::String(format!("2026-10-18T10:00:{second:02}Z")),
            )
            .field("level", Value::String(level.into()))
            .field("msg", Value::String(message.into()))
    }

    fn run(view: LogView, script: Script, rows: u16) -> headless::Record {
        let (outcome, record) = headless::run(view, script, 80, rows);
        assert_eq!(outcome.unwrap(), Outcome::Done(()));
        record
    }

    #[test]
    fn rows_show_time_level_message_and_fields() {
        let view = LogView::new().records([record("warn", "slow", 0)
            .field("service", Value::String("api".into()))
            .field("latency_ms", Value::Integer(812))]);
        let frames = run(view, Script::new().keys("q"), 5);
        let frame = frames.last_frame();
        assert!(
            frame.contains("❯ 2026-10-18T10:00:00Z WARN  slow service=api latency_ms=812"),
            "{frame}"
        );
        assert!(frame.contains("1 record · at the end"), "{frame}");
    }

    #[test]
    fn repeats_fold_into_one_row_with_a_count() {
        let view = LogView::new().records([
            record("error", "refused", 0),
            record("error", "refused", 1),
            record("error", "refused", 2),
            record("info", "ok", 3),
        ]);
        assert_eq!(view.rows(), 2);
        let frames = run(view, Script::new().keys("q"), 6);
        assert!(
            frames.last_frame().contains("ERROR ×3 refused"),
            "{}",
            frames.last_frame()
        );
        let flat = LogView::new()
            .collapse(false)
            .records([record("error", "refused", 0), record("error", "refused", 1)]);
        assert_eq!(flat.rows(), 2);
    }

    #[test]
    fn the_filter_line_takes_words_and_predicates() {
        let records = [
            record("info", "GET /a", 0).field("status", Value::Integer(200)),
            record("warn", "GET /b", 1).field("status", Value::Integer(404)),
            record("error", "POST /c", 2).field("status", Value::Integer(503)),
        ];
        let view = LogView::new().records(records.clone());
        let frames = run(
            view,
            Script::new()
                .keys("/")
                .text("status>=400 get")
                .keys("enter q"),
            8,
        );
        let frame = frames.last_frame();
        assert!(frame.contains("GET /b"), "{frame}");
        assert!(!frame.contains("POST /c"), "{frame}");
        assert!(!frame.contains("GET /a"), "{frame}");
        assert!(frame.contains("1 matching"), "{frame}");
        // Escape clears it.
        let view = LogView::new().records(records);
        let frames = run(
            view,
            Script::new().keys("/").text("post").keys("escape q"),
            8,
        );
        let typed = frames
            .frames
            .iter()
            .find(|frame| frame.contains("/ post"))
            .expect("the filter line shows what is typed");
        assert!(!typed.contains("GET /a"), "{typed}");
        assert!(frames.last_frame().contains("GET /a"));
        assert!(frames.last_frame().contains("3 records"));
    }

    #[test]
    fn enter_shows_the_fields_and_continuation_lines() {
        let mut failed = record("error", "failed", 0).field("trace_id", Value::String("t1".into()));
        failed
            .continuation
            .push("Traceback (most recent call last):".into());
        let view = LogView::new().records([failed]);
        let frames = run(view, Script::new().keys("enter q"), 10);
        let frame = frames.last_frame();
        assert!(frame.contains("trace_id: t1"), "{frame}");
        assert!(
            frame.contains("│ Traceback (most recent call last):"),
            "{frame}"
        );
        let first = &frames.frames[0];
        assert!(first.contains("(+1 line)"), "{first}");
    }

    #[test]
    fn continuation_lines_are_capped_and_counted() {
        let mut view = LogView::new().records([record("error", "failed", 0)]);
        let extra = 5;
        for n in 0..Assembler::CONTINUATION_LIMIT + extra {
            view.push_continuation(format!("line {n}"));
        }
        let entry = view.entries.back().unwrap();
        assert_eq!(
            entry.record.continuation.len(),
            Assembler::CONTINUATION_LIMIT
        );
        assert_eq!(entry.more, extra);
        let row = view.rows[0];
        let details = view.details(&row, 80, usize::MAX);
        let last: String = details
            .last()
            .unwrap()
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(last.contains("… 5 more lines"), "{last}");
        // Only what fits the page is built.
        assert_eq!(view.details(&row, 80, 4).len(), 4);
        let frames = run(view, Script::new().keys("enter q"), 10);
        let frame = frames.last_frame();
        assert!(frame.contains("line 0"), "{frame}");
        let first = &frames.frames[0];
        assert!(first.contains("(+1005 lines)"), "{first}");
    }

    #[test]
    fn relative_time_before_the_origin_does_not_overflow() {
        let mut view = LogView::new();
        // The first record's time (the origin) is i64::MAX microseconds,
        // the second's i64::MIN.
        for micros in [i64::MAX, i64::MIN] {
            let time = Value::Integer128(i128::from(micros) * 1_000);
            view.push(Record::new("json").field("time", time));
        }
        let record = view.entries.back().unwrap().record.clone();
        assert_eq!(record.instant(), Some(i64::MIN));
        view.time = TimeMode::Relative;
        assert!(view.time_of(&record).is_some());
    }

    #[test]
    fn a_channel_sink_feeds_the_view_with_events() {
        use rich_ext::adapters::EventSink;
        use rich_ext::event::{EventContext, Message, Severity, StructuredEvent};
        use rich_ext::log_stream::ChannelSink;

        let (sink, receiver) = ChannelSink::<LogEvent>::bounded(8);
        let event =
            StructuredEvent::new(Message::Literal("disk full".into())).context(EventContext {
                severity: Some(Severity::Error),
                target: Some("store".into()),
                ..Default::default()
            });
        sink.emit(event).unwrap();
        let view = LogView::new().source(receiver);
        let frames = run(
            view,
            Script::new().wait(Duration::from_millis(150)).keys("q"),
            4,
        );
        drop(sink);
        let frame = frames.last_frame();
        assert!(frame.contains("ERROR disk full"), "{frame}");
        assert!(frame.contains("target=store"), "{frame}");
    }

    #[test]
    fn it_follows_a_source_and_pauses_on_space() {
        let (sender, receiver) = mpsc::channel();
        for second in 0..3 {
            sender
                .send(LogEvent::Record(record(
                    "info",
                    &format!("n{second}"),
                    second,
                )))
                .unwrap();
        }
        let view = LogView::new().source(receiver);
        // The script cannot send, so the records are queued up front; the
        // view takes them when it starts, and the sender stays open, so it
        // goes on following.
        let script = Script::new()
            .wait(Duration::from_millis(150))
            .keys("space")
            .wait(Duration::from_millis(150))
            .keys("space q");
        let frames = run(view, script, 3);
        drop(sender);
        let frame = frames.last_frame();
        assert!(
            frame.contains("n2") && frame.contains("following"),
            "{frame}"
        );
        assert!(
            frames.frames.iter().any(|f| f.contains("paused · 0 new")),
            "{:#?}",
            frames.frames
        );
    }

    #[test]
    fn a_live_source_keeps_the_end_in_view_until_the_cursor_moves_up() {
        let mut view = LogView::new();
        for second in 0..20 {
            view.push(record("info", &format!("n{second}"), second));
        }
        assert!(view.is_following());
        let frames = run(view, Script::new().keys("up up q"), 6);
        let followed = &frames.frames[0];
        assert!(followed.contains("n19"), "{followed}");
        let held = frames.last_frame();
        assert!(held.contains("❯") && held.contains("n17"), "{held}");
        assert!(held.contains("end follows"), "{held}");
        // Pushed while held: counted as new, not scrolled to.
        let mut view = LogView::new();
        for second in 0..5 {
            view.push(record("info", &format!("n{second}"), second));
        }
        view.hold(3);
        view.push(record("info", "later", 9));
        assert_eq!(view.unseen, 1);
        view.follow();
        assert_eq!(view.cursor, view.rows() - 1);
    }

    #[test]
    fn relative_times_count_from_the_first_record() {
        let view = LogView::new()
            .time(TimeMode::Relative)
            .records([record("info", "a", 0), record("info", "b", 5)]);
        let frames = run(view, Script::new().keys("q"), 5);
        assert!(
            frames.last_frame().contains("+5.000s INFO  b"),
            "{}",
            frames.last_frame()
        );
    }

    #[test]
    fn the_oldest_records_drop_past_the_limit() {
        let mut view = LogView::new().limit(10).collapse(false);
        for n in 0..30 {
            view.push(record("info", &format!("n{n}"), n % 60));
        }
        assert!(view.len() <= 11, "{}", view.len());
        assert_eq!(view.len() + view.dropped(), 30);
        assert_eq!(view.rows(), view.len());
    }

    #[test]
    fn filter_words_keep_quotes_together() {
        assert_eq!(
            words(r#"msg~"timed out" api 'two words'"#),
            [r#"msg~"timed out""#, "api", "two words"]
        );
        assert!(words("   ").is_empty());
    }

    #[test]
    fn without_a_terminal_it_writes_the_rows() {
        struct Lines(String);
        impl LineIo for Lines {
            fn write(&mut self, text: &str) {
                self.0.push_str(text);
            }
            fn read_line(&mut self) -> Option<String> {
                None
            }
        }
        let mut view = LogView::new().records([record("info", "a", 0), record("warn", "b", 1)]);
        let mut io = Lines(String::new());
        assert_eq!(view.prompt(&mut io), Ok(Some(())));
        assert_eq!(
            io.0,
            "2026-10-18T10:00:00Z INFO  a\n2026-10-18T10:00:01Z WARN  b\n"
        );
    }
}
