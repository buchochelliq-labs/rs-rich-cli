//! A level meter, as a mixing desk's or a DAW's: a bar for the level, a
//! mark for the peak and one for the peak held, green to red along the
//! bar; and [`PeakHold`], which keeps a peak for a while.

use std::time::{Duration, Instant};

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{cells, lines_to_segments, theme_style, truncate, Charset, Line, Orientation, Scale};

/// Eighths of a cell, across.
const ACROSS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
/// Eighths of a cell, up.
const UP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Rows a vertical meter takes when it is given no height.
const DEFAULT_HEIGHT: usize = 8;
/// Cells a horizontal meter's bar takes when nothing else says.
const DEFAULT_BAR: usize = 20;

/// How a [`LevelMeter`] maps a level onto its bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeterScale {
    /// Evenly from the range's low end to its high end (default 0 to 1).
    Linear,
    /// Levels are amplitudes (1 is full scale), drawn by their decibels,
    /// `20 log10(level)`, from `floor` dB (an empty bar) to 0 dB (a full
    /// one), as an audio meter is: each 6 dB a halving.
    Decibels {
        /// The decibels at the bar's start, such as -60.
        floor: f64,
    },
}

/// A level against a scale, with its peak and the peak held: a VU or
/// peak-programme meter, as every DAW draws one per channel.
///
/// - **Across** (the default) it is a label, a bar and a readout on one
///   line. The bar fills to the level in eighths of a cell, and an unfilled
///   track (`─`) runs on to its end. **Up**
///   ([`orientation`](Self::orientation)`(Orientation::Vertical)`) it is a
///   column that fills from the bottom, as tall as it is given (8 rows
///   without a height) and as wide.
/// - The bar's cells take `chart.ok` (green) up to
///   [`bands`](Self::bands)' first share of its length, `chart.warning`
///   (yellow) to the second and `chart.critical` (red) past it: 70% and
///   90% by default. Colour is not the only cue: the length is the level.
/// - [`peak`](Self::peak) puts a mark (`│`, `─` up, `|` in ASCII) where the
///   peak is, and [`held`](Self::held) another where a [`PeakHold`] holds
///   it, each in the band's style there.
/// - [`readout`](Self::readout) writes the highest of the level, peak and
///   held peak after the bar, at a fixed width so the bar does not jump:
///   `0.42`, or `-12.3 dB` on a [`MeterScale::Decibels`] scale.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, LevelMeter};
///
/// let console = Console::builder().width(16).color_system(None).build();
/// let meter = LevelMeter::new(0.5)
///     .peak(0.75)
///     .label("L")
///     .readout(true)
///     .charset(Charset::Ascii);
/// assert_eq!(console.render_export(&meter), "L #####-|-- 0.75\n");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct LevelMeter {
    level: f64,
    peak: Option<f64>,
    held: Option<f64>,
    min: f64,
    max: f64,
    scale: MeterScale,
    bands: (f64, f64),
    orientation: Orientation,
    label: String,
    readout: bool,
    charset: Charset,
    width: Option<usize>,
    height: Option<usize>,
}

impl LevelMeter {
    /// A meter showing `level`, from 0 to 1 by default.
    pub fn new(level: f64) -> Self {
        LevelMeter {
            level,
            peak: None,
            held: None,
            min: 0.0,
            max: 1.0,
            scale: MeterScale::Linear,
            bands: (0.7, 0.9),
            orientation: Orientation::Horizontal,
            label: String::new(),
            readout: false,
            charset: Charset::Auto,
            width: None,
            height: None,
        }
    }

    /// The peak since the last reading, marked on the bar.
    pub fn peak(mut self, peak: f64) -> Self {
        self.peak = Some(peak);
        self
    }

    /// The peak held (by a [`PeakHold`]), marked on the bar.
    pub fn held(mut self, held: f64) -> Self {
        self.held = Some(held);
        self
    }

    /// The levels at the bar's ends on a [`MeterScale::Linear`] scale
    /// (default 0 and 1).
    pub fn range(mut self, min: f64, max: f64) -> Self {
        let scale = Scale::new(min, max);
        (self.min, self.max) = (scale.min(), scale.max());
        self
    }

    /// Linear (the default) or in decibels.
    pub fn scale(mut self, scale: MeterScale) -> Self {
        self.scale = scale;
        self
    }

    /// The shares of the bar's length where yellow and red begin (default
    /// 0.7 and 0.9), each clamped to `0..=1`.
    pub fn bands(mut self, warning: f64, critical: f64) -> Self {
        let share = |v: f64| if v.is_nan() { 1.0 } else { v.clamp(0.0, 1.0) };
        self.bands = (share(warning), share(critical).max(share(warning)));
        self
    }

    /// Across (the default) or up.
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Text before the bar, across (`L`, `R`, a channel's name).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Whether to write the reading after the bar, across (default no).
    pub fn readout(mut self, readout: bool) -> Self {
        self.readout = readout;
        self
    }

    /// Which glyphs to draw with (default [`Charset::Auto`]: blocks, or
    /// ASCII on an ASCII-only console).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Width in cells, label and readout included (default: the width it
    /// is given, or a 20-cell bar when measured).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Height in rows (default: one across; up, the height it is given or
    /// 8).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = Some(rows);
        self
    }

    /// The level.
    pub fn level(&self) -> f64 {
        self.level
    }

    /// Where `value` falls along the bar, from 0 to 1; `None` for NaN.
    pub fn share(&self, value: f64) -> Option<f64> {
        if value.is_nan() {
            return None;
        }
        let share = match self.scale {
            MeterScale::Linear => Scale::new(self.min, self.max).normalize(value)?,
            MeterScale::Decibels { floor } => {
                let floor = if floor.is_finite() && floor < 0.0 {
                    floor
                } else {
                    -60.0
                };
                if value <= 0.0 {
                    0.0
                } else {
                    (20.0 * value.log10() - floor) / -floor
                }
            }
        };
        Some(share.clamp(0.0, 1.0))
    }

    /// The reading the readout writes: the highest of the level, peak and
    /// held peak.
    fn reading(&self) -> f64 {
        [Some(self.level), self.peak, self.held]
            .into_iter()
            .flatten()
            .filter(|v| !v.is_nan())
            .fold(f64::NAN, f64::max)
    }

    fn reading_text(&self) -> String {
        let value = self.reading();
        match self.scale {
            _ if value.is_nan() => "-".to_string(),
            MeterScale::Linear => format!("{value:.2}"),
            MeterScale::Decibels { .. } if value <= 0.0 => "-inf dB".to_string(),
            MeterScale::Decibels { .. } => format!("{:.1} dB", 20.0 * value.log10()),
        }
    }

    /// The readout's width: as wide as the widest reading it can show.
    fn readout_width(&self) -> usize {
        match self.scale {
            MeterScale::Linear => {
                let widest = |v: f64| format!("{v:.2}").len();
                widest(self.min)
                    .max(widest(self.max))
                    .max(cells(&self.reading_text()))
            }
            MeterScale::Decibels { .. } => 8.max(cells(&self.reading_text())),
        }
    }

    /// The style of the band a share of the bar falls in.
    fn band(&self, console: &Console, share: f64) -> Style {
        let key = if share >= self.bands.1 {
            "chart.critical"
        } else if share >= self.bands.0 {
            "chart.warning"
        } else {
            "chart.ok"
        };
        theme_style(console, key)
    }

    /// Where the peak and held marks go along `cells`: each a cell.
    fn marks(&self, cells: usize) -> Vec<usize> {
        let mut marks: Vec<usize> = [self.peak, self.held]
            .into_iter()
            .flatten()
            .filter_map(|v| self.share(v))
            .map(|s| ((s * cells as f64).ceil() as usize).clamp(1, cells.max(1)) - 1)
            .collect();
        marks.dedup();
        marks
    }

    fn across(&self, console: &Console, width: usize, ascii: bool) -> Vec<Line> {
        let mut line = Line::new();
        let label = truncate(&self.label, width, ascii);
        let mut room = width;
        if !label.is_empty() {
            line.push(&label, Some(theme_style(console, "chart.label")));
            line.pad(1);
            room = room.saturating_sub(cells(&label) + 1);
        }
        let readout = if self.readout {
            let w = self.readout_width();
            (w < room).then(|| (w, self.reading_text()))
        } else {
            None
        };
        let bar = room.saturating_sub(readout.as_ref().map_or(0, |(w, _)| w + 1));
        let eighths = self
            .share(self.level)
            .map_or(0, |s| (s * (bar * 8) as f64).round() as usize);
        let marks = self.marks(bar);
        let track = theme_style(console, "chart.track");
        for cell in 0..bar {
            let share = cell as f64 / bar as f64;
            let filled = eighths.saturating_sub(cell * 8).min(8);
            // ASCII has no part cells: half or more is a whole one.
            let filled = if ascii && filled < 4 { 0 } else { filled };
            if filled > 0 && !(filled < 8 && marks.contains(&cell)) {
                let glyph = if ascii { '#' } else { ACROSS[filled - 1] };
                line.push(&glyph.to_string(), Some(self.band(console, share)));
            } else if marks.contains(&cell) {
                let glyph = if ascii { "|" } else { "│" };
                let style = self
                    .band(console, share)
                    .combine(&Style::parse("bold").unwrap_or_default());
                line.push(glyph, Some(style));
            } else {
                line.push(if ascii { "-" } else { "─" }, Some(track.clone()));
            }
        }
        if let Some((w, text)) = readout {
            line.pad(1 + w - cells(&text));
            line.push(&text, Some(theme_style(console, "chart.value")));
        }
        vec![line]
    }

    fn up(&self, console: &Console, width: usize, height: usize, ascii: bool) -> Vec<Line> {
        let eighths = self
            .share(self.level)
            .map_or(0, |s| (s * (height * 8) as f64).round() as usize);
        let marks = self.marks(height);
        (0..height)
            .map(|r| {
                // Rows counted from the bottom.
                let row = height - 1 - r;
                let share = row as f64 / height as f64;
                let filled = eighths.saturating_sub(row * 8).min(8);
                let filled = if ascii && filled < 4 { 0 } else { filled };
                let (glyph, style) = if filled > 0 && !(filled < 8 && marks.contains(&row)) {
                    let glyph = if ascii {
                        "#".into()
                    } else {
                        UP[filled - 1].to_string()
                    };
                    (glyph, Some(self.band(console, share)))
                } else if marks.contains(&row) {
                    let glyph = if ascii { "-" } else { "─" };
                    (glyph.to_string(), Some(self.band(console, share)))
                } else {
                    (" ".to_string(), None)
                };
                let mut line = Line::new();
                line.push(&glyph.repeat(width), style);
                line
            })
            .collect()
    }
}

impl Renderable for LevelMeter {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = self
            .width
            .map_or(options.max_width, |w| w.min(options.max_width));
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let lines = match self.orientation {
            Orientation::Horizontal => {
                let line = self.across(console, width, ascii);
                let rows = self.height.unwrap_or(1).max(1);
                std::iter::repeat_n(line, rows).flatten().collect()
            }
            Orientation::Vertical => {
                let height = self.height.or(options.height).unwrap_or(DEFAULT_HEIGHT);
                self.up(console, width, height, ascii)
            }
        };
        lines_to_segments(lines, width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let natural = match (self.width, self.orientation) {
            (Some(width), _) => width,
            (None, Orientation::Vertical) => 1,
            (None, Orientation::Horizontal) => {
                let label = cells(&self.label);
                let readout = if self.readout {
                    self.readout_width() + 1
                } else {
                    0
                };
                label + usize::from(label > 0) + DEFAULT_BAR + readout
            }
        };
        Measurement::new(natural.min(1), natural)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Level meter L: 0.5, 50 percent, peak 0.75, band warning`: the level, its
/// place on the bar, the peaks and the band it is in.
impl crate::a11y::AccessibleText for LevelMeter {
    fn accessible_text(&self, _width: usize) -> String {
        let what = if self.label.is_empty() {
            "Level meter".to_string()
        } else {
            format!("Level meter {}", self.label)
        };
        let text = |v: f64| match self.scale {
            MeterScale::Linear => super::ValueFormat::Compact.format(v),
            MeterScale::Decibels { .. } if v <= 0.0 => "-inf dB".to_string(),
            MeterScale::Decibels { .. } => format!("{:.1} dB", 20.0 * v.log10()),
        };
        let Some(share) = self.share(self.level) else {
            return format!("{what}: no level");
        };
        let mut parts = vec![text(self.level), super::describe::percent(share)];
        if let Some(peak) = self.peak.filter(|p| !p.is_nan()) {
            parts.push(format!("peak {}", text(peak)));
        }
        if let Some(held) = self.held.filter(|p| !p.is_nan()) {
            parts.push(format!("held {}", text(held)));
        }
        let top = self.reading();
        let band = match self.share(top) {
            Some(s) if s >= self.bands.1 => "critical",
            Some(s) if s >= self.bands.0 => "warning",
            _ => "ok",
        };
        parts.push(format!("band {band}"));
        format!("{what}: {}", parts.join(", "))
    }
}

/// A peak kept for a while: [`update`](Self::update) it with each new
/// peak, and it returns the highest seen in the last
/// [`hold`](Self::new) (then it falls back to the current peak, at once or
/// at [`fall`](Self::fall)'s rate), as a meter's peak-hold mark.
///
/// ```
/// use std::time::{Duration, Instant};
/// use rich_ext::chart::PeakHold;
///
/// let start = Instant::now();
/// let at = |ms| start + Duration::from_millis(ms);
/// let mut hold = PeakHold::new(Duration::from_millis(500));
/// assert_eq!(hold.update(0.8, at(0)), 0.8);
/// assert_eq!(hold.update(0.3, at(400)), 0.8);
/// // Held for half a second, then back to the peak now.
/// assert_eq!(hold.update(0.3, at(600)), 0.3);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct PeakHold {
    hold: Duration,
    /// Level units a second it falls by once the hold is over; 0 drops.
    fall: f64,
    held: f64,
    since: Option<Instant>,
    shown: f64,
}

impl PeakHold {
    /// Hold each peak for `hold`.
    pub fn new(hold: Duration) -> Self {
        PeakHold {
            hold,
            fall: 0.0,
            held: f64::NAN,
            since: None,
            shown: f64::NAN,
        }
    }

    /// Once the hold is over, fall by `per_second` (level units a second)
    /// rather than drop to the current peak at once (default 0: drop).
    pub fn fall(mut self, per_second: f64) -> Self {
        self.fall = if per_second.is_finite() {
            per_second.max(0.0)
        } else {
            0.0
        };
        self
    }

    /// Take the peak at `now`; the peak shown. A NaN peak changes nothing.
    pub fn update(&mut self, peak: f64, now: Instant) -> f64 {
        if peak.is_nan() {
            return self.shown;
        }
        let since = match self.since {
            Some(since) if peak < self.held => since,
            _ => {
                (self.held, self.since, self.shown) = (peak, Some(now), peak);
                return peak;
            }
        };
        let age = now.saturating_duration_since(since);
        let shown = match age.checked_sub(self.hold) {
            None => self.held,
            Some(_) if self.fall == 0.0 => peak,
            Some(over) => (self.held - self.fall * over.as_secs_f64()).max(peak),
        };
        if shown <= peak {
            (self.held, self.since) = (peak, Some(now));
        }
        self.shown = shown;
        shown
    }

    /// The peak shown after the last update (NaN before the first).
    pub fn value(&self) -> f64 {
        self.shown
    }

    /// Forget the peak held.
    pub fn reset(&mut self) {
        (self.held, self.since, self.shown) = (f64::NAN, None, f64::NAN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a11y::AccessibleText;

    fn plain(width: usize, meter: &LevelMeter) -> String {
        let console = Console::builder().width(width).color_system(None).build();
        console.render_export(meter)
    }

    #[test]
    fn the_bar_fills_in_eighths() {
        let meter = LevelMeter::new(0.5).charset(Charset::Blocks);
        // 2.5 cells of 5.
        assert_eq!(plain(5, &meter), "██▌──\n");
        assert_eq!(plain(5, &LevelMeter::new(0.0)), "─────\n");
        assert_eq!(plain(5, &LevelMeter::new(2.0)), "█████\n");
    }

    #[test]
    fn peaks_are_marked_past_the_level() {
        let meter = LevelMeter::new(0.2).peak(0.6).held(1.0);
        assert_eq!(plain(5, &meter), "█─│─│\n");
        // A mark under a part-filled cell wins over it.
        let under = LevelMeter::new(0.5).peak(0.5);
        assert_eq!(plain(4, &under), "██──\n");
        assert_eq!(plain(5, &under), "██│──\n");
    }

    #[test]
    fn decibels_put_minus_six_near_the_middle_of_twelve() {
        let half = 10f64.powf(-6.0 / 20.0);
        let meter = LevelMeter::new(half)
            .scale(MeterScale::Decibels { floor: -12.0 })
            .readout(true)
            .charset(Charset::Ascii);
        assert_eq!(plain(13, &meter), "##--  -6.0 dB\n");
        let silent = LevelMeter::new(0.0)
            .scale(MeterScale::Decibels { floor: -60.0 })
            .readout(true);
        assert_eq!(plain(12, &silent), "───  -inf dB\n");
    }

    #[test]
    fn the_bands_colour_the_bar() {
        let console = Console::builder()
            .width(10)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Standard))
            .build();
        let out = console.render_export(&LevelMeter::new(1.0));
        // Seven green, two yellow and one red cell.
        assert!(out.contains("\u{1b}[32m███████"), "{out:?}");
        assert!(out.contains("\u{1b}[33m██"), "{out:?}");
        assert!(out.contains("\u{1b}[1;31m█"), "{out:?}");
    }

    #[test]
    fn up_it_fills_from_the_bottom() {
        let meter = LevelMeter::new(0.5)
            .peak(0.9)
            .orientation(Orientation::Vertical)
            .height(4);
        assert_eq!(plain(2, &meter), "──\n  \n██\n██\n");
    }

    #[test]
    fn it_measures_its_parts() {
        let console = Console::builder().width(80).build();
        let meter = LevelMeter::new(0.1).label("L").readout(true);
        let m = meter.measure(&console, &console.options());
        assert_eq!(m.maximum, 2 + 20 + 5);
    }

    #[test]
    fn accessible_text_reads_the_level() {
        let meter = LevelMeter::new(0.5).peak(0.95).label("L");
        assert_eq!(
            meter.accessible_text(80),
            "Level meter L: 0.5, 50 percent, peak 0.95, band critical"
        );
        assert_eq!(
            LevelMeter::new(f64::NAN).accessible_text(80),
            "Level meter: no level"
        );
    }

    #[test]
    fn a_held_peak_falls_at_its_rate() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut hold = PeakHold::new(Duration::from_millis(100)).fall(1.0);
        hold.update(1.0, at(0));
        // Half a second past the hold: down by a half.
        assert!((hold.update(0.0, at(600)) - 0.5).abs() < 1e-9);
        // Never below the peak now.
        assert_eq!(hold.update(0.9, at(700)), 0.9);
        // A higher peak takes over at once.
        assert_eq!(hold.update(0.95, at(710)), 0.95);
        assert!(hold.update(f64::NAN, at(720)) == 0.95);
        hold.reset();
        assert!(hold.value().is_nan());
    }
}
