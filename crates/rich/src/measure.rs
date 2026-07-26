//! Renderable measurement.
//!
//! Port of upstream `rich/measure.py`. A [`Measurement`] is the minimum and
//! maximum number of cells a renderable needs. Only the type and its clamp
//! helper are ported in this slice; the `Measurement.get` machinery lands with
//! the layout widgets that need it.

/// The minimum and maximum width, in cells, a renderable can occupy.
/// Mirrors `rich.measure.Measurement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    pub minimum: usize,
    pub maximum: usize,
}

impl Measurement {
    pub fn new(minimum: usize, maximum: usize) -> Self {
        Measurement { minimum, maximum }
    }

    /// Clamp both bounds into `[min_width, max_width]`. Port of `Measurement.clamp`.
    pub fn clamp(&self, min_width: Option<usize>, max_width: Option<usize>) -> Measurement {
        let mut minimum = self.minimum;
        let mut maximum = self.maximum;
        if let Some(lo) = min_width {
            minimum = minimum.max(lo);
            maximum = maximum.max(lo);
        }
        if let Some(hi) = max_width {
            minimum = minimum.min(hi);
            maximum = maximum.min(hi);
        }
        Measurement { minimum, maximum }
    }
}
