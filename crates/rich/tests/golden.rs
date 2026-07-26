//! Golden parity tests.
//!
//! Each fixture line is a `(name, markup, expected-ansi)` triple whose expected
//! column was captured from the **real Python `rich`** library (see
//! `scripts/capture_golden.py`). We assert byte-for-byte equality so that any
//! drift from upstream fails loudly. This is the backbone of the "stay in sync"
//! guarantee described in AGENTS.md.

use rich::markdown::Markdown;
use rich::r#box::{Box as BoxSet, HEAVY_HEAD, SQUARE};
use rich::{
    Align, Bar, ColorSystem, Columns, Console, Constrain, HorizontalAlign, Json, Justify, Padding,
    Panel, ProgressBar, Renderable, Rule, Table, Text, Tree,
};

fn justified_panel(justify: Justify) -> Panel {
    Panel::new(Box::new(Text::new("hi").justify(justify))).box_set(SQUARE)
}

/// Must match `JSON_SAMPLE` in scripts/capture_golden.py.
const JSON_SAMPLE: &str =
    r#"{"name": "Alice", "age": 30, "admin": true, "tags": ["a", "b"], "meta": null}"#;

fn columns(items: &[&str]) -> Columns {
    Columns::new(items.iter().map(|s| s.to_string()).collect())
}

/// The shared sample tree used by the `tree_*` fixtures.
fn sample_tree() -> Tree {
    let mut tree = Tree::new("root");
    let child_a = tree.add("child A");
    child_a.add("leaf A1");
    child_a.add("leaf A2");
    tree.add("child B");
    tree
}

/// The shared sample table used by the `table_*` fixtures.
fn sample_table(box_set: BoxSet) -> Table {
    let mut table = Table::new().box_set(box_set);
    table.add_column("Name");
    table.add_column("Age");
    table.add_row(&["Alice", "30"]);
    table.add_row(&["Bob", "7"]);
    table
}

/// A table whose wide column must shrink and wrap to fit.
fn shrink_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("Name");
    table.add_column("Description");
    table.add_row(&["Alice", "A software engineer who likes Rust"]);
    table.add_row(&["Bob", "Short bio"]);
    table
}

fn expand_table() -> Table {
    let mut table = Table::new().box_set(SQUARE).expand(true);
    table.add_column("Name");
    table.add_column("Age");
    table.add_row(&["Alice", "30"]);
    table.add_row(&["Bob", "7"]);
    table
}

fn title_table() -> Table {
    let mut table = Table::new()
        .box_set(SQUARE)
        .title("Users")
        .caption("2 rows");
    table.add_column("Name");
    table.add_column("Age");
    table.add_row(&["Alice", "30"]);
    table.add_row(&["Bob", "7"]);
    table
}

fn lines_table() -> Table {
    let mut table = Table::new().box_set(SQUARE).show_lines(true);
    table.add_column("Name");
    table.add_column("Age");
    table.add_row(&["Alice", "30"]);
    table.add_row(&["Bob", "7"]);
    table
}

fn justify_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column_justify("L", Justify::Left);
    table.add_column_justify("C", Justify::Center);
    table.add_column_justify("R", Justify::Right);
    table.add_row(&["a", "bb", "ccc"]);
    table.add_row(&["xxxx", "y", "zz"]);
    table
}

fn truecolor_console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .no_color(false)
        .build()
}

/// Turn the human-readable `\x1b` / `\n` markers in a fixture into real bytes.
fn unescape(s: &str) -> String {
    s.replace("\\x1b", "\x1b").replace("\\n", "\n")
}

/// Build the renderable matching a fixture `name`. Must stay in sync with
/// `RENDERABLE_CASES` in `scripts/capture_golden.py`.
fn build_renderable(name: &str) -> Box<dyn Renderable> {
    match name {
        "rule_plain" => Box::new(Rule::line()),
        "rule_title" | "rule_title_odd" => Box::new(Rule::new("Hi")),
        "rule_left" => Box::new(Rule::new("Hi").align(HorizontalAlign::Left)),
        "rule_right" => Box::new(Rule::new("Hi").align(HorizontalAlign::Right)),
        "panel_plain" => Box::new(Panel::new(Box::new(Text::new("hello")))),
        "panel_title" => Box::new(Panel::new(Box::new(Text::new("hello"))).title("T")),
        "panel_title_left" => Box::new(
            Panel::new(Box::new(Text::new("x")))
                .title("T")
                .title_align(HorizontalAlign::Left)
                .box_set(SQUARE),
        ),
        "panel_title_right" => Box::new(
            Panel::new(Box::new(Text::new("x")))
                .title("T")
                .title_align(HorizontalAlign::Right)
                .box_set(SQUARE),
        ),
        "panel_subtitle" => Box::new(
            Panel::new(Box::new(Text::new("x")))
                .subtitle("S")
                .box_set(SQUARE),
        ),
        "panel_subtitle_left" => Box::new(
            Panel::new(Box::new(Text::new("x")))
                .subtitle("S")
                .subtitle_align(HorizontalAlign::Left)
                .box_set(SQUARE),
        ),
        "panel_title_and_sub" => Box::new(
            Panel::new(Box::new(Text::new("x")))
                .title("T")
                .subtitle("S")
                .box_set(SQUARE),
        ),
        "panel_square" => Box::new(Panel::new(Box::new(Text::new("hi"))).box_set(SQUARE)),
        "padding_1_2" => Box::new(Padding::new(Box::new(Text::new("hi")), (1, 2, 1, 2))),
        "padding_0_1" => Box::new(Padding::new(Box::new(Text::new("hi")), (0, 1, 0, 1))),
        "wrap_words" => Box::new(Text::new("The quick brown fox")),
        "wrap_fold" => Box::new(Text::new("abcdefghij")),
        "panel_wrap" => {
            Box::new(Panel::new(Box::new(Text::new("The quick brown fox"))).box_set(SQUARE))
        }
        "panel_just_center" => Box::new(justified_panel(Justify::Center)),
        "panel_just_right" => Box::new(justified_panel(Justify::Right)),
        "panel_just_left" => Box::new(justified_panel(Justify::Left)),
        "text_justify_bare" => Box::new(Text::new("hi").justify(Justify::Center)),
        "table_square" => Box::new(sample_table(SQUARE)),
        "table_default" => Box::new(sample_table(HEAVY_HEAD)),
        "table_shrink" => Box::new(shrink_table()),
        "table_expand" => Box::new(expand_table()),
        "table_justify" => Box::new(justify_table()),
        "table_title" => Box::new(title_table()),
        "table_lines" => Box::new(lines_table()),
        "tree_nested" => Box::new(sample_tree()),
        "align_center" | "align_center_odd" => Box::new(Align::center(Box::new(Text::new("hi")))),
        "align_right" => Box::new(Align::right(Box::new(Text::new("hi")))),
        "constrain_panel" => Box::new(Constrain::new(
            Box::new(Panel::new(Box::new(Text::new("hi"))).box_set(SQUARE)),
            Some(10),
        )),
        "columns_two_rows" => Box::new(columns(&["one", "two", "three", "four", "five", "six"])),
        "columns_one_row" => Box::new(columns(&["alpha", "beta", "gamma", "delta"])),
        "bar_empty" => Box::new(ProgressBar::new(100.0, 0.0).width(20)),
        "bar_half" => Box::new(ProgressBar::new(100.0, 50.0).width(20)),
        "bar_third" => Box::new(ProgressBar::new(100.0, 33.0).width(20)),
        "bar_full" => Box::new(ProgressBar::new(100.0, 100.0).width(20)),
        "json_object" => Box::new(Json::new(JSON_SAMPLE).expect("valid JSON")),
        "markdown_doc" => Box::new(Markdown::new(
            "# Title\n\nHello **bold** and *italic* and `code`.",
        )),
        "hbar_full" => Box::new(Bar::new(100.0, 0.0, 100.0).width(20)),
        "hbar_mid" => Box::new(Bar::new(100.0, 25.0, 75.0).width(20)),
        "hbar_edge" => Box::new(Bar::new(100.0, 0.0, 33.0).width(20)),
        other => panic!("no builder for renderable fixture {other:?}"),
    }
}

#[test]
fn truecolor_parity() {
    let data = include_str!("golden/truecolor.tsv");
    let console = truecolor_console(80);
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let markup = parts
            .next()
            .unwrap_or_else(|| panic!("line {}: missing markup", index + 1));
        let expected = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expected", index + 1)),
        );
        let got = console.render_str_to_string(markup);
        assert_eq!(
            got,
            expected,
            "golden case {name:?} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no golden cases were checked");
}

#[test]
fn renderables_parity() {
    let data = include_str!("golden/renderables.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let width: usize = parts
            .next()
            .unwrap_or_else(|| panic!("line {}: missing width", index + 1))
            .parse()
            .unwrap_or_else(|_| panic!("line {}: bad width", index + 1));
        let expected = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expected", index + 1)),
        );
        let console = truecolor_console(width);
        // Most renderables print with a trailing newline, but a few (e.g.
        // ProgressBar) do not — accept either form.
        let got = console.render_to_string(build_renderable(name).as_ref());
        let matches = expected == got || expected == format!("{got}\n");
        assert!(
            matches,
            "renderable case {name:?} (line {}) diverged from upstream rich\n got: {got:?}\n exp: {expected:?}",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no renderable cases were checked");
}
