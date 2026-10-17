//! Date and time pickers (#719): [`DatePicker`], [`TimePicker`] and
//! [`DateTimePicker`].
//!
//! A date is picked on a month grid: ←/→ move a day, ↑/↓ a week,
//! PgUp/PgDn (or `[`/`]`) a month, Shift+PgUp/PgDn (or `{`/`}`) a year,
//! Home/End to the first and last of the month, and `t` to today. A time
//! is picked field by field: ←/→ (or Tab) move between the hour, the
//! minute, the second and AM/PM, ↑/↓ change the field, and digits type it.
//! With both, Tab moves between the grid and the time. Enter picks; Esc
//! cancels. Every key comes from the keymap (context `date`), so it can be
//! rebound.
//!
//! A range ([`min`](DatePicker::min), [`max`](DatePicker::max)) limits
//! what can be picked: days outside it are dimmed and the grid does not go
//! to them; a time outside it is refused on Enter, with a message under
//! the picker that says what the limit is, as is an answer a
//! [validator](DatePicker::validate) refuses. The value so far shows on the
//! question line, and the selected day is in brackets as well as reversed,
//! so both read without colour.
//!
//! Without a terminal each asks for a line and parses it: `2026-10-17`,
//! `14:30` (or `2:30 pm`), or `2026-10-17 14:30`; an empty line answers
//! with the default. A line outside the range or refused by the validator
//! is an error, as is one that does not parse.

use std::cell::Cell;
use std::sync::Arc;

use rich::{Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, question, text, Theme};
use crate::datetime::{Date, DateTime, Time};
use crate::event::{Button, Event, Key, KeyCode, MouseKind};
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};

/// The keys of the date and time pickers, in context `date`.
pub fn date_keymap() -> Keymap {
    Keymap::new("date")
        .bind("pick", keys("enter"), "pick")
        .bind("cancel", keys("escape"), "cancel")
        .bind("left", keys("left h"), "a day earlier; the field before")
        .bind("right", keys("right l"), "a day later; the next field")
        .bind("up", keys("up k"), "a week earlier; the field up")
        .bind("down", keys("down j"), "a week later; the field down")
        .bind("previous-month", keys("pageup ["), "a month earlier")
        .bind("next-month", keys("pagedown ]"), "a month later")
        .bind("previous-year", keys("shift+pageup {"), "a year earlier")
        .bind("next-year", keys("shift+pagedown }"), "a year later")
        .bind("first-day", keys("home"), "the month's first day")
        .bind("last-day", keys("end"), "the month's last day")
        .bind("today", keys("t"), "today")
        .bind("next-field", keys("tab"), "the next part")
        .bind("previous-field", keys("shift+tab"), "the part before")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Date,
    Time,
    DateTime,
}

/// What has the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Grid,
    Hour,
    Minute,
    Second,
    Half,
}

type Check = Arc<dyn Fn(DateTime) -> Result<(), String> + Send + Sync>;

/// The rows of a month grid: always six weeks, so the picker keeps its
/// height from month to month.
const WEEKS: usize = 6;
/// The grid's width: a space, then a separator and two digits a day, then
/// a last separator.
const GRID_WIDTH: usize = 23;
/// The rows above the grid's weeks: the question, the month, the weekdays.
const GRID_TOP: usize = 3;

/// The state the three pickers share.
struct Chooser {
    prompt: String,
    mode: Mode,
    value: DateTime,
    min: Option<DateTime>,
    max: Option<DateTime>,
    seconds: bool,
    twelve_hour: bool,
    today: Date,
    part: Part,
    /// Digits typed into the focused time field so far.
    typed: String,
    /// Why the last Enter was refused, until the next key.
    message: Option<String>,
    check: Option<Check>,
    default: Option<DateTime>,
    theme: Theme,
    mouse: bool,
    keymap: Keymap,
    /// The day the last click selected: a click on it again picks it.
    clicked: Option<Date>,
    answer: Option<Option<String>>,
    /// The width last drawn at, for the mouse.
    width: Cell<usize>,
}

impl Chooser {
    fn new(prompt: String, mode: Mode) -> Chooser {
        let now = DateTime::now_utc();
        let value = match mode {
            Mode::Date => DateTime::new(now.date, Time::MIDNIGHT),
            Mode::Time | Mode::DateTime => {
                DateTime::new(now.date, Time::new(now.time.hour, now.time.minute, 0))
            }
        };
        Chooser {
            prompt,
            mode,
            value,
            min: None,
            max: None,
            seconds: false,
            twelve_hour: false,
            today: now.date,
            part: if mode == Mode::Time {
                Part::Hour
            } else {
                Part::Grid
            },
            typed: String::new(),
            message: None,
            check: None,
            default: None,
            theme: Theme::default(),
            mouse: false,
            keymap: date_keymap(),
            clicked: None,
            answer: None,
            width: Cell::new(80),
        }
    }

    fn has_date(&self) -> bool {
        self.mode != Mode::Time
    }

    fn has_time(&self) -> bool {
        self.mode != Mode::Date
    }

    /// The parts Tab goes through, in order.
    fn parts(&self) -> Vec<Part> {
        let mut parts = Vec::new();
        if self.has_date() {
            parts.push(Part::Grid);
        }
        if self.has_time() {
            parts.extend([Part::Hour, Part::Minute]);
            if self.seconds {
                parts.push(Part::Second);
            }
            if self.twelve_hour {
                parts.push(Part::Half);
            }
        }
        parts
    }

    /// Whether `date` has a moment in the range.
    fn day_allowed(&self, date: Date) -> bool {
        self.min.is_none_or(|min| date >= min.date) && self.max.is_none_or(|max| date <= max.date)
    }

    /// `date`, moved into the range.
    fn clamp_day(&self, date: Date) -> Date {
        let mut date = date;
        if let Some(min) = self.min {
            date = date.max(min.date);
        }
        if let Some(max) = self.max {
            date = date.min(max.date);
        }
        date
    }

    /// What is compared with the range: a date picker's day, a time
    /// picker's time, or both.
    fn key_of(&self, at: DateTime) -> (Option<Date>, Option<Time>) {
        match self.mode {
            Mode::Date => (Some(at.date), None),
            Mode::Time => (None, Some(at.time)),
            Mode::DateTime => (Some(at.date), Some(at.time)),
        }
    }

    /// How a value is written: as the picker answers it.
    fn format(&self, at: DateTime) -> String {
        match self.mode {
            Mode::Date => at.date.to_string(),
            Mode::Time => at.time.format(self.seconds, self.twelve_hour),
            Mode::DateTime => at.format(self.seconds, self.twelve_hour),
        }
    }

    /// Why `at` cannot be picked, if it cannot.
    fn refusal(&self, at: DateTime) -> Option<String> {
        let at_key = self.key_of(at);
        if let Some(min) = self.min {
            if at_key < self.key_of(min) {
                return Some(format!("the earliest is {}", self.format(min)));
            }
        }
        if let Some(max) = self.max {
            if at_key > self.key_of(max) {
                return Some(format!("the latest is {}", self.format(max)));
            }
        }
        self.check.as_ref().and_then(|check| check(at).err())
    }

    fn set_day(&mut self, date: Date) {
        self.value.date = self.clamp_day(date);
    }

    /// Change the focused time field by `delta`, wrapping round.
    fn step_field(&mut self, delta: i64) {
        let time = self.value.time;
        let wrap =
            |value: u32, by: i64, modulus: i64| (value as i64 + by).rem_euclid(modulus) as u32;
        self.value.time = match self.part {
            Part::Hour => Time {
                hour: wrap(time.hour, delta, 24),
                ..time
            },
            Part::Minute => Time {
                minute: wrap(time.minute, delta, 60),
                ..time
            },
            Part::Second => Time {
                second: wrap(time.second, delta, 60),
                ..time
            },
            Part::Half => Time {
                hour: (time.hour + 12) % 24,
                ..time
            },
            Part::Grid => time,
        };
    }

    /// A digit typed into the focused time field: two make the field, and
    /// the focus moves on.
    fn type_digit(&mut self, digit: char) {
        self.typed.push(digit);
        let number: u32 = self.typed.parse().unwrap_or(0);
        let time = self.value.time;
        let (most, full) = match self.part {
            Part::Hour if self.twelve_hour => (12, number >= 2 || self.typed.len() == 2),
            Part::Hour => (23, number >= 3 || self.typed.len() == 2),
            _ => (59, number >= 6 || self.typed.len() == 2),
        };
        let number = number.min(most);
        self.value.time = match self.part {
            Part::Hour if self.twelve_hour => {
                let pm = time.hour >= 12;
                let hour = if self.typed.len() == 2 || full {
                    number.max(1) % 12
                } else {
                    number % 12
                };
                Time {
                    hour: hour + if pm { 12 } else { 0 },
                    ..time
                }
            }
            Part::Hour => Time {
                hour: number,
                ..time
            },
            Part::Minute => Time {
                minute: number,
                ..time
            },
            Part::Second => Time {
                second: number,
                ..time
            },
            _ => time,
        };
        if full {
            self.typed.clear();
            self.move_part(1, false);
        }
    }

    /// Move the focus `by` parts; `wrap` goes round from the last to the
    /// first (Tab), otherwise it stops at the ends (the arrows).
    fn move_part(&mut self, by: isize, wrap: bool) {
        let parts = self.parts();
        let at = parts
            .iter()
            .position(|part| *part == self.part)
            .unwrap_or(0);
        let count = parts.len() as isize;
        let next = at as isize + by;
        let next = if wrap {
            next.rem_euclid(count)
        } else {
            // The arrows stay among the time fields.
            let first = if self.has_date() { 1 } else { 0 };
            next.clamp(first.min(count - 1), count - 1)
        };
        self.part = parts[next as usize];
        self.typed.clear();
    }

    fn pick(&mut self) -> Flow<DateTime> {
        if let Some(why) = self.refusal(self.value) {
            self.message = Some(why);
            return Flow::Continue;
        }
        self.answer = Some(Some(self.format(self.value)));
        Flow::Done(self.value)
    }

    fn act(&mut self, action: &str) -> Flow<DateTime> {
        let on_grid = self.part == Part::Grid;
        let day = self.value.date;
        match action {
            "pick" => return self.pick(),
            "cancel" => {
                self.answer = Some(None);
                return Flow::Cancel;
            }
            "next-field" | "previous-field" if self.parts().len() > 1 => {
                self.move_part(if action == "next-field" { 1 } else { -1 }, true);
            }
            "left" if on_grid => self.set_day(day.add_days(-1)),
            "right" if on_grid => self.set_day(day.add_days(1)),
            "up" if on_grid => self.set_day(day.add_days(-7)),
            "down" if on_grid => self.set_day(day.add_days(7)),
            "left" => self.move_part(-1, false),
            "right" => self.move_part(1, false),
            "up" => self.step_field(1),
            "down" => self.step_field(-1),
            "previous-month" if self.has_date() => self.set_day(day.add_months(-1)),
            "next-month" if self.has_date() => self.set_day(day.add_months(1)),
            "previous-year" if self.has_date() => self.set_day(day.add_months(-12)),
            "next-year" if self.has_date() => self.set_day(day.add_months(12)),
            "first-day" if on_grid => self.set_day(day.first_of_month()),
            "last-day" if on_grid => self.set_day(day.last_of_month()),
            "today" if self.has_date() => self.set_day(self.today),
            _ => return Flow::Ignored,
        }
        Flow::Continue
    }

    fn key(&mut self, key: Key) -> Flow<DateTime> {
        let plain_key = !key.modifiers.ctrl && !key.modifiers.alt;
        // Digits and AM/PM in a time field come before the bindings (`h`
        // and `l` move, but not in the middle of `14`).
        if plain_key && self.part != Part::Grid {
            match key.code {
                KeyCode::Char(c @ '0'..='9') if self.part != Part::Half => {
                    self.type_digit(c);
                    return Flow::Continue;
                }
                KeyCode::Char(c @ ('a' | 'A' | 'p' | 'P')) if self.twelve_hour => {
                    let pm = c.eq_ignore_ascii_case(&'p');
                    let hour = self.value.time.hour % 12 + if pm { 12 } else { 0 };
                    self.value.time.hour = hour;
                    return Flow::Continue;
                }
                _ => {}
            }
        }
        let Some(action) = self.keymap.action(key).map(str::to_string) else {
            return Flow::Ignored;
        };
        // A move away from a field ends what was typed into it.
        self.typed.clear();
        self.act(&action)
    }

    /// Where the time row is, under the question (and the grid).
    fn time_row(&self) -> usize {
        if self.has_date() {
            GRID_TOP + WEEKS
        } else {
            1
        }
    }

    /// The day under a click at `column`, `row`, if a day is there.
    fn day_at(&self, column: usize, row: usize) -> Option<Date> {
        if !self.has_date() || !(GRID_TOP..GRID_TOP + WEEKS).contains(&row) || column < 2 {
            return None;
        }
        let cell = (column - 1) / 3;
        if cell >= 7 {
            return None;
        }
        let first = self.value.date.first_of_month();
        let at = (row - GRID_TOP) * 7 + cell;
        let day = (at + 1).checked_sub(first.weekday() as usize)?;
        (1..=Date::days_in_month(first.year, first.month) as usize)
            .contains(&day)
            .then_some(Date {
                day: day as u32,
                ..first
            })
    }

    /// The time field under `column` of the time row.
    fn field_at(&self, column: usize) -> Option<Part> {
        let mut at = self.time_label().len();
        for (part, width) in self.time_fields() {
            if (at..at + width).contains(&column) {
                return Some(part);
            }
            at += width + 1;
        }
        None
    }

    fn mouse_event(&mut self, kind: MouseKind, column: usize, row: usize) -> Flow<DateTime> {
        let over_grid = self.has_date() && row < GRID_TOP + WEEKS;
        match kind {
            MouseKind::Down(Button::Left) => {
                if let Some(day) = self.day_at(column, row) {
                    if !self.day_allowed(day) {
                        return Flow::Continue;
                    }
                    self.part = Part::Grid;
                    if self.clicked == Some(day) && self.value.date == day {
                        return self.pick();
                    }
                    self.value.date = day;
                    self.clicked = Some(day);
                } else if self.has_time() && row == self.time_row() {
                    if let Some(part) = self.field_at(column) {
                        self.part = part;
                        self.typed.clear();
                    }
                }
            }
            MouseKind::ScrollUp | MouseKind::ScrollDown => {
                let by = if kind == MouseKind::ScrollUp { -1 } else { 1 };
                if over_grid {
                    self.set_day(self.value.date.add_months(by));
                } else if self.has_time() && self.part != Part::Grid {
                    // The wheel turns the focused field, as ↑ does.
                    self.step_field(-(by as i64));
                }
            }
            _ => return Flow::Ignored,
        }
        Flow::Continue
    }

    fn event(&mut self, event: &Event) -> Flow<DateTime> {
        if let Some(mouse) = event.mouse() {
            self.message = None;
            return self.mouse_event(mouse.kind, mouse.column as usize, mouse.row as usize);
        }
        match event.key() {
            Some(key) => {
                self.message = None;
                self.key(key)
            }
            None => Flow::Ignored,
        }
    }

    /// The text before the time fields.
    fn time_label(&self) -> &'static str {
        if self.has_date() {
            "  Time "
        } else {
            "  "
        }
    }

    /// The time's fields and their widths, with brackets round the
    /// focused one.
    fn time_fields(&self) -> Vec<(Part, usize)> {
        let mut fields = vec![(Part::Hour, 2), (Part::Minute, 2)];
        if self.seconds {
            fields.push((Part::Second, 2));
        }
        if self.twelve_hour {
            fields.push((Part::Half, 2));
        }
        fields
            .into_iter()
            .map(|(part, width)| (part, width + if part == self.part { 2 } else { 0 }))
            .collect()
    }

    fn time_line(&self, width: usize) -> Vec<Segment> {
        let theme = &self.theme;
        let time = self.value.time;
        let out = self
            .refusal(self.value)
            .is_some_and(|_| self.min.is_some() || self.max.is_some());
        let reverse = Style::parse("reverse").expect("a built-in style");
        let mut line = vec![plain(self.time_label())];
        let (hour, pm) = time.twelve_hour();
        for (index, (part, _)) in self.time_fields().into_iter().enumerate() {
            if index > 0 {
                line.push(plain(if part == Part::Half { " " } else { ":" }));
            }
            let shown = match part {
                Part::Hour if self.twelve_hour => format!("{hour:02}"),
                Part::Hour => format!("{:02}", time.hour),
                Part::Minute => format!("{:02}", time.minute),
                Part::Second => format!("{:02}", time.second),
                _ => (if pm { "PM" } else { "AM" }).to_string(),
            };
            let shown = if part == self.part && !self.typed.is_empty() {
                format!("{:_<2}", self.typed)
            } else {
                shown
            };
            if part == self.part {
                line.push(text(format!("[{shown}]"), &theme.focused.combine(&reverse)));
            } else if out {
                line.push(text(shown, &theme.error));
            } else {
                line.push(plain(shown));
            }
        }
        fit(line, width)
    }

    fn grid_lines(&self, width: usize) -> Vec<Vec<Segment>> {
        let theme = &self.theme;
        let selected = self.value.date;
        let first = selected.first_of_month();
        let title = format!("{} {}", selected.month_name(), selected.year);
        let pad = GRID_WIDTH.saturating_sub(title.len()) / 2;
        let title_style = Style::parse("bold").expect("a built-in style");
        let mut lines = vec![
            fit(
                vec![plain(" ".repeat(pad)), text(title, &title_style)],
                width,
            ),
            fit(vec![text("  Mo Tu We Th Fr Sa Su", &theme.hint)], width),
        ];
        let reverse = Style::parse("reverse").expect("a built-in style");
        let underline = Style::parse("underline").expect("a built-in style");
        let today_style = Style::parse("bold").expect("a built-in style");
        let chosen = if self.part == Part::Grid {
            theme.focused.combine(&reverse)
        } else {
            underline
        };
        let offset = first.weekday() as usize;
        let days = Date::days_in_month(first.year, first.month) as usize;
        for week in 0..WEEKS {
            let mut line = vec![plain(" ")];
            for cell in 0..7 {
                let at = week * 7 + cell;
                let day = (at + 1)
                    .checked_sub(offset)
                    .filter(|day| (1..=days).contains(day));
                let is = |cell: usize| {
                    (week * 7 + cell + 1)
                        .checked_sub(offset)
                        .is_some_and(|day| day == selected.day as usize)
                };
                // The separator before the day: a bracket opens the
                // selected day and closes the one before.
                line.push(plain(if is(cell) {
                    "["
                } else if cell > 0 && is(cell - 1) {
                    "]"
                } else {
                    " "
                }));
                let Some(day) = day else {
                    line.push(plain("  "));
                    continue;
                };
                let date = Date {
                    day: day as u32,
                    ..first
                };
                let label = format!("{day:>2}");
                line.push(if date == selected {
                    text(label, &chosen)
                } else if !self.day_allowed(date) {
                    text(label, &theme.hint)
                } else if date == self.today {
                    text(label, &today_style)
                } else {
                    plain(label)
                });
            }
            if is_last_selected(week, offset, selected.day) {
                line.push(plain("]"));
            }
            lines.push(fit(line, width));
        }
        lines
    }

    fn hint(&self) -> String {
        let mut hint = String::from("  ");
        hint.push_str(match self.part {
            Part::Grid => "←→↑↓ day · pgup/pgdn month · t today",
            _ => "←→ field · ↑↓ change · type digits",
        });
        if self.parts().len() > 1 {
            hint.push_str(match (self.part, self.mode) {
                (Part::Grid, Mode::DateTime) => " · tab time",
                (_, Mode::DateTime) => " · tab date",
                _ => " · tab next",
            });
        }
        hint.push_str(" · enter pick · esc cancel");
        hint
    }

    fn range_line(&self) -> Option<String> {
        let shown = |at: DateTime| self.format(at);
        match (self.min, self.max) {
            (None, None) => None,
            (Some(min), Some(max)) => Some(format!("  from {} to {}", shown(min), shown(max))),
            (Some(min), None) => Some(format!("  from {}", shown(min))),
            (None, Some(max)) => Some(format!("  until {}", shown(max))),
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        self.width.set(width);
        let theme = &self.theme;
        let mut header = question(theme, &self.prompt);
        if let Some(answer) = &self.answer {
            header.push(match answer {
                Some(answer) => text(answer.clone(), &theme.answer),
                None => text("cancelled", &theme.hint),
            });
            return View::new(vec![fit(header, width)]);
        }
        // The value so far, as it would be answered.
        header.push(plain(self.format(self.value)));
        let mut lines = vec![fit(header, width)];
        if self.has_date() {
            lines.extend(self.grid_lines(width));
        }
        if self.has_time() {
            lines.push(self.time_line(width));
        }
        if let Some(range) = self.range_line() {
            lines.push(fit(vec![text(range, &theme.hint)], width));
        }
        lines.push(fit(vec![text(self.hint(), &theme.hint)], width));
        if let Some(message) = &self.message {
            lines.push(fit(
                vec![text(format!("  ✗ {message}"), &theme.error)],
                width,
            ));
        }
        View::new(lines)
    }

    /// What the line-based form expects, for its prompt.
    fn shape(&self) -> &'static str {
        match (self.mode, self.seconds) {
            (Mode::Date, _) => "YYYY-MM-DD",
            (Mode::Time, false) => "HH:MM",
            (Mode::Time, true) => "HH:MM:SS",
            (Mode::DateTime, false) => "YYYY-MM-DD HH:MM",
            (Mode::DateTime, true) => "YYYY-MM-DD HH:MM:SS",
        }
    }

    fn parse(&self, line: &str) -> Result<DateTime, String> {
        match self.mode {
            Mode::Date => line.parse::<Date>().map(DateTime::from),
            Mode::Time => line
                .parse::<Time>()
                .map(|time| DateTime::new(self.value.date, time)),
            Mode::DateTime => line.parse::<DateTime>(),
        }
    }

    fn ask(&mut self, io: &mut dyn LineIo) -> Result<Option<DateTime>, NotInteractive> {
        let mut prompt = format!("{} ({}", self.prompt, self.shape());
        if let Some(range) = self.range_line() {
            prompt.push_str(&format!(", {}", range.trim()));
        }
        prompt.push(')');
        if let Some(default) = self.default {
            prompt.push_str(&format!(" [{}]", self.format(default)));
        }
        io.write(&format!("{prompt}: "));
        let line = match io.read_line() {
            Some(line) if !line.trim().is_empty() => line,
            // Input ended, or an empty line: the default answers.
            ended => {
                return match self.default {
                    Some(default) => Ok(Some(default)),
                    None if ended.is_none() => Err(NotInteractive::Ended),
                    None => Err(NotInteractive::Invalid(format!(
                        "no answer given ({})",
                        self.shape()
                    ))),
                }
            }
        };
        let at = self.parse(&line).map_err(NotInteractive::Invalid)?;
        if let Some(why) = self.refusal(at) {
            return Err(NotInteractive::Invalid(format!(
                "{}: {why}",
                self.format(at)
            )));
        }
        Ok(Some(at))
    }
}

/// Whether the selected `day` is the last cell of `week` (Sunday), so the
/// row's closing bracket is after it.
fn is_last_selected(week: usize, offset: usize, day: u32) -> bool {
    (week * 7 + 7).checked_sub(offset) == Some(day as usize)
}

/// The builders every picker has.
macro_rules! shared_builders {
    () => {
        pub fn theme(mut self, theme: Theme) -> Self {
            self.0.theme = theme;
            self
        }

        /// Report the mouse (#476): a click selects a day (a second click
        /// on it picks it) or focuses a time field, and the wheel turns the
        /// month or the focused field.
        pub fn with_mouse(mut self, on: bool) -> Self {
            self.0.mouse = on;
            self
        }

        /// Make `keys` do `action` (see [`date_keymap`]) on this picker
        /// only; no keys unbinds it.
        pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
            self.0.keymap.rebind(action, keys);
            self
        }
    };
}

/// The builders of a picker with a time.
macro_rules! time_builders {
    () => {
        /// Pick the seconds too.
        pub fn seconds(mut self, on: bool) -> Self {
            self.0.seconds = on;
            if !on {
                self.0.value.time.second = 0;
            }
            self
        }

        /// Show a 12-hour clock with AM and PM (`a` and `p` set them).
        pub fn twelve_hour(mut self, on: bool) -> Self {
            self.0.twelve_hour = on;
            self
        }
    };
}

/// The builders of a picker with a date.
macro_rules! date_builders {
    () => {
        /// The day marked as today and `t` goes to (default: today in
        /// UTC).
        pub fn today(mut self, today: Date) -> Self {
            self.0.today = today;
            self
        }
    };
}

/// Pick a day from a month grid. Returns the [`Date`].
///
/// ```
/// use rich_interact::datetime::Date;
/// use rich_interact::headless::{self, Script};
/// use rich_interact::{DatePicker, Outcome};
///
/// let picker = DatePicker::new("Due").value(Date::new(2026, 10, 17));
/// let script = Script::new().keys("right down enter");
/// let (outcome, record) = headless::run(picker, script, 40, 14);
/// assert_eq!(outcome.unwrap(), Outcome::Done(Date::new(2026, 10, 25)));
/// assert_eq!(record.last_frame(), "? Due › 2026-10-25");
/// ```
pub struct DatePicker(Chooser);

impl DatePicker {
    pub fn new(prompt: impl Into<String>) -> DatePicker {
        DatePicker(Chooser::new(prompt.into(), Mode::Date))
    }

    shared_builders!();
    date_builders!();

    /// Start on this day.
    pub fn value(mut self, date: Date) -> Self {
        self.0.value.date = date;
        self
    }

    /// Start on this day, and answer it without a terminal and to an empty
    /// line.
    pub fn default(mut self, date: Date) -> Self {
        self.0.value.date = date;
        self.0.default = Some(date.into());
        self
    }

    /// The earliest day that can be picked.
    pub fn min(mut self, date: Date) -> Self {
        self.0.min = Some(date.at(Time::MIDNIGHT));
        self
    }

    /// The latest day that can be picked.
    pub fn max(mut self, date: Date) -> Self {
        self.0.max = Some(date.at(Time::LAST));
        self
    }

    /// Refuse a day `check` returns an error for (a weekend, a holiday):
    /// Enter shows the error and waits.
    pub fn validate(
        mut self,
        check: impl Fn(Date) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.0.check = Some(Arc::new(move |at: DateTime| check(at.date)));
        self
    }

    /// The day selected so far.
    pub fn selected(&self) -> Date {
        self.0.value.date
    }
}

/// Pick a time of day, field by field. Returns the [`Time`].
///
/// ```
/// use rich_interact::datetime::Time;
/// use rich_interact::headless::{self, Script};
/// use rich_interact::{Outcome, TimePicker};
///
/// let picker = TimePicker::new("Start").value(Time::new(9, 0, 0));
/// let script = Script::new().text("1415").keys("enter");
/// let (outcome, _) = headless::run(picker, script, 40, 6);
/// assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(14, 15, 0)));
/// ```
pub struct TimePicker(Chooser);

impl TimePicker {
    pub fn new(prompt: impl Into<String>) -> TimePicker {
        TimePicker(Chooser::new(prompt.into(), Mode::Time))
    }

    shared_builders!();
    time_builders!();

    /// Start at this time.
    pub fn value(mut self, time: Time) -> Self {
        self.0.value.time = time;
        self
    }

    /// Start at this time, and answer it without a terminal and to an
    /// empty line.
    pub fn default(mut self, time: Time) -> Self {
        self.0.value.time = time;
        self.0.default = Some(self.0.value.date.at(time));
        self
    }

    /// The earliest time that can be picked.
    pub fn min(mut self, time: Time) -> Self {
        self.0.min = Some(self.0.value.date.at(time));
        self
    }

    /// The latest time that can be picked.
    pub fn max(mut self, time: Time) -> Self {
        self.0.max = Some(self.0.value.date.at(time));
        self
    }

    /// Refuse a time `check` returns an error for: Enter shows the error
    /// and waits.
    pub fn validate(
        mut self,
        check: impl Fn(Time) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.0.check = Some(Arc::new(move |at: DateTime| check(at.time)));
        self
    }

    /// The time selected so far.
    pub fn selected(&self) -> Time {
        self.0.value.time
    }
}

/// Pick a day and a time: the month grid, then the time under it (Tab
/// moves between them). Returns the [`DateTime`].
///
/// ```
/// use rich_interact::datetime::{Date, DateTime, Time};
/// use rich_interact::headless::{self, Script};
/// use rich_interact::{DateTimePicker, Outcome};
///
/// let start = DateTime::new(Date::new(2026, 10, 17), Time::new(9, 0, 0));
/// let picker = DateTimePicker::new("Meeting").value(start);
/// let script = Script::new().keys("right tab").text("1430").keys("enter");
/// let (outcome, record) = headless::run(picker, script, 40, 16);
/// let expected = DateTime::new(Date::new(2026, 10, 18), Time::new(14, 30, 0));
/// assert_eq!(outcome.unwrap(), Outcome::Done(expected));
/// assert_eq!(record.last_frame(), "? Meeting › 2026-10-18 14:30");
/// ```
pub struct DateTimePicker(Chooser);

impl DateTimePicker {
    pub fn new(prompt: impl Into<String>) -> DateTimePicker {
        DateTimePicker(Chooser::new(prompt.into(), Mode::DateTime))
    }

    shared_builders!();
    date_builders!();
    time_builders!();

    /// Start at this moment.
    pub fn value(mut self, at: DateTime) -> Self {
        self.0.value = at;
        self
    }

    /// Start at this moment, and answer it without a terminal and to an
    /// empty line.
    pub fn default(mut self, at: DateTime) -> Self {
        self.0.value = at;
        self.0.default = Some(at);
        self
    }

    /// The earliest moment that can be picked: days before its day are
    /// dimmed, and on its day an earlier time is refused.
    pub fn min(mut self, at: DateTime) -> Self {
        self.0.min = Some(at);
        self
    }

    /// The latest moment that can be picked.
    pub fn max(mut self, at: DateTime) -> Self {
        self.0.max = Some(at);
        self
    }

    /// Refuse a moment `check` returns an error for: Enter shows the error
    /// and waits.
    pub fn validate(
        mut self,
        check: impl Fn(DateTime) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.0.check = Some(Arc::new(check));
        self
    }

    /// The moment selected so far.
    pub fn selected(&self) -> DateTime {
        self.0.value
    }
}

/// The [`Component`] of a picker answering `$output`, made from the
/// shared state by `$from`.
macro_rules! picker_component {
    ($picker:ty, $output:ty, $from:expr) => {
        impl Component for $picker {
            type Output = $output;

            fn start(&mut self, _context: &Context<'_>) -> Flow<$output> {
                // A start outside the range moves into it.
                let day = self.0.clamp_day(self.0.value.date);
                self.0.value.date = day;
                Flow::Continue
            }

            fn handle(&mut self, event: &Event, _context: &Context<'_>) -> Flow<$output> {
                match self.0.event(event) {
                    Flow::Done(at) => Flow::Done($from(at)),
                    Flow::Cancel => Flow::Cancel,
                    Flow::Ignored => Flow::Ignored,
                    _ => Flow::Continue,
                }
            }

            fn render(&self, context: &Context<'_>) -> View {
                self.0.render(context)
            }

            fn mouse(&self) -> bool {
                self.0.mouse
            }

            fn keymap(&self) -> Keymap {
                self.0.keymap.clone()
            }

            fn default_value(&self) -> Option<$output> {
                self.0.default.map($from)
            }

            fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<$output>, NotInteractive> {
                Ok(self.0.ask(io)?.map($from))
            }
        }
    };
}

picker_component!(DatePicker, Date, |at: DateTime| at.date);
picker_component!(TimePicker, Time, |at: DateTime| at.time);
picker_component!(DateTimePicker, DateTime, |at: DateTime| at);
