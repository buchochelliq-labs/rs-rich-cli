//! A linear numeric scale with "nice" ticks, and how values are written.

use crate::format;

/// A linear scale from [`min`](Self::min) to [`max`](Self::max).
///
/// Every chart maps its values through one. [`Scale::from_values`] skips NaN
/// and infinite values and never returns an empty range, so the charts never
/// divide by zero:
///
/// - no finite values: `0..1`;
/// - every value equal to `v`: `v - |v| .. v + |v|`, so `v` sits in the
///   middle (`0..1` when `v` is 0);
/// - otherwise the smallest and largest value.
///
/// ```
/// use rich_ext::chart::Scale;
///
/// let scale = Scale::from_values([3.0, f64::NAN, 17.0, 9.0]);
/// assert_eq!((scale.min(), scale.max()), (3.0, 17.0));
/// assert_eq!(scale.nice(5).ticks(5), vec![0.0, 5.0, 10.0, 15.0, 20.0]);
/// assert_eq!(scale.normalize(10.0), Some(0.5));
///
/// // Explicit bounds win, in either order.
/// assert_eq!(Scale::new(10.0, -10.0).normalize(0.0), Some(0.5));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    min: f64,
    max: f64,
}

impl Default for Scale {
    fn default() -> Self {
        Scale { min: 0.0, max: 1.0 }
    }
}

impl Scale {
    /// A scale between two bounds, in either order. Non-finite bounds fall
    /// back to `0..1`; equal bounds are widened as in
    /// [`from_values`](Self::from_values).
    pub fn new(a: f64, b: f64) -> Self {
        if !a.is_finite() || !b.is_finite() {
            return Scale::default();
        }
        let (min, max) = if a <= b { (a, b) } else { (b, a) };
        if min == max {
            return Self::degenerate(min);
        }
        Scale { min, max }
    }

    fn degenerate(v: f64) -> Self {
        if v == 0.0 {
            Scale { min: 0.0, max: 1.0 }
        } else {
            Scale {
                min: v - v.abs(),
                max: v + v.abs(),
            }
        }
    }

    /// The range of the finite values in `values`.
    pub fn from_values(values: impl IntoIterator<Item = f64>) -> Self {
        let mut range: Option<(f64, f64)> = None;
        for v in values.into_iter().filter(|v| v.is_finite()) {
            range = Some(match range {
                None => (v, v),
                Some((lo, hi)) => (lo.min(v), hi.max(v)),
            });
        }
        match range {
            None => Scale::default(),
            Some((lo, hi)) => Scale::new(lo, hi),
        }
    }

    /// Override either bound. A bound left `None` keeps its value; if the
    /// result is empty or reversed it is fixed up as [`new`](Self::new) does.
    pub fn bounds(self, min: Option<f64>, max: Option<f64>) -> Self {
        let min = min.filter(|v| v.is_finite());
        let max = max.filter(|v| v.is_finite());
        let width = self.span().abs().max(1.0);
        match (min, max) {
            // Only one bound was given and it passed the other: keep the
            // given one and widen away from it.
            (Some(lo), None) if lo > self.max => Scale::new(lo, lo + width),
            (None, Some(hi)) if hi < self.min => Scale::new(hi - width, hi),
            (lo, hi) => Scale::new(lo.unwrap_or(self.min), hi.unwrap_or(self.max)),
        }
    }

    /// Stretch the scale so it contains 0, as bars need.
    pub fn include_zero(self) -> Self {
        Scale::new(self.min.min(0.0), self.max.max(0.0))
    }

    /// Widen the bounds outward to multiples of the tick step that
    /// [`ticks`](Self::ticks) would use for `count` ticks.
    pub fn nice(self, count: usize) -> Self {
        let step = self.step(count);
        let min = (self.min / step).floor() * step;
        let max = (self.max / step).ceil() * step;
        Scale::new(clean(min), clean(max))
    }

    /// The lower bound.
    pub fn min(&self) -> f64 {
        self.min
    }

    /// The upper bound.
    pub fn max(&self) -> f64 {
        self.max
    }

    /// `max - min`, always above zero. Bounds near `±f64::MAX` would
    /// overflow it; it is then `f64::MAX`.
    pub fn span(&self) -> f64 {
        let span = self.max - self.min;
        if span.is_finite() {
            span
        } else {
            f64::MAX
        }
    }

    /// Where `value` falls, from 0 (at `min`) to 1 (at `max`), clamped.
    /// `None` for NaN and infinities.
    pub fn normalize(&self, value: f64) -> Option<f64> {
        if !value.is_finite() {
            return None;
        }
        let span = self.max - self.min;
        let at = if span.is_finite() {
            (value - self.min) / span
        } else {
            // Halved, so neither difference overflows.
            (value * 0.5 - self.min * 0.5) / (self.max * 0.5 - self.min * 0.5)
        };
        Some(at.clamp(0.0, 1.0))
    }

    /// The value a fraction `t` of the way from `min` to `max` (0 gives
    /// `min`, 1 gives `max`), without overflowing.
    pub(crate) fn lerp(&self, t: f64) -> f64 {
        let span = self.max - self.min;
        if span.is_finite() {
            self.min + t * span
        } else {
            self.min * (1.0 - t) + self.max * t
        }
    }

    /// The tick step for about `count` ticks: 1, 2 or 5 times a power of ten.
    pub fn step(&self, count: usize) -> f64 {
        let intervals = count.max(2) - 1;
        nice_number(self.span() / intervals as f64, true)
    }

    /// Round values inside the scale, about `count` of them (at least 2),
    /// spaced by [`step`](Self::step). They include the bounds only when the
    /// bounds are themselves multiples of the step; use [`nice`](Self::nice)
    /// first for ticks that span the whole scale.
    pub fn ticks(&self, count: usize) -> Vec<f64> {
        self.ticks_every(self.step(count))
    }

    /// The multiples of `step` inside the scale. After [`nice`](Self::nice)
    /// the range is wider, so its own [`step`](Self::step) may differ; pass
    /// the step of the range before widening to keep the ticks the bounds
    /// were rounded to.
    pub fn ticks_every(&self, step: f64) -> Vec<f64> {
        if !(step.is_finite() && step > 0.0) || self.span() / step > 10_000.0 {
            return vec![self.min, self.max];
        }
        let first = (self.min / step - 1e-9).ceil() as i64;
        let last = (self.max / step + 1e-9).floor() as i64;
        (first..=last).map(|k| clean(k as f64 * step)).collect()
    }
}

/// Heckbert's "nice number": 1, 2, 5 or 10 times a power of ten near `x`.
fn nice_number(x: f64, round: bool) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let exponent = x.log10().floor();
    let power = 10f64.powf(exponent);
    let fraction = x / power;
    let nice = if round {
        match fraction {
            f if f < 1.5 => 1.0,
            f if f < 3.0 => 2.0,
            f if f < 7.0 => 5.0,
            _ => 10.0,
        }
    } else {
        match fraction {
            f if f <= 1.0 => 1.0,
            f if f <= 2.0 => 2.0,
            f if f <= 5.0 => 5.0,
            _ => 10.0,
        }
    };
    nice * power
}

/// `lo + k * unit`, without overflowing on the way when the result itself
/// is finite.
pub(crate) fn along(lo: f64, k: f64, unit: f64) -> f64 {
    let offset = k * unit;
    let value = lo + offset;
    if value.is_finite() || !lo.is_finite() || !unit.is_finite() {
        value
    } else {
        (lo * 0.5 + k * (unit * 0.5)) * 2.0
    }
}

/// `(value - lo) / unit`: how many `unit`s `value` is past `lo`, without
/// overflowing on the way.
pub(crate) fn units_past(value: f64, lo: f64, unit: f64) -> f64 {
    let diff = value - lo;
    if diff.is_finite() {
        diff / unit
    } else {
        (value * 0.5 - lo * 0.5) / unit * 2.0
    }
}

/// Drop floating-point noise (`0.30000000000000004`) and negative zero,
/// keeping twelve significant digits whatever the magnitude.
pub(crate) fn clean(v: f64) -> f64 {
    if !v.is_finite() {
        return v;
    }
    let rounded: f64 = format!("{v:.11e}").parse().unwrap_or(v);
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

/// How a chart writes values on axes, bars and summaries.
///
/// ```
/// use rich_ext::chart::ValueFormat;
///
/// assert_eq!(ValueFormat::Compact.format(1_234.0), "1.2k");
/// assert_eq!(ValueFormat::Compact.format(3_400_000.0), "3.4M");
/// assert_eq!(ValueFormat::Compact.format(0.25), "0.25");
/// assert_eq!(ValueFormat::Fixed(1).format(2.0), "2.0");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ValueFormat {
    /// Short forms: `950`, `0.25`, `12.5`, then `1.2k`, `3.4M`, `5.0B`
    /// ([`format::compact`] from a thousand up, at most two decimals
    /// below), and `1.0e15` from a thousand trillion. A chart axis or
    /// histogram whose round values this cannot write exactly (`2015`
    /// would read `2.0k`) writes them in full instead.
    #[default]
    Compact,
    /// A fixed number of decimals: `Fixed(2)` writes `3.14`. At most
    /// [`MAX_DECIMALS`](Self::MAX_DECIMALS) are written; more are capped.
    Fixed(usize),
}

impl ValueFormat {
    /// The most decimals [`Fixed`](Self::Fixed) writes: an `f64` holds
    /// at most 17 significant digits.
    pub const MAX_DECIMALS: usize = 17;

    /// `value` in this format. NaN and infinities are written `-`.
    pub fn format(self, value: f64) -> String {
        if !value.is_finite() {
            return "-".to_string();
        }
        let value = if value == 0.0 { 0.0 } else { value };
        match self {
            ValueFormat::Fixed(decimals) => {
                let decimals = decimals.min(Self::MAX_DECIMALS);
                let text = format!("{value:.decimals$}");
                // `-0.0` rounds to "-0.0"; write it as zero.
                if text
                    .trim_start_matches('-')
                    .chars()
                    .all(|c| c == '0' || c == '.')
                {
                    text.trim_start_matches('-').to_string()
                } else {
                    text
                }
            }
            ValueFormat::Compact => {
                if value.abs() >= 1e15 {
                    // Past `T`, compact would write every digit.
                    return format!("{value:.1e}");
                }
                if value.abs() >= 1000.0 {
                    return format::compact(value);
                }
                let text = format!("{value:.2}");
                let text = text.trim_end_matches('0').trim_end_matches('.');
                if text == "-0" {
                    "0".to_string()
                } else {
                    text.to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round_numbers_inside_the_scale() {
        let s = Scale::new(0.0, 100.0);
        // 100 / 4 = 25 rounds to a step of 20.
        assert_eq!(s.ticks(5), vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        assert_eq!(s.ticks(6), vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        assert_eq!(s.ticks(3), vec![0.0, 50.0, 100.0]);
        assert_eq!(
            Scale::new(0.0, 1.0).ticks(5),
            vec![0.0, 0.2, 0.4, 0.6, 0.8, 1.0]
        );
        assert_eq!(Scale::new(3.0, 17.0).ticks(4), vec![5.0, 10.0, 15.0]);
        assert_eq!(Scale::new(0.1, 0.35).ticks(3), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn nice_widens_to_whole_steps() {
        let s = Scale::new(3.0, 17.0).nice(5);
        assert_eq!((s.min(), s.max()), (0.0, 20.0));
        assert_eq!(s.ticks(5), vec![0.0, 5.0, 10.0, 15.0, 20.0]);
        // The ticks of the widened range at the step it was rounded to.
        let data = Scale::new(1.0, 5.0);
        let wide = data.nice(3);
        assert_eq!((wide.min(), wide.max()), (0.0, 6.0));
        assert_eq!(wide.ticks_every(data.step(3)), vec![0.0, 2.0, 4.0, 6.0]);
        assert_eq!(wide.ticks(3), vec![0.0, 5.0]);
        assert_eq!(wide.ticks_every(0.0), vec![0.0, 6.0]);
        let s = Scale::new(-7.0, 42.0).nice(5);
        assert_eq!((s.min(), s.max()), (-10.0, 50.0));
        // Already nice: unchanged.
        let s = Scale::new(0.0, 100.0).nice(5);
        assert_eq!((s.min(), s.max()), (0.0, 100.0));
    }

    #[test]
    fn a_single_bound_past_the_data_is_kept() {
        let data = Scale::from_values([150.0, 200.0]);
        let s = data.bounds(None, Some(100.0));
        assert_eq!((s.min(), s.max()), (50.0, 100.0));
        let s = data.bounds(Some(300.0), None);
        assert_eq!((s.min(), s.max()), (300.0, 350.0));
        // Both given, in either order.
        let s = data.bounds(Some(10.0), Some(0.0));
        assert_eq!((s.min(), s.max()), (0.0, 10.0));
    }

    #[test]
    fn tiny_scales_keep_their_magnitude() {
        let s = Scale::new(1e-12, 5e-12).nice(5);
        assert_eq!((s.min(), s.max()), (1e-12, 5e-12));
        assert_eq!(s.ticks(5), vec![1e-12, 2e-12, 3e-12, 4e-12, 5e-12]);
        assert_eq!(clean(0.1 + 0.2), 0.3);
        assert_eq!(clean(-0.0).to_bits(), 0.0f64.to_bits());
    }

    #[test]
    fn degenerate_ranges_never_collapse() {
        let zero = Scale::from_values([0.0, 0.0, 0.0]);
        assert_eq!((zero.min(), zero.max()), (0.0, 1.0));
        let five = Scale::from_values([5.0; 4]);
        assert_eq!((five.min(), five.max()), (0.0, 10.0));
        assert_eq!(five.normalize(5.0), Some(0.5));
        let neg = Scale::from_values([-4.0]);
        assert_eq!((neg.min(), neg.max()), (-8.0, 0.0));
        let empty = Scale::from_values(std::iter::empty());
        assert_eq!((empty.min(), empty.max()), (0.0, 1.0));
        assert_eq!(Scale::new(2.0, 2.0).span(), 4.0);
        assert!(Scale::new(f64::NAN, 3.0) == Scale::default());
        for s in [zero, five, neg, empty] {
            assert!(s.span() > 0.0);
            assert!(!s.ticks(5).is_empty());
        }
    }

    #[test]
    fn negative_values_and_zero() {
        let s = Scale::from_values([-30.0, -5.0, 20.0]);
        assert_eq!((s.min(), s.max()), (-30.0, 20.0));
        assert_eq!(s.ticks(6), vec![-30.0, -20.0, -10.0, 0.0, 10.0, 20.0]);
        assert_eq!(s.normalize(-30.0), Some(0.0));
        assert_eq!(s.normalize(-5.0), Some(0.5));
        let pos = Scale::from_values([4.0, 9.0]).include_zero();
        assert_eq!((pos.min(), pos.max()), (0.0, 9.0));
        let neg = Scale::from_values([-4.0, -9.0]).include_zero();
        assert_eq!((neg.min(), neg.max()), (-9.0, 0.0));
        // No "-0" ticks.
        assert!(Scale::new(-1.0, 1.0)
            .ticks(5)
            .iter()
            .all(|t| t.to_string() != "-0"));
    }

    #[test]
    fn nan_and_infinities_are_skipped() {
        let s = Scale::from_values([f64::NAN, 1.0, f64::INFINITY, 3.0, f64::NEG_INFINITY]);
        assert_eq!((s.min(), s.max()), (1.0, 3.0));
        assert_eq!(s.normalize(f64::NAN), None);
        assert_eq!(s.normalize(f64::INFINITY), None);
        assert_eq!(Scale::from_values([f64::NAN]), Scale::default());
    }

    #[test]
    fn explicit_bounds_override_and_clamp() {
        let s = Scale::from_values([3.0, 7.0]).bounds(Some(0.0), Some(10.0));
        assert_eq!((s.min(), s.max()), (0.0, 10.0));
        assert_eq!(s.normalize(15.0), Some(1.0));
        assert_eq!(s.normalize(-5.0), Some(0.0));
        let only_max = Scale::from_values([3.0, 7.0]).bounds(None, Some(100.0));
        assert_eq!((only_max.min(), only_max.max()), (3.0, 100.0));
        // A min above the data's max still gives a usable scale.
        let past = Scale::from_values([3.0, 7.0]).bounds(Some(50.0), None);
        assert_eq!(past.min(), 50.0);
        assert!(past.span() > 0.0);
    }

    #[test]
    fn value_formats() {
        let c = ValueFormat::Compact;
        assert_eq!(c.format(0.0), "0");
        assert_eq!(c.format(-0.0), "0");
        assert_eq!(c.format(42.0), "42");
        assert_eq!(c.format(12.5), "12.5");
        assert_eq!(c.format(0.126), "0.13");
        assert_eq!(c.format(-3.0), "-3");
        assert_eq!(c.format(1_250.0), "1.2k");
        assert_eq!(c.format(-3_400_000.0), "-3.4M");
        assert_eq!(c.format(f64::NAN), "-");
        assert_eq!(c.format(-0.001), "0");
        let f = ValueFormat::Fixed(2);
        assert_eq!(f.format(1.23456), "1.23");
        assert_eq!(f.format(-0.001), "0.00");
        assert_eq!(ValueFormat::Fixed(0).format(2.6), "3");
    }
}
