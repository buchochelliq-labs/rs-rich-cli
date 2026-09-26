//! Parity for the second renderables audit (core side), against
//! `golden/audit2.tsv` (captured by `AUDIT2_CASES` in
//! `scripts/capture_golden.py` from real rich 15.0.0): zero-width characters
//! past a crop, Markdown inheriting overflow / no-wrap / justify, the Windows
//! palette, blank lines of empty renderables, line separators in measurement,
//! an explicit `justify="default"`, GFM delimiter rows, markup error
//! positions, bare `grey`, and span rendering. Each fixture line is
//! `name<TAB>json(expected)`; every name has a Rust builder below.

use std::sync::Arc;

use rich::containers::Renderables;
use rich::errors::RichError;
use rich::markdown::Markdown;
use rich::table::Cell;
use rich::text::Span;
use rich::{
    Color, ColorSystem, Console, ConsoleOptions, Justify, Overflow, Panel, Renderable, Style,
    Syntax, Table, Text, Tree,
};

/// `_cg_console`: truecolor, no highlighting.
fn builder(width: usize) -> rich::console::ConsoleBuilder {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .highlight(false)
        .no_color(false)
        .legacy_windows(false)
}

fn print(console: &Console, renderable: &dyn Renderable) -> String {
    console.capture(|c| c.print(renderable))
}

/// `console.print(renderable, width=…, **options)`.
fn print_with(
    console: &Console,
    renderable: &dyn Renderable,
    width: usize,
    configure: impl FnOnce(&mut ConsoleOptions),
) -> String {
    let mut options = console.options();
    options.min_width = width;
    options.max_width = width;
    configure(&mut options);
    console.capture(|c| c.print_with(renderable, &options))
}

fn crop_zero_width() -> String {
    let mut table = Table::new()
        .show_header(false)
        .without_box()
        .padding(0, 0, 0, 0)
        .width(Some(19));
    table.add_column("");
    table.add_column("");
    table.add_row(&["", "\u{200b}[dim]r[/dim]"]);
    print(&builder(10).build(), &table)
}

fn markdown_overflow() -> String {
    let console = builder(12).build();
    let mut table = Table::new();
    table.add_column("h");
    table.add_row_cells(vec![Cell::Renderable(Arc::new(Markdown::new(
        "supercalifragilistic word",
    )))]);
    let mut out = print(&console, &table);
    out += &print_with(&console, &Markdown::new("hello"), 3, |o| {
        o.overflow = Some(Overflow::Crop)
    });
    out += &print_with(&console, &Markdown::new("hello world"), 8, |o| {
        o.no_wrap = Some(true);
        o.overflow = Some(Overflow::Ellipsis);
    });
    for justify in [Justify::Right, Justify::Center] {
        let image = Markdown::new("![alt](x)").hyperlinks(false);
        out += &print_with(&console, &image, 12, |o| o.justify = justify);
    }
    out
}

fn empty_blank_lines() -> String {
    let group = Renderables::new(vec![
        Arc::new(
            Text::new("")
                .justify(Justify::Left)
                .overflow(Overflow::Ignore),
        ),
        Arc::new(builder(10).build().build_text("x")),
    ]);
    let console = builder(10).build();
    let mut out = print(&console, &Tree::new(""));
    out += &print(&console, &Syntax::new("", "python").theme("ansi_dark"));
    out += &print(&console, &group);
    out
}

fn default_justify_column() -> String {
    let mut table = Table::new();
    table
        .add_column_justify("h", Justify::Right)
        .column_width(6);
    // `Text("x", justify="default")`: explicit, so the column cannot override it.
    let mut explicit = Text::new("x");
    explicit.set_justify_option(Some(Justify::Default));
    table.add_row_cells(vec![Cell::Text(explicit)]);
    table.add_row_cells(vec![Cell::Text(Text::new("y"))]);
    print(&builder(20).build(), &table)
}

fn markup_error_position() -> String {
    ["é中[/i]", ":smile: [/i]", "中[/]", ":smile:[b]:smile:[/i]"]
        .into_iter()
        .map(|source| match builder(40).build().try_build_text(source) {
            Ok(_) => "ok\n".to_string(),
            Err(RichError::Markup(message)) => format!("{message}\n"),
            Err(other) => panic!("{other:?}"),
        })
        .collect()
}

fn many_spans() -> String {
    let console = builder(30).highlight(true).build();
    let items: Vec<String> = (0..60).map(|n| n.to_string()).collect();
    let mut out = console.capture(|c| {
        c.print_str(&format!(
            "[{}]\n{{'a': 1, 'b': [True, None]}}",
            items.join(", ")
        ))
    });
    let span = |start, end, style: &str| Span {
        start,
        end,
        style: style.into(),
    };
    for spans in [
        vec![span(1, 1, "bold"), span(0, 4, "red")],
        vec![span(0, 5, "red"), span(2, 2, "bold"), span(3, 5, "blue")],
    ] {
        for plain in ["ab cd", "ab\ncd"] {
            let mut text = Text::new(plain);
            text.set_spans(spans.clone());
            out += &print(&console, &text);
        }
    }
    out
}

fn build(name: &str) -> String {
    match name {
        "crop_zero_width_past_edge" => crop_zero_width(),
        "markdown_inherits_overflow" => markdown_overflow(),
        "windows_palette" => {
            let console = builder(20).color_system(Some(ColorSystem::Windows)).build();
            console.capture(|c| {
                c.print_str(
                    "[#808080 on #82c9b0]x[/] [color(100)]y[/] [color(9) on color(200)]z[/] [red on bright_black]w",
                )
            })
        }
        "empty_blank_lines" => empty_blank_lines(),
        "unicode_line_separators" => {
            let console = builder(12).build();
            ["\u{2028}", "\u{2029}", "\u{1c}", "\u{85}", "\r"]
                .into_iter()
                .map(|separator| {
                    let text = console.build_text(&format!("a{separator}o"));
                    print(&console, &Panel::new(Box::new(text)).expand(false))
                })
                .collect()
        }
        "default_justify_column" => default_justify_column(),
        "panel_width0_empty" => print(
            &builder(5).build(),
            &Panel::new(Box::new(Text::new(""))).width(0),
        ),
        "gfm_invalid_delimiter_row" => ["a||\n-|:", "a|b\n-|:", "a|b\n-|-:", "a|b\n:|-"]
            .into_iter()
            .map(|source| print(&builder(10).build(), &Markdown::new(source)))
            .collect(),
        "markup_error_position" => markup_error_position(),
        "grey_is_not_a_colour" => {
            let mut out = builder(20)
                .build()
                .capture(|c| c.print_str("[grey]x[/] [on gray]y [gray50]z"));
            assert!(Color::parse("grey").is_err());
            assert!(Style::parse("on gray").is_err());
            out += "'grey' is not a valid color\n";
            out
        }
        "brackets_and_tildes" => print(
            &builder(30).build(),
            &Markdown::new("[[~~x~~]] a~[~b ~~[c]~~ [~~~d~~~]"),
        ),
        "many_spans" => many_spans(),
        other => panic!("no Rust builder for audit 2 case {other:?}"),
    }
}

#[test]
fn audit2_parity() {
    let data = include_str!("golden/audit2.tsv");
    let mut failures = Vec::new();
    let mut checked = 0;
    for raw in data.lines() {
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let (name, expected) = raw.split_once('\t').expect("name<TAB>json");
        let expected: String = serde_json::from_str(expected).expect("json string");
        let got = build(name);
        if got != expected {
            failures.push(format!(
                "{name}:\n  expected {expected:?}\n  got      {got:?}"
            ));
        }
        checked += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(checked, 12, "expected every audit 2 case to run");
}
