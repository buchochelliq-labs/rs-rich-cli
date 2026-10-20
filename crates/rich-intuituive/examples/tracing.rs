//! ratatui's `tracing` example, rebuilt, with what it leaves to the
//! third-party tui-logger: the app's own `tracing` events in a log pane.
//!
//!     cargo run -p rs-rich-intuituive --example tracing --features tracing
//!
//! A worker thread logs at random levels under three targets. The log
//! follows the newest line until you scroll back (↑, PgUp, Home, the
//! wheel); End follows again. The targets panel sets each target's level:
//! ↑ and ↓ choose, ← shows less of it, → more, Space turns it off or back
//! on. `-` and `+` raise and lower the log's own level. The box under the
//! log filters as you type: words, `level:warn`, `target:worker::db`.
//! Tab moves between the panes. `q` quits (or Ctrl+C in the box).

use std::thread::JoinHandle;
use std::time::Duration;

use intuituive::log::{Severity, TracingFeed};
use intuituive::prelude::*;
use intuituive::widgets::Scrollbar;
use tracing::Dispatch;
use tracing_subscriber::prelude::*;

/// The levels `-` steps through, from showing everything to errors only.
const LEVELS: [Option<Severity>; 5] = [
    None,
    Some(Severity::Debug),
    Some(Severity::Info),
    Some(Severity::Warn),
    Some(Severity::Error),
];

/// The app, and its log for the layer to feed.
pub fn tracing_app() -> (App, Log) {
    let mut made = None;
    let app = App::new(|| {
        let log = Log::new(5_000);
        // ratatui's example filters at DEBUG unless RUST_LOG says otherwise.
        log.set_level(Some(Severity::Debug));
        made = Some(log);
        let filter = signal(String::new());
        watch(
            move || filter.get(),
            move |text, _| {
                // A query typed halfway (`level:`) keeps the last one.
                let _ = log.set_filter(&text);
            },
        );
        let step = move |by: isize| {
            let at = LEVELS.iter().position(|l| *l == log.level()).unwrap_or(0);
            let to = (at as isize + by).clamp(0, LEVELS.len() as isize - 1) as usize;
            if to != at {
                log.set_level(LEVELS[to]);
                tracing::info!(target: "tracing_app", level = level_name(LEVELS[to]), "level set");
            }
        };
        column([
            row([
                log.targets_view().panel("Targets").fixed(26),
                log.view()
                    .scrollbar(Scrollbar::new())
                    .focusable()
                    .panel("Log")
                    .flex(1),
            ])
            .flex(1),
            row([
                label("[dim]Filter[/] ").fixed(7),
                input(filter)
                    .placeholder("words, level:warn, target:worker::db")
                    .flex(1),
            ])
            .fixed(1),
            text(move || status(log)).fixed(1),
        ])
        .on_key("-", move |_| step(1))
        .on_key("+", move |_| step(-1))
        .on_key("q", |cx| cx.quit())
    });
    (app, made.expect("the app is built at once"))
}

fn level_name(level: Option<Severity>) -> &'static str {
    level.map_or("trace", Severity::as_str)
}

/// The status line: following or how much is new, what shows, the level.
fn status(log: Log) -> String {
    let place = if log.is_following() {
        "[green]following[/]".to_string()
    } else {
        format!(
            "[yellow]scrolled back[/] · {} new · End follows",
            log.unseen()
        )
    };
    format!(
        "{place} · {} of {} lines · level {}+ · [dim]q quits, -/+ level, Tab moves[/]",
        log.shown(),
        log.len(),
        level_name(log.level()),
    )
}

/// The next number of a xorshift generator: random enough for levels.
fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// One event at a level `roll` picks, under `target`.
macro_rules! work {
    ($target:literal, $roll:expr, $job:expr) => {
        match $roll % 5 {
            0 => tracing::trace!(target: $target, job = $job, "polling"),
            1 => tracing::debug!(target: $target, job = $job, "picked up"),
            2 => tracing::info!(target: $target, job = $job, ms = $roll % 900, "done"),
            3 => tracing::warn!(target: $target, job = $job, "slow, retrying"),
            _ => tracing::error!(target: $target, job = $job, "failed"),
        }
    };
}

/// A worker logging `count` events (forever with `None`) through
/// `dispatch`, one every `tick`, at levels from `seed`.
pub fn worker(dispatch: Dispatch, tick: Duration, seed: u64, count: Option<u64>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let mut state = seed.max(1);
            let mut job = 0u64;
            while count.is_none_or(|count| job < count) {
                let roll = next(&mut state);
                match (roll >> 8) % 3 {
                    0 => work!("worker::jobs", roll, job),
                    1 => work!("worker::net", roll, job),
                    _ => work!("worker::db", roll, job),
                }
                job += 1;
                if !tick.is_zero() {
                    std::thread::sleep(tick);
                }
            }
        })
    })
}

/// The subscriber: the layer feeding `log` (filters are the log's own).
pub fn dispatch(app: &App, log: Log) -> Dispatch {
    let layer = TracingFeed::new(log, app.proxy()).layer();
    Dispatch::new(tracing_subscriber::registry().with(layer))
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let (app, log) = tracing_app();
    let dispatch = dispatch(&app, log);
    let _ = tracing::dispatcher::set_global_default(dispatch.clone());
    tracing::info!(target: "tracing_app", "started");
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |since| since.as_nanos() as u64);
    let _worker = worker(dispatch, Duration::from_millis(200), seed, None);
    app.run()
}
