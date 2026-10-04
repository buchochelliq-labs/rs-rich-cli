//! Guide: Charts — run: cargo run -p rs-rich-ext --example guide_charts [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/charts.md` comes from this file.
use std::path::PathBuf;
use std::sync::Arc;

use rich::{Cell, ColorSystem, Console, Panel, Table, Text};
use rich_ext::chart::{
    Bar, BarChart, Charset, Histogram, LineChart, Series, Sparkline, ValueFormat,
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

fn main() {
    let shots = Shots::from_args();
    shots.shot("sparkline", 60, "Sparkline", show_sparklines);
    shots.shot("bars", 60, "BarChart", show_bars);
    shots.shot("histogram", 60, "Histogram", show_histogram);
    shots.shot("line", 66, "LineChart", show_line);
    shots.shot("plain", 66, "LineChart without colour", show_without_colour);
    shots.shot("ascii", 66, "ASCII", show_ascii);
    shots.shot("table", 66, "Charts in a table and a panel", show_table);
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
