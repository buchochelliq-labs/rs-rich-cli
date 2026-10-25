//! A spectrogram, or waterfall: spectra over time, the newest at the top,
//! each a row of shades, as cava, spek and Audacity draw them.

use std::collections::VecDeque;

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{has_colour, lines_to_segments, theme_style, AxisScale, Charset, Line, Scale};

/// Levels for blocks, lowest first (as a [`Heatmap`](super::Heatmap)'s).
const SHADES: [char; 5] = [' ', '░', '▒', '▓', '█'];
/// Levels for ASCII, lowest first.
const ASCII_SHADES: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];
/// How many `chart.heat.N` styles there are.
const HEAT_STYLES: usize = 5;
/// The tallest spectrogram drawn, in rows.
const MAX_HEIGHT: usize = u16::MAX as usize;

/// Spectra against time: each [`push`](Self::push)ed spectrum (the
/// magnitudes of its bins, lowest frequency first) becomes a row, the
/// newest at the top, and older rows move down until `capacity` are kept.
///
/// - Each column stands for a span of frequencies: the bins are spread
///   across the width, evenly, or by their logarithm with
///   [`scale`](Self::scale)`(AxisScale::Log)` so each octave takes the same
///   width. A column covering several bins shows the loudest; one between
///   bins (the low end of a log scale) shows the nearest.
///   [`resolution`](Self::resolution) says how many Hz apart the bins are
///   and [`bounds`](Self::bounds) which frequencies the width spans
///   (default: every bin, from 0 Hz).
/// - Levels are scaled from the smallest to the largest kept (or
///   [`range`](Self::range)) into the shades of a
///   [`Heatmap`](super::Heatmap): ` ░▒▓█` in blocks, ` .:-=+*#%@` in
///   ASCII, and with colour the `chart.heat.N` styles, cold to hot. A NaN
///   level is `·` (`?` in ASCII); an infinite one is drawn at the end it
///   passes, so the decibels of silence (`-inf`) are the lowest shade.
///   Feed it decibels (`20 log10(magnitude)`) to see quiet partials beside
///   loud ones.
/// - It fills the height it is given (a `Layout` region, an intuiTUIve
///   node) unless [`height`](Self::height) is set; given none, it is as
///   tall as the rows it holds. Rows below the oldest are blank.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, Spectrogram};
///
/// let console = Console::builder().width(4).color_system(None).build();
/// let mut waterfall = Spectrogram::new(3).charset(Charset::Ascii).range(0.0, 9.0);
/// waterfall.push([9.0, 0.0, 0.0, 0.0]);
/// waterfall.push([0.0, 9.0, 0.0, 0.0]);
/// waterfall.push([0.0, 0.0, 9.0, 4.5]);
/// // The newest at the top.
/// assert_eq!(console.render_export(&waterfall), "  @+\n @  \n@   \n");
/// waterfall.push([0.0; 4]);
/// // Three kept: the oldest has gone.
/// assert_eq!(console.render_export(&waterfall), "    \n  @+\n @  \n");
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct Spectrogram {
    rows: VecDeque<Vec<f64>>,
    capacity: usize,
    min: Option<f64>,
    max: Option<f64>,
    resolution: f64,
    bounds: Option<(f64, f64)>,
    scale: AxisScale,
    charset: Charset,
    height: Option<usize>,
}

impl Spectrogram {
    /// An empty spectrogram keeping the newest `capacity` spectra (at
    /// least one).
    pub fn new(capacity: usize) -> Self {
        Spectrogram {
            rows: VecDeque::new(),
            capacity: capacity.max(1),
            min: None,
            max: None,
            resolution: 1.0,
            bounds: None,
            scale: AxisScale::Linear,
            charset: Charset::Auto,
            height: None,
        }
    }

    /// Add a spectrum at the top: the levels of its bins, lowest frequency
    /// first. The oldest goes once there are `capacity`.
    pub fn push(&mut self, bins: impl IntoIterator<Item = f64>) {
        if self.rows.len() == self.capacity {
            self.rows.pop_back();
        }
        self.rows.push_front(bins.into_iter().collect());
    }

    /// [`push`](Self::push) as a builder.
    pub fn row(mut self, bins: impl IntoIterator<Item = f64>) -> Self {
        self.push(bins);
        self
    }

    /// Forget every spectrum.
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// How many spectra it holds.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The spectra, newest first.
    pub fn rows(&self) -> impl Iterator<Item = &[f64]> {
        self.rows.iter().map(Vec::as_slice)
    }

    /// The levels shaded lowest and highest (default: the smallest and
    /// largest kept). Levels outside are drawn at the end they pass.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// How many Hz apart the bins are (default 1): `rate / n` for a
    /// spectrum of `n` samples taken at `rate`. Bin `i` is at
    /// `i × resolution` Hz.
    pub fn resolution(mut self, hz: f64) -> Self {
        if hz.is_finite() && hz > 0.0 {
            self.resolution = hz;
        }
        self
    }

    /// The frequencies the width spans, in Hz (default: every bin, from 0
    /// Hz to the top of the last; from the first bin above 0 on a log
    /// scale). Bin `i` covers `i × resolution` up to the next bin.
    pub fn bounds(mut self, low: f64, high: f64) -> Self {
        self.bounds = Some((low.min(high), low.max(high)));
        self
    }

    /// How frequency runs across: evenly (the default) or by its logarithm.
    pub fn scale(mut self, scale: AxisScale) -> Self {
        self.scale = scale;
        self
    }

    /// Which glyphs to draw with (default [`Charset::Auto`]: blocks, or
    /// ASCII on an ASCII-only console).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Height in rows (at most 65535), whatever height it is given.
    pub fn height(mut self, rows: usize) -> Self {
        self.height = Some(rows.min(MAX_HEIGHT));
        self
    }

    /// The scale every level is shaded on.
    pub fn level_scale(&self) -> Scale {
        let values = self.rows.iter().flat_map(|r| r.iter().copied());
        Scale::from_values(values).bounds(self.min, self.max)
    }

    fn bins(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// The frequency span across the width, mapped (Hz, or log10 Hz).
    fn span(&self) -> Option<(f64, f64)> {
        let bins = self.bins();
        if bins == 0 {
            return None;
        }
        let top = bins as f64 * self.resolution;
        let (low, high) = self.bounds.unwrap_or(match self.scale {
            AxisScale::Linear => (0.0, top),
            AxisScale::Log => (self.resolution, top),
        });
        match self.scale {
            AxisScale::Linear => Some((low, high)),
            AxisScale::Log if low > 0.0 && high > 0.0 => Some((low.log10(), high.log10())),
            AxisScale::Log => None,
        }
    }

    /// For each of `width` columns, the bins it shows: a range, never
    /// empty.
    fn columns(&self, width: usize) -> Vec<(usize, usize)> {
        let bins = self.bins();
        let Some((lo, hi)) = self.span() else {
            return Vec::new();
        };
        let unmap = |m: f64| match self.scale {
            AxisScale::Linear => m,
            AxisScale::Log => 10f64.powf(m),
        };
        // The bin a frequency falls in, within the bins there are.
        let bin = |hz: f64| ((hz / self.resolution).max(0.0) as usize).min(bins - 1);
        (0..width)
            .map(|c| {
                let at = |k: f64| lo + (hi - lo) * k / width as f64;
                let (from, to) = (unmap(at(c as f64)), unmap(at(c as f64 + 1.0)));
                let first = (from / self.resolution).ceil().max(0.0) as usize;
                let last = (to / self.resolution).ceil().max(0.0) as usize;
                if first < last && first < bins {
                    (first, last.min(bins))
                } else {
                    let nearest = bin((from + to) / 2.0);
                    (nearest, nearest + 1)
                }
            })
            .collect()
    }

    fn lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let width = options.max_width;
        let height = self
            .height
            .or(options.height)
            .unwrap_or(self.rows.len().max(1))
            .min(MAX_HEIGHT);
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let colour = has_colour(console);
        let shades: &[char] = if ascii { &ASCII_SHADES } else { &SHADES };
        let missing = if ascii { "?" } else { "·" };
        let levels = shades.len();
        let scale = self.level_scale();
        let columns = self.columns(width);
        let style = |level: usize| {
            let key = format!("chart.heat.{}", level * HEAT_STYLES / levels + 1);
            colour.then(|| theme_style(console, &key))
        };
        (0..height)
            .map(|r| {
                let mut line = Line::new();
                let Some(row) = self.rows.get(r) else {
                    line.pad(width);
                    return line;
                };
                for &(first, last) in &columns {
                    let loudest = row
                        .get(first..last.min(row.len()))
                        .unwrap_or_default()
                        .iter()
                        .copied()
                        .filter(|v| !v.is_nan())
                        .fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v))));
                    let level = loudest
                        .and_then(|v| scale.normalize(v.clamp(scale.min(), scale.max())))
                        .map(|n| ((n * levels as f64).floor() as usize).min(levels - 1));
                    match level {
                        Some(level) => line.push(&shades[level].to_string(), style(level)),
                        None => line.push(missing, None::<Style>),
                    }
                }
                line.pad(width.saturating_sub(columns.len()));
                line
            })
            .collect()
    }
}

impl crate::surface::Draw for Spectrogram {
    fn draw(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        screen: &mut crate::surface::Screen,
        rect: crate::surface::Rect,
    ) {
        super::draw_lines(
            self.lines(console, options),
            options.max_width,
            options,
            screen,
            rect,
        );
    }
}

impl Renderable for Spectrogram {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.lines(console, options), options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let bins = self.bins().max(1);
        Measurement::new(1, bins)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Spectrogram, 12 spectra of 513 bins, 0 to 24k Hz, levels from 0 to 3`,
/// then where the newest spectrum is loudest (`Newest peaks at 440 Hz`).
impl crate::a11y::AccessibleText for Spectrogram {
    fn accessible_text(&self, _width: usize) -> String {
        use super::describe::{counted, extent, range};
        use super::ValueFormat;
        if self.rows.is_empty() {
            return "Spectrogram, no spectra".to_string();
        }
        let mut head = format!(
            "Spectrogram, {} of {}",
            counted(self.rows.len(), "spectrum", "spectra"),
            counted(self.bins(), "bin", "bins")
        );
        if let Some((lo, hi)) = self.span() {
            let unmap = |m: f64| match self.scale {
                AxisScale::Linear => m,
                AxisScale::Log => 10f64.powf(m),
            };
            head.push_str(&format!(
                ", {} Hz",
                range(unmap(lo), unmap(hi), ValueFormat::Compact).trim_start_matches("from ")
            ));
            if self.scale == AxisScale::Log {
                head.push_str(", log scale");
            }
        }
        let levels = self.rows.iter().flat_map(|r| r.iter().copied());
        if let Some((lo, hi)) = extent(levels) {
            head.push_str(&format!(", levels {}", range(lo, hi, ValueFormat::Compact)));
        }
        let mut out = vec![head];
        let newest = &self.rows[0];
        let peak = newest
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .max_by(|a, b| a.1.total_cmp(b.1));
        if let Some((bin, _)) = peak {
            let hz = bin as f64 * self.resolution;
            out.push(format!(
                "Newest peaks at {} Hz",
                ValueFormat::Compact.format(hz)
            ));
        }
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::AccessibleText;

    fn plain(width: usize, spectrogram: &Spectrogram) -> String {
        let console = Console::builder().width(width).color_system(None).build();
        console.render_export(spectrogram)
    }

    #[test]
    fn wide_spectra_show_the_loudest_bin_of_each_column() {
        let mut s = Spectrogram::new(2).charset(Charset::Ascii).range(0.0, 9.0);
        s.push([0.0, 9.0, 0.0, 0.0, 0.0, 0.0, 9.0, 0.0]);
        assert_eq!(plain(4, &s), "@  @\n");
    }

    #[test]
    fn narrow_spectra_repeat_the_nearest_bin() {
        let s = Spectrogram::new(1)
            .charset(Charset::Ascii)
            .range(0.0, 9.0)
            .row([0.0, 9.0]);
        assert_eq!(plain(4, &s), "  @@\n");
    }

    #[test]
    fn a_log_scale_gives_each_octave_the_same_width() {
        // Bins 1 Hz apart up to 16 Hz: 1-2, 2-4, 4-8 and 8-16 Hz a column.
        let mut bins = vec![0.0; 17];
        bins[1] = 9.0;
        bins[12] = 9.0;
        let s = Spectrogram::new(1)
            .charset(Charset::Ascii)
            .range(0.0, 9.0)
            .scale(AxisScale::Log)
            .bounds(1.0, 16.0)
            .row(bins);
        assert_eq!(plain(4, &s), "@  @\n");
    }

    #[test]
    fn missing_levels_and_padding() {
        let s = Spectrogram::new(4)
            .charset(Charset::Ascii)
            .range(0.0, 1.0)
            .height(2)
            .row([f64::NAN, 1.0, f64::NEG_INFINITY]);
        assert_eq!(plain(3, &s), "?@ \n   \n");
        assert_eq!(plain(3, &Spectrogram::new(1)), "   \n");
    }

    #[test]
    fn heat_styles_with_colour() {
        let console = Console::builder()
            .width(2)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Standard))
            .build();
        let s = Spectrogram::new(1).range(0.0, 1.0).row([0.0, 1.0]);
        let out = console.render_export(&s);
        // The loudest in chart.heat.5, red.
        assert!(out.contains("\u{1b}[31m█"), "{out:?}");
    }

    #[test]
    fn accessible_text_names_the_peak() {
        let s = Spectrogram::new(4)
            .resolution(10.0)
            .row([1.0, 3.0, 2.0])
            .row([0.0, 1.0, 5.0]);
        assert_eq!(
            s.accessible_text(80),
            "Spectrogram, 2 spectra of 3 bins, 0 to 30 Hz, levels from 0 to 5\n\
             Newest peaks at 20 Hz"
        );
        assert_eq!(
            Spectrogram::new(1).accessible_text(80),
            "Spectrogram, no spectra"
        );
    }
}
