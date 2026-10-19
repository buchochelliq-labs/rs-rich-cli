//! A bar spectrum, as cava draws one: a level per band as a column rising
//! from the bottom, optionally mirrored about the middle.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::plot_canvas::spec_style;
use super::{lines_to_segments, Charset, Line};

/// A column's top, in eighths of a cell, lowest first.
const EIGHTHS: [char; 8] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇'];
/// Rows a bar spectrum takes when it is given no height and
/// [`BarSpectrum::height`] is not set.
const DEFAULT_HEIGHT: usize = 8;
/// The tallest bar spectrum drawn, in rows.
const MAX_HEIGHT: usize = u16::MAX as usize;

/// Levels as columns rising from the bottom, as cava and a graphic
/// equaliser show a spectrum: each band (from
/// [`dsp::bands`](crate::dsp::bands), lowest first, falling under
/// [`dsp::Falloff`](crate::dsp::Falloff) for cava's look) a bar
/// [`bar_width`](Self::bar_width) cells wide, [`gap`](Self::gap) cells
/// apart, its top in eighths of a cell.
///
/// - [`mirror`](Self::mirror) draws the bands twice, the low ones in the
///   middle and the high ones at both edges (cava's mirrored layout);
///   [`stereo`](Self::stereo) puts one channel on each side, the left
///   reversed.
/// - Levels are scaled from 0 to the loudest band (or
///   [`range`](Self::range)); a level outside is drawn at the end it
///   passes, a NaN as 0.
/// - The bars are centred in the width they are given. When they do not
///   fit, the gaps go first, then the bars narrow to a cell, then
///   neighbouring bands share a column (the louder shown).
/// - It fills the height it is given (a `Layout` region, an intuiTUIve
///   node) unless [`height`](Self::height) is set; given none, it is 8
///   rows. In ASCII, each bar is `#`s to the nearest whole cell.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::BarSpectrum;
///
/// let console = Console::builder().width(7).color_system(None).build();
/// let bars = BarSpectrum::new([1.0, 0.5, 0.25]).range(1.0).height(2);
/// assert_eq!(console.render_export(&bars), " █     \n █ █ ▄ \n");
/// // Mirrored: the low bands in the middle.
/// let console = Console::builder().width(11).color_system(None).build();
/// let mirrored = bars.mirror(true);
/// assert_eq!(console.render_export(&mirrored), "    █ █    \n▄ █ █ █ █ ▄\n");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct BarSpectrum {
    left: Vec<f64>,
    right: Option<Vec<f64>>,
    mirror: bool,
    max: Option<f64>,
    bar_width: usize,
    gap: usize,
    style: Option<String>,
    charset: Charset,
    height: Option<usize>,
}

impl BarSpectrum {
    /// A bar per band of `bands`, lowest frequency first.
    pub fn new(bands: impl IntoIterator<Item = f64>) -> Self {
        BarSpectrum {
            left: bands.into_iter().collect(),
            right: None,
            mirror: false,
            max: None,
            bar_width: 1,
            gap: 1,
            style: None,
            charset: Charset::Auto,
            height: None,
        }
    }

    /// Two channels, mirrored: `left`'s bands from the middle out to the
    /// left edge, `right`'s from the middle to the right edge.
    pub fn stereo(
        left: impl IntoIterator<Item = f64>,
        right: impl IntoIterator<Item = f64>,
    ) -> Self {
        let mut bars = BarSpectrum::new(left);
        bars.right = Some(right.into_iter().collect());
        bars.mirror = true;
        bars
    }

    /// Draw the bands twice, the low ones in the middle (default false).
    /// A [`stereo`](Self::stereo) spectrum is always mirrored.
    pub fn mirror(mut self, mirror: bool) -> Self {
        self.mirror = mirror || self.right.is_some();
        self
    }

    /// The level of a full bar (default: the loudest band). Bars start at
    /// 0.
    pub fn range(mut self, max: f64) -> Self {
        self.max = (max.is_finite() && max > 0.0).then_some(max);
        self
    }

    /// Cells a bar is wide (default 1, at least 1).
    pub fn bar_width(mut self, cells: usize) -> Self {
        self.bar_width = cells.max(1);
        self
    }

    /// Cells between bars (default 1).
    pub fn gap(mut self, cells: usize) -> Self {
        self.gap = cells;
        self
    }

    /// The bars' style: a theme key or a definition (default
    /// `chart.bar`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
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

    /// The bands, as given: the left channel's (or the only one), and the
    /// right channel's for a [`stereo`](Self::stereo) spectrum.
    pub fn bands(&self) -> (&[f64], Option<&[f64]>) {
        (&self.left, self.right.as_deref())
    }

    /// The levels in the order they are drawn, left to right.
    fn bars(&self) -> Vec<f64> {
        let clean = |v: &f64| if v.is_nan() { 0.0 } else { *v };
        let mut bars: Vec<f64> = Vec::new();
        if self.mirror {
            bars.extend(self.left.iter().rev().map(clean));
        }
        bars.extend(self.right.as_ref().unwrap_or(&self.left).iter().map(clean));
        bars
    }

    /// The level of a full bar.
    fn top(&self, bars: &[f64]) -> f64 {
        self.max.unwrap_or_else(|| {
            let loudest = bars
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .fold(0.0, f64::max);
            if loudest > 0.0 {
                loudest
            } else {
                1.0
            }
        })
    }

    fn lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let width = options.max_width;
        let height = self
            .height
            .or(options.height)
            .unwrap_or(DEFAULT_HEIGHT)
            .min(MAX_HEIGHT);
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let style: Option<Style> =
            spec_style(console, self.style.as_deref().unwrap_or("chart.bar"));
        let mut bars = self.bars();
        let top = self.top(&bars);
        // Fit: the gaps, then the bars' width, then share columns.
        let (mut bar_width, mut gap) = (self.bar_width, self.gap);
        let span = |n: usize, w: usize, g: usize| n * w + n.saturating_sub(1) * g;
        if span(bars.len(), bar_width, gap) > width {
            gap = 0;
        }
        if span(bars.len(), bar_width, gap) > width {
            bar_width = (width / bars.len().max(1)).max(1);
        }
        if bars.len() > width && width > 0 {
            let n = bars.len();
            bars = (0..width)
                .map(|c| {
                    let (from, to) = (c * n / width, ((c + 1) * n / width).max(c * n / width + 1));
                    bars[from..to.min(n)].iter().copied().fold(0.0, f64::max)
                })
                .collect();
        }
        let used = span(bars.len(), bar_width, gap).min(width);
        let left = (width - used) / 2;
        // Each bar's height in eighths of a cell (whole cells in ASCII).
        let eighths: Vec<usize> = bars
            .iter()
            .map(|v| {
                let share = (v / top).clamp(0.0, 1.0);
                if ascii {
                    (share * height as f64).round() as usize * 8
                } else {
                    (share * (height * 8) as f64).round() as usize
                }
            })
            .collect();
        (0..height)
            .map(|r| {
                let above = (height - 1 - r) * 8;
                let mut line = Line::new();
                line.pad(left);
                for (i, &e) in eighths.iter().enumerate() {
                    if i > 0 {
                        line.pad(gap);
                    }
                    let glyph = match e.saturating_sub(above) {
                        0 => ' ',
                        n if n >= 8 => {
                            if ascii {
                                '#'
                            } else {
                                '█'
                            }
                        }
                        n => EIGHTHS[n],
                    };
                    if glyph == ' ' {
                        line.pad(bar_width);
                    } else {
                        line.push(&glyph.to_string().repeat(bar_width), style.clone());
                    }
                }
                line.pad(width.saturating_sub(line.width()));
                line
            })
            .collect()
    }
}

impl Renderable for BarSpectrum {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.lines(console, options), options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let n = self.bars().len().max(1);
        let natural = n * self.bar_width + (n - 1) * self.gap;
        Measurement::new(n.min(natural), natural)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Bar spectrum, 16 bands, mirrored, levels from 0 to 0.8`, then the
/// loudest band (`Loudest: band 3 of 16, 0.8`); a stereo one says each
/// channel's.
impl crate::a11y::AccessibleText for BarSpectrum {
    fn accessible_text(&self, _width: usize) -> String {
        use super::describe::{counted, extent, range};
        use super::ValueFormat;
        if self.left.is_empty() && self.right.as_ref().is_none_or(Vec::is_empty) {
            return "Bar spectrum, no bands".to_string();
        }
        let mut head = format!(
            "Bar spectrum, {}",
            counted(self.left.len(), "band", "bands")
        );
        if self.right.is_some() {
            head.push_str(", stereo");
        } else if self.mirror {
            head.push_str(", mirrored");
        }
        let all = self.left.iter().chain(self.right.iter().flatten()).copied();
        if let Some((lo, hi)) = extent(all) {
            head.push_str(&format!(", levels {}", range(lo, hi, ValueFormat::Compact)));
        }
        let mut out = vec![head];
        let loudest = |what: &str, bands: &[f64]| {
            let (at, level) = bands
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_finite())
                .max_by(|a, b| a.1.total_cmp(b.1))?;
            Some(format!(
                "{what}: band {} of {}, {}",
                at + 1,
                bands.len(),
                ValueFormat::Compact.format(*level)
            ))
        };
        match &self.right {
            None => out.extend(loudest("Loudest", &self.left)),
            Some(right) => {
                out.extend(loudest("Left loudest", &self.left));
                out.extend(loudest("Right loudest", right));
            }
        }
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::AccessibleText;

    fn plain(width: usize, bars: &BarSpectrum) -> Vec<String> {
        let console = Console::builder().width(width).color_system(None).build();
        console
            .render_export(bars)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn tops_are_eighths_of_a_cell() {
        let bars = BarSpectrum::new([1.0, 0.5, 1.0 / 16.0])
            .range(1.0)
            .gap(0)
            .height(1);
        assert_eq!(plain(3, &bars), ["█▄▁"]);
    }

    #[test]
    fn stereo_puts_a_channel_each_side() {
        let bars = BarSpectrum::stereo([1.0, 0.0], [0.0, 1.0])
            .range(1.0)
            .gap(0)
            .height(1);
        // Left reversed (high, low), then right (low, high).
        assert_eq!(plain(4, &bars), [" █ █"]);
        // A stereo spectrum stays mirrored.
        assert_eq!(plain(4, &bars.mirror(false)), [" █ █"]);
    }

    #[test]
    fn too_many_bands_lose_gaps_then_share_columns() {
        let bars = BarSpectrum::new([1.0, 0.0, 0.0, 1.0])
            .range(1.0)
            .bar_width(3)
            .height(1);
        // 4 × 3 + 3 gaps is 15: without gaps 12 fits.
        assert_eq!(plain(12, &bars), ["███      ███"]);
        // Six wide: bars narrow to a cell and are centred.
        assert_eq!(plain(6, &bars), [" █  █ "]);
        // Two wide: two bands a column, the louder shown.
        assert_eq!(plain(2, &bars), ["██"]);
    }

    #[test]
    fn levels_are_clamped_and_nan_is_silence() {
        let bars = BarSpectrum::new([2.0, -1.0, f64::NAN]).range(1.0).height(1);
        assert_eq!(plain(5, &bars), ["█    "]);
        // Without a range, the loudest band is full; all silent is empty.
        assert_eq!(plain(3, &BarSpectrum::new([0.0, 0.0]).height(1)), ["   "]);
        assert_eq!(plain(3, &BarSpectrum::new([2.0, 1.0]).height(1)), ["█ ▄"]);
    }

    #[test]
    fn ascii_bars_round_to_whole_cells() {
        let bars = BarSpectrum::new([1.0, 0.4])
            .range(1.0)
            .charset(Charset::Ascii)
            .height(2);
        assert_eq!(plain(3, &bars), ["#  ", "# #"]);
    }

    #[test]
    fn accessible_text_names_the_loudest_band() {
        let bars = BarSpectrum::new([0.2, 0.8, 0.1]).mirror(true);
        assert_eq!(
            bars.accessible_text(80),
            "Bar spectrum, 3 bands, mirrored, levels from 0.1 to 0.8\n\
             Loudest: band 2 of 3, 0.8"
        );
        let stereo = BarSpectrum::stereo([1.0], [2.0]);
        assert_eq!(
            stereo.accessible_text(80),
            "Bar spectrum, 1 band, stereo, levels from 1 to 2\n\
             Left loudest: band 1 of 1, 1\nRight loudest: band 1 of 1, 2"
        );
        assert_eq!(
            BarSpectrum::new([]).accessible_text(80),
            "Bar spectrum, no bands"
        );
    }
}
