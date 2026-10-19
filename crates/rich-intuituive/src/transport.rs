//! A play head on the app's clock: [`transport`], what a media player, a
//! slideshow, an animation's timeline or a replay moves through.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::app::{after, Ctx};
use crate::reactive::{signal, Runtime, Signal};

/// What [`Transport::on_end`] runs.
type EndHook = Rc<RefCell<dyn FnMut(&mut Ctx)>>;

/// A play head on the app's clock: a position that moves while playing
/// (at a [`rate`](Self::set_rate)), stops at the [`length`](Self::length),
/// and can be paused, sought and sped up, as a player's transport
/// controls. Generic: a track, a slideshow, a timeline, a recording's
/// replay. Readers of [`position`](Self::position) are drawn again every
/// [`tick`](Self::tick) while it plays (a tenth of a second by default),
/// and its end is met on the app's clock exactly, so a test stepping the
/// clock sees it at the same time on every run. `Copy`; made by
/// [`transport`].
pub struct Transport {
    length: Signal<Option<Duration>>,
    playing: Signal<bool>,
    rate: Signal<f64>,
    /// The position at a time on the app's clock, the play head moving on
    /// from there while it plays.
    anchor: Signal<(Duration, Duration)>,
    /// Bumped each tick while playing, so readers draw again.
    ticks: Signal<u64>,
    every: Signal<Duration>,
    /// Which run of ticks is current: a play, a seek or a new rate starts
    /// another, and the last stops.
    run: Signal<u64>,
    on_end: Signal<Option<EndHook>>,
}

impl Clone for Transport {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for Transport {}

/// A [`Transport`] at the start, paused, `length` long (`None`: no end, a
/// stream), at rate 1. Call it while the app is built or running.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::transport;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let head = transport(Duration::from_secs(3));
///     let ended = signal(false);
///     let head = head.on_end(move |_| ended.set(true));
///     text(move || format!("{:.1} {} {}", head.position().as_secs_f64(), head.is_playing(), ended.get()))
///         .on_key("space", move |_| head.toggle())
///         .on_key("l", move |_| head.seek_by(1.0))
///         .on_key("f", move |_| head.set_rate(2.0))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(20, 1);
/// driver.press("space");
/// driver.advance(Duration::from_millis(500));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "0.5 true false");
/// // A second on, then twice as fast: the end, a quarter second later.
/// driver.press("l f");
/// driver.advance(Duration::from_millis(500));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "2.5 true false");
/// driver.advance(Duration::from_millis(250));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "3.0 false true");
/// ```
///
/// # Panics
/// Outside a running or building app.
pub fn transport(length: impl Into<Option<Duration>>) -> Transport {
    Runtime::current_for("transport()");
    Transport {
        length: signal(length.into()),
        playing: signal(false),
        rate: signal(1.0),
        anchor: signal((Duration::ZERO, Duration::ZERO)),
        ticks: signal(0),
        every: signal(Duration::from_millis(100)),
        run: signal(0),
        on_end: signal(None),
    }
}

/// The app's clock now.
fn now() -> Duration {
    Runtime::current().now.get()
}

impl Transport {
    /// Draw readers of the position again every `every` while playing
    /// (default a tenth of a second; at least a millisecond).
    pub fn tick(self, every: Duration) -> Transport {
        self.every.set(every.max(Duration::from_millis(1)));
        self
    }

    /// Run `hook` when the play head reaches the end while playing (it
    /// stops there first): load the next track, loop back
    /// (`seek(Duration::ZERO)` and `play()`), close the view.
    pub fn on_end(self, hook: impl FnMut(&mut Ctx) + 'static) -> Transport {
        self.on_end.replace(Some(Rc::new(RefCell::new(hook))));
        self
    }

    /// Where the play head is, subscribing (drawn again each tick while
    /// it plays).
    pub fn position(&self) -> Duration {
        self.ticks.with(|_| ());
        self.playing.with(|_| ());
        self.anchor.with(|_| ());
        self.position_untracked()
    }

    /// Where the play head is, without subscribing.
    pub fn position_untracked(&self) -> Duration {
        let (at, since) = self.anchor.get_untracked();
        let moved = if self.playing.get_untracked() {
            now()
                .saturating_sub(since)
                .mul_f64(self.rate.get_untracked())
        } else {
            Duration::ZERO
        };
        let position = at + moved;
        match self.length.get_untracked() {
            Some(length) => position.min(length),
            None => position,
        }
    }

    /// How far through it is, from 0 to 1 (0 without a length),
    /// subscribing: for a progress bar.
    pub fn progress(&self) -> f64 {
        let position = self.position();
        match self.length.get() {
            Some(length) if !length.is_zero() => {
                (position.as_secs_f64() / length.as_secs_f64()).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    /// Its length (`None`: no end), subscribing.
    pub fn length(&self) -> Option<Duration> {
        self.length.get()
    }

    /// A new length (the next track's), the play head kept within it.
    pub fn set_length(&self, length: impl Into<Option<Duration>>) {
        let position = self.position_untracked();
        self.length.set(length.into());
        self.place(position);
    }

    /// Whether it is playing, subscribing.
    pub fn is_playing(&self) -> bool {
        self.playing.get()
    }

    /// Whether it is playing, without subscribing.
    pub fn is_playing_untracked(&self) -> bool {
        self.playing.get_untracked()
    }

    /// Its rate, subscribing: 1 is real time.
    pub fn rate(&self) -> f64 {
        self.rate.get()
    }

    /// Play from where it is (from the start again at the end).
    pub fn play(&self) {
        if self.playing.get_untracked() {
            return;
        }
        let mut position = self.position_untracked();
        if self.length.get_untracked() == Some(position) {
            position = Duration::ZERO;
        }
        self.playing.set(true);
        self.place(position);
    }

    /// Stop where it is.
    pub fn pause(&self) {
        if !self.playing.get_untracked() {
            return;
        }
        let position = self.position_untracked();
        self.playing.set(false);
        self.place(position);
    }

    /// Play if paused, pause if playing.
    pub fn toggle(&self) {
        if self.playing.get_untracked() {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Move the play head to `to` (kept within the length), playing on if
    /// it was.
    pub fn seek(&self, to: Duration) {
        self.place(to);
    }

    /// Move the play head `seconds` on (back, for less than 0).
    pub fn seek_by(&self, seconds: f64) {
        let position = self.position_untracked().as_secs_f64() + seconds;
        self.place(Duration::from_secs_f64(position.max(0.0)));
    }

    /// Play at `rate` (1 is real time; kept from 0.01 to 100): a player's
    /// speed.
    pub fn set_rate(&self, rate: f64) {
        if !rate.is_finite() {
            return;
        }
        let position = self.position_untracked();
        self.rate.set(rate.clamp(0.01, 100.0));
        self.place(position);
    }

    /// Put the play head at `position` now, and time the ticks and the end
    /// from here.
    fn place(&self, position: Duration) {
        let position = match self.length.get_untracked() {
            Some(length) => position.min(length),
            None => position,
        };
        self.anchor.set((position, now()));
        self.run.update(|r| *r += 1);
        self.ticks.update(|t| *t += 1);
        if self.playing.get_untracked() {
            self.schedule(self.run.get_untracked());
        }
    }

    /// The next tick of run `run`: at the tick's time, or at the end if
    /// that comes first.
    fn schedule(&self, run: u64) {
        let head = *self;
        let mut wait = self.every.get_untracked();
        if let Some(length) = self.length.get_untracked() {
            let left = length.saturating_sub(self.position_untracked());
            let real = left.div_f64(self.rate.get_untracked());
            wait = wait.min(real);
        }
        after(wait, move |cx| head.on_tick(run, cx));
    }

    fn on_tick(&self, run: u64, cx: &mut Ctx) {
        if self.run.get_untracked() != run || !self.playing.get_untracked() {
            return;
        }
        let at_end = self
            .length
            .get_untracked()
            .is_some_and(|length| self.position_untracked() >= length);
        if !at_end {
            self.ticks.update(|t| *t += 1);
            self.schedule(run);
            return;
        }
        let length = self.length.get_untracked().unwrap_or_default();
        self.playing.set(false);
        self.place(length);
        let hook = self.on_end.with_untracked(Clone::clone);
        if let Some(hook) = hook {
            (hook.borrow_mut())(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_has_no_end_and_a_seek_back_stops_at_zero() {
        let app = crate::App::new(|| {
            let head = transport(None);
            head.play();
            crate::text(move || format!("{:.1}", head.position().as_secs_f64()))
                .on_key("h", move |_| head.seek_by(-10.0))
                .on_key("p", move |_| head.pause())
        })
        .wait_for_tasks(true);
        let mut driver = app.driver(10, 1);
        driver.advance(Duration::from_secs(100));
        assert_eq!(driver.screen().plain()[0].trim_end(), "100.0");
        driver.press("p h");
        assert_eq!(driver.screen().plain()[0].trim_end(), "90.0");
        driver.advance(Duration::from_secs(5));
        assert_eq!(driver.screen().plain()[0].trim_end(), "90.0", "paused");
    }

    #[test]
    fn the_end_hook_can_loop() {
        let app = crate::App::new(|| {
            let loops = signal(0);
            let head = transport(Duration::from_secs(1));
            let head = head.on_end(move |_| {
                loops.update(|l| *l += 1);
                head.play();
            });
            head.play();
            crate::text(move || format!("{} {:.1}", loops.get(), head.position().as_secs_f64()))
        })
        .wait_for_tasks(true);
        let mut driver = app.driver(10, 1);
        driver.advance(Duration::from_millis(2500));
        assert_eq!(driver.screen().plain()[0].trim_end(), "2 0.5");
    }
}
