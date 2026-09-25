//! Parity for the core gaps found while building the Python bindings, against
//! `golden/core_gaps.tsv` (captured by `CORE_GAP_CASES` in
//! `scripts/capture_golden.py` from real rich 15.0.0). Each fixture line is
//! `name<TAB>json(expected)`; every name has a Rust builder below.

use std::sync::Arc;

use rich::containers::Renderables;
use rich::control::Control;
use rich::emoji::EmojiVariant;
use rich::json::JsonOptions;
use rich::log_render::LogRender;
use rich::measure::Measurement;
use rich::terminal_theme::{DEFAULT_TERMINAL_THEME, SVG_EXPORT_THEME};
use rich::{
    Align, Cell, ColorSystem, Console, ConsoleOptions, HorizontalAlign, Json, Justify, Overflow,
    Padding, Panel, RenderStrOptions, Renderable, Rule, Segment, Style, Table, Text, Tree,
    VerticalAlign, VerticalCenter,
};

const JSON_SAMPLE: &str =
    r#"{"name": "Alice", "age": 30, "admin": true, "tags": ["a", "b"], "meta": null}"#;
const EXPORT_MARKUP: &str =
    "[link=https://example.com/a?b=1&c=2]hi[/link] [blink]b[/] [bold red]r[/] <&>";
const SVG_FORMAT: &str = "{unique_id}|{char_width}|{char_height}|{line_height}|{terminal_width}|{terminal_height}|{width}|{height}|{terminal_x}|{terminal_y}|{{lit}}\n{styles}\n{matrix}\n{backgrounds}\n{lines}\n{chrome}";

/// Upstream's `Console(force_terminal=True, color_system="truecolor",
/// highlight=False, …)` as the capture script builds it.
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

/// A `Send + Sync` wrapper for a container built on demand (see `Built` in
/// `golden.rs`).
struct Built(fn() -> Box<dyn Renderable>);

impl Renderable for Built {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        (self.0)().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        (self.0)().measure(console, options)
    }

    fn vertical(&self) -> Option<VerticalAlign> {
        (self.0)().vertical()
    }
}

fn panel(child: impl Renderable + 'static) -> Panel {
    Panel::new(Box::new(child))
}

fn print(console: &Console, renderable: &dyn Renderable) -> String {
    console.capture(|c| c.print(renderable))
}

fn render_str_case(content: &str, console: Console, options: RenderStrOptions<'_>) -> String {
    match console.render_str_with(content, &options) {
        Ok(text) => print(&console, &panel(text)),
        Err(_) => "<ERROR>".to_string(),
    }
}

fn json_case(source: &str, width: usize, options: JsonOptions) -> String {
    json_print_case(source, width, options, |_| {})
}

fn json_print_case(
    source: &str,
    width: usize,
    options: JsonOptions,
    print_options: impl FnOnce(&mut ConsoleOptions),
) -> String {
    let console = console(width);
    let Ok(json) = Json::with_options(source, &options) else {
        return "<ERROR>".to_string();
    };
    let mut render_options = console.options();
    print_options(&mut render_options);
    console.capture(|c| c.print_with(&json, &render_options))
}

fn json_options(update: impl FnOnce(&mut JsonOptions)) -> JsonOptions {
    let mut options = JsonOptions::default();
    update(&mut options);
    options
}

fn log_renderables() -> String {
    let console = console(50);
    let render = LogRender::new().show_level(true);
    let level = || Text::styled(format!("{:<8}", "INFO"), "logging.level.info");
    let table = || -> Arc<dyn Renderable + Send + Sync> {
        let mut table = Table::new();
        table.add_column("a").add_column("b");
        table.add_row(&["1", "2"]);
        Arc::new(table)
    };
    let records: Vec<(&str, Vec<Arc<dyn Renderable + Send + Sync>>)> = vec![
        ("[t1]", vec![Arc::new(Text::new("plain message"))]),
        (
            "[t1]",
            vec![
                Arc::new(Text::new("before the panel")),
                Arc::new(Built(|| Box::new(Panel::fit(Box::new(Text::new("boxed")))))),
            ],
        ),
        (
            "[t2]",
            vec![
                table(),
                Arc::new(Text::from_markup("[bold]after[/] the table").unwrap()),
            ],
        ),
    ];
    let mut out = String::new();
    for (time, renderables) in records {
        let table = render.render_renderables(
            &console,
            renderables,
            Some(Text::new(time)),
            level(),
            Some("app.py"),
            Some(7),
            None,
        );
        out.push_str(&print(&console, &table));
    }
    out
}

fn record_export(width: usize) -> (Console, Vec<Segment>) {
    let console = builder(width).build();
    let segments = console.record_output(|c| c.print_str(EXPORT_MARKUP));
    (console, segments)
}

fn svg_export(font_aspect_ratio: f64, code_format: &str) -> String {
    let console = console(12);
    let segments = console.record_output(|c| c.print_str("[bold red]Hi[/] ok\n[on blue]x[/]"));
    rich::svg::export_svg_with(
        &segments,
        &SVG_EXPORT_THEME,
        "T",
        "u",
        12,
        code_format,
        font_aspect_ratio,
    )
    .expect("valid template")
}

fn svg_of(width: usize, renderable: &dyn Renderable) -> String {
    console(width).export_svg("S", "p", |c| c.print(renderable))
}

fn justify_padding_runs() -> String {
    let console = console(16);
    let mut out = String::new();
    for justify in [
        Justify::Left,
        Justify::Center,
        Justify::Right,
        Justify::Full,
    ] {
        out.push_str(&print(
            &console,
            &panel(Text::styled("abc", Style::parse("red").unwrap()).justify(justify)),
        ));
        let mut text = Text::from_markup("x[red]abc[/]").unwrap();
        text.set_base_style(Style::parse("bold").unwrap());
        out.push_str(&print(&console, &panel(text.justify(justify))));
        out.push_str(&print(
            &console,
            &panel(
                Text::from_markup("[red]abc[/] de")
                    .unwrap()
                    .justify(justify),
            ),
        ));
    }
    let mut table = Table::new();
    table.add_column("hhhhhhh");
    table.add_row_text(vec![Text::from_markup("[red]a[/][red]b[/]").unwrap()]);
    table.add_row_text(vec![
        Text::styled("c", Style::parse("green").unwrap()).justify(Justify::Center)
    ]);
    out.push_str(&print(&console, &table));
    out
}

fn build(name: &str) -> String {
    let default_str = RenderStrOptions::default;
    match name {
        "render_panel_width20" => {
            let console = console(40);
            let panel = panel(Text::new("hello")).title("T");
            let segments = console.render(&panel, Some(&console.options().update_width(20)));
            // The port's renderables separate lines rather than ending each
            // one, so upstream's final newline is added back here.
            console.segments_to_string(&segments) + "\n"
        }
        "render_zero_width" => {
            let console = console(40);
            let segments = console.render(
                &Text::new("hello"),
                Some(&console.options().update_width(0)),
            );
            console.segments_to_string(&segments)
        }
        "options_highlight_panel_default" => print(
            &builder(30).highlight(true).build(),
            &panel("x 123 True".to_string()),
        ),
        "options_highlight_panel_on" => print(
            &console(30),
            &panel("x 123 True".to_string()).highlight(true),
        ),
        "options_highlight_tree_on" => print(
            &console(30),
            &Tree::new(Cell::Renderable(Arc::new("x 123".to_string()))).highlight(true),
        ),
        "render_str_markup_off" => render_str_case(
            "[b]x[/] 1",
            console(20),
            RenderStrOptions {
                markup: Some(false),
                ..default_str()
            },
        ),
        "render_str_highlight_drops_style" => render_str_case(
            "abc 12",
            console(20),
            RenderStrOptions {
                style: Style::parse("red").unwrap().into(),
                justify: Some(Justify::Center),
                highlight: Some(true),
                ..default_str()
            },
        ),
        "render_str_style_justify" => render_str_case(
            "[b]abc[/]",
            console(20),
            RenderStrOptions {
                style: "red".into(),
                justify: Some(Justify::Right),
                highlight: Some(false),
                ..default_str()
            },
        ),
        "render_str_emoji_off" => render_str_case(
            ":rocket: x",
            console(20),
            RenderStrOptions {
                emoji: Some(false),
                ..default_str()
            },
        ),
        "render_str_console_markup_off" => render_str_case(
            "[b]x[/] :rocket:",
            builder(20).markup(false).build(),
            default_str(),
        ),
        "render_str_bad_markup" => render_str_case("x [/b]", console(20), default_str()),
        "tab_console_4" => print(&builder(20).tab_size(4).build(), &Text::new("a\tbc\td")),
        "tab_text_2" => print(&console(20), &Text::new("a\tb").tab_size(2)),
        "tab_markup_console_3" => builder(20)
            .tab_size(3)
            .build()
            .capture(|c| c.print_str("x\ty")),
        "tab_table_cell_console_4" => {
            let mut table = Table::new();
            table.add_column("h");
            table.add_row_text(vec![Text::new("a\tb")]);
            print(&builder(20).tab_size(4).build(), &table)
        }
        "emoji_variant_text" => builder(20)
            .emoji_variant(Some(EmojiVariant::Text))
            .build()
            .capture(|c| c.print_str(":rocket: hi")),
        "emoji_variant_markup" => builder(20)
            .emoji_variant(Some(EmojiVariant::Text))
            .build()
            .capture(|c| c.print_str(":rocket: [b]hi[/]")),
        "emoji_variant_explicit" => builder(20)
            .emoji_variant(Some(EmojiVariant::Emoji))
            .build()
            .capture(|c| c.print_str(":rocket-text: :rocket:")),
        "log_renderables" => log_renderables(),
        "json_indent_4" => json_case(
            JSON_SAMPLE,
            40,
            json_options(|o| o.indent = JsonOptions::indent_spaces(4)),
        ),
        "json_indent_none" => json_case(JSON_SAMPLE, 80, json_options(|o| o.indent = None)),
        "json_indent_none_wrapped" => json_case(JSON_SAMPLE, 30, json_options(|o| o.indent = None)),
        "json_indent_zero" => json_case(
            r#"{"a": [1, {"b": []}]}"#,
            40,
            json_options(|o| o.indent = JsonOptions::indent_spaces(0)),
        ),
        "json_indent_tab" => json_case(
            r#"{"a": [1, 2]}"#,
            40,
            json_options(|o| o.indent = Some("\t".to_string())),
        ),
        "json_sort_keys" => json_case(
            r#"{"b": 1, "a": {"d": 2, "c": 3}, "B": 4}"#,
            40,
            json_options(|o| o.sort_keys = true),
        ),
        "json_ensure_ascii" => json_case(
            "{\"caf\u{e9}\": \"\u{2764} \u{1f600} \u{7f}\"}",
            40,
            json_options(|o| o.ensure_ascii = true),
        ),
        "json_no_highlight" => json_case(JSON_SAMPLE, 40, json_options(|o| o.highlight = false)),
        "json_trailing_backslash" => {
            json_case(r#"{"k": "a\\", "b": 1}"#, 40, JsonOptions::default())
        }
        "json_allow_nan_off" => json_case("[NaN]", 40, json_options(|o| o.allow_nan = false)),
        "json_print_no_wrap_ellipsis" => {
            json_print_case(JSON_SAMPLE, 16, JsonOptions::default(), |o| {
                o.no_wrap = Some(true);
                o.overflow = Some(Overflow::Ellipsis);
            })
        }
        "json_print_crop" => json_print_case(JSON_SAMPLE, 16, JsonOptions::default(), |o| {
            o.overflow = Some(Overflow::Crop)
        }),
        "json_print_center" => json_print_case(r#"{"a": 1}"#, 20, JsonOptions::default(), |o| {
            o.justify = Justify::Center
        }),
        "json_nested_panel" => print(
            &console(16),
            &panel(Json::new(JSON_SAMPLE).unwrap().no_wrap(true)),
        ),
        "export_html_inline_links" => {
            let (_, segments) = record_export(40);
            rich::export::export_html_with(&segments, &DEFAULT_TERMINAL_THEME, None, true).unwrap()
        }
        "export_html_classes_links" => {
            let (_, segments) = record_export(40);
            rich::export::export_html_with(&segments, &DEFAULT_TERMINAL_THEME, None, false).unwrap()
        }
        "export_html_code_format" => {
            let (console, _) = record_export(40);
            console
                .export_html_with(
                    &DEFAULT_TERMINAL_THEME,
                    Some("<{foreground}|{background}>{stylesheet}<pre>{code}</pre>{{x}}"),
                    false,
                    |c| c.print_str(EXPORT_MARKUP),
                )
                .unwrap()
        }
        "export_svg_aspect" => svg_export(0.5, rich::svg::CONSOLE_SVG_FORMAT),
        "export_svg_code_format" => svg_export(0.61, SVG_FORMAT),
        "control_alt_screen" => format!(
            "{}|{}",
            Control::alt_screen(true).as_str(),
            Control::alt_screen(false).as_str()
        ),
        "control_title" => Control::title("my title").as_str().to_string(),
        "justify_padding_runs" => justify_padding_runs(),
        "panel_title_svg" => svg_of(
            22,
            &panel(Text::new("hi")).title("Title").subtitle("[b]S[/]"),
        ),
        "panel_title_styled_border_svg" => svg_of(
            22,
            &panel(Text::new("hi"))
                .title("Title")
                .border_style(Style::parse("red").unwrap()),
        ),
        "rule_title_svg" => svg_of(22, &Rule::new("Title")),
        "panel_narrow_heights" => {
            let mut out = String::new();
            for width in [2, 3, 4, 5] {
                for padding in [(0, 1, 0, 1), (1, 1, 1, 1)] {
                    for height in [None, Some(5)] {
                        let console = console(width);
                        let mut options = console.options();
                        options.height = height;
                        let panel = panel(Text::new("hi")).padding(padding);
                        out.push_str(&console.capture(|c| c.print_with(&panel, &options)));
                    }
                }
            }
            out
        }
        "align_vertical" => {
            let console = console(10);
            let mut options = console.options();
            options.height = Some(4);
            let text = || Box::new(Text::new("hi")) as Box<dyn Renderable>;
            let blue = || Style::parse("on blue").unwrap();
            let aligns = [
                Align::center(text())
                    .vertical(VerticalAlign::Top)
                    .style(blue()),
                Align::center(text())
                    .vertical(VerticalAlign::Middle)
                    .style(blue()),
                Align::right(text()).vertical(VerticalAlign::Bottom),
                Align::new(text(), HorizontalAlign::Left)
                    .vertical(VerticalAlign::Middle)
                    .pad(false),
                Align::center(Box::new(Text::new("a b c d")))
                    .width(3)
                    .vertical(VerticalAlign::Middle)
                    .style(Style::parse("on red").unwrap()),
            ];
            aligns
                .iter()
                .map(|align| console.capture(|c| c.print_with(align, &options)))
                .collect()
        }
        "vertical_center" => {
            let console = console(6);
            let mut options = console.options();
            options.height = Some(3);
            let center = VerticalCenter::new(Box::new(Text::new("x")))
                .style(Style::parse("on red").unwrap());
            console.capture(|c| c.print_with(&center, &options))
        }
        "table_vertical" => {
            let mut table = Table::new();
            table.add_column("a").column_vertical(VerticalAlign::Middle);
            table.add_column("b");
            table.add_column("c").column_vertical(VerticalAlign::Bottom);
            table.add_row(&["x", "1\n2\n3\n4", "z"]);
            table.add_row_cells(vec![
                Cell::Renderable(Arc::new(Built(|| {
                    Box::new(Align::left(Box::new(Text::new("y"))).vertical(VerticalAlign::Bottom))
                }))),
                "1\n2\n3".into(),
                Cell::Renderable(Arc::new(Built(|| {
                    Box::new(Align::left(Box::new(Text::new("w"))).vertical(VerticalAlign::Top))
                }))),
            ]);
            print(&console(30), &table)
        }
        "empty_text_justify" => {
            let console = console(10);
            let red = || Style::parse("on red").unwrap();
            let mut out = String::new();
            for justify in [
                Justify::Left,
                Justify::Center,
                Justify::Right,
                Justify::Full,
                Justify::Default,
            ] {
                out.push_str(&print(
                    &console,
                    &panel(Text::styled("", red()).justify(justify)),
                ));
            }
            let mut options = console.options();
            options.justify = Justify::Left;
            out.push_str(&console.capture(|c| c.print_with(&Text::styled("", red()), &options)));
            out
        }
        "padding_options" => {
            let console = console(20);
            let blue = || Style::parse("on blue").unwrap();
            let mut options = console.options();
            options.height = Some(5);
            [
                print(
                    &console,
                    &Padding::new(Box::new(Text::new("hi")), (0, 2, 0, 2))
                        .expand(false)
                        .style(blue()),
                ),
                print(&console, &Padding::indent(Box::new(Text::new("x")), 3)),
                console.capture(|c| {
                    c.print_with(
                        &Padding::new(Box::new(Text::new("a")), (1, 1, 1, 1)).style(blue()),
                        &options,
                    )
                }),
            ]
            .concat()
        }
        "bar_colours" => print(
            &console(20),
            &rich::Bar::new(10.0, 2.0, 6.0)
                .color(rich::Color::parse("red").unwrap())
                .bgcolor(rich::Color::parse("blue").unwrap())
                .width(10),
        ),
        other => panic!("no Rust builder for core gap case {other:?}"),
    }
}

#[test]
fn core_gaps_parity() {
    let data = include_str!("golden/core_gaps.tsv");
    let mut checked = 0;
    let mut failures = Vec::new();
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
    assert_eq!(checked, 51, "expected every core gap case to run");
}

/// `Renderables` renders nothing for no children and measures `(1, 1)`, as
/// upstream's `Renderables([])` does.
#[test]
fn empty_renderables() {
    let console = console(20);
    let empty = Renderables::default();
    assert!(empty.rich_render(&console, &console.options()).is_empty());
    assert_eq!(
        empty.measure(&console, &console.options()),
        Measurement::new(1, 1)
    );
}

/// `Console` is `Clone + Send + Sync`: a clone keeps its configuration and
/// captures independently of the original.
#[test]
fn console_is_clone_send_sync() {
    fn assert_traits<T: Clone + Send + Sync>() {}
    assert_traits::<Console>();
    let original = builder(20).tab_size(4).markup(false).build();
    let clone = original.clone();
    assert_eq!(clone.tab_size(), 4);
    assert!(!clone.markup());
    let outer = original.capture(|c| {
        c.print_str("outer");
        assert_eq!(clone.capture(|c| c.print_str("inner")), "inner\n");
    });
    assert_eq!(outer, "outer\n");
}
