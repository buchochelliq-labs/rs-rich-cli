//! The `viz` plugin: levels and spectra as nodes, each drawn by one of
//! rs-rich-ext's charts from a closure that reads signals. A level meter
//! with peak hold, a sparkline, a spectrogram (a waterfall), a bar
//! spectrum (cava's bars), a trace with a phosphor's persistence (an
//! analogue scope's) and a tuner's readout. There only with the off-by-default `viz`
//! feature: they are views of one domain's data (levels and spectra), not
//! components other components build on, so they are not in the core.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rich::{Segment, Style};
use rich_ext::chart::{
    Axis as ChartAxis, BarSpectrum, Chart, Layering, LevelMeter, PeakHold, Persistence, Sparkline,
    Spectrogram,
};
use rich_ext::dsp::Reading;

use crate::digits::digits;
use crate::node::{column, described, leaf, text, Axis, Node};
use crate::screen::Rect;
use crate::widget::{widget, Canvas, DrawCx, MeasureCx, Widget};

/// A level meter: rs-rich-ext's [`LevelMeter`] showing `level` with a mark
/// at `peak`, both closures that may read signals, as a VU or peak meter
/// per channel beside a scope. Across, one row: a bar green to red, the
/// peak's mark, and with [`hold`](Node::hold) a second mark that keeps the
/// highest peak for a while. Its accessible name says the level, the peaks
/// and the band (`Level meter L: 0.5, 50 percent, …`).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::meter;
///
/// let app = App::new(|| {
///     let level = signal(0.5);
///     meter(move || level.get(), move || level.get() + 0.25)
///         .hold(std::time::Duration::from_secs(1))
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 8, 1).unwrap();
/// assert_eq!(screen[0], "████─│──");
/// ```
pub fn meter(level: impl Fn() -> f64 + 'static, peak: impl Fn() -> f64 + 'static) -> Node {
    meter_with(level, peak, |m| m)
}

/// A [`meter`] whose [`LevelMeter`] `configure` sets up (`.label("L")`,
/// `.readout(true)`, `.scale(MeterScale::Decibels { floor: -60.0 })`,
/// `.orientation(Orientation::Vertical)`): it is given one showing the
/// level and peak, and returns it changed.
pub fn meter_with(
    level: impl Fn() -> f64 + 'static,
    peak: impl Fn() -> f64 + 'static,
    configure: impl Fn(LevelMeter) -> LevelMeter + 'static,
) -> Node {
    let hold: Rc<RefCell<Option<PeakHold>>> = Rc::new(RefCell::new(None));
    let held = hold.clone();
    let child = described(move || {
        let (level, peak) = (level(), peak());
        let mut meter = LevelMeter::new(level).peak(peak);
        if let Some(hold) = held.borrow_mut().as_mut() {
            meter = meter.held(hold.update(peak, Instant::now()));
        }
        configure(meter)
    });
    widget(Meter { child, hold }).what("meter")
}

/// The node [`meter`] makes: the drawing, and its peak hold.
struct Meter {
    child: Node,
    hold: Rc<RefCell<Option<PeakHold>>>,
}

impl Widget for Meter {
    fn name(&self) -> &'static str {
        "meter"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        cx.measure(&self.child, axis, width, height)
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![rect]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}
}

/// A [`meter`]'s options. On any other node they do nothing.
impl Node {
    /// Keep each peak marked for `hold` after it (rs-rich-ext's
    /// [`PeakHold`]), as a DAW's meter does: the mark stays at the highest
    /// peak until the hold is over, then drops to the peak then. The time
    /// is read when the meter draws, which is whenever its level or peak
    /// changes.
    pub fn hold(self, hold: Duration) -> Node {
        self.widget_mut(|meter: &mut Meter| *meter.hold.borrow_mut() = Some(PeakHold::new(hold)));
        self
    }
}

/// A sparkline of `values`, a closure that may read signals: rs-rich-ext's
/// [`Sparkline`] (ratatui's and Textual's), one cell a value, as tall as
/// the node. Given more values than cells it resamples them, so keep as
/// many as should show (a history the width of the node, the newest last).
/// Its accessible name says the values' range and trend.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::sparkline;
///
/// let app = App::new(|| {
///     let history = signal(vec![1.0, 3.0, 2.0, 8.0]);
///     sparkline(move || history.get()).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 4, 1).unwrap();
/// assert_eq!(screen[0], "▁▃▂█");
/// ```
pub fn sparkline(values: impl Fn() -> Vec<f64> + 'static) -> Node {
    sparkline_with(values, |s| s)
}

/// A [`sparkline`] whose [`Sparkline`] `configure` sets up (`.range(0.0,
/// 1.0)`, `.style("green")`, `.min_max(true)`).
pub fn sparkline_with(
    values: impl Fn() -> Vec<f64> + 'static,
    configure: impl Fn(Sparkline) -> Sparkline + 'static,
) -> Node {
    described(move || configure(Sparkline::new(values())))
}

/// A spectrogram (a waterfall) of `spectra`, a closure that may read
/// signals and returns the spectra to show, oldest first, each the levels
/// of its bins, lowest frequency first: rs-rich-ext's [`Spectrogram`], the
/// newest at the top, as tall as the node. Keep the history with the
/// core's [`recent`](crate::recent) (`recent(spectrum, rows)` keeps the
/// newest `rows`, oldest first). Its accessible name says the spectra's
/// range and where the newest peaks.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{recent, spectrogram_with};
/// use rich_ext::chart::Charset;
///
/// let app = App::new(|| {
///     let spectrum = signal(Vec::new());
///     // The last two spectra written.
///     let rows = recent(spectrum, 2);
///     spectrogram_with(
///         move || rows.get(),
///         |s| s.charset(Charset::Ascii).range(0.0, 9.0),
///     )
///     .on_key("l", move |_| spectrum.set(vec![9.0, 0.0, 0.0]))
///     .on_key("h", move |_| spectrum.set(vec![0.0, 0.0, 9.0]))
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["l", "h", "q"], 3, 2).unwrap();
/// // The newest at the top.
/// assert_eq!(screen, ["  @", "@  "]);
/// ```
pub fn spectrogram<I>(spectra: impl Fn() -> I + 'static) -> Node
where
    I: IntoIterator<Item = Vec<f64>>,
{
    spectrogram_with(spectra, |s| s)
}

/// A [`spectrogram`] whose [`Spectrogram`] `configure` sets up
/// (`.resolution(rate / n)`, `.scale(AxisScale::Log)`, `.bounds(20.0,
/// 20_000.0)`, `.range(-60.0, 0.0)`, `.charset(..)`): it is given one
/// holding the spectra, and returns it changed.
pub fn spectrogram_with<I>(
    spectra: impl Fn() -> I + 'static,
    configure: impl Fn(Spectrogram) -> Spectrogram + 'static,
) -> Node
where
    I: IntoIterator<Item = Vec<f64>>,
{
    described(move || {
        let rows: Vec<Vec<f64>> = spectra().into_iter().collect();
        let mut waterfall = Spectrogram::new(rows.len());
        for row in rows {
            waterfall.push(row);
        }
        configure(waterfall)
    })
}

/// A bar spectrum of `bands`, a closure that may read signals: rs-rich-ext's
/// [`BarSpectrum`], a column per band rising from the bottom, as tall as
/// the node, as cava draws. Feed it rs-rich-ext's `dsp::bands` of a
/// spectrum, through a `dsp::Falloff` for cava's falling bars, or the
/// core's [`smoothed`](crate::smoothed) for each. Its accessible name says
/// the bands' range and the loudest.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::bar_spectrum_with;
///
/// let app = App::new(|| {
///     let bands = signal(vec![1.0, 0.5]);
///     bar_spectrum_with(move || bands.get(), |b| b.range(1.0).mirror(true))
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 7, 1).unwrap();
/// assert_eq!(screen[0], "▄ █ █ ▄");
/// ```
pub fn bar_spectrum(bands: impl Fn() -> Vec<f64> + 'static) -> Node {
    bar_spectrum_with(bands, |b| b)
}

/// A [`bar_spectrum`] whose [`BarSpectrum`] `configure` sets up
/// (`.mirror(true)`, `.range(1.0)`, `.bar_width(2)`, `.gap(1)`,
/// `.style("green")`). For two channels, make the node with
/// [`described`](crate::described) and `BarSpectrum::stereo(left, right)`.
pub fn bar_spectrum_with(
    bands: impl Fn() -> Vec<f64> + 'static,
    configure: impl Fn(BarSpectrum) -> BarSpectrum + 'static,
) -> Node {
    described(move || configure(BarSpectrum::new(bands())))
}

/// A trace's last frames drawn together, each older one fainter, as an
/// analogue scope's phosphor (xoscope's persistence): rs-rich-ext's
/// [`Persistence`] on a [`Chart`], from `frames`, a closure that may read
/// signals and returns the frames, oldest first, each a trace's points.
/// Keep them with the core's [`recent`](crate::recent) (`recent(trace,
/// 8)`) or [`recent_of`](crate::recent_of). The axes span the points
/// (-1 to 1 up the side while they are all within it); the newest frame
/// is drawn on top.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{persistence_with, recent};
///
/// let app = App::new(|| {
///     let trace = signal(vec![(0.0, -1.0), (1.0, 1.0)]);
///     let frames = recent(trace, 4);
///     persistence_with(move || frames.get(), |p| p.style("#00ff00").fade(0.25), |chart| chart)
///         .on_key("f", move |_| trace.set(vec![(0.0, 1.0), (1.0, -1.0)]))
/// });
/// let mut driver = app.driver(10, 4);
/// driver.press("f");
/// // Both frames are drawn: an X.
/// let rows = driver.screen().plain();
/// assert!(rows[0].trim() != "" && rows[3].trim() != "", "{rows:?}");
/// ```
pub fn persistence<I>(frames: impl Fn() -> I + 'static) -> Node
where
    I: IntoIterator<Item = Vec<(f64, f64)>>,
{
    persistence_with(frames, |p| p, |chart| chart)
}

/// A [`persistence`] whose [`Persistence`] `trace` sets up (`.style(..)`,
/// `.fade(..)`, `.background(..)`, `.marker(..)`) and whose [`Chart`]
/// `chart` sets up (its axes' bounds and labels, a legend), each given one
/// made from the frames.
pub fn persistence_with<I>(
    frames: impl Fn() -> I + 'static,
    trace: impl Fn(Persistence) -> Persistence + 'static,
    chart: impl Fn(Chart) -> Chart + 'static,
) -> Node
where
    I: IntoIterator<Item = Vec<(f64, f64)>>,
{
    described(move || {
        let frames: Vec<Vec<(f64, f64)>> = frames().into_iter().collect();
        let (x, y) = spans(&frames);
        let datasets = trace(Persistence::new(frames)).datasets();
        chart(
            Chart::new(datasets)
                .x_axis(ChartAxis::default().bounds(x.0, x.1))
                .y_axis(ChartAxis::default().bounds(y.0, y.1))
                .layering(Layering::PerDataset),
        )
    })
}

/// The x and y spans of `frames`' points: y at least -1 to 1, an empty
/// or flat span widened by one each way.
fn spans(frames: &[Vec<(f64, f64)>]) -> ((f64, f64), (f64, f64)) {
    let points = frames
        .iter()
        .flatten()
        .filter(|(x, y)| x.is_finite() && y.is_finite());
    let (mut x, mut y) = ((f64::INFINITY, f64::NEG_INFINITY), (-1.0f64, 1.0f64));
    for &(px, py) in points {
        x = (x.0.min(px), x.1.max(px));
        y = (y.0.min(py), y.1.max(py));
    }
    if x.0 >= x.1 || x.0.is_infinite() {
        let mid = if x.0.is_finite() { x.0 } else { 0.0 };
        x = (mid - 1.0, mid + 1.0);
    }
    (x, y)
}

/// How far off a [`tuner`] reads before its colour changes: green within
/// this many cents, yellow within three times it, red beyond.
const IN_TUNE: f64 = 5.0;

/// A tuner's readout of `reading`, a closure that may read signals (an
/// rs-rich-ext [`Tuner`](rich_ext::dsp::Tuner)'s reading of the newest
/// spectrum): the note in big glyphs ([`digits`]), a needle from flat to
/// sharp across the width, its centre the note, and the frequency and
/// cents. Green within 5 cents, yellow within 15, red beyond; `--` and
/// `no signal` with no reading.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::tuner;
/// use rich_ext::dsp::Tuner;
///
/// let app = App::new(|| {
///     let samples: Vec<f64> = rich_ext::dsp::tone(442.0, 48_000.0).take(4096).collect();
///     let reading = Tuner::new().listen(&samples, 48_000.0);
///     tuner(move || reading)
/// });
/// let mut driver = app.driver(21, 5);
/// driver.settle();
/// let rows = driver.screen().plain();
/// // A4, big; the needle right of the centre, sharp.
/// assert_eq!(rows[0].trim_end(), "┌─┐╷ ╷");
/// assert_eq!(rows[3].trim_end(), "b─────────┼●────────#");
/// assert_eq!(rows[4].trim_end(), "441.8 Hz  +7.1 cents");
/// ```
pub fn tuner(reading: impl Fn() -> Option<Reading> + 'static) -> Node {
    let reading = std::rc::Rc::new(reading);
    let (note, needle, readout) = (reading.clone(), reading.clone(), reading);
    let colour = |r: &Reading| {
        let off = r.cents.abs();
        if off <= IN_TUNE {
            "green"
        } else if off <= 3.0 * IN_TUNE {
            "yellow"
        } else {
            "red"
        }
    };
    column([
        digits(move || match note() {
            Some(r) => format!("[{}]{}", colour(&r), r.name()),
            None => "[bright_black]--".to_string(),
        })
        .fixed(3),
        leaf(move |_, width, _| vec![needle_row(needle().as_ref(), width, colour)]).fixed(1),
        text(move || match readout() {
            Some(r) => format!("{:.1} Hz  [{}]{:+.1} cents", r.hz, colour(&r), r.cents),
            None => "[bright_black]no signal".to_string(),
        })
        .fixed(1),
    ])
    .what("tuner")
}

/// The needle's row, `width` cells: `b` (flat) at the left, `#` (sharp) at
/// the right, the note's mark in the middle, and the needle where the
/// cents put it.
fn needle_row(
    reading: Option<&Reading>,
    width: u16,
    colour: fn(&Reading) -> &'static str,
) -> Vec<Segment> {
    let width = usize::from(width);
    if width < 3 {
        return Vec::new();
    }
    let centre = (width - 1) / 2;
    let mut cells: Vec<char> = (0..width)
        .map(|i| match i {
            0 => 'b',
            i if i == width - 1 => '#',
            i if i == centre => '┼',
            _ => '─',
        })
        .collect();
    let at = reading.map(|r| {
        let reach = (width - 1 - centre).min(centre).saturating_sub(1) as f64;
        let off = (r.cents / 50.0).clamp(-1.0, 1.0) * reach;
        (centre as f64 + off).round() as usize
    });
    let mut out = Vec::new();
    if let (Some(at), Some(reading)) = (at, reading) {
        cells[at] = '●';
        let before: String = cells[..at].iter().collect();
        let after: String = cells[at + 1..].iter().collect();
        out.push(Segment::new(before, None));
        out.push(Segment::new("●", Style::parse(colour(reading)).ok()));
        out.push(Segment::new(after, None));
    } else {
        out.push(Segment::new(cells.into_iter().collect::<String>(), None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::signal;
    use rich_ext::chart::Charset;

    #[test]
    fn the_hold_keeps_the_highest_peak() {
        let app = crate::App::new(|| {
            let peak = signal(0.9);
            meter_with(
                move || 0.0,
                move || peak.get(),
                |m| m.charset(Charset::Ascii),
            )
            .hold(Duration::from_secs(3600))
            .on_key("d", move |_| peak.set(0.2))
            .on_key("q", |cx| cx.quit())
        });
        let screen = app.render_with(&["d", "q"], 10, 1).unwrap();
        // Peak now 0.2, the 0.9 held.
        assert_eq!(screen[0], "-|------|-");
    }

    #[test]
    fn without_a_hold_only_the_peak_is_marked() {
        let app = crate::App::new(|| {
            let peak = signal(0.9);
            meter_with(
                move || 0.0,
                move || peak.get(),
                |m| m.charset(Charset::Ascii),
            )
            .on_key("d", move |_| peak.set(0.2))
            .on_key("q", |cx| cx.quit())
        });
        let screen = app.render_with(&["d", "q"], 10, 1).unwrap();
        assert_eq!(screen[0], "-|--------");
    }

    #[test]
    fn a_meter_says_its_level() {
        let app = crate::App::new(|| meter(|| 0.5, || 0.75).label("L"));
        let mut driver = app.driver(10, 1);
        driver.update(Duration::ZERO);
        driver.render();
        let tree = driver.accessibility();
        let said: Vec<String> = tree.iter().map(|n| n.describe()).collect();
        assert!(
            said.iter()
                .any(|s| s.contains("Level meter: 0.5, 50 percent, peak 0.75")),
            "{said:?}"
        );
    }

    #[test]
    fn a_spectrogram_keeps_the_newest_on_top_as_the_history_grows() {
        let app = || {
            crate::App::new(|| {
                let spectrum = signal(Vec::new());
                let rows = crate::recent(spectrum, 2);
                spectrogram_with(
                    move || rows.get(),
                    |s| s.charset(Charset::Ascii).range(0.0, 1.0),
                )
                .on_key("l", move |_| spectrum.set(vec![1.0, 0.0]))
                .on_key("h", move |_| spectrum.set(vec![0.0, 1.0]))
                .on_key("q", |cx| cx.quit())
            })
        };
        let screen = app().render_with(&["l", "q"], 2, 3).unwrap();
        // One row so far; the rows below the oldest are blank.
        assert_eq!(screen, ["@ ", "  ", "  "]);
        let screen = app().render_with(&["l", "h", "l", "q"], 2, 3).unwrap();
        // Two kept, the newest on top: the oldest has gone.
        assert_eq!(screen, ["@ ", " @", "  "]);
    }

    #[test]
    fn a_bar_spectrum_says_its_loudest_band() {
        let app = crate::App::new(|| bar_spectrum(|| vec![0.25, 1.0]));
        let mut driver = app.driver(3, 2);
        driver.update(Duration::ZERO);
        driver.render();
        assert_eq!(driver.screen().plain(), ["  █", "▄ █"]);
        let said: Vec<String> = driver
            .accessibility()
            .iter()
            .map(|n| n.describe())
            .collect();
        assert!(
            said.iter().any(|s| s.contains("Loudest: band 2 of 2, 1")),
            "{said:?}"
        );
    }

    #[test]
    fn a_sparkline_follows_its_signal() {
        let app = crate::App::new(|| {
            let history = signal(vec![1.0, 2.0]);
            sparkline_with(move || history.get(), |s| s.range(0.0, 8.0))
                .on_key("a", move |_| history.update(|h| h.push(8.0)))
                .on_key("q", |cx| cx.quit())
        });
        let screen = app.render_with(&["a", "q"], 3, 1).unwrap();
        assert_eq!(screen[0], "▂▃█");
    }

    #[test]
    fn a_tuner_with_nothing_heard_says_so_and_colours_by_cents() {
        let app = crate::App::new(|| {
            let reading = signal(None::<Reading>);
            tuner(move || reading.get()).on_key("r", move |_| {
                reading.set(Some(Reading {
                    hz: 100.0,
                    note: "G",
                    octave: 2,
                    cents: -30.0,
                }))
            })
        });
        let mut driver = app.driver(11, 5);
        driver.settle();
        let rows = driver.screen().plain();
        assert_eq!(rows[3].trim_end(), "b────┼────#");
        assert_eq!(rows[4].trim_end(), "no signal");
        driver.press("r");
        let rows = driver.screen().plain();
        assert_eq!(rows[0].trim_end(), "┌─╴╶─┐");
        // Thirty cents flat: left of the centre, red.
        assert_eq!(rows[3].trim_end(), "b──●─┼────#");
        let styled = crate::testing::styled(&driver);
        assert!(styled.contains("[red]●"), "{styled}");
    }

    #[test]
    fn spans_cover_the_points_and_at_least_a_full_scale() {
        let frames = vec![vec![(0.0, 0.2), (4.0, 3.0)]];
        assert_eq!(spans(&frames), ((0.0, 4.0), (-1.0, 3.0)));
        assert_eq!(spans(&[]), ((-1.0, 1.0), (-1.0, 1.0)));
    }
}
