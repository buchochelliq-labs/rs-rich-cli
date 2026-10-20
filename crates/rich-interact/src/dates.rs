//! Dates as people type them: `tomorrow`, `eow`, `monday`, `3d`,
//! `2026-11-01`, read relative to a given now. Generic: a todo list's due
//! dates, a log viewer's `since`, a calendar's jump; the names are
//! Taskwarrior's, which most such tools share.
//!
//! ```
//! use rich_interact::dates;
//! use rich_interact::datetime::{Date, DateTime, Time};
//!
//! // Monday 19 October 2026, noon.
//! let now = DateTime::new(Date::new(2026, 10, 19), Time::new(12, 0, 0));
//! let at = |text| dates::parse(text, now).unwrap();
//! assert_eq!(at("tomorrow"), Date::new(2026, 10, 20).into());
//! assert_eq!(at("friday"), Date::new(2026, 10, 23).into());
//! assert_eq!(at("eom"), Date::new(2026, 10, 31).at(Time::LAST));
//! assert_eq!(at("3d"), DateTime::new(Date::new(2026, 10, 22), Time::new(12, 0, 0)));
//! assert_eq!(at("2026-11-01"), Date::new(2026, 11, 1).into());
//! assert!(dates::parse("someday soon", now).is_err());
//! ```
//!
//! The words, with `now` a [`DateTime`] (of whatever zone the caller
//! keeps; [`DateTime::now_local`] for the local one):
//!
//! | Word | Is |
//! |---|---|
//! | `now` | now |
//! | `today`, `sod` | today at 00:00 |
//! | `yesterday`, `tomorrow` | that day at 00:00 |
//! | `eod` | today at 23:59:59 |
//! | `monday` … `sunday`, `mon` … `sun` | the next such day (never today) at 00:00 |
//! | `january` … `december`, `jan` … `dec` | the 1st of the next such month (never this one) |
//! | `sow`, `som`, `soq`, `soy` | the start of the next week, month, quarter, year |
//! | `socw`, `socm`, `socq`, `socy` | the start of the current one |
//! | `eow`, `eom`, `eoq`, `eoy` (and `eocw` …) | the end of the current one, at 23:59:59 |
//! | `later`, `someday` | 9999-12-30 |
//! | `2026-11-01`, `2026-11-01 14:30`, `2026-11-01T14:30` | that day (and time) |
//! | `14:30`, `2pm` | today at that time |
//! | `3d`, `2w`, `1mo`, `1y`, `4h`, `30min`, `10s`, `-2d`, `+1w` | now plus that long |
//!
//! Weeks start on Monday; [`parse_with`] takes another first day.

use std::fmt;

use crate::datetime::{Date, DateTime, Time};

/// Every word [`parse`] knows by name, for completion: the day and month
/// names in full, then the rest.
pub const NAMES: &[&str] = &[
    "now",
    "today",
    "sod",
    "eod",
    "yesterday",
    "tomorrow",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "sow",
    "som",
    "soq",
    "soy",
    "socw",
    "socm",
    "socq",
    "socy",
    "eow",
    "eom",
    "eoq",
    "eoy",
    "eocw",
    "eocm",
    "eocq",
    "eocy",
    "later",
    "someday",
];

/// How [`parse_with`] reads: the day weeks start on, 0 for Monday to 6
/// for Sunday, as [`Date::weekday`] counts (Taskwarrior's default is
/// Sunday, 6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DateOptions {
    pub week_start: u32,
}

impl DateOptions {
    pub fn new() -> DateOptions {
        DateOptions::default()
    }

    /// Weeks start on `weekday` (0 Monday to 6 Sunday).
    #[must_use]
    pub fn week_start(mut self, weekday: u32) -> DateOptions {
        self.week_start = weekday % 7;
        self
    }
}

/// Why a text is not a date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateError {
    pub text: String,
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a date", self.text)
    }
}

impl std::error::Error for DateError {}

/// `text` as a moment, relative to `now`, with weeks from Monday.
pub fn parse(text: &str, now: DateTime) -> Result<DateTime, DateError> {
    parse_with(text, now, DateOptions::default())
}

/// [`parse`] as `options` say.
///
/// ```
/// use rich_interact::dates::{parse_with, DateOptions};
/// use rich_interact::datetime::{Date, DateTime, Time};
///
/// let now = DateTime::new(Date::new(2026, 10, 19), Time::new(12, 0, 0));
/// let sunday = DateOptions::new().week_start(6);
/// // The week of Monday 19 October started on Sunday the 18th.
/// assert_eq!(parse_with("eow", now, sunday).unwrap().date, Date::new(2026, 10, 24));
/// assert_eq!(parse_with("sow", now, sunday).unwrap().date, Date::new(2026, 10, 25));
/// ```
pub fn parse_with(text: &str, now: DateTime, options: DateOptions) -> Result<DateTime, DateError> {
    let bad = || DateError {
        text: text.to_string(),
    };
    let word = text.trim().to_ascii_lowercase();
    if word.is_empty() {
        return Err(bad());
    }
    let today = now.date;
    let start = |d: Date| DateTime::from(d);
    let end = |d: Date| d.at(Time::LAST);
    // Days since the week began, and the first days of the periods.
    let into_week = (today.weekday() + 7 - options.week_start) % 7;
    let week = today.add_days(-(into_week as i64));
    let month = today.first_of_month();
    let quarter = Date::new(today.year, (today.month - 1) / 3 * 3 + 1, 1);
    let year = Date::new(today.year, 1, 1);
    let at = match word.as_str() {
        "now" => now,
        "today" | "sod" => start(today),
        "yesterday" => start(today.add_days(-1)),
        "tomorrow" => start(today.add_days(1)),
        "eod" => end(today),
        "sow" => start(week.add_days(7)),
        "socw" => start(week),
        "eow" | "eocw" => end(week.add_days(6)),
        "som" => start(month.add_months(1)),
        "socm" => start(month),
        "eom" | "eocm" => end(today.last_of_month()),
        "soq" => start(quarter.add_months(3)),
        "socq" => start(quarter),
        "eoq" | "eocq" => end(quarter.add_months(2).last_of_month()),
        "soy" => start(Date::new(today.year + 1, 1, 1)),
        "socy" => start(year),
        "eoy" | "eocy" => end(Date::new(today.year, 12, 31)),
        "later" | "someday" => start(Date::new(9999, 12, 30)),
        _ => {
            return named_day(&word, today)
                .map(start)
                .or_else(|| other(&word, now))
                .ok_or_else(bad)
        }
    };
    Ok(at)
}

/// `text` as a day, relative to `today`: [`parse`]'s words, without the
/// time.
pub fn parse_date(text: &str, today: Date) -> Result<Date, DateError> {
    parse(text, today.into()).map(|at| at.date)
}

/// A weekday's or a month's name, in full or its first three letters:
/// the next such day, or the first of the next such month.
fn named_day(word: &str, today: Date) -> Option<Date> {
    let matches = |name: &str| word == name || (word.len() == 3 && name.starts_with(word));
    if let Some(day) = NAMES[6..13].iter().position(|name| matches(name)) {
        let ahead = (day as i64 - today.weekday() as i64 - 1).rem_euclid(7) + 1;
        return Some(today.add_days(ahead));
    }
    let month = NAMES[13..25].iter().position(|name| matches(name))? as u32 + 1;
    let year = today.year + (month <= today.month) as i32;
    Some(Date::new(year, month, 1))
}

/// A date, a time today, or a duration from now.
fn other(word: &str, now: DateTime) -> Option<DateTime> {
    if word.as_bytes().first().is_some_and(u8::is_ascii_digit) && word.contains('-') {
        return word.parse::<DateTime>().ok();
    }
    if let Ok(time) = word.parse::<Time>() {
        return Some(now.date.at(time));
    }
    let (sign, rest) = match word.as_bytes().first() {
        Some(b'-') => (-1, &word[1..]),
        Some(b'+') => (1, &word[1..]),
        _ => (1, word),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let n: i64 = rest[..digits].parse().ok()?;
    let n = sign * n;
    let unit = rest[digits..].trim_start();
    let seconds = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86_400,
        "w" | "wk" | "wks" | "week" | "weeks" => 7 * 86_400,
        "mo" | "mos" | "month" | "months" => {
            return Some(DateTime::new(
                now.date.add_months(i32::try_from(n).ok()?),
                now.time,
            ));
        }
        "q" | "qtr" | "quarter" | "quarters" => {
            return Some(DateTime::new(
                now.date.add_months(i32::try_from(n * 3).ok()?),
                now.time,
            ));
        }
        "y" | "yr" | "yrs" | "year" | "years" => {
            return Some(DateTime::new(
                now.date.add_months(i32::try_from(n * 12).ok()?),
                now.time,
            ));
        }
        _ => return None,
    };
    Some(now.add_seconds(n.checked_mul(seconds)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 19 October 2026, noon.
    fn now() -> DateTime {
        DateTime::new(Date::new(2026, 10, 19), Time::new(12, 0, 0))
    }

    fn day(text: &str) -> Date {
        parse(text, now()).unwrap().date
    }

    #[test]
    fn named_days() {
        assert_eq!(parse("now", now()).unwrap(), now());
        assert_eq!(
            parse("today", now()).unwrap(),
            Date::new(2026, 10, 19).into()
        );
        assert_eq!(day("yesterday"), Date::new(2026, 10, 18));
        assert_eq!(parse("eod", now()).unwrap().time, Time::LAST);
        // The next such day, never today.
        assert_eq!(day("monday"), Date::new(2026, 10, 26));
        assert_eq!(day("tue"), Date::new(2026, 10, 20));
        assert_eq!(day("Sunday"), Date::new(2026, 10, 25));
        assert_eq!(day("jan"), Date::new(2027, 1, 1));
        assert_eq!(day("october"), Date::new(2027, 10, 1));
        assert_eq!(day("december"), Date::new(2026, 12, 1));
    }

    #[test]
    fn starts_and_ends_of_periods() {
        assert_eq!(day("sow"), Date::new(2026, 10, 26));
        assert_eq!(day("socw"), Date::new(2026, 10, 19));
        assert_eq!(day("eow"), Date::new(2026, 10, 25));
        assert_eq!(day("som"), Date::new(2026, 11, 1));
        assert_eq!(day("socm"), Date::new(2026, 10, 1));
        assert_eq!(day("eom"), Date::new(2026, 10, 31));
        assert_eq!(day("soq"), Date::new(2027, 1, 1));
        assert_eq!(day("socq"), Date::new(2026, 10, 1));
        assert_eq!(day("eoq"), Date::new(2026, 12, 31));
        assert_eq!(day("soy"), Date::new(2027, 1, 1));
        assert_eq!(day("socy"), Date::new(2026, 1, 1));
        assert_eq!(
            parse("eoy", now()).unwrap(),
            Date::new(2026, 12, 31).at(Time::LAST)
        );
        assert_eq!(day("someday"), Date::new(9999, 12, 30));
    }

    #[test]
    fn dates_times_and_durations() {
        assert_eq!(day("2026-11-01"), Date::new(2026, 11, 1));
        assert_eq!(
            parse("2026-11-01T14:30", now()).unwrap(),
            DateTime::new(Date::new(2026, 11, 1), Time::new(14, 30, 0))
        );
        assert_eq!(
            parse("2pm", now()).unwrap(),
            Date::new(2026, 10, 19).at(Time::new(14, 0, 0))
        );
        assert_eq!(day("3d"), Date::new(2026, 10, 22));
        assert_eq!(day("2w"), Date::new(2026, 11, 2));
        assert_eq!(day("-2d"), Date::new(2026, 10, 17));
        assert_eq!(day("1mo"), Date::new(2026, 11, 19));
        assert_eq!(day("1y"), Date::new(2027, 10, 19));
        assert_eq!(
            parse("13h", now()).unwrap(),
            Date::new(2026, 10, 20).at(Time::new(1, 0, 0))
        );
        assert_eq!(parse("30min", now()).unwrap().time, Time::new(12, 30, 0));
        for bad in ["", "3", "3x", "nextweek", "2026-13-01", "d3", "--1d"] {
            assert!(parse(bad, now()).is_err(), "{bad:?}");
        }
        assert_eq!(
            parse_date("tomorrow", Date::new(2026, 12, 31)).unwrap(),
            Date::new(2027, 1, 1)
        );
        assert_eq!(
            parse("x", now()).unwrap_err().to_string(),
            "\"x\" is not a date"
        );
    }
}
