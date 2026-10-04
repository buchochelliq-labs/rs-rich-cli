//! Fitting a chart axis to whole cells.
//!
//! An axis of `cells + 1` rows or columns gives each one an exact value,
//! `lo + p * unit` at position `p` from the low end (the centre of the row
//! or column). Labels go on every `gap`-th position from the low end, so
//! they are evenly spaced, and each is written only when the formatter
//! writes its value exactly: a label is always the value of the row or
//! column it sits on.

use super::cells;
use super::scale::{Scale, ValueFormat};

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
        Scale::new(self.lo, self.lo + self.cells as f64 * self.unit)
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

/// Whether `label` writes `value` exactly.
fn exact(label: &str, value: f64) -> bool {
    read(label).is_some_and(|v| (v - value).abs() <= 1e-9 * value.abs().max(1.0))
}

/// Drop floating-point noise and negative zero.
fn clean(v: f64) -> f64 {
    let magnitude = 10f64.powi(9 - v.abs().max(1.0).log10().floor() as i32);
    let rounded = (v * magnitude).round() / magnitude;
    if rounded == 0.0 {
        0.0
    } else {
        rounded
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

impl Candidate {
    /// The labels, or `None` when one would not be exact or they would not
    /// fit side by side.
    fn labels(&self, request: &AxisRequest) -> Option<Vec<(usize, String)>> {
        let mut out = Vec::new();
        let mut widest = 0;
        let mut k = 0usize;
        while k * self.gap <= self.cells {
            let value = clean(self.lo + k as f64 * self.step);
            let label = request.format.format(value);
            if !exact(&label, value) {
                return None;
            }
            widest = widest.max(cells(&label));
            out.push((k * self.gap, label));
            k += 1;
        }
        (!request.label_room || widest + 2 <= self.gap).then_some(out)
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
            data.span() / cells as f64
        },
        labels: if cells == 0 {
            Vec::new()
        } else {
            vec![
                (0, request.format.format(data.min())),
                (cells, request.format.format(data.max())),
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
    for &cells in request.cells.iter().filter(|&&c| c > 0) {
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
            for step in steps_near(data.span(), cells) {
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
    for candidate in &candidates {
        if let Some(labels) = candidate.labels(request) {
            return AxisFit {
                cells: candidate.cells,
                lo: candidate.lo,
                unit: candidate.unit,
                labels,
            };
        }
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
    fn labels_read_back() {
        assert_eq!(read("1.2k"), Some(1200.0));
        assert_eq!(read("-2.5M"), Some(-2_500_000.0));
        assert!(exact("0.25", 0.25));
        assert!(!exact("1.2k", 1234.0));
    }
}
