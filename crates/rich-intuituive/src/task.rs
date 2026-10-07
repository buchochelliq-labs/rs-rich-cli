//! Background work: run something slow off the app's thread, and write
//! signals with its result when it is done.
//!
//! - [`spawn`] runs a closure on its own thread;
//! - [`spawn_future`] drives a future on its own thread, for async code
//!   that does not need a particular runtime (a tokio app keeps its own
//!   runtime and sends results back with a [`Proxy`](crate::Proxy));
//! - [`resource`] is a value that loads in the background, with
//!   [`Load::Loading`] until it arrives, and reloads on demand.
//!
//! The `done` closure runs back on the app's thread, between frames, with a
//! [`Ctx`]: it can write signals, open a screen, or quit. A cancelled
//! [`Task`] still finishes its work (a thread cannot be stopped from
//! outside), but its result is dropped.

use std::fmt::Display;
use std::future::Future;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use crate::app::Ctx;
use crate::reactive::{signal, Runtime, Signal};

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
/// # Panics
/// Outside a running or building app.
pub fn spawn<T: Send + 'static>(
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
    std::thread::spawn(move || {
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
        proxy.run_with(move |cx| {
            if !cancelled.load(Ordering::SeqCst) {
                done(value, cx);
            }
        });
    });
    task
}

/// Drive `future` to completion on its own thread, then run `done` with its
/// output on the app's thread. The future is polled with a waker that
/// parks and unparks that thread, so it suits futures that do not need a
/// specific async runtime.
pub fn spawn_future<F>(future: F, done: impl FnOnce(F::Output, &mut Ctx) + Send + 'static) -> Task
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    spawn(move || block_on(future), done)
}

/// Poll `future` on this thread until it is ready.
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
    state: Signal<Load<T>>,
    start: Signal<Rc<dyn Fn()>>,
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
pub fn resource<T, E>(fetch: impl Fn() -> Result<T, E> + Send + Sync + 'static) -> Resource<T>
where
    T: Send + 'static,
    E: Display,
{
    let state = signal(Load::<T>::Loading);
    let generation = signal(0u64);
    let fetch = Arc::new(fetch);
    let start: Rc<dyn Fn()> = Rc::new(move || {
        generation.update(|g| *g += 1);
        let mine = generation.get_untracked();
        state.update(|s| *s = Load::Loading);
        let fetch = fetch.clone();
        spawn(
            move || fetch().map_err(|error| error.to_string()),
            move |result, _| {
                // A later fetch started: this result is stale.
                if generation.get_untracked() == mine {
                    state.update(|s| {
                        *s = match result {
                            Ok(value) => Load::Ready(value),
                            Err(error) => Load::Failed(error),
                        }
                    });
                }
            },
        );
    });
    start();
    Resource {
        state,
        start: signal(start),
    }
}
