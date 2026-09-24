//! A simulated download session: three transfers, one retry with a
//! countdown, and toasts under the live display.
//!
//! Run: `cargo run -p rs-rich-ext --example transfers`
//!
//! On a terminal the transfers redraw in place. Piped (`| cat`), the retry
//! and each toast are printed once and the final state is written at the
//! end. With `RICH_A11Y=reduced-motion` the retry countdown is printed once
//! instead of redrawn.

use std::io::IsTerminal;
use std::time::{Duration, Instant};

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console};
use rich_ext::a11y::AccessibilityPolicy;
use rich_ext::capabilities::SystemEnvironment;
use rich_ext::countdown::{Backoff, CountdownWait, Motion};
use rich_ext::live::{LiveCoordinator, LiveError};
use rich_ext::notify::{Notification, Notifications};
use rich_ext::target::{RenderTarget, TargetKind};
use rich_ext::theme::extended_theme;
use rich_ext::transfer::{Transfer, Transfers};

const TICK: Duration = Duration::from_millis(100);

fn main() -> Result<(), LiveError> {
    let console = Console::new();
    let interactive = std::io::stdout().is_terminal();
    let target = RenderTarget::new(
        if interactive {
            TargetKind::Terminal
        } else {
            TargetKind::PlainStream
        },
        TargetCapabilities {
            width: console.width().min(100),
            height: console.height(),
            color_system: Some(ColorSystem::Truecolor),
            interactive,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        extended_theme(),
    );
    let policy = AccessibilityPolicy::from_env(&SystemEnvironment);
    let motion = Motion::for_target(&target, &policy);

    let mut live = LiveCoordinator::new(std::io::stdout(), target.clone());
    let mut toasts = Notifications::for_target(&target).default_ttl(Some(Duration::from_secs(2)));
    let mut toast_region = None;

    let mut group = Transfers::new().summary(true);
    let iso = group.push(Transfer::download("debian-13.iso").total(60_000_000));
    let pkg = group.push(
        Transfer::download("packages.tar.zst")
            .total(24_000_000)
            .max_attempts(3),
    );
    let log = group.push(Transfer::upload("session.log").total(3_000_000));
    let speeds = [(iso, 1_500_000), (pkg, 700_000), (log, 90_000)];
    let body = live.add(target.segments(&group))?;

    let backoff = Backoff::new(Duration::from_secs(2)).attempts(3);
    let origin = Instant::now();
    let mut failed_once = false;

    while !group.finished() {
        std::thread::sleep(TICK);
        let now = origin.elapsed();
        for (index, speed) in speeds {
            let t = &mut group[index];
            if t.state().is_finished() {
                continue;
            }
            t.advance(speed, now);
            if t.fraction() == Some(1.0) {
                t.finish(now);
                toasts.push(
                    Notification::ok(format!("{} complete", t.name())).title("Transfer"),
                    now,
                );
            }
        }

        // Halfway through, the package download drops its connection once.
        if !failed_once && group[pkg].fraction().is_some_and(|f| f > 0.4) {
            failed_once = true;
            group[pkg].retry("connection reset by peer");
            let delay = backoff.delay(1).unwrap_or_default();
            let status = backoff
                .status(1, "connection reset by peer")
                .expect("attempt 1")
                .bar(delay);
            live.update(body.clone(), target.segments(&group))?;
            // Only this transfer waits; the display keeps its place.
            CountdownWait::new(delay)
                .run_live(&mut live, &target, motion, |left| status.at(left))?;
            toasts.push(
                Notification::warning(format!(
                    "resuming packages.tar.zst from {}",
                    rich_ext::format::percent(group[pkg].fraction().unwrap_or(0.0), 0)
                ))
                .title("Retry"),
                origin.elapsed(),
            );
        }

        live.update(body.clone(), target.segments(&group))?;
        toasts.present(&mut live, &target, &mut toast_region, origin.elapsed())?;
    }

    // Let the last toasts fade, then leave the final state on screen.
    while !toasts.is_empty() {
        std::thread::sleep(TICK);
        toasts.present(&mut live, &target, &mut toast_region, origin.elapsed())?;
    }
    live.update(body.clone(), target.segments(&group))?;
    live.remove(body)?;
    live.print(&target.segments(&group))?;
    live.finish()
}
