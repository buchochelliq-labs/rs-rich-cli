//! The `viz` plugin: levels and spectra as nodes, each drawn by one of
//! rs-rich-ext's charts from a closure that reads signals. A level meter
//! with peak hold, a sparkline, a spectrogram (a waterfall) and a bar
//! spectrum (cava's bars). There only with the off-by-default `viz`
//! feature: they are views of one domain's data (levels and spectra), not
//! components other components build on, so they are not in the core.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rich_ext::chart::{BarSpectrum, LevelMeter, PeakHold, Sparkline, Spectrogram};

use crate::node::{described, Axis, Node};
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
}
