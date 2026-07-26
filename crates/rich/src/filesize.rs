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
}
