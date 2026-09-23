//! Guide: Layout — run: cargo run -p rs-rich --example guide_layout [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/layout.md are cut from this file.

#[path = "guide_support/mod.rs"]
mod guide_support;

use std::sync::Arc;

use guide_support::Shots;

// --8<-- [start:imports]
use rich::r#box::{self as boxes, Box as BoxSet, DOUBLE, HEAVY};
use rich::{
    Align, Cell, Columns, Console, Constrain, HorizontalAlign, Layout, Padding, Panel, Renderable,
    Rule, Style, Styled, Table, Text,
};
// --8<-- [end:imports]

fn main() {
    let shots = Shots::from_args("guide_layout");
    shots.shot("panel", 60, panels);
    shots.shot("padding", 60, padding);
    shots.shot("align", 60, align);
    shots.shot("columns", 60, columns);
    shots.shot("rule", 60, rules);
    shots.shot("layout", 72, layout);
    shots.shot("constrain", 60, constrain_and_style);
    shots.shot("boxes", 100, box_gallery);
}

// --8<-- [start:panel]
fn panels(console: &Console) {
    // The smallest panel: any renderable, boxed.
    console.print(&Panel::new(Box::new(Text::new("Hello from a panel"))));

    // Titles and subtitles are markup, and can be aligned.
    let body =
        Text::from_markup("Panels take [b]any[/b] renderable,\nincluding other panels.").unwrap();
    let panel = Panel::new(Box::new(body))
        .title("[bold]Title[/]")
        .title_align(HorizontalAlign::Left)
        .subtitle("subtitle")
        .subtitle_align(HorizontalAlign::Right)
        .box_set(DOUBLE)
        .padding((1, 4, 1, 4)) // top, right, bottom, left
        .border_style(Style::parse("cyan").unwrap());
    console.print(&panel);
}
// --8<-- [end:panel]

// --8<-- [start:padding]
fn padding(console: &Console) {
    let text = || Box::new(Text::new("padded")) as Box<dyn Renderable>;
    let shaded = Style::parse("on grey23").unwrap();

    console.print(&Padding::new(text(), (1, 2, 1, 8)).style(shaded.clone()));
    console.print(&Padding::uniform(text(), 1).style(shaded.clone()));
    console.print(&Padding::symmetric(text(), 0, 4).style(shaded));
}
// --8<-- [end:padding]

// --8<-- [start:align]
fn align(console: &Console) {
    let label = |s: &str| Box::new(Text::styled(s.to_string(), "reverse")) as Box<dyn Renderable>;
    console.print(&Align::left(label(" left ")));
    console.print(&Align::center(label(" center ")));
    console.print(&Align::right(label(" right ")));

    // Align works on any renderable with a natural width, such as a table.
    let mut table = Table::new();
    table.add_column("centred table");
    table.add_row(&["cell"]);
    console.print(&Align::center(Box::new(table)));
}
// --8<-- [end:align]

// --8<-- [start:columns]
fn columns(console: &Console) {
    let crates: Vec<String> = [
        "rs-rich",
        "rs-rich-ext",
        "rs-rich-cli",
        "rs-rich-art",
        "rs-rich-macros",
        "syntect",
        "serde_json",
        "pulldown-cmark",
        "fancy-regex",
        "terminal_size",
    ]
    .iter()
    .map(|name| name.to_string())
    .collect();
    // As many columns as fit, filled row by row.
    console.print(&Columns::new(crates));
}
// --8<-- [end:columns]

// --8<-- [start:rule]
fn rules(console: &Console) {
    console.print(&Rule::line());
    console.print(&Rule::new("[b]Centred title[/b]"));
    console.print(&Rule::new("Left").align(HorizontalAlign::Left));
    console.print(
        &Rule::new("Custom")
            .align(HorizontalAlign::Right)
            .characters("=-")
            .style(Style::parse("magenta").unwrap()),
    );
}
// --8<-- [end:rule]

// --8<-- [start:layout]
fn layout(console: &Console) {
    let panel = |title: &str, body: &str| -> Layout {
        Layout::with_renderable(Box::new(
            Panel::new(Box::new(Text::new(body.to_string()))).title(title.to_string()),
        ))
    };

    let mut body = Layout::new();
    body.split_row(vec![
        panel("Sidebar", "ratio 1").ratio(1),
        panel("Main", "ratio 3, so three times as wide").ratio(3),
    ]);

    let mut root = Layout::new();
    root.split_column(vec![
        panel("Header", "size 3: exactly three rows").size(3),
        body.ratio(1).minimum_size(4), // takes what is left
        Layout::with_renderable(Box::new(Text::styled("footer: size 1", "dim"))).size(1),
    ]);

    // A layout fills the height it is given: the console's height by default,
    // or an explicit height in the render options.
    let mut options = console.options();
    options.height = Some(12);
    console.print_with(&root, &options);
}
// --8<-- [end:layout]

// --8<-- [start:constrain]
fn constrain_and_style(console: &Console) {
    // Constrain caps the width a child may use. Panels fill the width they
    // are given, so this is how to get a narrow one.
    let panel = Panel::new(Box::new(Text::new("at most 30 cells wide")));
    console.print(&Constrain::new(Box::new(panel), Some(30)));

    // Styled lays a style under everything its child renders.
    let panel = Panel::new(Box::new(
        Text::from_markup("[b]white on blue[/b], borders too").unwrap(),
    ));
    let styled = Styled::new(Box::new(panel), Style::parse("white on dark_blue").unwrap());
    console.print(&Constrain::new(Box::new(styled), Some(40)));
}
// --8<-- [end:constrain]

// --8<-- [start:boxes]
fn box_gallery(console: &Console) {
    let sets: [(&str, BoxSet); 20] = [
        ("ASCII", boxes::ASCII),
        ("ASCII2", boxes::ASCII2),
        ("ASCII_DOUBLE_HEAD", boxes::ASCII_DOUBLE_HEAD),
        ("SQUARE", boxes::SQUARE),
        ("SQUARE_DOUBLE_HEAD", boxes::SQUARE_DOUBLE_HEAD),
        ("MINIMAL", boxes::MINIMAL),
        ("MINIMAL_HEAVY_HEAD", boxes::MINIMAL_HEAVY_HEAD),
        ("MINIMAL_DOUBLE_HEAD", boxes::MINIMAL_DOUBLE_HEAD),
        ("SIMPLE", boxes::SIMPLE),
        ("SIMPLE_HEAD", boxes::SIMPLE_HEAD),
        ("SIMPLE_HEAVY", boxes::SIMPLE_HEAVY),
        ("HORIZONTALS", boxes::HORIZONTALS),
        ("ROUNDED", boxes::ROUNDED),
        ("HEAVY", HEAVY),
        ("HEAVY_EDGE", boxes::HEAVY_EDGE),
        ("HEAVY_HEAD", boxes::HEAVY_HEAD),
        ("DOUBLE", DOUBLE),
        ("DOUBLE_EDGE", boxes::DOUBLE_EDGE),
        ("MARKDOWN", boxes::MARKDOWN),
        ("NONE", boxes::NONE),
    ];
    // Each sample is a small table, so the header row separator shows too.
    let mut grid = Table::grid().padding(0, 2, 1, 0);
    for _ in 0..4 {
        grid.add_column("");
    }
    for chunk in sets.chunks(4) {
        let row = chunk
            .iter()
            .map(|(name, set)| {
                let mut sample = Table::new().box_set(*set).show_lines(true);
                sample.add_column(*name);
                sample.add_row(&["cell"]);
                sample.add_row(&["cell"]);
                Cell::Renderable(Arc::new(sample))
            })
            .collect();
        grid.add_row_cells(row);
    }
    console.print(&grid);
}
// --8<-- [end:boxes]
