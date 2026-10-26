//! A worker thread that steps in lockstep with the app's clock.
//!
//! [`stepped_worker`] runs a simulation that must live on its own thread
//! (an emulator core, a physics or cellular-automaton model, a script VM,
//! a deterministic replay: anything whose stepping blocks) so that it
//! advances exactly as the app's clock does. A [`fixed_step`] on the app's
//! side hands the worker the ticks due, waits (up to a timeout) for it to
//! finish them, and puts its newest output in a signal. In a terminal that
//! is real time; under a [`Driver`](crate::Driver) it is the driver's time,
//! exactly, so a test of the worker is repeatable.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::app::{fixed_step, Timer};
use crate::reactive::{signal, Signal};

/// How [`stepped_worker_with`] paces its worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct Lockstep {
    /// One tick of the app's clock.
    pub period: Duration,
    /// The most ticks handed over at once, after a stall; the rest are
    /// dropped.
    pub max_catch_up: u64,
    /// How long the app's step waits for the worker to finish its ticks
    /// before drawing without it. A worker that falls behind gets the
    /// ticks it missed with the next step.
    pub timeout: Duration,
}

impl Lockstep {
    /// Ticks of `period`, at most `max_catch_up` at once, waited for up to
    /// 250 ms.
    pub fn new(period: Duration, max_catch_up: u64) -> Lockstep {
        Lockstep {
            period,
            max_catch_up: max_catch_up.max(1),
            timeout: Duration::from_millis(250),
        }
    }

    /// Wait up to `timeout` for the worker each step.
    pub fn timeout(mut self, timeout: Duration) -> Lockstep {
        self.timeout = timeout;
        self
    }
}

/// What the app and the worker share.
struct State<T> {
    /// Ticks the app has given, in all.
    given: u64,
    /// Ticks the worker has taken, in all.
    taken: u64,
    /// Ticks the worker has finished: it took them and came back for more.
    done: u64,
    /// The newest output not yet handed to the app.
    output: Option<T>,
    /// The worker has returned (or panicked).
    ended: bool,
    /// The app has gone: the worker's next wait returns `None`.
    closed: bool,
}

struct Shared<T> {
    state: Mutex<State<T>>,
    changed: Condvar,
}

impl<T> Shared<T> {
    fn lock(&self) -> MutexGuard<'_, State<T>> {
        // A panic while holding the lock leaves plain counters behind;
        // they are still good.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The worker's side of a [`stepped_worker`]: [`wait`](Steps::wait) for
/// ticks, [`publish`](Steps::publish) output. Dropping it (the worker's
/// function returning, or panicking) tells the app the worker has ended.
pub struct Steps<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Steps<T> {
    /// Block until the app gives ticks, and return how many: the ticks
    /// handed over before this call are counted as done, and the app's
    /// step stops waiting for them. `None` once the app has gone: return
    /// then.
    pub fn wait(&self) -> Option<u64> {
        let mut state = self.shared.lock();
        state.done = state.taken;
        self.shared.changed.notify_all();
        loop {
            if state.closed {
                return None;
            }
            if state.given > state.taken {
                let ticks = state.given - state.taken;
                state.taken = state.given;
                return Some(ticks);
            }
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Hand `output` to the app: it shows in the signal when the app's
    /// step next takes it (at once, if the step is waiting). A newer
    /// output replaces one not taken yet.
    pub fn publish(&self, output: T) {
        self.shared.lock().output = Some(output);
    }
}

impl<T> Drop for Steps<T> {
    fn drop(&mut self) {
        self.shared.lock().ended = true;
        self.shared.changed.notify_all();
    }
}

/// The app's side: closes the worker's waits when the app drops its step.
struct AppSide<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Drop for AppSide<T> {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.changed.notify_all();
    }
}

/// Run `work` on a thread of its own, stepped by the app's clock: each
/// `period` the app hands it the ticks due (at most `max_catch_up` after
/// a stall), waits up to 250 ms for it to finish them, and shows its
/// newest output in the signal returned (`None` until the first).
/// `work` calls [`Steps::wait`] for ticks and [`Steps::publish`] for
/// output, and returns when `wait` gives `None` (the app has gone). A
/// worker that returns early makes each step return at once.
/// [`stepped_worker_with`] takes the timeout too.
///
/// Under a [`Driver`](crate::Driver) it is exact: `advance` hands the
/// worker every tick in turn and waits for each, so the signal holds the
/// last tick's output when `advance` returns.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::stepped_worker;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     // A counter on its own thread, one count a tick.
///     let count = stepped_worker(Duration::from_millis(10), 5, |steps| {
///         let mut n = 0u64;
///         while let Some(ticks) = steps.wait() {
///             n += ticks;
///             steps.publish(n);
///         }
///     });
///     text!("{:?}", count.get())
/// });
/// let mut driver = app.driver(10, 1);
/// driver.advance(Duration::from_millis(100));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Some(10)");
/// ```
///
/// # Panics
/// Outside `App::new`'s closure or a screen's, as [`fixed_step`].
#[track_caller]
pub fn stepped_worker<T: Send + 'static>(
    period: Duration,
    max_catch_up: u64,
    work: impl FnOnce(Steps<T>) + Send + 'static,
) -> Signal<Option<T>> {
    stepped_worker_with(Lockstep::new(period, max_catch_up), work).0
}

/// [`stepped_worker`] paced as `pace` says, with its timer: to pause or
/// cancel the stepping.
#[track_caller]
pub fn stepped_worker_with<T: Send + 'static>(
    pace: Lockstep,
    work: impl FnOnce(Steps<T>) + Send + 'static,
) -> (Signal<Option<T>>, Timer) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            given: 0,
            taken: 0,
            done: 0,
            output: None,
            ended: false,
            closed: false,
        }),
        changed: Condvar::new(),
    });
    let output: Signal<Option<T>> = signal(None);
    let app = AppSide {
        shared: shared.clone(),
    };
    let timeout = pace.timeout;
    let timer = fixed_step(pace.period, move |_, ticks| {
        let shared = &app.shared;
        let mut state = shared.lock();
        if !state.ended {
            state.given += ticks;
            shared.changed.notify_all();
            let deadline = Instant::now() + timeout;
            while !state.ended && state.done < state.given {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                state = shared
                    .changed
                    .wait_timeout(state, left)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
        if let Some(newest) = state.output.take() {
            drop(state);
            output.update(|shown| *shown = Some(newest));
        }
    })
    .max_catch_up(pace.max_catch_up);
    let steps = Steps { shared };
    std::thread::Builder::new()
        .name("stepped-worker".into())
        .spawn(move || work(steps))
        .expect("spawn the stepped worker's thread");
    (output, timer)
}
