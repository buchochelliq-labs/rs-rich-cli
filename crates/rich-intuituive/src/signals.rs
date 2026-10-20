//! Signals that follow other signals over time: [`history`] (back and
//! forward through a signal's past values), [`recent`] (its last writes;
//! [`recent_of`] a part of each, [`recent_timed`] each with its time),
//! [`smoothed`] (a number easing after another on the app's clock) and
//! [`falling`] (a level that jumps up and falls with gravity).

use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use crate::app::Ctx;
use crate::reactive::{on_write_map, signal, watch, Memo, Runtime, Signal};

/// Back and forward through the values a signal has had, as a browser's
/// history, a pager's or a file manager's: [`history`] makes one. A `Copy`
/// handle, as a signal is.
pub struct History<T: 'static> {
    source: Signal<T>,
    state: Signal<Past<T>>,
}

impl<T> Clone for History<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for History<T> {}

/// A history's values, oldest first, the one shown, and how many it keeps.
struct Past<T> {
    values: Vec<T>,
    at: usize,
    limit: usize,
}

/// Keep track of the values `source` takes, so a [`History`] can go
/// [`back`](History::back) to an earlier one and [`forward`](History::forward)
/// again. Each change of `source` (as a [`watch`] sees it: an equal value
/// is no change) after the one shown drops those after it, as a browser
/// does after going back; a history's own moves are not recorded.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::history;
///
/// let app = App::new(|| {
///     let dir = signal("/");
///     let past = history(dir);
///     text!("{dir}")
///         .on_key("u", move |_| dir.set("/usr"))
///         .on_key("b", move |_| dir.set("/usr/bin"))
///         .on_key("h", move |_| {
///             past.back();
///         })
///         .on_key("l", move |_| {
///             past.forward();
///         })
/// });
/// let mut driver = app.driver(10, 1);
/// driver.press("u b h h");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "/");
/// driver.press("l");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "/usr");
/// ```
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn history<T: PartialEq + Clone + 'static>(source: Signal<T>) -> History<T> {
    let first = source.get_untracked();
    let state = signal(Past {
        values: vec![first],
        at: 0,
        limit: usize::MAX,
    });
    watch(
        move || source.get(),
        move |value, _| {
            state.update(|past| {
                // A move of the history's own: it is shown already.
                if past.values.get(past.at) == Some(&value) {
                    return;
                }
                past.values.truncate(past.at + 1);
                past.values.push(value);
                if past.values.len() > past.limit {
                    let over = past.values.len() - past.limit;
                    past.values.drain(..over);
                }
                past.at = past.values.len() - 1;
            })
        },
    );
    History { source, state }
}

impl<T: PartialEq + Clone + 'static> History<T> {
    /// Keep the last `limit` values at most (at least one); every value,
    /// by default.
    #[must_use]
    pub fn limit(self, limit: usize) -> History<T> {
        self.state.update(|past| {
            past.limit = limit.max(1);
            if past.values.len() > past.limit {
                let over = past.values.len() - past.limit;
                past.values.drain(..over);
                past.at = past.at.saturating_sub(over);
            }
        });
        self
    }

    /// Set the signal back to the value before; whether there was one.
    pub fn back(&self) -> bool {
        self.go(-1)
    }

    /// Set the signal on to the value after (one [`back`](Self::back) went
    /// from); whether there was one.
    pub fn forward(&self) -> bool {
        self.go(1)
    }

    fn go(&self, by: isize) -> bool {
        let next = self.state.with_untracked(|past| {
            let at = past.at.checked_add_signed(by)?;
            past.values.get(at).map(|value| (at, value.clone()))
        });
        let Some((at, value)) = next else {
            return false;
        };
        self.state.update(|past| past.at = at);
        self.source.set(value);
        true
    }

    /// Whether there is a value to go [`back`](Self::back) to. A node that
    /// reads it draws again when it changes (a back button, dimmed).
    pub fn can_back(&self) -> bool {
        self.state.with(|past| past.at > 0)
    }

    /// Whether there is a value to go [`forward`](Self::forward) to.
    pub fn can_forward(&self) -> bool {
        self.state.with(|past| past.at + 1 < past.values.len())
    }

    /// The values it keeps, oldest first, and which is shown.
    pub fn entries(&self) -> (Vec<T>, usize) {
        self.state.with(|past| (past.values.clone(), past.at))
    }
}

/// How many: a number, or a signal or memo of one that may change.
pub trait Count: 'static {
    fn count(&self) -> usize;
}

impl Count for usize {
    fn count(&self) -> usize {
        *self
    }
}

impl Count for Signal<usize> {
    fn count(&self) -> usize {
        self.get()
    }
}

impl Count for Memo<usize> {
    fn count(&self) -> usize {
        self.get()
    }
}

/// The last `n` values written to `source`, oldest first: each write, an
/// equal value too, as [`on_write`](crate::on_write) sees them. An
/// average over the last frames, a meter's peak, an oscilloscope's
/// persistence. `n` may be a signal (or memo): when it shrinks, the
/// oldest go.
///
/// Each value is cloned once, into the history. For a big value (a
/// buffer, a frame) shown as well as kept, hold it in the signal as an
/// `Arc` (or `Rc`), so the history shares it, or keep a part of each with
/// [`recent_of`].
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::recent;
///
/// let app = App::new(|| {
///     let level = signal(0);
///     let last = recent(level, 3);
///     text(move || last.with(|l| format!("{l:?}")))
///         .on_key("+", move |_| level.update(|n| *n += 1))
/// });
/// let mut driver = app.driver(12, 1);
/// driver.press("+ + + +");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "[2, 3, 4]");
/// ```
///
/// # Panics
/// Outside a running or building app, or for a signal of another app.
#[track_caller]
pub fn recent<T: Clone + 'static>(source: Signal<T>, n: impl Count) -> Memo<VecDeque<T>> {
    keep_last(source, n, T::clone, |value, _| value)
}

/// [`recent`], keeping `keep` of each value written rather than a clone of
/// it: a part (a channel of a frame, its peak), a summary, or a shared
/// handle. Nothing else of the value is copied.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::recent_of;
///
/// let app = App::new(|| {
///     let frame = signal(vec![0u8; 4096]);
///     // The last three frames' peaks, not the frames.
///     let peaks = recent_of(frame, 3, |f: &Vec<u8>| f.iter().copied().max().unwrap_or(0));
///     text(move || peaks.with(|p| format!("{p:?}")))
///         .on_key("w", move |_| frame.update(|f| f[0] += 1))
/// });
/// let mut driver = app.driver(12, 1);
/// driver.press("w w w w");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "[2, 3, 4]");
/// ```
///
/// # Panics
/// Outside a running or building app, or for a signal of another app.
#[track_caller]
pub fn recent_of<T: 'static, U: 'static>(
    source: Signal<T>,
    n: impl Count,
    keep: impl Fn(&T) -> U + 'static,
) -> Memo<VecDeque<U>> {
    keep_last(source, n, keep, |value, _| value)
}

/// [`recent`], each value with the time it was written on the app's clock
/// ([`Ctx::now`](crate::Ctx::now) gives the time to measure its age
/// against): a trace whose old frames fade by age, toasts or log lines
/// that go after a while.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::recent_timed;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let level = signal(0);
///     let last = recent_timed(level, 2);
///     text(move || last.with(|l| format!("{l:?}")))
///         .on_key("+", move |_| level.update(|n| *n += 1))
/// });
/// let mut driver = app.driver(40, 1);
/// driver.press("+");
/// driver.advance(Duration::from_millis(500));
/// driver.press("+");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "[(0ns, 1), (500ms, 2)]");
/// ```
///
/// # Panics
/// Outside a running or building app, or for a signal of another app.
#[track_caller]
pub fn recent_timed<T: Clone + 'static>(
    source: Signal<T>,
    n: impl Count,
) -> Memo<VecDeque<(Duration, T)>> {
    keep_last(source, n, T::clone, |value, cx| (cx.now(), value))
}

/// The last `n` writes to `source`, each kept as `keep` takes it and
/// `stamp` finishes it on the app's thread.
#[track_caller]
fn keep_last<T: 'static, U: 'static, V: 'static>(
    source: Signal<T>,
    n: impl Count,
    keep: impl Fn(&T) -> U + 'static,
    stamp: impl Fn(U, &Ctx) -> V + 'static,
) -> Memo<VecDeque<V>> {
    let n = Rc::new(n);
    let kept = signal(VecDeque::new());
    let room = n.clone();
    on_write_map(source, keep, move |value, cx| {
        let n = room.count();
        let value = stamp(value, cx);
        kept.update(|kept: &mut VecDeque<V>| {
            kept.push_back(value);
            while kept.len() > n {
                kept.pop_front();
            }
        })
    });
    watch(
        move || n.count(),
        move |n, _| {
            if kept.with_untracked(|kept| kept.len() > n) {
                kept.update(|kept| {
                    let over = kept.len() - n;
                    kept.drain(..over);
                });
            }
        },
    );
    Memo::of(kept)
}

/// How often a [`smoothed`] value moves while it eases: a frame's time.
const SMOOTH_STEP: Duration = Duration::from_millis(16);

/// A number that follows `source` on the app's clock, rising with the time
/// constant `attack` and falling with `release` (an exponential ease: after
/// one time constant it has come about 63% of the way). A level meter's
/// needle, a progress bar that glides, a spectrum's bars falling back
/// slowly. A zero time constant follows at once. It moves a frame at a
/// time while it eases, and stops once it is there.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::smoothed;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let level = signal(0.0);
///     let needle = smoothed(level, Duration::from_millis(100), Duration::from_secs(1));
///     text(move || format!("{:.2}", needle.get())).on_key("x", move |_| level.set(1.0))
/// });
/// let mut driver = app.driver(10, 1);
/// driver.press("x");
/// driver.advance(Duration::from_millis(100));
/// // About 63% of the way after one time constant.
/// let shown: f64 = driver.screen().plain()[0].trim().parse().unwrap();
/// assert!((0.55..0.72).contains(&shown), "{shown}");
/// driver.advance(Duration::from_secs(1));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "1.00");
/// ```
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn smoothed<T: Copy + Into<f64> + 'static>(
    source: Signal<T>,
    attack: Duration,
    release: Duration,
) -> Memo<f64> {
    let out = signal(source.get_untracked().into());
    // Easing: when the last step was, on the app's clock.
    let last: Rc<std::cell::Cell<Option<Duration>>> = Rc::default();
    watch(
        move || source.with(|value| (*value).into()),
        move |target: f64, cx| {
            if last.get().is_none() && out.get_untracked() != target {
                last.set(Some(now()));
                step(cx, source, out, last.clone(), attack, release);
            }
        },
    );
    Memo::of(out)
}

/// A number that follows `source` up at once and falls back with
/// `gravity` (in units a second, a second): it falls slowly at first and
/// faster and faster, as a level meter's bar or peak, cava's bars, or a
/// badge's glow fading. It never falls below `source`, and stops once it
/// is there. Steps a frame at a time on the app's clock while it falls.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::falling;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let level = signal(0.0);
///     let bar = falling(level, 8.0);
///     text(move || format!("{:.2}", bar.get()))
///         .on_key("x", move |_| {
///             level.set(1.0);
///         })
///         .on_key("z", move |_| level.set(0.0))
/// });
/// let mut driver = app.driver(10, 1);
/// driver.press("x z");
/// // Up at once; after a quarter of a second, ½·8·0.25² = 0.25 down.
/// driver.advance(Duration::from_millis(250));
/// let shown: f64 = driver.screen().plain()[0].trim().parse().unwrap();
/// assert!((0.70..0.80).contains(&shown), "{shown}");
/// driver.advance(Duration::from_secs(1));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "0.00");
/// ```
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn falling<T: Copy + Into<f64> + 'static>(source: Signal<T>, gravity: f64) -> Memo<f64> {
    let out = signal(source.get_untracked().into());
    // Falling: since when, from what height, on the app's clock.
    let fall: Rc<std::cell::Cell<Option<(Duration, f64)>>> = Rc::default();
    watch(
        move || source.with(|value| (*value).into()),
        move |target: f64, cx| {
            if target >= out.get_untracked() {
                // Up at once; a fall under way stops.
                fall.set(None);
                out.set(target);
            } else if fall.get().is_none() {
                let since = now();
                fall.set(Some((since, out.get_untracked())));
                fall_step(cx, source, out, fall.clone(), since, gravity);
            }
        },
    );
    Memo::of(out)
}

/// One step of a [`falling`] value's fall, and the next after a frame
/// while it is above its source.
fn fall_step<T: Copy + Into<f64> + 'static>(
    cx: &mut Ctx,
    source: Signal<T>,
    out: Signal<f64>,
    fall: Rc<std::cell::Cell<Option<(Duration, f64)>>>,
    since: Duration,
    gravity: f64,
) {
    // Stopped, or a later fall took over.
    let Some((_, from)) = fall.get().filter(|(at, _)| *at == since) else {
        return;
    };
    let target: f64 = source.get_untracked().into();
    let t = now().saturating_sub(since).as_secs_f64();
    let next = (from - 0.5 * gravity.max(0.0) * t * t).max(target);
    out.set(next);
    if next <= target || (gravity <= 0.0 && t > 0.0) {
        fall.set(None);
        out.set(target);
        return;
    }
    cx.after(SMOOTH_STEP, move |cx| {
        fall_step(cx, source, out, fall, since, gravity)
    });
}

/// The app's clock at the top of this turn.
fn now() -> Duration {
    Runtime::current().now.get()
}

/// One step of a [`smoothed`] value's ease, and the next one after a frame
/// while it is not there.
fn step<T: Copy + Into<f64> + 'static>(
    cx: &mut Ctx,
    source: Signal<T>,
    out: Signal<f64>,
    last: Rc<std::cell::Cell<Option<Duration>>>,
    attack: Duration,
    release: Duration,
) {
    let now = now();
    let target: f64 = source.get_untracked().into();
    let at = out.get_untracked();
    let since = last
        .get()
        .map_or(Duration::ZERO, |last| now.saturating_sub(last));
    let tau = if target > at { attack } else { release };
    let next = if tau.is_zero() {
        target
    } else {
        at + (target - at) * (1.0 - (-since.as_secs_f64() / tau.as_secs_f64()).exp())
    };
    // Close enough: there.
    let there = (target - next).abs() <= 1e-4 * target.abs().max(1.0);
    out.set(if there { target } else { next });
    if there {
        last.set(None);
        return;
    }
    last.set(Some(now));
    cx.after(SMOOTH_STEP, move |cx| {
        step(cx, source, out, last, attack, release)
    });
}
