//! Dates and times of day (#719), without a time zone: what the date and
//! time pickers ([`DatePicker`](crate::DatePicker),
//! [`TimePicker`](crate::TimePicker),
//! [`DateTimePicker`](crate::DateTimePicker)) and intuiTUIve's calendar
//! choose. Hand-rolled, with no dependency: a [`Date`] is a day of the
//! proleptic Gregorian calendar, a [`Time`] a time of day to the second,
//! and a [`DateTime`] the two together. Each parses the ISO 8601 form it
//! prints (`2026-10-17`, `14:30`, `2026-10-17 14:30`), and a time also
//! parses a 12-hour clock (`2:30 pm`).
//!
//! ```
//! use rich_interact::datetime::{Date, DateTime, Time};
//!
//! let at: DateTime = "2026-10-17 14:30".parse().unwrap();
//! assert_eq!(at.date, Date::new(2026, 10, 17));
//! assert_eq!(at.time, Time::new(14, 30, 0));
//! assert_eq!(at.to_string(), "2026-10-17 14:30");
//! assert_eq!("2:30 pm".parse::<Time>().unwrap(), Time::new(14, 30, 0));
//! ```
//!
//! A [`Zone`] turns a moment (seconds since 1970 UTC) into a local
//! [`DateTime`] and back, following summer time: [`Zone::Local`] is the
//! platform's zone, and [`DateTime::now_local`] and [`Date::today_local`]
//! read the clock in it. Named dates (`tomorrow`, `eow`, `3d`) are
//! [`dates`](crate::dates)'.

use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

pub use crate::zone::{Rules, Zone};

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

/// Seconds since 1970-01-01 00:00 UTC, now.
fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

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

    /// Whether the month is 1 to 12 and the day is in it.
    pub const fn is_valid(self) -> bool {
        self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= Date::days_in_month(self.year, self.month)
    }

    /// Today in UTC. There is no time zone here: pass the local day to a
    /// picker when the difference matters.
    pub fn today_utc() -> Date {
        Date::from_days(unix_now().div_euclid(86_400))
    }

    /// Today in the local zone, summer time included.
    pub fn today_local() -> Date {
        DateTime::now_local().date
    }

    /// Today in `zone`.
    pub fn today_in(zone: &Zone) -> Date {
        DateTime::now_in(zone).date
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

    /// The day at `time`.
    pub const fn at(self, time: Time) -> DateTime {
        DateTime { date: self, time }
    }
}

impl fmt::Display for Date {
    /// `2026-10-07`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for Date {
    type Err = String;

    /// `YYYY-MM-DD` (the year four digits, 0001 to 9999; the month and day
    /// may have one), or `today` (in UTC).
    fn from_str(text: &str) -> Result<Date, String> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("today") {
            return Ok(Date::today_utc());
        }
        let bad = || format!("{text:?} is not a date (YYYY-MM-DD)");
        let mut parts = text.split('-');
        let (Some(year), Some(month), Some(day), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        let number = |part: &str, most: usize| {
            (!part.is_empty() && part.len() <= most && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse::<u32>().ok())
                .flatten()
        };
        // Four digits make the year, as the date is printed: `26-10-17` is
        // not the year 26.
        let year = (year.len() == 4).then(|| number(year, 4)).flatten();
        let (Some(year @ 1..), Some(month), Some(day)) = (year, number(month, 2), number(day, 2))
        else {
            return Err(bad());
        };
        let date = Date::new(year as i32, month, day);
        if !date.is_valid() {
            return Err(format!("{text:?} is not a day of the calendar"));
        }
        Ok(date)
    }
}

/// A time of day, to the second: 00:00:00 to 23:59:59.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Time {
    /// Midnight, the first time of a day.
    pub const MIDNIGHT: Time = Time::new(0, 0, 0);
    /// 23:59:59, the last time of a day.
    pub const LAST: Time = Time::new(23, 59, 59);

    pub const fn new(hour: u32, minute: u32, second: u32) -> Time {
        Time {
            hour,
            minute,
            second,
        }
    }

    /// Whether the hour is below 24 and the minute and second below 60.
    pub const fn is_valid(self) -> bool {
        self.hour < 24 && self.minute < 60 && self.second < 60
    }

    /// Seconds since midnight.
    pub const fn seconds(self) -> u32 {
        self.hour * 3600 + self.minute * 60 + self.second
    }

    /// The time `seconds` after midnight, wrapping round the day.
    pub const fn from_seconds(seconds: i64) -> Time {
        let seconds = seconds.rem_euclid(86_400) as u32;
        Time::new(seconds / 3600, seconds / 60 % 60, seconds % 60)
    }

    /// Now, in UTC.
    pub fn now_utc() -> Time {
        Time::from_seconds(unix_now())
    }

    /// Now, in the local zone.
    pub fn now_local() -> Time {
        DateTime::now_local().time
    }

    /// The hour on a 12-hour clock (12, then 1 to 11), and whether it is
    /// after noon.
    pub const fn twelve_hour(self) -> (u32, bool) {
        let pm = self.hour >= 12;
        let hour = match self.hour % 12 {
            0 => 12,
            hour => hour,
        };
        (hour, pm)
    }

    /// The time written with or without its seconds, on a 24-hour clock
    /// (`14:30`, `14:30:05`) or a 12-hour one (`2:30 PM`, `2:30:05 PM`).
    pub fn format(self, seconds: bool, twelve_hour: bool) -> String {
        let tail = if seconds {
            format!(":{:02}", self.second)
        } else {
            String::new()
        };
        if twelve_hour {
            let (hour, pm) = self.twelve_hour();
            let half = if pm { "PM" } else { "AM" };
            format!("{hour}:{:02}{tail} {half}", self.minute)
        } else {
            format!("{:02}:{:02}{tail}", self.hour, self.minute)
        }
    }
}

impl fmt::Display for Time {
    /// `14:30`, or `14:30:05` when the seconds are not 0.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format(self.second != 0, false))
    }
}

impl FromStr for Time {
    type Err = String;

    /// `HH:MM` or `HH:MM:SS` on a 24-hour clock, or with `am` or `pm` after
    /// it on a 12-hour one (`2:30 pm`, `2pm`, `12:05AM`), or `now` (in UTC).
    fn from_str(text: &str) -> Result<Time, String> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("now") {
            return Ok(Time::now_utc());
        }
        let bad = || format!("{text:?} is not a time (HH:MM)");
        let lower = text.to_ascii_lowercase();
        let (clock, half) = if let Some(rest) = lower.strip_suffix("am") {
            (rest.trim_end().trim_end_matches('.'), Some(false))
        } else if let Some(rest) = lower.strip_suffix("pm") {
            (rest.trim_end().trim_end_matches('.'), Some(true))
        } else {
            (lower.as_str(), None)
        };
        let parts: Vec<&str> = clock.split(':').collect();
        // `2pm` has no minutes; a 24-hour time needs them.
        if parts.len() > 3 || (parts.len() == 1 && half.is_none()) {
            return Err(bad());
        }
        let mut numbers = [0u32; 3];
        for (slot, part) in numbers.iter_mut().zip(&parts) {
            if part.is_empty() || part.len() > 2 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            *slot = part.parse().map_err(|_| bad())?;
        }
        let [mut hour, minute, second] = numbers;
        if let Some(pm) = half {
            if !(1..=12).contains(&hour) {
                return Err(format!("{text:?}: a 12-hour clock goes from 1 to 12"));
            }
            hour = hour % 12 + if pm { 12 } else { 0 };
        }
        let time = Time::new(hour, minute, second);
        if !time.is_valid() {
            return Err(format!("{text:?} is not a time of day"));
        }
        Ok(time)
    }
}

/// A day and a time of day, without a time zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub date: Date,
    pub time: Time,
}

impl DateTime {
    pub const fn new(date: Date, time: Time) -> DateTime {
        DateTime { date, time }
    }

    /// Now, in UTC.
    pub fn now_utc() -> DateTime {
        let now = unix_now();
        DateTime::new(
            Date::from_days(now.div_euclid(86_400)),
            Time::from_seconds(now),
        )
    }

    /// Now, in the local zone, summer time included.
    ///
    /// ```
    /// use rich_interact::datetime::DateTime;
    ///
    /// assert!(DateTime::now_local().is_valid());
    /// ```
    pub fn now_local() -> DateTime {
        DateTime::now_in(&Zone::Local)
    }

    /// Now, on `zone`'s clocks.
    pub fn now_in(zone: &Zone) -> DateTime {
        DateTime::from_unix(unix_now(), zone)
    }

    /// The moment `unix` (seconds since 1970-01-01 00:00 UTC) on `zone`'s
    /// clocks.
    ///
    /// ```
    /// use rich_interact::datetime::{Date, DateTime, Time, Zone};
    ///
    /// let at = DateTime::from_unix(1_792_404_000, &Zone::Fixed(7200));
    /// assert_eq!(at, DateTime::new(Date::new(2026, 10, 19), Time::new(12, 0, 0)));
    /// assert_eq!(at.to_unix(&Zone::Fixed(7200)), 1_792_404_000);
    /// ```
    pub fn from_unix(unix: i64, zone: &Zone) -> DateTime {
        let local = unix + zone.offset_at(unix) as i64;
        DateTime::new(
            Date::from_days(local.div_euclid(86_400)),
            Time::from_seconds(local),
        )
    }

    /// The moment this is on `zone`'s clocks, in seconds since 1970 UTC.
    /// A time the clocks pass twice (when summer time ends) is the first
    /// of the two; one they skip (when it starts) is read on the clock of
    /// before the change, so 02:30 in a gap from 02:00 to 03:00 is 03:30.
    pub fn to_unix(self, zone: &Zone) -> i64 {
        let wall = self.date.days() * 86_400 + self.time.seconds() as i64;
        // The offsets around: transitions are days apart.
        let around = [
            zone.offset_at(wall - 172_800),
            zone.offset_at(wall),
            zone.offset_at(wall + 172_800),
        ];
        around
            .iter()
            .map(|&offset| wall - offset as i64)
            .filter(|&unix| (unix + zone.offset_at(unix) as i64) == wall)
            .min()
            .unwrap_or(wall - around[0] as i64)
    }

    /// The moment `seconds` later (earlier if negative), on a clock that
    /// does not change (as UTC's): add to [`to_unix`](Self::to_unix) for
    /// a zone's.
    pub fn add_seconds(self, seconds: i64) -> DateTime {
        DateTime::from_unix(self.to_unix(&Zone::Utc) + seconds, &Zone::Utc)
    }

    /// Whether both halves are valid.
    pub const fn is_valid(self) -> bool {
        self.date.is_valid() && self.time.is_valid()
    }

    /// Written as `2026-10-17 14:30`, with the time as
    /// [`Time::format`] writes it.
    pub fn format(self, seconds: bool, twelve_hour: bool) -> String {
        format!("{} {}", self.date, self.time.format(seconds, twelve_hour))
    }
}

impl fmt::Display for DateTime {
    /// `2026-10-17 14:30`, or `2026-10-17 14:30:05` when the seconds are
    /// not 0.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.date, self.time)
    }
}

impl FromStr for DateTime {
    type Err = String;

    /// A [`Date`], then a space or `T`, then a [`Time`]
    /// (`2026-10-17 14:30`, `2026-10-17T14:30:05`, `2026-10-17 2:30 pm`);
    /// a date alone is at midnight; `now` is now (in UTC).
    fn from_str(text: &str) -> Result<DateTime, String> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("now") {
            return Ok(DateTime::now_utc());
        }
        // A space, or ISO 8601's `T` after the day's last digit, whichever
        // comes first: the time may hold a space (`2026-10-17T2:30 pm`).
        let split = text
            .char_indices()
            .skip(1)
            .find(|&(at, c)| {
                c == ' ' || (matches!(c, 'T' | 't') && text.as_bytes()[at - 1].is_ascii_digit())
            })
            .map(|(at, _)| (&text[..at], &text[at + 1..]));
        match split {
            Some((date, time)) => Ok(DateTime::new(date.parse()?, time.parse()?)),
            None => Ok(DateTime::new(text.parse()?, Time::MIDNIGHT)),
        }
    }
}

impl From<Date> for DateTime {
    /// The day at midnight.
    fn from(date: Date) -> DateTime {
        DateTime::new(date, Time::MIDNIGHT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn dates_parse_and_refuse() {
        assert_eq!("2026-10-17".parse(), Ok(Date::new(2026, 10, 17)));
        assert_eq!(" 2026-1-5 ".parse(), Ok(Date::new(2026, 1, 5)));
        assert_eq!("2024-02-29".parse(), Ok(Date::new(2024, 2, 29)));
        for bad in [
            "",
            "2026",
            "2026-10",
            "2026-13-01",
            "2026-02-29",
            "2026-10-32",
            "2026-10-0",
            "26-10-17x",
            "2026/10/17",
            "2026-10-17-1",
            "+2026-10-17",
        ] {
            assert!(bad.parse::<Date>().is_err(), "{bad:?}");
        }
        assert!("today".parse::<Date>().unwrap().is_valid());
    }

    #[test]
    fn times_parse_on_either_clock() {
        assert_eq!("14:30".parse(), Ok(Time::new(14, 30, 0)));
        assert_eq!("9:05:07".parse(), Ok(Time::new(9, 5, 7)));
        assert_eq!("2:30 pm".parse(), Ok(Time::new(14, 30, 0)));
        assert_eq!("2PM".parse(), Ok(Time::new(14, 0, 0)));
        assert_eq!("12:05am".parse(), Ok(Time::new(0, 5, 0)));
        assert!("12 p.m.".parse::<Time>().is_err());
        assert_eq!("12pm".parse(), Ok(Time::new(12, 0, 0)));
        for bad in [
            "", "14", "24:00", "12:60", "1:2:3:4", "13pm", "0am", "a:b", "-1:00",
        ] {
            assert!(bad.parse::<Time>().is_err(), "{bad:?}");
        }
        assert_eq!(Time::new(14, 30, 0).to_string(), "14:30");
        assert_eq!(Time::new(14, 30, 5).to_string(), "14:30:05");
        assert_eq!(Time::new(0, 5, 0).format(false, true), "12:05 AM");
        assert_eq!(Time::new(12, 5, 9).format(true, true), "12:05:09 PM");
        assert_eq!(Time::from_seconds(-1), Time::LAST);
    }

    #[test]
    fn date_times_parse_and_order() {
        let at = DateTime::new(Date::new(2026, 10, 17), Time::new(14, 30, 0));
        assert_eq!("2026-10-17 14:30".parse(), Ok(at));
        assert_eq!("2026-10-17T14:30".parse(), Ok(at));
        assert_eq!("2026-10-17 2:30 pm".parse(), Ok(at));
        assert_eq!(
            "2026-10-17".parse(),
            Ok(DateTime::from(Date::new(2026, 10, 17)))
        );
        assert!("2026-10-17 25:00".parse::<DateTime>().is_err());
        assert!("2026-10-17 x".parse::<DateTime>().is_err());
        assert!("today 14:30".parse::<DateTime>().is_ok());
        assert!(at < DateTime::new(Date::new(2026, 10, 17), Time::new(14, 30, 1)));
        assert!(at > DateTime::new(Date::new(2026, 10, 16), Time::LAST));
        assert_eq!(at.to_string(), "2026-10-17 14:30");
        assert!(DateTime::now_utc().is_valid());
    }

    #[test]
    fn moments_round_trip_across_a_change_of_summer_time() {
        let cet = Zone::posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        let utc = |y, m, d, h, min| DateTime::new(Date::new(y, m, d), Time::new(h, min, 0));
        // Summer time starts at 01:00 UTC on 29 March 2026: 02:00 is 03:00.
        let before = utc(2026, 3, 29, 0, 30).to_unix(&Zone::Utc);
        let after = utc(2026, 3, 29, 1, 30).to_unix(&Zone::Utc);
        assert_eq!(DateTime::from_unix(before, &cet), utc(2026, 3, 29, 1, 30));
        assert_eq!(DateTime::from_unix(after, &cet), utc(2026, 3, 29, 3, 30));
        assert_eq!(utc(2026, 3, 29, 3, 30).to_unix(&cet), after);
        // In the gap: on the clock of before, an hour on.
        assert_eq!(utc(2026, 3, 29, 2, 30).to_unix(&cet), after);
        // It ends at 01:00 UTC on 25 October: 02:30 comes twice, the
        // first (summer) one is taken.
        let first = utc(2026, 10, 25, 0, 30).to_unix(&Zone::Utc);
        let second = utc(2026, 10, 25, 1, 30).to_unix(&Zone::Utc);
        assert_eq!(DateTime::from_unix(first, &cet), utc(2026, 10, 25, 2, 30));
        assert_eq!(DateTime::from_unix(second, &cet), utc(2026, 10, 25, 2, 30));
        assert_eq!(utc(2026, 10, 25, 2, 30).to_unix(&cet), first);
        // A day across it is 25 hours.
        let day = utc(2026, 10, 26, 0, 0).to_unix(&cet) - utc(2026, 10, 25, 0, 0).to_unix(&cet);
        assert_eq!(day, 25 * 3600);
    }

    /// The local zone follows summer time: run in a child with `TZ` set,
    /// as the C library reads it once.
    #[test]
    fn the_local_zone_follows_summer_time() {
        if std::env::var_os("RICH_INTERACT_TZ_CHILD").is_none() {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "datetime::tests::the_local_zone_follows_summer_time",
                    "--quiet",
                ])
                .env("RICH_INTERACT_TZ_CHILD", "1")
                .env("TZ", "CET-1CEST,M3.5.0,M10.5.0/3")
                .output()
                .unwrap();
            let said = String::from_utf8_lossy(&child.stdout);
            assert!(child.status.success(), "{said}");
            return;
        }
        let utc = |m, d, h| DateTime::new(Date::new(2026, m, d), Time::new(h, 0, 0));
        let winter = utc(1, 15, 12).to_unix(&Zone::Utc);
        let summer = utc(7, 15, 12).to_unix(&Zone::Utc);
        assert_eq!(Zone::Local.offset_at(winter), 3600);
        assert_eq!(Zone::Local.offset_at(summer), 7200);
        assert_eq!(DateTime::from_unix(summer, &Zone::Local), utc(7, 15, 14));
        assert_eq!(
            utc(10, 25, 2).to_unix(&Zone::Local),
            utc(10, 25, 0).to_unix(&Zone::Utc)
        );
    }
}
