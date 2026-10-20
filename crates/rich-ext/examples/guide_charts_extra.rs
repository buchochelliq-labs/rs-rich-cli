//! Guide: Pie charts, stacked charts, QR codes and gradients — run: cargo run -p rs-rich-ext --example guide_charts_extra --features qr [-- --svg docs/media/guide]
//!
//! The pie, stacked, QR code and gradient snippets on
//! `docs/guide/ext/charts.md` come from this file; the last shot puts them
//! together.
use std::path::PathBuf;
use std::sync::Arc;

use rich::{Cell, Color, ColorSystem, Console, Table, Text};
use rich_ext::a11y::AccessibleText;
use rich_ext::chart::{
    Axis, Bar, BarChart, BarGroup, Chart, Dataset, GraphType, Labels, Marker, Orientation, PieChart,
};
use rich_ext::gradient::{GradientDirection, GradientText};
use rich_ext::palette::tailwind;
use rich_ext::qr::{ErrorCorrection, QrCode};

// --8<-- [start:pie]
fn traffic() -> PieChart {
    PieChart::from_pairs([
        ("api", 412.0),
        ("web", 268.5),
        ("worker", 97.0),
        ("cron", 12.0),
    ])
}

fn show_pie(console: &Console) {
    console.print(&traffic());
    // A ring, with the total in the hole, and the values in the legend.
    console.print(
        &traffic()
            .donut(0.5)
            .centre_text("790")
            .show_values(true)
            .height(10),
    );
    // In words, for a screen reader.
    console.print(&Text::new(traffic().accessible_text(80)));
}
// --8<-- [end:pie]

// --8<-- [start:stacked]
fn quarters() -> BarChart {
    let quarter = |label: &str, api: f64, web: f64, cron: f64| {
        BarGroup::new(label).stacked().bars([
            Bar::new("api", api),
            Bar::new("web", web),
            Bar::new("cron", cron),
        ])
    };
    BarChart::new()
        .group(quarter("Q1", 41.0, 26.0, 8.0))
        .group(quarter("Q2", 38.0, 31.0, 11.0))
        .group(quarter("Q3", 52.0, 24.0, 9.5))
        .group(quarter("Q4", 47.0, 35.0, 14.0))
}

fn show_stacked(console: &Console) {
    console.print(&quarters().bar_width(40));
    console.print(&quarters().orientation(Orientation::Vertical).bar_width(8));
}
// --8<-- [end:stacked]

// --8<-- [start:areas]
fn show_areas(console: &Console) {
    let layer = |name: &str, values: [f64; 8]| {
        let points = values.into_iter().enumerate().map(|(i, v)| (i as f64, v));
        Dataset::new(points)
            .name(name)
            .graph_type(GraphType::Area)
            .marker(Marker::HalfBlock)
    };
    let chart = Chart::new(vec![
        layer("api", [12.0, 18.0, 25.0, 31.0, 28.0, 22.0, 35.0, 30.0]),
        layer("web", [8.0, 10.0, 14.0, 12.0, 18.0, 20.0, 16.0, 14.0]),
        layer("cron", [2.0, 2.0, 6.0, 2.0, 2.0, 8.0, 2.0, 2.0]),
    ])
    .x_axis(Axis::default().title("hour").labels(Labels::Auto(4)))
    .y_axis(Axis::default().labels(Labels::Auto(3)))
    .stacked(true)
    .height(12);
    console.print(&chart);
}
// --8<-- [end:areas]

// --8<-- [start:qr]
fn show_qr(console: &Console) {
    let code = QrCode::with_error_correction(
        "https://github.com/buchochelliq-labs/rs-rich-cli",
        ErrorCorrection::Medium,
    )
    .expect("fits a QR code");
    // Scanners manage with two modules of quiet zone; the spec asks for four.
    console.print(&code.quiet_zone(2));
}
// --8<-- [end:qr]

// --8<-- [start:gradient]
fn show_gradient(console: &Console) {
    let sunset = [
        tailwind::AMBER.c400,
        tailwind::ROSE.c500,
        tailwind::VIOLET.c600,
    ]
    .map(|hex| Color::parse(hex).expect("a colour"));
    let title = Text::from_markup("[bold]rs-rich: rich, in Rust[/]").expect("markup");
    console.print(&GradientText::new(title, sunset.clone()));
    let block = Text::new("one gradient\nfrom the top\nto the bottom");
    console.print(&GradientText::new(block, sunset.clone()).direction(GradientDirection::Vertical));
    let banner = Text::new("  behind the text  ");
    console.print(&GradientText::new(banner, sunset).background(true));
}
// --8<-- [end:gradient]

// --8<-- [start:together]
fn show_together(console: &Console) {
    let blue = [tailwind::SKY.c400, tailwind::INDIGO.c500].map(|c| Color::parse(c).unwrap());
    let title = Text::from_markup("[bold]Traffic this year[/]").unwrap();
    console.print(&GradientText::new(title, blue));
    let code = QrCode::new("https://example.com/traffic")
        .unwrap()
        .quiet_zone(1);
    let mut row = Table::new();
    row.add_column("By service");
    row.add_column("Details");
    row.add_row_cells(vec![
        Cell::Renderable(Arc::new(traffic().height(6))),
        Cell::Renderable(Arc::new(code)),
    ]);
    console.print(&row);
    console.print(&quarters().bar_width(36));
}
// --8<-- [end:together]

fn main() {
    let shots = Shots::from_args();
    shots.shot("pie", 60, "PieChart", show_pie);
    shots.shot("stacked", 66, "Stacked bars", show_stacked);
    shots.shot("areas", 60, "Stacked areas", show_areas);
    shots.shot("qr", 60, "QrCode", show_qr);
    shots.shot("gradient", 60, "GradientText", show_gradient);
    shots.shot("together", 76, "Together", show_together);
}

/// `--svg DIR` writes each shot as `DIR/guide_charts_extra-<shot>.svg`;
/// without it, shots print to the terminal.
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
        let id = format!("guide_charts_extra-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}
