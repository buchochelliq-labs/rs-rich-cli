//! Human-readable file sizes.
//!
//! Port of upstream `rich/filesize.py`. [`decimal`] formats a byte count using
//! SI (base-1000) units, matching upstream's `filesize.decimal`.

/// Format `size` bytes as a decimal (base-1000) string, e.g. `1.5 kB`.
/// Port of `filesize.decimal`.
pub fn decimal(size: u64) -> String {
    to_str(
        size,
        &["kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"],
        1000.0,
        1,
    )
}

/// [`decimal`] for a signed count. Upstream takes any `int`: a negative size
/// is below every unit, so it prints as grouped bytes, e.g. `-1,234 bytes`.
pub fn decimal_signed(size: i64) -> String {
    match u64::try_from(size) {
        Ok(size) => decimal(size),
        Err(_) => format!("{} bytes", group_thousands(size)),
    }
}

/// [`pick_unit_and_suffix`] for a signed size: a negative size is below the
/// first unit (`if size < unit * base: break` on the first pass).
pub fn pick_unit_and_suffix_signed<'a>(
    size: i64,
    suffixes: &[&'a str],
    base: u64,
) -> (u64, &'a str) {
    match u64::try_from(size) {
        Ok(size) => pick_unit_and_suffix(size, suffixes, base),
        Err(_) => (1, suffixes[0]),
    }
}

/// `f"{n:,}"`: an integer with comma thousands separators.
fn group_thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        grouped.push('-');
    }
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Pick the largest unit whose value doesn't exceed `size`, returning the unit
/// (`base**i`) and its suffix. Port of `filesize.pick_unit_and_suffix`.
///
/// For any `u64` `size` the chosen unit maxes out at `base**6` (≈1e18 for
/// base 1000), which fits `u64` — the loop always breaks by then because
/// `size < base**6 * base` holds for every `u64`. Computed in `u128` to keep the
/// intermediate `unit * base` from overflowing.
pub fn pick_unit_and_suffix<'a>(size: u64, suffixes: &[&'a str], base: u64) -> (u64, &'a str) {
    let size = size as u128;
    let base = base as u128;
    let mut unit: u128 = 1;
    let mut suffix = suffixes[0];
    for (index, &candidate) in suffixes.iter().enumerate() {
        unit = base.pow(index as u32);
        suffix = candidate;
        if size < unit * base {
            break;
        }
    }
    (unit as u64, suffix)
}

fn to_str(size: u64, suffixes: &[&str], base: f64, precision: usize) -> String {
    if size == 1 {
        return "1 byte".to_string();
    }
    if (size as f64) < base {
        return format!("{size} bytes");
    }
    let size_f = size as f64;
    // `enumerate` from 2 in upstream: unit = base**(i+2).
    let mut unit = base;
    let mut suffix = suffixes[0];
    for (index, candidate) in suffixes.iter().enumerate() {
        unit = base.powi(index as i32 + 2);
        suffix = candidate;
        if size_f < unit {
            break;
        }
    }
    let value = base * size_f / unit;
    format!("{value:.precision$} {suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_upstream_reference_values() {
        // Captured from real rich 15.0.0 `filesize.decimal`.
        assert_eq!(decimal(0), "0 bytes");
        assert_eq!(decimal(1), "1 byte");
        assert_eq!(decimal(500), "500 bytes");
        assert_eq!(decimal(999), "999 bytes");
        assert_eq!(decimal(1000), "1.0 kB");
        assert_eq!(decimal(1500), "1.5 kB");
        assert_eq!(decimal(1024), "1.0 kB");
        assert_eq!(decimal(1_000_000), "1.0 MB");
        assert_eq!(decimal(1_500_000_000), "1.5 GB");
        assert_eq!(decimal(1_000_000_000_000_000_000), "1.0 EB");
    }

    #[test]
    fn negative_sizes_match_upstream() {
        // Captured from real rich 15.0.0 `filesize.decimal` /
        // `pick_unit_and_suffix` on negative ints.
        assert_eq!(decimal_signed(-1), "-1 bytes");
        assert_eq!(decimal_signed(-8), "-8 bytes");
        assert_eq!(decimal_signed(-1234), "-1,234 bytes");
        assert_eq!(decimal_signed(-1_234_567), "-1,234,567 bytes");
        assert_eq!(decimal_signed(1), "1 byte");
        assert_eq!(decimal_signed(1500), "1.5 kB");
        let suffixes = &["bytes", "kB", "MB"];
        assert_eq!(
            pick_unit_and_suffix_signed(-5000, suffixes, 1000),
            (1, "bytes")
        );
        assert_eq!(
            pick_unit_and_suffix_signed(5000, suffixes, 1000),
            (1000, "kB")
        );
    }

    #[test]
    fn pick_unit_matches_upstream() {
        // Captured from real rich 15.0.0 `filesize.pick_unit_and_suffix`.
        let suffixes = &["bytes", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
        assert_eq!(pick_unit_and_suffix(999, suffixes, 1000), (1, "bytes"));
        assert_eq!(pick_unit_and_suffix(1000, suffixes, 1000), (1000, "kB"));
        assert_eq!(pick_unit_and_suffix(1024, suffixes, 1000), (1000, "kB"));
        assert_eq!(
            pick_unit_and_suffix(3_000_000, suffixes, 1000),
            (1_000_000, "MB")
        );
        assert_eq!(
            pick_unit_and_suffix(10_000_000_000, suffixes, 1000),
            (1_000_000_000, "GB")
        );
    }
}
