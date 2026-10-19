//! A capture as the app sees it: the newest buffer and how the feed is
//! doing, on the core's `latest_with_state`, whether a sound card fills
//! it (`audio`) or a simulation does (any build with `pcm`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use super::{Frame, Matrix};
use crate::feed::{latest_with_state, FeedState};
#[cfg(feature = "audio")]
use crate::log::Log;
use crate::reactive::{Proxy, Runtime, Signal};
use crate::task::Latest;

/// How long a sound card's feed thread waits for a buffer before it looks
/// at whether the capture has stopped.
const POLL: Duration = Duration::from_millis(100);

/// A running capture, from [`open`](super::open) (a sound card, with the
/// `audio` feature) or [`simulated`](Capture::simulated). Clones are the
/// same capture. It captures until [`stop`](Self::stop) is called or,
/// once its signals are in an app, until the app ends; a capture never put
/// in an app stops when its last clone is dropped.
///
/// Its signals come from the core's
/// [`latest_with_state`](crate::latest_with_state):
/// [`samples`](Self::samples) is the newest [`Frame`] and
/// [`status`](Self::status) a [`FeedState`]: `Running`, `Stalled` once no
/// buffer has come for the capture's stall time (timed on the app's
/// clock), `Failed` when the stream broke, `Ended` once stopped.
#[derive(Clone)]
pub struct Capture {
    pub(super) shared: Arc<Shared>,
    _handle: Arc<Handle>,
}

impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture")
            .field("device", &self.shared.device)
            .field("failed", &*lock(&self.shared.failed))
            .finish()
    }
}

/// Dropped with the last clone of a [`Capture`].
struct Handle(Arc<Shared>);

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.dropped.store(true, Ordering::SeqCst);
    }
}

/// What the capture's threads and the app share.
pub(super) struct Shared {
    /// The device opened, by name.
    pub(super) device: String,
    /// The newest frame not yet taken by the feed.
    latest: Mutex<Option<Frame>>,
    /// A frame came, the stream failed, or the capture stopped.
    ready: Condvar,
    /// Buffers delivered.
    count: AtomicU64,
    /// Why the stream broke, once it has.
    failed: Mutex<Option<String>>,
    /// How long without a buffer before the capture is stalled.
    stall: Mutex<Duration>,
    /// What a simulated capture plays, until its signals are taken.
    simulation: Mutex<Option<Simulation>>,
    /// Buffers are held back ([`Capture::stall`]).
    quiet: AtomicBool,
    /// Where the samples and status go, once in an app.
    sink: Mutex<Option<Sink>>,
    /// Where ALSA's messages go, once given.
    #[cfg(feature = "audio")]
    pub(super) log: Mutex<Option<Log>>,
    pub(super) stop: AtomicBool,
    pub(super) dropped: AtomicBool,
}

/// A simulated capture's buffers and beat.
struct Simulation {
    frames: Box<dyn Iterator<Item = Matrix> + Send>,
    period: Duration,
}

/// The app side of a capture.
#[derive(Clone)]
pub(super) struct Sink {
    /// A device's thread knows by it when the app has gone.
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    pub(super) proxy: Proxy,
    samples: Signal<Option<Frame>>,
    status: Signal<FeedState>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    pub(super) fn new(device: String, stall: Duration) -> Shared {
        Shared {
            device,
            latest: Mutex::new(None),
            ready: Condvar::new(),
            count: AtomicU64::new(0),
            failed: Mutex::new(None),
            stall: Mutex::new(stall),
            simulation: Mutex::new(None),
            quiet: AtomicBool::new(false),
            sink: Mutex::new(None),
            #[cfg(feature = "audio")]
            log: Mutex::new(None),
            stop: AtomicBool::new(false),
            dropped: AtomicBool::new(false),
        }
    }

    pub(super) fn sink(&self) -> Option<Sink> {
        lock(&self.sink).clone()
    }

    /// The next frame's number.
    fn number(&self) -> u64 {
        self.count.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// A full buffer from the device: the newest, numbered, waits for the
    /// feed (an older one waiting is dropped).
    #[cfg_attr(not(feature = "audio"), allow(dead_code))]
    pub(super) fn deliver(&self, samples: Matrix) {
        let number = self.number();
        *lock(&self.latest) = Some(Frame { number, samples });
        self.ready.notify_all();
    }

    /// The stream broke: the first reason is kept.
    pub(super) fn fail(&self, why: String) {
        lock(&self.failed).get_or_insert(why);
        self.ready.notify_all();
    }

    /// Stop the feed.
    pub(super) fn halt(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.ready.notify_all();
    }

    /// Stopped by [`Capture::stop`] or the device's thread. In an app, a
    /// capture runs on with its clones dropped, until the app ends.
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// The feed of a sound card's capture: each buffer as it comes, until
    /// the stream fails or the capture stops.
    fn feed_device(&self, tx: &Latest<Frame>) -> Result<(), String> {
        loop {
            let mut latest = lock(&self.latest);
            while latest.is_none() && lock(&self.failed).is_none() && !self.stopped() {
                latest = self
                    .ready
                    .wait_timeout(latest, POLL)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            let frame = latest.take();
            drop(latest);
            if let Some(frame) = frame {
                if !self.quiet.load(Ordering::SeqCst) && !tx.send(frame) {
                    return Ok(());
                }
            }
            if let Some(why) = lock(&self.failed).clone() {
                return Err(why);
            }
            if self.stopped() {
                return Ok(());
            }
        }
    }

    /// The feed of a simulated capture: its buffers a period apart, then
    /// none (a device gone quiet), until it fails or stops.
    fn feed_simulation(&self, simulation: Simulation, tx: &Latest<Frame>) -> Result<(), String> {
        let Simulation { mut frames, period } = simulation;
        let mut first = true;
        loop {
            if !first && !tx.sleep(period) {
                return Ok(());
            }
            first = false;
            if let Some(why) = lock(&self.failed).clone() {
                return Err(why);
            }
            if self.stopped() {
                return Ok(());
            }
            if self.quiet.load(Ordering::SeqCst) {
                continue;
            }
            if let Some(samples) = frames.next() {
                let number = self.number();
                if !tx.send(Frame { number, samples }) {
                    return Ok(());
                }
            }
        }
    }
}

impl Capture {
    pub(super) fn from_shared(shared: Arc<Shared>) -> Capture {
        Capture {
            _handle: Arc::new(Handle(shared.clone())),
            shared,
        }
    }

    /// A capture with no sound card: `frames` played a `period` apart (the
    /// first at once), then nothing, as a device that has gone quiet. For
    /// tests and demos of what an app does with a capture:
    /// [`stall`](Self::stall) holds the buffers back, so the status turns
    /// `Stalled` after [`stall_after`](Self::stall_after) (a second by
    /// default), and [`fail`](Self::fail) breaks it. In an app that
    /// [waits for its tasks](crate::App::wait_for_tasks) it keeps to the
    /// app's clock, so a test sees the same frames at the same times on
    /// every run.
    ///
    /// ```
    /// use intuituive::audio::Capture;
    /// use intuituive::prelude::*;
    /// use intuituive::FeedState;
    /// use std::time::Duration;
    ///
    /// let capture = Capture::simulated(
    ///     std::iter::repeat(vec![vec![0.0; 4]]),
    ///     Duration::from_millis(100),
    /// )
    /// .stall_after(Duration::from_millis(500));
    /// let control = capture.clone();
    /// let app = App::new(move || {
    ///     let (samples, status) = (capture.samples(), capture.status());
    ///     text(move || {
    ///         let number = samples.with(|f| f.as_ref().map_or(0, |f| f.number));
    ///         format!("{number} {:?}", status.get())
    ///     })
    /// })
    /// .wait_for_tasks(true);
    /// let mut driver = app.driver(30, 1);
    /// driver.advance(Duration::from_millis(250));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "3 Running");
    /// control.stall();
    /// driver.advance(Duration::from_millis(600));
    /// assert!(driver.screen().plain()[0].starts_with("3 Stalled"));
    /// control.fail("unplugged");
    /// driver.advance(Duration::from_millis(100));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "3 Failed(\"unplugged\")");
    /// ```
    pub fn simulated<I>(frames: I, period: Duration) -> Capture
    where
        I: IntoIterator<Item = Matrix>,
        I::IntoIter: Send + 'static,
    {
        let shared = Shared::new("simulated".to_string(), Duration::from_secs(1));
        *lock(&shared.simulation) = Some(Simulation {
            frames: Box::new(frames.into_iter()),
            period,
        });
        Capture::from_shared(Arc::new(shared))
    }

    /// How long without a buffer before the [`status`](Self::status) is
    /// `Stalled`, for a capture whose signals are not yet taken (a device's
    /// is set by [`AudioOptions::stall_after`](super::AudioOptions::stall_after)
    /// when the `audio` feature is on). At least a millisecond.
    pub fn stall_after(self, quiet: Duration) -> Capture {
        *lock(&self.shared.stall) = quiet.max(Duration::from_millis(1));
        self
    }

    /// Hold the buffers back from now on, as a device that stops sending
    /// would: the signal keeps the last, and the status turns `Stalled`
    /// after the stall time. [`resume`](Self::resume) lets them through
    /// again. For tests, mostly; a simulated capture's buffers wait, a
    /// sound card's are dropped.
    pub fn stall(&self) {
        self.shared.quiet.store(true, Ordering::SeqCst);
    }

    /// Let the buffers through again after a [`stall`](Self::stall): the
    /// next one makes the status `Running`.
    pub fn resume(&self) {
        self.shared.quiet.store(false, Ordering::SeqCst);
        self.shared.ready.notify_all();
    }

    /// Break the capture, as an unplugged device does: no more buffers, and
    /// the status `Failed(why)` (the first reason given is kept).
    pub fn fail(&self, why: impl Into<String>) {
        self.shared.fail(why.into());
    }

    /// The newest buffer, as a signal of this app: `None` until the first,
    /// then each [`Frame`] as it comes (see the [module docs](super)). The
    /// first call puts the capture in the app (it then runs until the app
    /// ends); later calls return the same signal.
    ///
    /// # Panics
    /// Outside a running or building app.
    #[track_caller]
    pub fn samples(&self) -> Signal<Option<Frame>> {
        self.attach("Capture::samples()").samples
    }

    /// How the capture is going, as a signal of this app ([`FeedState`]):
    /// watch it to end the app on a stall (`--timeout`) or a failure.
    ///
    /// ```no_run
    /// # #[cfg(feature = "audio")] {
    /// use std::time::Duration;
    /// use intuituive::audio::{self, AudioOptions};
    /// use intuituive::prelude::*;
    /// use intuituive::FeedState;
    ///
    /// let capture = audio::open(AudioOptions::new().stall_after(Duration::from_secs(5)))?;
    /// App::new(move || {
    ///     let samples = capture.samples();
    ///     let status = capture.status();
    ///     watch(move || status.get(), |status, cx| match status {
    ///         FeedState::Stalled(_) => cx.fail("no audio for 5 s"),
    ///         FeedState::Failed(why) => cx.fail(why),
    ///         FeedState::Running | FeedState::Ended => {}
    ///     });
    ///     text(move || format!("{:?}", samples.with(|f| f.as_ref().map(|f| f.number))))
    /// })
    /// .run()?;
    /// # }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Panics
    /// Outside a running or building app.
    #[track_caller]
    pub fn status(&self) -> Signal<FeedState> {
        self.attach("Capture::status()").status
    }

    /// Send ALSA's messages to `log` from now on, the ones already kept
    /// first (see [`messages`](super::messages)).
    #[cfg(feature = "audio")]
    pub fn log(&self, log: Log) {
        *lock(&self.shared.log) = Some(log);
    }

    /// Stop capturing. The samples keep the last buffer; the status is
    /// `Ended`.
    pub fn stop(&self) {
        self.shared.halt();
    }

    /// The device opened, by name (`simulated` for a
    /// [simulated](Self::simulated) capture).
    pub fn device(&self) -> &str {
        &self.shared.device
    }

    #[track_caller]
    fn attach(&self, what: &str) -> Sink {
        if let Some(sink) = self.shared.sink() {
            return sink;
        }
        let runtime = Runtime::current_for(what);
        let shared = self.shared.clone();
        let simulation = lock(&shared.simulation).take();
        let stall = *lock(&shared.stall);
        let (samples, status) = latest_with_state(stall, move |tx| match simulation {
            Some(simulation) => shared.feed_simulation(simulation, &tx),
            None => shared.feed_device(&tx),
        });
        let sink = Sink {
            proxy: runtime.proxy(),
            samples,
            status,
        };
        *lock(&self.shared.sink) = Some(sink.clone());
        sink
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shows(capture: &Capture) -> crate::Node {
        let (samples, status) = (capture.samples(), capture.status());
        crate::text(move || {
            let number = samples.with(|f| f.as_ref().map_or(0, |f| f.number));
            format!("{number} {:?}", status.get())
        })
    }

    #[test]
    fn frames_are_numbered_and_the_newest_waits_for_the_app() {
        let shared = Arc::new(Shared::new("test".into(), Duration::from_secs(60)));
        shared.deliver(vec![vec![0.0]]);
        shared.deliver(vec![vec![0.0]]);
        // Not in an app yet: the newest waits, numbered.
        let latest = lock(&shared.latest).clone();
        assert_eq!(latest.map(|f| f.number), Some(2));
        let capture = Capture::from_shared(shared);
        let app = crate::App::new(move || {
            let node = shows(&capture);
            // Two equal buffers are two frames.
            capture.shared.deliver(vec![vec![0.0]]);
            node
        });
        let mut driver = app.driver(20, 1);
        let mut shown = String::new();
        for _ in 0..200 {
            driver.settle();
            shown = driver.screen().plain()[0].trim_end().to_string();
            if shown == "3 Running" {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(shown, "3 Running");
    }

    #[test]
    fn a_device_feed_fails_and_ends() {
        let failing =
            Capture::from_shared(Arc::new(Shared::new("t".into(), Duration::from_secs(60))));
        let stopping =
            Capture::from_shared(Arc::new(Shared::new("t".into(), Duration::from_secs(60))));
        let (f, s) = (failing.clone(), stopping.clone());
        let app = crate::App::new(move || crate::column([shows(&f), shows(&s)]));
        let mut driver = app.driver(30, 2);
        driver.settle();
        failing.shared.deliver(vec![vec![0.5]]);
        failing.fail("unplugged");
        failing.fail("again");
        stopping.stop();
        let mut rows = Vec::new();
        for _ in 0..200 {
            driver.settle();
            rows = driver.screen().plain();
            if !rows[1].contains("Running") && !rows[0].contains("Running") {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // The last buffer before the failure is kept.
        assert_eq!(rows[0].trim_end(), "1 Failed(\"unplugged\")");
        assert_eq!(rows[1].trim_end(), "0 Ended");
    }

    #[test]
    fn a_simulation_stalls_resumes_and_ends() {
        let capture = Capture::simulated(
            (0..).map(|i| vec![vec![f64::from(i)]]),
            Duration::from_millis(10),
        )
        .stall_after(Duration::from_millis(50));
        let control = capture.clone();
        let app = crate::App::new(move || shows(&capture)).wait_for_tasks(true);
        let mut driver = app.driver(30, 1);
        let row = |driver: &crate::Driver| driver.screen().plain()[0].trim_end().to_string();
        driver.advance(Duration::from_millis(35));
        assert_eq!(row(&driver), "4 Running");
        control.stall();
        driver.advance(Duration::from_millis(100));
        assert!(row(&driver).starts_with("4 Stalled"), "{}", row(&driver));
        control.resume();
        driver.advance(Duration::from_millis(10));
        assert_eq!(row(&driver), "5 Running");
        control.stop();
        driver.advance(Duration::from_millis(10));
        assert_eq!(row(&driver), "5 Ended");
    }

    #[test]
    fn a_simulation_that_runs_out_goes_quiet() {
        let capture = Capture::simulated(vec![vec![vec![0.0]]; 2], Duration::from_millis(10))
            .stall_after(Duration::from_millis(30));
        let app = crate::App::new(move || shows(&capture)).wait_for_tasks(true);
        let mut driver = app.driver(30, 1);
        driver.advance(Duration::from_millis(100));
        let row = driver.screen().plain()[0].trim_end().to_string();
        // Its last buffer at 10 ms, stalled since then.
        assert_eq!(row, "2 Stalled(10ms)");
    }
}
