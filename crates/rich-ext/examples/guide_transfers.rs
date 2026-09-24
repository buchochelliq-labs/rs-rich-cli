//! Guide: Transfers, retries and notifications — run: cargo run -p rs-rich-ext --example guide_transfers [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/transfers-and-status.md` come from this
//! file. With `--svg DIR` every shot is written as
//! `DIR/guide_transfers-<shot>.svg`. Every time is fixed, so the shots are
//! the same on every run.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Progress, Text, Theme};
use rich_ext::a11y::{AccessibilityPolicy, SymbolSet};
use rich_ext::cancel::CancelToken;
use rich_ext::countdown::{Backoff, CountdownWait, Motion, RateLimit, WaitOutcome};
use rich_ext::live::LiveCoordinator;
use rich_ext::notify::{Notification, Notifications, ToastStyle};
use rich_ext::target::{RenderTarget, TargetKind};
use rich_ext::theme::extended_theme;
use rich_ext::transfer::{transfer_columns, Transfer, Transfers};

/// Where shots go: the terminal, or one SVG per shot.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let dir = args
            .iter()
            .position(|arg| arg == "--svg")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        Shots { dir }
    }

    fn shot(&self, name: &str, width: usize, body: impl FnOnce(&Console)) {
        match &self.dir {
            None => {
                let console = Console::builder().theme(extended_theme()).build();
                console.print_str(&format!("[dim]── {name} ──[/]"));
                body(&console);
            }
            Some(dir) => {
                let console = Console::builder()
                    .width(width)
                    .force_terminal(true)
                    .color_system(Some(ColorSystem::Truecolor))
                    .theme(extended_theme())
                    .build();
                let id = format!("guide_transfers-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

// --8<-- [start:group]
fn session() -> Transfers {
    let mut group = Transfers::new().summary(true);
    let iso = group.push(Transfer::download("debian-13.iso").total(650_000_000));
    let pkg = group.push(
        Transfer::download("packages.tar.zst")
            .total(48_000_000)
            .max_attempts(3),
    );
    let log = group.push(Transfer::upload("session.log")); // size unknown
    let docs = group.push(Transfer::download("docs.zip").total(2_400_000));

    // Every update takes the time: here, seconds since the session started.
    for s in 0..=10 {
        group[iso].set_completed(s * 21_000_000, secs(s));
        group[log].set_completed(s * 40_000, secs(s));
    }
    group[pkg].advance(19_500_000, secs(6));
    group[pkg].retry("connection reset by peer"); // attempt 2 of 3 next
    group[docs].finish(secs(4));
    group
}
// --8<-- [end:group]

fn main() {
    let shots = Shots::from_args();

    shots.shot("group", 90, |console| {
        console.print(&session());
    });

    shots.shot("plain", 90, |console| {
        // --8<-- [start:plain]
        // No colour: the bar is drawn in brackets and every state is a word.
        let plain = Console::builder().width(90).no_color(true).build();
        let words = session().symbols(SymbolSet::Words);
        let out = plain.render_to_string(&words);
        // --8<-- [end:plain]
        console.print(&Text::new(out));
    });

    shots.shot("progress", 90, |console| {
        // --8<-- [start:progress]
        // A deterministic clock for the core Progress (seconds as f64 bits).
        let clock = Arc::new(AtomicU64::new(0f64.to_bits()));
        let reader = clock.clone();
        let mut progress = Progress::new()
            .columns(transfer_columns())
            .clock(move || f64::from_bits(reader.load(Ordering::SeqCst)));

        let mut transfer = Transfer::download("model.safetensors").total(4_000_000_000);
        let task = progress.add_task("model.safetensors", 0.0, 0.0);
        for s in 1..=5 {
            clock.store((s as f64).to_bits(), Ordering::SeqCst);
            transfer.advance(120_000_000, secs(s));
            progress.update(task, transfer.task_update());
        }
        console.print(&progress);
        // --8<-- [end:progress]
    });

    shots.shot("retry", 80, |console| {
        // --8<-- [start:backoff]
        let backoff = Backoff::new(secs(1))
            .factor(2.0)
            .max(secs(30))
            .attempts(5)
            .jitter(0.25, 7); // same seed, same delays: tests stay exact
        for attempt in 1..=5 {
            let status = backoff
                .status(attempt, "503 Service Unavailable")
                .expect("attempts count from 1");
            console.print(&status);
        }
        // --8<-- [end:backoff]
    });

    shots.shot("countdown", 80, |console| {
        // --8<-- [start:countdown]
        let delay = secs(8);
        let status = Backoff::new(delay)
            .attempts(3)
            .status(1, "connection refused")
            .expect("attempt 1")
            .bar(delay);
        // The frames an animated wait draws, one per second.
        for left in [8, 5, 2] {
            console.print(&status.at(secs(left)));
        }
        let limit = RateLimit::new(secs(42))
            .scope("search API")
            .limit(30)
            .remaining(0)
            .bar(secs(60));
        console.print(&limit);
        // --8<-- [end:countdown]
    });

    shots.shot("toasts", 70, |console| {
        // --8<-- [start:toasts]
        let mut toasts = Notifications::new()
            .default_ttl(Some(secs(4)))
            .max_visible(3);
        toasts.push(
            Notification::ok("debian-13.iso verified").title("Checksum"),
            secs(0),
        );
        toasts.push(
            Notification::info("switched to mirror 2").title("Net"),
            secs(1),
        );
        toasts.push(
            Notification::warning("92% used")
                .title("Disk")
                .ttl(secs(30)),
            secs(2),
        );
        toasts.push(
            Notification::error("session.log: 403").title("Upload"),
            secs(3),
        );
        console.print(&toasts); // the newest three, and "+1 more"

        toasts.expire(secs(5)); // the first two have had their 4 seconds
        console.print(&Text::new(""));
        console.print(&toasts);
        // --8<-- [end:toasts]
    });

    shots.shot("toast-panel", 70, |console| {
        // --8<-- [start:toast-panel]
        let toast = Notification::error("upload rejected: 403 Forbidden")
            .title("session.log")
            .toast_style(ToastStyle::Panel);
        console.print(&toast);
        // --8<-- [end:toast-panel]
    });

    shots.shot("log", 80, |console| {
        let out = log_fallback();
        console.print(&Text::new(out));
    });
}

/// The non-interactive fallback: what a log or a pipe receives.
fn log_fallback() -> String {
    // --8<-- [start:log]
    let stream = RenderTarget::new(
        TargetKind::PlainStream, // a pipe: not interactive, no colour
        TargetCapabilities {
            width: 80,
            height: 24,
            color_system: None,
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    );
    let motion = Motion::for_target(&stream, &AccessibilityPolicy::default());
    assert_eq!(motion, Motion::Static); // print once, never redraw

    let mut out = Vec::new();
    {
        let mut live = LiveCoordinator::new(&mut out, stream.clone());
        let mut toasts = Notifications::for_target(&stream); // prints each once
        let mut region = None;
        let backoff = Backoff::new(secs(2)).attempts(3);
        let cancel = CancelToken::new();

        for attempt in 1..=3 {
            let status = backoff
                .status(attempt, "connection refused")
                .expect("from 1");
            let Some(delay) = backoff.delay(attempt) else {
                live.print(&stream.segments(&status)).expect("print");
                break;
            };
            let outcome = CountdownWait::new(delay)
                .cancel(cancel.clone())
                .sleeper(|_| {}) // a real program leaves the default sleeper
                .run_live(&mut live, &stream, motion, |left| status.at(left))
                .expect("live output");
            assert_eq!(outcome, WaitOutcome::Elapsed);
        }
        toasts.push(
            Notification::error("giving up on mirror 1").title("Net"),
            secs(9),
        );
        toasts
            .present(&mut live, &stream, &mut region, secs(9))
            .expect("live output");
        live.finish().expect("live output");
    }
    // --8<-- [end:log]
    String::from_utf8(out).expect("UTF-8")
}
