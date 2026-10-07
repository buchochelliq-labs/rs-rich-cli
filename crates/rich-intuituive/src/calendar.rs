//! A month calendar with a selected day, and the date arithmetic it needs.

use std::fmt;

use rich_interact::{Button, KeyCode, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// A day of the proleptic Gregorian calendar. Months and days count from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

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

impl Date {
    pub const fn new(year: i32, month: u32, day: u32) -> Date {
        Date { year, month, day }
    }

    /// Whether `year` has a 29th of February.
    pub const fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    /// The days in `month` of `year`.
    pub const fn days_in_month(year: i32, month: u32) -> u32 {
        match month {
            2 if Date::is_leap_year(year) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }

    /// Days since 1970-01-01 (negative before it).
    pub fn days(self) -> i64 {
        // Howard Hinnant's days_from_civil.
        let year = self.year as i64 - (self.month <= 2) as i64;
        let era = year.div_euclid(400);
        let yoe = year - era * 400;
        let month = self.month as i64;
        let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date `days` after 1970-01-01.
    pub fn from_days(days: i64) -> Date {
        // Howard Hinnant's civil_from_days.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + (month <= 2) as i64) as i32;
        Date { year, month, day }
    }

    /// The day of the week: 0 for Monday to 6 for Sunday.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday.
        (self.days() + 3).rem_euclid(7) as u32
    }

    /// The date `days` later (earlier if negative).
    pub fn add_days(self, days: i64) -> Date {
        Date::from_days(self.days() + days)
    }

    /// The same day `months` later (earlier if negative), or the last day
    /// of that month if it is shorter.
    pub fn add_months(self, months: i32) -> Date {
        let index = self.year as i64 * 12 + self.month as i64 - 1 + months as i64;
        let year = index.div_euclid(12) as i32;
        let month = index.rem_euclid(12) as u32 + 1;
        let day = self.day.min(Date::days_in_month(year, month));
        Date { year, month, day }
    }

    /// The first day of its month.
    pub fn first_of_month(self) -> Date {
        Date { day: 1, ..self }
    }

    /// The last day of its month.
    pub fn last_of_month(self) -> Date {
        Date {
            day: Date::days_in_month(self.year, self.month),
            ..self
        }
    }

    /// The month's name, in English.
    pub fn month_name(self) -> &'static str {
        MONTHS[(self.month.clamp(1, 12) - 1) as usize]
    }
}

impl fmt::Display for Date {
    /// `2026-10-07`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

struct Calendar {
    selected: Signal<Date>,
    today: Option<Date>,
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
    calendar_with(selected, None)
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
    widget(Calendar { selected, today })
}

impl Calendar {
    fn change(&self, f: impl FnOnce(Date) -> Date) {
        let date = f(self.selected.get_untracked());
        self.selected.set(date);
    }
}

impl Widget for Calendar {
    fn name(&self) -> &'static str {
        "calendar"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => WIDTH,
            Axis::Vertical => HEIGHT,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let selected = self.selected.get();
        let title = format!("{} {}", selected.month_name(), selected.year);
        let x = WIDTH.saturating_sub(title.len() as u16) / 2;
        canvas.print(x, 0, &title, Some(&cx.style("calendar.title", "bold")));
        let header = cx.style("calendar.header", "dim");
        canvas.print(0, 1, "Mo Tu We Th Fr Sa Su", Some(&header));
        let highlight = if cx.focused() {
            cx.style("selected", "reverse")
        } else {
            cx.style("calendar.selected", "underline")
        };
        let today = cx.style("calendar.today", "bold");
        let offset = selected.first_of_month().weekday();
        for day in 1..=Date::days_in_month(selected.year, selected.month) {
            let at = offset + day - 1;
            let (x, y) = ((at % 7) as u16 * 3, 2 + (at / 7) as u16);
            let date = Date { day, ..selected };
            let style = if date == selected {
                let mut style = highlight.clone();
                if Some(date) == self.today {
                    style = today.combine(&style);
                }
                Some(style)
            } else if Some(date) == self.today {
                Some(today.clone())
            } else {
                None
            };
            canvas.print(x, y, &format!("{day:>2}"), style.as_ref());
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
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
                    let selected = self.selected.get_untracked();
                    let at = (mouse.row as u32 - 2) * 7 + mouse.column as u32 / 3;
                    let offset = selected.first_of_month().weekday();
                    let days = Date::days_in_month(selected.year, selected.month);
                    match (at + 1).checked_sub(offset) {
                        Some(day) if (1..=days).contains(&day) => {
                            self.selected.set(Date { day, ..selected });
                            // Another button leaves the press to the node's
                            // own handler, as a table does.
                            if button != Button::Left {
                                return Used::No;
                            }
                        }
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

#[cfg(test)]
mod tests {
    use super::Date;

    #[test]
    fn leap_years_and_month_lengths() {
        assert!(Date::is_leap_year(2000));
        assert!(Date::is_leap_year(2024));
        assert!(!Date::is_leap_year(1900));
        assert!(!Date::is_leap_year(2026));
        assert_eq!(Date::days_in_month(2024, 2), 29);
        assert_eq!(Date::days_in_month(2026, 2), 28);
        assert_eq!(Date::days_in_month(2026, 4), 30);
        assert_eq!(Date::days_in_month(2026, 12), 31);
    }

    #[test]
    fn weekdays_of_known_dates() {
        // Monday is 0.
        assert_eq!(Date::new(2026, 10, 7).weekday(), 2);
        assert_eq!(Date::new(2000, 2, 29).weekday(), 1);
        assert_eq!(Date::new(1970, 1, 1).weekday(), 3);
        assert_eq!(Date::new(1969, 12, 31).weekday(), 2);
        assert_eq!(Date::new(2026, 10, 1).weekday(), 3);
    }

    #[test]
    fn days_round_trip() {
        assert_eq!(Date::new(1970, 1, 1).days(), 0);
        assert_eq!(Date::new(2000, 3, 1).days(), 11_017);
        assert_eq!(Date::new(1969, 12, 31).days(), -1);
        for days in -800_000..800_000 {
            if days % 997 == 0 {
                assert_eq!(Date::from_days(days).days(), days);
            }
        }
        assert_eq!(Date::new(2000, 2, 28).add_days(1), Date::new(2000, 2, 29));
        assert_eq!(Date::new(2026, 12, 31).add_days(1), Date::new(2027, 1, 1));
        assert_eq!(Date::new(2026, 3, 1).add_days(-1), Date::new(2026, 2, 28));
    }

    #[test]
    fn months_keep_the_day_where_they_can() {
        assert_eq!(Date::new(2026, 1, 31).add_months(1), Date::new(2026, 2, 28));
        assert_eq!(Date::new(2024, 1, 31).add_months(1), Date::new(2024, 2, 29));
        assert_eq!(
            Date::new(2026, 1, 15).add_months(-1),
            Date::new(2025, 12, 15)
        );
        assert_eq!(
            Date::new(2026, 10, 7).add_months(14),
            Date::new(2027, 12, 7)
        );
        assert_eq!(Date::new(2026, 10, 7).to_string(), "2026-10-07");
    }
}
