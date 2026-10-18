//! What charts say to a screen reader: the pieces every chart's
//! [`AccessibleText`](crate::a11y::AccessibleText) is written from.
//!
//! A chart's accessible text is a few short lines of plain words: what it
//! is and how much it holds, the ranges of its axes, then its key values
//! (a series' minimum, maximum, last value and trend; the largest bars; a
//! gauge's place on its scale). Values are written in the chart's own
//! [`ValueFormat`], counts with thousands separators, and nothing depends
//! on the width or the console, so the same chart always reads the same.
//!
//! The **trend** of a series is the least-squares line through its finite
//! points: `up` or `down` when that line rises or falls by at least a tenth
//! of the values' range across the series, `flat` otherwise. A series of
//! fewer than two points has none.

use super::ValueFormat;

/// The share of a series' range its fitted line must rise or fall by to be
/// a trend rather than flat.
const TREND_SHARE: f64 = 0.1;

/// `n` with thousands separators.
pub(crate) fn count(n: usize) -> String {
    crate::format::number(i64::try_from(n).unwrap_or(i64::MAX))
}

/// `n` and the noun, singular for one: `1 value`, `1,000 values`.
pub(crate) fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", count(n), if n == 1 { one } else { many })
}

/// `from 2 to 9`, or `at 5` when both ends are the same.
pub(crate) fn range(lo: f64, hi: f64, format: ValueFormat) -> String {
    let (a, b) = (format.format(lo), format.format(hi));
    if a == b {
        format!("at {a}")
    } else {
        format!("from {a} to {b}")
    }
}

/// The smallest and largest finite value, if any.
pub(crate) fn extent(values: impl IntoIterator<Item = f64>) -> Option<(f64, f64)> {
    values
        .into_iter()
        .filter(|v| v.is_finite())
        .fold(None, |acc, v| match acc {
            None => Some((v, v)),
            Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
        })
}

/// Which way a series goes; see the [module docs](self).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Trend {
    Up,
    Down,
    Flat,
}

impl Trend {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Trend::Up => "up",
            Trend::Down => "down",
            Trend::Flat => "flat",
        }
    }
}

/// The trend of the finite `(x, y)` points.
pub(crate) fn trend(points: &[(f64, f64)]) -> Option<Trend> {
    let finite: Vec<(f64, f64)> = points
        .iter()
        .copied()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect();
    if finite.len() < 2 {
        return None;
    }
    let n = finite.len() as f64;
    let mx = finite.iter().map(|p| p.0).sum::<f64>() / n;
    let my = finite.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = finite.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = finite.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let (x_lo, x_hi) = extent(finite.iter().map(|p| p.0))?;
    let (y_lo, y_hi) = extent(finite.iter().map(|p| p.1))?;
    if sxx == 0.0 || y_hi == y_lo || !(sxx.is_finite() && sxy.is_finite()) {
        return Some(Trend::Flat);
    }
    let rise = sxy / sxx * (x_hi - x_lo);
    let share = rise / (y_hi - y_lo);
    Some(if !share.is_finite() || share.abs() < TREND_SHARE {
        Trend::Flat
    } else if share > 0.0 {
        Trend::Up
    } else {
        Trend::Down
    })
}

/// A series' facts: `24 points, min 12, max 52, last 14, trend up`, with
/// `missing` gaps counted, in `format`. `noun` names one point (`value`,
/// `point`).
pub(crate) fn series(points: &[(f64, f64)], format: ValueFormat, noun: (&str, &str)) -> String {
    let finite = points
        .iter()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .count();
    let missing = points.len() - finite;
    let mut parts = vec![counted(points.len(), noun.0, noun.1)];
    if missing > 0 {
        parts.push(format!("{} missing", count(missing)));
    }
    let ys = points
        .iter()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .map(|p| p.1);
    match extent(ys) {
        Some((lo, hi)) => {
            parts.push(format!("min {}", format.format(lo)));
            parts.push(format!("max {}", format.format(hi)));
            let last = points
                .iter()
                .rev()
                .find(|(x, y)| x.is_finite() && y.is_finite())
                .map_or(f64::NAN, |p| p.1);
            parts.push(format!("last {}", format.format(last)));
            if let Some(trend) = trend(points) {
                parts.push(format!("trend {}", trend.word()));
            }
        }
        None if !points.is_empty() => parts.push("no values to show".to_string()),
        None => {}
    }
    parts.join(", ")
}

/// Values at x = 0, 1, 2, …
pub(crate) fn indexed(values: &[f64]) -> Vec<(f64, f64)> {
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| (i as f64, v))
        .collect()
}

/// `name value` entries as a list: every one in order when there are at
/// most `all` of them; otherwise the three largest (`Largest: …`) and the
/// smallest (`smallest …`). Entries without a finite value are left out of
/// the ranking.
pub(crate) fn ranked(entries: &[(String, String, f64)], all: usize) -> Vec<String> {
    let entry = |(name, text, _): &(String, String, f64)| {
        if name.is_empty() {
            text.clone()
        } else {
            format!("{name} {text}")
        }
    };
    if entries.len() <= all {
        return vec![entries.iter().map(entry).collect::<Vec<_>>().join(", ")];
    }
    let mut order: Vec<usize> = (0..entries.len())
        .filter(|&i| entries[i].2.is_finite())
        .collect();
    // Largest first; ties keep their order.
    order.sort_by(|&a, &b| entries[b].2.total_cmp(&entries[a].2));
    if order.is_empty() {
        return Vec::new();
    }
    let top: Vec<String> = order.iter().take(3).map(|&i| entry(&entries[i])).collect();
    let mut out = vec![format!("Largest: {}", top.join(", "))];
    if order.len() > 3 {
        let low = *order.last().expect("not empty");
        out.push(format!("Smallest: {}", entry(&entries[low])));
    }
    out
}

/// At most `cap` items joined with `, `, then `and N more`.
pub(crate) fn capped(items: &[String], cap: usize) -> String {
    let mut shown: Vec<String> = items.iter().take(cap).cloned().collect();
    if items.len() > cap {
        shown.push(format!("and {} more", count(items.len() - cap)));
    }
    shown.join(", ")
}

/// `percent` (0 to 100) as a whole number of percent, in words.
pub(crate) fn percent(ratio: f64) -> String {
    if !ratio.is_finite() {
        return "no value".to_string();
    }
    format!("{:.0} percent", (ratio.clamp(0.0, 1.0) * 100.0).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trends_follow_the_fitted_line() {
        let up = indexed(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(trend(&up), Some(Trend::Up));
        let down = indexed(&[4.0, 3.0, 2.0, 1.0]);
        assert_eq!(trend(&down), Some(Trend::Down));
        // A hump that ends where it began is flat.
        let hump = indexed(&[1.0, 5.0, 9.0, 5.0, 1.0]);
        assert_eq!(trend(&hump), Some(Trend::Flat));
        assert_eq!(trend(&indexed(&[3.0, 3.0, 3.0])), Some(Trend::Flat));
        assert_eq!(trend(&indexed(&[3.0])), None);
        assert_eq!(trend(&indexed(&[f64::NAN, 2.0, f64::NAN])), None);
    }

    #[test]
    fn series_facts_count_gaps() {
        let points = indexed(&[2.0, f64::NAN, 9.0, 5.0]);
        assert_eq!(
            series(&points, ValueFormat::Compact, ("value", "values")),
            "4 values, 1 missing, min 2, max 9, last 5, trend up"
        );
        assert_eq!(
            series(&[], ValueFormat::Compact, ("value", "values")),
            "0 values"
        );
        assert_eq!(
            series(
                &indexed(&[f64::NAN]),
                ValueFormat::Compact,
                ("value", "values")
            ),
            "1 value, 1 missing, no values to show"
        );
    }

    #[test]
    fn rankings_list_few_and_rank_many() {
        let e = |n: &str, v: f64| (n.to_string(), ValueFormat::Compact.format(v), v);
        let few = [e("a", 1.0), e("b", 3.0)];
        assert_eq!(ranked(&few, 6), vec!["a 1, b 3"]);
        let many = [
            e("a", 1.0),
            e("b", 3.0),
            e("c", 2.0),
            e("d", 5.0),
            e("e", 0.5),
        ];
        assert_eq!(
            ranked(&many, 3),
            vec!["Largest: d 5, b 3, c 2", "Smallest: e 0.5"]
        );
    }
}
