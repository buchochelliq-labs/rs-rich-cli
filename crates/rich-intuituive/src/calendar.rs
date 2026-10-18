//! A month calendar with a selected day, within an optional range (#719).
//! The date types are rs-rich-interact's, shared with its date pickers.

use rich_interact::{Button, KeyCode, MouseKind};

pub use rich_interact::datetime::{Date, DateTime, Time};

use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// What a [`calendar_options`] marks and limits: today, and the first and
/// last days that can be selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CalendarOptions {
    pub today: Option<Date>,
    pub min: Option<Date>,
    pub max: Option<Date>,
}

impl CalendarOptions {
    pub fn new() -> CalendarOptions {
        CalendarOptions::default()
    }

    /// Mark `today` in the theme's `calendar.today` style.
    pub fn today(mut self, today: Date) -> CalendarOptions {
        self.today = Some(today);
        self
    }

    /// The earliest day that can be selected.
    pub fn min(mut self, min: Date) -> CalendarOptions {
        self.min = Some(min);
        self
    }

    /// The latest day that can be selected.
    pub fn max(mut self, max: Date) -> CalendarOptions {
        self.max = Some(max);
        self
    }

    /// Whether `date` is in the range.
    pub fn allows(&self, date: Date) -> bool {
        self.min.is_none_or(|min| date >= min) && self.max.is_none_or(|max| date <= max)
    }

    /// Whether no day is in the range: its min is after its max.
    pub(crate) fn empty(&self) -> bool {
        empty(self.min, self.max)
    }

    /// `date`, moved into the range.
    pub fn clamp(&self, date: Date) -> Date {
        let mut date = date;
        if let Some(min) = self.min {
            date = date.max(min);
        }
        if let Some(max) = self.max {
            date = date.min(max);
        }
        date
    }
}

/// Where a calendar's day is kept: a day, or the day of a moment whose
/// time a [`datetime_picker`](crate::widgets::datetime_picker) picks
/// beside it.
#[derive(Clone, Copy)]
pub(crate) enum DaySource {
    Day(Signal<Date>),
    Moment {
        value: Signal<DateTime>,
        min: Option<DateTime>,
        max: Option<DateTime>,
        /// Why the last change was moved into the range.
        message: Signal<Option<String>>,
    },
}

impl DaySource {
    /// The day, subscribing the widget that is drawing.
    fn get(&self) -> Date {
        match self {
            DaySource::Day(day) => day.get(),
            DaySource::Moment { value, .. } => value.get().date,
        }
    }

    fn get_untracked(&self) -> Date {
        match self {
            DaySource::Day(day) => day.get_untracked(),
            DaySource::Moment { value, .. } => value.get_untracked().date,
        }
    }

    fn set(&self, date: Date) {
        match *self {
            DaySource::Day(day) => day.set(date),
            DaySource::Moment {
                value,
                min,
                max,
                message,
            } => {
                let at = value.get_untracked();
                set_moment(value, message, min, max, DateTime { date, ..at });
            }
        }
    }
}

/// Whether a range from `min` to `max` holds nothing: `min` is after
/// `max`.
pub(crate) fn empty<T: PartialOrd>(min: Option<T>, max: Option<T>) -> bool {
    matches!((min, max), (Some(min), Some(max)) if min > max)
}

/// What a picker whose range is empty says, as rs-rich-interact's pickers
/// do ("the range is empty: min is after max"), on two lines of twenty
/// cells.
pub(crate) const EMPTY: [&str; 2] = ["✗ the range is empty", "min is after max"];

/// `at` moved into the range, and why it had to move.
pub(crate) fn into_range(
    min: Option<DateTime>,
    max: Option<DateTime>,
    at: DateTime,
) -> (DateTime, Option<String>) {
    match (min, max) {
        (Some(min), _) if at < min => (min, Some(format!("the earliest is {min}"))),
        (_, Some(max)) if at > max => (max, Some(format!("the latest is {max}"))),
        _ => (at, None),
    }
}

/// Set `value` to `at` moved into the range, and say in `message` when it
/// had to move. An empty range changes nothing.
pub(crate) fn set_moment(
    value: Signal<DateTime>,
    message: Signal<Option<String>>,
    min: Option<DateTime>,
    max: Option<DateTime>,
    at: DateTime,
) {
    if empty(min, max) {
        return;
    }
    let (at, why) = into_range(min, max, at);
    value.set(at);
    if message.get_untracked() != why {
        message.set(why);
    }
}

struct Calendar {
    source: DaySource,
    options: CalendarOptions,
}

/// The width and height of a calendar.
const WIDTH: u16 = 20;
const HEIGHT: u16 = 8;

/// A month of days, with one selected: the month and year, the weekdays
/// from Monday, then the weeks. ←/→ move the selection a day, ↑/↓ a week,
/// PgUp/PgDn a month, and Home/End to the first and last day of the month.
/// A click selects a day, and the wheel turns the month. The selected day
/// is highlighted in the theme's `selected` style while the calendar has
/// the focus, and `calendar.selected` otherwise. It is 20 x 8 cells.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{calendar, Date};
///
/// let app = App::new(|| {
///     let day = signal(Date::new(2026, 10, 7));
///     calendar(day).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 20, 8).unwrap();
/// assert_eq!(screen[0].trim_end(), "    October 2026");
/// assert_eq!(screen[1].trim_end(), "Mo Tu We Th Fr Sa Su");
/// assert_eq!(screen[2].trim_end(), "          1  2  3  4");
/// ```
pub fn calendar(selected: Signal<Date>) -> Node {
    calendar_options(selected, CalendarOptions::default())
}

/// A [`calendar`] that marks `today` in the theme's `calendar.today` style.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{calendar_with, Date};
///
/// let app = App::new(|| {
///     let day = signal(Date::new(2026, 10, 7));
///     calendar_with(day, Some(Date::new(2026, 10, 7))).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["right", "q"], 20, 8).unwrap();
/// assert_eq!(screen[3].trim_end(), " 5  6  7  8  9 10 11");
/// ```
pub fn calendar_with(selected: Signal<Date>, today: Option<Date>) -> Node {
    calendar_options(
        selected,
        CalendarOptions {
            today,
            ..CalendarOptions::default()
        },
    )
}

/// A [`calendar`] with a range (#719): days before
/// [`min`](CalendarOptions::min) or after [`max`](CalendarOptions::max)
/// are drawn in the theme's `calendar.disabled` style (dim unless set),
/// the keys and the wheel stop at the range's ends, and a click on a day
/// outside it does nothing. A range whose min is after its max is empty:
/// where the weeks go, the calendar says so, as rs-rich-interact's pickers
/// do ("the range is empty: min is after max"), in the theme's `error`
/// style, and nothing changes the day.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{calendar_options, CalendarOptions, Date};
///
/// let app = App::new(|| {
///     let day = signal(Date::new(2026, 10, 7));
///     let range = CalendarOptions::new()
///         .min(Date::new(2026, 10, 5))
///         .max(Date::new(2026, 10, 9));
///     column([
///         calendar_options(day, range).fixed(8),
///         text(move || format!("on {}", day.get())),
///     ])
///     .on_key("q", |cx| cx.quit())
/// });
/// // A week on is past the end: the selection stops on the last day.
/// let screen = app.render_with(&["down", "q"], 20, 9).unwrap();
/// assert_eq!(screen[8].trim_end(), "on 2026-10-09");
/// ```
pub fn calendar_options(selected: Signal<Date>, options: CalendarOptions) -> Node {
    widget(Calendar {
        source: DaySource::Day(selected),
        options,
    })
}

/// The calendar half of a date-and-time picker.
pub(crate) fn moment_calendar(source: DaySource, options: CalendarOptions) -> Node {
    widget(Calendar { source, options })
}

impl Calendar {
    fn change(&self, f: impl FnOnce(Date) -> Date) {
        let date = self.options.clamp(f(self.source.get_untracked()));
        self.source.set(date);
    }
}

impl Widget for Calendar {
    fn name(&self) -> &'static str {
        "calendar"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Grid
    }

    fn cursor(&self) -> Option<crate::screen::Rect> {
        if self.options.empty() {
            return None;
        }
        // The selected day's cell, as `draw` places it.
        let date = self.source.get_untracked();
        let at = date.first_of_month().weekday() + date.day - 1;
        let (x, y) = ((at % 7) as u16 * 3, 2 + (at / 7) as u16);
        Some(crate::screen::Rect::new(x, y, 2, 1))
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        let empty = self.options.empty();
        crate::a11y::AccessState {
            selected: !empty,
            invalid: empty,
            ..Default::default()
        }
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => WIDTH,
            Axis::Vertical => HEIGHT,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let selected = self.source.get();
        let title = format!("{} {}", selected.month_name(), selected.year);
        let x = WIDTH.saturating_sub(title.len() as u16) / 2;
        canvas.print(x, 0, &title, Some(&cx.style("calendar.title", "bold")));
        if self.options.empty() {
            // No day can be selected: why, where the weeks go.
            let style = cx.theme().error_style("error");
            for (row, line) in (2..).zip(EMPTY) {
                canvas.print(0, row, line, Some(&style));
            }
            return;
        }
        let header = cx.style("calendar.header", "dim");
        canvas.print(0, 1, "Mo Tu We Th Fr Sa Su", Some(&header));
        let highlight = if cx.focused() {
            cx.style("selected", "reverse")
        } else {
            cx.style("calendar.selected", "underline")
        };
        let today = cx.style("calendar.today", "bold");
        let disabled = cx.style("calendar.disabled", "dim");
        let offset = selected.first_of_month().weekday();
        for day in 1..=Date::days_in_month(selected.year, selected.month) {
            let at = offset + day - 1;
            let (x, y) = ((at % 7) as u16 * 3, 2 + (at / 7) as u16);
            let date = Date { day, ..selected };
            let style = if date == selected {
                let mut style = highlight.clone();
                if Some(date) == self.options.today {
                    style = today.combine(&style);
                }
                Some(style)
            } else if !self.options.allows(date) {
                Some(disabled.clone())
            } else if Some(date) == self.options.today {
                Some(today.clone())
            } else {
                None
            };
            canvas.print(x, y, &format!("{day:>2}"), style.as_ref());
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if self.options.empty() {
            // Nothing can be selected, so nothing changes the day.
            return Used::No;
        }
        match event {
            WidgetEvent::Key(key) => {
                if key.modifiers.ctrl || key.modifiers.alt {
                    return Used::No;
                }
                match key.code {
                    KeyCode::Left => self.change(|d| d.add_days(-1)),
                    KeyCode::Right => self.change(|d| d.add_days(1)),
                    KeyCode::Up => self.change(|d| d.add_days(-7)),
                    KeyCode::Down => self.change(|d| d.add_days(7)),
                    KeyCode::PageUp => self.change(|d| d.add_months(-1)),
                    KeyCode::PageDown => self.change(|d| d.add_months(1)),
                    KeyCode::Home => self.change(Date::first_of_month),
                    KeyCode::End => self.change(Date::last_of_month),
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(button) if mouse.row >= 2 && mouse.column < WIDTH => {
                    let selected = self.source.get_untracked();
                    let at = (mouse.row as u32 - 2) * 7 + mouse.column as u32 / 3;
                    let offset = selected.first_of_month().weekday();
                    let days = Date::days_in_month(selected.year, selected.month);
                    match (at + 1).checked_sub(offset) {
                        // A day outside the range is used up: nothing
                        // selects it, and no menu speaks for the old date.
                        Some(day)
                            if (1..=days).contains(&day)
                                && !self.options.allows(Date { day, ..selected }) =>
                        {
                            return Used::Yes
                        }
                        Some(day) if (1..=days).contains(&day) => {
                            self.source.set(Date { day, ..selected });
                            // Another button leaves the press to the node's
                            // own handler, as a table does.
                            if button != Button::Left {
                                return Used::No;
                            }
                        }
                        // Not a day: another button is used up, so no
                        // menu speaks for the old date.
                        _ if button != Button::Left => return Used::Yes,
                        _ => return Used::No,
                    }
                }
                MouseKind::ScrollUp => self.change(|d| d.add_months(-1)),
                MouseKind::ScrollDown => self.change(|d| d.add_months(1)),
                _ => return Used::No,
            },
            _ => return Used::No,
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}
