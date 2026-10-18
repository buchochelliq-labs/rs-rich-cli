//! Regression comparisons (#706): two build-time or size reports compared
//! unit by unit, with what grew past a threshold flagged.
//!
//! A unit is matched by name: a timing unit by its [label](super::timings::Unit::label)
//! (`syn`, `app build-script (run)`), a size entry by its
//! [key](super::sizes::SizeEntry::key) (`syn`, `app (bin)`). A name that
//! appears more than once on one side (a crate built at two versions) is
//! added up, so a duplicate dropped reads as a smaller unit rather than as a
//! removed one.
//!
//! [`Thresholds`] say what counts as a change: a unit **regresses** when it
//! grows by more than the percentage *and* by more than the absolute floor,
//! and improves when it shrinks by both. The floor keeps noise on small
//! units from counting: by default 10% and 0.5 s for build times, 10% and
//! 64 KiB for sizes ([`Thresholds::TIMINGS`], [`Thresholds::SIZES`]). A new
//! unit regresses when it is larger than the floor (its percentage has no
//! meaning); a removed one is listed as removed. The totals are compared
//! with the same thresholds.
//!
//! [`Comparison`] draws the counts, then a table of the units, regressions
//! first, with the totals underneath; [`Comparison::regressions`] lists the
//! units that regressed, for a gate.
//!
//! ```
//! use rich_ext::deps::compare::{Comparison, Quantity, Thresholds, Verdict};
//!
//! let old = [("syn".to_string(), 4.0), ("serde".to_string(), 2.0)];
//! let new = [("syn".to_string(), 5.0), ("serde".to_string(), 2.1)];
//! let comparison = Comparison::new(Quantity::Seconds, old, new, Thresholds::TIMINGS);
//! let regressed: Vec<&str> = comparison.regressions().iter().map(|r| r.key.as_str()).collect();
//! assert_eq!(regressed, ["syn"]);
//! assert_eq!(comparison.rows()[1].verdict, Verdict::Unchanged);
//! ```

use std::collections::HashMap;

use rich::table::Table;
use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Style, Text};

use super::sizes::Sizes;
use super::timings::Timings;
use super::{clean, stack, stack_measure, theme_style, DepsError};
use crate::format;

/// What the reports measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantity {
    /// Seconds of build time.
    Seconds,
    /// Bytes.
    Bytes,
}

impl Quantity {
    /// `value` as the reports write it: `2.31s`, `1.5 MiB`.
    pub fn format(self, value: f64) -> String {
        match self {
            Quantity::Seconds => format!("{value:.2}s"),
            Quantity::Bytes => format::bytes_binary(value.max(0.0).round() as u64),
        }
    }

    /// A change, signed: `+0.42s`, `-120.0 KiB`; no change has no sign.
    pub fn format_change(self, change: f64) -> String {
        let size = self.format(change.abs());
        if size == self.format(0.0) {
            return size;
        }
        let sign = if change < 0.0 { "-" } else { "+" };
        format!("{sign}{size}")
    }
}

/// `+12.5%`, `-3.0%`, `0.0%`.
fn format_percent(percent: f64) -> String {
    let text = format!("{percent:+.1}%");
    if text == "+0.0%" || text == "-0.0%" {
        "0.0%".into()
    } else {
        text
    }
}

/// One `--threshold` value: a percentage, a time or a size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Threshold {
    /// `10%`, or a bare number.
    Percent(f64),
    /// `0.5s`, `500ms`.
    Seconds(f64),
    /// `100KiB`, `1MB`, `512B`.
    Bytes(u64),
}

impl Threshold {
    /// Read a threshold: a percentage (`10%`, or a bare `10`), a time
    /// (`0.5s`, `500ms`, `2min`) or a size (`512B`, `100KiB`, `2MiB`,
    /// `100kB`, `1MB`, and `K`, `M`, `G` as binary units). Units ignore
    /// case.
    pub fn parse(text: &str) -> Result<Self, DepsError> {
        let invalid = || {
            DepsError::new(format!(
                "{:?} is not a threshold: give a percentage (10%), a time (0.5s) or a size \
                 (100KiB)",
                clean(text)
            ))
        };
        let trimmed = text.trim();
        let split = trimmed
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(trimmed.len());
        let (number, unit) = trimmed.split_at(split);
        let value: f64 = number.parse().map_err(|_| invalid())?;
        if !value.is_finite() || value < 0.0 {
            return Err(invalid());
        }
        let unit = unit.trim().to_ascii_lowercase();
        let bytes = |scale: f64| {
            let bytes = value * scale;
            if bytes > u64::MAX as f64 {
                Err(invalid())
            } else {
                Ok(Threshold::Bytes(bytes.round() as u64))
            }
        };
        match unit.as_str() {
            "" | "%" => Ok(Threshold::Percent(value)),
            "s" | "sec" | "secs" => Ok(Threshold::Seconds(value)),
            "ms" => Ok(Threshold::Seconds(value / 1000.0)),
            "min" => Ok(Threshold::Seconds(value * 60.0)),
            "b" | "byte" | "bytes" => bytes(1.0),
            "k" | "kib" => bytes(1024.0),
            "kb" => bytes(1000.0),
            "m" | "mib" => bytes(1024.0 * 1024.0),
            "mb" => bytes(1e6),
            "g" | "gib" => bytes(1024.0 * 1024.0 * 1024.0),
            "gb" => bytes(1e9),
            _ => Err(invalid()),
        }
    }
}

/// What counts as a change. See the [module docs](self).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    /// The percentage of its baseline a unit must change by.
    pub percent: f64,
    /// The amount (seconds or bytes) it must also change by.
    pub absolute: f64,
}

impl Thresholds {
    /// Build times: 10% and half a second.
    pub const TIMINGS: Thresholds = Thresholds {
        percent: 10.0,
        absolute: 0.5,
    };

    /// Sizes: 10% and 64 KiB.
    pub const SIZES: Thresholds = Thresholds {
        percent: 10.0,
        absolute: 64.0 * 1024.0,
    };

    /// The defaults for `quantity`.
    pub fn defaults(quantity: Quantity) -> Self {
        match quantity {
            Quantity::Seconds => Thresholds::TIMINGS,
            Quantity::Bytes => Thresholds::SIZES,
        }
    }

    /// These thresholds with `threshold` in place of the percentage or the
    /// floor; a time for sizes, or a size for times, is an error.
    pub fn with(mut self, threshold: Threshold, quantity: Quantity) -> Result<Self, DepsError> {
        match (threshold, quantity) {
            (Threshold::Percent(percent), _) => self.percent = percent,
            (Threshold::Seconds(seconds), Quantity::Seconds) => self.absolute = seconds,
            (Threshold::Bytes(bytes), Quantity::Bytes) => self.absolute = bytes as f64,
            (Threshold::Seconds(_), Quantity::Bytes) => {
                return Err(DepsError::new(
                    "a time threshold applies to build times, not sizes",
                ))
            }
            (Threshold::Bytes(_), Quantity::Seconds) => {
                return Err(DepsError::new(
                    "a size threshold applies to sizes, not build times",
                ))
            }
        }
        Ok(self)
    }

    /// The verdict on a unit that went from `baseline` to `current`.
    pub fn verdict(&self, baseline: Option<f64>, current: Option<f64>) -> Verdict {
        match (baseline, current) {
            (None, Some(current)) if current > self.absolute => Verdict::Regression,
            (None, Some(_)) => Verdict::New,
            (Some(_), None) => Verdict::Removed,
            (None, None) => Verdict::Unchanged,
            (Some(baseline), Some(current)) => {
                let change = current - baseline;
                let percent = if baseline > 0.0 {
                    change / baseline * 100.0
                } else if change > 0.0 {
                    f64::INFINITY
                } else {
                    0.0
                };
                if change > self.absolute && percent > self.percent {
                    Verdict::Regression
                } else if -change > self.absolute && -percent > self.percent {
                    Verdict::Improvement
                } else {
                    Verdict::Unchanged
                }
            }
        }
    }

    /// `over 10% and over 0.50s`.
    pub fn describe(&self, quantity: Quantity) -> String {
        format!(
            "over {}% and over {}",
            trim_number(self.percent),
            quantity.format(self.absolute)
        )
    }
}

/// `10` for 10.0, `2.5` for 2.5.
fn trim_number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// What happened to a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// It grew past the thresholds (or is new and larger than the floor).
    Regression,
    /// It is new, and no larger than the floor.
    New,
    /// It shrank past the thresholds.
    Improvement,
    /// It is gone.
    Removed,
    /// It changed less than the thresholds.
    Unchanged,
}

impl Verdict {
    /// The word for it.
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Regression => "regression",
            Verdict::New => "new",
            Verdict::Improvement => "improvement",
            Verdict::Removed => "removed",
            Verdict::Unchanged => "unchanged",
        }
    }

    fn style(self, console: &Console) -> Style {
        theme_style(
            console,
            match self {
                Verdict::Regression => "deps.regression",
                Verdict::Improvement => "deps.improvement",
                Verdict::New | Verdict::Removed => "deps.new",
                Verdict::Unchanged => "deps.off",
            },
        )
    }
}

/// One unit in both reports.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    pub baseline: Option<f64>,
    pub current: Option<f64>,
    pub verdict: Verdict,
}

impl Row {
    /// How much it changed: what was added, or taken away.
    pub fn change(&self) -> f64 {
        self.current.unwrap_or(0.0) - self.baseline.unwrap_or(0.0)
    }

    /// The change as a percentage of the baseline, when there is one.
    pub fn percent(&self) -> Option<f64> {
        let baseline = self.baseline.filter(|b| *b > 0.0)?;
        Some((self.current? - baseline) / baseline * 100.0)
    }
}

/// Values by name, each name's values added up, in first-seen order.
fn summed(values: impl IntoIterator<Item = (String, f64)>) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (key, value) in values {
        if !value.is_finite() {
            continue;
        }
        match index.get(&key) {
            Some(&at) => out[at].1 += value,
            None => {
                index.insert(key.clone(), out.len());
                out.push((key, value));
            }
        }
    }
    out
}

/// Two reports compared. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Comparison {
    quantity: Quantity,
    thresholds: Thresholds,
    rows: Vec<Row>,
    total: Row,
    names: Option<(String, String)>,
    limit: Option<usize>,
}

impl Comparison {
    /// Compare `current` with `baseline`, each a list of (unit, value).
    pub fn new(
        quantity: Quantity,
        baseline: impl IntoIterator<Item = (String, f64)>,
        current: impl IntoIterator<Item = (String, f64)>,
        thresholds: Thresholds,
    ) -> Self {
        let baseline = summed(baseline);
        let current = summed(current);
        let old: HashMap<&str, f64> = baseline.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let new: HashMap<&str, f64> = current.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        let mut rows: Vec<Row> = Vec::new();
        for (key, value) in &baseline {
            let now = new.get(key.as_str()).copied();
            rows.push(Row {
                key: key.clone(),
                baseline: Some(*value),
                current: now,
                verdict: thresholds.verdict(Some(*value), now),
            });
        }
        for (key, value) in &current {
            if !old.contains_key(key.as_str()) {
                rows.push(Row {
                    key: key.clone(),
                    baseline: None,
                    current: Some(*value),
                    verdict: thresholds.verdict(None, Some(*value)),
                });
            }
        }
        // Regressions first, then by how much each changed, then by name.
        rows.sort_by(|a, b| {
            a.verdict
                .cmp(&b.verdict)
                .then_with(|| b.change().abs().total_cmp(&a.change().abs()))
                .then_with(|| a.key.cmp(&b.key))
        });
        let sum = |list: &[(String, f64)]| list.iter().map(|(_, v)| v).sum::<f64>();
        let (old_total, new_total) = (sum(&baseline), sum(&current));
        let total = Row {
            key: "total".into(),
            baseline: Some(old_total),
            current: Some(new_total),
            verdict: thresholds.verdict(Some(old_total), Some(new_total)),
        };
        Comparison {
            quantity,
            thresholds,
            rows,
            total,
            names: None,
            limit: None,
        }
    }

    /// Two build-time reports, unit by unit.
    pub fn timings(baseline: &Timings, current: &Timings, thresholds: Thresholds) -> Self {
        let values = |timings: &Timings| -> Vec<(String, f64)> {
            timings
                .units()
                .iter()
                .map(|unit| (unit.label(), unit.duration))
                .collect()
        };
        Comparison::new(
            Quantity::Seconds,
            values(baseline),
            values(current),
            thresholds,
        )
    }

    /// Two size reports, entry by entry.
    pub fn sizes(baseline: &Sizes, current: &Sizes, thresholds: Thresholds) -> Self {
        let values = |sizes: &Sizes| -> Vec<(String, f64)> {
            sizes
                .entries()
                .iter()
                .map(|entry| (entry.key(), entry.bytes as f64))
                .collect()
        };
        Comparison::new(
            Quantity::Bytes,
            values(baseline),
            values(current),
            thresholds,
        )
    }

    /// Name the two reports in the heading (their files, say).
    pub fn names(mut self, baseline: impl Into<String>, current: impl Into<String>) -> Self {
        self.names = Some((baseline.into(), current.into()));
        self
    }

    /// Show at most `limit` units (regressions first); the rest are
    /// counted underneath.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit.max(1));
        self
    }

    pub fn quantity(&self) -> Quantity {
        self.quantity
    }

    pub fn thresholds(&self) -> Thresholds {
        self.thresholds
    }

    /// Every unit, regressions first.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The totals of both reports.
    pub fn total(&self) -> &Row {
        &self.total
    }

    /// The units that regressed.
    pub fn regressions(&self) -> Vec<&Row> {
        self.rows
            .iter()
            .filter(|row| row.verdict == Verdict::Regression)
            .collect()
    }

    /// How many units have `verdict`.
    pub fn count(&self, verdict: Verdict) -> usize {
        self.rows
            .iter()
            .filter(|row| row.verdict == verdict)
            .count()
    }

    /// Whether a unit, or the total, regressed: what a gate fails on.
    pub fn failed(&self) -> bool {
        self.total.verdict == Verdict::Regression || !self.regressions().is_empty()
    }

    fn heading(&self, console: &Console) -> Text {
        let what = match self.quantity {
            Quantity::Seconds => "Build times",
            Quantity::Bytes => "Sizes",
        };
        let mut line = format!("{what} against the baseline");
        if let Some((old, new)) = &self.names {
            line.push_str(&format!(" ({} → {})", clean(old), clean(new)));
        }
        let mut text = Text::styled(line, theme_style(console, "deps.root"));
        text.append("\n", None);
        let counts = [
            Verdict::Regression,
            Verdict::Improvement,
            Verdict::Unchanged,
            Verdict::New,
            Verdict::Removed,
        ];
        for (index, verdict) in counts.into_iter().enumerate() {
            let n = self.count(verdict);
            if index > 0 {
                text.append(", ", None);
            }
            let word = match (verdict, n) {
                (Verdict::Regression | Verdict::Improvement, 1) => verdict.name().to_string(),
                (Verdict::Regression | Verdict::Improvement, _) => format!("{}s", verdict.name()),
                _ => verdict.name().to_string(),
            };
            let style = if n > 0 && verdict != Verdict::Unchanged {
                Some(verdict.style(console).into())
            } else {
                None
            };
            text.append(&format!("{n} {word}"), style);
        }
        let total = &self.total;
        text.append(
            &format!(
                "\ntotal {} → {}: {}",
                self.quantity.format(total.baseline.unwrap_or(0.0)),
                self.quantity.format(total.current.unwrap_or(0.0)),
                self.quantity.format_change(total.change())
            ),
            None,
        );
        if let Some(percent) = total.percent() {
            text.append(&format!(" ({})", format_percent(percent)), None);
        }
        if matches!(total.verdict, Verdict::Regression | Verdict::Improvement) {
            text.append(", ", None);
            text.append(
                &format!(
                    "a{} {}",
                    if total.verdict == Verdict::Improvement {
                        "n"
                    } else {
                        ""
                    },
                    total.verdict.name()
                ),
                Some(total.verdict.style(console).into()),
            );
        }
        text.append(
            &format!("\nregression: {}", self.thresholds.describe(self.quantity)),
            Some(theme_style(console, "deps.section").into()),
        );
        text
    }

    fn table(&self, console: &Console) -> Table {
        let mut table = Table::new();
        table.add_column("Unit");
        for header in ["Baseline", "Current", "Change", "%"] {
            table.add_column_justify(header, Justify::Right);
        }
        table.add_column("Verdict");
        let none = || Text::styled("–", theme_style(console, "deps.off"));
        let value = |value: Option<f64>| {
            value.map_or_else(none, |value| Text::new(self.quantity.format(value)))
        };
        let row_text = |row: &Row| {
            let style = row.verdict.style(console);
            let percent = match (row.percent(), row.verdict) {
                (Some(percent), _) => format_percent(percent),
                (None, Verdict::Removed) => "removed".into(),
                (None, _) if row.baseline.is_none() => "new".into(),
                (None, _) => "–".into(),
            };
            vec![
                Text::new(clean(&row.key).into_owned()),
                value(row.baseline),
                value(row.current),
                Text::styled(self.quantity.format_change(row.change()), style.clone()),
                Text::styled(percent, style.clone()),
                Text::styled(row.verdict.name(), style),
            ]
        };
        let limit = self.limit.unwrap_or(usize::MAX).min(self.rows.len());
        let (shown, rest) = self.rows.split_at(limit);
        for row in shown {
            table.add_row_text(row_text(row));
        }
        if !rest.is_empty() {
            table.add_section();
            let mut cells = vec![Text::styled(
                format!("and {} more", rest.len()),
                theme_style(console, "deps.off"),
            )];
            cells.extend((0..5).map(|_| Text::new("")));
            table.add_row_text(cells);
        }
        table.add_section();
        let mut total = row_text(&self.total);
        total[0] = Text::styled("total", theme_style(console, "deps.root"));
        table.add_row_text(total);
        table
    }
}

impl Renderable for Comparison {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (heading, table) = (self.heading(console), self.table(console));
        stack(&[&heading, &table], console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let (heading, table) = (self.heading(console), self.table(console));
        stack_measure(&[&heading, &table], console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_parse_with_units() {
        assert_eq!(Threshold::parse("10%").unwrap(), Threshold::Percent(10.0));
        assert_eq!(Threshold::parse("2.5").unwrap(), Threshold::Percent(2.5));
        assert_eq!(Threshold::parse("0.5s").unwrap(), Threshold::Seconds(0.5));
        assert_eq!(Threshold::parse("250ms").unwrap(), Threshold::Seconds(0.25));
        assert_eq!(
            Threshold::parse("100KiB").unwrap(),
            Threshold::Bytes(102_400)
        );
        assert_eq!(
            Threshold::parse("100kb").unwrap(),
            Threshold::Bytes(100_000)
        );
        assert_eq!(Threshold::parse("2M").unwrap(), Threshold::Bytes(2 << 20));
        assert_eq!(
            Threshold::parse("1 MB").unwrap(),
            Threshold::Bytes(1_000_000)
        );
        assert_eq!(Threshold::parse("0B").unwrap(), Threshold::Bytes(0));
        for bad in [
            "",
            "%",
            "-1%",
            "abc",
            "10 parsecs",
            "1e400",
            "NaN",
            "inf",
            "1.2.3s",
        ] {
            assert!(Threshold::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_threshold_must_fit_the_quantity() {
        let sizes = Thresholds::SIZES;
        assert!(sizes
            .with(Threshold::Seconds(1.0), Quantity::Bytes)
            .is_err());
        assert!(Thresholds::TIMINGS
            .with(Threshold::Bytes(1), Quantity::Seconds)
            .is_err());
        let set = sizes
            .with(Threshold::Percent(5.0), Quantity::Bytes)
            .unwrap()
            .with(Threshold::Bytes(1024), Quantity::Bytes)
            .unwrap();
        assert_eq!((set.percent, set.absolute), (5.0, 1024.0));
    }

    #[test]
    fn verdicts_need_both_the_percentage_and_the_floor() {
        let t = Thresholds {
            percent: 10.0,
            absolute: 1.0,
        };
        // +50% but only +0.5: under the floor.
        assert_eq!(t.verdict(Some(1.0), Some(1.5)), Verdict::Unchanged);
        // +2 but only +5%: under the percentage.
        assert_eq!(t.verdict(Some(40.0), Some(42.0)), Verdict::Unchanged);
        assert_eq!(t.verdict(Some(10.0), Some(12.0)), Verdict::Regression);
        assert_eq!(t.verdict(Some(10.0), Some(8.0)), Verdict::Improvement);
        // From nothing: any growth past the floor.
        assert_eq!(t.verdict(Some(0.0), Some(2.0)), Verdict::Regression);
        assert_eq!(t.verdict(None, Some(2.0)), Verdict::Regression);
        assert_eq!(t.verdict(None, Some(0.5)), Verdict::New);
        assert_eq!(t.verdict(Some(3.0), None), Verdict::Removed);
        // Exactly at the threshold is not over it.
        assert_eq!(t.verdict(Some(10.0), Some(11.0)), Verdict::Unchanged);
    }

    #[test]
    fn duplicate_names_add_up_and_rows_sort_regressions_first() {
        let old = vec![
            ("syn".to_string(), 3.0),
            ("syn".to_string(), 2.0),
            ("gone".to_string(), 1.0),
            ("same".to_string(), 1.0),
        ];
        let new = vec![
            ("syn".to_string(), 3.0),
            ("same".to_string(), 1.0),
            ("fresh".to_string(), 0.1),
            ("big".to_string(), 9.0),
        ];
        let c = Comparison::new(Quantity::Seconds, old, new, Thresholds::TIMINGS);
        let order: Vec<(&str, Verdict)> = c
            .rows()
            .iter()
            .map(|r| (r.key.as_str(), r.verdict))
            .collect();
        assert_eq!(
            order,
            [
                ("big", Verdict::Regression),
                ("fresh", Verdict::New),
                ("syn", Verdict::Improvement),
                ("gone", Verdict::Removed),
                ("same", Verdict::Unchanged),
            ]
        );
        assert_eq!(c.total().baseline, Some(7.0));
        assert_eq!(c.total().current, Some(13.1));
        assert!(c.failed());
        assert_eq!(c.rows()[2].percent().map(|p| p.round()), Some(-40.0));
    }
}
