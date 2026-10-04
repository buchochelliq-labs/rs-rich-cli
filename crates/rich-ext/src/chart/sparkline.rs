//! A one-line chart of a series of values.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{
    cells, has_colour, lines_to_segments, theme_style, user_style, Charset, Line, Scale,
    ValueFormat,
};

/// Heights for [`Charset::Blocks`], lowest first.
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Heights for [`Charset::Ascii`], lowest first (the ramp `qa::bench` uses).
const ASCII: [char; 8] = ['_', '.', '-', ':', '=', '+', '*', '#'];

/// A one-line chart: each value is a cell whose height shows where it falls
/// between the smallest and largest value.
///
/// - [`Charset::Blocks`] (the default) draws one value per cell with
///   `▁▂▃▄▅▆▇█`; [`Charset::Braille`] draws two values per cell, four
///   heights each; [`Charset::Ascii`] uses the ramp `_.-:=+*#`.
/// - NaN and infinite values are gaps (a space).
/// - It measures to one cell per value (half that in Braille). Given fewer
///   cells, it **resamples**: the values are split into as many equal
///   buckets as there are cells (two per cell in Braille) and each bucket
///   is drawn at the mean of its finite values.
/// - [`min_max`](Self::min_max) styles the lowest and highest cell and
///   writes `min 2 max 9` after the line; [`threshold`](Self::threshold)
///   styles the cells above it and writes `3 > 80`. The words are what make
///   them readable without colour, and they are the first thing left out
///   when the width is short.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, Sparkline};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let values = [2.0, 4.0, 3.0, 9.0, 6.0, 5.0];
/// let spark = Sparkline::new(values).min_max(true);
/// assert_eq!(console.render_to_string(&spark), "▁▃▂█▅▄ min 2 max 9\n");
///
/// let ascii = Sparkline::new(values).charset(Charset::Ascii);
/// assert_eq!(console.render_to_string(&ascii), "_-.#=:\n");
///
/// let braille = Sparkline::new(values).charset(Charset::Braille);
/// assert_eq!(console.render_to_string(&braille), "⣠⣸⣦\n");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Sparkline {
    values: Vec<f64>,
    min: Option<f64>,
    max: Option<f64>,
    charset: Charset,
    style: Option<String>,
    min_max: bool,
    threshold: Option<f64>,
    format: ValueFormat,
}

impl Sparkline {
    /// A sparkline of `values`, in order.
    pub fn new(values: impl IntoIterator<Item = f64>) -> Self {
        Sparkline {
            values: values.into_iter().collect(),
            min: None,
            max: None,
            charset: Charset::Auto,
            style: None,
            min_max: false,
            threshold: None,
            format: ValueFormat::Compact,
        }
    }

    /// Fix the bottom and top of the scale instead of using the data's
    /// range; values outside are clamped.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Fix only the bottom of the scale (`0.0` to start bars at zero).
    pub fn min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    /// Fix only the top of the scale.
    pub fn max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// The line's style: a theme key or a definition (default
    /// `chart.spark`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Mark the lowest and highest values (`chart.min`, `chart.max`) and
    /// write them after the line.
    pub fn min_max(mut self, show: bool) -> Self {
        self.min_max = show;
        self
    }

    /// Style values above `threshold` with `chart.over`, and write how many
    /// there are after the line.
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// How the summary writes values (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// The values.
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// The charset this sparkline draws with on `console`.
    pub(crate) fn resolved_charset(&self, console: &Console, options: &ConsoleOptions) -> Charset {
        self.charset.resolve(console, options, Charset::Blocks)
    }

    /// Cells for every value and the summary, at `charset`.
    pub(crate) fn natural_width(&self, charset: Charset) -> usize {
        self.natural_cells(charset) + self.summary_width()
    }

    fn scale(&self) -> Scale {
        Scale::from_values(self.values.iter().copied()).bounds(self.min, self.max)
    }

    /// Values per cell for a charset.
    fn per_cell(charset: Charset) -> usize {
        if charset == Charset::Braille {
            2
        } else {
            1
        }
    }

    /// The text after the line, as (text, theme key) parts.
    fn summary(&self) -> Vec<(String, &'static str)> {
        let finite: Vec<f64> = self
            .values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .collect();
        let mut parts = Vec::new();
        if self.min_max && !finite.is_empty() {
            let lo = finite.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            parts.push((format!("min {}", self.format.format(lo)), "chart.min"));
            parts.push((format!("max {}", self.format.format(hi)), "chart.max"));
        }
        if let Some(t) = self.threshold {
            let over = finite.iter().filter(|v| **v > t).count();
            if over > 0 {
                parts.push((format!("{over} > {}", self.format.format(t)), "chart.over"));
            }
        }
        parts
    }

    fn summary_width(&self) -> usize {
        self.summary().iter().map(|(t, _)| 1 + cells(t)).sum()
    }

    fn natural_cells(&self, charset: Charset) -> usize {
        self.values.len().div_ceil(Self::per_cell(charset))
    }

    /// The values to draw in `slots` slots: as they are when they fit,
    /// otherwise the mean of each of `slots` equal buckets.
    fn resample(&self, slots: usize) -> Vec<(f64, usize)> {
        let n = self.values.len();
        if n <= slots {
            return self
                .values
                .iter()
                .copied()
                .enumerate()
                .map(|(i, v)| (v, i))
                .collect();
        }
        (0..slots)
            .map(|i| {
                let start = i * n / slots;
                let end = ((i + 1) * n / slots).max(start + 1);
                let bucket: Vec<f64> = self.values[start..end]
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .collect();
                let mean = if bucket.is_empty() {
                    f64::NAN
                } else {
                    bucket.iter().sum::<f64>() / bucket.len() as f64
                };
                (mean, start)
            })
            .collect()
    }

    pub(crate) fn line(&self, console: &Console, charset: Charset, width: usize) -> Line {
        let per_cell = Self::per_cell(charset);
        let natural = self.natural_cells(charset);
        let summary_w = self.summary_width();
        let (cells_used, show_summary) = if natural + summary_w <= width {
            (natural, true)
        } else {
            (natural.min(width), false)
        };
        let samples = self.resample(cells_used * per_cell);
        let scale = self.scale();
        let base = self
            .style
            .as_deref()
            .map(|s| user_style(console, s))
            .unwrap_or_else(|| theme_style(console, "chart.spark"));
        let colour = has_colour(console);
        let finite = || self.values.iter().copied().filter(|v| v.is_finite());
        let lo = finite().fold(f64::INFINITY, f64::min);
        let hi = finite().fold(f64::NEG_INFINITY, f64::max);
        // Mark extremes on the drawn samples (the first of each).
        let drawn = |pick: f64| {
            samples.iter().position(|(v, _)| *v == pick).or_else(|| {
                // Resampled: the slot whose bucket holds the extreme.
                let at = self.values.iter().position(|v| *v == pick)?;
                samples.iter().rposition(|(_, start)| *start <= at)
            })
        };
        let (min_slot, max_slot) = if self.min_max && lo.is_finite() {
            (drawn(lo), drawn(hi))
        } else {
            (None, None)
        };
        let slot_style = |i: usize, v: f64| -> Option<Style> {
            if !colour {
                return None;
            }
            let key = if self.threshold.is_some_and(|t| v > t) {
                "chart.over"
            } else if max_slot == Some(i) {
                "chart.max"
            } else if min_slot == Some(i) {
                "chart.min"
            } else {
                return Some(base.clone());
            };
            Some(theme_style(console, key))
        };

        let mut line = Line::new();
        match charset {
            Charset::Braille => {
                for (cell, pair) in samples.chunks(2).enumerate() {
                    let mut bits = 0u32;
                    let mut style = None;
                    for (half, (v, _)) in pair.iter().enumerate() {
                        let Some(n) = scale.normalize(*v) else {
                            continue;
                        };
                        // One to four dots, filled from the bottom.
                        let dots = 1 + (n * 3.0).round() as usize;
                        let column: [u32; 4] = if half == 0 {
                            [0x40, 0x04, 0x02, 0x01]
                        } else {
                            [0x80, 0x20, 0x10, 0x08]
                        };
                        bits |= column[..dots].iter().sum::<u32>();
                        let s = slot_style(cell * 2 + half, *v);
                        if style.is_none() || s.as_ref() != Some(&base) {
                            style = s;
                        }
                    }
                    let glyph = if bits == 0 {
                        ' '
                    } else {
                        char::from_u32(0x2800 + bits).unwrap_or(' ')
                    };
                    line.push(&glyph.to_string(), style);
                }
            }
            _ => {
                let ramp = if charset == Charset::Ascii {
                    &ASCII
                } else {
                    &BLOCKS
                };
                for (i, (v, _)) in samples.iter().enumerate() {
                    match scale.normalize(*v) {
                        Some(n) => {
                            let glyph = ramp[(n * 7.0).round() as usize];
                            line.push(&glyph.to_string(), slot_style(i, *v));
                        }
                        None => line.push(" ", None),
                    }
                }
            }
        }
        if show_summary {
            for (text, key) in self.summary() {
                line.push(" ", None);
                let style = colour.then(|| theme_style(console, key));
                line.push(&text, style);
            }
        }
        line
    }
}

impl Renderable for Sparkline {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let width = options.max_width;
        lines_to_segments(vec![self.line(console, charset, width)], width)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let natural = self.natural_cells(charset);
        let max = natural + self.summary_width();
        Measurement::new(natural.min(4), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}
