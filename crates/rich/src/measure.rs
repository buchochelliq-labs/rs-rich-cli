//! Renderable measurement.
//!
//! Port of upstream `rich/measure.py`. A [`Measurement`] is the minimum and
//! maximum number of cells a renderable needs.

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;

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

    /// Keep both bounds non-negative with `minimum <= maximum`. Port of
    /// `Measurement.normalize`.
    pub fn normalize(&self) -> Measurement {
        let minimum = self.minimum.min(self.maximum);
        Measurement::new(minimum, self.maximum.max(minimum))
    }

    /// Cap both bounds at `width`. Port of `Measurement.with_maximum`.
    pub fn with_maximum(&self, width: usize) -> Measurement {
        Measurement::new(self.minimum.min(width), self.maximum.min(width))
    }

    /// Measure `renderable` within `options.max_width`. Port of
    /// `Measurement.get` for a renderable that defines `__rich_measure__`.
    pub fn get(console: &Console, options: &ConsoleOptions, renderable: &dyn Renderable) -> Self {
        let max_width = options.max_width;
        if max_width < 1 {
            return Measurement::new(0, 0);
        }
        let width = renderable
            .measure(console, options)
            .normalize()
            .with_maximum(max_width);
        if width.maximum < 1 {
            return Measurement::new(0, 0);
        }
        width.normalize()
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
