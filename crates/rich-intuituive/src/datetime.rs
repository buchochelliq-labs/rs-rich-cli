//! A time of day, picked field by field, and a date-and-time picker: a
//! calendar over a time (#719).

use rich_interact::{Button, KeyCode, MouseKind};

use crate::calendar::{moment_calendar, set_moment, CalendarOptions, DaySource};
use crate::calendar::{Date, DateTime, Time};
use crate::node::{column, label, row, text, Axis, Node};
use crate::reactive::{signal, Signal};
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// How a [`time_picker_with`] shows and limits the time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimeOptions {
    /// Pick the seconds too.
    pub seconds: bool,
    /// A 12-hour clock with AM and PM.
    pub twelve_hour: bool,
    pub min: Option<Time>,
    pub max: Option<Time>,
}

impl TimeOptions {
    pub fn new() -> TimeOptions {
        TimeOptions::default()
    }

    pub fn seconds(mut self) -> TimeOptions {
        self.seconds = true;
        self
    }

    pub fn twelve_hour(mut self) -> TimeOptions {
        self.twelve_hour = true;
        self
    }

    /// The earliest time that can be picked.
    pub fn min(mut self, min: Time) -> TimeOptions {
        self.min = Some(min);
        self
    }

    /// The latest time that can be picked.
    pub fn max(mut self, max: Time) -> TimeOptions {
        self.max = Some(max);
        self
    }
}

/// How a [`datetime_picker`] shows and limits the moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DateTimeOptions {
    /// The day the calendar marks as today.
    pub today: Option<Date>,
    /// The earliest moment: days before its day are dimmed, and on its day
    /// an earlier time moves up to it.
    pub min: Option<DateTime>,
    pub max: Option<DateTime>,
    pub seconds: bool,
    pub twelve_hour: bool,
}

impl DateTimeOptions {
    pub fn new() -> DateTimeOptions {
        DateTimeOptions::default()
    }

    pub fn today(mut self, today: Date) -> DateTimeOptions {
        self.today = Some(today);
        self
    }

    pub fn min(mut self, min: DateTime) -> DateTimeOptions {
        self.min = Some(min);
        self
    }

    pub fn max(mut self, max: DateTime) -> DateTimeOptions {
        self.max = Some(max);
        self
    }

    pub fn seconds(mut self) -> DateTimeOptions {
        self.seconds = true;
        self
    }

    pub fn twelve_hour(mut self) -> DateTimeOptions {
        self.twelve_hour = true;
        self
    }
}

/// A time field's parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Hour,
    Minute,
    Second,
    Half,
}

/// Where a time field's time is kept.
#[derive(Clone, Copy)]
enum TimeSource {
    Time(Signal<Time>),
    Moment {
        value: Signal<DateTime>,
        min: Option<DateTime>,
        max: Option<DateTime>,
        message: Signal<Option<String>>,
    },
}

struct TimeField {
    source: TimeSource,
    options: TimeOptions,
    /// The focused part, by index into [`parts`](Self::parts).
    part: usize,
    /// Digits typed into the focused part so far.
    typed: String,
}

impl TimeField {
    fn parts(&self) -> Vec<Part> {
        let mut parts = vec![Part::Hour, Part::Minute];
        if self.options.seconds {
            parts.push(Part::Second);
        }
        if self.options.twelve_hour {
            parts.push(Part::Half);
        }
        parts
    }

    /// Each part's column and width.
    fn places(&self) -> Vec<(Part, u16, u16)> {
        let mut x = 0;
        self.parts()
            .into_iter()
            .map(|part| {
                let place = (part, x, 2);
                x += 3;
                place
            })
            .collect()
    }

    fn get(&self) -> Time {
        match self.source {
            TimeSource::Time(time) => time.get(),
            TimeSource::Moment { value, .. } => value.get().time,
        }
    }

    fn get_untracked(&self) -> Time {
        match self.source {
            TimeSource::Time(time) => time.get_untracked(),
            TimeSource::Moment { value, .. } => value.get_untracked().time,
        }
    }

    /// Set the time, moved into the range.
    fn set(&self, time: Time) {
        match self.source {
            TimeSource::Time(signal) => {
                let mut time = time;
                if let Some(min) = self.options.min {
                    time = time.max(min);
                }
                if let Some(max) = self.options.max {
                    time = time.min(max);
                }
                signal.set(time);
            }
            TimeSource::Moment {
                value,
                min,
                max,
                message,
            } => {
                let at = value.get_untracked();
                set_moment(value, message, min, max, DateTime { time, ..at });
            }
        }
    }

    fn focused_part(&self) -> Part {
        let parts = self.parts();
        parts[self.part.min(parts.len() - 1)]
    }

    /// Change the focused part by `delta`, wrapping round.
    fn step(&mut self, delta: i64) {
        let time = self.get_untracked();
        let wrap = |value: u32, modulus: i64| (value as i64 + delta).rem_euclid(modulus) as u32;
        self.set(match self.focused_part() {
            Part::Hour => Time {
                hour: wrap(time.hour, 24),
                ..time
            },
            Part::Minute => Time {
                minute: wrap(time.minute, 60),
                ..time
            },
            Part::Second => Time {
                second: wrap(time.second, 60),
                ..time
            },
            Part::Half => Time {
                hour: (time.hour + 12) % 24,
                ..time
            },
        });
    }

    /// A digit typed into the focused part: two fill it, or one that no
    /// second digit could follow, and the focus moves on.
    fn digit(&mut self, digit: char) {
        let part = self.focused_part();
        if part == Part::Half {
            return;
        }
        self.typed.push(digit);
        let number: u32 = self.typed.parse().unwrap_or(0);
        let twelve = self.options.twelve_hour;
        let full = self.typed.len() == 2
            || match part {
                Part::Hour if twelve => number >= 2,
                Part::Hour => number >= 3,
                _ => number >= 6,
            };
        let time = self.get_untracked();
        self.set(match part {
            Part::Hour if twelve => {
                let pm = time.hour >= 12;
                let hour = number.min(12);
                let hour = if full { hour.max(1) } else { hour } % 12;
                Time {
                    hour: hour + if pm { 12 } else { 0 },
                    ..time
                }
            }
            Part::Hour => Time {
                hour: number.min(23),
                ..time
            },
            Part::Minute => Time {
                minute: number.min(59),
                ..time
            },
            _ => Time {
                second: number.min(59),
                ..time
            },
        });
        if full {
            self.typed.clear();
            self.part = (self.part + 1).min(self.parts().len() - 1);
        }
    }
}

impl Widget for TimeField {
    fn name(&self) -> &'static str {
        "time_picker"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::TextBox
    }

    fn cursor(&self) -> Option<crate::screen::Rect> {
        let (_, x, width) = self.places()[self.part.min(self.parts().len() - 1)];
        Some(crate::screen::Rect::new(x, 0, width, 1))
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => self.parts().len() as u16 * 3 - 1,
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let time = self.get();
        let focused = cx.focused();
        let highlight = cx.style("selected", "reverse");
        let (hour12, pm) = time.twelve_hour();
        let current = self.focused_part();
        for (part, x, _) in self.places() {
            if x > 0 {
                let separator = if part == Part::Half { " " } else { ":" };
                canvas.print(x - 1, 0, separator, None);
            }
            let shown = match part {
                Part::Hour if self.options.twelve_hour => format!("{hour12:02}"),
                Part::Hour => format!("{:02}", time.hour),
                Part::Minute => format!("{:02}", time.minute),
                Part::Second => format!("{:02}", time.second),
                Part::Half => (if pm { "PM" } else { "AM" }).to_string(),
            };
            let on = focused && part == current;
            let shown = if on && !self.typed.is_empty() {
                format!("{:_<2}", self.typed)
            } else {
                shown
            };
            canvas.print(x, 0, &shown, on.then_some(&highlight));
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let count = self.parts().len();
        match event {
            WidgetEvent::Key(key) => {
                if key.modifiers.ctrl || key.modifiers.alt {
                    return Used::No;
                }
                match key.code {
                    KeyCode::Char(c @ '0'..='9') => self.digit(c),
                    KeyCode::Char(c @ ('a' | 'A' | 'p' | 'P')) if self.options.twelve_hour => {
                        let time = self.get_untracked();
                        let pm = c.eq_ignore_ascii_case(&'p');
                        self.set(Time {
                            hour: time.hour % 12 + if pm { 12 } else { 0 },
                            ..time
                        });
                    }
                    // At the ends the arrows leave the key to the app.
                    KeyCode::Left if self.part > 0 => {
                        self.part -= 1;
                        self.typed.clear();
                    }
                    KeyCode::Right if self.part + 1 < count => {
                        self.part += 1;
                        self.typed.clear();
                    }
                    KeyCode::Up => {
                        self.typed.clear();
                        self.step(1);
                    }
                    KeyCode::Down => {
                        self.typed.clear();
                        self.step(-1);
                    }
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(Button::Left) => {
                    let at = self
                        .places()
                        .iter()
                        .position(|&(_, x, width)| (x..x + width).contains(&mouse.column));
                    let Some(at) = at else {
                        return Used::No;
                    };
                    self.part = at;
                    self.typed.clear();
                }
                MouseKind::ScrollUp => self.step(1),
                MouseKind::ScrollDown => self.step(-1),
                _ => return Used::No,
            },
            WidgetEvent::Focus(false) => {
                self.typed.clear();
                return Used::No;
            }
            _ => return Used::No,
        }
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// A time of day, picked field by field (#719): `14:30`. ←/→ move between
/// the hour and the minute, ↑/↓ change the focused one (wrapping round),
/// and digits type it: two fill a field, and the focus moves on. The
/// focused field is in the theme's `selected` style while the picker has
/// the focus. A click focuses a field; the wheel turns it. It is one row,
/// five cells wide.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{time_picker, Time};
///
/// let app = App::new(|| {
///     let at = signal(Time::new(9, 0, 0));
///     column([
///         time_picker(at).fixed(1),
///         text(move || format!("at {}", at.get())),
///     ])
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["1", "4", "3", "0", "q"], 20, 2).unwrap();
/// assert_eq!(screen[0].trim_end(), "14:30");
/// assert_eq!(screen[1].trim_end(), "at 14:30");
/// ```
pub fn time_picker(selected: Signal<Time>) -> Node {
    time_picker_with(selected, TimeOptions::default())
}

/// A [`time_picker`] with seconds, a 12-hour clock (`a` and `p` set AM and
/// PM) or a range, as `options` asks. A change past the range stops at its
/// end.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{time_picker_with, Time, TimeOptions};
///
/// let app = App::new(|| {
///     let at = signal(Time::new(17, 0, 0));
///     let options = TimeOptions::new().twelve_hour().max(Time::new(17, 30, 0));
///     time_picker_with(at, options).on_key("q", |cx| cx.quit())
/// });
/// // An hour later is past 17:30: it stops there.
/// let screen = app.render_with(&["up", "q"], 20, 1).unwrap();
/// assert_eq!(screen[0].trim_end(), "05:30 PM");
/// ```
pub fn time_picker_with(selected: Signal<Time>, options: TimeOptions) -> Node {
    widget(TimeField {
        source: TimeSource::Time(selected),
        options,
        part: 0,
        typed: String::new(),
    })
}

/// A date and a time (#719): a [`calendar`](crate::widgets::calendar) over
/// a time picker, both kept in `selected`; Tab moves between them. Under
/// them two rows show the range, or, when a change went past it and
/// stopped at its end, why. Days outside the range are dimmed and cannot
/// be selected, and on the first and last days the time stops at the
/// range's ends. Eleven rows, twenty cells wide.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{datetime_picker, Date, DateTime, DateTimeOptions, Time};
///
/// let app = App::new(|| {
///     let at = signal(DateTime::new(Date::new(2026, 10, 18), Time::new(9, 0, 0)));
///     let earliest = DateTime::new(Date::new(2026, 10, 17), Time::new(12, 0, 0));
///     let options = DateTimeOptions::new().min(earliest);
///     column([
///         datetime_picker(at, options).fixed(11),
///         text(move || format!("at {}", at.get())),
///     ])
///     .on_key("q", |cx| cx.quit())
/// });
/// // The 17th at 09:00 is too early: the time moves up to 12:00.
/// let screen = app.render_with(&["left", "q"], 20, 12).unwrap();
/// assert_eq!(screen[9].trim_end(), "✗ the earliest is");
/// assert_eq!(screen[10].trim_end(), "2026-10-17 12:00");
/// assert_eq!(screen[11].trim_end(), "at 2026-10-17 12:00");
/// // A day later, then to the time and an hour on.
/// let app = App::new(|| {
///     let at = signal(DateTime::new(Date::new(2026, 10, 17), Time::new(9, 0, 0)));
///     column([
///         datetime_picker(at, DateTimeOptions::new()).fixed(11),
///         text(move || format!("at {}", at.get())),
///     ])
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["right", "tab", "up", "q"], 20, 12).unwrap();
/// assert_eq!(screen[8].trim_end(), "Time 10:00");
/// assert_eq!(screen[11].trim_end(), "at 2026-10-18 10:00");
/// ```
pub fn datetime_picker(selected: Signal<DateTime>, options: DateTimeOptions) -> Node {
    let message = signal(None::<String>);
    let (min, max) = (options.min, options.max);
    let calendar = moment_calendar(
        DaySource::Moment {
            value: selected,
            min,
            max,
            message,
        },
        CalendarOptions {
            today: options.today,
            min: min.map(|at| at.date),
            max: max.map(|at| at.date),
        },
    );
    let time = widget(TimeField {
        source: TimeSource::Moment {
            value: selected,
            min,
            max,
            message,
        },
        options: TimeOptions {
            seconds: options.seconds,
            twelve_hour: options.twelve_hour,
            min: None,
            max: None,
        },
        part: 0,
        typed: String::new(),
    });
    let shown = move |at: DateTime| at.format(options.seconds, options.twelve_hour);
    let range = match (min, max) {
        (Some(min), Some(max)) => format!("[dim]{} to\n{}[/]", shown(min), shown(max)),
        (Some(min), None) => format!("[dim]earliest\n{}[/]", shown(min)),
        (None, Some(max)) => format!("[dim]latest\n{}[/]", shown(max)),
        (None, None) => String::new(),
    };
    let note = text(move || match message.get() {
        Some(why) => format!("[bold]✗ {why}[/]"),
        None => range.clone(),
    });
    column([
        calendar.fixed(8),
        row([label("Time").fixed(5), time]).fixed(1),
        note.fixed(2),
    ])
    .name("datetime_picker")
}
