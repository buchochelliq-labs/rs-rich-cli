//! Guide: Tables — run: cargo run -p rs-rich --example guide_tables [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/tables.md are cut from this file.

#[path = "guide_support/mod.rs"]
mod guide_support;

use std::sync::Arc;

use guide_support::Shots;

// --8<-- [start:imports]
use rich::r#box::{DOUBLE_EDGE, ROUNDED, SIMPLE_HEAD, SQUARE};
use rich::{Cell, ColumnOptions, Console, Justify, Overflow, ProgressBar, Style, Table, Text};
// --8<-- [end:imports]

fn main() {
    let shots = Shots::from_args("guide_tables");
    shots.shot("basic", 60, basic);
    shots.shot("columns", 60, column_options);
    shots.shot("expand", 60, expand);
    shots.shot("styling", 60, styling);
    shots.shot("cells", 60, rich_cells);
    shots.shot("grid", 60, grid);
    shots.shot("nested", 60, nested);
    shots.shot("edges", 60, edges);
}

// --8<-- [start:basic]
fn basic(console: &Console) {
    let mut table = Table::new()
        .title("Star Wars Movies")
        .caption("Box office, USD");

    table.add_column("Released");
    table.add_column("Title");
    table.add_column_justify("Box Office", Justify::Right);

    table.add_row(&["Dec 20, 2019", "The Rise of Skywalker", "$952,110,690"]);
    table.add_row(&["May 25, 2018", "Solo", "$393,151,347"]);
    table.add_row(&["Dec 15, 2017", "The Last Jedi", "$1,332,539,889"]);

    console.print(&table);
}
// --8<-- [end:basic]

// --8<-- [start:columns]
fn column_options(console: &Console) {
    let mut table = Table::new();

    // Chain column_* setters after add_column; they modify the last column.
    table
        .add_column("Id")
        .column_style(Style::parse("dim").unwrap());
    table.add_column("Name").column_min_width(10);
    table
        .add_column("Notes")
        .column_max_width(22)
        .column_overflow(Overflow::Fold);

    // Or set everything at once, as upstream's add_column(**kwargs).
    table.add_column_with(
        Text::styled("Size", "bold cyan"),
        ColumnOptions {
            justify: Justify::Right,
            width: Some(7),
            no_wrap: true,
            style: Style::parse("green").unwrap(),
            ..ColumnOptions::default()
        },
    );

    table.add_row(&["1", "alpha", "short", "1.2 kB"]);
    table.add_row(&[
        "2",
        "bravo",
        "a much longer note that has to wrap inside its column",
        "310.4 MB",
    ]);
    console.print(&table);
}
// --8<-- [end:columns]

// --8<-- [start:expand]
fn expand(console: &Console) {
    // expand(true) fills the width; ratio columns share the spare space.
    let mut table = Table::new().expand(true);
    table.add_column("Key").column_width(8);
    table.add_column("1 share").column_ratio(1);
    table.add_column("2 shares").column_ratio(2);
    table.add_row(&["fixed", "ratio 1", "ratio 2"]);
    console.print(&table);
}
// --8<-- [end:expand]

// --8<-- [start:styling]
fn styling(console: &Console) {
    let mut table = Table::new()
        .box_set(ROUNDED)
        .border_style(Style::parse("bright_blue").unwrap())
        .show_lines(true) // a rule between every row
        .title("[b]Deploys[/b] :rocket:"); // titles are markup

    table.add_column("Service");
    table
        .add_column("State")
        .column_header_style(Style::parse("magenta").unwrap());
    table
        .add_column_justify("Latency", Justify::Right)
        .column_header_fill(Style::parse("on grey23").unwrap());

    // Styled cells: build a Text (markup, or spans) per cell.
    let cell = |markup: &str| Text::from_markup(markup).unwrap();
    table.add_row_text(vec![cell("api"), cell("[green]up[/]"), cell("12 ms")]);
    table.add_row_text(vec![
        cell("worker"),
        cell("[yellow]degraded[/]"),
        cell("840 ms"),
    ]);
    table.add_row_text(vec![cell("billing"), cell("[bold red]down[/]"), cell("—")]);
    console.print(&table);
}
// --8<-- [end:styling]

// --8<-- [start:cells]
fn rich_cells(console: &Console) {
    let mut table = Table::new();
    table.add_column("Task");
    table.add_column("Progress").column_width(20);

    for (name, done) in [("download", 80.0), ("extract", 35.0), ("verify", 0.0)] {
        table.add_row_cells(vec![
            Cell::from(name),
            // Any Send + Sync renderable can be a cell.
            Cell::Renderable(Arc::new(ProgressBar::new(100.0, done))),
        ]);
    }
    console.print(&table);
}
// --8<-- [end:cells]

// --8<-- [start:grid]
fn grid(console: &Console) {
    // No borders, no header, no padding: a layout tool.
    let mut grid = Table::grid().expand(true);
    grid.add_column("");
    grid.add_column_justify("", Justify::Right);
    grid.add_row_text(vec![
        Text::styled("rs-rich", "bold"),
        Text::styled("v0.0.7", "dim"),
    ]);
    grid.add_row(&["left-aligned", "right-aligned"]);
    console.print(&grid);
}
// --8<-- [end:grid]

// --8<-- [start:nested]
fn nested(console: &Console) {
    let mut inner = Table::new().box_set(SIMPLE_HEAD);
    inner.add_column("k");
    inner.add_column("v");
    inner.add_row(&["cpu", "4"]);
    inner.add_row(&["mem", "8 GB"]);

    let mut outer = Table::new().box_set(DOUBLE_EDGE);
    outer.add_column("Host");
    // A nested table asks for the full width, so pin the column.
    outer.add_column("Resources").column_width(16);
    outer.add_row_cells(vec![Cell::from("web-1"), Cell::Renderable(Arc::new(inner))]);
    console.print(&outer);
}
// --8<-- [end:nested]

// --8<-- [start:edges]
fn edges(console: &Console) {
    let build = || {
        let mut t = Table::new().box_set(SQUARE);
        t.add_column("a");
        t.add_column("b");
        t.add_row(&["1", "2"]);
        t
    };
    console.print(&build().show_edge(false));
    console.print(&build().show_header(false).pad_edge(false));
    console.print(&build().padding(0, 3, 0, 3).collapse_padding(true));
}
// --8<-- [end:edges]
