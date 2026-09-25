//! Parity for the last Rich options core lacked, against
//! `golden/api_gaps.tsv` (captured by `API_GAP_CASES` in
//! `scripts/capture_golden.py` from real rich 15.0.0): Table's width,
//! footers, leading, row styles, sections and annotation styles, Panel's
//! `Text` subtitle and `safe_box`, Syntax's signed `start_line` /
//! `line_range`, and `Layout.refresh_screen`. Each fixture line is
//! `name<TAB>json(expected)`; every name has a Rust builder below.

use std::sync::Arc;

use rich::r#box::{
    ASCII2, ASCII_DOUBLE_HEAD, DOUBLE_EDGE, HEAVY, MARKDOWN, MINIMAL, MINIMAL_DOUBLE_HEAD, ROUNDED,
    SIMPLE,
};
use rich::region::Region;
use rich::{
    Cell, ColorSystem, Columns, Console, Justify, Layout, Panel, Renderable, Style, StyleType,
    Syntax, Table, Text, Theme,
};

/// `_cg_console`: truecolor, no highlighting.
fn builder(width: usize) -> rich::console::ConsoleBuilder {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .highlight(false)
        .no_color(false)
}

fn console(width: usize) -> Console {
    builder(width).build()
}

fn print(console: &Console, renderable: &dyn Renderable) -> String {
    console.capture(|c| c.print(renderable))
}

fn print_all(console: &Console, renderables: &[&dyn Renderable]) -> String {
    renderables
        .iter()
        .map(|renderable| print(console, *renderable))
        .collect()
}

fn style(definition: &str) -> Style {
    Style::parse(definition).unwrap()
}

fn text(plain: &str) -> Box<dyn Renderable> {
    Box::new(Text::new(plain))
}

/// `_ag_table(**options)`: the options are applied by `configure`.
fn ag_table(configure: impl FnOnce(Table) -> Table) -> Table {
    let mut table = configure(Table::new());
    table
        .add_column("Name")
        .column_footer("Total")
        .column_footer_fill(style("green"));
    table
        .add_column_justify("Qty", Justify::Right)
        .column_footer("12");
    table.add_row(&["apple", "3"]);
    table.add_row(&["banana split", "4"]);
    table.add_row(&["cherry", "5"]);
    table
}

fn ag_ratio_table(width: usize) -> Table {
    let mut table = Table::new().width(Some(width));
    table.add_column("fixed");
    table.add_column("one").column_ratio(1);
    table.add_column("two").column_ratio(2);
    table.add_row(&["a", "b", "c"]);
    table
}

fn ag_sections() -> Table {
    let mut table = Table::new().title("Sections").show_footer(true);
    table.add_column("a").column_footer("A");
    table.add_column("b").column_footer("B");
    table.add_row_with(vec!["1".into(), "one".into()], None, true);
    table.add_row_with(
        vec!["2".into(), "two".into()],
        Some(StyleType::from("on blue")),
        false,
    );
    table.add_section();
    table.add_row_with(
        vec!["3".into(), "three".into()],
        Some(StyleType::from("bold")),
        false,
    );
    table.add_row(&["4", "four"]);
    table
}

fn ag_extra_cells() -> Table {
    let mut table = Table::new();
    table.add_column("x");
    table.add_row(&["1"]);
    table.add_row(&["2", "extra", "more"]);
    table
}

fn ag_annotations(configure: impl FnOnce(Table) -> Table) -> Table {
    let mut table = configure(Table::new().title("The [b]title[/b]").caption("a caption"));
    table.add_column("column one");
    table.add_column("two");
    table.add_row(&["x", "y"]);
    table
}

const UF_CODE: &str =
    "def f(x):\n    if x:\n        return 'a very long line of code'\n\n    return x\n";

/// `_AG_SYNTAX_OPTIONS`, in order.
fn syntax_options() -> Vec<fn(Syntax) -> Syntax> {
    vec![
        |s| s.line_numbers(true).start_line(0),
        |s| s.line_numbers(true).start_line(-3).highlight_lines([-2, 0]),
        |s| s.line_numbers(true).start_line(-12),
        |s| s.line_numbers(true).line_range(Some(0), Some(2)),
        |s| s.line_numbers(true).line_range(Some(-2), Some(3)),
        |s| s.line_numbers(true).line_range(Some(2), Some(-1)),
        |s| s.line_numbers(true).line_range(Some(1), Some(-2)),
        |s| s.line_numbers(true).line_range(None, Some(-1)),
        |s| s.line_numbers(true).line_range(Some(3), Some(0)),
        |s| s.line_numbers(true).line_range(None, None),
        |s| s.line_numbers(true).line_range(Some(4), Some(99)),
        |s| s.line_range(Some(2), Some(-1)),
        |s| s.line_range(Some(-5), None).word_wrap(true),
        |s| {
            s.line_numbers(true)
                .start_line(-1)
                .line_range(Some(2), Some(4))
                .word_wrap(true)
        },
    ]
}

/// Python's `repr` of a `str` holding no backslashes or control codes but
/// newlines.
fn py_repr(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in value.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\'' if quote == '\'' => out.push_str("\\'"),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn ag_layout() -> Layout {
    let mut layout = Layout::new().name("root");
    layout.split_row(vec![
        Layout::with_renderable(Box::new(Panel::new(text("left")))).name("a"),
        Layout::new().name("right"),
    ]);
    layout["right"].split_column(vec![
        Layout::with_renderable(text("top")).name("b"),
        Layout::with_renderable(text("bottom")).name("c"),
    ]);
    layout
}

fn ag_refresh_screen() -> String {
    let console = builder(20).height(6).build();
    let mut layout = ag_layout();
    let mut out = String::new();
    let mut refused = false;
    let first = console.capture(|c| {
        c.print(&layout);
        refused = matches!(
            layout.refresh_screen(c, "b"),
            Err(rich::RichError::NoAltScreen(_))
        );
    });
    if refused {
        out.push_str("<NoAltScreen>");
    }
    out.push_str(&first);
    let mut missing = false;
    let second = console.capture(|c| {
        c.set_alt_screen(true);
        c.print(&layout);
        layout["b"].update(Box::new(Text::styled("changed", style("bold"))));
        layout.refresh_screen(c, "b").unwrap();
        layout["a"].update(Box::new(Panel::new(text("new")).title("t")));
        layout.refresh_screen(c, "a").unwrap();
        missing = !layout.refresh_screen(c, "right").unwrap();
        c.update_screen(&Text::new("region"), Some(Region::new(3, 1, 6, 2)), None)
            .unwrap();
        c.set_alt_screen(false);
    });
    if missing {
        out.push_str("<KeyError>");
    }
    out.push_str(&second);
    out
}

fn build(name: &str) -> String {
    match name {
        "table_width" => print_all(
            &console(50),
            &[
                &ag_table(|t| t.width(Some(30))),
                &ag_table(|t| t.width(Some(12))),
                &ag_table(|t| t.width(Some(45)).show_edge(false)),
                &ag_table(|t| t.width(Some(10)).without_box()),
            ],
        ),
        "table_width_ratio" => print(&console(50), &ag_ratio_table(40)),
        "table_min_width" => print_all(
            &console(50),
            &[
                &ag_table(|t| t.min_width(Some(40))),
                &ag_table(|t| t.min_width(Some(5))),
                &ag_table(|t| t.min_width(Some(80))),
                &ag_table(|t| t.min_width(Some(30)).expand(true)),
            ],
        ),
        "table_width_measure" => {
            let columns = Columns::from_cells(vec![
                Cell::Renderable(Arc::new(ag_table(|t| t.width(Some(20))))),
                Cell::Renderable(Arc::new(ag_table(|t| t.min_width(Some(24))))),
            ]);
            print_all(
                &console(50),
                &[
                    &Panel::fit(Box::new(ag_table(|t| t.width(Some(28))))),
                    &Panel::fit(Box::new(ag_table(|t| t.min_width(Some(36))))),
                    &columns,
                ],
            )
        }
        "table_footer" => print_all(
            &console(40),
            &[
                &ag_table(|t| t.show_footer(true)),
                &ag_table(|t| t.show_footer(true).show_edge(false)),
                &ag_table(|t| t.show_footer(true).show_lines(true)),
                &ag_table(|t| t.show_footer(true).without_box()),
                &ag_table(|t| t.show_footer(true).show_header(false).box_set(SIMPLE)),
                &ag_table(|t| {
                    t.show_footer(true)
                        .footer_style("italic red")
                        .box_set(DOUBLE_EDGE)
                }),
                &ag_table(|t| t.show_footer(true).footer_style("").padding(1, 1, 1, 1)),
                &ag_table(|t| {
                    t.show_footer(true)
                        .box_set(MINIMAL_DOUBLE_HEAD)
                        .pad_edge(false)
                }),
            ],
        ),
        "table_leading" => print_all(
            &console(40),
            &[
                &ag_table(|t| t.leading(1)),
                &ag_table(|t| t.leading(2).width(Some(18))),
                &ag_table(|t| t.leading(1).show_footer(true).show_edge(false)),
                &ag_table(|t| t.leading(1).box_set(SIMPLE)),
            ],
        ),
        "table_sections" => print(&console(40), &ag_sections()),
        "table_row_styles" => print_all(
            &console(40),
            &[
                &ag_table(|t| t.row_styles(vec!["".into(), "on blue".into()])),
                &ag_table(|t| {
                    t.row_styles(vec!["red".into(), "green".into(), "italic".into()])
                        .box_set(SIMPLE)
                }),
                &ag_table(|t| {
                    t.row_styles(vec!["on red".into()])
                        .box_set(MINIMAL)
                        .show_footer(true)
                }),
            ],
        ),
        "table_row_style_markup" => {
            let console = builder(40)
                .theme(Theme::from_styles([("zebra", "on magenta")], true).unwrap())
                .build();
            print(
                &console,
                &ag_table(|t| {
                    t.row_styles(vec!["zebra".into(), "none".into()])
                        .header_style("zebra")
                }),
            )
        }
        "table_header_style" => print_all(
            &console(40),
            &[
                &ag_table(|t| t.header_style("magenta")),
                &ag_table(|t| t.header_style("")),
                &ag_table(|t| {
                    t.header_style("bold on blue")
                        .show_footer(true)
                        .footer_style("underline")
                }),
            ],
        ),
        "table_annotation_styles" => print_all(
            &console(40),
            &[
                &ag_annotations(|t| t.title_style("bold red").caption_style("green")),
                &ag_annotations(|t| {
                    t.title_justify(Justify::Left)
                        .caption_justify(Justify::Right)
                }),
                &ag_annotations(|t| {
                    t.title_justify(Justify::Right)
                        .caption_justify(Justify::Left)
                }),
                &ag_annotations(|t| {
                    t.title_justify(Justify::Full)
                        .caption_justify(Justify::Full)
                        .width(Some(16))
                }),
                &ag_annotations(|t| t.title_justify(Justify::Default)),
            ],
        ),
        "table_text_annotations" => {
            let mut first = Table::new()
                .title_text(Text::styled("Text title", style("blue")))
                .caption_text(Text::new("right").justify(Justify::Right))
                .title_style("red");
            first.add_column("a");
            first.add_column("b");
            let mut second = Table::new()
                .title_text(Text::new("t").justify(Justify::Left))
                .title_justify(Justify::Right)
                .caption_text(Text::new(""));
            second.add_column("a");
            print_all(&console(40), &[&first, &second])
        }
        "table_extra_cells" => print(&console(40), &ag_extra_cells()),
        "safe_box" => {
            let safe = builder(30).legacy_windows(true).safe_box(true).build();
            let unsafe_ = builder(30).legacy_windows(true).safe_box(false).build();
            print_all(
                &safe,
                &[
                    &ag_table(|t| t),
                    &ag_table(|t| t.safe_box(Some(false))),
                    &Panel::new(text("p")),
                    &Panel::new(text("p")).safe_box(Some(false)),
                ],
            ) + &print_all(
                &unsafe_,
                &[
                    &ag_table(|t| t.safe_box(Some(true))),
                    &Panel::new(text("p")).safe_box(Some(true)),
                    &Panel::new(text("p")).box_set(HEAVY),
                ],
            )
        }
        "ascii_boxes" => print_all(
            &builder(30).ascii_only(true).build(),
            &[
                &ag_table(|t| t.box_set(ASCII2)),
                &ag_table(|t| t.box_set(ASCII_DOUBLE_HEAD)),
                &ag_table(|t| t.box_set(MARKDOWN)),
                &ag_table(|t| t.box_set(ROUNDED)),
                &Panel::new(text("p")).box_set(ASCII2),
            ],
        ),
        "panel_text_subtitle" => {
            // `Text.assemble(("a", "green"), " b\nc")`: a span, not a base style.
            let mut assembled = Text::new("");
            assembled.append("a", Some(style("green").into()));
            assembled.append(" b\nc", None);
            print_all(
                &console(30),
                &[
                    &Panel::new(text("body"))
                        .subtitle_as_text(Text::styled("sub", style("bold red"))),
                    &Panel::new(text("body"))
                        .subtitle_as_text(assembled)
                        .subtitle_align(rich::HorizontalAlign::Left)
                        .border_style("blue"),
                    &Panel::new(text("body"))
                        .title_as_text(Text::styled("T", style("italic")))
                        .subtitle_as_text(Text::new("S"))
                        .title_align(rich::HorizontalAlign::Right)
                        .subtitle_align(rich::HorizontalAlign::Right),
                    &Panel::fit(text("body")).subtitle_as_text(Text::new("a long subtitle here")),
                    &Panel::new(text("body")).subtitle_as_text(Text::new("")),
                ],
            )
        }
        "syntax_signed_lines" => syntax_options()
            .into_iter()
            .map(|configure| {
                print(
                    &console(30),
                    &configure(Syntax::new(UF_CODE, "text").theme("ansi_dark")),
                )
            })
            .collect(),
        "syntax_signed_stylize" => {
            let mut syntax = Syntax::new(UF_CODE, "text")
                .theme("ansi_dark")
                .line_numbers(true);
            syntax
                .stylize_range(style("bold"), (-1, 2), (2, 3), false)
                .stylize_range(style("reverse"), (2, -4), (3, 2), false)
                .stylize_range(style("underline"), (0, 1), (1, 3), false)
                .stylize_range(style("italic"), (-3, 0), (1, 1), false)
                .stylize_range(style("on blue"), (4, -30), (5, -1), true);
            print(&console(30), &syntax)
        }
        "syntax_highlight_range" => {
            let console = console(40);
            let ranges: [Option<(Option<i64>, Option<i64>)>; 6] = [
                None,
                Some((Some(-1), Some(-2))),
                Some((Some(2), Some(-1))),
                Some((Some(0), Some(0))),
                Some((None, Some(-1))),
                Some((Some(3), None)),
            ];
            let mut out = String::new();
            for range in ranges {
                let plain = Syntax::new(UF_CODE, "text")
                    .theme("ansi_dark")
                    .highlight_range(range, Some(&console));
                out.push_str(&print(&console, &Text::new(py_repr(plain.plain()))));
                let text = Syntax::new(UF_CODE, "text")
                    .theme("ansi_dark")
                    .background_color("red")
                    .highlight_range(range, Some(&console));
                out.push_str(&print(&console, &text));
            }
            out
        }
        "layout_refresh_screen" => ag_refresh_screen(),
        other => panic!("no Rust builder for API gap case {other:?}"),
    }
}

#[test]
fn api_gaps_parity() {
    let data = include_str!("golden/api_gaps.tsv");
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
            let at = expected
                .char_indices()
                .zip(got.chars())
                .find(|((_, a), b)| a != b)
                .map_or(expected.len().min(got.len()), |((at, _), _)| at);
            let from = expected.floor_char_boundary(at.saturating_sub(160));
            let near = |text: &str| {
                let start = text.floor_char_boundary(from.min(text.len()));
                let end = text.floor_char_boundary((at + 160).min(text.len()));
                text[start..end].to_string()
            };
            failures.push(format!(
                "{name} (first difference at byte {at}):\n  expected …{:?}…\n  got      …{:?}…",
                near(&expected),
                near(&got)
            ));
        }
        checked += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(checked, 20, "expected every API gap case to run");
}
