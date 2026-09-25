//! Parity for upstream options the Python bindings needed from core, against
//! `golden/upstream_features.tsv` (captured by `UPSTREAM_FEATURE_CASES` in
//! `scripts/capture_golden.py` from real rich 15.0.0). Each fixture line is
//! `name<TAB>json(expected)`; every name has a Rust builder below.

use std::sync::Arc;

use rich::measure::Measurement;
use rich::{
    Cell, ColorSystem, Columns, Console, ConsoleOptions, HorizontalAlign, Justify, Panel,
    Renderable, Rule, Segment, Style, Table, Text, Theme, Tree, VerticalAlign,
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
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
