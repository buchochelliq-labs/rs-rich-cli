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
