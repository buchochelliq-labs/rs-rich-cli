//! Threads that feed a signal, beyond [`latest`]: one that says how it is
//! doing ([`latest_with_state`]: running, stalled, ended or failed), and an
//! iterator played onto a signal at a pace ([`latest_iter`]).

use std::cell::Cell;
use std::fmt::Display;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::time::Duration;

use crate::app::{after, Ctx};
use crate::reactive::{on_write_ref, signal, Runtime, Signal};
use crate::task::{latest, Latest};

/// How a thread feeding a signal is doing: [`latest_with_state`]'s second
/// signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeedState {
    /// Sending values (or not yet sent one, and not stalled).
    Running,
    /// No value for the feed's `stall_after` time: the time on the app's
    /// clock of its last value (or of its start, with none). A value
    /// coming makes it `Running` again.
    Stalled(Duration),
    /// Its thread finished: the end of a file, a pipe closed.
    Ended,
    /// Its thread failed, saying why (or panicked).
    Failed(String),
}

impl FeedState {
    /// Whether the thread is still there: running or stalled.
    pub fn is_live(&self) -> bool {
        matches!(self, FeedState::Running | FeedState::Stalled(_))
    }
}

/// What a [`latest_with_state`] thread's end means: `()` (or `Ok(())`) is
/// [`FeedState::Ended`], an `Err` [`FeedState::Failed`] with its message.
pub trait FeedEnd: Send + 'static {
    fn state(self) -> FeedState;
}

impl FeedEnd for () {
    fn state(self) -> FeedState {
        FeedState::Ended
    }
}

impl<E: Display + Send + 'static> FeedEnd for Result<(), E> {
    fn state(self) -> FeedState {
        match self {
            Ok(()) => FeedState::Ended,
            Err(error) => FeedState::Failed(error.to_string()),
        }
    }
}

/// [`latest`], with a second signal saying how its thread is doing
/// ([`FeedState`]): `Running`; `Stalled` once no value has come for
/// `stall_after` (`None`: never), until the next comes; `Ended` when `run`
/// returns `()` or `Ok(())`; `Failed` when it returns an `Err` (its
/// message) or panics. Any thread: a sensor, a log's tail, a socket, a
/// capture. Stalls are timed on the app's clock.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{latest_with_state, FeedState};
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let (line, state) = latest_with_state(Duration::from_millis(500), |tx| {
///         tx.send("first".to_string());
///         // Silent for a second, then the end.
///         tx.sleep(Duration::from_secs(1));
///         Err::<(), _>("the pipe closed")
///     });
///     text(move || format!("{:?} {:?}", line.get(), state.get()))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(60, 1);
/// driver.advance(Duration::from_millis(600));
/// assert_eq!(
///     driver.screen().plain()[0].trim_end(),
///     "Some(\"first\") Stalled(0ns)"
/// );
/// driver.advance(Duration::from_millis(500));
/// assert_eq!(
///     driver.screen().plain()[0].trim_end(),
///     "Some(\"first\") Failed(\"the pipe closed\")"
/// );
/// ```
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn latest_with_state<T: Send + 'static, R: FeedEnd>(
    stall_after: impl Into<Option<Duration>>,
    run: impl FnOnce(Latest<T>) -> R + Send + 'static,
) -> (Signal<Option<T>>, Signal<FeedState>) {
    feed_with_state("latest_with_state()", stall_after.into(), |tx, _| run(tx))
}

/// Tells a feed's state from its thread, in order with its values.
struct StateTeller {
    state: Signal<FeedState>,
    proxy: crate::reactive::Proxy,
}

impl StateTeller {
    fn tell(&self, now: FeedState) {
        let state = self.state;
        self.proxy.run(move || state.set(now));
    }
}

/// [`latest_with_state`], its thread also given a [`StateTeller`].
#[track_caller]
fn feed_with_state<T: Send + 'static, R: FeedEnd>(
    what: &str,
    stall_after: Option<Duration>,
    run: impl FnOnce(Latest<T>, &StateTeller) -> R + Send + 'static,
) -> (Signal<Option<T>>, Signal<FeedState>) {
    let runtime = Runtime::current_for(what);
    let state = signal(FeedState::Running);
    let proxy = runtime.proxy();
    let teller = StateTeller {
        state,
        proxy: proxy.clone(),
    };
    let value = latest(move |tx| {
        let end = match catch_unwind(AssertUnwindSafe(|| run(tx, &teller))) {
            Ok(end) => end.state(),
            Err(_) => FeedState::Failed("the feed's thread panicked".to_string()),
        };
        // After its last value: the proxy keeps the order.
        proxy.run(move || state.set(end));
    });
    if let Some(stall) = stall_after {
        let last = Rc::new(Cell::new(runtime.now.get()));
        let seen = last.clone();
        on_write_ref(value, move |_, cx| {
            seen.set(now());
            if matches!(state.get_untracked(), FeedState::Stalled(_)) {
                state.set(FeedState::Running);
                let last = seen.clone();
                cx.after(stall, move |cx| check_stall(cx, state, last, stall));
            }
        });
        let first = last.clone();
        after(stall, move |cx| check_stall(cx, state, first, stall));
    }
    (value, state)
}

/// The app's clock at the top of this turn.
fn now() -> Duration {
    Runtime::current().now.get()
}

/// Mark the feed stalled if no value came for `stall` since `last`, or
/// look again when it would be.
fn check_stall(cx: &mut Ctx, state: Signal<FeedState>, last: Rc<Cell<Duration>>, stall: Duration) {
    if state.get_untracked() != FeedState::Running {
        return;
    }
    let quiet = now().saturating_sub(last.get());
    if quiet >= stall {
        // Looked at again when a value comes.
        state.set(FeedState::Stalled(last.get()));
    } else {
        cx.after(stall - quiet, move |cx| check_stall(cx, state, last, stall));
    }
}

/// How fast [`latest_iter`] plays its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// One every this long, on the app's clock in an app that
    /// [waits for its tasks](crate::App::wait_for_tasks) (as
    /// [`Latest::sleep`] keeps to it), the first at once.
    Every(Duration),
    /// As fast as the thread makes them: in a running app, the newest wins
    /// (as any [`latest`] value); waiting for tasks, all of them at once.
    Free,
}

/// An iterator to play onto a signal: [`latest_iter`] makes it, and
/// [`signal`](Self::signal), [`with_state`](Self::with_state),
/// [`then`](Self::then) or [`then_with`](Self::then_with) starts it;
/// [`then_at`](Self::then_at) adds a tail at a pace of its own.
#[must_use = "nothing plays until .signal(), .with_state(), .then(..) or .then_with(..)"]
pub struct Play<T> {
    values: Box<dyn Iterator<Item = T> + Send>,
    pace: Pace,
    /// What plays after `values`, each at its own pace.
    tails: Vec<(Box<dyn Iterator<Item = T> + Send>, Pace)>,
    /// For [`with_state`](Play::with_state): stalled after this long
    /// with no value.
    stall_after: Option<Duration>,
}

/// Play `values` onto a signal on a thread of its own, at `pace`: a
/// recording played back, a demo's script, frames of a file. Then
/// something after: another iterator at the same pace
/// ([`then`](Play::then): a fallback, silence forever) or at its own
/// ([`then_at`](Play::then_at)), or a closure with the thread's
/// [`Latest`] ([`then_with`](Play::then_with)); or nothing
/// ([`signal`](Play::signal)), and the signal keeps the last value.
/// [`with_state`](Play::with_state) also says when `values` ran out.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{latest_iter, Pace};
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let frame = latest_iter(1..=3, Pace::Every(Duration::from_millis(100)))
///         .then(std::iter::repeat(0));
///     text!("{}", frame.get().unwrap_or(-1))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(5, 1);
/// driver.advance(Duration::from_millis(250));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "3");
/// driver.advance(Duration::from_millis(100));
/// // Then the fallback, on the same beat.
/// assert_eq!(driver.screen().plain()[0].trim_end(), "0");
/// ```
///
/// # Panics
/// (When started) outside a running or building app.
pub fn latest_iter<T: Send + 'static, I>(values: I, pace: Pace) -> Play<T>
where
    I: IntoIterator<Item = T>,
    I::IntoIter: Send + 'static,
{
    Play {
        values: Box::new(values.into_iter()),
        pace,
        tails: Vec::new(),
        stall_after: None,
    }
}

impl<T: Send + 'static> Play<T> {
    /// Start playing, with nothing after: the signal keeps the last value.
    #[track_caller]
    pub fn signal(self) -> Signal<Option<T>> {
        self.then_with(|_| {})
    }

    /// Start playing, and then play `after` at the same pace.
    #[track_caller]
    pub fn then<I>(self, after: I) -> Signal<Option<T>>
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: Send + 'static,
    {
        let pace = self.pace;
        self.then_at(pace, after).signal()
    }

    /// After the values (and any tail added before), play `after` at a
    /// pace of its own: a pipe read as fast as it comes, then silence a
    /// buffer's time apart. Nothing plays yet: start it with
    /// [`signal`](Self::signal) or [`with_state`](Self::with_state).
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{latest_iter, Pace};
    /// use std::time::Duration;
    ///
    /// let app = App::new(|| {
    ///     let frame = latest_iter(vec![1, 2, 3], Pace::Free)
    ///         .then_at(Pace::Every(Duration::from_millis(100)), (10..).step_by(10))
    ///         .signal();
    ///     text!("{}", frame.get().unwrap_or(-1))
    /// })
    /// .wait_for_tasks(true);
    /// let mut driver = app.driver(5, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "3");
    /// // The three at once, then the tail a beat apart.
    /// driver.advance(Duration::from_millis(250));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "20");
    /// ```
    pub fn then_at<I>(mut self, pace: Pace, after: I) -> Play<T>
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: Send + 'static,
    {
        self.tails.push((Box::new(after.into_iter()), pace));
        self
    }

    /// For [`with_state`](Self::with_state): `Stalled` once no value has
    /// come for `after` (`None`, the default: never), as
    /// [`latest_with_state`] times it.
    pub fn stall_after(mut self, after: impl Into<Option<Duration>>) -> Play<T> {
        self.stall_after = after.into();
        self
    }

    /// Start playing, with a second signal saying how it goes
    /// ([`FeedState`]): `Running` while the values play, `Ended` once they
    /// run out (while a tail from [`then_at`](Self::then_at) plays on: the
    /// end of a file, padded with silence), `Failed` if the iterator
    /// panics, and `Stalled` as [`stall_after`](Self::stall_after) says.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{latest_iter, FeedState, Pace};
    /// use std::time::Duration;
    ///
    /// let app = App::new(|| {
    ///     let beat = Pace::Every(Duration::from_millis(100));
    ///     let (frame, state) = latest_iter(1..=2, beat)
    ///         .then_at(beat, std::iter::repeat(0))
    ///         .with_state();
    ///     text(move || format!("{:?} {:?}", frame.get(), state.get()))
    /// })
    /// .wait_for_tasks(true);
    /// let mut driver = app.driver(20, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "Some(1) Running");
    /// driver.advance(Duration::from_millis(200));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "Some(0) Ended");
    /// ```
    #[track_caller]
    pub fn with_state(self) -> (Signal<Option<T>>, Signal<FeedState>) {
        let stall = self.stall_after;
        feed_with_state(
            "latest_iter(..).with_state()",
            stall,
            move |tx: Latest<T>, teller| {
                self.play(&tx, Some(teller));
            },
        )
    }

    /// Start playing, and then call `after` on the thread with its
    /// [`Latest`] (if the app has not finished): to send more, or mark the
    /// end. Its [`sleep`](Latest::sleep) keeps to the same beat.
    #[track_caller]
    pub fn then_with(self, after: impl FnOnce(&Latest<T>) + Send + 'static) -> Signal<Option<T>> {
        latest(move |tx| {
            if self.play(&tx, None).is_some() {
                after(&tx);
            }
        })
    }

    /// Play the values, then the tails, on the thread's `tx`, telling
    /// `teller` `Ended` when the values run out; `None` once the app has
    /// finished.
    fn play(self, tx: &Latest<T>, teller: Option<&StateTeller>) -> Option<()> {
        let Play {
            values,
            pace,
            tails,
            ..
        } = self;
        let mut first = true;
        for (n, (values, pace)) in std::iter::once((values, pace)).chain(tails).enumerate() {
            if n == 1 {
                if let Some(teller) = teller {
                    teller.tell(FeedState::Ended);
                }
            }
            for value in values {
                if let (Pace::Every(period), false) = (pace, first) {
                    if !tx.sleep(period) {
                        return None;
                    }
                }
                first = false;
                if !tx.send(value) {
                    return None;
                }
            }
        }
        Some(())
    }
}
