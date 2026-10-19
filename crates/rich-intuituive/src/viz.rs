//! Levels as nodes: a level meter with peak hold, and a sparkline, both
//! drawn by rs-rich-ext's charts from closures that read signals.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rich_ext::chart::{LevelMeter, PeakHold, Sparkline};

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
