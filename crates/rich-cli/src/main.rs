//! `rich` — a Rust port of the `rich-cli` terminal toolbox.
//!
//! This binary mirrors the upstream `rich-cli` command-line tool and is built on
//! the [`rich`] library crate. The first slice implements argument handling, a
//! plain-file printer, and a capability demo; the rich rendering subcommands
//! (markdown, syntax, csv/json, …) are tracked as roadmap issues.

use std::process::ExitCode;

use rich::text::Text;
use rich::{ColorSystem, Console, Panel, Rule, Table};
use rich_ext::ConsoleExt;

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
    console.print_str("");
    console.print_str("Markup:   [bold]bold[/], [italic]italic[/], [red]red[/], [green]green on[/] [white on blue]bg[/]");
    console.print_str("Theme:    [error]error[/], [warning]warning[/], [info]info[/]");
    console.print_str("Extension: numbers like 3.14 and 2026 are auto-highlighted");
    console.print_str("");
    console.print(
        &Panel::new(Box::new(Text::new(
            "Panels, rules, and padding now render.",
        )))
        .title("panel"),
    );
    console.print_str("");
    let mut table = Table::new();
    table.add_column("Feature");
    table.add_column("Status");
    table.add_row(&["markup", "done"]);
    table.add_row(&["panel / rule", "done"]);
    table.add_row(&["table", "done"]);
    console.print(&table);
}
