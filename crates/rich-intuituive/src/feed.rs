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
    let runtime = Runtime::current_for("latest_with_state()");
    let state = signal(FeedState::Running);
    let proxy = runtime.proxy();
    let value = latest(move |tx| {
        let end = match catch_unwind(AssertUnwindSafe(|| run(tx))) {
            Ok(end) => end.state(),
            Err(_) => FeedState::Failed("the feed's thread panicked".to_string()),
        };
        // After its last value: the proxy keeps the order.
        proxy.run(move || state.set(end));
    });
    if let Some(stall) = stall_after.into() {
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
/// [`signal`](Self::signal), [`then`](Self::then) or
/// [`then_with`](Self::then_with) starts it.
#[must_use = "nothing plays until .signal(), .then(..) or .then_with(..)"]
pub struct Play<T> {
    values: Box<dyn Iterator<Item = T> + Send>,
    pace: Pace,
}

/// Play `values` onto a signal on a thread of its own, at `pace`: a
/// recording played back, a demo's script, frames of a file. Then
/// something after: another iterator at the same pace
/// ([`then`](Play::then): a fallback, silence forever), or a closure with
/// the thread's [`Latest`] ([`then_with`](Play::then_with)); or nothing
/// ([`signal`](Play::signal)), and the signal keeps the last value.
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
        Play {
            values: Box::new(self.values.chain(after)),
            pace: self.pace,
        }
        .signal()
    }

    /// Start playing, and then call `after` on the thread with its
    /// [`Latest`] (if the app has not finished): to send more, or mark the
    /// end. Its [`sleep`](Latest::sleep) keeps to the same beat.
    #[track_caller]
    pub fn then_with(self, after: impl FnOnce(&Latest<T>) + Send + 'static) -> Signal<Option<T>> {
        let Play { values, pace } = self;
        latest(move |tx| {
            let mut first = true;
            for value in values {
                if let (Pace::Every(period), false) = (pace, first) {
                    if !tx.sleep(period) {
                        return;
                    }
                }
                first = false;
                if !tx.send(value) {
                    return;
                }
            }
            after(&tx);
        })
    }
}
