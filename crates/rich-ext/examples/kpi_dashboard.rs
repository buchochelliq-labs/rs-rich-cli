//! A service dashboard from the KPI and status charts: cards, a status
//! matrix and a timeline in a `Layout`, updating in `Live`.
//!
//! Run: `cargo run -p rs-rich-ext --example kpi_dashboard`. Pass `--ascii`
//! for the ASCII forms, `--no-color` for the forms that read without colour,
//! and `--frames N` for how many updates to show (default 24; `--frames 1`
//! prints one frame and exits).
use std::thread::sleep;
use std::time::Duration;

use rich::{Console, Layout, Live, Panel};
use rich_ext::chart::{
    Band, BulletChart, Gauge, KpiCard, State, Status, StatusMatrix, Timeline, ValueFormat,
};

/// Rows the dashboard takes.
const HEIGHT: usize = 25;

/// Requests per second for the last `n` ticks, ending at `tick`.
fn requests(tick: usize, n: usize) -> Vec<f64> {
    (tick..tick + n)
        .map(|t| {
            let t = t as f64;
            1200.0 + 400.0 * (t / 5.0).sin() + 150.0 * (t / 1.7).cos()
        })
        .collect()
}

/// Errors per minute at tick `t`.
fn errors(t: usize) -> f64 {
    3.0 + (t % 7) as f64
}

/// p95 latency in ms at tick `t`.
fn latency(t: usize) -> f64 {
    (180.0 + 40.0 * (t as f64 / 3.0).sin()).round()
}

fn cards(tick: usize) -> Layout {
    let history = requests(tick, 24);
    let now = history[history.len() - 1];
    let before = history[history.len() - 2];
    let last = tick + 23;
    let errors_now = errors(last);
    let latency_now = latency(last);
    let latency_status = if latency_now > 210.0 {
        Status::Warning
    } else {
        Status::Ok
    };
    let card = |c: KpiCard| Layout::with_renderable(Box::new(c.expand(true)));
    let mut row = Layout::new().size(6);
    row.split_row(vec![
        card(
            KpiCard::new("Requests", now)
                .format(ValueFormat::Fixed(0))
                .unit("/s")
                .previous(before)
                .caption("vs last tick")
                .trend(history)
                .status(Status::Ok),
        ),
        card(
            KpiCard::new("Errors", errors_now)
                .delta(errors_now - errors(last - 1))
                .higher_is_better(false)
                .caption("per minute")
                .trend((tick..=last).map(errors))
                .status(if errors_now > 7.0 {
                    Status::Critical
                } else {
                    Status::Ok
                }),
        ),
        card(
            KpiCard::new("p95 latency", latency_now)
                .unit(" ms")
                .previous(latency(last - 1))
                .higher_is_better(false)
                .trend((tick..=last).map(latency))
                .status(latency_status),
        ),
    ]);
    row
}

fn checks(tick: usize) -> StatusMatrix {
    // One check flips between pass and flaky, one is still running.
    let flaky = if tick % 4 < 2 { "pass" } else { "flaky" };
    let running = if tick % 6 < 3 { "running" } else { "pass" };
    StatusMatrix::new()
        .state(State::new("running", '◌', 'o', "chart.state.unknown"))
        .columns(["eu", "us", "ap"])
        .row("api", ["pass", "pass", flaky])
        .row("web", ["pass", "pass", "pass"])
        .row("worker", ["pass", "fail", running])
        .row("cron", ["skip", "pass", "pass"])
}

fn capacity(tick: usize) -> BulletChart {
    let gauge = |label: &str, value: f64| {
        Gauge::new(label, value)
            .range(0.0, 100.0)
            .target(80.0)
            .band(Band::new(70.0, "ok"))
            .band(Band::new(90.0, "busy").style("chart.warning"))
            .band(Band::new(100.0, "full").style("chart.critical"))
            .unit("%")
    };
    let t = tick as f64;
    BulletChart::new()
        .gauge(gauge("cpu", 62.0 + 20.0 * (t / 4.0).sin()))
        .gauge(gauge("memory", 71.0 + 3.0 * (t / 9.0).cos()))
        .gauge(gauge("disk", 88.0 + t * 0.2))
        .full_width(true)
}

fn deploy(tick: usize) -> Timeline {
    // The deploy advances one second per tick.
    let now = 40.0 + tick as f64;
    let until = |end: f64| end.min(now);
    let mut timeline = Timeline::new()
        .span("build", 0.0, until(18.0))
        .span("test", 12.0, until(34.0))
        .span("test", 20.0, until(31.0))
        .unit("s");
    if now > 36.0 {
        timeline = timeline.span("rollout", 36.0, until(58.0));
    }
    timeline.milestone("approved", 35.0).milestone("live", 60.0)
}

fn dashboard(tick: usize) -> Layout {
    let mut middle = Layout::new().size(10);
    middle.split_row(vec![
        Layout::with_renderable(Box::new(Panel::new(Box::new(checks(tick))).title("checks")))
            .ratio(2),
        Layout::with_renderable(Box::new(
            Panel::new(Box::new(capacity(tick))).title("capacity"),
        ))
        .ratio(3),
    ]);
    let mut root = Layout::new();
    root.split_column(vec![
        cards(tick),
        middle,
        Layout::with_renderable(Box::new(Panel::new(Box::new(deploy(tick))).title("deploy"))),
    ]);
    root
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let frames = args
        .iter()
        .position(|a| a == "--frames")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(24)
        .max(1);
    let mut builder = Console::builder()
        .theme(rich_ext::extended_theme())
        .height(HEIGHT);
    if args.iter().any(|a| a == "--ascii") {
        builder = builder.ascii_only(true);
    }
    if args.iter().any(|a| a == "--no-color") {
        builder = builder.no_color(true);
    }
    let console = builder.build();
    if frames == 1 {
        console.print(&dashboard(0));
        return;
    }
    let mut live = Live::new(Box::new(dashboard(0)), console, std::io::stdout());
    live.start();
    for tick in 1..frames {
        sleep(Duration::from_millis(250));
        live.update(Box::new(dashboard(tick)));
    }
    live.stop();
}
