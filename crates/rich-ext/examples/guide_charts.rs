//! Guide: Charts — run: cargo run -p rs-rich-ext --example guide_charts [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/charts.md` comes from this file.
use std::path::PathBuf;
use std::sync::Arc;

use rich::{Cell, ColorSystem, Columns, Console, Layout, Panel, Table, Text};
use rich_ext::chart::{
    Band, Bar, BarChart, BulletChart, Charset, Gauge, Heatmap, Histogram, KpiCard, LineChart,
    Orientation, Series, Sparkline, State, Status, StatusMatrix, Timeline, ValueFormat,
};

const REQUESTS: [f64; 24] = [
    12.0, 15.0, 14.0, 18.0, 25.0, 31.0, 28.0, 22.0, 35.0, 41.0, 38.0, 30.0, 26.0, 33.0, 47.0, 52.0,
    44.0, 39.0, 36.0, 29.0, 24.0, 21.0, 17.0, 14.0,
];

// --8<-- [start:sparkline]
fn show_sparklines(console: &Console) {
    console.print(&Sparkline::new(REQUESTS));
    // Name the extremes and count the values over a threshold.
    console.print(&Sparkline::new(REQUESTS).min_max(true).threshold(40.0));
    // Two values per cell.
    console.print(&Sparkline::new(REQUESTS).charset(Charset::Braille));
    // Fewer cells than values: each cell is the mean of its bucket.
    let narrow = Console::builder().width(8).color_system(None).build();
    console.print(&Text::new(
        narrow.render_to_string(&Sparkline::new(REQUESTS)),
    ));
}
// --8<-- [end:sparkline]

// --8<-- [start:bars]
fn show_bars(console: &Console) {
    let chart = BarChart::new()
        .bar("api", 412.0)
        .bar("web", 268.5)
        .bar("worker", 97.0)
        .push(Bar::new("cron", 12.0).style("chart.over"))
        .bar_width(30);
    console.print(&chart);
    // Negative values extend left of zero.
    console.print(
        &BarChart::from_pairs([
            ("north", 18.0),
            ("south", -7.5),
            ("east", 4.0),
            ("west", -12.0),
        ])
        .bar_width(30),
    );
}
// --8<-- [end:bars]

// --8<-- [start:vertical]
fn show_vertical(console: &Console) {
    let week = BarChart::from_pairs([
        ("mon", 31.0),
        ("tue", 42.5),
        ("wed", 38.0),
        ("thu", 51.0),
        ("fri", 47.0),
        ("sat", 12.0),
        ("sun", 9.5),
    ])
    .orientation(Orientation::Vertical)
    .bar_width(8);
    console.print(&week);
    // Negative values hang below zero, their values under them.
    console.print(
        &BarChart::from_pairs([("q1", 18.0), ("q2", -7.5), ("q3", 4.0), ("q4", -12.0)])
            .orientation(Orientation::Vertical)
            .bar_width(6),
    );
}
// --8<-- [end:vertical]

fn latencies() -> Vec<f64> {
    (0..400)
        .map(|i| {
            let a = ((i * 7919) % 1000) as f64 / 1000.0;
            let b = ((i * 104_729) % 1000) as f64 / 1000.0;
            20.0 + (a + b) * 60.0 + if i % 17 == 0 { 120.0 } else { 0.0 }
        })
        .collect()
}

// --8<-- [start:histogram]
fn show_histogram(console: &Console) {
    console.print(&Histogram::new(latencies()).bins(8).bar_width(30));
}
// --8<-- [end:histogram]

fn traffic() -> LineChart {
    // --8<-- [start:line]
    let cpu = (0..60).map(|i| (i as f64, 50.0 + 30.0 * (i as f64 / 8.0).sin()));
    let memory = (0..60).map(|i| (i as f64, 30.0 + i as f64 * 0.8));
    let alerts = [(7.0, 92.0), (23.0, 88.0), (41.0, 95.0), (55.0, 90.0)];
    LineChart::new()
        .series(Series::line("cpu %", cpu))
        .series(Series::line("memory %", memory))
        .series(Series::scatter("alerts", alerts))
        .y_range(0.0, 100.0)
        .height(11)
        .width(64)
    // --8<-- [end:line]
}

fn show_line(console: &Console) {
    console.print(&traffic());
}

// --8<-- [start:plain]
fn show_without_colour(console: &Console) {
    // No colour: two or more series plot with markers, not Braille.
    let plain = Console::builder().width(64).color_system(None).build();
    console.print(&Text::new(plain.render_to_string(&traffic().height(6))));
}
// --8<-- [end:plain]

// --8<-- [start:ascii]
fn show_ascii(console: &Console) {
    // An ASCII-only console, or `.charset(Charset::Ascii)`.
    let ascii = Console::builder()
        .width(64)
        .color_system(None)
        .ascii_only(true)
        .build();
    let out = [
        ascii.render_to_string(&Sparkline::new(REQUESTS).min_max(true)),
        ascii.render_to_string(
            &BarChart::new()
                .bar("api", 412.0)
                .bar("web", 268.5)
                .bar_width(20),
        ),
        ascii.render_to_string(&traffic().height(6)),
    ];
    console.print(&Text::new(out.concat().trim_end()));
}
// --8<-- [end:ascii]

// --8<-- [start:table]
fn show_table(console: &Console) {
    let mut table = Table::new().title("Services");
    table.add_column("Service");
    table.add_column("Requests, last 24 h");
    table.add_column("p95 ms");
    for (name, offset, p95) in [("api", 0, 412.0), ("web", 6, 268.0), ("worker", 12, 97.0)] {
        let values = REQUESTS.iter().cycle().skip(offset).take(24).copied();
        table.add_row_cells(vec![
            Cell::Markup(name.into()),
            Cell::Renderable(Arc::new(Sparkline::new(values))),
            Cell::Renderable(Arc::new(
                BarChart::new()
                    .bar("", p95)
                    .max(500.0)
                    .bar_width(12)
                    .format(ValueFormat::Fixed(0)),
            )),
        ]);
    }
    console.print(&table);
    let panel = Panel::new(Box::new(traffic().width(56).height(5)))
        .title("cpu and memory")
        .expand(false);
    console.print(&panel);
}
// --8<-- [end:table]

// --8<-- [start:gauge]
fn show_gauges(console: &Console) {
    let cpu = Gauge::new("cpu", 72.0)
        .range(0.0, 100.0)
        .target(80.0)
        .band(Band::new(60.0, "ok").style("chart.ok"))
        .band(Band::new(85.0, "high").style("chart.warning"))
        .band(Band::new(100.0, "critical").style("chart.critical"))
        .unit("%");
    // One line: label, bar, value and the band the value is in.
    console.print(&cpu.clone().bar_width(24));
    // Full width: a scale under the bar and a legend.
    console.print(&cpu.full_width(true));
    console.print(&Text::new(""));
    // Several gauges with their columns aligned.
    let quarter = BulletChart::new()
        .gauge(Gauge::new("revenue", 270.0).range(0.0, 300.0).target(250.0))
        .gauge(Gauge::new("profit", 22.5).range(0.0, 30.0).target(26.0))
        .gauge(
            Gauge::new("new customers", 1650.0)
                .range(0.0, 2000.0)
                .target(1800.0)
                .band(Band::new(1400.0, "poor").style("chart.critical"))
                .band(Band::new(1700.0, "fair").style("chart.warning"))
                .band(Band::new(2000.0, "good").style("chart.ok")),
        )
        .bar_width(30);
    console.print(&quarter);
}
// --8<-- [end:gauge]

// --8<-- [start:heatmap]
fn show_heatmap(console: &Console) {
    let hours: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
    let mut map = Heatmap::new().columns(hours);
    for (day, name) in ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
        .into_iter()
        .enumerate()
    {
        let weekend = if day >= 5 { 0.4 } else { 1.0 };
        let load = (0..24).map(|h| {
            let peak = (-((h as f64 - 14.0).powi(2)) / 30.0).exp();
            (peak * 900.0 * weekend + 40.0 * day as f64).round()
        });
        map = map.row(name, load);
    }
    console.print(&map);
    // In ASCII: ten shades, so it reads in black and white.
    let ascii = Console::builder()
        .width(60)
        .color_system(None)
        .ascii_only(true)
        .build();
    console.print(&Text::new(ascii.render_to_string(&map).trim_end()));
}
// --8<-- [end:heatmap]

// --8<-- [start:matrix]
fn show_matrix(console: &Console) {
    let matrix = StatusMatrix::new()
        // Your own state: a name, a symbol, an ASCII symbol and a style.
        .state(State::new("running", '◌', 'o', "chart.state.unknown"))
        .columns(["linux", "macos", "windows", "wasm"])
        .row("unit", ["pass", "pass", "pass", "pass"])
        .row("integration", ["pass", "flaky", "fail", "skip"])
        .row("docs", ["pass", "pass", "running", "skip"]);
    console.print(&matrix);
}
// --8<-- [end:matrix]

fn trend(seed: f64) -> Vec<f64> {
    (0..20)
        .map(|i| 50.0 + 30.0 * ((i as f64 + seed) / 3.0).sin())
        .collect()
}

// --8<-- [start:cards]
fn show_cards(console: &Console) {
    let cards = vec![
        KpiCard::new("Requests", 12_400.0)
            .unit("/s")
            .previous(11_430.0)
            .caption("last week")
            .trend(trend(0.0))
            .status(Status::Ok),
        KpiCard::new("Errors", 42.0)
            .delta(-14.0)
            .higher_is_better(false)
            .caption("per hour")
            .trend(trend(4.0))
            .status(Status::Warning),
        KpiCard::new("Latency", 412.0)
            .unit(" ms")
            .previous(260.0)
            .higher_is_better(false)
            .trend(trend(8.0))
            .status(Status::Critical),
    ];
    let cells = cards
        .into_iter()
        .map(|card| Cell::Renderable(Arc::new(card.width(24))))
        .collect();
    console.print(&Columns::from_cells(cells));
}
// --8<-- [end:cards]

// --8<-- [start:timeline]
fn show_timeline(console: &Console) {
    let build = Timeline::new()
        .span("fetch", 0.0, 4.0)
        .span("compile", 4.0, 26.0)
        // Overlapping ranges on one row are stacked.
        .span("test", 12.0, 30.0)
        .span("test", 20.0, 34.0)
        .span("package", 34.0, 39.0)
        .milestone("ship", 40.0)
        .unit("s")
        .width(60);
    console.print(&build);
    console.print(&Text::new(""));
    // Ten minutes idle between two short bursts: the gap is cut out.
    let jobs = Timeline::new()
        .span("worker 1", 0.0, 3.0)
        .span("worker 1", 3.0, 5.0)
        .span("worker 2", 2.0, 6.0)
        .span("worker 1", 600.0, 604.0)
        .span("worker 2", 602.0, 610.0)
        .milestone("deploy", 611.0)
        .unit("s")
        .width(60);
    console.print(&jobs);
}
// --8<-- [end:timeline]

// --8<-- [start:dashboard]
fn dashboard() -> Layout {
    let card = |c: KpiCard| Layout::with_renderable(Box::new(c.expand(true)));
    let mut cards = Layout::new().size(6);
    cards.split_row(vec![
        card(
            KpiCard::new("Requests", 1240.0)
                .unit("/s")
                .previous(1180.0)
                .trend(trend(0.0)),
        ),
        card(
            KpiCard::new("Errors", 7.0)
                .delta(2.0)
                .higher_is_better(false)
                .trend(trend(5.0))
                .status(Status::Warning),
        ),
    ]);
    let checks = StatusMatrix::new()
        .columns(["eu", "us", "ap"])
        .row("api", ["pass", "pass", "flaky"])
        .row("worker", ["pass", "fail", "pass"]);
    let deploy = Timeline::new()
        .span("build", 0.0, 18.0)
        .span("test", 12.0, 34.0)
        .span("rollout", 36.0, 52.0)
        .milestone("live", 60.0)
        .unit("s");
    let mut root = Layout::new();
    root.split_column(vec![
        cards,
        Layout::with_renderable(Box::new(Panel::new(Box::new(checks)).title("checks"))).size(6),
        Layout::with_renderable(Box::new(Panel::new(Box::new(deploy)).title("deploy"))),
    ]);
    root
}
// --8<-- [end:dashboard]

/// A renderable drawn `height` rows tall: a `Layout` otherwise fills the
/// console's height.
struct Rows(Layout, usize);

impl rich::Renderable for Rows {
    fn rich_render(&self, console: &Console, options: &rich::ConsoleOptions) -> Vec<rich::Segment> {
        self.0.rich_render(console, &options.update_height(self.1))
    }
}

fn show_dashboard(console: &Console) {
    // In `Live`, rebuild it each tick: `live.update(Box::new(dashboard()))`.
    console.print(&Rows(dashboard(), 20));
}

fn main() {
    let shots = Shots::from_args();
    shots.shot("sparkline", 60, "Sparkline", show_sparklines);
    shots.shot("bars", 60, "BarChart", show_bars);
    shots.shot("vertical", 60, "Vertical BarChart", show_vertical);
    shots.shot("histogram", 60, "Histogram", show_histogram);
    shots.shot("line", 66, "LineChart", show_line);
    shots.shot("plain", 66, "LineChart without colour", show_without_colour);
    shots.shot("ascii", 66, "ASCII", show_ascii);
    shots.shot("table", 66, "Charts in a table and a panel", show_table);
    shots.shot("gauge", 60, "Gauge and BulletChart", show_gauges);
    shots.shot("heatmap", 60, "Heatmap", show_heatmap);
    shots.shot("matrix", 60, "StatusMatrix", show_matrix);
    shots.shot("cards", 76, "KpiCard", show_cards);
    shots.shot("timeline", 62, "Timeline", show_timeline);
    shots.shot("dashboard", 66, "A dashboard in a Layout", show_dashboard);
}

/// `--svg DIR` writes each shot as `DIR/guide_charts-<shot>.svg`; without it,
/// shots print to the terminal.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { dir }
    }

    fn shot(&self, name: &str, width: usize, title: &str, f: impl FnOnce(&Console)) {
        let Some(dir) = &self.dir else {
            let console = Console::builder().theme(rich_ext::extended_theme()).build();
            return f(&console);
        };
        let console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .theme(rich_ext::extended_theme())
            .build();
        let id = format!("guide_charts-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}
