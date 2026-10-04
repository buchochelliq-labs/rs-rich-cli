//! Horizontal bar charts and histograms.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{
    cells, has_colour, lines_to_segments, theme_style, truncate, user_style, Charset, Line, Scale,
    ValueFormat,
};

/// Partial cells for [`Charset::Blocks`]: 1/8 to 7/8 of a cell.
const EIGHTHS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const FULL_BLOCK: char = '█';
/// [`Charset::Braille`]: a full cell and its left half.
const BRAILLE_FULL: char = '⣿';
const BRAILLE_HALF: char = '⡇';
/// [`Charset::Ascii`]: a full cell and a half.
const ASCII_FULL: char = '#';
const ASCII_HALF: char = '=';

/// The narrowest bar drawn before labels and values give way.
const MIN_BAR: usize = 4;
/// Bar cells when [`BarChart::bar_width`] is not set.
const DEFAULT_BAR: usize = 40;

/// One row of a [`BarChart`].
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// Written before the bar.
    pub label: String,
    /// The bar's length on the chart's scale.
    pub value: f64,
    /// A theme key or style definition for this bar, instead of
    /// `chart.bar` (`chart.negative` below zero).
    pub style: Option<String>,
}

impl Bar {
    /// A bar called `label` of `value`.
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Bar {
            label: label.into(),
            value,
            style: None,
        }
    }

    /// This bar's style: a theme key or a definition.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }
}

/// Horizontal bars: a label, a bar and the value on each line.
///
/// - Lengths are measured from zero on a scale that runs from the smallest
///   to the largest value and always includes zero (override with
///   [`range`](Self::range)). Negative values extend left of zero, at
///   whole-cell precision, in `chart.negative`; their written value keeps
///   its minus sign.
/// - [`Charset::Blocks`] draws with eighth-cell precision (`█▉▊▋▌▍▎▏`),
///   [`Charset::Braille`] with half cells (`⣿⡇`), [`Charset::Ascii`] with
///   `#` and a half `=`. A value above zero always shows at least a sliver.
/// - It measures to the label, the bar ([`bar_width`](Self::bar_width),
///   40 by default) and the value. Given less, the bar shrinks to 4 cells,
///   then the values go, then the labels are cut (with `…`, or `.` in
///   ASCII), then the bar takes whatever is left.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{BarChart, Charset};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let chart = BarChart::new()
///     .bar("api", 31.0)
///     .bar("web", 12.5)
///     .bar("worker", 4.0)
///     .bar_width(8);
/// assert_eq!(
///     console.render_to_string(&chart),
///     "api    ████████   31\n\
///      web    ███▎     12.5\n\
///      worker █           4\n"
/// );
/// let ascii = chart.charset(Charset::Ascii);
/// assert_eq!(
///     console.render_to_string(&ascii),
///     "api    ########   31\n\
///      web    ###      12.5\n\
///      worker #           4\n"
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct BarChart {
    bars: Vec<Bar>,
    min: Option<f64>,
    max: Option<f64>,
    charset: Charset,
    bar_width: Option<usize>,
    show_values: bool,
    format: ValueFormat,
    style: Option<String>,
}

impl Default for BarChart {
    fn default() -> Self {
        Self::new()
    }
}

impl BarChart {
    /// An empty chart; add rows with [`bar`](Self::bar).
    pub fn new() -> Self {
        BarChart {
            bars: Vec::new(),
            min: None,
            max: None,
            charset: Charset::Auto,
            bar_width: None,
            show_values: true,
            format: ValueFormat::Compact,
            style: None,
        }
    }

    /// A chart of `(label, value)` pairs.
    pub fn from_pairs<L: Into<String>>(pairs: impl IntoIterator<Item = (L, f64)>) -> Self {
        pairs
            .into_iter()
            .fold(Self::new(), |chart, (label, value)| chart.bar(label, value))
    }

    /// Add a bar.
    pub fn bar(mut self, label: impl Into<String>, value: f64) -> Self {
        self.bars.push(Bar::new(label, value));
        self
    }

    /// Add a [`Bar`] built with its own style.
    pub fn push(mut self, bar: Bar) -> Self {
        self.bars.push(bar);
        self
    }

    /// The bars.
    pub fn bars(&self) -> &[Bar] {
        &self.bars
    }

    /// Fix the scale (default: the values' range, stretched to include
    /// zero). Values outside are clamped.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Fix only the top of the scale, such as `100.0` for percentages.
    pub fn max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Cells for the longest bar (default 40, at least 1).
    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = Some(width.max(1));
        self
    }

    /// Write each value after its bar (default on).
    pub fn show_values(mut self, show: bool) -> Self {
        self.show_values = show;
        self
    }

    /// How values are written (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// The style of every bar: a theme key or a definition (default
    /// `chart.bar`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    fn scale(&self) -> Scale {
        let values = self.bars.iter().map(|b| b.value);
        let data = Scale::from_values(values);
        let data = if self.bars.iter().any(|b| b.value.is_finite()) {
            data.include_zero()
        } else {
            data
        };
        data.bounds(self.min, self.max)
    }

    fn label_width(&self) -> usize {
        self.bars.iter().map(|b| cells(&b.label)).max().unwrap_or(0)
    }

    fn value_width(&self) -> usize {
        if !self.show_values {
            return 0;
        }
        self.bars
            .iter()
            .map(|b| cells(&self.format.format(b.value)))
            .max()
            .unwrap_or(0)
    }

    fn natural_bar(&self) -> usize {
        self.bar_width.unwrap_or(DEFAULT_BAR)
    }

    /// (label, bar, value) widths for `width` cells, after shrinking.
    fn layout(&self, width: usize) -> (usize, usize, usize) {
        let mut label = self.label_width();
        let mut value = self.value_width();
        let gap = |w: usize| usize::from(w > 0);
        let fixed = |l: usize, v: usize| l + gap(l) + v + gap(v);
        let natural = self.natural_bar();
        let floor = MIN_BAR.min(natural);
        if width >= fixed(label, value) + floor {
            let bar = natural.min(width - fixed(label, value));
            return (label, bar, value);
        }
        value = 0;
        if width >= fixed(label, 0) + floor {
            let bar = natural.min(width - fixed(label, 0));
            return (label, bar, value);
        }
        // Cut labels down, leaving the bar its floor.
        label = width.saturating_sub(floor + 1).min(label);
        let bar = width
            .saturating_sub(fixed(label, 0))
            .max(1)
            .min(width.max(1));
        (label, bar, value)
    }

    /// The glyphs for a bar of `length` cells (a fraction of a cell
    /// allowed).
    fn glyphs(charset: Charset, length: f64, sliver: bool) -> String {
        let full = length.floor() as usize;
        let rest = length - full as f64;
        let mut out = String::new();
        match charset {
            Charset::Ascii | Charset::Braille => {
                let (whole, half) = if charset == Charset::Ascii {
                    (ASCII_FULL, ASCII_HALF)
                } else {
                    (BRAILLE_FULL, BRAILLE_HALF)
                };
                let halves = (rest * 2.0).round() as usize;
                let full = full + halves / 2;
                out.extend(std::iter::repeat_n(whole, full));
                if halves == 1 {
                    out.push(half);
                }
                if out.is_empty() && sliver {
                    out.push(half);
                }
            }
            _ => {
                let eighths = (rest * 8.0).round() as usize;
                let full = full + eighths / 8;
                out.extend(std::iter::repeat_n(FULL_BLOCK, full));
                if !eighths.is_multiple_of(8) {
                    out.push(EIGHTHS[eighths % 8 - 1]);
                }
                if out.is_empty() && sliver {
                    out.push(EIGHTHS[0]);
                }
            }
        }
        out
    }

    pub(crate) fn render_lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let width = options.max_width;
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let ascii = charset == Charset::Ascii;
        let colour = has_colour(console);
        if self.bars.is_empty() {
            let mut line = Line::new();
            line.push(&truncate("no data", width, ascii), None);
            return vec![line];
        }
        let (label_w, bar_w, value_w) = self.layout(width);
        let scale = self.scale();
        let zero = scale.normalize(0.0).unwrap_or(0.0) * bar_w as f64;
        let zero_cell = zero.round() as usize;
        let style_of = |bar: &Bar, negative: bool| -> Option<Style> {
            if !colour {
                return None;
            }
            Some(match (&bar.style, &self.style) {
                (Some(s), _) => user_style(console, s),
                (None, _) if negative => theme_style(console, "chart.negative"),
                (None, Some(s)) => user_style(console, s),
                (None, None) => theme_style(console, "chart.bar"),
            })
        };
        let label_style = colour.then(|| theme_style(console, "chart.label"));
        let value_style = colour.then(|| theme_style(console, "chart.value"));
        self.bars
            .iter()
            .map(|bar| {
                let mut line = Line::new();
                if label_w > 0 {
                    let label = truncate(&bar.label, label_w, ascii);
                    let pad = label_w - cells(&label);
                    line.push(&label, label_style.clone());
                    line.pad(pad + 1);
                }
                let pos = scale.normalize(bar.value);
                match pos {
                    None => line.pad(bar_w),
                    Some(n) if bar.value < 0.0 => {
                        let start = (n * bar_w as f64).round() as usize;
                        let len = zero_cell.saturating_sub(start).max(1).min(zero_cell);
                        let start = zero_cell - len;
                        let glyph = match charset {
                            Charset::Ascii => ASCII_FULL,
                            Charset::Braille => BRAILLE_FULL,
                            _ => FULL_BLOCK,
                        };
                        line.pad(start);
                        line.push(&glyph.to_string().repeat(len), style_of(bar, true));
                        line.pad(bar_w.saturating_sub(zero_cell));
                    }
                    Some(n) => {
                        let length = (n * bar_w as f64 - zero).max(0.0);
                        let glyphs = Self::glyphs(charset, length, bar.value > 0.0);
                        let drawn = cells(&glyphs).min(bar_w - zero_cell.min(bar_w));
                        let text: String = glyphs.chars().take(drawn).collect();
                        line.pad(zero_cell.min(bar_w));
                        line.push(&text, style_of(bar, false));
                        line.pad(bar_w.saturating_sub(zero_cell.min(bar_w) + cells(&text)));
                    }
                }
                if value_w > 0 {
                    // Right-aligned, so the digits line up.
                    let value = self.format.format(bar.value);
                    line.pad(1 + value_w - cells(&value));
                    line.push(&value, value_style.clone());
                }
                line
            })
            .collect()
    }

    fn natural_width(&self) -> usize {
        let l = self.label_width();
        let v = self.value_width();
        l + usize::from(l > 0) + self.natural_bar() + v + usize::from(v > 0)
    }
}

impl Renderable for BarChart {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.render_lines(console, options), options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        if self.bars.is_empty() {
            return Measurement::new(7, 7).with_maximum(options.max_width);
        }
        let max = self.natural_width();
        Measurement::new(MIN_BAR.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// Raw values counted into equal-width bins and drawn as a [`BarChart`],
/// one bar per bin labelled with its range.
///
/// Bins split the values' range (or [`range`](Self::range)) into
/// [`bins`](Self::bins) equal parts (10 by default). Each label is
/// `[low, high)`, the last `[low, high]`, so every value falls in exactly
/// one; NaN and infinite values are not counted, and values outside an
/// explicit range are left out.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::Histogram;
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let latencies = [12.0, 14.0, 15.0, 21.0, 22.0, 23.0, 24.0, 38.0];
/// let histogram = Histogram::new(latencies).bins(3).range(10.0, 40.0).bar_width(8);
/// assert_eq!(
///     console.render_to_string(&histogram),
///     "[10, 20) ██████   3\n\
///      [20, 30) ████████ 4\n\
///      [30, 40] ██       1\n"
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    values: Vec<f64>,
    bins: usize,
    min: Option<f64>,
    max: Option<f64>,
    chart: BarChart,
}

impl Histogram {
    /// A histogram of `values`.
    pub fn new(values: impl IntoIterator<Item = f64>) -> Self {
        Histogram {
            values: values.into_iter().collect(),
            bins: 10,
            min: None,
            max: None,
            chart: BarChart::new(),
        }
    }

    /// How many bins (default 10, at least 1).
    pub fn bins(mut self, bins: usize) -> Self {
        self.bins = bins.max(1);
        self
    }

    /// The range the bins cover (default: the values' range).
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.chart = self.chart.charset(charset);
        self
    }

    /// Cells for the tallest bin (default 40).
    pub fn bar_width(mut self, width: usize) -> Self {
        self.chart = self.chart.bar_width(width);
        self
    }

    /// Write each count after its bar (default on).
    pub fn show_values(mut self, show: bool) -> Self {
        self.chart = self.chart.show_values(show);
        self
    }

    /// The bars' style: a theme key or a definition (default `chart.bar`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.chart = self.chart.style(style);
        self
    }

    /// The bin edges, `bins + 1` of them, low to high.
    ///
    /// With an explicit [`range`](Self::range) the bins split it exactly.
    /// Otherwise the bin width is rounded up to 1, 2, 2.5 or 5 times a power
    /// of ten and the first edge down to a multiple of it, so the labels are
    /// round numbers; the last bins may then be empty.
    pub fn edges(&self) -> Vec<f64> {
        let data = Scale::from_values(self.values.iter().copied());
        let bins = self.bins as f64;
        let (start, step) = if self.min.is_some() || self.max.is_some() {
            let scale = data.bounds(self.min, self.max);
            (scale.min(), scale.span() / bins)
        } else {
            let mut step = bin_step(data.span() / bins);
            let mut start = (data.min() / step).floor() * step;
            // Rounding the start down can leave the top uncovered: widen.
            while start + step * bins < data.max() {
                step = bin_step(step * 1.001);
                start = (data.min() / step).floor() * step;
            }
            (start, step)
        };
        (0..=self.bins).map(|i| start + step * i as f64).collect()
    }

    /// How many values fall in each bin.
    pub fn counts(&self) -> Vec<usize> {
        let edges = self.edges();
        let (lo, hi) = (edges[0], edges[self.bins]);
        let width = (hi - lo) / self.bins as f64;
        let mut counts = vec![0; self.bins];
        for v in self.values.iter().copied().filter(|v| v.is_finite()) {
            if v < lo || v > hi {
                continue;
            }
            let bin = (((v - lo) / width).floor() as usize).min(self.bins - 1);
            counts[bin] += 1;
        }
        counts
    }

    /// The [`BarChart`] this histogram draws.
    pub fn to_bar_chart(&self) -> BarChart {
        if !self.values.iter().any(|v| v.is_finite()) {
            return BarChart {
                bars: Vec::new(),
                ..self.chart.clone()
            };
        }
        let edges = self.edges();
        let format = ValueFormat::Compact;
        let counts = self.counts();
        let mut chart = BarChart {
            bars: Vec::new(),
            ..self.chart.clone()
        };
        for (i, count) in counts.into_iter().enumerate() {
            let close = if i + 1 == self.bins { ']' } else { ')' };
            let label = format!(
                "[{}, {}{close}",
                format.format(edges[i]),
                format.format(edges[i + 1])
            );
            chart.bars.push(Bar::new(label, count as f64));
        }
        chart
    }
}

impl Renderable for Histogram {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.to_bar_chart().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.to_bar_chart().measure(console, options)
    }
}

/// The smallest of 1, 2, 2.5 and 5 times a power of ten that is at least `x`.
fn bin_step(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let power = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|m| m * power)
        .find(|step| *step >= x * (1.0 - 1e-12))
        .unwrap_or(10.0 * power)
}
