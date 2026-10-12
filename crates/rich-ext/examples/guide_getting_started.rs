//! Guide: Getting started — run: cargo run -p rs-rich-ext --example guide_getting_started [-- --svg docs/media/guide]
//!
//! The snippets in docs/getting-started.md and the overview pictures in
//! docs/guide/core/index.md are cut from this file.

#[path = "guide_support/mod.rs"]
mod guide_support;

use guide_support::Shots;

use rich::{ColorSystem, Console, Panel, Table};

fn main() {
    let shots = Shots::from_args("guide_getting_started");
    shots.shot("hello", 40, hello);
    controlling();
    capturing();
    shots.shot("nesting", 40, nesting);
}

// --8<-- [start:hello]
fn hello(console: &Console) {
    console.print_str("[bold magenta]Hello[/] [green]World[/]");
}
// --8<-- [end:hello]

// --8<-- [start:controlling]
fn controlling() {
    let console = Console::builder()
        .force_terminal(true) // style even when redirected
        .color_system(Some(ColorSystem::Truecolor))
        .width(80) // ignore the real width
        .build();
    assert_eq!(console.width(), 80);
}
// --8<-- [end:controlling]

// --8<-- [start:capturing]
fn capturing() {
    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let ansi = console.capture(|c| c.print_str("[bold]hi[/]"));
    assert_eq!(ansi, "\x1b[1mhi\x1b[0m\n");
}
// --8<-- [end:capturing]

// --8<-- [start:nesting]
fn nesting(console: &Console) {
    let mut table = Table::new();
    table.add_column("crate");
    table.add_column("imported as");
    table.add_row(&["rs-rich", "[bold]rich[/]"]);
    table.add_row(&["rs-rich-ext", "[bold]rich_ext[/]"]);

    // A table in a panel: the panel measures the table, then renders it
    // inside its border, through the same Renderable protocol.
    console.print(&Panel::fit(Box::new(table)).title("[b]Renderables nest[/]"));
}
// --8<-- [end:nesting]
