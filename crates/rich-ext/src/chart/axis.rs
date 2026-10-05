//! Fitting a chart axis to whole cells.
//!
//! An axis of `cells + 1` rows or columns gives each one an exact value,
//! `lo + p * unit` at position `p` from the low end (the centre of the row
//! or column). Labels go on every `gap`-th position from the low end, so
//! they are evenly spaced, and each is written only when the formatter
//! writes its value exactly: a label is always the value of the row or
//! column it sits on. Labels [`ValueFormat::Compact`] cannot write exactly
//! (`2015` would read `2.0k`) are all written in full instead.

use super::cells;
use super::scale::{along, clean, Scale, ValueFormat};

/// Where an axis's labels go and what every position is worth.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AxisFit {
    /// Positions past the first: the axis has `cells + 1` rows or columns.
    pub cells: usize,
    /// The value at position 0.
    pub lo: f64,
    /// The value from one position to the next.
    pub unit: f64,
    /// `(position, label)`, positions counted from the low end, evenly
    /// spaced.
    pub labels: Vec<(usize, String)>,
}

impl AxisFit {
    /// The scale positions `0..=cells` cover.
    pub fn scale(&self) -> Scale {
        Scale::new(self.lo, along(self.lo, self.cells as f64, self.unit))
    }

    /// Where `value` falls, in positions from the low end (fractional,
    /// clamped to the axis).
    pub fn position(&self, value: f64) -> f64 {
        if self.cells == 0 {
            return 0.0;
        }
        self.scale().normalize(value).unwrap_or(0.0) * self.cells as f64
    }
}

/// What an axis is fitted to.
pub(crate) struct AxisRequest<'a> {
    /// The data's range (or the fixed range, when `fixed`).
    pub data: Scale,
    /// Keep `data`'s bounds exactly instead of rounding out to labels.
    pub fixed: bool,
    /// The lengths (positions past the first) the axis may take; the first
    /// is preferred and each further one from it costs a little.
    pub cells: &'a [usize],
    /// About how many labels to aim for.
    pub wanted: usize,
    /// Labels are written side by side (columns) and need their width plus
    /// two cells between them; rows only need a row each.
    pub label_room: bool,
    pub format: ValueFormat,
}

/// Steps that read well: 1, 2, 2.5 and 5 times a power of ten.
fn steps_near(span: f64, cells: usize) -> Vec<f64> {
    let span = if span.is_finite() && span > 0.0 {
        span
    } else {
        1.0
    };
    let low = (span / (cells.max(1) as f64 * 2.0)).log10().floor() as i32 - 1;
    let high = span.log10().ceil() as i32 + 1;
    let mut out = Vec::new();
    for e in low..=high {
        let power = 10f64.powi(e);
        for m in [1.0, 2.0, 2.5, 5.0] {
            out.push(m * power);
        }
    }
    out
}

/// Whether `step` is 2.5 times a power of ten, which reads less easily
/// than 1, 2 or 5.
fn quarter_step(step: f64) -> bool {
    let mantissa = step / 10f64.powf(step.log10().floor());
    (mantissa - 2.5).abs() < 1e-6
}

/// Whether `step` is a round number: at most two significant digits
/// after the first (`1`, `2.5`, `125`), so labels on it read well.
fn round_step(step: f64) -> bool {
    if !(step.is_finite() && step > 0.0) {
        return false;
    }
    let scaled = step / 10f64.powf(step.log10().floor()) * 100.0;
    (scaled - scaled.round()).abs() < 1e-6
}

/// Undo `ValueFormat::format`: the value a label says.
fn read(label: &str) -> Option<f64> {
    let (number, scale) = match label.chars().last()? {
        'k' => (&label[..label.len() - 1], 1e3),
        'M' => (&label[..label.len() - 1], 1e6),
        'B' => (&label[..label.len() - 1], 1e9),
        'T' => (&label[..label.len() - 1], 1e12),
        _ => (label, 1.0),
    };
    number.parse::<f64>().ok().map(|v| v * scale)
}

/// Whether `label` writes `value` exactly (to float noise, relative to the
/// value, so `0` never passes for a tiny non-zero value).
fn exact(label: &str, value: f64) -> bool {
    read(label).is_some_and(|v| (v - value).abs() <= 1e-9 * value.abs())
}

/// `value` in `format` when that writes it exactly, else in full: a
/// fallback label still names the value its row or column stands for.
pub(crate) fn exact_label(format: ValueFormat, value: f64) -> String {
    let label = format.format(value);
    if exact(&label, value) {
        return label;
    }
    in_full(value)
}

/// `value` with every digit it has (to twelve significant), `1e-12` style
/// when very small or large.
fn in_full(value: f64) -> String {
    let value = clean(value);
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-4..1e15).contains(&magnitude) {
        format!("{value:e}")
    } else {
        format!("{value}")
    }
}

/// Labels for `values`, all in `format` when it writes every one exactly.
/// Otherwise, for [`ValueFormat::Compact`], all in full, so a set of labels
/// never mixes `1.5k` with `1510`; for [`ValueFormat::Fixed`], `None`.
/// `None` too when a value is not finite.
pub(crate) fn exact_labels(format: ValueFormat, values: &[f64]) -> Option<(Vec<String>, bool)> {
    if values.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let labels: Vec<String> = values.iter().map(|&v| format.format(v)).collect();
    if labels.iter().zip(values).all(|(l, &v)| exact(l, v)) {
        return Some((labels, false));
    }
    match format {
        ValueFormat::Compact => Some((values.iter().map(|&v| in_full(v)).collect(), true)),
        ValueFormat::Fixed(_) => None,
    }
}

struct Candidate {
    cost: f64,
    cells: usize,
    lo: f64,
    unit: f64,
    gap: usize,
    step: f64,
}

/// What writing a candidate's labels in full instead of in the chosen
/// format costs: a round axis in the format's own short form wins a near
/// tie, but not over one that spreads the data out.
const IN_FULL: f64 = 0.2;

impl Candidate {
    /// The labels and whether they had to be written in full, or `None`
    /// when one would not be exact, the axis would run past the largest
    /// `f64`, or they would not fit side by side.
    fn labels(&self, request: &AxisRequest) -> Option<(Vec<(usize, String)>, bool)> {
        if !along(self.lo, self.cells as f64, self.unit).is_finite() {
            return None;
        }
        let count = self.cells / self.gap + 1;
        let values: Vec<f64> = (0..count)
            .map(|k| clean(along(self.lo, k as f64, self.step)))
            .collect();
        let (labels, full) = exact_labels(request.format, &values)?;
        // Written in full, a fixed range's labels are only worth it on
        // round steps: `33.3333333333` is no label.
        if full && request.fixed && !round_step(self.step) {
            return None;
        }
        let widest = labels.iter().map(|l| cells(l)).max().unwrap_or(0);
        if request.label_room && widest + 2 > self.gap {
            return None;
        }
        let out = labels
            .into_iter()
            .enumerate()
            .map(|(k, label)| (k * self.gap, label))
            .collect();
        Some((out, full))
    }
}

/// Fit an axis: the length, the value of each position and the labels.
pub(crate) fn fit(request: &AxisRequest) -> AxisFit {
    let preferred = request.cells.first().copied().unwrap_or(0);
    let data = request.data;
    let fallback = |cells: usize| AxisFit {
        cells,
        lo: data.min(),
        unit: if cells == 0 {
            0.0
        } else {
            // Divided first, so bounds near `±f64::MAX` do not overflow.
            data.max() / cells as f64 - data.min() / cells as f64
        },
        labels: if cells == 0 {
            Vec::new()
        } else {
            vec![
                (0, exact_label(request.format, data.min())),
                (cells, exact_label(request.format, data.max())),
            ]
        },
    };
    if preferred == 0 {
        return AxisFit {
            labels: Vec::new(),
            ..fallback(0)
        };
    }

    let wanted = request.wanted.max(2) as f64;
    let mut candidates = Vec::new();
    // A range whose span overflows has no round steps to try.
    let finite = (data.max() - data.min()).is_finite();
    for &cells in request.cells.iter().filter(|&&c| c > 0 && finite) {
        let height_cost = cells.abs_diff(preferred) as f64 * 0.1;
        let mut push = |gap: usize, lo: f64, unit: f64, step: f64, waste: f64| {
            let labels = (cells / gap + 1) as f64;
            // Too few labels is worse than a few too many.
            let count = if labels < wanted {
                (wanted - labels) * 0.1
            } else {
                (labels - wanted) * 0.05
            };
            let top = if cells % gap == 0 { 0.0 } else { 0.15 };
            let dense = if gap == 1 && cells >= 4 { 0.1 } else { 0.0 };
            let quarter = if quarter_step(step) { 0.15 } else { 0.0 };
            // Empty space past the data matters most; past a quarter of
            // the axis it is a last resort.
            let empty = waste * 2.0 + if waste > 0.25 { 1.0 } else { 0.0 };
            candidates.push(Candidate {
                cost: empty + count + top + dense + quarter + height_cost,
                cells,
                lo,
                unit,
                gap,
                step,
            });
        };
        if request.fixed {
            let unit = data.span() / cells as f64;
            for gap in 1..=cells {
                push(gap, data.min(), unit, unit * gap as f64, 0.0);
            }
        } else {
            for step in steps_near(data.span(), cells)
                .into_iter()
                .filter(|s| s.is_finite())
            {
                let lo = clean((data.min() / step + 1e-9).floor() * step);
                let need = data.max() - lo;
                // The widest gap that still reaches the data's top.
                let widest = if need <= 0.0 {
                    cells
                } else {
                    ((cells as f64 * step / need) + 1e-9).floor() as usize
                };
                let widest = widest.min(cells);
                if widest == 0 {
                    continue;
                }
                let mut gaps: Vec<usize> = (widest.saturating_sub(3).max(1)..=widest).collect();
                gaps.extend((1..=widest).filter(|g| cells % g == 0));
                gaps.sort_unstable();
                gaps.dedup();
                for gap in gaps {
                    let unit = step / gap as f64;
                    let span = unit * cells as f64;
                    let waste = (1.0 - data.span() / span).clamp(0.0, 1.0);
                    push(gap, lo, unit, step, waste);
                }
            }
        }
    }
    candidates.sort_by(|a, b| a.cost.total_cmp(&b.cost));
    let mut best: Option<(f64, AxisFit)> = None;
    for candidate in &candidates {
        if best
            .as_ref()
            .is_some_and(|(cost, _)| candidate.cost >= *cost)
        {
            break;
        }
        if let Some((labels, full)) = candidate.labels(request) {
            let cost = candidate.cost + if full { IN_FULL } else { 0.0 };
            if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                let fit = AxisFit {
                    cells: candidate.cells,
                    lo: candidate.lo,
                    unit: candidate.unit,
                    labels,
                };
                best = Some((cost, fit));
            }
        }
    }
    if let Some((_, fit)) = best {
        return fit;
    }
    // Nothing writes exactly: the bounds alone, at both ends.
    let mut fit = fallback(preferred);
    if request.label_room {
        let room: usize = fit.labels.iter().map(|(_, l)| cells(l) + 1).sum();
        if room > preferred + 1 {
            fit.labels.truncate(1);
        }
    }
    fit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(data: Scale, fixed: bool, cells: &[usize]) -> AxisFit {
        fit(&AxisRequest {
            data,
            fixed,
            cells,
            wanted: 5,
            label_room: false,
            format: ValueFormat::Compact,
        })
    }

    fn assert_even(fit: &AxisFit) {
        let gaps: Vec<usize> = fit.labels.windows(2).map(|w| w[1].0 - w[0].0).collect();
        assert!(gaps.windows(2).all(|g| g[0] == g[1]), "{fit:?}");
        for (p, label) in &fit.labels {
            let value = fit.lo + *p as f64 * fit.unit;
            assert!(exact(label, clean(value)), "{label} at {p} is {value}");
        }
    }

    #[test]
    fn fixed_range_and_height_labels_only_exact_rows() {
        let fit = rows(Scale::new(0.0, 100.0), true, &[10]);
        assert_eq!(
            fit.labels,
            [
                (0, "0"),
                (2, "20"),
                (4, "40"),
                (6, "60"),
                (8, "80"),
                (10, "100")
            ]
            .map(|(p, l)| (p, l.to_string()))
        );
        // 100 / 9 rows is no round step: the bounds alone.
        let fit = rows(Scale::new(0.0, 100.0), true, &[9]);
        assert_eq!(fit.labels, [(0, "0".to_string()), (9, "100".to_string())]);
    }

    #[test]
    fn free_height_picks_a_height_the_step_divides() {
        let fit = rows(Scale::new(0.0, 100.0), true, &[7, 5, 6, 8, 9]);
        assert_eq!(fit.cells, 8);
        assert_eq!(fit.labels.len(), 5);
        assert_even(&fit);
    }

    #[test]
    fn data_ranges_round_out_and_stay_even() {
        for (lo, hi) in [
            (12.0, 52.0),
            (-3.0, 7.5),
            (0.001, 0.0042),
            (1200.0, 98_000.0),
        ] {
            for cells in 3..=19 {
                let fit = rows(Scale::new(lo, hi), false, &[cells]);
                assert_even(&fit);
                assert!(fit.scale().min() <= lo && fit.scale().max() >= hi - 1e-9);
            }
        }
    }

    #[test]
    fn fallback_labels_are_exact() {
        // No round step lands on these bounds' rows, and compact would
        // write them `1.2k` and `5.7k`.
        let fit = rows(Scale::new(1234.0, 5678.0), true, &[7]);
        assert_eq!(
            fit.labels,
            [(0, "1234".to_string()), (7, "5678".to_string())]
        );
        // Too small for compact's two decimals: never `0`, and every row
        // is a round step, so each is labelled in full.
        let fit = rows(Scale::new(1e-12, 7e-12), true, &[5]);
        assert_eq!(
            fit.labels,
            [
                (0, "1e-12"),
                (1, "2.2e-12"),
                (2, "3.4e-12"),
                (3, "4.6e-12"),
                (4, "5.8e-12"),
                (5, "7e-12")
            ]
            .map(|(p, l)| (p, l.to_string()))
        );
        assert_even(&fit);
    }

    #[test]
    fn compact_labels_it_cannot_write_go_in_full() {
        // Years: compact writes `2.0k`, so a 100 step was the only exact
        // one; now the labels are the years.
        let fit = rows(Scale::new(2015.0, 2024.0), false, &[9]);
        assert!(fit.labels.len() >= 3, "{fit:?}");
        assert!(fit.labels.iter().all(|(_, l)| !l.ends_with('k')), "{fit:?}");
        assert_even(&fit);
        assert!(fit.scale().max() - fit.scale().min() < 20.0, "{fit:?}");
        // Never mixed: all short or all in full.
        let fit = rows(Scale::new(1500.0, 1563.0), false, &[5]);
        assert!(fit.labels.iter().all(|(_, l)| !l.ends_with('k')), "{fit:?}");
        // A fixed range in full only on round steps.
        let fit = rows(Scale::new(0.0, 100.0), true, &[9]);
        assert_eq!(fit.labels, [(0, "0".to_string()), (9, "100".to_string())]);
    }

    #[test]
    fn labels_read_back() {
        assert_eq!(read("1.2k"), Some(1200.0));
        assert_eq!(read("-2.5M"), Some(-2_500_000.0));
        assert!(exact("0.25", 0.25));
        assert!(!exact("1.2k", 1234.0));
    }
}
