//! Semantic formatters: sizes, rates, durations, times, percentages and
//! numbers as people read them.
//!
//! Every function is pure and locale-neutral: `.` for the decimal point, `,`
//! for thousands, English words, UTC timestamps. They return plain strings,
//! so they drop into any renderable, table cell or log line.
//!
//! ```
//! use std::time::Duration;
//! use rich_ext::format;
//!
//! assert_eq!(format::bytes(1_500_000), "1.5 MB");
//! assert_eq!(format::bytes_binary(1_572_864), "1.5 MiB");
//! assert_eq!(format::rate(2_400_000.0), "2.4 MB/s");
//! assert_eq!(format::duration(Duration::from_secs(3723)), "1h 02m 03s");
//! assert_eq!(format::clock(Duration::from_secs(3723)), "1:02:03");
//! assert_eq!(format::percent(0.4251, 1), "42.5%");
//! assert_eq!(format::number(1234567), "1,234,567");
//! assert_eq!(format::compact(1_250_000.0), "1.2M");
//! ```

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DECIMAL: [&str; 9] = ["bytes", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
const BINARY: [&str; 9] = [
    "bytes", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB",
];

/// A byte count in decimal (base-1000) units, as upstream's
/// `filesize.decimal`: `1 byte`, `999 bytes`, `1.5 kB`.
pub fn bytes(n: u64) -> String {
    rich::filesize::decimal(n)
}

/// A byte count in binary (base-1024) units: `1 byte`, `1.5 KiB`, `2.0 GiB`.
pub fn bytes_binary(n: u64) -> String {
    scaled(n as f64, 1024.0, &BINARY)
}

/// A transfer rate in decimal units per second: `512 bytes/s`, `2.4 MB/s`.
///
/// Negative or non-finite rates (no progress yet) read `0 bytes/s`.
pub fn rate(bytes_per_second: f64) -> String {
    let rate = if bytes_per_second.is_finite() && bytes_per_second > 0.0 {
        bytes_per_second
    } else {
        0.0
    };
    format!("{}/s", scaled(rate, 1000.0, &DECIMAL))
}

/// Shared by [`bytes_binary`] and [`rate`]: one decimal above the first unit,
/// whole numbers below it, `byte` for exactly one.
fn scaled(value: f64, base: f64, units: &[&str]) -> String {
    if value < base {
        let whole = value.round() as u64;
        return if whole == 1 {
            "1 byte".to_string()
        } else {
            format!("{whole} {}", units[0])
        };
    }
    let mut unit = 0;
    let mut scaled = value;
    while scaled >= base && unit + 1 < units.len() {
        scaled /= base;
        unit += 1;
    }
    format!("{scaled:.1} {}", units[unit])
}

/// An elapsed time, compact and with the two largest units that matter:
///
/// | span | shown as |
/// |---|---|
/// | under 1 ms | `850µs` |
/// | under 1 s | `120ms` |
/// | under 1 min | `4.2s` |
/// | under 1 h | `3m 07s` |
/// | under 1 day | `1h 02m 03s` |
/// | longer | `2d 04h` |
pub fn duration(d: Duration) -> String {
    duration_with(d, false)
}

/// As [`duration`], with `us` instead of `µs` for ASCII-only output.
pub fn duration_with(d: Duration, ascii: bool) -> String {
    let secs = d.as_secs();
    if secs == 0 {
        let micros = d.as_micros();
        if micros < 1000 {
            return format!("{micros}{}", if ascii { "us" } else { "µs" });
        }
        return format!("{}ms", d.as_millis());
    }
    if secs < 60 {
        return format!("{:.1}s", d.as_secs_f64());
    }
    let (days, hours, minutes, seconds) =
        (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if days > 0 {
        format!("{days}d {hours:02}h")
    } else if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else {
        format!("{minutes}m {seconds:02}s")
    }
}

/// An elapsed time as a clock, `H:MM:SS`, the way upstream's progress
/// columns show it: `0:00:07`, `1:02:03`, `26:00:00`.
pub fn clock(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// How far `then` is from `now`, in words: `just now`, `45 seconds ago`,
/// `3 minutes ago`, `in 2 hours`, `5 days ago`, `in 3 months`, `2 years ago`.
///
/// Under ten seconds either way is `just now`. Months are 30 days and years
/// 365, which is what a relative time needs; use [`timestamp`] when the
/// exact moment matters.
pub fn relative(then: SystemTime, now: SystemTime) -> String {
    let (span, future) = match then.duration_since(now) {
        Ok(ahead) => (ahead, true),
        Err(behind) => (behind.duration(), false),
    };
    let secs = span.as_secs();
    if secs < 10 {
        return "just now".to_string();
    }
    let (count, unit) = match secs {
        s if s < 60 => (s, "second"),
        s if s < 3600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3600, "hour"),
        s if s < 30 * 86_400 => (s / 86_400, "day"),
        s if s < 365 * 86_400 => (s / (30 * 86_400), "month"),
        s => (s / (365 * 86_400), "year"),
    };
    let plural = if count == 1 { "" } else { "s" };
    if future {
        format!("in {count} {unit}{plural}")
    } else {
        format!("{count} {unit}{plural} ago")
    }
}

/// A moment as an ISO 8601 UTC timestamp to the second:
/// `2026-09-24T11:58:41Z`. Times before 1970 clamp to the epoch.
pub fn timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A ratio as a percentage with `decimals` places: `percent(0.4251, 1)` is
/// `42.5%`. Non-finite ratios read `-`.
pub fn percent(ratio: f64, decimals: usize) -> String {
    if !ratio.is_finite() {
        return "-".to_string();
    }
    format!("{:.decimals$}%", ratio * 100.0)
}

/// An integer with `,` between thousands: `-1,234,567`.
pub fn number(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A number shortened with a suffix for counts at a glance: `950`, `1.2k`,
/// `3.4M`, `1.0B`, `2.5T`. One decimal above a thousand, truncated rather
/// than rounded so `1999` never reads `2.0k`. Non-finite values read `-`.
pub fn compact(n: f64) -> String {
    if !n.is_finite() {
        return "-".to_string();
    }
    let sign = if n < 0.0 { "-" } else { "" };
    let abs = n.abs();
    if abs < 1000.0 {
        let whole = abs.trunc();
        return if abs == whole {
            format!("{sign}{whole}")
        } else {
            format!("{sign}{abs:.1}")
        };
    }
    let (value, suffix) = [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "k")]
        .into_iter()
        .find(|(scale, _)| abs >= *scale)
        .map(|(scale, suffix)| (abs / scale, suffix))
        .unwrap_or((abs, ""));
    format!("{sign}{:.1}{suffix}", (value * 10.0).trunc() / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_units() {
        assert_eq!(bytes(0), "0 bytes");
        assert_eq!(bytes(1), "1 byte");
        assert_eq!(bytes(999), "999 bytes");
        assert_eq!(bytes(1000), "1.0 kB");
        assert_eq!(bytes_binary(0), "0 bytes");
        assert_eq!(bytes_binary(1), "1 byte");
        assert_eq!(bytes_binary(1023), "1023 bytes");
        assert_eq!(bytes_binary(1024), "1.0 KiB");
        assert_eq!(bytes_binary(3 * 1024 * 1024 * 1024), "3.0 GiB");
        assert_eq!(bytes_binary(u64::MAX), "16.0 EiB");
    }

    #[test]
    fn rates() {
        assert_eq!(rate(0.0), "0 bytes/s");
        assert_eq!(rate(-5.0), "0 bytes/s");
        assert_eq!(rate(f64::NAN), "0 bytes/s");
        assert_eq!(rate(512.0), "512 bytes/s");
        assert_eq!(rate(1000.0), "1.0 kB/s");
        assert_eq!(rate(2.4e6), "2.4 MB/s");
    }

    #[test]
    fn durations() {
        let ms = Duration::from_millis;
        assert_eq!(duration(Duration::ZERO), "0µs");
        assert_eq!(duration(Duration::from_micros(850)), "850µs");
        assert_eq!(duration_with(Duration::from_micros(850), true), "850us");
        assert_eq!(duration(ms(120)), "120ms");
        assert_eq!(duration(ms(4200)), "4.2s");
        assert_eq!(duration(ms(59_999)), "60.0s");
        assert_eq!(duration(Duration::from_secs(187)), "3m 07s");
        assert_eq!(duration(Duration::from_secs(3723)), "1h 02m 03s");
        assert_eq!(
            duration(Duration::from_secs(2 * 86_400 + 4 * 3600 + 59)),
            "2d 04h"
        );
    }

    #[test]
    fn clocks() {
        assert_eq!(clock(Duration::ZERO), "0:00:00");
        assert_eq!(clock(Duration::from_millis(7900)), "0:00:07");
        assert_eq!(clock(Duration::from_secs(26 * 3600)), "26:00:00");
    }

    #[test]
    fn relative_times() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let ago = |s| relative(now - Duration::from_secs(s), now);
        let ahead = |s| relative(now + Duration::from_secs(s), now);
        assert_eq!(ago(0), "just now");
        assert_eq!(ahead(9), "just now");
        assert_eq!(ago(45), "45 seconds ago");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(3 * 60 + 59), "3 minutes ago");
        assert_eq!(ahead(2 * 3600), "in 2 hours");
        assert_eq!(ago(86_400), "1 day ago");
        assert_eq!(ahead(95 * 86_400), "in 3 months");
        assert_eq!(ago(800 * 86_400), "2 years ago");
    }

    #[test]
    fn timestamps() {
        let at = |s| timestamp(UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(at(1_790_251_121), "2026-09-24T11:58:41Z");
        assert_eq!(at(4_102_444_799), "2099-12-31T23:59:59Z");
        assert_eq!(
            timestamp(UNIX_EPOCH - Duration::from_secs(5)),
            "1970-01-01T00:00:00Z"
        );
    }

    #[test]
    fn percents() {
        assert_eq!(percent(0.0, 0), "0%");
        assert_eq!(percent(0.4251, 1), "42.5%");
        assert_eq!(percent(1.0, 2), "100.00%");
        assert_eq!(percent(f64::INFINITY, 1), "-");
    }

    #[test]
    fn numbers() {
        assert_eq!(number(0), "0");
        assert_eq!(number(999), "999");
        assert_eq!(number(1000), "1,000");
        assert_eq!(number(-1_234_567), "-1,234,567");
        assert_eq!(number(i64::MIN), "-9,223,372,036,854,775,808");
    }

    #[test]
    fn compact_numbers() {
        assert_eq!(compact(0.0), "0");
        assert_eq!(compact(950.0), "950");
        assert_eq!(compact(12.5), "12.5");
        assert_eq!(compact(1999.0), "1.9k");
        assert_eq!(compact(1_250_000.0), "1.2M");
        assert_eq!(compact(-3.4e9), "-3.4B");
        assert_eq!(compact(2.5e12), "2.5T");
        assert_eq!(compact(f64::NAN), "-");
    }
}
