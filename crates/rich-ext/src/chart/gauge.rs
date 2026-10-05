//! Gauges and bullet charts: a value against a range, a target and
//! threshold bands.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{
    cell_units, cells, has_colour, lines_to_segments, theme_style, truncate, user_style,
    wrap_entries, BarChart, Charset, Line, Scale, ValueFormat,
};

/// Bar cells in the compact form when [`Gauge::bar_width`] is not set.
const DEFAULT_BAR: usize = 20;
/// The narrowest bar drawn before the band word, the value and the label
/// give way.
const MIN_BAR: usize = 4;

/// Track shades for alternate bands: blocks, then ASCII.
const SHADES: [char; 2] = ['░', '▒'];
const ASCII_SHADES: [char; 2] = ['.', ':'];

/// A threshold band of a [`Gauge`]: from the band before it (or the bottom
/// of the scale) up to and including [`upto`](Self::upto).
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    /// The top of the band, inclusive.
    pub upto: f64,
    /// The band's name, written after the value when the value is in it.
    pub label: String,
    /// The bar's style while the value is in this band: a theme key or a
    /// definition. `None` keeps the gauge's style.
    pub style: Option<String>,
}

impl Band {
    /// A band up to `upto` called `label`.
    pub fn new(upto: f64, label: impl Into<String>) -> Self {
        Band {
            upto,
            label: label.into(),
            style: None,
        }
    }

    /// The bar's style while the value is in this band, such as
    /// `chart.critical`.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }
}

/// Column widths for a gauge line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Columns {
    label: usize,
    bar: usize,
    value: usize,
    band: usize,
}

/// Fit `natural` columns into `width`: the bar shrinks to 4 cells, then the
/// band word goes, then the value, then the label is cut. `full` makes the
/// bar take all the room left.
fn fit(width: usize, natural: Columns, full: bool) -> Columns {
    let gap = |w: usize| usize::from(w > 0);
    let fixed =
        |c: &Columns| c.label + gap(c.label) + c.value + gap(c.value) + c.band + gap(c.band);
    let floor = if full {
        MIN_BAR
    } else {
        MIN_BAR.min(natural.bar)
    };
    let mut c = natural;
    for step in 0..3 {
        if width >= fixed(&c) + floor {
            let room = width - fixed(&c);
            c.bar = if full { room } else { c.bar.min(room) };
            return c;
        }
        match step {
            0 => c.band = 0,
            _ => c.value = 0,
        }
    }
    c.label = width.saturating_sub(floor + 1).min(c.label);
    // A label cut to one cell would be only the `…`.
    if c.label < 2 {
        c.label = 0;
    }
    c.bar = width.saturating_sub(fixed(&c)).max(1).min(width.max(1));
    c
}

/// A value against a range: a bar, a target marker and threshold bands.
///
/// - The bar fills from the bottom of the scale to the value with
///   eighth-cell blocks (`█▉▊▋▌▍▎▏`), half-cell Braille (`⣿⡇`) or `#` and a
///   half `=` in ASCII. The rest of the track is shaded by band, alternating
///   `░` and `▒` (`.` and `:` in ASCII) so each band's extent shows.
/// - [`target`](Self::target) draws a `│` (`|`) across the bar where the
///   target is.
/// - [`band`](Self::band) adds threshold bands. The bar takes the style of
///   the band the value is in, and the band's name is written after the
///   value, so the state reads without colour.
/// - The scale runs from 0 (or the smallest of the value, the target and
///   the bands) to the largest of them; [`range`](Self::range) fixes it.
/// - The compact form (the default) is one line: the label, a bar of
///   [`bar_width`](Self::bar_width) cells (20 by default), the value and
///   the band. [`full_width`](Self::full_width) makes the bar fill the width
///   and adds a scale line (the bounds, the target and the band edges, each
///   under its cell) and a legend naming the bands and the target.
/// - Given less width, the bar shrinks to 4 cells, then the band word goes,
///   then the value, then the label is cut.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Band, Charset, Gauge};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let gauge = Gauge::new("cpu", 72.0)
///     .range(0.0, 100.0)
///     .target(80.0)
///     .band(Band::new(60.0, "ok"))
///     .band(Band::new(85.0, "high"))
///     .band(Band::new(100.0, "critical"))
///     .unit("%")
///     .bar_width(20);
/// assert_eq!(
///     console.render_export(&gauge),
///     "cpu ██████████████▍▒│░░░ 72% high\n"
/// );
/// assert_eq!(
///     console.render_export(&gauge.clone().charset(Charset::Ascii)),
///     "cpu ##############=:|... 72% high\n"
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Gauge {
    label: String,
    value: f64,
    min: Option<f64>,
    max: Option<f64>,
    target: Option<f64>,
    bands: Vec<Band>,
    charset: Charset,
    bar_width: Option<usize>,
    full_width: bool,
    format: ValueFormat,
    unit: String,
    style: Option<String>,
}

impl Gauge {
    /// A gauge called `label` showing `value`. An empty label leaves the
    /// label column out.
    pub fn new(label: impl Into<String>, value: f64) -> Self {
        Gauge {
            label: label.into(),
            value,
            min: None,
            max: None,
            target: None,
            bands: Vec::new(),
            charset: Charset::Auto,
            bar_width: None,
            full_width: false,
            format: ValueFormat::Compact,
            unit: String::new(),
            style: None,
        }
    }

    /// Fix the scale; values outside it are clamped.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Mark a target on the bar.
    pub fn target(mut self, target: f64) -> Self {
        self.target = Some(target);
        self
    }

    /// Add a threshold band. Bands may be added in any order; each runs
    /// from the next lower band's top.
    pub fn band(mut self, band: Band) -> Self {
        self.bands.push(band);
        self.bands.sort_by(|a, b| a.upto.total_cmp(&b.upto));
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Cells for the bar in the compact form (default 20, at least 1).
    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = Some(width.max(1));
        self
    }

    /// Fill the width given, with a scale line and a legend under the bar
    /// (default off: one compact line).
    pub fn full_width(mut self, full: bool) -> Self {
        self.full_width = full;
        self
    }

    /// How values are written (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// Written right after every value, such as `%` or ` ms`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// The bar's style outside any styled band: a theme key or a definition
    /// (default `chart.bar`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// The value.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The band the value is in: the first whose top is at or above it.
    /// `None` without bands, above the last band, or for NaN.
    pub fn current_band(&self) -> Option<&Band> {
        if !self.value.is_finite() {
            return None;
        }
        self.band_at(self.value)
    }

    fn band_at(&self, v: f64) -> Option<&Band> {
        self.bands.iter().find(|b| v <= b.upto)
    }

    fn scale(&self) -> Scale {
        let values = [0.0, self.value]
            .into_iter()
            .chain(self.target)
            .chain(self.bands.iter().map(|b| b.upto));
        Scale::from_values(values).bounds(self.min, self.max)
    }

    fn text(&self, v: f64) -> String {
        let value = self.format.format(v);
        if v.is_finite() {
            format!("{value}{}", self.unit)
        } else {
            value
        }
    }

    fn natural(&self) -> Columns {
        Columns {
            label: cells(&self.label),
            bar: self.bar_width.unwrap_or(DEFAULT_BAR),
            value: cells(&self.text(self.value)),
            band: self.current_band().map_or(0, |b| cells(&b.label)),
        }
    }

    fn natural_width(&self) -> usize {
        let c = self.natural();
        let gap = |w: usize| usize::from(w > 0);
        c.label + gap(c.label) + c.bar + gap(c.value) + c.value + gap(c.band) + c.band
    }

    /// The band index of the cell at `i` of `width`, by its centre.
    fn band_index(&self, scale: &Scale, i: usize, width: usize) -> usize {
        let centre = scale.min() + (i as f64 + 0.5) / width as f64 * scale.span();
        self.bands
            .iter()
            .position(|b| centre <= b.upto)
            .unwrap_or(self.bands.len())
    }

    /// The bar's cells for `width` cells.
    fn bar(&self, console: &Console, charset: Charset, width: usize) -> Line {
        let ascii = charset == Charset::Ascii;
        let colour = has_colour(console);
        let scale = self.scale();
        let bar_style = colour.then(
            || match self.current_band().and_then(|b| b.style.as_ref()) {
                Some(style) => user_style(console, style),
                None => self
                    .style
                    .as_deref()
                    .map(|s| user_style(console, s))
                    .unwrap_or_else(|| theme_style(console, "chart.bar")),
            },
        );
        let track_style = colour.then(|| theme_style(console, "chart.track"));
        let target_style = colour.then(|| theme_style(console, "chart.target"));
        let mut row: Vec<(char, Option<Style>)> = Vec::with_capacity(width);
        if let Some(n) = scale.normalize(self.value) {
            let glyphs = BarChart::glyphs(charset, n * width as f64, self.value > scale.min());
            row.extend(glyphs.chars().take(width).map(|c| (c, bar_style.clone())));
        }
        let shades = if ascii { ASCII_SHADES } else { SHADES };
        for i in row.len()..width {
            let shade = shades[self.band_index(&scale, i, width) % 2];
            row.push((shade, track_style.clone()));
        }
        if let Some(col) = self.target_column(&scale, width) {
            row[col] = (if ascii { '|' } else { '│' }, target_style);
        }
        let mut line = Line::new();
        for (c, style) in row {
            line.push(&c.to_string(), style);
        }
        line
    }

    fn target_column(&self, scale: &Scale, width: usize) -> Option<usize> {
        let n = scale.normalize(self.target?)?;
        (width > 0).then(|| ((n * width as f64).floor() as usize).min(width - 1))
    }

    /// One gauge line at the given column widths.
    fn line(&self, console: &Console, charset: Charset, c: Columns) -> Line {
        let ascii = charset == Charset::Ascii;
        let colour = has_colour(console);
        let label_style = colour.then(|| theme_style(console, "chart.label"));
        let value_style = colour.then(|| theme_style(console, "chart.value"));
        let mut line = Line::new();
        if c.label > 0 {
            let label = truncate(&self.label, c.label, ascii);
            let pad = c.label - cells(&label);
            line.push(&label, label_style);
            line.pad(pad + 1);
        }
        line.append(self.bar(console, charset, c.bar));
        if c.value > 0 {
            let value = self.text(self.value);
            line.pad(1 + c.value.saturating_sub(cells(&value)));
            line.push(&truncate(&value, c.value, ascii), value_style);
        }
        if c.band > 0 {
            line.pad(1);
            let band = self.current_band();
            let text = band.map(|b| b.label.as_str()).unwrap_or("");
            let text = truncate(text, c.band, ascii);
            let style = colour
                .then(|| {
                    band.and_then(|b| b.style.as_ref())
                        .map(|s| user_style(console, s))
                })
                .flatten();
            let pad = c.band - cells(&text);
            line.push(&text, style);
            line.pad(pad);
        }
        line
    }

    /// The scale line: bounds, target and band edges under their cells.
    fn scale_line(&self, console: &Console, ascii: bool, offset: usize, width: usize) -> Line {
        let scale = self.scale();
        let mut row: Vec<String> = vec![" ".to_string(); width];
        let mut taken: Vec<(usize, usize)> = Vec::new();
        // Each label goes at the first of its places that is free.
        let mut place = |text: &str, starts: &[usize]| {
            let text = truncate(text, width, ascii);
            let len = cells(&text);
            if len == 0 {
                return;
            }
            for &start in starts {
                let end = start + len;
                if end > width || taken.iter().any(|&(a, b)| start < b + 1 && a < end + 1) {
                    continue;
                }
                for (i, unit) in cell_units(&text).into_iter().enumerate() {
                    row[start + i] = unit;
                }
                taken.push((start, end));
                return;
            }
        };
        // Centred on the column, else ending at it, else starting at it.
        let around = |col: usize, len: usize| {
            [
                col.saturating_sub(len / 2),
                (col + 1).saturating_sub(len),
                col,
            ]
        };
        let lo = self.text(scale.min());
        let hi = self.text(scale.max());
        place(&lo, &[0]);
        place(&hi, &[width.saturating_sub(cells(&hi))]);
        if let (Some(t), Some(col)) = (self.target, self.target_column(&scale, width)) {
            let text = self.text(t);
            place(&text, &around(col, cells(&text)));
        }
        for band in &self.bands {
            if band.upto >= scale.max() || band.upto <= scale.min() {
                continue;
            }
            let n = scale.normalize(band.upto).unwrap_or(0.0);
            let col = ((n * width as f64).round() as usize).min(width);
            let text = self.text(band.upto);
            place(&text, &around(col, cells(&text)));
        }
        let mut line = Line::new();
        line.pad(offset);
        let style = has_colour(console).then(|| theme_style(console, "chart.axis"));
        line.push(&row.concat(), style);
        line
    }

    fn legend(&self, console: &Console, ascii: bool, width: usize) -> Vec<Line> {
        let colour = has_colour(console);
        let track = colour.then(|| theme_style(console, "chart.track"));
        let label = colour.then(|| theme_style(console, "chart.label"));
        let shades = if ascii { ASCII_SHADES } else { SHADES };
        let mut entries = Vec::new();
        for (i, band) in self.bands.iter().enumerate() {
            let name = truncate(&band.label, usize::MAX, ascii);
            entries.push(vec![
                (shades[i % 2].to_string(), track.clone()),
                (
                    format!(" {name} up to {}", self.text(band.upto)),
                    label.clone(),
                ),
            ]);
        }
        if let Some(t) = self.target {
            entries.push(vec![
                (
                    if ascii { "|" } else { "│" }.to_string(),
                    colour.then(|| theme_style(console, "chart.target")),
                ),
                (format!(" target {}", self.text(t)), label.clone()),
            ]);
        }
        wrap_entries(entries, width)
    }
}

impl Renderable for Gauge {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let c = fit(width, self.natural(), self.full_width);
        let mut lines = vec![self.line(console, charset, c)];
        if self.full_width {
            let ascii = charset == Charset::Ascii;
            let offset = if c.label > 0 { c.label + 1 } else { 0 };
            lines.push(self.scale_line(console, ascii, offset, c.bar));
            lines.extend(self.legend(console, ascii, width));
        }
        lines_to_segments(lines, width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let max = if self.full_width {
            options.max_width
        } else {
            self.natural_width()
        };
        Measurement::new(MIN_BAR.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// Several [`Gauge`]s, one per line, with their labels, bars, values and
/// band words in aligned columns. Each gauge keeps its own scale, target
/// and bands.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Band, BulletChart, Gauge};
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let chart = BulletChart::new()
///     .gauge(Gauge::new("revenue", 270.0).range(0.0, 300.0).target(250.0))
///     .gauge(Gauge::new("profit", 22.5).range(0.0, 30.0).target(26.0))
///     .bar_width(12);
/// assert_eq!(
///     console.render_export(&chart),
///     "revenue ██████████│░  270\n\
///      profit  █████████░│░ 22.5\n"
/// );
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BulletChart {
    gauges: Vec<Gauge>,
    bar_width: Option<usize>,
    full_width: bool,
    charset: Charset,
}

impl BulletChart {
    /// An empty chart; add rows with [`gauge`](Self::gauge).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a gauge as the next row.
    pub fn gauge(mut self, gauge: Gauge) -> Self {
        self.gauges.push(gauge);
        self
    }

    /// Cells for every bar (default 20).
    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = Some(width.max(1));
        self
    }

    /// Make the bars fill the width given (default off).
    pub fn full_width(mut self, full: bool) -> Self {
        self.full_width = full;
        self
    }

    /// Glyphs to draw every row with (default [`Charset::Auto`]: blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    fn natural(&self) -> Columns {
        let mut c = Columns {
            label: 0,
            bar: self.bar_width.unwrap_or(DEFAULT_BAR),
            value: 0,
            band: 0,
        };
        for g in &self.gauges {
            let n = g.natural();
            c.label = c.label.max(n.label);
            c.value = c.value.max(n.value);
            c.band = c.band.max(n.band);
        }
        c
    }
}

impl Renderable for BulletChart {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        if self.gauges.is_empty() {
            let mut line = Line::new();
            line.push(&truncate("no data", width, charset == Charset::Ascii), None);
            return lines_to_segments(vec![line], width);
        }
        let c = fit(width, self.natural(), self.full_width);
        let lines = self
            .gauges
            .iter()
            .map(|g| g.line(console, charset, c))
            .collect();
        lines_to_segments(lines, width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        if self.gauges.is_empty() {
            return Measurement::new(7, 7).with_maximum(options.max_width);
        }
        let max = if self.full_width {
            options.max_width
        } else {
            let c = self.natural();
            let gap = |w: usize| usize::from(w > 0);
            c.label + gap(c.label) + c.bar + gap(c.value) + c.value + gap(c.band) + c.band
        };
        Measurement::new(MIN_BAR.min(max), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}
