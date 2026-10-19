//! Time zones for [`datetime`](crate::datetime): how far a zone's clocks
//! are from UTC at a moment, summer time included. The local zone is the
//! platform's (`localtime_r` on Unix, `SystemTimeToTzSpecificLocalTime`
//! on Windows); a zone can also be read from a TZif file (the zoneinfo
//! database's format) or a POSIX `TZ` rule, with no dependency.

use std::sync::Arc;

/// A time zone: how many seconds its clocks are ahead of UTC at each
/// moment. [`Zone::Local`] is the platform's local zone, which follows
/// summer time; [`Zone::posix`], [`Zone::tzif`] and [`Zone::named`] read
/// a zone's rules; [`Zone::Fixed`] never changes.
///
/// ```
/// use rich_interact::datetime::{Date, DateTime, Time, Zone};
///
/// // Central Europe: an hour ahead in winter, two in summer.
/// let cet = Zone::posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
/// let winter = DateTime::new(Date::new(2026, 1, 15), Time::new(12, 0, 0));
/// let summer = DateTime::new(Date::new(2026, 7, 15), Time::new(12, 0, 0));
/// assert_eq!(cet.offset_at(winter.to_unix(&Zone::Utc)), 3600);
/// assert_eq!(cet.offset_at(summer.to_unix(&Zone::Utc)), 7200);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum Zone {
    /// Coordinated Universal Time.
    Utc,
    /// The platform's local zone, as it is now configured (`TZ`,
    /// `/etc/localtime`, the system's setting).
    Local,
    /// Always this many seconds ahead of UTC (behind if negative).
    Fixed(i32),
    /// A zone's rules, read by [`Zone::posix`], [`Zone::tzif`] or
    /// [`Zone::named`].
    Rules(Arc<Rules>),
}

/// A zone's rules: the transitions it has had, and the POSIX rule for
/// the years after them.
#[derive(Debug, PartialEq)]
pub struct Rules {
    /// Each transition: from when (seconds since 1970 UTC), the offset.
    transitions: Vec<(i64, i32)>,
    /// The offset before the first transition.
    before: i32,
    /// After the last transition (or with none): the rule.
    rule: Option<Posix>,
}

/// A POSIX `TZ` rule: a standard offset, and a summer time with the days
/// it starts and ends.
#[derive(Clone, Debug, PartialEq)]
struct Posix {
    /// Seconds ahead of UTC (POSIX writes it the other way round).
    std: i32,
    dst: Option<Summer>,
}

#[derive(Clone, Debug, PartialEq)]
struct Summer {
    offset: i32,
    start: (Day, i32),
    end: (Day, i32),
}

/// The day of the year a summer time starts or ends.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Day {
    /// `Jn`: day 1 to 365, the 29th of February never counted.
    Julian(u32),
    /// `n`: day 0 to 365, counting it.
    Zero(u32),
    /// `Mm.w.d`: day `d` (0 Sunday) of week `w` (5 the last) of month `m`.
    Month(u32, u32, u32),
}

impl Zone {
    /// How many seconds the zone's clocks are ahead of UTC at `unix`
    /// (seconds since 1970-01-01 00:00 UTC).
    pub fn offset_at(&self, unix: i64) -> i32 {
        match self {
            Zone::Utc => 0,
            Zone::Fixed(offset) => *offset,
            Zone::Local => local_offset(unix),
            Zone::Rules(rules) => rules.offset_at(unix),
        }
    }

    /// The zone a POSIX `TZ` rule describes: `"CET-1CEST,M3.5.0,M10.5.0/3"`,
    /// `"EST5EDT,M3.2.0,M11.1.0"`, `"<+0330>-3:30"`, `"UTC0"`. A summer
    /// time with no days given starts and ends on the United States'.
    pub fn posix(rule: &str) -> Result<Zone, String> {
        let posix = parse_posix(rule).ok_or_else(|| format!("{rule:?} is not a POSIX TZ rule"))?;
        Ok(Zone::Rules(Arc::new(Rules {
            transitions: Vec::new(),
            before: posix.std,
            rule: Some(posix),
        })))
    }

    /// The zone in a TZif file (`/usr/share/zoneinfo/Europe/Berlin`):
    /// its transitions, and the rule in its footer for the years after.
    pub fn tzif(bytes: &[u8]) -> Result<Zone, String> {
        parse_tzif(bytes)
            .map(|rules| Zone::Rules(Arc::new(rules)))
            .ok_or_else(|| "not a TZif file".to_string())
    }

    /// The zone named `name` in the zoneinfo database (`"Europe/London"`),
    /// read from `$TZDIR` or `/usr/share/zoneinfo`; an error where there is
    /// no such file (as on Windows).
    pub fn named(name: &str) -> Result<Zone, String> {
        if name.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(format!("{name:?} is not a zone name"));
        }
        let dir = std::env::var_os("TZDIR")
            .filter(|d| !d.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "/usr/share/zoneinfo".into());
        let bytes = std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        Zone::tzif(&bytes)
    }
}

impl Rules {
    fn offset_at(&self, unix: i64) -> i32 {
        match self.transitions.partition_point(|&(at, _)| at <= unix) {
            0 if !self.transitions.is_empty() => self.before,
            n if n < self.transitions.len() => self.transitions[n - 1].1,
            n => match &self.rule {
                Some(rule) => rule.offset_at(unix),
                None => self
                    .transitions
                    .get(n.wrapping_sub(1))
                    .map_or(self.before, |t| t.1),
            },
        }
    }
}

impl Posix {
    fn offset_at(&self, unix: i64) -> i32 {
        let Some(dst) = &self.dst else {
            return self.std;
        };
        // The year as the zone's standard time has it.
        let year =
            crate::datetime::Date::from_days((unix + self.std as i64).div_euclid(86_400)).year;
        // Summer time starts at a standard-time hour and ends at a summer one.
        let start = day_start(year, dst.start.0) + dst.start.1 as i64 - self.std as i64;
        let end = day_start(year, dst.end.0) + dst.end.1 as i64 - dst.offset as i64;
        let summer = if start < end {
            unix >= start && unix < end
        } else {
            // The southern hemisphere: summer over the new year.
            !(unix >= end && unix < start)
        };
        if summer {
            dst.offset
        } else {
            self.std
        }
    }
}

/// Seconds from 1970 to the start of `day` of `year` (as if UTC).
fn day_start(year: i32, day: Day) -> i64 {
    use crate::datetime::Date;
    let jan1 = Date::new(year, 1, 1).days();
    let days = match day {
        Day::Julian(n) => {
            let n = n.clamp(1, 365) as i64;
            // The 29th of February is not counted: day 60 is 1 March.
            jan1 + n - 1 + (Date::is_leap_year(year) && n >= 60) as i64
        }
        Day::Zero(n) => jan1 + n.min(365) as i64,
        Day::Month(month, week, weekday) => {
            let month = month.clamp(1, 12);
            let first = Date::new(year, month, 1);
            // `Date::weekday` is 0 for Monday; POSIX's 0 is Sunday.
            let first_wd = (first.weekday() + 1) % 7;
            let mut day = 1 + (weekday + 7 - first_wd) % 7 + (week.clamp(1, 5) - 1) * 7;
            let last = Date::days_in_month(year, month);
            while day > last {
                day -= 7;
            }
            Date::new(year, month, day).days()
        }
    };
    days * 86_400
}

/// A POSIX TZ rule.
fn parse_posix(text: &str) -> Option<Posix> {
    let mut s = text.trim().trim_start_matches(':');
    let _std_name = take_name(&mut s)?;
    let std = -take_offset(&mut s)?;
    if s.is_empty() {
        return Some(Posix { std, dst: None });
    }
    let _dst_name = take_name(&mut s)?;
    let offset = if s.starts_with(',') || s.is_empty() {
        std + 3600
    } else {
        -take_offset(&mut s)?
    };
    let (start, end) = if s.is_empty() {
        // The United States' rule since 2007.
        ((Day::Month(3, 2, 0), 7200), (Day::Month(11, 1, 0), 7200))
    } else {
        s = s.strip_prefix(',')?;
        let start = take_rule_day(&mut s)?;
        s = s.strip_prefix(',')?;
        let end = take_rule_day(&mut s)?;
        if !s.is_empty() {
            return None;
        }
        (start, end)
    };
    Some(Posix {
        std,
        dst: Some(Summer { offset, start, end }),
    })
}

/// A zone's abbreviation: three letters or more, or anything in `<>`.
fn take_name<'a>(s: &mut &'a str) -> Option<&'a str> {
    if let Some(rest) = s.strip_prefix('<') {
        let end = rest.find('>')?;
        let name = &rest[..end];
        *s = &rest[end + 1..];
        return Some(name);
    }
    let end = s
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    if end < 3 {
        return None;
    }
    let name = &s[..end];
    *s = &s[end..];
    Some(name)
}

/// `[+-]hh[:mm[:ss]]`, in seconds.
fn take_offset(s: &mut &str) -> Option<i32> {
    let (sign, rest) = match s.as_bytes().first() {
        Some(b'-') => (-1, &s[1..]),
        Some(b'+') => (1, &s[1..]),
        _ => (1, *s),
    };
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ':'))
        .unwrap_or(rest.len());
    let mut total = 0i32;
    for (i, part) in rest[..end].split(':').enumerate() {
        if i > 2 || part.is_empty() || part.len() > 3 {
            return None;
        }
        let n: i32 = part.parse().ok()?;
        total += n * [3600, 60, 1][i];
    }
    *s = &rest[end..];
    Some(sign * total)
}

/// A rule's day and the time on it (`M3.5.0/3`; 02:00 by default).
fn take_rule_day(s: &mut &str) -> Option<(Day, i32)> {
    let end = s.find([',', '/']).unwrap_or(s.len());
    let part = &s[..end];
    let day = if let Some(n) = part.strip_prefix('J') {
        Day::Julian(n.parse().ok().filter(|n| (1..=365).contains(n))?)
    } else if let Some(m) = part.strip_prefix('M') {
        let mut it = m.split('.').map(|p| p.parse::<u32>().ok());
        let (month, week, day) = (it.next()??, it.next()??, it.next()??);
        if it.next().is_some() || !(1..=12).contains(&month) || !(1..=5).contains(&week) || day > 6
        {
            return None;
        }
        Day::Month(month, week, day)
    } else {
        Day::Zero(part.parse().ok().filter(|n| *n <= 365)?)
    };
    *s = &s[end..];
    let time = match s.strip_prefix('/') {
        Some(rest) => {
            *s = rest;
            take_offset(s)?
        }
        None => 7200,
    };
    Some((day, time))
}

/// The rules in a TZif file, version 1 to 4.
fn parse_tzif(bytes: &[u8]) -> Option<Rules> {
    struct Header {
        version: u8,
        isut: usize,
        isstd: usize,
        leap: usize,
        time: usize,
        types: usize,
        chars: usize,
    }
    fn header(b: &[u8]) -> Option<Header> {
        if b.len() < 44 || &b[..4] != b"TZif" {
            return None;
        }
        let count =
            |i: usize| u32::from_be_bytes(b[20 + i * 4..24 + i * 4].try_into().unwrap()) as usize;
        Some(Header {
            version: b[4],
            isut: count(0),
            isstd: count(1),
            leap: count(2),
            time: count(3),
            types: count(4),
            chars: count(5),
        })
    }
    fn block_len(h: &Header, time_size: usize) -> usize {
        h.time * time_size
            + h.time
            + h.types * 6
            + h.chars
            + h.leap * (time_size + 4)
            + h.isstd
            + h.isut
    }
    let first = header(bytes)?;
    let (h, data, time_size) = if first.version >= b'2' {
        let second_at = 44 + block_len(&first, 4);
        let second = header(bytes.get(second_at..)?)?;
        (second, bytes.get(second_at + 44..)?, 8)
    } else {
        (first, &bytes[44..], 4)
    };
    let len = block_len(&h, time_size);
    if data.len() < len || h.types == 0 {
        return None;
    }
    let times = &data[..h.time * time_size];
    let indices = &data[h.time * time_size..h.time * time_size + h.time];
    let types_at = h.time * time_size + h.time;
    let utoff = |i: usize| -> Option<i32> {
        let at = types_at + i * 6;
        Some(i32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
    };
    let mut transitions = Vec::with_capacity(h.time);
    for i in 0..h.time {
        let at = if time_size == 8 {
            i64::from_be_bytes(times[i * 8..i * 8 + 8].try_into().ok()?)
        } else {
            i32::from_be_bytes(times[i * 4..i * 4 + 4].try_into().ok()?) as i64
        };
        let index = indices[i] as usize;
        if index >= h.types {
            return None;
        }
        transitions.push((at, utoff(index)?));
    }
    // RFC 8536: before the first transition, the first type.
    let before = utoff(0)?;
    // The footer: a POSIX rule between line breaks (version 2 on).
    let rule = (time_size == 8)
        .then(|| {
            let footer = data.get(len..)?;
            let text = std::str::from_utf8(footer).ok()?;
            let text = text.strip_prefix('\n')?;
            let line = &text[..text.find('\n')?];
            (!line.is_empty()).then(|| parse_posix(line)).flatten()
        })
        .flatten();
    Some(Rules {
        transitions,
        before,
        rule,
    })
}

/// The local zone's offset at `unix`, from the platform.
pub(crate) fn local_offset(unix: i64) -> i32 {
    platform::offset(unix).unwrap_or_else(|| fallback().offset_at(unix))
}

/// The local zone where the platform has no answer: `TZ` (a POSIX rule,
/// or a zone name), else `/etc/localtime`, else UTC.
fn fallback() -> &'static Zone {
    static ZONE: std::sync::OnceLock<Zone> = std::sync::OnceLock::new();
    ZONE.get_or_init(|| {
        if let Some(tz) = std::env::var("TZ").ok().filter(|t| !t.is_empty()) {
            let tz = tz.trim_start_matches(':');
            if let Ok(zone) = Zone::named(tz) {
                return zone;
            }
            if let Ok(zone) = Zone::posix(tz) {
                return zone;
            }
        }
        std::fs::read("/etc/localtime")
            .ok()
            .and_then(|bytes| Zone::tzif(&bytes).ok())
            .unwrap_or(Zone::Utc)
    })
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
mod platform {
    extern "C" {
        fn tzset();
    }

    /// `localtime_r`'s `tm_gmtoff`: the C library reads `TZ` and the
    /// zoneinfo database, summer time included.
    #[allow(unsafe_code)]
    pub(super) fn offset(unix: i64) -> Option<i32> {
        static TZSET: std::sync::Once = std::sync::Once::new();
        // SAFETY: `tzset` takes no arguments and only reads the
        // environment; `localtime_r` writes only to `tm`, which is ours,
        // and reads `time`, which lives across the call.
        unsafe {
            TZSET.call_once(|| tzset());
            let time: libc::time_t = unix.try_into().ok()?;
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&time, &mut tm).is_null() {
                return None;
            }
            i32::try_from(tm.tm_gmtoff).ok()
        }
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

    use crate::datetime::{Date, DateTime, Time};

    /// The system's zone, through `SystemTimeToTzSpecificLocalTime`: the
    /// local time at `unix`, less `unix`.
    #[allow(unsafe_code)]
    pub(super) fn offset(unix: i64) -> Option<i32> {
        let utc = DateTime::from_unix(unix, &super::Zone::Utc);
        let st = |at: DateTime| SYSTEMTIME {
            wYear: at.date.year as u16,
            wMonth: at.date.month as u16,
            wDayOfWeek: 0,
            wDay: at.date.day as u16,
            wHour: at.time.hour as u16,
            wMinute: at.time.minute as u16,
            wSecond: at.time.second as u16,
            wMilliseconds: 0,
        };
        let universal = st(utc);
        let mut local = st(utc);
        // SAFETY: both pointers are to SYSTEMTIMEs that live across the
        // call; a null zone asks for the system's current one.
        let ok =
            unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &universal, &mut local) };
        if ok == 0 {
            return None;
        }
        let local = DateTime::new(
            Date::new(local.wYear as i32, local.wMonth as u32, local.wDay as u32),
            Time::new(
                local.wHour as u32,
                local.wMinute as u32,
                local.wSecond as u32,
            ),
        );
        i32::try_from(local.to_unix(&super::Zone::Utc) - unix).ok()
    }
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
mod platform {
    pub(super) fn offset(_unix: i64) -> Option<i32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datetime::{Date, DateTime, Time};

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        DateTime::new(Date::new(y, m, d), Time::new(h, min, 0)).to_unix(&Zone::Utc)
    }

    #[test]
    fn posix_rules_follow_summer_time() {
        let cet = Zone::posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        // 2026: summer time from 29 March 01:00 UTC to 25 October 01:00 UTC.
        assert_eq!(cet.offset_at(at(2026, 3, 29, 0, 59)), 3600);
        assert_eq!(cet.offset_at(at(2026, 3, 29, 1, 0)), 7200);
        assert_eq!(cet.offset_at(at(2026, 10, 25, 0, 59)), 7200);
        assert_eq!(cet.offset_at(at(2026, 10, 25, 1, 0)), 3600);
        let us = Zone::posix("EST5EDT").unwrap();
        // 8 March 2026 07:00 UTC, and 1 November 06:00 UTC.
        assert_eq!(us.offset_at(at(2026, 3, 8, 6, 59)), -5 * 3600);
        assert_eq!(us.offset_at(at(2026, 3, 8, 7, 0)), -4 * 3600);
        assert_eq!(us.offset_at(at(2026, 11, 1, 5, 59)), -4 * 3600);
        assert_eq!(us.offset_at(at(2026, 11, 1, 6, 0)), -5 * 3600);
        // The south: summer over the new year.
        let nz = Zone::posix("NZST-12NZDT,M9.5.0,M4.1.0/3").unwrap();
        assert_eq!(nz.offset_at(at(2026, 1, 10, 0, 0)), 13 * 3600);
        assert_eq!(nz.offset_at(at(2026, 7, 10, 0, 0)), 12 * 3600);
        let tehran = Zone::posix("<+0330>-3:30").unwrap();
        assert_eq!(tehran.offset_at(0), 3 * 3600 + 1800);
        for bad in [
            "",
            "X",
            "CET",
            "CET-1CEST,M13.5.0,M10.5.0",
            "CET-1CEST,M3.5.0",
        ] {
            assert!(Zone::posix(bad).is_err(), "{bad:?}");
        }
    }

    /// A version-2 TZif file: one transition to summer time, then the rule.
    fn tzif() -> Vec<u8> {
        fn header(out: &mut Vec<u8>, version: u8, time: u32, types: u32, chars: u32) {
            out.extend(b"TZif");
            out.push(version);
            out.extend([0; 15]);
            for n in [0, 0, 0, time, types, chars] {
                out.extend((n as u32).to_be_bytes());
            }
        }
        let mut out = Vec::new();
        header(&mut out, b'2', 0, 1, 4);
        out.extend(3600i32.to_be_bytes());
        out.extend([0, 0]);
        out.extend(b"CET\0");
        header(&mut out, b'2', 1, 2, 9);
        out.extend(at(2026, 3, 29, 1, 0).to_be_bytes());
        out.push(1);
        out.extend(3600i32.to_be_bytes());
        out.extend([0, 0]);
        out.extend(7200i32.to_be_bytes());
        out.extend([1, 4]);
        out.extend(b"CET\0CEST\0");
        out.extend(b"\nCET-1CEST,M3.5.0,M10.5.0/3\n");
        out
    }

    #[test]
    fn tzif_files_read_with_their_footer() {
        let zone = Zone::tzif(&tzif()).unwrap();
        assert_eq!(zone.offset_at(at(2026, 3, 1, 0, 0)), 3600);
        assert_eq!(zone.offset_at(at(2026, 3, 29, 1, 0)), 7200);
        // After the last transition, the footer's rule.
        assert_eq!(zone.offset_at(at(2026, 10, 25, 1, 0)), 3600);
        assert_eq!(zone.offset_at(at(2030, 7, 1, 0, 0)), 7200);
        assert!(Zone::tzif(b"nope").is_err());
        // The system's database, where there is one.
        if let Ok(london) = Zone::named("Europe/London") {
            assert_eq!(london.offset_at(at(2026, 1, 15, 12, 0)), 0);
            assert_eq!(london.offset_at(at(2026, 7, 15, 12, 0)), 3600);
            assert_eq!(london.offset_at(at(2040, 7, 15, 12, 0)), 3600);
        }
        assert!(Zone::named("../etc/passwd").is_err());
    }
}
