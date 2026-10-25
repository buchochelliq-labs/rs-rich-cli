//! Parity for the widget and layout edges (0.0.22 workstream 4), against
//! `golden/parity_widgets.tsv` (captured by `PARITY_WIDGET_CASES` in
//! `scripts/capture_golden.py` from real rich 15.0.0): `padding`, `panel`,
//! `align`, `constrain`, `table`, `layout`, `screen`, `bar`,
//! `box.substitute` and `_ratio`. Each fixture line is
//! `name<TAB>json(expected)`; every name has a Rust builder below.

use std::sync::Arc;

use rich::containers::Renderables;
use rich::measure::Measurement;
use rich::r#box::{self as boxes, Box as BoxSet};
use rich::ratio::{ratio_distribute, ratio_reduce, ratio_resolve, Edge};
use rich::table::Cell;
use rich::{
    Align, Bar, Color, ColorSystem, Console, ConsoleOptions, Constrain, HorizontalAlign, Justify,
    Layout, LiveRender, Overflow, Padding, Panel, Renderable, Rule, Screen, Segment, Table, Text,
    VerticalAlign, VerticalCenter,
};

/// `_cg_console`: truecolor, no highlighting, `safe_box=False`.
fn builder(width: usize) -> rich::console::ConsoleBuilder {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .highlight(false)
        .no_color(false)
        .legacy_windows(false)
        .safe_box(false)
}

fn console(width: usize) -> Console {
    builder(width).build()
}

fn sized(width: usize, height: usize) -> Console {
    builder(width).height(height).build()
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

fn text(plain: &str) -> Box<dyn Renderable> {
    Box::new(Text::new(plain))
}

fn markup(source: &str) -> Box<dyn Renderable> {
    Box::new(source.to_string())
}

/// A `Send + Sync` cell or group item holding a container renderable, rebuilt
/// for every call (see `tests/golden.rs`).
struct Built(fn() -> Box<dyn Renderable>);

impl Renderable for Built {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        (self.0)().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        (self.0)().measure(console, options)
    }
}

/// `_pw_table_cell`: one row of renderables under headers `c0`, `c1`, ….
fn table_cell(cells: &[fn() -> Box<dyn Renderable>]) -> Table {
    let mut table = Table::new();
    for index in 0..cells.len() {
        table.add_column(format!("c{index}"));
    }
    table.add_row_cells(
        cells
            .iter()
            .map(|build| Cell::Renderable(Arc::new(Built(*build))))
            .collect(),
    );
    table
}

/// `_pw_leaf`: the only leaf of a layout.
fn leaf(renderable: Box<dyn Renderable>, width: usize, height: usize) -> String {
    print(&sized(width, height), &Layout::with_renderable(renderable))
}

fn layout(width: usize, height: usize, build: impl FnOnce(&mut Layout)) -> String {
    let mut layout = Layout::new();
    build(&mut layout);
    print(&sized(width, height), &layout)
}

fn table_boxes(console: &Console) -> String {
    [
        boxes::SIMPLE_HEAVY,
        boxes::HEAVY_EDGE,
        boxes::MINIMAL_HEAVY_HEAD,
        boxes::ROUNDED,
        boxes::DOUBLE_EDGE,
        boxes::MARKDOWN,
    ]
    .into_iter()
    .map(|table_box| {
        let mut table = Table::new().box_set(table_box).show_footer(true);
        table.add_column("h");
        table.column_footer("f");
        table.add_column("i");
        table.add_row(&["a", "b"]);
        print(console, &table)
    })
    .collect()
}

fn ascii_console(width: usize) -> Console {
    builder(width).ascii_only(true).build()
}

fn custom_box() -> String {
    let template = "+-++\n| ||\n+-++\n| ||\n+-++\n+-++\n| ||\n+-++\n";
    let console = ascii_console(10);
    [false, true]
        .into_iter()
        .map(|ascii| {
            let custom = BoxSet::new(template, ascii).expect("a valid box");
            print(&console, &Panel::new(markup("x")).box_set(custom))
                + &print(
                    &console,
                    &Panel::new(markup("y"))
                        .box_set(custom)
                        .safe_box(Some(false)),
                )
        })
        .collect()
}

fn edge(size: Option<usize>, ratio: usize, minimum_size: usize) -> Edge {
    Edge::new(size, ratio, minimum_size)
}

fn ratio() -> String {
    let flexible = edge(None, 1, 1);
    let mut out = Vec::new();
    for (total, edges) in [
        (10, vec![flexible, flexible, flexible]),
        (10, vec![edge(None, 1, 8), edge(None, 1, 8)]),
        (7, vec![edge(None, 0, 1), flexible, edge(Some(3), 1, 1)]),
        (5, vec![edge(Some(4), 1, 1), edge(Some(4), 1, 1), flexible]),
        (
            100,
            vec![edge(None, 3, 50), edge(None, 1, 1), edge(None, 2, 1)],
        ),
        (3, vec![edge(None, 1, 0), edge(None, 1, 0)]),
        (0, vec![flexible, edge(Some(2), 1, 1)]),
    ] {
        out.push(format!(
            "resolve {total} {:?}",
            ratio_resolve(total, &edges)
        ));
    }
    for (total, ratios, maximums, values) in [
        (
            5,
            [1, 1, 1].as_slice(),
            [3, 3, 3].as_slice(),
            [10, 10, 10].as_slice(),
        ),
        (7, &[1, 2, 0], &[10, 10, 10], &[8, 8, 8]),
        (4, &[1, 1], &[0, 5], &[6, 6]),
        (3, &[0, 0], &[1, 1], &[2, 2]),
        (5, &[1, 3], &[1, 9], &[9, 9]),
    ] {
        out.push(format!(
            "reduce {total} {:?}",
            ratio_reduce(total, ratios, maximums, values)
        ));
    }
    for (total, ratios, minimums) in [
        (10, [1, 1, 1].as_slice(), None),
        (10, &[1, 2, 3], Some([1, 1, 1].as_slice())),
        (4, &[1, 1, 1], Some(&[5, 1, 1])),
        (9, &[2, 0, 1], Some(&[1, 0, 1])),
        (1, &[1, 1, 1, 1], Some(&[1, 1, 1, 1])),
    ] {
        out.push(format!(
            "distribute {total} {:?}",
            ratio_distribute(total, ratios, minimums)
        ));
    }
    out.join("\n") + "\n"
}

fn screen(screen: Screen, width: usize, height: usize) -> String {
    print(&sized(width, height), &screen)
}

fn squeezed_table(variant: fn(Table) -> Table) -> Table {
    let mut table = variant(Table::new());
    table.add_column("key");
    table.column_no_wrap();
    table.add_column("value");
    table.add_row(&["abcdefgh", "wrap me"]);
    table
}

/// `_pw_squeezed_three`: `n` a long `no_wrap` column, `w` a wrapping one,
/// `m` a wrapping one with `min_width=1`.
fn squeezed_three(layout: &str) -> Table {
    let mut table = Table::new().padding(0, 1, 0, 1);
    for (index, kind) in layout.chars().enumerate() {
        table.add_column(format!("h{index}"));
        match kind {
            'n' => {
                table.column_no_wrap();
            }
            'm' => {
                table.column_min_width(1);
            }
            _ => {}
        }
    }
    let cells: Vec<&str> = layout
        .chars()
        .map(|kind| if kind == 'n' { "abcdefghij" } else { "ab cd" })
        .collect();
    table.add_row(&cells);
    table
}

fn panel_boxes() -> String {
    let console = console(10);
    [
        boxes::MARKDOWN,
        boxes::MINIMAL,
        boxes::SIMPLE,
        boxes::HORIZONTALS,
        boxes::HEAVY_EDGE,
        boxes::ASCII_DOUBLE_HEAD,
        boxes::SQUARE_DOUBLE_HEAD,
    ]
    .into_iter()
    .map(|panel_box| {
        print(
            &console,
            &Panel::new(markup("x"))
                .box_set(panel_box)
                .title("T")
                .subtitle("S"),
        )
    })
    .collect()
}

fn bar_clamp() -> String {
    let red = Color::parse("red").unwrap();
    let blue = Color::parse("blue").unwrap();
    let green = Color::parse("green").unwrap();
    print_all(
        &console(8),
        &[
            &Bar::new(10.0, -2.0, 20.0).width(8),
            &Bar::new(10.0, 3.0, 30.0).width(8),
            &Bar::new(10.0, 5.0, 5.0),
            &Bar::new(10.0, 2.5, 7.75).color(red).bgcolor(blue),
            &Bar::new(10.0, 0.0, 10.0).width(20),
            &Bar::new(10.0, 9.0, 3.0).width(4).color(green),
        ],
    )
}

fn build(name: &str) -> String {
    match name {
        // constrain.py
        "constrain_zero" => {
            let group = Renderables::new(vec![
                Arc::new(Built(|| Box::new(Constrain::new(text("abc"), Some(0))))),
                Arc::new("x".to_string()),
            ]);
            print_all(
                &console(8),
                &[
                    &Panel::new(Box::new(Constrain::new(text("abc"), Some(0)))),
                    &Panel::new(Box::new(group)),
                ],
            )
        }
        "constrain_none" => print(&console(5), &Constrain::new(text("abc def ghi"), None)),
        "constrain_in_table" => print(
            &console(30),
            &table_cell(&[|| Box::new(Constrain::new(text("hello world again"), Some(7)))]),
        ),
        // padding.py
        "padding_style_name" => print_all(
            &console(6),
            &[
                &Padding::new(markup("x"), Padding::unpack(&[1]).unwrap()).style("repr.number"),
                &Padding::new(markup("y"), Padding::unpack(&[0, 1]).unwrap()).style("bold on red"),
            ],
        ),
        "padding_unpack" => {
            let pads: [&[usize]; 4] = [&[1], &[1, 2], &[1], &[0, 1, 1, 2]];
            let console = console(6);
            ["a", "b", "c", "d"]
                .into_iter()
                .zip(pads)
                .map(|(child, pad)| {
                    print(
                        &console,
                        &Padding::new(markup(child), Padding::unpack(pad).unwrap())
                            .style("on blue"),
                    )
                })
                .collect()
        }
        "padding_no_room" => print_all(
            &console(6),
            &[
                &Padding::new(text("abc"), (1, 5, 1, 5)).style("on red"),
                &Padding::new(text("abc"), (1, 5, 1, 5))
                    .style("on red")
                    .expand(false),
                &Padding::new(text("abc"), (0, 3, 0, 3)).style("on red"),
            ],
        ),
        "padding_wide_fit" => print_all(
            &console(5),
            &[
                &Padding::new(text("界界"), (0, 1, 0, 1))
                    .style("on red")
                    .expand(false),
                &Padding::new(text("界界界"), (0, 1, 0, 1))
                    .style("on red")
                    .expand(false),
            ],
        ),
        "padding_in_layout" => {
            leaf(
                Box::new(Padding::new(text("a\nb\nc"), (2, 0, 2, 0)).style("on red")),
                4,
                3,
            ) + &leaf(
                Box::new(Padding::new(text("a\nb\nc"), (1, 1, 1, 1)).style("on red")),
                4,
                4,
            )
        }
        // panel.py
        "panel_height_zero_in_layout" => {
            leaf(Box::new(Panel::new(markup("x")).height(0)), 8, 4)
                + &leaf(Box::new(Panel::new(markup("x")).height(3)), 8, 5)
        }
        "panel_height_small" => print_all(
            &console(8),
            &[
                &Panel::new(text("a\nb\nc")).height(1),
                &Panel::new(text("a\nb\nc")).height(2),
                &Panel::new(text("a\nb\nc")).height(4).padding((1, 1, 1, 1)),
                &Panel::new(text("a\nb\nc")).height(6).padding((1, 1, 1, 1)),
            ],
        ),
        "panel_title_wide" => print_all(
            &console(12),
            &[
                &Panel::new(markup("x")).title("界界界界").width(9),
                &Panel::new(markup("x")).title("界界界界").width(10),
                &Panel::new(markup("x"))
                    .subtitle("a界b界c")
                    .subtitle_align(HorizontalAlign::Left)
                    .width(8),
            ],
        ),
        "panel_title_text_options" => print_all(
            &console(14),
            &[
                &Panel::new(markup("x"))
                    .title_as_text(Text::new("a long title").overflow(Overflow::Ellipsis))
                    .width(12),
                &Panel::new(markup("x"))
                    .title_as_text(Text::styled("T", "bold").justify(Justify::Right))
                    .title_align(HorizontalAlign::Left),
                &Panel::new(markup("x"))
                    .subtitle_as_text(Text::styled("S", "repr.number"))
                    .border_style("red"),
                &Panel::new(markup("x"))
                    .title_as_text(Text::new("tab\there"))
                    .width(14),
            ],
        ),
        "panel_title_newline_control" => print_all(
            &console(14),
            &[
                &Panel::new(markup("x")).title("a\nb\x07c"),
                &Panel::new(markup("x")).title("ab\x07c"),
                &Panel::new(markup("x")).subtitle_as_text(Text::new("s\nt\x1b[1mu")),
            ],
        ),
        "panel_boxes" => panel_boxes(),
        "panel_style_names" => print_all(
            &console(10),
            &[
                &Panel::new(markup("x"))
                    .style("on red")
                    .border_style("repr.number"),
                &Panel::new(markup("x"))
                    .style("repr.str")
                    .border_style("bold")
                    .title("T"),
            ],
        ),
        "panel_padding" => print_all(
            &console(12),
            &[
                &Panel::new(markup("x")).padding((1, 2, 0, 3)),
                &Panel::fit(markup("xy")).padding((1, 2, 0, 3)),
                &Panel::fit(markup("xy")).padding(Padding::unpack(&[1]).unwrap()),
                &Panel::new(markup("xy")).padding((0, 0, 0, 0)).expand(false),
            ],
        ),
        "panel_fit_title" => print_all(
            &console(12),
            &[
                &Panel::fit(markup("x")).title("a very long title indeed"),
                &Panel::fit(markup("x")).title("mid"),
                &Panel::new(markup("x")).width(50),
                &Panel::fit(markup("x")).width(6).title("title"),
            ],
        ),
        "panel_highlight" => print_all(
            &console(14),
            &[
                &Panel::new(markup("1 'a' [x]")).highlight(true),
                &Panel::new(markup("1 'a' [x]")),
            ],
        ),
        "panel_measure" => print(
            &console(40),
            &table_cell(&[
                || {
                    Box::new(
                        Panel::new(Box::new("hello there".to_string())).title("a longer title"),
                    )
                },
                || Box::new(Panel::new(Box::new("x".to_string())).width(8)),
            ]),
        ),
        // box.substitute
        "box_legacy_windows" => {
            table_boxes(&builder(20).legacy_windows(true).safe_box(true).build())
        }
        "box_legacy_windows_unsafe" => {
            table_boxes(&builder(20).legacy_windows(true).safe_box(false).build())
                + &print_all(
                    &builder(10).legacy_windows(true).safe_box(true).build(),
                    &[
                        &Panel::new(markup("x")).box_set(boxes::HEAVY_EDGE),
                        &Panel::new(markup("y"))
                            .box_set(boxes::HEAVY_EDGE)
                            .safe_box(Some(false)),
                    ],
                )
        }
        "box_ascii_only" => {
            table_boxes(&ascii_console(20))
                + &print_all(
                    &ascii_console(10),
                    &[
                        &Panel::new(markup("x")),
                        &Panel::new(markup("y")).box_set(boxes::MARKDOWN),
                    ],
                )
        }
        "box_custom_ascii_flag" => custom_box(),
        // align.py
        "align_width_zero" => print_all(
            &console(6),
            &[
                &Align::center(text("ab"))
                    .width(0)
                    .vertical(VerticalAlign::Middle)
                    .height(3)
                    .style("on red"),
                &Align::right(text("ab")).width(0),
                &Text::new("end"),
            ],
        ),
        "align_height_zero_in_layout" => {
            leaf(
                Box::new(
                    Align::left(text("a"))
                        .vertical(VerticalAlign::Bottom)
                        .height(0),
                ),
                4,
                3,
            ) + &leaf(
                Box::new(
                    Align::left(text("a"))
                        .vertical(VerticalAlign::Middle)
                        .height(1),
                ),
                4,
                3,
            )
        }
        "align_vertical_overflow" => print_all(
            &console(4),
            &[
                &Align::left(text("a\nb\nc\nd"))
                    .vertical(VerticalAlign::Middle)
                    .height(2),
                &Align::left(text("a\nb\nc\nd"))
                    .vertical(VerticalAlign::Bottom)
                    .height(2),
                &Align::left(text("a\nb"))
                    .vertical(VerticalAlign::Middle)
                    .height(5)
                    .pad(false)
                    .style("on red"),
            ],
        ),
        "align_style_pad" => print_all(
            &console(8),
            &[
                &Align::center(text("ab"))
                    .style("on red")
                    .pad(false)
                    .vertical(VerticalAlign::Middle)
                    .height(3),
                &Align::right(text("ab"))
                    .style("repr.number")
                    .width(4)
                    .vertical(VerticalAlign::Top)
                    .height(2),
                &Align::center(text("a\nbcd")).style("on blue"),
            ],
        ),
        "vertical_center" => {
            leaf(
                Box::new(VerticalCenter::new(text("a")).style("on red")),
                4,
                4,
            ) + &leaf(Box::new(VerticalCenter::new(text("a\nb\nc\nd\ne"))), 4, 3)
                + &print(
                    &sized(4, 3),
                    &VerticalCenter::new(text("ab")).style("on red"),
                )
        }
        // bar.py
        "bar_clamp" => bar_clamp(),
        "bar_measure" => print(
            &console(30),
            &table_cell(&[
                || Box::new(Bar::new(10.0, 2.0, 8.0)),
                || Box::new(Bar::new(10.0, 2.0, 8.0).width(3)),
            ]),
        ),
        // screen.py
        "screen_print" => screen(Screen::new(text("hi")), 6, 3),
        "screen_application_mode" => screen(Screen::new(text("hi")).application_mode(true), 6, 3),
        "screen_style" => screen(
            Screen::new(Box::new(Text::from_markup("[bold]hi[/] x").unwrap())).style("on blue"),
            8,
            2,
        ),
        "screen_renderables" => screen(
            Screen::from_renderables(vec![text("a"), text("bb"), text("ccc")]),
            6,
            2,
        ),
        "screen_empty" => screen(Screen::empty(), 3, 2),
        "screen_then_text" => print_all(
            &sized(6, 2),
            &[&Screen::new(text("s")), &Text::new("after")],
        ),
        "screen_in_panel" => print(
            &sized(8, 4),
            &Panel::new(Box::new(Screen::new(text("s")).style("on red"))),
        ),
        // layout.py
        "layout_minimum_overflow" => {
            layout(10, 2, |l| {
                l.split_row(vec![
                    Layout::with_renderable(text("aaaaaaaa")).minimum_size(8),
                    Layout::with_renderable(text("bbbbbbbb")).minimum_size(8),
                ])
            }) + &layout(4, 3, |l| {
                l.split_column(vec![
                    Layout::with_renderable(text("a")).size(2),
                    Layout::with_renderable(text("b")).size(2),
                    Layout::with_renderable(text("c")),
                ])
            })
        }
        "layout_ratio_zero" => layout(9, 1, |l| {
            l.split_row(vec![
                Layout::with_renderable(text("a")).ratio(0),
                Layout::with_renderable(text("b")),
                Layout::with_renderable(text("c")).ratio(2),
            ])
        }),
        "layout_invisible" => layout(9, 2, |l| {
            l.split_row(vec![
                Layout::with_renderable(text("a")),
                Layout::with_renderable(text("b")).visible(false),
                Layout::with_renderable(text("c")),
            ])
        }),
        "layout_nested" => layout(12, 4, |l| {
            l.split_row(vec![
                Layout::new().name("left"),
                Layout::new().name("right").ratio(2),
            ]);
            l.get_mut("left").unwrap().split_column(vec![
                Layout::with_renderable(text("1")).size(1),
                Layout::with_renderable(text("2")),
            ]);
            l.get_mut("right").unwrap().split_column(vec![
                Layout::with_renderable(text("3")).minimum_size(3),
                Layout::with_renderable(text("4")),
            ]);
        }),
        "layout_ratio" => ratio(),
        // table.py
        "table_squeezed_column" => {
            let variants: [fn(Table) -> Table; 6] = [
                |t| t,
                |t| t.without_box(),
                |t| t.show_edge(false),
                |t| t.pad_edge(false),
                |t| t.padding(0, 2, 0, 2),
                |t| t.collapse_padding(true).box_set(boxes::SIMPLE),
            ];
            variants
                .into_iter()
                .flat_map(|variant| {
                    (1..16).map(move |width| print(&console(width), &squeezed_table(variant)))
                })
                .collect()
        }
        "table_squeezed_three" => ["nww", "wnw", "nnw", "nmw", "wwn"]
            .into_iter()
            .flat_map(|layout| {
                (3..26)
                    .step_by(2)
                    .map(move |width| print(&console(width), &squeezed_three(layout)))
            })
            .collect(),
        "table_many_columns" => [
            (20, 80, Some(boxes::SQUARE)),
            (29, 80, Some(boxes::SQUARE)),
            (40, 80, Some(boxes::SQUARE)),
            (12, 20, Some(boxes::ROUNDED)),
            (12, 20, None),
        ]
        .into_iter()
        .map(|(count, width, table_box)| {
            let mut table = match table_box {
                Some(table_box) => Table::new().box_set(table_box),
                None => Table::new().without_box(),
            };
            for index in 0..count {
                table.add_column(format!("c{index}"));
            }
            let cells: Vec<String> = (0..count).map(|index| index.to_string()).collect();
            table.add_row(&cells.iter().map(String::as_str).collect::<Vec<_>>());
            print(&console(width), &table)
        })
        .collect(),
        // rule.py / live_render.py
        "rule_end" => {
            print_all(
                &console(10),
                &[
                    &Rule::new("t").end(""),
                    &Rule::new("t").end("!"),
                    &Rule::new("t").end("\n\n"),
                    &Rule::line().end(""),
                    &Rule::new("t").align(HorizontalAlign::Left).end("x\ny"),
                    &Text::new("after"),
                ],
            ) + &print_all(
                &console(4),
                &[
                    &Rule::new("t").end(""),
                    &Rule::new("t").align(HorizontalAlign::Right).end(""),
                    &Text::new("z"),
                ],
            )
        }
        "live_render_print" => print_all(
            &console(10),
            &[&LiveRender::new(text("a\nb")), &Text::new("after")],
        ),
        other => panic!("no Rust builder for parity widget case {other:?}"),
    }
}

#[test]
fn parity_widgets() {
    let data = include_str!("golden/parity_widgets.tsv");
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
    assert_eq!(checked, 47, "expected every parity widget case to run");
}
