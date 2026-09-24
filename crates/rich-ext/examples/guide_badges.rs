//! Guide: Badges, size bars, formatters and redaction — run: cargo run -p rs-rich-ext --example guide_badges [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/badges-and-redaction.md` comes from this file.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use rich::{Cell, ColorSystem, Console, Panel, Table, Text};
use rich_ext::a11y::Status;
use rich_ext::badge::{Badge, Badges};
use rich_ext::format;
use rich_ext::redact::{Redacted, Redactor};
use rich_ext::size_bar::{SizeBar, Units};

// --8<-- [start:badges]
fn release_badges() -> Badges {
    Badges::new([
        Badge::status(Status::Ok, "build"),
        Badge::status(Status::Error, "tests"),
        Badge::status(Status::Warning, "lint"),
        Badge::status(Status::Pending, "deploy"),
        Badge::label("beta"),
        Badge::meta("version", "0.0.11"),
        Badge::link("docs", "https://example.com/docs"),
    ])
}

fn show_badges(console: &Console) {
    console.print(&release_badges());
}
// --8<-- [end:badges]

// --8<-- [start:plain]
fn show_plain(console: &Console) {
    // No colour system, NO_COLOR or no_color(true): brackets and tags
    // carry the meaning, and a link spells out its URL.
    let plain = Console::builder().width(60).color_system(None).build();
    console.print(&Text::new(plain.render_to_string(&release_badges())));
    // The same text for screen readers and logs.
    assert!(release_badges()
        .plain()
        .starts_with("[OK build] [ERROR tests]"));
}
// --8<-- [end:plain]

// --8<-- [start:sizes]
fn show_sizes(console: &Console) {
    let limit = 10_000_000;
    for (name, size) in [
        ("rs-rich", 2_400_000),
        ("rs-rich-ext", 9_300_000),
        ("rs-rich-art", 12_600_000),
    ] {
        console.print(&SizeBar::limit(size, limit).label(format!("{name:<12}")));
    }
    console.print(
        &SizeBar::new(3 << 30, 8 << 30)
            .label("disk        ")
            .units(Units::Binary),
    );
}
// --8<-- [end:sizes]

// --8<-- [start:table]
fn show_table(console: &Console) {
    let total = 6_000_000;
    let mut table = Table::new().title("Bundle");
    table.add_column("File");
    table.add_column("Size");
    table.add_column("Checks");
    for (file, size, ok) in [
        ("app.wasm", 3_900_000, true),
        ("vendor.js", 1_500_000, false),
        ("styles.css", 120_000, true),
    ] {
        let status = if ok { Status::Ok } else { Status::Warning };
        table.add_row_cells(vec![
            Cell::Markup(file.into()),
            Cell::Renderable(Arc::new(SizeBar::new(size, total).bar_width(12))),
            Cell::Renderable(Arc::new(Badge::status(status, "size"))),
        ]);
    }
    console.print(&table);
}
// --8<-- [end:table]

// --8<-- [start:format]
fn show_format(console: &Console) {
    let then = UNIX_EPOCH + Duration::from_secs(1_790_000_000);
    let rows = [
        ("bytes(1_500_000)", format::bytes(1_500_000)),
        ("bytes_binary(1_572_864)", format::bytes_binary(1_572_864)),
        ("rate(2_400_000.0)", format::rate(2_400_000.0)),
        (
            "duration(3723 s)",
            format::duration(Duration::from_secs(3723)),
        ),
        ("clock(3723 s)", format::clock(Duration::from_secs(3723))),
        (
            "relative(then, then + 3 h)",
            format::relative(then, then + Duration::from_secs(3 * 3600)),
        ),
        ("timestamp(then)", format::timestamp(then)),
        ("percent(0.4251, 1)", format::percent(0.4251, 1)),
        ("number(1234567)", format::number(1_234_567)),
        ("compact(1_250_000.0)", format::compact(1_250_000.0)),
    ];
    let mut table = Table::new();
    table.add_column("Call");
    table.add_column("Result");
    for (call, result) in rows {
        table.add_row(&[call, &result]);
    }
    console.print(&table);
}
// --8<-- [end:format]

// --8<-- [start:redact]
fn show_redact(console: &Console) {
    let log = Text::new(
        "GET /api?token=abc123 200\n\
         Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln\n\
         push with ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\n\
         DATABASE_URL=postgres://app:s3cret@db/app\n\
         order 1234-5678 shipped",
    );
    let redactor = Redactor::secrets()
        .named_pattern("order", r"order (?P<secret>\d{4})")
        .expect("valid pattern");
    // Masks keep the cells they replace, so the border stays put.
    console.print(&Redacted::new(
        Panel::new(Box::new(log)).title("server.log"),
        redactor,
    ));
}
// --8<-- [end:redact]

// --8<-- [start:export]
fn redacted_exports(console: &Console) -> (String, String) {
    let redactor = Redactor::secrets();
    let print = |c: &Console| c.print(&Text::new("password=hunter2"));
    // Record, redact, export: the secret never reaches the file.
    let svg = redactor.export_svg(console, "Redacted", "redacted", print);
    let html = redactor.export_html(console, print);
    // Or plain strings, such as log lines.
    assert_eq!(redactor.redact_str("api_key: abc"), "api_key: ********");
    (svg, html)
}
// --8<-- [end:export]

fn main() {
    let shots = Shots::from_args();
    shots.shot("badges", 80, "Badges", show_badges);
    shots.shot("plain", 80, "Plain badges", show_plain);
    shots.shot("sizes", 70, "SizeBar", show_sizes);
    shots.shot("table", 70, "Badges and bars in a table", show_table);
    shots.shot("format", 60, "rich_ext::format", show_format);
    shots.shot("redact", 72, "Redacted", show_redact);
    if !shots.svg() {
        let (svg, html) = redacted_exports(&Console::new());
        assert!(!svg.contains("hunter2") && !html.contains("hunter2"));
    }
}

/// `--svg DIR` writes each shot as `DIR/guide_badges-<shot>.svg`; without it,
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

    fn svg(&self) -> bool {
        self.dir.is_some()
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
        let id = format!("guide_badges-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}
