//! Timestamps: read from RFC 3339 text, the access log's format and epoch
//! numbers, and shown as written, relative to the first record, or not at
//! all. Hand-written, with no time-zone data: a time without an offset is
//! read as UTC.

use std::fmt;
use std::str::FromStr;

use super::Record;
use crate::event::Value;

/// `text` as microseconds since the Unix epoch: RFC 3339 and the ISO 8601
/// forms logs use (`2026-10-18T10:00:00Z`, `2026-10-18 10:00:00,123`,
/// `2026-10-18T12:00:00.5+02:00`, `2026/10/18 10:00:00`, a bare date), the
/// access log's `10/Oct/2000:13:55:36 -0700`, or an epoch number (seconds, milliseconds,
/// microseconds or nanoseconds, told apart by size). A time without an
/// offset is UTC.
///
/// ```
/// use rich_ext::log_stream::parse_timestamp;
///
/// assert_eq!(parse_timestamp("1970-01-01T00:00:01Z"), Some(1_000_000));
/// assert_eq!(parse_timestamp("1970-01-01T02:00:01+02:00"), Some(1_000_000));
/// assert_eq!(parse_timestamp("1500"), Some(1_500_000_000));
/// assert_eq!(parse_timestamp("soon"), None);
/// ```
pub fn parse_timestamp(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Some((micros, used)) = iso_prefix(text) {
        if used == text.len() {
            return Some(micros);
        }
    }
    if let Some((micros, used)) = clf_prefix(text) {
        if used == text.len() {
            return Some(micros);
        }
    }
    if super::filter::is_number(text) {
        if let Ok(integer) = text.parse::<i128>() {
            return epoch_integer(integer);
        }
        return text.parse::<f64>().ok().and_then(epoch_float);
    }
    None
}

/// A number value as an epoch time (see [`parse_timestamp`]).
pub(crate) fn epoch(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(v) => epoch_integer(i128::from(*v)),
        Value::Unsigned(v) => epoch_integer(i128::from(*v)),
        Value::Integer128(v) => epoch_integer(*v),
        Value::Unsigned128(v) => i128::try_from(*v).ok().and_then(epoch_integer),
        Value::Float(v) => epoch_float(*v),
        Value::String(text) => parse_timestamp(text),
        _ => None,
    }
}

fn epoch_integer(value: i128) -> Option<i64> {
    let size = value.unsigned_abs();
    let micros = if size < 100_000_000_000 {
        value.checked_mul(1_000_000)?
    } else if size < 100_000_000_000_000 {
        value.checked_mul(1_000)?
    } else if size < 100_000_000_000_000_000 {
        value
    } else {
        value / 1_000
    };
    i64::try_from(micros).ok()
}

fn epoch_float(value: f64) -> Option<i64> {
    if !value.is_finite() {
        return None;
    }
    let size = value.abs();
    let micros = if size < 1e11 {
        value * 1e6
    } else if size < 1e14 {
        value * 1e3
    } else if size < 1e17 {
        value
    } else {
        value / 1e3
    };
    (micros.abs() < 9.2e18).then(|| micros.round() as i64)
}

/// Days from 1970-01-01 to `year-month-day` (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

/// `count` ASCII digits at `at`, as a number.
fn digits(bytes: &[u8], at: usize, count: usize) -> Option<i64> {
    let slice = bytes.get(at..at + count)?;
    slice.iter().try_fold(0i64, |n, b| {
        b.is_ascii_digit().then(|| n * 10 + i64::from(b - b'0'))
    })
}

fn micros_of(date: (i64, i64, i64), time: (i64, i64, i64), fraction: i64, offset: i64) -> i64 {
    let (year, month, day) = date;
    let (hour, minute, second) = time;
    let seconds =
        days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second - offset;
    seconds * 1_000_000 + fraction
}

fn valid_date(year: i64, month: i64, day: i64) -> bool {
    (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)
}

/// An ISO 8601 / RFC 3339 time at the start of `text`, and the bytes it
/// took: `YYYY-MM-DD` (or `YYYY/MM/DD`), then optionally `T` or a space,
/// `HH:MM`, `:SS`, a fraction after `.` or `,`, and `Z` or an offset.
pub(crate) fn iso_prefix(text: &str) -> Option<(i64, usize)> {
    let bytes = text.as_bytes();
    let year = digits(bytes, 0, 4)?;
    let separator = *bytes.get(4)?;
    if !matches!(separator, b'-' | b'/') || bytes.get(7) != Some(&separator) {
        return None;
    }
    let (month, day) = (digits(bytes, 5, 2)?, digits(bytes, 8, 2)?);
    if !valid_date(year, month, day) {
        return None;
    }
    let date = (year, month, day);
    let mut at = 10;
    let timed = matches!(bytes.get(at), Some(b'T' | b't' | b' '))
        && bytes.get(at + 3) == Some(&b':')
        && digits(bytes, at + 1, 2).is_some();
    if !timed {
        // A bare date ends at a word boundary.
        let ends = bytes.get(at).is_none_or(|b| !b.is_ascii_alphanumeric());
        return ends.then(|| (micros_of(date, (0, 0, 0), 0, 0), at));
    }
    at += 1;
    let hour = digits(bytes, at, 2)?;
    let minute = digits(bytes, at + 3, 2)?;
    at += 5;
    let mut second = 0;
    if bytes.get(at) == Some(&b':') {
        second = digits(bytes, at + 1, 2)?;
        at += 3;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut fraction = 0;
    if matches!(bytes.get(at), Some(b'.' | b','))
        && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)
    {
        at += 1;
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        // Microseconds: the first six digits, padded.
        let mut micros = 0;
        for index in 0..6 {
            let digit = bytes
                .get(start + index)
                .filter(|b| b.is_ascii_digit() && start + index < at)
                .map_or(0, |b| i64::from(b - b'0'));
            micros = micros * 10 + digit;
        }
        fraction = micros;
    }
    let mut offset = 0;
    match bytes.get(at) {
        Some(b'Z' | b'z') => at += 1,
        Some(sign @ (b'+' | b'-')) => {
            let sign = if *sign == b'-' { -1 } else { 1 };
            let hours = digits(bytes, at + 1, 2)?;
            at += 3;
            let mut minutes = 0;
            if bytes.get(at) == Some(&b':') {
                minutes = digits(bytes, at + 1, 2)?;
                at += 3;
            } else if let Some(m) = digits(bytes, at, 2) {
                minutes = m;
                at += 2;
            }
            if hours > 23 || minutes > 59 {
                return None;
            }
            offset = sign * (hours * 3_600 + minutes * 60);
        }
        _ => {}
    }
    if bytes.get(at).is_some_and(u8::is_ascii_alphanumeric) {
        return None;
    }
    Some((
        micros_of(date, (hour, minute, second), fraction, offset),
        at,
    ))
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The access log's `10/Oct/2000:13:55:36 -0700` at the start of `text`,
/// and the bytes it took.
pub(crate) fn clf_prefix(text: &str) -> Option<(i64, usize)> {
    let bytes = text.as_bytes();
    let day = digits(bytes, 0, 2)?;
    if bytes.get(2) != Some(&b'/') || bytes.get(6) != Some(&b'/') || bytes.get(11) != Some(&b':') {
        return None;
    }
    let month = MONTHS
        .iter()
        .position(|name| text.get(3..6) == Some(*name))? as i64
        + 1;
    let year = digits(bytes, 7, 4)?;
    if !valid_date(year, month, day) {
        return None;
    }
    let hour = digits(bytes, 12, 2)?;
    let minute = digits(bytes, 15, 2)?;
    let second = digits(bytes, 18, 2)?;
    if bytes.get(14) != Some(&b':') || bytes.get(17) != Some(&b':') {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut at = 20;
    let mut offset = 0;
    if bytes.get(at) == Some(&b' ') && matches!(bytes.get(at + 1), Some(b'+' | b'-')) {
        let sign = if bytes[at + 1] == b'-' { -1 } else { 1 };
        let hours = digits(bytes, at + 2, 2)?;
        let minutes = digits(bytes, at + 4, 2)?;
        offset = sign * (hours * 3_600 + minutes * 60);
        at += 6;
    }
    Some((
        micros_of((year, month, day), (hour, minute, second), 0, offset),
        at,
    ))
}

/// A time difference, signed, the way a relative time shows it: `+0.250s`,
/// `+1m02.500s`, `+2h03m04s`, `+3d02h03m`, `-0.100s`.
///
/// ```
/// use rich_ext::log_stream::format_offset;
///
/// assert_eq!(format_offset(1_250_000), "+1.250s");
/// assert_eq!(format_offset(62_500_000), "+1m02.500s");
/// assert_eq!(format_offset(-100_000), "-0.100s");
/// ```
pub fn format_offset(micros: i64) -> String {
    let sign = if micros < 0 { '-' } else { '+' };
    let millis = micros.unsigned_abs() / 1_000;
    let (seconds, millis) = (millis / 1_000, millis % 1_000);
    if seconds < 60 {
        format!("{sign}{seconds}.{millis:03}s")
    } else if seconds < 3_600 {
        format!("{sign}{}m{:02}.{millis:03}s", seconds / 60, seconds % 60)
    } else if seconds < 86_400 {
        format!(
            "{sign}{}h{:02}m{:02}s",
            seconds / 3_600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!(
            "{sign}{}d{:02}h{:02}m",
            seconds / 86_400,
            seconds / 3_600 % 24,
            seconds / 60 % 60
        )
    }
}

/// How a record's time is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimeMode {
    /// As written (the default).
    #[default]
    Absolute,
    /// Since the first record whose time parses (see [`format_offset`]); a
    /// time that does not parse is shown as written.
    Relative,
    /// Not at all.
    Hidden,
}

impl TimeMode {
    pub fn name(self) -> &'static str {
        match self {
            TimeMode::Absolute => "absolute",
            TimeMode::Relative => "relative",
            TimeMode::Hidden => "none",
        }
    }
}

impl fmt::Display for TimeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for TimeMode {
    type Err = String;

    /// `absolute`, `relative` or `none`.
    fn from_str(name: &str) -> Result<TimeMode, String> {
        match name {
            "absolute" => Ok(TimeMode::Absolute),
            "relative" => Ok(TimeMode::Relative),
            "none" => Ok(TimeMode::Hidden),
            other => Err(format!(
                "unknown time mode {other:?} (absolute, relative, none)"
            )),
        }
    }
}

/// Shows records' times in a [`TimeMode`], remembering the first time for
/// [`TimeMode::Relative`].
#[derive(Clone, Debug, Default)]
pub struct Clock {
    mode: TimeMode,
    origin: Option<i64>,
}

impl Clock {
    pub fn new(mode: TimeMode) -> Clock {
        Clock { mode, origin: None }
    }

    pub fn mode(&self) -> TimeMode {
        self.mode
    }

    /// The first time, once one was seen.
    pub fn origin(&self) -> Option<i64> {
        self.origin
    }

    /// What to show for `record`'s time; `None` when it has none or times
    /// are hidden.
    pub fn show(&mut self, record: &Record) -> Option<String> {
        match self.mode {
            TimeMode::Hidden => None,
            TimeMode::Absolute => record.time().map(|time| time.into_owned()),
            TimeMode::Relative => {
                let written = record.time()?;
                self.show_at(Some(&written), record.instant())
            }
        }
    }

    /// What to show for a time `written` that parsed to `instant`.
    pub fn show_at(&mut self, written: Option<&str>, instant: Option<i64>) -> Option<String> {
        match self.mode {
            TimeMode::Hidden => None,
            TimeMode::Absolute => written.map(str::to_string),
            TimeMode::Relative => match instant {
                Some(instant) => {
                    let origin = *self.origin.get_or_insert(instant);
                    Some(format_offset(instant.saturating_sub(origin)))
                }
                None => written.map(str::to_string),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rfc_3339_and_the_forms_logs_write() {
        let at = |text| parse_timestamp(text);
        assert_eq!(at("2026-10-18T10:00:00Z"), Some(1_792_317_600_000_000));
        assert_eq!(at("2026-10-18 10:00:00"), Some(1_792_317_600_000_000));
        assert_eq!(at("2026-10-18t10:00z"), Some(1_792_317_600_000_000));
        assert_eq!(
            at("2024-05-01T12:00:00.123456789+02:00"),
            Some(1_714_557_600_123_456)
        );
        assert_eq!(
            at("2024-05-01 12:00:00,123456+0200"),
            Some(1_714_557_600_123_456)
        );
        assert_eq!(at("2024-05-01T12:00:00.5+02"), Some(1_714_557_600_500_000));
        assert_eq!(at("2024-02-29"), Some(1_709_164_800_000_000));
        assert_eq!(at("2026/10/18 10:00:00"), Some(1_792_317_600_000_000));
        assert_eq!(at("2026/10-18"), None);
        assert_eq!(at("1969-12-31T23:59:59Z"), Some(-1_000_000));
        assert_eq!(at("10/Oct/2000:13:55:36 -0700"), Some(971_211_336_000_000));
        for bad in [
            "2023-02-29",
            "2024-13-01",
            "2024-05-01T25:00:00Z",
            "2024-05-01T10:61Z",
            "2024-05-01T10:00:00+99:00",
            "2024-05-01X",
            "2024-05-01T10:00:00Zulu",
            "10/Foo/2000:13:55:36 -0700",
            "",
            "-",
            "1e400",
        ] {
            assert_eq!(at(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn epoch_numbers_are_told_apart_by_size() {
        assert_eq!(parse_timestamp("1690000000"), Some(1_690_000_000_000_000));
        assert_eq!(
            parse_timestamp("1690000000123"),
            Some(1_690_000_000_123_000)
        );
        assert_eq!(
            parse_timestamp("1690000000123456"),
            Some(1_690_000_000_123_456)
        );
        assert_eq!(
            parse_timestamp("1690000000123456789"),
            Some(1_690_000_000_123_456)
        );
        assert_eq!(epoch(&Value::Float(1.5)), Some(1_500_000));
        assert_eq!(epoch(&Value::Float(f64::NAN)), None);
        assert_eq!(epoch(&Value::Unsigned128(u128::MAX)), None);
        assert_eq!(epoch(&Value::Integer(i64::MAX)), Some(i64::MAX / 1_000));
    }

    #[test]
    fn prefixes_report_what_they_took() {
        assert_eq!(
            iso_prefix("2026-10-18T10:00:00Z INFO x").map(|(_, n)| n),
            Some(20)
        );
        assert_eq!(iso_prefix("2026-10-18 x").map(|(_, n)| n), Some(10));
        assert_eq!(
            clf_prefix("10/Oct/2000:13:55:36 -0700] x").map(|(_, n)| n),
            Some(26)
        );
    }

    #[test]
    fn offsets_read_at_a_glance() {
        assert_eq!(format_offset(0), "+0.000s");
        assert_eq!(format_offset(999), "+0.000s");
        assert_eq!(format_offset(59_999_000), "+59.999s");
        assert_eq!(format_offset(3_599_000_000), "+59m59.000s");
        assert_eq!(format_offset(7_384_000_000), "+2h03m04s");
        assert_eq!(format_offset(266_580_000_000), "+3d02h03m");
        assert_eq!(format_offset(i64::MIN).chars().next(), Some('-'));
    }

    #[test]
    fn a_relative_clock_starts_at_the_first_time_that_parses() {
        let record = |time: &str| Record::new("json").field("time", Value::String(time.into()));
        let mut clock = Clock::new(TimeMode::Relative);
        assert_eq!(clock.show(&record("later")).as_deref(), Some("later"));
        assert_eq!(
            clock.show(&record("2026-10-18T10:00:00Z")).as_deref(),
            Some("+0.000s")
        );
        assert_eq!(
            clock.show(&record("2026-10-18T10:00:01.5Z")).as_deref(),
            Some("+1.500s")
        );
        assert_eq!(clock.show(&Record::text("no time")), None);
        let mut hidden = Clock::new(TimeMode::Hidden);
        assert_eq!(hidden.show(&record("2026-10-18T10:00:00Z")), None);
        let mut absolute = Clock::new(TimeMode::Absolute);
        assert_eq!(absolute.show(&record("x")).as_deref(), Some("x"));
        assert_eq!("none".parse::<TimeMode>(), Ok(TimeMode::Hidden));
        assert!("soon".parse::<TimeMode>().is_err());
    }
}
