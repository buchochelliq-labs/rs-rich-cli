//! `rich` — a Rust port of the `rich-cli` terminal toolbox.
//!
//! This binary mirrors the upstream `rich-cli` command-line tool and is built on
//! the [`rich`] library crate. The first slice implements argument handling, a
//! plain-file printer, and a capability demo; the rich rendering subcommands
//! (markdown, syntax, csv/json, …) are tracked as roadmap issues.

use std::process::ExitCode;

use rich::r#box::{DOUBLE, SQUARE};
use rich::text::Text;
use rich::{
    filesize, Align, ColorSystem, Columns, Console, Constrain, HorizontalAlign, Padding, Panel,
    ProgressBar, Renderable, Rule, Spinner, Table, Tree,
};
use rich_ext::ConsoleExt;

/// Boxed `Text` helper to cut down on `Box::new(Text::new(...))` noise.
fn text(content: &str) -> Box<dyn Renderable> {
    Box::new(Text::new(content))
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut no_color = false;
    let mut path: Option<String> = None;

    for arg in &args {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("rich (rs-rich-cli) {VERSION}");
                return ExitCode::SUCCESS;
            }
            "--no-color" => no_color = true,
            other if other.starts_with('-') => {
                eprintln!("rich: unknown option {other:?} (try --help)");
                return ExitCode::FAILURE;
            }
            other => path = Some(other.to_string()),
        }
    }

    let mut console = Console::builder().no_color(no_color).build();
    console.install_extensions();

    match path {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(contents) => {
                console.print_str(&format!("[bold]── {path} ──[/]"));
                // Print file contents as *plain* text (no markup interpretation),
                // so arbitrary file bytes aren't treated as tags.
                console.print(&Text::new(contents));
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("rich: cannot read {path:?}: {err}");
                ExitCode::FAILURE
            }
        },
        None => {
            run_demo();
            ExitCode::SUCCESS
        }
    }
}

fn print_help() {
    println!(
        "rich {VERSION} — Rust port of the rich-cli terminal toolbox\n\
\n\
USAGE:\n\
    rich [OPTIONS] [FILE]\n\
\n\
OPTIONS:\n\
    -h, --help       Show this help\n\
    -V, --version    Show the version (mirrors upstream rich-cli)\n\
        --no-color   Disable colored output\n\
\n\
With no FILE, a capability demo is shown.\n\
\n\
PLANNED SUBCOMMANDS (tracked as roadmap issues, not yet implemented):\n\
    markdown, syntax, json, csv/tsv, rule, panel, padding, ipynb, export-html\n"
    );
}

fn run_demo() {
    // Force truecolor so the demo looks the same regardless of TERM.
    let mut console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    console.install_extensions();

    console.print(&Rule::new("rs-rich-cli"));
    console.print_str("[bold magenta]rs-rich-cli[/] — a Rust port of [italic]rich[/]");
    console.print_str("Everything below is byte-parity-tested against Python rich 15.0.0.");

    // Markup, color, theme, and the rich-ext highlighter.
    console.print(&Rule::new("markup · color · theme · extension"));
    console.print_str(
        "Styles:   [bold]bold[/] [dim]dim[/] [italic]italic[/] [underline]underline[/] [reverse]reverse[/]",
    );
    console.print_str(
        "Color:    [red]red[/] [green]green[/] [blue]blue[/] [#ff8800]#ff8800[/] [white on blue]on blue[/]",
    );
    console.print_str("Theme:    [error]error[/], [warning]warning[/], [info]info[/]");
    console.print_str("Extension: numbers like 3.14 and 2026 are auto-highlighted");

    // Rule variants (title alignment).
    console.print(&Rule::new("rule"));
    console.print(&Rule::line());
    console.print(&Rule::new("centered"));
    console.print(&Rule::new("left").align(HorizontalAlign::Left));
    console.print(&Rule::new("right").align(HorizontalAlign::Right));

    // Panels with different boxes.
    console.print(&Rule::new("panel"));
    console.print(&Panel::new(text("rounded box (default)")).title("rounded"));
    console.print(
        &Panel::new(text("square box"))
            .box_set(SQUARE)
            .title("square"),
    );
    console.print(
        &Panel::new(text("double box"))
            .box_set(DOUBLE)
            .title("double"),
    );
    console.print(
        &Panel::new(text("title + subtitle"))
            .title("top")
            .subtitle("bottom"),
    );

    // Padding (shown inside a panel so the blank space is visible).
    console.print(&Rule::new("padding"));
    console.print(
        &Panel::new(Box::new(Padding::new(text("padded (1, 4)"), (1, 4, 1, 4)))).title("padding"),
    );

    // Horizontal alignment (fills the console width).
    console.print(&Rule::new("align"));
    console.print(&Align::left(text("← left")));
    console.print(&Align::center(text("center")));
    console.print(&Align::right(text("right →")));

    // Constrain — same panel, capped to 24 cells.
    console.print(&Rule::new("constrain (width 24)"));
    console.print(&Constrain::new(
        Box::new(Panel::new(text("constrained")).title("≤24")),
        Some(24),
    ));

    // Table.
    console.print(&Rule::new("table"));
    let mut table = Table::new();
    table.add_column("Renderable");
    table.add_column("Module");
    table.add_column("Parity");
    for (renderable, module, parity) in [
        ("Text / markup", "text, markup", "✓"),
        ("Rule", "rule", "✓"),
        ("Panel", "panel", "✓"),
        ("Padding", "padding", "✓"),
        ("Align", "align", "✓"),
        ("Constrain", "constrain", "✓"),
        ("Columns", "columns", "✓"),
        ("Table", "table", "✓"),
        ("Tree", "tree", "✓"),
        ("ProgressBar", "progress_bar", "✓"),
    ] {
        table.add_row(&[renderable, module, parity]);
    }
    console.print(&table);

    // Columns — packs items into as many columns as fit the width.
    console.print(&Rule::new("columns"));
    let months = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    console.print(&Columns::new(
        months.iter().map(|m| m.to_string()).collect(),
    ));

    // Tree.
    console.print(&Rule::new("tree"));
    let mut tree = Tree::new("rs-rich-cli");
    let core = tree.add("crates/rich (core, mirrors rich 15.0.0)");
    core.add("color · style · text · console");
    core.add("box · rule · panel · padding · align");
    core.add("table · tree · wrap · filesize");
    tree.add("crates/rich-ext (our extensions)");
    tree.add("crates/rich-cli (this binary)");
    console.print(&tree);

    // Progress bars at a few completion levels (0 → 100%).
    console.print(&Rule::new("progress bar"));
    for pct in [0.0, 33.0, 66.0, 100.0] {
        console.print(&ProgressBar::new(100.0, pct).width(48));
    }

    // Spinners — a few frames of each built-in (animation needs a Live loop).
    console.print(&Rule::new("spinner (frames)"));
    for name in ["dots", "line", "arrow", "simpleDots"] {
        let spinner = Spinner::new(name);
        let frames: String = (0..6)
            .map(|i| console.render_to_string(&spinner.render(i as f64 * 0.15)))
            .collect::<Vec<_>>()
            .join(" ");
        console.print_str(&format!("  {name:>11}: {frames}"));
    }

    // filesize.
    console.print(&Rule::new("filesize"));
    for bytes in [1u64, 999, 1_000, 1_500, 1_000_000, 1_500_000_000] {
        console.print_str(&format!("  {bytes:>13} → {}", filesize::decimal(bytes)));
    }
    console.print(&Rule::line());
}
