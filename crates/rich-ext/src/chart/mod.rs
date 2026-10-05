//! Charts drawn with text: sparklines, bars, histograms, line and scatter
//! plots, and the KPI and status renderables a dashboard is made of
//! (gauges, heatmaps, status matrices, KPI cards and timelines).
//!
//! Every chart here is a [`Renderable`](rich::Renderable) that measures
//! itself, so it can sit in a `Table` cell, a `Panel` or a `Live` display.
//! None ever writes a line wider than the width it is given; at very small
//! widths it drops labels, values and axes before it drops data.
//!
//! | Type | Draws |
//! |---|---|
//! | [`Sparkline`] | one line, one cell per value (two in Braille) |
//! | [`BarChart`] | a label, a bar and a value per row, or columns side by side ([`Orientation::Vertical`]) |
//! | [`Histogram`] | raw values counted into bins, drawn as a [`BarChart`] |
//! | [`LineChart`] | one or more [`Series`] as lines or scattered points, with axes and a legend |
//! | [`Gauge`] | a value against a range, with a target and threshold [`Band`]s, on one line or full width |
//! | [`BulletChart`] | several gauges, one per line, their columns aligned |
//! | [`Heatmap`] | a labelled grid of values as shades, with a legend |
//! | [`StatusMatrix`] | rows and columns of [`State`]s (pass, fail, skip, flaky, your own), each a symbol and a colour |
//! | [`KpiCard`] | a label, a value, a delta with its direction, a sparkline and a [`Status`] |
//! | [`Timeline`] | labelled ranges on a numeric or seconds scale, overlaps stacked, milestones |
//!
//! What they share:
//!
//! - [`Scale`]: a linear scale with round ticks that skips NaN and infinite
//!   values and never has an empty range.
//! - [`ValueFormat`]: how values are written (`1.2k`, `3.4M`, or a fixed
//!   number of decimals).
//! - [`Charset`]: which glyphs to draw with. [`Charset::Auto`] picks the
//!   chart's natural set, and ASCII whenever the console's encoding is not
//!   UTF-8 or its [`Fidelity`](crate::fidelity::Fidelity) is
//!   [`Ascii`](crate::fidelity::Fidelity::Ascii). An ASCII console never gets
//!   anything else, whatever the chart asks for.
//!
//! Colour is never the only way to read a chart: heights and lengths carry
//! the values, numbers are written out, thresholds and extremes are named in
//! words, and series have their own markers. Styles come from the theme keys
//! in [`STYLES`] (chained into
//! [`extended_theme`](crate::theme::extended_theme)); the same values are the
//! fallbacks when a theme lacks them.
//!
//! ```
//! use rich::Console;
//! use rich_ext::chart::{BarChart, Sparkline};
//!
//! let console = Console::builder().width(40).color_system(None).build();
//! let spark = Sparkline::new([1.0, 3.0, 2.0, 8.0, 5.0]);
//! assert_eq!(console.render_export(&spark), "▁▃▂█▅\n");
//!
//! let bars = BarChart::new().bar("api", 30.0).bar("web", 12.0).bar_width(10);
//! assert_eq!(
//!     console.render_export(&bars),
//!     "api ██████████ 30\nweb ████       12\n"
//! );
//! ```

mod axis;
mod bar;
mod canvas;
mod gauge;
mod heatmap;
mod kpi;
mod line;
mod scale;
mod sparkline;
mod status;
mod timeline;

pub use bar::{Bar, BarChart, Histogram, Orientation};
pub use canvas::DotCanvas;
pub use gauge::{Band, BulletChart, Gauge};
pub use heatmap::Heatmap;
pub use kpi::KpiCard;
pub use line::{LineChart, Series, SeriesKind};
pub use scale::{Scale, ValueFormat};
pub use sparkline::Sparkline;
pub use status::{State, Status, StatusMatrix};
pub use timeline::{Milestone, Span, Timeline};

use rich::cells::char_cell_width;
use rich::{Console, ConsoleOptions, Segment, Style};

use crate::fidelity::{Fidelity, Policy};

/// Theme keys for charts, chained into
/// [`extended_theme`](crate::theme::extended_theme).
pub const STYLES: &[(&str, &str)] = &[
    ("chart.axis", "bright_black"),
    ("chart.label", "none"),
    ("chart.value", "none"),
    ("chart.bar", "cyan"),
    ("chart.negative", "magenta"),
    ("chart.spark", "cyan"),
    ("chart.over", "bold red"),
    ("chart.min", "blue"),
    ("chart.max", "bold green"),
    ("chart.series.1", "cyan"),
    ("chart.series.2", "magenta"),
    ("chart.series.3", "yellow"),
    ("chart.series.4", "green"),
    ("chart.series.5", "blue"),
    ("chart.track", "bright_black"),
    ("chart.target", "bold"),
    ("chart.ok", "green"),
    ("chart.warning", "yellow"),
    ("chart.critical", "bold red"),
    ("chart.unknown", "bright_black"),
    ("chart.delta.good", "green"),
    ("chart.delta.bad", "red"),
    ("chart.delta.flat", "bright_black"),
    ("chart.kpi.label", "none"),
    ("chart.kpi.value", "bold"),
    ("chart.kpi.border", "bright_black"),
    ("chart.heat.1", "blue"),
    ("chart.heat.2", "cyan"),
    ("chart.heat.3", "green"),
    ("chart.heat.4", "yellow"),
    ("chart.heat.5", "red"),
    ("chart.state.pass", "green"),
    ("chart.state.fail", "bold red"),
    ("chart.state.skip", "bright_black"),
    ("chart.state.flaky", "yellow"),
    ("chart.state.unknown", "magenta"),
    ("chart.milestone", "bold yellow"),
];

/// How many `chart.series.N` styles there are; series past it cycle.
const SERIES_STYLES: usize = 5;

/// Which glyphs a chart draws with.
///
/// Whatever is asked for, an ASCII-only console (its encoding is not UTF-8,
/// or [`Fidelity`] is `Ascii`) gets [`Ascii`](Self::Ascii).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Charset {
    /// The chart's natural set: blocks for sparklines and bars, Braille for
    /// line and scatter charts; ASCII on an ASCII-only console.
    #[default]
    Auto,
    /// Block elements: `▁▂▃▄▅▆▇█` heights, `▏▎▍▌▋▊▉█` eighths, and marker
    /// glyphs (`●◆▲■○`) at cell resolution for line charts.
    Blocks,
    /// Braille dots: 2×4 dots per cell, so twice the values per cell in a
    /// sparkline, half cells in a bar and 2×4 points per cell in a plot.
    Braille,
    /// Printable ASCII only: the ramp `_.-:=+*#`, `#` and `=` bars, and
    /// `*`, `+`, `o`, `x`, `.` markers.
    Ascii,
}

impl Charset {
    /// The set to draw with on `console`: [`Ascii`](Self::Ascii) when the
    /// console or the options are ASCII-only, `self` otherwise, with
    /// [`Auto`](Self::Auto) replaced by `natural`.
    pub fn resolve(self, console: &Console, options: &ConsoleOptions, natural: Charset) -> Charset {
        let ascii = options.ascii_only()
            || console.ascii_only()
            || Fidelity::for_console(console, &Policy::default()) == Fidelity::Ascii;
        match self {
            _ if ascii => Charset::Ascii,
            Charset::Auto => natural,
            set => set,
        }
    }
}

/// Whether `console` shows colour: [`Fidelity`] `Rich` or above. Below
/// it, charts that would tell things apart by colour alone switch to shapes.
pub(crate) fn has_colour(console: &Console) -> bool {
    Fidelity::for_console(console, &Policy::default()) >= Fidelity::Rich
}

/// A theme style by key, falling back to [`STYLES`].
pub(crate) fn theme_style(console: &Console, key: &str) -> Style {
    if let Some(style) = console.theme().get(key) {
        return style.clone();
    }
    STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, spec)| Style::parse(spec).ok())
        .unwrap_or_default()
}

/// A style that is either a theme key or a definition (`"bold red"`).
pub(crate) fn user_style(console: &Console, spec: &str) -> Style {
    console
        .theme()
        .get(spec)
        .cloned()
        .or_else(|| Style::parse(spec).ok())
        .unwrap_or_default()
}

/// The theme key for series `index` (from 0).
pub(crate) fn series_key(index: usize) -> String {
    format!("chart.series.{}", index % SERIES_STYLES + 1)
}

/// One line of a chart: styled runs, cropped to a width in cells.
#[derive(Clone, Debug, Default)]
pub(crate) struct Line {
    spans: Vec<(String, Option<Style>)>,
    width: usize,
}

impl Line {
    pub(crate) fn new() -> Self {
        Line::default()
    }

    /// Cells used so far.
    pub(crate) fn width(&self) -> usize {
        self.width
    }

    /// Append `text`. Runs with the same style are merged.
    pub(crate) fn push(&mut self, text: &str, style: Option<Style>) {
        if text.is_empty() {
            return;
        }
        self.width += text.chars().map(char_cell_width).sum::<usize>();
        if let Some((last, last_style)) = self.spans.last_mut() {
            if *last_style == style {
                last.push_str(text);
                return;
            }
        }
        self.spans.push((text.to_string(), style));
    }

    /// Append the runs of `other`.
    pub(crate) fn append(&mut self, other: Line) {
        for (text, style) in other.spans {
            self.push(&text, style);
        }
    }

    /// Append `n` spaces.
    pub(crate) fn pad(&mut self, n: usize) {
        if n > 0 {
            self.push(&" ".repeat(n), None);
        }
    }

    /// Cut to at most `width` cells; a wide character that would straddle
    /// the edge becomes a space.
    pub(crate) fn crop(&mut self, width: usize) {
        if self.width <= width {
            return;
        }
        let mut used = 0;
        let mut kept = Vec::new();
        for (text, style) in self.spans.drain(..) {
            if used >= width {
                break;
            }
            let mut out = String::new();
            for c in text.chars() {
                let w = char_cell_width(c);
                if used + w > width {
                    if used < width {
                        out.push(' ');
                        used += 1;
                    }
                    break;
                }
                out.push(c);
                used += w;
            }
            if !out.is_empty() {
                kept.push((out, style));
            }
        }
        self.spans = kept;
        self.width = used;
    }

    fn into_segments(self, out: &mut Vec<Segment>) {
        for (text, style) in self.spans {
            out.push(Segment::new(text, style));
        }
    }
}

/// `lines` as segments, each cropped to `width` and padded to the widest
/// line so the block is square, with a newline between them. Like core's
/// renderables, the last line has none: `print` ends it.
pub(crate) fn lines_to_segments(lines: Vec<Line>, width: usize) -> Vec<Segment> {
    let block = lines
        .iter()
        .map(|l| l.width().min(width))
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();
    for (index, mut line) in lines.into_iter().enumerate() {
        if index > 0 {
            out.push(Segment::line());
        }
        line.crop(width);
        let fill = block - line.width();
        line.pad(fill);
        line.into_segments(&mut out);
    }
    out
}

/// `text` as terminal cells, one entry per cell: a character with the
/// zero-width marks after it, and an empty entry for the second cell of a
/// wide character. Its length is `cells(text)`, so a label placed with it
/// lands on cell offsets, not character indices. Zero-width characters with
/// nothing before them are dropped.
pub(crate) fn cell_units(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut wide = false;
    for c in text.chars() {
        match char_cell_width(c) {
            0 => {
                // On the character's own cell, past a wide one's spacer.
                let at = out.len().checked_sub(if wide { 2 } else { 1 });
                if let Some(cell) = at.and_then(|i| out.get_mut(i)) {
                    cell.push(c);
                }
            }
            w => {
                out.push(c.to_string());
                wide = w > 1;
                if wide {
                    out.push(String::new());
                }
            }
        }
    }
    out
}

/// `text` cut to `width` cells, ending in `…` (`.` in ASCII) when cut. In
/// ASCII, characters past U+007F first go through
/// [`ascii_text`](crate::fidelity::ascii_text).
pub(crate) fn truncate(text: &str, width: usize, ascii: bool) -> String {
    let owned;
    let text = if ascii && !text.is_ascii() {
        owned = crate::fidelity::ascii_text(text);
        owned.as_str()
    } else {
        text
    };
    let len: usize = text.chars().map(char_cell_width).sum();
    if len <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = char_cell_width(c);
        if used + w > width - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push_str(&" ".repeat(width - 1 - used));
    out.push(if ascii { '.' } else { '…' });
    out
}

/// Legend entries, each a list of styled runs, laid out two cells apart
/// and wrapped to `width`. An entry wider than `width` is left to be cropped.
pub(crate) fn wrap_entries(entries: Vec<Vec<(String, Option<Style>)>>, width: usize) -> Vec<Line> {
    let mut out = Vec::new();
    let mut line = Line::new();
    for entry in entries {
        let entry_w: usize = entry.iter().map(|(t, _)| cells(t)).sum();
        if line.width() > 0 && line.width() + 2 + entry_w > width {
            out.push(std::mem::take(&mut line));
        }
        if line.width() > 0 {
            line.pad(2);
        }
        for (text, style) in entry {
            line.push(&text, style);
        }
    }
    if line.width() > 0 {
        out.push(line);
    }
    out
}

/// The width of legend `entries` on one line.
pub(crate) fn entries_width(entries: &[Vec<(String, Option<Style>)>]) -> usize {
    let total: usize = entries
        .iter()
        .map(|e| e.iter().map(|(t, _)| cells(t)).sum::<usize>())
        .sum();
    total + 2 * entries.len().saturating_sub(1)
}

/// `text` centred in `width` cells (cut first when longer).
pub(crate) fn centre(line: &mut Line, text: &str, width: usize, style: Option<Style>, ascii: bool) {
    let text = truncate(text, width, ascii);
    let pad = width - cells(&text);
    line.pad(pad / 2);
    line.push(&text, style);
    line.pad(pad - pad / 2);
}

/// The width of `text` in cells.
pub(crate) fn cells(text: &str) -> usize {
    rich::cells::cell_len(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_counts_cells_not_bytes() {
        let mut line = Line::new();
        line.push("ab", None);
        line.push("日本", Some(Style::parse("red").unwrap()));
        assert_eq!(line.width(), 6);
        line.crop(5);
        assert_eq!(line.width(), 5);
        let mut segs = Vec::new();
        line.into_segments(&mut segs);
        let text: String = segs.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "ab日 ");
    }

    #[test]
    fn truncation_marks_the_cut() {
        assert_eq!(truncate("latency", 4, false), "lat…");
        assert_eq!(truncate("latency", 4, true), "lat.");
        assert_eq!(truncate("latency", 1, true), ".");
        assert_eq!(truncate("latency", 0, true), "");
        assert_eq!(truncate("ok", 4, true), "ok");
        assert_eq!(truncate("日本語", 4, false), "日 …");
        assert_eq!(truncate("café", 4, true), "caf?");
    }
}
