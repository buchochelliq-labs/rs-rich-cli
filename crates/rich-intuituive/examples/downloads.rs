//! A port of ratatui's `inline` example
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps/inline>),
//! rebuilt on intuiTUIve. It reuses none of ratatui's code: it rebuilds the
//! behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs.
//!
//!     cargo run -p rs-rich-intuituive --example downloads
//!     cargo run -p rs-rich-intuituive --example downloads -- --downloads 30 --seed 7
//!
//! An inline app: eight rows under the prompt, not the whole screen. Four
//! workers download a queue of files (pretend ones: nothing is fetched),
//! each on a thread of its own, and report through a `Proxy`. The region
//! shows the total as a line gauge and each worker's file as a gauge; each
//! finished file is printed above the region, into the scrollback, as
//! "Finished 7 (34 KB) in 1.2s". When all are done the app ends, and its
//! last frame stays in the scrollback. q stops early.
//!
//! `--clock` runs the workers on the app's own timer rather than threads,
//! so the run is the same every time: the tests use it, on a virtual clock.
//!
//! What intuiTUIve adds: `cx.print_above` writes into the scrollback above
//! the region and moves the region down, with no frame of its own to
//! keep; and a worker's report redraws that worker's gauge and the total,
//! nothing else.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use intuituive::prelude::*;
use rich_ext::chart::{FillGauge, LineGauge, LineSet};

/// How many workers download at once.
pub const WORKERS: usize = 4;

/// How many rows the region takes.
pub const ROWS: u16 = 8;

/// A pretend file: its number, its size and how fast it comes, in bytes a
/// tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Download {
    pub id: usize,
    pub size: u64,
    pub speed: u64,
}

/// A worker's file in progress: how much has come, and for how many ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    pub download: Download,
    pub done: u64,
    pub ticks: u32,
}

impl Job {
    fn new(download: Download) -> Job {
        Job {
            download,
            done: 0,
            ticks: 0,
        }
    }

    /// A tick on: another `speed` bytes. Whether the file is complete.
    pub fn step(&mut self) -> bool {
        self.done = (self.done + self.download.speed).min(self.download.size);
        self.ticks += 1;
        self.done == self.download.size
    }

    pub fn ratio(&self) -> f64 {
        self.done as f64 / self.download.size.max(1) as f64
    }
}

/// The line printed above the region when `job` is complete, `tick` long
/// a tick.
pub fn finished_line(job: &Job, tick: Duration) -> String {
    let took = tick * job.ticks;
    format!(
        "Finished [bold]{}[/] ({} KB) in {:.1}s",
        job.download.id,
        job.download.size / 1024,
        took.as_secs_f64()
    )
}

/// A seeded xorshift: the same numbers every run (the crate has no `rand`).
#[derive(Clone, Copy, Debug)]
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> XorShift {
        XorShift(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number in `low..=high`.
    pub fn between(&mut self, low: u64, high: u64) -> u64 {
        low + self.next_u64() % (high - low + 1)
    }
}

/// The files to fetch: `count` of them, 8 to 96 KB, each coming at 2 to 9
/// KB a tick.
pub fn downloads(count: usize, seed: u64) -> Vec<Download> {
    let mut random = XorShift::new(seed);
    (1..=count)
        .map(|id| Download {
            id,
            size: random.between(8, 96) * 1024,
            speed: random.between(2, 9) * 1024,
        })
        .collect()
}

/// How the app runs: its command line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// How many files.
    pub downloads: usize,
    /// The seed of their sizes and speeds.
    pub seed: u64,
    /// How long a tick is: a worker gets a file's `speed` bytes a tick.
    pub tick: Duration,
    /// Run the workers on threads (`true`), or on the app's timer.
    pub threads: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            downloads: 20,
            seed: 0xd0_d0,
            tick: Duration::from_millis(100),
            threads: true,
        }
    }
}

impl Options {
    /// `--downloads N`, `--seed N`, `--tick MS` and `--clock`.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut number = |what: &str| -> Result<u64, String> {
                args.next()
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| format!("{arg} takes {what}"))
            };
            match arg.as_str() {
                "--downloads" => options.downloads = number("a number")?.max(1) as usize,
                "--seed" => options.seed = number("a number")?,
                "--tick" => options.tick = Duration::from_millis(number("milliseconds")?.max(1)),
                "--clock" => options.threads = false,
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(options)
    }
}

/// What the region shows, each in a signal of its own.
#[derive(Clone, Copy)]
struct State {
    /// Each worker's file, while it has one.
    workers: [Signal<Option<Job>>; WORKERS],
    /// Bytes come so far, of all the files.
    received: Signal<u64>,
    /// Files complete.
    finished: Signal<usize>,
}

impl State {
    /// Note a step of worker `w` on its file: the gauges, and when it is
    /// complete, the line above and the count.
    fn report(
        &self,
        cx: &mut Ctx,
        w: usize,
        before: u64,
        job: Job,
        complete: bool,
        tick: Duration,
    ) {
        self.received
            .update(|r| *r += job.done.saturating_sub(before));
        if complete {
            cx.print_above(finished_line(&job, tick));
            self.finished.update(|f| *f += 1);
            self.workers[w].set(None);
        } else {
            self.workers[w].set(Some(job));
        }
    }
}

/// The region: the total, then a gauge for each worker.
fn view(state: State, files: usize, total: u64) -> Node {
    let State {
        workers,
        received,
        finished,
    } = state;
    let heading = text(move || {
        format!(
            " [bold]Downloads[/]  {} of {files} done · {} of {} KB",
            finished.get(),
            received.get() / 1024,
            total / 1024
        )
    })
    .name("heading")
    .fixed(1);
    let gauge = renderable(move || {
        let ratio = received.get() as f64 / total.max(1) as f64;
        LineGauge::new(ratio)
            .label(format!("{:>3.0}%", ratio * 100.0))
            .filled_style("green")
            .line(LineSet::Thick)
    })
    .name("total")
    .label("All the downloads")
    .padding(0, 1)
    .fixed(1);
    let mut rows = vec![heading, gauge, label("").fixed(1)];
    for (w, worker) in workers.into_iter().enumerate() {
        let name = text(move || match worker.get() {
            Some(job) => format!(" worker {} [dim]#{:<3}[/]", w + 1, job.download.id),
            None => format!(" worker {} [dim]idle[/]", w + 1),
        })
        .fixed(15);
        let bar = renderable(move || {
            let ratio = worker.with(|job| job.map_or(0.0, |j| j.ratio()));
            FillGauge::new(ratio)
                .label(format!("{:.0}%", ratio * 100.0))
                .style("cyan on grey15")
        })
        .label(format!("Worker {}", w + 1));
        rows.push(row([name, bar.flex(1)]).name("worker").fixed(1));
    }
    rows.push(label(" [dim]q stops[/]").fixed(1));
    column(rows)
}

/// Run the workers on threads, each taking files from `queue` until it is
/// empty, a tick a step, reporting each step through `proxy`.
fn start_threads(proxy: Proxy, state: State, queue: Vec<Download>, tick: Duration) {
    let queue = Arc::new(Mutex::new(VecDeque::from(queue)));
    for w in 0..WORKERS {
        let (proxy, queue) = (proxy.clone(), queue.clone());
        std::thread::spawn(move || loop {
            let Some(download) = queue.lock().ok().and_then(|mut q| q.pop_front()) else {
                return;
            };
            let mut job = Job::new(download);
            let mut complete = false;
            while !complete {
                std::thread::sleep(tick);
                let before = job.done;
                complete = job.step();
                let report = job;
                let sent =
                    proxy.run_with(move |cx| state.report(cx, w, before, report, complete, tick));
                if !sent {
                    // The app has ended.
                    return;
                }
            }
        });
    }
}

/// The app, as `options` says.
pub fn downloads_app(options: Options) -> App {
    let files = downloads(options.downloads, options.seed);
    let total: u64 = files.iter().map(|d| d.size).sum();
    let count = files.len();
    let mut made = None;
    let queue = files.clone();
    let app = App::new(|| {
        let state = State {
            workers: std::array::from_fn(|_| signal(None)),
            received: signal(0),
            finished: signal(0),
        };
        made = Some(state);
        let queue = Rc::new(RefCell::new(VecDeque::from(queue)));
        let threads = options.threads;
        let tick = options.tick;
        every(tick, move |cx| {
            // Done: one frame shows it, and the next tick ends the app.
            if state.finished.get_untracked() == count {
                cx.quit();
                return;
            }
            if threads {
                return;
            }
            // On the timer: each worker takes a file when it has none,
            // and steps it on.
            for w in 0..WORKERS {
                let job = state.workers[w]
                    .get_untracked()
                    .or_else(|| queue.borrow_mut().pop_front().map(Job::new));
                if let Some(mut job) = job {
                    let before = job.done;
                    let complete = job.step();
                    state.report(cx, w, before, job, complete, tick);
                }
            }
        });
        view(state, count, total).bind("q", "stop", |cx| cx.quit())
    })
    .inline(ROWS)
    .help_key("?");
    if options.threads {
        if let Some(state) = made {
            start_threads(app.proxy(), state, files, options.tick);
        }
    }
    app
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    downloads_app(options).run()
}
