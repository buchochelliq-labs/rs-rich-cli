//! Column statistics (#261): count, nulls, distinct values, min, max, mean,
//! median, quantiles and the most common values, per column.
//!
//! A column is numeric when every non-null cell is a number, or text that
//! parses as one ([`infer::parse_float`](crate::infer::parse_float)); its
//! min and max are numeric, and it gets a mean, a median and quantiles
//! (linear interpolation between closest ranks, as NumPy's default). Any
//! other column's min and max compare the cells' text. Nulls are
//! [`Value::Null`] and empty text; run [`Inference::apply`] first for other
//! null tokens.
//!
//! [`Stats`] renders as a table, a row per column; [`Stats::headers`] gives
//! a short summary to show under each column heading instead.
//!
//! [`Inference::apply`]: crate::infer::Inference::apply
//!
//! ```
//! use rich_data::stats::Stats;
//! use rich_data::{Rows, Value};
//!
//! let mut rows = Rows::new(["service", "p99"]);
//! for (service, p99) in [("web", 120), ("api", 35), ("web", 80), ("db", 5)] {
//!     rows.push([service.into(), Value::Int(p99)]);
//! }
//! let stats = Stats::of(&rows);
//! let p99 = &stats.columns()[1];
//! assert_eq!((p99.count, p99.nulls, p99.distinct), (4, 0, 4));
//! assert_eq!(p99.mean, Some(60.0));
//! assert_eq!(p99.median, Some(57.5));
//! let service = &stats.columns()[0];
//! assert_eq!(service.top[0], ("web".to_string(), 2));
//! assert_eq!(service.min.as_deref(), Some("api"));
//! ```

use std::collections::HashMap;

use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Table, Text};

use crate::infer::{parse_float, parse_int};
use crate::{Rows, Value};

/// What [`Stats`] computes beyond the counts.
#[derive(Clone, Debug, PartialEq)]
pub struct StatsOptions {
    /// The quantiles to compute, each in `0.0..=1.0` (default the quartiles
    /// 0.25 and 0.75; the median is always computed).
    pub quantiles: Vec<f64>,
    /// How many of the most common values to keep (default 3).
    pub top: usize,
}

impl Default for StatsOptions {
    fn default() -> Self {
        StatsOptions {
            quantiles: vec![0.25, 0.75],
            top: 3,
        }
    }
}

/// One column's statistics.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnStats {
    /// The column name.
    pub name: String,
    /// Non-null cells.
    pub count: usize,
    /// Null or empty cells.
    pub nulls: usize,
    /// Distinct non-null values (by their text).
    pub distinct: usize,
    /// Whether every non-null cell is a number.
    pub numeric: bool,
    /// The smallest value, as text (numerically for a numeric column).
    pub min: Option<String>,
    /// The largest value, as text.
    pub max: Option<String>,
    /// The mean, for a numeric column.
    pub mean: Option<f64>,
    /// The median, for a numeric column.
    pub median: Option<f64>,
    /// Each requested quantile and its value, for a numeric column.
    pub quantiles: Vec<(f64, f64)>,
    /// The most common values and their counts, most common first (ties in
    /// text order).
    pub top: Vec<(String, usize)>,
}

impl ColumnStats {
    /// A one-line summary: `n=4 · min 5 · max 120 · mean 60`, or for text
    /// `n=4 · 3 distinct · top web (2)`, with `· 1 null` when there are any.
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("n={}", self.count)];
        if self.nulls > 0 {
            parts.push(format!("{} null", self.nulls));
        }
        if self.numeric {
            if let (Some(min), Some(max)) = (&self.min, &self.max) {
                parts.push(format!("min {min}"));
                parts.push(format!("max {max}"));
            }
            if let Some(mean) = self.mean {
                parts.push(format!("mean {}", number(mean)));
            }
        } else {
            parts.push(format!("{} distinct", self.distinct));
            if let Some((value, count)) = self.top.first() {
                parts.push(format!("top {value} ({count})"));
            }
        }
        parts.join(" · ")
    }
}

/// Statistics for every column. See the [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    columns: Vec<ColumnStats>,
    quantiles: Vec<f64>,
}

impl Stats {
    /// Statistics for `rows` with the default options.
    pub fn of(rows: &Rows) -> Self {
        Stats::with_options(rows, &StatsOptions::default())
    }

    /// Statistics for `rows`.
    pub fn with_options(rows: &Rows, options: &StatsOptions) -> Self {
        let quantiles: Vec<f64> = options
            .quantiles
            .iter()
            .copied()
            .filter(|q| (0.0..=1.0).contains(q))
            .collect();
        let columns = rows
            .columns()
            .iter()
            .enumerate()
            .map(|(index, name)| column(name, rows.column(index), &quantiles, options.top))
            .collect();
        Stats { columns, quantiles }
    }

    /// One entry per column, in order.
    pub fn columns(&self) -> &[ColumnStats] {
        &self.columns
    }

    /// Each column's name with its [`summary`](ColumnStats::summary) on a
    /// second line, to use as table headings.
    pub fn headers(&self) -> Vec<String> {
        self.columns
            .iter()
            .map(|c| format!("{}\n{}", c.name, c.summary()))
            .collect()
    }

    /// The table this renders as: a row per column with count, nulls,
    /// distinct, min, max, mean, median, the quantiles and the top values.
    pub fn to_table(&self) -> Table {
        let mut table = Table::new();
        table.add_column("column");
        for heading in ["count", "nulls", "distinct", "min", "max", "mean", "median"] {
            table.add_column_justify(heading, Justify::Right);
        }
        for q in &self.quantiles {
            table.add_column_justify(format!("p{}", number(q * 100.0)), Justify::Right);
        }
        table.add_column("top");
        for c in &self.columns {
            let optional = |v: Option<f64>| v.map(number).unwrap_or_default();
            let mut cells = vec![
                c.name.clone(),
                c.count.to_string(),
                c.nulls.to_string(),
                c.distinct.to_string(),
                c.min.clone().unwrap_or_default(),
                c.max.clone().unwrap_or_default(),
                optional(c.mean),
                optional(c.median),
            ];
            for q in &self.quantiles {
                let value = c.quantiles.iter().find(|(at, _)| at == q).map(|(_, v)| *v);
                cells.push(optional(value));
            }
            let top: Vec<String> = c.top.iter().map(|(v, n)| format!("{v} ({n})")).collect();
            cells.push(top.join(", "));
            table.add_row_text(cells.into_iter().map(Text::new).collect());
        }
        table
    }
}

impl Renderable for Stats {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.to_table().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.to_table().measure(console, options)
    }
}

fn column<'a>(
    name: &str,
    values: impl Iterator<Item = &'a Value>,
    quantiles: &[f64],
    top: usize,
) -> ColumnStats {
    let mut nulls = 0;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut numbers: Vec<f64> = Vec::new();
    // Every value as an exact integer while they all are one, so integers
    // past 2^53 keep their order and their printed min and max.
    let mut integers: Option<Vec<i64>> = Some(Vec::new());
    let mut numeric = true;
    for value in values {
        if value.is_empty() {
            nulls += 1;
            continue;
        }
        let text = value.plain();
        if numeric {
            let integer = match value {
                Value::Int(n) => Some(*n),
                Value::Float(_) => None,
                _ => parse_int(text.trim()),
            };
            match (integer, integers.as_mut()) {
                (Some(n), Some(exact)) => exact.push(n),
                _ => integers = None,
            }
            match value.as_f64().or_else(|| parse_float(text.trim())) {
                Some(n) if !n.is_nan() => numbers.push(n),
                _ => numeric = false,
            }
        }
        *counts.entry(text).or_insert(0) += 1;
    }
    let count: usize = counts.values().sum();
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|(a, m), (b, n)| n.cmp(m).then_with(|| a.cmp(b)));
    let distinct = ranked.len();
    let numeric = numeric && count > 0;
    let (min, max, mean, median, quantile_values) = if numeric && integers.is_some() {
        let mut exact = integers.unwrap_or_default();
        exact.sort_unstable();
        let sum: i128 = exact.iter().map(|&n| i128::from(n)).sum();
        (
            exact.first().map(i64::to_string),
            exact.last().map(i64::to_string),
            Some(sum as f64 / exact.len() as f64),
            Some(integer_quantile(&exact, 0.5)),
            quantiles
                .iter()
                .map(|&q| (q, integer_quantile(&exact, q)))
                .collect(),
        )
    } else if numeric {
        numbers.sort_by(f64::total_cmp);
        let n = numbers.len() as f64;
        let mut mean = numbers.iter().sum::<f64>() / n;
        if !mean.is_finite() && numbers.iter().all(|x| x.is_finite()) {
            // The sum overflowed; scaled first, it cannot.
            mean = numbers.iter().map(|x| x / n).sum();
        }
        (
            numbers.first().map(|n| number(*n)),
            numbers.last().map(|n| number(*n)),
            Some(mean),
            Some(quantile(&numbers, 0.5)),
            quantiles
                .iter()
                .map(|&q| (q, quantile(&numbers, q)))
                .collect(),
        )
    } else {
        let min = ranked.iter().map(|(v, _)| v).min().cloned();
        let max = ranked.iter().map(|(v, _)| v).max().cloned();
        (min, max, None, None, Vec::new())
    };
    ranked.truncate(top);
    ColumnStats {
        name: name.to_string(),
        count,
        nulls,
        distinct,
        numeric,
        min,
        max,
        mean,
        median,
        quantiles: quantile_values,
        top: ranked,
    }
}

/// The `q` quantile of sorted, non-empty `values`, interpolating linearly
/// between the closest ranks.
fn quantile(values: &[f64], q: f64) -> f64 {
    let position = q * (values.len() - 1) as f64;
    let below = position.floor() as usize;
    let above = position.ceil() as usize;
    if values[below] == values[above] {
        // An exact rank, or equal neighbours: no interpolation, so an
        // infinite value is itself rather than `inf - inf`'s NaN.
        return values[below];
    }
    values[below] + (values[above] - values[below]) * (position - below as f64)
}

/// [`quantile`] over sorted, non-empty integers: the interpolation runs on
/// the exact neighbours, and only the result becomes a float.
fn integer_quantile(values: &[i64], q: f64) -> f64 {
    let position = q * (values.len() - 1) as f64;
    let below = position.floor() as usize;
    let above = position.ceil() as usize;
    let gap = i128::from(values[above]) - i128::from(values[below]);
    values[below] as f64 + gap as f64 * (position - below as f64)
}

/// A number for display: whole numbers without a point, others to at most
/// four decimals, and very small ones in scientific notation.
pub fn number(n: f64) -> String {
    if !n.is_finite() {
        return n.to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{n:.0}");
    }
    let fixed = format!("{n:.4}");
    let fixed = fixed.trim_end_matches('0').trim_end_matches('.');
    if fixed == "0" || fixed == "-0" {
        format!("{n:.3e}")
    } else {
        fixed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_past_two_to_the_53_keep_their_min_and_max() {
        let values = [
            Value::Int(i64::MAX),
            Value::Int(i64::MAX - 1),
            Value::Str("9007199254740993".into()),
        ];
        let stats = column("big", values.iter(), &[0.25], 3);
        assert!(stats.numeric);
        assert_eq!(stats.min.as_deref(), Some("9007199254740993"));
        assert_eq!(stats.max.as_deref(), Some("9223372036854775807"));
        let only = [Value::Int(i64::MAX)];
        let stats = column("max", only.iter(), &[], 3);
        assert_eq!(stats.min.as_deref(), Some("9223372036854775807"));
        assert_eq!(stats.max.as_deref(), Some("9223372036854775807"));
        assert_eq!(integer_quantile(&[1, 2, 3, 4], 0.5), 2.5);
    }

    #[test]
    fn numbers_print_short() {
        assert_eq!(number(3.0), "3");
        assert_eq!(number(-2.5), "-2.5");
        assert_eq!(number(1.0 / 3.0), "0.3333");
        assert_eq!(number(0.000012), "1.200e-5");
        assert_eq!(number(1e20), "100000000000000000000");
    }

    #[test]
    fn quantiles_interpolate() {
        let values = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(quantile(&values, 0.0), 1.0);
        assert_eq!(quantile(&values, 0.25), 1.75);
        assert_eq!(quantile(&values, 0.5), 2.5);
        assert_eq!(quantile(&values, 1.0), 4.0);
    }

    /// An infinite value (text such as `1e999` parses as one) at an exact
    /// rank is that value, not `inf - inf`'s NaN; and finite values whose
    /// sum overflows still have a finite mean.
    #[test]
    fn infinities_and_huge_values_keep_their_statistics() {
        let mut rows = Rows::new(["inf", "huge"]);
        for (a, b) in [("1", "1e308"), ("1e999", "1e308"), ("1e999", "1e300")] {
            rows.push([a.into(), b.into()]);
        }
        let stats = Stats::of(&rows);
        let inf = &stats.columns()[0];
        assert_eq!(inf.median, Some(f64::INFINITY));
        assert_eq!(inf.quantiles[1], (0.75, f64::INFINITY));
        assert_eq!(inf.mean, Some(f64::INFINITY));
        let huge = &stats.columns()[1];
        let mean = huge.mean.unwrap();
        let expected = 2.0 / 3.0 * 1e308 + 1e300 / 3.0;
        assert!((mean / expected - 1.0).abs() < 1e-12, "{mean}");
        assert_eq!(huge.median, Some(1e308));
    }

    #[test]
    fn numbers_in_text_count_and_nulls_are_skipped() {
        let mut rows = Rows::new(["n", "s"]);
        rows.push(["10".into(), "b".into()]);
        rows.push(["".into(), Value::Null]);
        rows.push([Value::Float(2.5), "a".into()]);
        rows.push(["-1".into(), "b".into()]);
        let stats = Stats::of(&rows);
        let n = &stats.columns()[0];
        assert!(n.numeric);
        assert_eq!((n.count, n.nulls, n.distinct), (3, 1, 3));
        assert_eq!(
            (n.min.as_deref(), n.max.as_deref()),
            (Some("-1"), Some("10"))
        );
        assert_eq!(n.quantiles, [(0.25, 0.75), (0.75, 6.25)]);
        assert_eq!(n.summary(), "n=3 · 1 null · min -1 · max 10 · mean 3.8333");
        let s = &stats.columns()[1];
        assert!(!s.numeric);
        assert_eq!(s.top, [("b".to_string(), 2), ("a".to_string(), 1)]);
        assert_eq!(s.summary(), "n=3 · 1 null · 2 distinct · top b (2)");
        assert_eq!(
            stats.headers()[1],
            "s\nn=3 · 1 null · 2 distinct · top b (2)"
        );
    }

    #[test]
    fn the_table_has_a_row_per_column() {
        let mut rows = Rows::new(["n"]);
        rows.push([Value::Int(1)]);
        rows.push([Value::Int(3)]);
        let out = Console::builder()
            .width(100)
            .build()
            .render_export(&Stats::of(&rows));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines[1],
            "┃ column ┃ count ┃ nulls ┃ distinct ┃ min ┃ max ┃ mean ┃ median ┃ p25 ┃ p75 ┃ top          ┃"
        );
        assert_eq!(
            lines[3],
            "│ n      │     2 │     0 │        2 │   1 │   3 │    2 │      2 │ 1.5 │ 2.5 │ 1 (1), 3 (1) │"
        );
    }
}
