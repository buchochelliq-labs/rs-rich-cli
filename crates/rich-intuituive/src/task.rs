//! Background work: run something slow off the app's thread, and write
//! signals with its result when it is done.
//!
//! - [`spawn`] runs a closure on its own thread;
//! - [`spawn_future`] drives a future on its own thread, for async code
//!   that does not need a particular runtime (a tokio app keeps its own
//!   runtime and sends results back with a [`Proxy`](crate::Proxy));
//! - [`resource`] is a value that loads in the background, with
//!   [`Load::Loading`] until it arrives, and reloads on demand;
//!   [`resource_for`] loads again whenever a key changes;
//! - [`latest`] and [`latest_every`] are the newest value a thread of
//!   their own makes.
//!
//! The `done` closure runs back on the app's thread, between frames, with a
//! [`Ctx`]: it can write signals, open a screen, or quit. A cancelled
//! [`Task`] still finishes its work (a thread cannot be stopped from
//! outside), but its result is dropped.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::future::Future;
use std::panic::Location;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
#[cfg(not(target_arch = "wasm32"))]
use std::task::Wake;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::app::Ctx;
use crate::reactive::{signal, watch, Runtime, Signal};

/// A running piece of background work.
#[derive(Clone, Debug)]
pub struct Task {
    cancelled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl Task {
    /// Drop the result when it arrives: `done` will not run.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether the work has finished (its result may still be on its way
    /// to the app's thread).
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

/// Run `work` on its own thread; when it returns, run `done` with the
/// result on the app's thread. Call it while the app is built or running
/// (in `App::new`'s closure, a handler, or a node's closure).
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let answer = signal(String::from("thinking…"));
///     spawn(|| 6 * 7, move |n, _| answer.set(n.to_string()));
///     text!("{answer}").on_key("q", |cx| cx.quit())
/// })
/// .wait_for_tasks(true);
/// let screen = app.render_with(&["q"], 20, 1).unwrap();
/// assert_eq!(screen[0].trim_end(), "42");
/// ```
///
/// # In a browser
/// On `wasm32-unknown-unknown` there are no threads: `work` runs at once,
/// on the app's thread, and its result arrives on the next turn.
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn spawn<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T, &mut Ctx) + Send + 'static,
) -> Task {
    spawn_at(Location::caller(), work, done)
}

/// [`spawn`], saying in the inspector's trace that it was started at
/// `site`.
fn spawn_at<T: Send + 'static>(
    site: &'static Location<'static>,
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T, &mut Ctx) + Send + 'static,
) -> Task {
    let runtime = Runtime::current_for("spawn()");
    let proxy = runtime.proxy();
    let tasks = runtime.tasks.clone();
    let task = Task {
        cancelled: Arc::new(AtomicBool::new(false)),
        finished: Arc::new(AtomicBool::new(false)),
    };
    let handle = task.clone();
    tasks.fetch_add(1, Ordering::SeqCst);
    crate::clock::off_thread(move || {
        // Count the task delivered however the work ends, a panic included.
        struct Delivered(Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Delivered {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _delivered = Delivered(tasks);
        let value = work();
        handle.finished.store(true, Ordering::SeqCst);
        let cancelled = handle.cancelled;
        proxy.send(Box::new(move |cx| {
            let dropped = cancelled.load(Ordering::SeqCst);
            crate::reactive::traced(|| crate::inspect::Traced::Task { at: site, dropped });
            if !dropped {
                done(value, cx);
            }
        }));
    });
    task
}

/// [`spawn`] whose `done` need not be [`Send`]: it stays on the app's
/// thread while `work` runs, so it can hold an `Rc`, a node's handle, or
/// anything else that cannot cross threads. Only the value travels.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::spawn_then;
/// use std::rc::Rc;
///
/// let app = App::new(|| {
///     let shown = signal(String::new());
///     let prefix = Rc::new(String::from("answer: "));
///     spawn_then(|| 6 * 7, move |n, _| shown.set(format!("{prefix}{n}")));
///     text!("{shown}")
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(12, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "answer: 42");
/// ```
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn spawn_then<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T, &mut Ctx) + 'static,
) -> Task {
    let runtime = Runtime::current_for("spawn_then()");
    let id = runtime.next_local.get();
    runtime.next_local.set(id + 1);
    let done: Then<T> = Box::new(done);
    runtime.locals.borrow_mut().insert(id, Box::new(done));
    spawn_at(Location::caller(), work, move |value, cx| {
        // The continuation waited on this thread, under its number.
        let done = Runtime::current().locals.borrow_mut().remove(&id);
        if let Some(done) = done.and_then(|d| d.downcast::<Then<T>>().ok()) {
            done(value, cx);
        }
    })
}

/// Run `work` on a thread of its own with a [`Proxy`](crate::Proxy) to send
/// results back as it goes (lines of a log, entries of a directory walk),
/// counted as a [task](Task) until it returns: an app that
/// [waits for its tasks](crate::App::wait_for_tasks) waits for the whole
/// stream each turn, so a test sees all of it. The [`Task`] says when it
/// has finished; [`cancel`](Task::cancel) sets a flag `work` can read from
/// the [`Stream`] it is given, to stop early.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::spawn_stream;
///
/// let app = App::new(|| {
///     let lines = signal(Vec::<String>::new());
///     spawn_stream(move |stream| {
///         for n in 1..=3 {
///             if stream.cancelled() {
///                 break;
///             }
///             stream.proxy().run(move || lines.update(|l| l.push(format!("line {n}"))));
///         }
///     });
///     text(move || lines.get().join(","))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(30, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "line 1,line 2,line 3");
/// ```
///
/// # In a browser
/// On `wasm32-unknown-unknown` there are no threads: `work` runs at once,
/// on the app's thread, and what it sends arrives on the next turn.
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn spawn_stream(work: impl FnOnce(Stream) + Send + 'static) -> Task {
    let runtime = Runtime::current_for("spawn_stream()");
    let stream = Stream {
        proxy: runtime.proxy(),
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let task = Task {
        cancelled: stream.cancelled.clone(),
        finished: Arc::new(AtomicBool::new(false)),
    };
    let tasks = runtime.tasks.clone();
    let finished = task.finished.clone();
    tasks.fetch_add(1, Ordering::SeqCst);
    crate::clock::off_thread(move || {
        // Done however the work ends, a panic included. The count goes
        // down after the last result is queued, so a waiting app takes
        // them all.
        struct Done(Arc<std::sync::atomic::AtomicUsize>, Arc<AtomicBool>);
        impl Drop for Done {
            fn drop(&mut self) {
                self.1.store(true, Ordering::SeqCst);
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _done = Done(tasks, finished);
        work(stream);
    });
    task
}

/// What a [`spawn_stream`] thread sends its results with.
#[derive(Clone)]
pub struct Stream {
    proxy: crate::reactive::Proxy,
    cancelled: Arc<AtomicBool>,
}

impl Stream {
    /// The proxy to the app: [`run`](crate::Proxy::run) a closure on the
    /// app's thread with each result.
    pub fn proxy(&self) -> &crate::reactive::Proxy {
        &self.proxy
    }

    /// Whether the stream's [`Task`] was cancelled: stop then.
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// A [`spawn_then`] continuation, kept on the app's thread.
type Then<T> = Box<dyn FnOnce(T, &mut Ctx)>;

/// Drive `future` to completion on its own thread, then run `done` with its
/// output on the app's thread. The future is polled with a waker that
/// parks and unparks that thread, so it suits futures that do not need a
/// specific async runtime.
#[track_caller]
pub fn spawn_future<F>(future: F, done: impl FnOnce(F::Output, &mut Ctx) + Send + 'static) -> Task
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    spawn_at(Location::caller(), move || block_on(future), done)
}

/// Poll `future` on this thread until it is ready.
#[cfg(not(target_arch = "wasm32"))]
fn block_on<F: Future>(future: F) -> F::Output {
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::park(),
        }
    }
}

/// Poll `future` until it is ready. On wasm32 there is no other thread to
/// wake it, so it is polled again at once: a future that waits on I/O or a
/// timer never finishes there.
#[cfg(target_arch = "wasm32")]
fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

/// The state of a [`Resource`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Load<T> {
    /// Not arrived yet.
    Loading,
    Ready(T),
    /// The fetch failed, with its error.
    Failed(String),
}

impl<T> Load<T> {
    pub fn is_loading(&self) -> bool {
        matches!(self, Load::Loading)
    }

    /// The value, if it arrived.
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// A value fetched in the background. `Copy`, like a signal: read it in a
/// node and the node redraws when it arrives.
pub struct Resource<T: 'static> {
    pub(crate) state: Signal<Load<T>>,
    start: Signal<Rc<dyn Fn()>>,
    /// How long the last value stays while the next loads
    /// ([`stale_after`](Resource::stale_after)).
    stale: Signal<Option<Duration>>,
}

impl<T> Clone for Resource<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Resource<T> {}

impl<T: 'static> Resource<T> {
    /// Read the state through `f`, subscribing the reader.
    pub fn with<R>(&self, f: impl FnOnce(&Load<T>) -> R) -> R {
        self.state.with(f)
    }

    /// Fetch again: the state goes back to [`Load::Loading`], and a result
    /// still on its way from an earlier fetch is dropped.
    pub fn reload(&self) {
        let start = self.start.with_untracked(Rc::clone);
        start();
    }

    /// When it fetches again (a [`reload`](Self::reload), or a new key of
    /// [`resource_for`]), keep the last value (or error) for up to `wait`
    /// rather than going back to [`Load::Loading`] at once: a preview pane
    /// that does not blink between two quick loads, and clears once a slow
    /// one has kept the old value on show for `wait`. The wait is on the
    /// app's clock ([`after`](crate::after)), so a test steps through it
    /// with [`Driver::advance`](crate::Driver::advance). `None` goes
    /// back to loading at once, as without it.
    pub fn stale_after(self, wait: impl Into<Option<Duration>>) -> Resource<T> {
        self.stale.set(wait.into());
        self
    }
}

impl<T: Clone + 'static> Resource<T> {
    /// The state (a clone), subscribing the reader.
    pub fn get(&self) -> Load<T> {
        self.state.get()
    }
}

/// Fetch a value in the background with `fetch` (on its own thread), now
/// and on every [`Resource::reload`].
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let user = resource(|| Ok::<_, String>("Ada"));
///     text(move || match user.get() {
///         Load::Loading => "loading…".into(),
///         Load::Ready(name) => format!("Hello, {name}"),
///         Load::Failed(error) => format!("[red]{error}"),
///     })
///     .on_key("q", |cx| cx.quit())
/// })
/// .wait_for_tasks(true);
/// let screen = app.render_with(&["q"], 20, 1).unwrap();
/// assert_eq!(screen[0].trim_end(), "Hello, Ada");
/// ```
#[track_caller]
pub fn resource<T, E>(fetch: impl Fn() -> Result<T, E> + Send + Sync + 'static) -> Resource<T>
where
    T: Send + 'static,
    E: Display,
{
    let site = Location::caller();
    let (state, generation, stale) = (signal(Load::<T>::Loading), signal(0u64), signal(None));
    let fetch = Arc::new(fetch);
    let start: Rc<dyn Fn()> = Rc::new(move || {
        let fetch = fetch.clone();
        load(site, state, generation, stale, move || {
            fetch().map_err(|error| error.to_string())
        });
    });
    start();
    Resource {
        state,
        start: signal(start),
        stale,
    }
}

/// A [`resource`] that fetches for a key: `key` reads signals (the
/// hovered file, the pane's width), and whenever what it returns changes,
/// `fetch` runs for the new key on its own thread. A result for a key
/// already left behind is dropped. With
/// [`stale_after`](Resource::stale_after), the last key's value stays on
/// show while the next one loads, for a while at most. The first fetch
/// starts on the app's first turn; [`reload`](Resource::reload) fetches
/// the current key again.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::resource_for;
///
/// let app = App::new(|| {
///     let id = signal(1);
///     let user = resource_for(move || id.get(), |id| Ok::<_, String>(format!("user {id}")));
///     text(move || match user.get() {
///         Load::Ready(name) => name,
///         _ => "…".into(),
///     })
///     .on_key("n", move |_| id.update(|i| *i += 1))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(10, 1);
/// driver.press("n");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "user 2");
/// ```
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn resource_for<K, T, E>(
    key: impl Fn() -> K + 'static,
    fetch: impl Fn(K) -> Result<T, E> + Send + Sync + 'static,
) -> Resource<T>
where
    K: PartialEq + Clone + Send + 'static,
    T: Send + 'static,
    E: Display,
{
    let site = Location::caller();
    let (state, generation, stale) = (signal(Load::<T>::Loading), signal(0u64), signal(None));
    let current: Signal<Option<K>> = signal(None);
    let fetch = Arc::new(fetch);
    let start: Rc<dyn Fn()> = Rc::new(move || {
        let Some(key) = current.get_untracked() else {
            return;
        };
        let fetch = fetch.clone();
        load(site, state, generation, stale, move || {
            fetch(key).map_err(|error| error.to_string())
        });
    });
    let start = signal(start);
    watch(key, move |key, _| {
        current.replace(Some(key));
        start.with_untracked(Rc::clone)();
    });
    Resource {
        state,
        start,
        stale,
    }
}

/// Start a fetch of `state` with `work`: a result for an earlier one is
/// dropped from now on. The state goes back to loading at once, or after
/// `stale`'s wait if the fetch is still under way then.
fn load<T: Send + 'static>(
    site: &'static Location<'static>,
    state: Signal<Load<T>>,
    generation: Signal<u64>,
    stale: Signal<Option<Duration>>,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) {
    generation.update(|g| *g += 1);
    let mine = generation.get_untracked();
    let arrived = Arc::new(AtomicBool::new(false));
    match stale.get_untracked() {
        Some(wait) if !state.with_untracked(Load::is_loading) => {
            let arrived = arrived.clone();
            // On the app's clock, so a test steps through the wait.
            crate::app::after(wait, move |_| {
                // Still waiting for this fetch: the old value goes.
                if generation.get_untracked() == mine && !arrived.load(Ordering::SeqCst) {
                    state.update(|s| *s = Load::Loading);
                }
            });
        }
        _ => state.update(|s| *s = Load::Loading),
    }
    spawn_at(site, work, move |result, _| {
        // A later fetch started: this result is stale.
        if generation.get_untracked() == mine {
            arrived.store(true, Ordering::SeqCst);
            state.update(|s| {
                *s = match result {
                    Ok(value) => Load::Ready(value),
                    Err(error) => Load::Failed(error),
                }
            });
        }
    });
}

/// The sending end of a [`latest`] signal, for its thread.
pub struct Latest<T> {
    /// The newest value not yet delivered.
    slot: Arc<std::sync::Mutex<Option<T>>>,
    /// A delivery is on its way to the app's thread; it takes the newest.
    queued: Arc<AtomicBool>,
    /// The app has gone: a delivery was dropped unrun.
    closed: Arc<AtomicBool>,
    proxy: crate::reactive::Proxy,
    signal: Signal<Option<T>>,
    /// The clock [`sleep`](Latest::sleep) keeps to, and the time of the
    /// last beat it slept to.
    clock: Arc<ThreadClock>,
    due: Cell<Option<Duration>>,
}

/// Marks the app gone if the delivery it rides with is dropped unrun.
struct Delivery {
    /// `None` once it ran.
    closed: Option<Arc<AtomicBool>>,
}

impl Delivery {
    fn ran(&mut self) {
        self.closed = None;
    }
}

impl Drop for Delivery {
    fn drop(&mut self) {
        if let Some(closed) = &self.closed {
            closed.store(true, Ordering::SeqCst);
        }
    }
}

impl<T: Send + 'static> Latest<T> {
    /// Make `value` the signal's value: on the app's thread, between
    /// frames, unless a newer one is sent before it gets there (nothing
    /// queues: the newest wins). Returns `false` once the app has finished,
    /// so the thread knows to stop.
    pub fn send(&self, value: T) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
        if self.queued.swap(true, Ordering::SeqCst) {
            // The delivery on its way takes this value.
            return true;
        }
        let (slot, queued, signal) = (self.slot.clone(), self.queued.clone(), self.signal);
        let mut delivery = Delivery {
            closed: Some(self.closed.clone()),
        };
        let sent = self.proxy.send(Box::new(move |_: &mut Ctx| {
            delivery.ran();
            queued.store(false, Ordering::SeqCst);
            let value = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(value) = value {
                signal.update(|slot| *slot = Some(value));
            }
        }));
        if !sent {
            self.closed.store(true, Ordering::SeqCst);
        }
        sent
    }

    /// Wait until `period` after the last beat (after now, the first
    /// time): a thread that makes a value every `period` keeps to its beat
    /// however long each takes to make. One that fell behind does not
    /// hurry through the beats it missed: as a timer made with
    /// [`every`](crate::every), it goes on at once and then keeps to its
    /// step. It waits on the wall clock; in an app
    /// that [waits for its tasks](crate::App::wait_for_tasks), as a test
    /// does, on the app's clock instead, so a headless script's
    /// `wait(..)` moves the thread on as far as it moves the app. Returns
    /// `false` once the app has finished, as [`send`](Self::send) does.
    ///
    /// ```
    /// use intuituive::interact::headless::{Headless, Script};
    /// use intuituive::prelude::*;
    /// use std::time::Duration;
    ///
    /// let app = App::new(|| {
    ///     let tick = latest(|tx| {
    ///         let mut n = 0;
    ///         while tx.send(n) && tx.sleep(Duration::from_millis(100)) {
    ///             n += 1;
    ///         }
    ///     });
    ///     text!("{}", tick.get().unwrap_or(0))
    /// })
    /// .wait_for_tasks(true);
    /// let script = Script::new().wait(Duration::from_millis(350)).keys("ctrl+c");
    /// let mut backend = Headless::new(script, 10, 1);
    /// let record = backend.record();
    /// app.run_on(&mut backend).unwrap();
    /// // Exactly: the thread kept to the app's clock.
    /// assert_eq!(record.borrow().last_frame().trim_end(), "3");
    /// ```
    pub fn sleep(&self, period: Duration) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }
        let now = self.clock.now();
        // The shortest beat is a millisecond, as a timer's.
        let period = period.max(Duration::from_millis(1));
        let next = self.due.get().unwrap_or(now) + period;
        let due = if next >= now {
            next
        } else {
            let behind = (now - next).as_nanos() % period.as_nanos();
            now + Duration::from_nanos((period.as_nanos() - behind) as u64)
        };
        self.due.set(Some(due));
        self.clock.sleep_until(due) && !self.closed.load(Ordering::SeqCst)
    }

    /// Wait until `ready` is true, or `timeout` has passed (`None`: no
    /// limit), looking again each time a [`Notifier`] from
    /// [`notifier`](Self::notifier) is notified: a device's buffer filled,
    /// a message in a channel. Whether `ready` was true; `false` on the
    /// timeout, or once the app has finished.
    ///
    /// Waiting this way, the thread counts as asleep for an app that
    /// [waits for its tasks](crate::App::wait_for_tasks), as a test does:
    /// its driver does not hold each turn for it (as it would for a
    /// thread blocked on its own), and the timeout is on the app's clock.
    /// Once notified it counts as running until it sleeps or waits again,
    /// so what it sends then is in the same turn on every run.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use std::sync::atomic::{AtomicU32, Ordering};
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// let filled = Arc::new(AtomicU32::new(0));
    /// let device = filled.clone();
    /// let app = App::new(move || {
    ///     let filled = filled.clone();
    ///     let level = latest(move |tx| {
    ///         let notifier = tx.notifier();
    ///         // A device's own thread fills a buffer every 50 ms.
    ///         std::thread::spawn(move || {
    ///             for _ in 0..3 {
    ///                 std::thread::sleep(Duration::from_millis(50));
    ///                 device.fetch_add(1, Ordering::SeqCst);
    ///                 notifier.notify();
    ///             }
    ///         });
    ///         let mut seen = 0;
    ///         while seen < 3 {
    ///             if !tx.wait_for(|| filled.load(Ordering::SeqCst) > seen, None) {
    ///                 return;
    ///             }
    ///             seen = filled.load(Ordering::SeqCst);
    ///             tx.send(seen);
    ///         }
    ///     });
    ///     text!("{}", level.get().unwrap_or(0))
    /// })
    /// .wait_for_tasks(true);
    /// let mut driver = app.driver(5, 1);
    /// let start = std::time::Instant::now();
    /// while driver.screen().plain()[0].trim_end() != "3" {
    ///     driver.advance(Duration::from_millis(10));
    /// }
    /// // No turn waited the five seconds a busy thread would hold it.
    /// assert!(start.elapsed() < Duration::from_secs(4));
    /// ```
    pub fn wait_for(
        &self,
        mut ready: impl FnMut() -> bool,
        timeout: impl Into<Option<Duration>>,
    ) -> bool {
        let timeout = timeout.into();
        let due = timeout.map(|t| self.clock.now() + t);
        self.clock.wait_for(&mut ready, due) && !self.closed.load(Ordering::SeqCst)
    }

    /// A handle any thread can [`notify`](Notifier::notify) a
    /// [`wait_for`](Self::wait_for) with: `Clone` and `Send`, for the
    /// device callback or the channel's sender.
    pub fn notifier(&self) -> Notifier {
        Notifier {
            clock: self.clock.clone(),
        }
    }

    /// Run `work`, which blocks on something else (a channel's `recv`, a
    /// read from a pipe), with the thread counted as asleep for an app on
    /// its own clock, as [`wait_for`](Self::wait_for) is: the driver's
    /// turns do not wait for it. What it sends after is picked up when it
    /// comes, not in a set turn; to keep a test exact, wait with
    /// [`wait_for`](Self::wait_for) and a [`Notifier`] instead.
    pub fn blocking<R>(&self, work: impl FnOnce() -> R) -> R {
        self.clock.running(false);
        let out = work();
        self.clock.running(true);
        out
    }
}

/// Wakes a [`Latest::wait_for`] to look again: made by
/// [`Latest::notifier`], sent to whatever thread makes the condition
/// true.
#[derive(Clone)]
pub struct Notifier {
    clock: Arc<ThreadClock>,
}

impl Notifier {
    /// Wake every thread of the app waiting in
    /// [`wait_for`](Latest::wait_for), to look at its condition again.
    pub fn notify(&self) {
        self.clock.notify();
    }
}

/// The clock [`latest`] threads keep to with [`Latest::sleep`]: the wall
/// clock, or, in an app that waits for its tasks, the app's own, which its
/// driver moves on. Then the driver also waits, each turn, until every
/// such thread is asleep for a later time or finished, so what they send
/// is the same on every run.
pub(crate) struct ThreadClock {
    state: Mutex<ClockState>,
    changed: Condvar,
    /// Where the wall clock's time starts.
    start: WallStart,
}

/// `Instant::now()` at the clock's start. On `wasm32-unknown-unknown` std
/// has no clock (`Instant::now` panics) and no threads to keep to it, so
/// the wall's time there stays at zero.
#[cfg(not(target_arch = "wasm32"))]
struct WallStart(std::time::Instant);
#[cfg(target_arch = "wasm32")]
struct WallStart;

impl WallStart {
    #[cfg(not(target_arch = "wasm32"))]
    fn now() -> WallStart {
        WallStart(std::time::Instant::now())
    }
    #[cfg(target_arch = "wasm32")]
    fn now() -> WallStart {
        WallStart
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
    #[cfg(target_arch = "wasm32")]
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
}

#[derive(Default)]
struct ClockState {
    /// Threads keep to the app's clock, not the wall's.
    manual: bool,
    /// The app's time, as its driver last set it.
    now: Duration,
    /// Threads started and neither asleep on the app's clock nor finished.
    running: usize,
    /// Threads asleep on the app's clock, by ticket, with when each wakes.
    asleep: HashMap<u64, Duration>,
    /// Threads in [`Latest::wait_for`], by ticket (with a timeout, also
    /// in `asleep`), until notified or due.
    waiting: std::collections::HashSet<u64>,
    /// Bumped by each notify: a waiter that saw it change looks again.
    notified: u64,
    next: u64,
    /// The app has finished: sleepers wake, and stop.
    closed: bool,
}

impl ThreadClock {
    pub(crate) fn new() -> Arc<ThreadClock> {
        Arc::new(ThreadClock {
            state: Mutex::new(ClockState::default()),
            changed: Condvar::new(),
            start: WallStart::now(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, ClockState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keep threads to the app's clock from now on (`manual`), or to the
    /// wall's.
    pub(crate) fn set_manual(&self, manual: bool) {
        self.lock().manual = manual;
    }

    fn now(&self) -> Duration {
        let state = self.lock();
        if state.manual {
            state.now
        } else {
            self.start.elapsed()
        }
    }

    /// A thread starts (or ends, if not `started`).
    fn running(&self, started: bool) {
        let mut state = self.lock();
        if started {
            state.running += 1;
        } else {
            state.running = state.running.saturating_sub(1);
            self.changed.notify_all();
        }
    }

    /// Sleep until `due`; `false` once the app has finished.
    fn sleep_until(&self, due: Duration) -> bool {
        let mut state = self.lock();
        if !state.manual {
            drop(state);
            let wait = due.saturating_sub(self.start.elapsed());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
            return !self.lock().closed;
        }
        if due <= state.now || state.closed {
            return !state.closed;
        }
        let ticket = state.next;
        state.next += 1;
        state.asleep.insert(ticket, due);
        state.running = state.running.saturating_sub(1);
        self.changed.notify_all();
        // Woken by `advance`, which counts it running again, or the end.
        while state.asleep.contains_key(&ticket) && !state.closed {
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        !state.closed
    }

    /// Wait until `ready`, or the app's time `due`, waking to look
    /// again on each [`notify`](Self::notify); whether `ready` was true.
    /// The thread counts as asleep meanwhile.
    fn wait_for(&self, ready: &mut dyn FnMut() -> bool, due: Option<Duration>) -> bool {
        loop {
            let seen = self.lock().notified;
            if ready() {
                return true;
            }
            let mut state = self.lock();
            if state.closed {
                return false;
            }
            if state.notified != seen {
                // Notified while looking: look again.
                continue;
            }
            if !state.manual {
                // The wall clock: a wait on the notifications, cut at `due`.
                let left = due.map(|due| due.saturating_sub(self.start.elapsed()));
                if left.is_some_and(|left| left.is_zero()) {
                    return false;
                }
                let wait = left.unwrap_or(Duration::from_secs(3600));
                drop(
                    self.changed
                        .wait_timeout_while(state, wait, |s| s.notified == seen && !s.closed),
                );
                continue;
            }
            if due.is_some_and(|due| due <= state.now) {
                return false;
            }
            let ticket = state.next;
            state.next += 1;
            state.waiting.insert(ticket);
            if let Some(due) = due {
                state.asleep.insert(ticket, due);
            }
            state.running = state.running.saturating_sub(1);
            self.changed.notify_all();
            // Woken by a notify or `advance`, which count it running again,
            // or the end.
            while state.waiting.contains(&ticket) && !state.closed {
                state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    /// Wake every [`wait_for`](Self::wait_for) to look again.
    fn notify(&self) {
        let mut state = self.lock();
        state.notified += 1;
        let woken: Vec<u64> = state.waiting.drain().collect();
        for ticket in &woken {
            state.asleep.remove(ticket);
        }
        state.running += woken.len();
        self.changed.notify_all();
    }

    /// The app's clock is at `now`: wake the threads that are due.
    pub(crate) fn advance(&self, now: Duration) {
        let mut state = self.lock();
        state.now = now;
        let due: Vec<u64> = state
            .asleep
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(ticket, _)| *ticket)
            .collect();
        for ticket in &due {
            state.asleep.remove(ticket);
            state.waiting.remove(ticket);
        }
        state.running += due.len();
        self.changed.notify_all();
    }

    /// Wait (a while at most) until every thread is asleep on the app's
    /// clock or finished. Only on the app's clock.
    pub(crate) fn settle(&self) {
        let deadline = crate::clock::Instant::now() + THREAD_SETTLE;
        let mut state = self.lock();
        while state.manual && state.running > 0 && !state.closed {
            let left = deadline.saturating_duration_since(crate::clock::Instant::now());
            if left.is_zero() {
                return;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// When the next thread asleep on the app's clock wakes; `None` on the
    /// wall clock, or with none asleep.
    pub(crate) fn next_due(&self) -> Option<Duration> {
        let state = self.lock();
        if !state.manual {
            return None;
        }
        state.asleep.values().min().copied()
    }

    /// The app has finished.
    pub(crate) fn close(&self) {
        self.lock().closed = true;
        self.changed.notify_all();
    }
}

/// How long a turn of an app on its own clock waits, at most, for its
/// `latest` threads to go to sleep: one that waits on something else (a
/// pipe, a device) holds each turn up this long.
const THREAD_SETTLE: Duration = Duration::from_secs(5);

/// A signal holding the newest value a thread of its own produces: `run`
/// gets the [`Latest`] to send them with, and runs until it returns.
/// `None` until the first value arrives. Values are not queued: when the
/// app is busy, the ones it had no time for are skipped, and the next
/// frame shows the newest (an audio buffer, a sensor, a log's tail). Each
/// delivery wakes the app's loop, which picks it up on its next turn.
/// `send` returns `false` once the app has finished; stop then. The thread
/// is not a [task](spawn): [`App::wait_for_tasks`](crate::App::wait_for_tasks)
/// does not wait for it; a thread that keeps to a beat with
/// [`Latest::sleep`] keeps to the app's clock under it. The thread starts
/// when the app starts running (or at once, when the app already is).
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let reading = latest(|tx| {
///         for sample in 0..=1000 {
///             if !tx.send(sample) {
///                 return;
///             }
///         }
///     });
///     text(move || match reading.get() {
///         Some(n) => format!("last {n}"),
///         None => "waiting".into(),
///     })
/// });
/// let mut driver = app.driver(12, 1);
/// while driver.screen().plain()[0].trim_end() != "last 1000" {
///     std::thread::sleep(std::time::Duration::from_millis(1));
///     driver.settle();
/// }
/// ```
///
/// # In a browser
/// On `wasm32-unknown-unknown` there are no threads: `run` never starts
/// and the signal stays `None`. [`latest_every`] works there.
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn latest<T: Send + 'static>(
    run: impl FnOnce(Latest<T>) + Send + 'static,
) -> Signal<Option<T>> {
    let runtime = Runtime::current_for("latest()");
    let value = signal(None);
    let clock = runtime.clock.clone();
    let sender = Latest {
        slot: Arc::new(std::sync::Mutex::new(None)),
        queued: Arc::new(AtomicBool::new(false)),
        closed: Arc::new(AtomicBool::new(false)),
        proxy: runtime.proxy(),
        signal: value,
        clock: clock.clone(),
        due: Cell::new(None),
    };
    // No threads on wasm32 (a browser tab): `run` never starts, and the
    // signal stays `None`. `latest_every` keeps to the app's timers there.
    #[cfg(target_arch = "wasm32")]
    let _ = (clock, sender, run);
    #[cfg(not(target_arch = "wasm32"))]
    runtime.start_thread(Box::new(move || {
        clock.running(true);
        std::thread::spawn(move || {
            // Counted finished however `run` ends, a panic included.
            struct Finished(Arc<ThreadClock>);
            impl Drop for Finished {
                fn drop(&mut self) {
                    self.0.running(false);
                }
            }
            let _finished = Finished(clock);
            run(sender)
        });
    }));
    value
}

/// A [`latest`] signal fed by `next`, called on a thread of its own once at
/// the start and then every `period`, keeping to the beat as
/// [`Latest::sleep`] does: on the app's clock in an app that
/// [waits for its tasks](crate::App::wait_for_tasks), so a test sees the
/// same values at the same times on every run; on the wall clock
/// otherwise. A generator, a sensor polled, a demo's signal.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::latest_every;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let mut n = 0;
///     let count = latest_every(Duration::from_millis(10), move || {
///         n += 1;
///         n
///     });
///     text!("{}", count.get().unwrap_or(0))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(10, 1);
/// driver.advance(Duration::from_millis(45));
/// // Made at 0, 10, 20, 30 and 40 ms: each beat is a step of the driver.
/// assert_eq!(driver.screen().plain()[0].trim_end(), "5");
/// ```
///
/// Each value is delivered: [`Driver::advance`](crate::Driver::advance)
/// and a headless script's `wait` step to each beat, and in a terminal a
/// value wakes the loop at once, so a [`rate`](crate::rate) counts every
/// one (100 a second at 10 ms), however fast the beat.
///
/// # In a browser
/// On `wasm32-unknown-unknown` there are no threads: `next` runs on the
/// app's thread, once at the start and then on an [`every`](crate::every)
/// timer.
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn latest_every<T: Send + 'static>(
    period: Duration,
    mut next: impl FnMut() -> T + Send + 'static,
) -> Signal<Option<T>> {
    #[cfg(not(target_arch = "wasm32"))]
    return latest(move |tx| while tx.send(next()) && tx.sleep(period) {});
    // No threads on wasm32 (a browser tab): `next` runs on the app's thread,
    // at the start and on an [`every`](crate::every) timer.
    #[cfg(target_arch = "wasm32")]
    {
        let value = signal(Some(next()));
        crate::every(period.max(Duration::from_millis(1)), move |_| {
            value.replace(Some(next()));
        });
        value
    }
}
