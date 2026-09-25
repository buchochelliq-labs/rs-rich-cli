//! Parity for upstream options the Python bindings needed from core, against
//! `golden/upstream_features.tsv` (captured by `UPSTREAM_FEATURE_CASES` in
//! `scripts/capture_golden.py` from real rich 15.0.0). Each fixture line is
//! `name<TAB>json(expected)`; every name has a Rust builder below.

use std::sync::Arc;

use rich::measure::Measurement;
use rich::progress::{BarColumn, CustomProgressColumn, Progress, ProgressColumn, Task, TextColumn};
use rich::table::ColumnOptions;
use rich::{
    Cell, ColorSystem, Columns, Console, ConsoleOptions, HorizontalAlign, Justify, Layout,
    LiveRender, Panel, Renderable, Rule, Segment, Style, Syntax, Table, Text, Theme, Tree,
    VerticalAlign, VerticalOverflow,
};

/// Upstream's `Console(force_terminal=True, color_system="truecolor",
/// highlight=False, …)` as `_cg_console` builds it.
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

fn print_with(console: &Console, renderable: &dyn Renderable, options: &ConsoleOptions) -> String {
    console.capture(|c| c.print_with(renderable, options))
}

/// `_uf_render_with`: `Console.render` with overridden options. The port's
/// renderables separate lines rather than ending each one, so upstream's final
/// newline is added back.
fn render_with(console: &Console, renderable: &dyn Renderable, options: &ConsoleOptions) -> String {
    let segments = console.render(renderable, Some(options));
    console.segments_to_string(&segments) + "\n"
}

fn style(definition: &str) -> Style {
    Style::parse(definition).unwrap()
}

const WORDS: [&str; 11] = [
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
];

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

// --- 1. Tree -----------------------------------------------------------------

fn uf_tree(hide_root: bool) -> Tree {
    let mut tree = Tree::new("root").hide_root(hide_root);
    let a = tree.add("child [b]A[/]");
    a.add("leaf A1");
    a.add(Text::new("leaf A2\nsecond line"));
    tree.add("child B").add("leaf B1");
    tree
}

fn uf_tree_styles() -> Tree {
    let mut tree = Tree::new("root").style("on blue").guide_style("red");
    let bold = tree.add("bold guides");
    bold.set_guide_style("bold green");
    bold.add("x");
    bold.add("y").add("z");
    let double = tree.add("double guides");
    double.set_guide_style("underline2").set_style("italic");
    double.add("p");
    double.add("q");
    tree
}

fn uf_tree_collapsed() -> Tree {
    let mut tree = Tree::new("root");
    let closed = tree.add("closed");
    closed.set_expanded(false);
    closed.add("hidden");
    tree.add("open").add("shown");
    tree
}

fn uf_layout() -> Layout {
    let mut layout = Layout::new().name("root");
    layout.split_column(vec![
        Layout::with_renderable(Box::new(Text::styled("head", style("on blue"))))
            .name("header")
            .size(1),
        Layout::new().name("body"),
    ]);
    layout["body"].split_row(vec![
        Layout::with_renderable(Box::new(Text::new("L"))).name("left"),
        Layout::new().name("hidden").visible(false),
        Layout::with_renderable(Box::new(Panel::new(Box::new(Text::new("side")))))
            .ratio(2)
            .minimum_size(3),
    ]);
    layout["body"].add_split(vec![Layout::new().name("extra").size(6)]);
    layout
}

fn uf_indented() -> Text {
    let mut text = Text::from_markup(
        "def f():\n    [red]if x:[/]\n        return 1\n\n      odd\n    [b]done[/]\n",
    )
    .unwrap();
    text.set_base_style(style("green"));
    text
}

fn live_render(overflow: VerticalOverflow, live_style: &str, content: &str) -> String {
    let mut live = LiveRender::new(Box::new(Text::new(content))).vertical_overflow(overflow);
    if !live_style.is_empty() {
        live = live.style(style(live_style));
    }
    let out = print(&builder(12).height(4).build(), &live);
    // Upstream's `LiveRender` yields no newline after its last line, so a
    // print of it ends there; the port's printer always ends the line.
    let out = out.strip_suffix('\n').unwrap_or(&out);
    format!("{out}|{}|", live.position_cursor().as_str())
}

struct Stars;

impl CustomProgressColumn for Stars {
    fn render(&self, task: &Task) -> Cell {
        let count = (task.percentage() / 20.0).floor() as usize;
        Text::styled("*".repeat(count), style("yellow")).into()
    }
}

struct Counter(std::sync::atomic::AtomicUsize);

impl CustomProgressColumn for Counter {
    fn render(&self, task: &Task) -> Cell {
        let calls = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        if task.description() == "p" {
            Cell::Renderable(Arc::new(Built(|| {
                // `Panel.fit(str(calls))`: squeezed into the 4-cell column
                // it shows only its borders, so the count never appears.
                Box::new(Panel::fit(Box::new(Text::new("3"))))
            })))
        } else {
            Text::new(calls.to_string()).into()
        }
    }

    fn table_column(&self) -> ColumnOptions {
        ColumnOptions {
            width: Some(4),
            justify: Justify::Right,
            ..ColumnOptions::default()
        }
    }

    fn max_refresh(&self) -> Option<f64> {
        Some(10.0)
    }
}

fn progress_custom_column() -> String {
    let now = Arc::new(std::sync::Mutex::new(100.0));
    let clock = now.clone();
    let mut progress = Progress::new()
        .clock(move || *clock.lock().unwrap())
        .columns(vec![
            ProgressColumn::custom(Stars),
            ProgressColumn::TextFormat(TextColumn::new("{task.description}")),
            ProgressColumn::custom(Counter(std::sync::atomic::AtomicUsize::new(0))),
            ProgressColumn::BarWith(BarColumn::new().bar_width(Some(6))),
        ]);
    progress.add_task("a", 100.0, 40.0);
    progress.add_task("b", 100.0, 0.0);
    progress.add_task("p", 100.0, 100.0);
    let console = console(40);
    let mut out = print(&console, &progress.make_tasks_table());
    *now.lock().unwrap() += 1.0;
    out.push_str(&print(&console, &progress.make_tasks_table()));
    *now.lock().unwrap() += 20.0;
    out + &print(&console, &progress.make_tasks_table())
}

const UF_CODE: &str =
    "def f(x):\n    if x:\n        return 'a very long line of code'\n\n    return x\n";

fn uf_syntax() -> Syntax {
    Syntax::new(UF_CODE, "text").theme("ansi_dark")
}

/// `_UF_SYNTAX_OPTIONS`, in order.
fn syntax_options() -> Vec<fn(Syntax) -> Syntax> {
    vec![
        |s| s,
        |s| s.line_numbers(true),
        |s| s.line_numbers(true).start_line(9).highlight_lines([10, 12]),
        |s| s.line_numbers(true).line_range(Some(2), Some(3)),
        |s| s.line_range(Some(4), None),
        |s| s.line_range(None, Some(2)).word_wrap(true),
        |s| s.code_width(12),
        |s| s.word_wrap(true),
        |s| s.word_wrap(true).line_numbers(true).code_width(14),
        |s| s.indent_guides(true),
        |s| s.indent_guides(true).line_numbers(true),
        |s| s.padding_sides((1, 2, 1, 2)),
        |s| s.padding_sides((0, 1, 2, 3)).line_numbers(true),
        |s| s.background_color("red"),
        |s| {
            s.background_color("red")
                .line_numbers(true)
                .highlight_lines([2])
        },
        |s| s.background_color("red").word_wrap(true).code_width(20),
        |s| s.tab_size(2).indent_guides(true),
    ]
}

/// Drop SGR sequences, as `_uf_strip_ansi` does.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == 'm' {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn small_table() -> Table {
    let mut table = Table::new();
    table.add_column("a").add_column("bb");
    table.add_row(&["1", "22"]);
    table
}

fn build(name: &str) -> String {
    match name {
        "tree_default" => print(&console(30), &uf_tree(false)),
        "tree_styles" => print(&console(30), &uf_tree_styles()),
        "tree_hide_root" => print(&console(30), &uf_tree(true)),
        "tree_collapsed" => print(&console(30), &uf_tree_collapsed()),
        "tree_ascii" => {
            let console = console(30);
            let mut options = console.options();
            options.encoding = "ascii".to_string();
            render_with(&console, &uf_tree_styles(), &options)
        }
        "tree_highlight" => {
            let mut tree = Tree::new("x 123").highlight(true).style("bold");
            tree.add("y 45 True");
            tree.add(Text::new("z 6"));
            print(&console(30), &tree)
        }
        "tree_narrow" => print(&console(9), &uf_tree_styles()),
        "tree_justify" => {
            let console = console(20);
            let mut options = console.options();
            options.justify = Justify::Right;
            print_with(&console, &uf_tree(false), &options)
        }
        "tree_measure" => print(&console(40), &Panel::fit(Box::new(uf_tree_collapsed()))),
        "columns_options" => {
            let console = console(30);
            type Configure = fn(Columns) -> Columns;
            let configs: [Configure; 13] = [
                |c| c,
                |c| c.padding((0, 2, 0, 2)),
                |c| c.padding((1, 1, 0, 3)),
                |c| c.width(6),
                |c| c.width(6).padding((0, 1, 0, 0)),
                |c| c.column_first(true),
                |c| c.column_first(true).equal(true),
                |c| c.right_to_left(true),
                |c| c.right_to_left(true).column_first(true).expand(true),
                |c| c.align(HorizontalAlign::Right).equal(true),
                |c| c.align(HorizontalAlign::Center).expand(true),
                |c| c.title("[b]Words[/]"),
                |c| c.title("T").expand(true).align(HorizontalAlign::Left),
            ];
            configs
                .iter()
                .map(|configure| {
                    let words = WORDS.iter().map(|w| w.to_string()).collect();
                    print(&console, &configure(Columns::new(words)))
                })
                .collect()
        }
        "columns_renderables" => {
            let columns = Columns::from_cells(vec![
                Cell::Renderable(Arc::new(Built(|| {
                    Box::new(Panel::new(Box::new(Text::new("a"))))
                }))),
                Text::styled("bb", style("red")).into(),
                "[i]ccc[/]".into(),
                Cell::Renderable(Arc::new(Built(|| {
                    Box::new(Panel::fit(Box::new(Text::new("dddd"))))
                }))),
            ])
            .align(HorizontalAlign::Center)
            .equal(true)
            .column_first(true);
            print(&console(30), &columns)
        }
        "rule_end" => {
            let console = console(12);
            console.capture(|c| {
                c.print(&Rule::new("t").end("\n\n"));
                c.print_str("y");
                c.print(&Rule::line().end("\n\n"));
                c.print_str("z");
                c.print(&Rule::new("t").end("\n"));
                c.print(&Rule::new("t").end("!\n"));
            })
        }
        "rule_ascii" => {
            let console = console(12);
            let mut options = console.options();
            options.encoding = "ascii".to_string();
            [
                Rule::new("t"),
                Rule::line(),
                Rule::new("t").characters("="),
                Rule::new("t").align(HorizontalAlign::Left),
                Rule::new("t").align(HorizontalAlign::Right),
            ]
            .iter()
            .map(|rule| render_with(&console, rule, &options))
            .collect()
        }
        "rule_style" => {
            let themed = |styles: &[(&str, &str)]| {
                builder(12)
                    .theme(Theme::from_styles(styles.iter().copied(), true).unwrap())
                    .build()
            };
            [
                print(&console(12), &Rule::new("t").style("bold red")),
                print(&console(12), &Rule::new("t").style("rule.text")),
                print(
                    &themed(&[("rule.line", "blue"), ("rule.text", "italic")]),
                    &Rule::new("t"),
                ),
                print(&themed(&[("rule.line", "blue")]), &Rule::line()),
            ]
            .concat()
        }
        "rule_text_title" => [
            HorizontalAlign::Left,
            HorizontalAlign::Center,
            HorizontalAlign::Right,
        ]
        .iter()
        .map(|&align| {
            print(
                &console(14),
                &Rule::with_title_text(Text::styled("a\tb\n[c]", style("red"))).align(align),
            )
        })
        .collect(),
        "panel_style" => {
            let console = console(16);
            let text = |s: &str| Box::new(Text::new(s)) as Box<dyn Renderable>;
            [
                Panel::new(text("hi")).style("on blue"),
                Panel::new(text("hi\nthere"))
                    .style("red on blue")
                    .border_style("bold")
                    .title("[i]T[/]")
                    .subtitle("s"),
                Panel::new(Box::new(Text::styled("x", style("green"))))
                    .style("on blue")
                    .padding((1, 2, 1, 2)),
                Panel::new(text("h")).height(5).style("on red"),
                Panel::new(text("h")).height(2),
                Panel::new(text("a\nb\nc\nd")).height(4),
                Panel::new(text("h")).style("repr.number"),
                Panel::fit(text("h")).style("on blue").border_style("red"),
            ]
            .iter()
            .map(|panel| print(&console, panel))
            .collect()
        }
        "layout_placeholder" => [
            print(
                &builder(30).height(7).build(),
                &Layout::new().name("root").size(3).ratio(2).minimum_size(4),
            ),
            print(&builder(20).height(5).build(), &Layout::new()),
            print(&builder(40).height(4).build(), &Layout::new().name("it's")),
        ]
        .concat(),
        "layout_split" => print(&builder(40).height(8).build(), &uf_layout()),
        "layout_tree" => print(&console(50), &uf_layout().tree()),
        "layout_map" => {
            let layout = uf_layout();
            print(&builder(40).height(8).build(), &layout);
            layout
                .map()
                .iter()
                .map(|leaf| {
                    let name = leaf.name.as_deref().unwrap_or("None");
                    let r = leaf.region;
                    format!(
                        "{name}|{},{},{},{}|{}",
                        r.x,
                        r.y,
                        r.width,
                        r.height,
                        leaf.render.len()
                    )
                })
                .collect::<Vec<_>>()
                .join(";")
        }
        "layout_update_unsplit" => {
            let mut layout = uf_layout();
            layout["left"].update(Box::new(Text::new("updated")));
            let console = builder(40).height(6).build();
            let out = print(&console, &layout);
            layout["body"].unsplit();
            out + &print(&console, &layout)
        }
        "text_indent_guides" => {
            let console = console(30);
            let indented = uf_indented();
            let texts = [
                indented.with_indent_guides(None, "│", "dim green"),
                indented.with_indent_guides(Some(2), "|", "red"),
                indented.with_indent_guides(Some(3), "│", "dim green"),
                Text::styled("a\n\tb\n\n", style("on blue")).with_indent_guides(
                    Some(4),
                    "│",
                    "dim green",
                ),
                Text::styled("x\n  ", style("italic")).with_indent_guides(None, "│", "dim green"),
            ];
            let out: String = texts.iter().map(|text| print(&console, text)).collect();
            format!(
                "{out}|{},{}",
                indented.detect_indentation(),
                Text::new("a\n   b\n     c").detect_indentation()
            )
        }
        "text_from_ansi" => {
            let console = console(20);
            [
                Text::from_ansi(
                    "\x1b[1mbold\x1b[0m plain\nnext \x1b[31mred",
                    style("on blue"),
                ),
                Text::from_ansi("a\tb", style("italic")),
                Text::from_ansi("x\r\ny\n", Style::new()),
            ]
            .iter()
            .map(|text| print(&console, text))
            .collect()
        }
        "text_stylize_before" => {
            let mut text = Text::new("hello world");
            text.stylize(style("red"), 0, 3);
            text.stylize_before(style("bold on blue"), 1, 7);
            text.stylize_before(style("italic"), 5, 20);
            print(&console(20), &text)
        }
        "live_render_vertical_overflow" => {
            let mut out = String::new();
            for overflow in [
                VerticalOverflow::Crop,
                VerticalOverflow::Ellipsis,
                VerticalOverflow::Visible,
            ] {
                for live_style in ["", "on blue"] {
                    out.push_str(&live_render(overflow, live_style, "1\n2\n3\n4\n5\n6"));
                }
            }
            out + &live_render(VerticalOverflow::Ellipsis, "", "1\n2\n3\n4")
        }
        "progress_custom_column" => progress_custom_column(),
        "syntax_ansi_options" => {
            let console = console(30);
            syntax_options()
                .into_iter()
                .map(|configure| print(&console, &configure(uf_syntax())))
                .collect()
        }
        "syntax_ansi_ranges" => {
            let mut syntax = uf_syntax().line_numbers(true);
            syntax
                .stylize_range(style("reverse"), (1, 4), (2, 6), false)
                .stylize_range(style("on blue"), (3, 0), (3, 99), true)
                .stylize_range(style("bold"), (4, 0), (9, 0), false)
                .stylize_range(style("underline"), (5, 2), (5, 5), false);
            print(&console(30), &syntax)
        }
        "syntax_measure" => {
            let console = console(40);
            let configs: [fn(Syntax) -> Syntax; 5] = [
                |s| s,
                |s| s.line_numbers(true),
                |s| s.code_width(8),
                |s| s.padding_sides((0, 2, 0, 2)),
                |s| s.line_numbers(true).code_width(6).padding(1),
            ];
            configs
                .iter()
                .map(|configure| print(&console, &Panel::fit(Box::new(configure(uf_syntax())))))
                .collect()
        }
        "syntax_plain_background_theme" => {
            let console = console(30);
            let out: String = syntax_options()
                .into_iter()
                .map(|configure| print(&console, &configure(Syntax::new(UF_CODE, "python"))))
                .collect();
            strip_ansi(&out)
        }
        "print_justify_renderables" => {
            let console = console(20);
            let mut out = String::new();
            for justify in [
                Justify::Left,
                Justify::Center,
                Justify::Right,
                Justify::Full,
                Justify::Default,
            ] {
                let mut options = console.options();
                options.justify = justify;
                let renderables: [Box<dyn Renderable>; 3] = [
                    Box::new(Panel::fit(Box::new(Text::new("hi")))),
                    Box::new(small_table()),
                    Box::new(Panel::new(Box::new(Text::new("wide")))),
                ];
                for renderable in &renderables {
                    out.push_str(&print_with(&console, renderable.as_ref(), &options));
                }
            }
            out
        }
        other => panic!("no Rust builder for upstream feature case {other:?}"),
    }
}

#[test]
fn upstream_features_parity() {
    let data = include_str!("golden/upstream_features.tsv");
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
            // Show the neighbourhood of the first difference.
            let at = expected
                .char_indices()
                .zip(got.chars())
                .find(|((_, a), b)| a != b)
                .map_or(expected.len().min(got.len()), |((at, _), _)| at);
            let from = expected.floor_char_boundary(at.saturating_sub(120));
            let near = |text: &str| {
                let start = text.floor_char_boundary(from.min(text.len()));
                let end = text.floor_char_boundary((at + 120).min(text.len()));
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
    assert_eq!(checked, 31, "expected every upstream feature case to run");
}
