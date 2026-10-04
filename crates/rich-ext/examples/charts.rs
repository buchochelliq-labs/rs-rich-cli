//! Every chart in `rich_ext::chart`, in colour, Braille and ASCII.
//!
//! Run: `cargo run -p rs-rich-ext --example charts`. Pass `--ascii` to see
//! the ASCII forms, `--no-color` for the forms that read without colour.
use std::sync::Arc;

use rich::{Cell, Console, Panel, Table, Text};
use rich_ext::chart::{BarChart, Charset, Histogram, LineChart, Series, Sparkline};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut builder = Console::builder().theme(rich_ext::extended_theme());
    if args.iter().any(|a| a == "--ascii") {
        builder = builder.ascii_only(true);
    }
    if args.iter().any(|a| a == "--no-color") {
        builder = builder.no_color(true);
    }
    let console = builder.build();

    let requests = [
        12.0, 15.0, 14.0, 18.0, 25.0, 31.0, 28.0, 22.0, 35.0, 41.0, 38.0, 30.0, 26.0, 33.0, 47.0,
        52.0, 44.0, 39.0, 36.0, 29.0,
    ];

    console.print(&Text::new("Sparklines"));
    console.print(&Sparkline::new(requests).min_max(true));
    console.print(&Sparkline::new(requests).threshold(40.0));
    console.print(&Sparkline::new(requests).charset(Charset::Braille));
    console.print(&Text::new(""));

    console.print(&Text::new("Bars"));
    console.print(
        &BarChart::new()
            .bar("api", 412.0)
            .bar("web", 268.5)
            .bar("worker", 97.0)
            .bar("cron", 12.0)
            .bar_width(30),
    );
    console.print(
        &BarChart::from_pairs([
            ("north", 18.0),
            ("south", -7.5),
            ("east", 4.0),
            ("west", -12.0),
        ])
        .bar_width(30),
    );
    console.print(&Text::new(""));

    console.print(&Text::new("Histogram"));
    let latencies: Vec<f64> = (0..200)
        .map(|i| {
            let x = (i as f64 * 0.37).sin() * 0.5 + (i as f64 * 0.11).cos() * 0.5;
            40.0 + x * x * 120.0 + (i % 7) as f64 * 3.0
        })
        .collect();
    console.print(&Histogram::new(latencies).bins(8).bar_width(30));
    console.print(&Text::new(""));

    console.print(&Text::new("Line and scatter"));
    let cpu: Vec<(f64, f64)> = (0..60)
        .map(|i| (i as f64, 50.0 + 30.0 * (i as f64 / 8.0).sin()))
        .collect();
    let mem: Vec<(f64, f64)> = (0..60).map(|i| (i as f64, 30.0 + i as f64 * 0.8)).collect();
    let spikes: Vec<(f64, f64)> = [(7.0, 92.0), (23.0, 88.0), (41.0, 95.0), (55.0, 90.0)].into();
    let chart = LineChart::new()
        .series(Series::line("cpu %", cpu))
        .series(Series::line("memory %", mem))
        .series(Series::scatter("alerts", spikes))
        .y_range(0.0, 100.0)
        .height(11)
        .width(64);
    console.print(&chart);
    console.print(&Text::new(""));

    console.print(&Text::new("In a table and a panel"));
    let mut table = Table::new();
    table.add_column("Service");
    table.add_column("Requests");
    table.add_column("p95");
    for (name, values, p95) in [
        ("api", &requests[..], 412.0),
        ("web", &requests[5..], 268.0),
    ] {
        table.add_row_cells(vec![
            Cell::Markup(name.into()),
            Cell::Renderable(Arc::new(Sparkline::new(values.iter().copied()))),
            Cell::Renderable(Arc::new(
                BarChart::new().bar("", p95).max(500.0).bar_width(12),
            )),
        ]);
    }
    console.print(&table);
    console.print(
        &Panel::new(Box::new(chart.width(40).height(5)))
            .title("cpu")
            .expand(false),
    );
}
