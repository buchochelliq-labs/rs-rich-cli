//! Bar charts, horizontal and vertical, and histograms.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::axis::exact_labels;
use super::scale::{along, clean};
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

/// Vertical bars, bottom up: 1/8 to 7/8 of a cell.
const LOWER_EIGHTHS: [char; 7] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇'];
/// Vertical [`Charset::Braille`] and [`Charset::Ascii`] half cells.
const BRAILLE_LOWER_HALF: char = '⣤';
const ASCII_LOWER_HALF: char = '.';
/// Rows for the tallest vertical bar when [`BarChart::bar_width`] is not set.
const DEFAULT_COLUMN: usize = 8;
/// The tallest vertical bar drawn, in rows, whatever
/// [`BarChart::bar_width`] asks for.
const MAX_COLUMN: usize = u16::MAX as usize;
/// The thickest vertical bar, in cells.
const MAX_THICKNESS: usize = 3;

/// Which way a [`BarChart`]'s bars run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Left to right, one bar per line with its label before it and its
    /// value after (the default).
    #[default]
    Horizontal,
    /// Bottom to top, side by side, with labels under the bars and values
    /// just above them (below, for negative values).
    Vertical,
}

/// How a vertical chart fits its width.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Columns {
    /// Cells per bar, its label and its value.
    slot: usize,
    /// Cells between slots.
    gap: usize,
    /// Cells of bar glyphs, centred in the slot.
    thickness: usize,
    labels: bool,
    values: bool,
    /// How many bars fit, from the first.
    shown: usize,
}

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

/// Bars: by default horizontal, a label, a bar and the value on each line;
/// with [`Orientation::Vertical`], columns side by side.
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
/// - [`Orientation::Vertical`] draws columns bottom up, as tall as
///   [`bar_width`](Self::bar_width) rows (8 by default): eighth-cell blocks
///   (`▁▂▃▄▅▆▇█`), half-cell Braille (`⣤⣿`), or `#` and a half `.` in
///   ASCII. Each bar has a slot as wide as the widest label or value, with
///   a cell between slots, and is up to 3 cells thick. Labels go under the
///   bars and each value just above its bar; negative values hang below
///   the zero line at whole-cell precision, their values under them. Given
///   less width the values go, then the labels are cut (and dropped below
///   two cells), then the gaps, and bars that still do not fit are left
///   off the right.
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
///     console.render_export(&chart),
///     "api    ████████   31\n\
///      web    ███▎     12.5\n\
///      worker █           4\n"
/// );
/// let ascii = chart.charset(Charset::Ascii);
/// assert_eq!(
///     console.render_export(&ascii),
///     "api    ########   31\n\
///      web    ###      12.5\n\
///      worker #           4\n"
/// );
/// ```
///
/// Vertical:
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{BarChart, Orientation};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let chart = BarChart::from_pairs([("mon", 4.0), ("tue", 7.5), ("wed", -2.0)])
///     .orientation(Orientation::Vertical)
///     .bar_width(4);
/// assert_eq!(
///     console.render_export(&chart),
///     concat!(
///         "    7.5    \n",
///         " 4  ███    \n",
///         "▅▅▅ ███    \n",
///         "███ ███    \n",
///         "        ███\n",
///         "        -2 \n",
///         "mon tue wed\n",
///     )
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
    orientation: Orientation,
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
            orientation: Orientation::Horizontal,
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

    /// Which way the bars run (default [`Orientation::Horizontal`]).
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Cells for the longest bar (default 40, at least 1); rows for the
    /// tallest when [`Orientation::Vertical`] (default 8, at most 65535).
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
        self.bar_width.unwrap_or(match self.orientation {
            Orientation::Horizontal => DEFAULT_BAR,
            Orientation::Vertical => DEFAULT_COLUMN,
        })
    }

    /// The vertical layout for `width` cells.
    fn columns(&self, width: usize) -> Columns {
        let n = self.bars.len().max(1);
        let label = self.label_width();
        let value = self.value_width();
        let fits = |slot: usize, gap: usize| n * slot + (n - 1) * gap <= width;
        let full = |slot: usize, labels: bool, values: bool| Columns {
            slot,
            gap: 1,
            thickness: slot.min(MAX_THICKNESS),
            labels,
            values,
            shown: n,
        };
        let with_values = label.max(value).max(1);
        if fits(with_values, 1) {
            return full(with_values, label > 0, value > 0);
        }
        let without = label.max(1);
        if fits(without, 1) {
            return full(without, label > 0, false);
        }
        let room = (width + 1) / n;
        if room >= 2 {
            // A label cut to one cell would be only the `…`.
            return full(room - 1, label > 0 && room > 2, false);
        }
        Columns {
            slot: 1,
            gap: 0,
            thickness: 1,
            labels: false,
            values: false,
            shown: n.min(width.max(1)),
        }
    }

    /// A vertical bar's cells, bottom up, for `length` rows (a fraction
    /// of a row allowed).
    fn column_glyphs(charset: Charset, length: f64, sliver: bool) -> Vec<char> {
        let full = length.floor() as usize;
        let rest = length - full as f64;
        let mut out = Vec::new();
        match charset {
            Charset::Ascii | Charset::Braille => {
                let (whole, half) = if charset == Charset::Ascii {
                    (ASCII_FULL, ASCII_LOWER_HALF)
                } else {
                    (BRAILLE_FULL, BRAILLE_LOWER_HALF)
                };
                let halves = (rest * 2.0).round() as usize;
                out.extend(std::iter::repeat_n(whole, full + halves / 2));
                if halves == 1 || (out.is_empty() && sliver) {
                    out.push(half);
                }
            }
            _ => {
                let eighths = (rest * 8.0).round() as usize;
                out.extend(std::iter::repeat_n(FULL_BLOCK, full + eighths / 8));
                if !eighths.is_multiple_of(8) {
                    out.push(LOWER_EIGHTHS[eighths % 8 - 1]);
                } else if out.is_empty() && sliver {
                    out.push(LOWER_EIGHTHS[0]);
                }
            }
        }
        out
    }

    fn bar_style(&self, console: &Console, bar: &Bar, negative: bool) -> Style {
        match (&bar.style, &self.style) {
            (Some(s), _) => user_style(console, s),
            (None, _) if negative => theme_style(console, "chart.negative"),
            (None, Some(s)) => user_style(console, s),
            (None, None) => theme_style(console, "chart.bar"),
        }
    }

    fn vertical_lines(&self, console: &Console, width: usize, charset: Charset) -> Vec<Line> {
        let ascii = charset == Charset::Ascii;
        let colour = has_colour(console);
        let layout = self.columns(width);
        let shown = layout.shown;
        let width = (shown * layout.slot + shown.saturating_sub(1) * layout.gap).min(width);
        let height = self.natural_bar().min(MAX_COLUMN);
        let scale = self.scale();
        let zero = scale.normalize(0.0).unwrap_or(0.0) * height as f64;
        let zero_row = (zero.round() as usize).min(height);
        let full = match charset {
            Charset::Ascii => ASCII_FULL,
            Charset::Braille => BRAILLE_FULL,
            _ => FULL_BLOCK,
        };
        let label_style = colour.then(|| theme_style(console, "chart.label"));
        let value_style = colour.then(|| theme_style(console, "chart.value"));

        // Rows counted from the bottom of the plot; a value may sit one row
        // above it or one below.
        let (bottom, top) = (-1i64, height as i64);
        let rows = (top - bottom + 1) as usize;
        let mut grid: Vec<Vec<(char, Option<Style>)>> = vec![vec![(' ', None); width]; rows];
        let mut put = |row: i64, col: usize, c: char, style: Option<Style>| {
            if (bottom..=top).contains(&row) && col < width {
                grid[(top - row) as usize][col] = (c, style);
            }
        };
        let mut used = (0i64, height as i64 - 1);
        for (index, bar) in self.bars.iter().take(layout.shown).enumerate() {
            let left = index * (layout.slot + layout.gap);
            let bar_left = left + (layout.slot - layout.thickness) / 2;
            let style = colour.then(|| self.bar_style(console, bar, bar.value < 0.0));
            // Draw the bar; the row its value goes in.
            let value_row = match scale.normalize(bar.value) {
                None => zero_row as i64,
                Some(n) if bar.value < 0.0 => {
                    let start = (n * height as f64).round() as usize;
                    let len = zero_row.saturating_sub(start).max(1).min(zero_row);
                    for row in zero_row - len..zero_row {
                        for col in bar_left..bar_left + layout.thickness {
                            put(row as i64, col, full, style.clone());
                        }
                    }
                    (zero_row - len) as i64 - 1
                }
                Some(n) => {
                    let length = (n * height as f64 - zero).max(0.0);
                    let glyphs = Self::column_glyphs(charset, length, bar.value > 0.0);
                    let drawn = glyphs.len().min(height - zero_row);
                    for (i, glyph) in glyphs.iter().take(drawn).enumerate() {
                        for col in bar_left..bar_left + layout.thickness {
                            put((zero_row + i) as i64, col, *glyph, style.clone());
                        }
                    }
                    (zero_row + drawn) as i64
                }
            };
            if layout.values {
                let value = self.format.format(bar.value);
                let start = left + (layout.slot - cells(&value)) / 2;
                for (i, c) in value.chars().enumerate() {
                    put(value_row, start + i, c, value_style.clone());
                }
                used = (used.0.min(value_row), used.1.max(value_row));
            }
        }

        // Only the rows above and below the plot that hold a value stay.
        let mut out: Vec<Line> = grid
            .into_iter()
            .enumerate()
            .filter(|(i, _)| (used.0..=used.1).contains(&(top - *i as i64)))
            .map(|(_, row)| {
                let mut line = Line::new();
                for (c, style) in row {
                    line.push(&c.to_string(), style);
                }
                line
            })
            .collect();
        if layout.labels {
            let mut line = Line::new();
            for (index, bar) in self.bars.iter().take(layout.shown).enumerate() {
                if index > 0 {
                    line.pad(layout.gap);
                }
                let label = truncate(&bar.label, layout.slot, ascii);
                let pad = layout.slot - cells(&label);
                line.pad(pad / 2);
                line.push(&label, label_style.clone());
                line.pad(pad - pad / 2);
            }
            line.pad(width.saturating_sub(line.width()));
            out.push(line);
        }
        out
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
    pub(crate) fn glyphs(charset: Charset, length: f64, sliver: bool) -> String {
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
        if self.orientation == Orientation::Vertical {
            return self.vertical_lines(console, width, charset);
        }
        let (label_w, bar_w, value_w) = self.layout(width);
        let scale = self.scale();
        let zero = scale.normalize(0.0).unwrap_or(0.0) * bar_w as f64;
        let zero_cell = zero.round() as usize;
        let style_of =
            |bar: &Bar, negative: bool| colour.then(|| self.bar_style(console, bar, negative));
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
        if self.orientation == Orientation::Vertical {
            let n = self.bars.len();
            let slot = self.label_width().max(self.value_width()).max(1);
            return n.saturating_mul(slot).saturating_add(n.saturating_sub(1));
        }
        let l = self.label_width();
        let v = self.value_width();
        (l + usize::from(l > 0) + v + usize::from(v > 0)).saturating_add(self.natural_bar())
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
        let min = match self.orientation {
            Orientation::Horizontal => MIN_BAR,
            Orientation::Vertical => self.bars.len(),
        };
        Measurement::new(min.min(max), max)
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
/// one, the one its label names; NaN and infinite values are not counted,
/// and values outside an explicit range are left out. The edges are
/// written exactly: in full (`[1000, 1250)`) when the compact form would
/// round one of them.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::Histogram;
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let latencies = [12.0, 14.0, 15.0, 21.0, 22.0, 23.0, 24.0, 38.0];
/// let histogram = Histogram::new(latencies).bins(3).range(10.0, 40.0).bar_width(8);
/// assert_eq!(
///     console.render_export(&histogram),
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

    /// Which way the bars run (default [`Orientation::Horizontal`]).
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.chart = self.chart.orientation(orientation);
        self
    }

    /// Cells for the tallest bin (default 40); rows when vertical
    /// (default 8).
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

    /// The bin edges, `bins + 1` of them, low to high, without
    /// floating-point noise (`0.3`, not `0.30000000000000004`).
    ///
    /// With an explicit [`range`](Self::range) the bins split it exactly.
    /// Otherwise the bin width is rounded up to 1, 2, 2.5 or 5 times a power
    /// of ten and the first edge down to a multiple of it, so the labels are
    /// round numbers; the last bins may then be empty. A range too wide for
    /// round bins (near `±f64::MAX`) is split exactly.
    pub fn edges(&self) -> Vec<f64> {
        let data = Scale::from_values(self.values.iter().copied());
        let bins = self.bins as f64;
        let split = |scale: Scale| -> Vec<f64> {
            (0..=self.bins)
                .map(|i| clean(scale.lerp(i as f64 / bins)))
                .collect()
        };
        if self.min.is_some() || self.max.is_some() {
            let scale = data.bounds(self.min, self.max);
            let step = scale.span() / bins;
            if (scale.max() - scale.min()).is_finite() {
                return (0..=self.bins)
                    .map(|i| clean(scale.min() + step * i as f64))
                    .collect();
            }
            return split(scale);
        }
        let mut step = bin_step(data.span() / bins);
        let mut start = (data.min() / step).floor() * step;
        // Rounding the start down can leave the top uncovered: widen.
        while step.is_finite() && along(start, bins, step) < data.max() {
            step = bin_step(step * 1.001);
            start = (data.min() / step).floor() * step;
        }
        if !(start.is_finite() && along(start, bins, step).is_finite()) {
            return split(data);
        }
        (0..=self.bins)
            .map(|i| clean(along(start, i as f64, step)))
            .collect()
    }

    /// How many values fall in each bin: `[low, high)`, the last
    /// `[low, high]`, against the edges [`edges`](Self::edges) gives.
    pub fn counts(&self) -> Vec<usize> {
        let edges = self.edges();
        let (lo, hi) = (edges[0], edges[self.bins]);
        let inner = &edges[1..self.bins];
        let mut counts = vec![0; self.bins];
        for v in self.values.iter().copied().filter(|v| v.is_finite()) {
            if v < lo || v > hi {
                continue;
            }
            // The bins whose low edge is at or below `v`, past the first.
            let bin = inner.partition_point(|&edge| edge <= v);
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
        let written = exact_labels(ValueFormat::Compact, &edges)
            .map(|(labels, _)| labels)
            .unwrap_or_else(|| {
                edges
                    .iter()
                    .map(|&e| ValueFormat::Compact.format(e))
                    .collect()
            });
        let counts = self.counts();
        let mut chart = BarChart {
            bars: Vec::new(),
            ..self.chart.clone()
        };
        for (i, count) in counts.into_iter().enumerate() {
            let close = if i + 1 == self.bins { ']' } else { ')' };
            let label = format!("[{}, {}{close}", written[i], written[i + 1]);
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
