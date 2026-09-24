//! Retry and rate-limit countdown renderables (#391).
use std::cell::RefCell;
use std::time::Duration;

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Renderable, Theme};
use rich_ext::a11y::{AccessibilityPolicy, Status, SymbolSet};
use rich_ext::cancel::CancelToken;
use rich_ext::countdown::{
    remaining_label, Backoff, CountdownBar, CountdownWait, Motion, RateLimit, RetryStatus,
    WaitOutcome,
};
use rich_ext::live::LiveCoordinator;
use rich_ext::target::{RenderTarget, TargetKind};

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn plain(width: usize, renderable: &dyn Renderable) -> String {
    Console::builder()
        .width(width)
        .build()
        .render_to_string(renderable)
}

fn color(width: usize, renderable: &dyn Renderable) -> String {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build()
        .render_to_string(renderable)
}

fn target(kind: TargetKind, interactive: bool) -> RenderTarget {
    RenderTarget::new(
        kind,
        TargetCapabilities {
            width: 60,
            height: 10,
            color_system: None,
            interactive,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}

#[test]
fn backoff_grows_caps_and_stops() {
    let backoff = Backoff::new(Duration::from_millis(500))
        .factor(2.0)
        .max(secs(3))
        .attempts(6);
    let delays: Vec<u128> = backoff.delays().map(|d| d.as_millis()).collect();
    assert_eq!(delays, [500, 1000, 2000, 3000, 3000]);
    assert_eq!(backoff.delay(6), None);
    assert_eq!(backoff.max_attempts(), Some(6));
    // A factor below 1 is treated as constant backoff; no limit is endless.
    let constant = Backoff::new(secs(2)).factor(0.5);
    assert!(constant.delays().take(50).all(|d| d == secs(2)));
}

#[test]
fn jitter_is_deterministic_per_seed() {
    let backoff = Backoff::new(secs(10)).factor(1.0).attempts(4);
    let a: Vec<Duration> = backoff.clone().jitter(0.5, 1).delays().collect();
    let b: Vec<Duration> = backoff.clone().jitter(0.5, 1).delays().collect();
    let c: Vec<Duration> = backoff.clone().jitter(0.5, 2).delays().collect();
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert!(a.iter().all(|d| *d >= secs(5) && *d <= secs(10)), "{a:?}");
    // No jitter leaves delays exact.
    assert_eq!(backoff.jitter(0.0, 99).delay(1), Some(secs(10)));
}

#[test]
fn retry_status_lines() {
    let retrying = RetryStatus::new(3)
        .max_attempts(5)
        .reason("connection reset")
        .retrying_in(Duration::from_millis(3200));
    assert_eq!(
        plain(80, &retrying),
        "⚠ warning attempt 3/5 failed: connection reset — retrying in 4s"
    );
    assert_eq!(retrying.status(), Status::Warning);
    assert_eq!(
        plain(80, &retrying.clone().symbols(SymbolSet::Words)),
        "warning: attempt 3/5 failed: connection reset — retrying in 4s"
    );
    let giving_up = RetryStatus::new(5).max_attempts(5).reason("timeout");
    assert!(giving_up.giving_up());
    assert_eq!(giving_up.status(), Status::Error);
    assert_eq!(
        plain(80, &giving_up),
        "✖ error attempt 5/5 failed: timeout — giving up"
    );
    assert_eq!(
        plain(80, &RetryStatus::new(2).retrying_in(secs(90))),
        "⚠ warning attempt 2 failed — retrying in 1m 30s"
    );
    let ascii = Console::builder().width(80).ascii_only(true).build();
    assert_eq!(
        ascii.render_to_string(&retrying),
        "[WARN] attempt 3/5 failed: connection reset - retrying in 4s"
    );
}

#[test]
fn retry_status_from_backoff_and_with_a_bar() {
    let backoff = Backoff::new(secs(2)).attempts(3);
    assert!(backoff.status(0, "x").is_none());
    let first = backoff.status(1, "503").unwrap().bar(secs(2));
    assert_eq!(
        plain(80, &first.at(Duration::from_millis(500))),
        "⚠ warning attempt 1/3 failed: 503 — retrying in 1s  [####..............]"
    );
    assert_eq!(
        plain(80, &backoff.status(3, "503").unwrap()),
        "✖ error attempt 3/3 failed: 503 — giving up"
    );
}

#[test]
fn retry_status_in_colour() {
    let status = RetryStatus::new(1).reason("boom").retrying_in(secs(4));
    assert_eq!(
        color(80, &status),
        concat!(
            "\u{1b}[33m⚠ warning\u{1b}[0m \u{1b}[1mattempt 1\u{1b}[0m failed: boom — ",
            "retrying in \u{1b}[1;36m4s\u{1b}[0m",
        )
    );
}

#[test]
fn rate_limit_lines() {
    assert_eq!(
        plain(80, &RateLimit::new(secs(42))),
        "⚠ warning rate limited — resets in 0:00:42"
    );
    assert_eq!(
        plain(80, &RateLimit::new(secs(3725)).limit(5000)),
        "⚠ warning rate limited — resets in 1:02:05 (limit 5000)"
    );
    let limit = RateLimit::new(secs(30))
        .remaining(0)
        .bar(secs(60))
        .bar_width(12)
        .symbols(SymbolSet::Ascii);
    assert_eq!(
        plain(80, &limit),
        "[WARN] rate limited — resets in 0:00:30 (0 left)  [#####.....]"
    );
    assert_eq!(
        plain(80, &limit.at(secs(6))),
        "[WARN] rate limited — resets in 0:00:06 (0 left)  [#.........]"
    );
}

#[test]
fn countdown_bar_shrinks() {
    let frames: Vec<String> = [10, 5, 0]
        .into_iter()
        .map(|left| plain(40, &CountdownBar::new(secs(10), secs(left)).width(12)))
        .collect();
    assert_eq!(
        frames,
        ["[##########]  10s", "[#####.....]  5s", "[..........]  0s"]
    );
    assert_eq!(
        color(40, &CountdownBar::new(secs(10), secs(5)).width(10)),
        concat!(
            "\u{1b}[36m━━━━━\u{1b}[0m\u{1b}[38;5;237m╺\u{1b}[0m",
            "\u{1b}[38;5;237m━━━━\u{1b}[0m  \u{1b}[1;36m5s\u{1b}[0m",
        )
    );
    assert_eq!(remaining_label(Duration::from_millis(1)), "1s");
}

#[test]
fn wait_ticks_through_an_injected_sleeper() {
    let slept = RefCell::new(Vec::new());
    let mut ticks = Vec::new();
    let outcome = CountdownWait::new(Duration::from_millis(2500))
        .tick(secs(1))
        .sleeper(|d| slept.borrow_mut().push(d.as_millis()))
        .run(|left| ticks.push(left.as_millis()));
    assert_eq!(outcome, WaitOutcome::Elapsed);
    assert_eq!(ticks, [2500, 1500, 500, 0]);
    assert_eq!(*slept.borrow(), [1000, 1000, 500]);
    // A zero wait ticks once and never sleeps.
    let outcome = CountdownWait::new(Duration::ZERO)
        .sleeper(|_| panic!("slept"))
        .run(|_| {});
    assert_eq!(outcome, WaitOutcome::Elapsed);
}

#[test]
fn wait_stops_when_cancelled() {
    let token = CancelToken::new();
    let canceller = token.clone();
    let mut sleeps = 0;
    let outcome = CountdownWait::new(secs(60))
        .tick(secs(1))
        .cancel(token)
        .sleeper(|_| {
            sleeps += 1;
            if sleeps == 3 {
                canceller.cancel();
            }
        })
        .run(|_| {});
    assert_eq!(outcome, WaitOutcome::Cancelled);
    assert_eq!(sleeps, 3);
}

#[test]
fn motion_follows_target_and_policy() {
    let terminal = target(TargetKind::Terminal, true);
    let plain_stream = target(TargetKind::PlainStream, true);
    let default = AccessibilityPolicy::default();
    assert_eq!(Motion::for_target(&terminal, &default), Motion::Animated);
    assert_eq!(Motion::for_target(&plain_stream, &default), Motion::Static);
    assert_eq!(
        Motion::for_target(&terminal, &AccessibilityPolicy::reduced_motion()),
        Motion::Static
    );
}

#[test]
fn static_wait_prints_one_line_per_attempt() {
    let backoff = Backoff::new(secs(1)).attempts(3);
    let stream = target(TargetKind::PlainStream, false);
    let mut out = Vec::new();
    {
        let mut live = LiveCoordinator::new(&mut out, stream.clone());
        for attempt in 1..=3 {
            let status = backoff.status(attempt, "refused").unwrap();
            let Some(delay) = backoff.delay(attempt) else {
                live.print(&stream.segments(&status)).unwrap();
                break;
            };
            let outcome = CountdownWait::new(delay)
                .sleeper(|_| {})
                .run_live(&mut live, &stream, Motion::Static, |left| status.at(left))
                .unwrap();
            assert_eq!(outcome, WaitOutcome::Elapsed);
        }
        live.finish().unwrap();
    }
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "⚠ warning attempt 1/3 failed: refused — retrying in 1s\n",
            "⚠ warning attempt 2/3 failed: refused — retrying in 2s\n",
            "✖ error attempt 3/3 failed: refused — giving up\n",
        )
    );
}

#[test]
fn animated_wait_redraws_a_region_and_removes_it() {
    let terminal = target(TargetKind::Terminal, true);
    let mut out = Vec::new();
    {
        let mut live = LiveCoordinator::new(&mut out, terminal.clone());
        let status = RetryStatus::new(1).reason("busy");
        let outcome = CountdownWait::new(secs(2))
            .tick(secs(1))
            .sleeper(|_| {})
            .run_live(&mut live, &terminal, Motion::Animated, |left| {
                status.at(left)
            })
            .unwrap();
        assert_eq!(outcome, WaitOutcome::Elapsed);
        live.finish().unwrap();
    }
    let out = String::from_utf8(out).unwrap();
    for frame in ["retrying in 2s", "retrying in 1s", "retrying in 0s"] {
        assert_eq!(out.matches(frame).count(), 1, "{frame}: {out:?}");
    }
    // Hidden cursor while drawing, restored, and the region erased at the end.
    assert!(
        out.contains("\u{1b}[?25l") && out.ends_with("\u{1b}[?25h"),
        "{out:?}"
    );
    assert!(out.contains("\u{1b}[2K"), "{out:?}");
}
