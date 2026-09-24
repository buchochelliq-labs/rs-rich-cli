//! Golden parity tests.
//!
//! Each fixture line is a `(name, markup, expected-ansi)` triple whose expected
//! column was captured from the **real Python `rich`** library (see
//! `scripts/capture_golden.py`). We assert byte-for-byte equality so that any
//! drift from upstream fails loudly. This is the backbone of the "stay in sync"
//! guarantee described in AGENTS.md.

use rich::markdown::Markdown;
use rich::measure::Measurement;
use rich::r#box::{Box as BoxSet, DOUBLE_EDGE, HEAVY_HEAD, SIMPLE, SQUARE};
use std::sync::Arc;

use rich::segment::Segment;
use rich::{
    Align, AnsiDecoder, Bar, Cell, ColorSystem, Columns, Console, ConsoleOptions, Constrain,
    Control, HorizontalAlign, Json, Justify, Layout, Overflow, Padding, Panel, ProgressBar,
    Renderable, Rule, Style, Styled, Syntax, Table, Text, Tree,
};

/// Build the layout matching a `layout_*` fixture name. Must stay in sync with
/// `LAYOUT_CASES` in scripts/capture_golden.py.
fn build_layout(name: &str) -> Layout {
    let leaf = |s: &str| Layout::with_renderable(Box::new(Text::new(s)));
    match name {
        "layout_column" => {
            let mut lay = Layout::new();
            lay.split_column(vec![leaf("top"), leaf("bottom")]);
            lay
        }
        "layout_row" => {
            let mut lay = Layout::new();
            lay.split_row(vec![leaf("L"), leaf("R")]);
            lay
        }
        "layout_nested" => {
            let mut top = Layout::new();
            top.split_row(vec![leaf("A"), leaf("B")]);
            let mut lay = Layout::new();
            lay.split_column(vec![top, leaf("bottom").size(1)]);
            lay
        }
        "layout_panel" => {
            let panel = Panel::new(Box::new(Text::new("hi"))).box_set(SQUARE);
            Layout::with_renderable(Box::new(panel))
        }
        "layout_row_panels" => {
            let panel = |s: &str| {
                Layout::with_renderable(Box::new(
                    Panel::new(Box::new(Text::new(s))).box_set(SQUARE),
                ))
            };
            let mut lay = Layout::new();
            lay.split_row(vec![panel("L"), panel("R")]);
            lay
        }
        other => panic!("no builder for layout fixture {other:?}"),
    }
}

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

/// Must match `_tree_deep` in scripts/capture_golden.py.
fn deep_tree() -> Tree {
    let mut tree = Tree::new("root");
    tree.add("child one").add("grand");
    tree
}

/// Must match `_tree_multiline` in scripts/capture_golden.py.
fn multiline_tree() -> Tree {
    let mut tree = Tree::new("root\nlabel");
    tree.add("child\nline two").add("grand kid");
    tree.add("last\nx");
    tree
}

/// Must match `COLUMNS_MIXED` / `COLUMNS_SIX` in scripts/capture_golden.py.
const COLUMNS_MIXED: &[&str] = &["a", "supercalifragilistic", "bc", "def"];
const COLUMNS_SIX: &[&str] = &["one", "two", "three", "four", "five", "six"];

/// A `Send + Sync` table cell holding a container renderable (`Panel`,
/// `Padding`, …), which owns a plain `Box<dyn Renderable>` and so cannot be a
/// [`Cell::Renderable`] itself. The container is rebuilt for every call, and
/// both rendering and measurement are delegated to it unchanged.
struct Built(fn() -> Box<dyn Renderable>);

impl Renderable for Built {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        (self.0)().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        (self.0)().measure(console, options)
    }
}

fn built(build: fn() -> Box<dyn Renderable>) -> Cell {
    Cell::Renderable(Arc::new(Built(build)))
}

fn text_box(s: &str) -> Box<dyn Renderable> {
    Box::new(Text::new(s))
}

/// Must match `_markup_table` in scripts/capture_golden.py.
fn markup_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("[b]Name");
    table.add_column_justify("[i]Age[/i] :rocket:", Justify::Right);
    table.add_row(&["[red]Alice[/]", "[green]30"]);
    table.add_row(&["plain [bold]x[/] y", "7"]);
    table.add_row_cells(vec![
        Cell::Text(Text::new("[b]literal[/b]")),
        Cell::from(r"\[b]escaped"),
    ]);
    table
}

/// Must match `_highlight_table` in scripts/capture_golden.py.
fn highlight_table() -> Table {
    let mut table = Table::new().box_set(SQUARE).highlight(true);
    table.add_column("value 1");
    table.add_column("[b]n[/] = 2");
    table.add_row(&["n = 42 True", "'s' None"]);
    table
}

/// Must match `_markup_tree` in scripts/capture_golden.py.
fn markup_tree() -> Tree {
    let mut tree = Tree::new("[b]root[/] :rocket:");
    let child = tree.add("[i]child[/i] 1");
    child.add("[red]leaf[/] True");
    tree.add(Text::new("[b]literal"));
    tree
}

/// Must match `_highlight_tree` in scripts/capture_golden.py.
fn highlight_tree() -> Tree {
    let mut tree = Tree::new("n = 1").highlight(true);
    tree.add("x = None");
    tree
}

/// Must match `_inner_table` in scripts/capture_golden.py.
fn inner_table() -> Table {
    let mut inner = Table::new().box_set(SQUARE);
    inner.add_column("k");
    inner.add_column("v");
    inner.add_row(&["a", "1"]);
    inner
}

/// Must match `_nested_table` in scripts/capture_golden.py.
fn nested_table() -> Table {
    let mut outer = Table::new().box_set(SQUARE);
    outer.add_column("Nested");
    outer.add_column("Note");
    outer.add_row_cells(vec![
        Cell::Renderable(Arc::new(inner_table())),
        "short".into(),
    ]);
    outer.add_row(&["x", "a longer note here"]);
    outer
}

/// Must match `_renderable_cells_table` in scripts/capture_golden.py.
fn renderable_cells_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    for header in ["Panel", "Fit", "Pad", "Align", "Constrain"] {
        table.add_column(header);
    }
    table.add_row_cells(vec![
        built(|| Box::new(Panel::new(text_box("hi")))),
        built(|| Box::new(Panel::fit(text_box("ok")))),
        built(|| Box::new(Padding::new(text_box("p"), (0, 2, 0, 2)))),
        built(|| Box::new(Align::center(text_box("mid")))),
        built(|| Box::new(Constrain::new(Box::new(Panel::new(text_box("c"))), Some(7)))),
    ]);
    table
}

/// Must match `_tree_cell_table` in scripts/capture_golden.py.
fn tree_cell_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("Tree");
    table.add_column("B");
    table.add_row_cells(vec![Cell::Renderable(Arc::new(markup_tree())), "b".into()]);
    table
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

fn width_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("Id");
    table.add_column("Note").column_width(8);
    table.add_row(&["1", "alpha beta gammagammagamma"]);
    table.add_row(&["2", "ok"]);
    table
}

fn style_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table
        .add_column("Name")
        .column_style(Style::parse("red").unwrap());
    table.add_column("Age");
    table.add_row(&["Alice", "30"]);
    table.add_row(&["Bob", "7"]);
    table
}

fn nowrap_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("Note").column_no_wrap();
    table.add_row(&["this is a fairly long note that will not fit"]);
    table.add_row(&["short"]);
    table
}

fn edge_table(pad_edge: bool, show_edge: bool) -> Table {
    let mut table = Table::new()
        .box_set(SQUARE)
        .pad_edge(pad_edge)
        .show_edge(show_edge);
    table.add_column("Name");
    table.add_column("Age");
    table.add_row(&["Alice", "30"]);
    table.add_row(&["Bob", "7"]);
    table
}

fn collapse_table() -> Table {
    let mut table = Table::new().box_set(SQUARE).collapse_padding(true);
    table.add_column("Name");
    table.add_column("Age");
    table.add_row(&["Alice", "30"]);
    table.add_row(&["Bob", "7"]);
    table
}

/// Vertical padding by row position (`_get_cells`' `get_padding`).
fn vpad_table(
    grid: bool,
    (top, right, bottom, left): (usize, usize, usize, usize),
    collapse: bool,
    pad_edge: bool,
    header: bool,
    rows: usize,
) -> Table {
    let base = if grid {
        Table::grid()
    } else {
        Table::new().box_set(SQUARE)
    };
    let mut table = base
        .padding(top, right, bottom, left)
        .collapse_padding(collapse)
        .pad_edge(pad_edge)
        .show_header(header);
    table.add_column("h1");
    table.add_column("h2");
    for index in 0..rows {
        table.add_row(&[format!("a{index}").as_str(), "b"]);
    }
    table
}

fn table_style_table() -> Table {
    let mut table = Table::new()
        .box_set(SQUARE)
        .style(Style::parse("blue").unwrap());
    table.add_column("Name");
    table.add_column("Age");
    table.add_row(&["Alice", "30"]);
    table.add_row(&["Bob", "7"]);
    table
}

fn csv_style_table() -> Table {
    // Mirrors rich-cli's render_csv styling: HEAVY_HEAD, blue border, numeric
    // column (Age) right-justified with a bold-green body + header cell.
    let mut table = Table::new()
        .box_set(HEAVY_HEAD)
        .border_style(Style::parse("blue").unwrap());
    table.add_column("Name");
    table.add_column_justify("Age", Justify::Right);
    table.column_style(Style::parse("bold green").unwrap());
    table.column_header_fill(Style::parse("bold green").unwrap());
    table.add_column("City");
    table.add_row(&["Alice", "30", "NYC"]);
    table.add_row(&["Bob", "25", "LA"]);
    table
}

fn table_ratio_table() -> Table {
    let mut table = Table::new().box_set(SQUARE).expand(true);
    table.add_column("A");
    table.column_ratio(1);
    table.add_column("B");
    table.column_ratio(2);
    table.add_row(&["x", "y"]);
    table
}

fn table_min_width_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("A");
    table.column_min_width(10);
    table.add_row(&["hi"]);
    table
}

fn table_max_width_table() -> Table {
    let mut table = Table::new().box_set(SQUARE);
    table.add_column("A");
    table.column_max_width(5);
    table.add_row(&["a very long cell value here"]);
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
        // Every fixture except `highlight.tsv` is captured with
        // `highlight=False`, so this must be set explicitly — the default is
        // ON, matching upstream.
        .highlight(false)
        .no_color(false)
        .build()
}

/// Turn the human-readable `\x1b` / `\n` / `\x1f` markers in a fixture into real
/// bytes. `\x1f` separates the pieces of a multi-result `text_ops` case.
fn unescape(s: &str) -> String {
    s.replace("\\x1b", "\x1b")
        .replace("\\n", "\n")
        .replace("\\x1f", "\x1f")
        .replace("\\x5c", "\\")
}

/// Build the renderable matching a fixture `name`. Must stay in sync with
/// `RENDERABLE_CASES` in `scripts/capture_golden.py`.
fn build_renderable(name: &str) -> Box<dyn Renderable> {
    match name {
        "text_tabs" => Box::new(Text::new("a\tb\tc")),
        "text_tabs_multiline" => Box::new(Text::new("ab\tc\nd\te")),
        "text_control_codes" => Box::new(Text::new("a\rb\x07c\x08d")),
        "rule_plain" => Box::new(Rule::line()),
        "rule_title" | "rule_title_odd" => Box::new(Rule::new("Hi")),
        "rule_left" => Box::new(Rule::new("Hi").align(HorizontalAlign::Left)),
        "rule_right" => Box::new(Rule::new("Hi").align(HorizontalAlign::Right)),
        "rule_title_markup" => Box::new(Rule::new("[bold red]Ready[/] :rocket:")),
        "rule_title_literal" => Box::new(Rule::new(r"\[red]literal\[/red]")),
        "rule_title_spaces" => Box::new(Rule::new("[bold]a\tb\nc[/]")),
        "rule_title_truncate" => Box::new(Rule::new("[red]long[/][bold blue]title[/]")),
        "rule_left_markup" => Box::new(Rule::new("[red]Title[/]").align(HorizontalAlign::Left)),
        "rule_right_markup" => Box::new(Rule::new("[red]Title[/]").align(HorizontalAlign::Right)),
        "rule_title_wide_truncate" => Box::new(Rule::new("[red]界[/][blue]abc[/]")),
        "rule_empty_title" => Box::new(Rule::new("")),
        "panel_title_markup" => Box::new(Panel::new(Box::new(Text::new("x")))
            .title("[bold red]Ready[/] :rocket:").border_style(Style::parse("blue").unwrap())),
        "panel_title_literal" => Box::new(Panel::new(Box::new(Text::new("x")))
            .title(r"\[red]literal\[/red]")),
        "panel_title_spaces" => Box::new(Panel::new(Box::new(Text::new("x")))
            .title("[bold]a\tb\nc[/]")),
        "panel_title_truncate" => Box::new(Panel::new(Box::new(Text::new("x")))
            .title("[red]long[/][bold blue]title[/]").border_style(Style::parse("green").unwrap())),
        "panel_subtitle_markup" => Box::new(Panel::new(Box::new(Text::new("x")))
            .subtitle("[italic yellow]Done[/] :rocket:").subtitle_align(HorizontalAlign::Right)
            .border_style(Style::parse("blue").unwrap())),
        "panel_subtitle_truncate" => Box::new(Panel::new(Box::new(Text::new("x")))
            .subtitle("[red]long[/][bold blue]title[/]").border_style(Style::parse("green").unwrap())),
        "panel_title_wide_truncate" => Box::new(Panel::new(Box::new(Text::new("x"))).title("[red]界[/][blue]abc[/]")),
        "panel_empty_title" => Box::new(Panel::new(Box::new(Text::new("x"))).title("").subtitle("")),
        "panel_tiny_title" => Box::new(Panel::new(Box::new(Text::new("x")))
            .title("[bold red]T[/]").subtitle("[green]S[/]")),
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
        "wrap_many_unicode" => Box::new(Text::from_markup(&format!(
            "{}\n{}", "[red]界é🙂[/]".repeat(32), "[blue]❤️xyz[/]".repeat(16)
        )).unwrap()),
        "wrap_many_words" => Box::new(Text::from_markup(&"[green]éclair 界 hello [/]".repeat(24)).unwrap()),
        "wrap_combining" => {
            let s: String = "abcdef".chars().flat_map(|c| [c, '\u{301}']).collect();
            Box::new(Text::new(s))
        }
        "panel_wrap" => {
            Box::new(Panel::new(Box::new(Text::new("The quick brown fox"))).box_set(SQUARE))
        }
        "panel_just_center" => Box::new(justified_panel(Justify::Center)),
        "panel_just_right" => Box::new(justified_panel(Justify::Right)),
        "panel_just_left" => Box::new(justified_panel(Justify::Left)),
        "text_justify_bare" => Box::new(Text::new("hi").justify(Justify::Center)),
        "table_square" => Box::new(sample_table(SQUARE)),
        "table_default" => Box::new(sample_table(HEAVY_HEAD)),
        "table_simple" => Box::new(sample_table(SIMPLE)),
        "table_double_edge" => Box::new(sample_table(DOUBLE_EDGE)),
        "table_shrink" => Box::new(shrink_table()),
        "table_expand" => Box::new(expand_table()),
        "table_justify" => Box::new(justify_table()),
        "table_title" => Box::new(title_table()),
        "table_title_markup" => Box::new(sample_table(SQUARE)
            .title("[bold red]Users[/] :rocket:").caption("[green]2 rows[/] :white_check_mark:")),
        "table_title_wrap" => Box::new(sample_table(SQUARE)
            .title("[red]Long title wraps onto lines[/]").caption("[green]a\tb\nc[/]")),
        "table_lines" => Box::new(lines_table()),
        "table_col_width" => Box::new(width_table()),
        "table_col_style" => Box::new(style_table()),
        "table_nowrap" => Box::new(nowrap_table()),
        "table_pad_edge" => Box::new(edge_table(false, true)),
        "table_no_edge" => Box::new(edge_table(true, false)),
        "table_collapse" => Box::new(collapse_table()),
        "table_vpad_grid" => Box::new(vpad_table(true, (0, 2, 1, 0), true, false, true, 3)),
        "table_vpad_boxed" => Box::new(vpad_table(false, (1, 1, 1, 1), false, true, true, 3)),
        "table_vpad_collapse" => Box::new(vpad_table(false, (2, 0, 1, 0), true, true, true, 3)),
        "table_vpad_no_edge" => Box::new(vpad_table(false, (1, 1, 2, 1), false, false, true, 3)),
        "table_vpad_header_only" => {
            Box::new(vpad_table(false, (1, 1, 1, 1), false, false, true, 0))
        }
        "table_vpad_no_header" => {
            Box::new(vpad_table(true, (1, 0, 2, 0), true, false, false, 3))
        }
        "table_style" => Box::new(table_style_table()),
        "table_csv_style" => Box::new(csv_style_table()),
        "table_ratio" => Box::new(table_ratio_table()),
        "table_min_width" => Box::new(table_min_width_table()),
        "table_max_width" => Box::new(table_max_width_table()),
        "tree_nested" => Box::new(sample_tree()),
        "tree_deep_w3" | "tree_deep_w4" | "tree_deep_w6" | "tree_deep_w10" | "tree_deep_w20" => {
            Box::new(deep_tree())
        }
        "tree_multiline_w6" | "tree_multiline_w12" => Box::new(multiline_tree()),
        "align_center" | "align_center_odd" => Box::new(Align::center(Box::new(Text::new("hi")))),
        "align_right" => Box::new(Align::right(Box::new(Text::new("hi")))),
        "constrain_panel" => Box::new(Constrain::new(
            Box::new(Panel::new(Box::new(Text::new("hi"))).box_set(SQUARE)),
            Some(10),
        )),
        "columns_two_rows" => Box::new(columns(&["one", "two", "three", "four", "five", "six"])),
        "columns_one_row" => Box::new(columns(&["alpha", "beta", "gamma", "delta"])),
        "columns_long_w8" | "columns_long_w5" | "columns_long_w3" => {
            Box::new(columns(&["supercalifragilistic"]))
        }
        "columns_wrap_w13" => Box::new(columns(&["name name name"])),
        "columns_mixed_w12" => Box::new(columns(COLUMNS_MIXED)),
        "columns_mixed_equal_w12" => Box::new(columns(COLUMNS_MIXED).equal(true)),
        "columns_mixed_expand_w12" => Box::new(columns(COLUMNS_MIXED).expand(true)),
        "columns_equal_w20" => Box::new(columns(COLUMNS_SIX).equal(true)),
        "columns_expand_w20" => Box::new(columns(COLUMNS_SIX).expand(true)),
        "columns_equal_expand_w20" => Box::new(columns(COLUMNS_SIX).equal(true).expand(true)),
        "columns_equal_expand_wrap_w13" => {
            Box::new(columns(&["name name name", "x"]).equal(true).expand(true))
        }
        "bar_empty" => Box::new(ProgressBar::new(100.0, 0.0).width(20)),
        "bar_half" => Box::new(ProgressBar::new(100.0, 50.0).width(20)),
        "bar_third" => Box::new(ProgressBar::new(100.0, 33.0).width(20)),
        "bar_full" => Box::new(ProgressBar::new(100.0, 100.0).width(20)),
        "json_python_floats" => Box::new(Json::new("[1e20,1e-7,1e16,1e15,0.0001,0.00001,-0.0,1.5,2.5e-300,123456789012345680000.0,0.1,1E+2,3.14159265358979,5e-324]").unwrap()),
        "json_python_numbers" => Box::new(Json::new("[1234567890123456789012345678901234567890,-1234567890123456789012345678901234567890,1e400,-1e400,-0]").unwrap()),
        "json_object" => Box::new(Json::new(JSON_SAMPLE).expect("valid JSON")),
        "json_unicode" => Box::new(
            Json::new("{\"name\": \"caf\u{e9}\", \"emoji\": \"\u{2764}\"}").expect("valid JSON"),
        ),
        "json_escapes_w8" | "json_escapes_w10" | "json_escapes_w12" => {
            Box::new(Json::new(r#"{"v":"a\"b\\c\nd\u0001e"}"#).expect("valid JSON"))
        }
        "markdown_doc" => Box::new(Markdown::new(
            "# Title\n\nHello **bold** and *italic* and `code`.",
        )),
        "markdown_list" => Box::new(Markdown::new("Items:\n\n- one\n- two\n\n1. a\n2. b")),
        "markdown_quote_hr" => Box::new(Markdown::new("Note:\n\n> important\n\n---\n\ndone")),
        "markdown_empty" => Box::new(Markdown::new("").hyperlinks(false)),
        "markdown_rule_only_quote" => Box::new(Markdown::new("> ---").hyperlinks(false)),
        "markdown_rule_then_quote_text" => Box::new(Markdown::new("> ---\n>\n> text").hyperlinks(false)),
        "markdown_html_then_paragraph" => Box::new(Markdown::new("<div>hidden</div>\n\nParagraph").hyperlinks(false)),
        "markdown_html_only" => Box::new(Markdown::new("<div>hidden</div>").hyperlinks(false)),
        "markdown_html_between_paragraphs" => Box::new(Markdown::new("A\n\n<div>x</div>\n\nB").hyperlinks(false)),
        "markdown_images_same_table_cell" => Box::new(Markdown::new("| h |\n|---|\n| ![a](x) ![b](y) |\n| ![c](z) |").hyperlinks(false)),
        "markdown_hr_end" => Box::new(Markdown::new("a\n\n---")),
        "markdown_image_table_cell" => Box::new(
            Markdown::new("| Icon | Name |\n| --- | --- |\n| ![crate](crate.svg) | rich |\n")
                .hyperlinks(false),
        ),
        "markdown_images_one_container" => Box::new(
            Markdown::new("Before ![one](one.svg) + ![two](two.svg) after.").hyperlinks(false),
        ),
        "markdown_badge_table" => Box::new(
            Markdown::new(
                "| Badge |\n| --- |\n| ![build](build.svg) |\n\
                 | ![docs](docs.svg) |\n| ![crate](crate.svg) |\n",
            )
            .hyperlinks(false),
        ),
        "markdown_table" => Box::new(Markdown::new(
            "| Name | Age |\n| :--- | ---: |\n| Alice | 30 |\n| Bob | 7 |\n",
        )),
        "hbar_full" => Box::new(Bar::new(100.0, 0.0, 100.0).width(20)),
        "hbar_mid" => Box::new(Bar::new(100.0, 25.0, 75.0).width(20)),
        "hbar_edge" => Box::new(Bar::new(100.0, 0.0, 33.0).width(20)),
        "control_clear" => Box::new(Control::clear()),
        "control_move" => Box::new(Control::move_(2, -1)),
        "control_move_to" => Box::new(Control::move_to(3, 4)),
        "control_hide_cursor" => Box::new(Control::show_cursor(false)),
        "ansi_bold_red" => Box::new(AnsiDecoder::new().decode_line("\x1b[1;31mhi\x1b[0m")),
        "ansi_8bit" => Box::new(AnsiDecoder::new().decode_line("\x1b[38;5;214mx\x1b[0m")),
        "ansi_truecolor" => {
            Box::new(AnsiDecoder::new().decode_line("\x1b[38;2;255;136;0mx\x1b[0m"))
        }
        "ansi_attrs" => Box::new(AnsiDecoder::new().decode_line("\x1b[3;4;9mstyled\x1b[0m")),
        "styled_on_red" => Box::new(Styled::new(
            Box::new(Text::new("hi")),
            Style::parse("on red").unwrap(),
        )),
        "styled_panel" => Box::new(Styled::new(
            Box::new(Panel::new(Box::new(Text::new("x"))).box_set(SQUARE)),
            Style::parse("green").unwrap(),
        )),
        "progress_three" => {
            // Explicit columns, as `_progress_table` in capture_golden.py:
            // upstream's default set also has a time-remaining column.
            let mut progress = rich::Progress::new().columns(vec![
                rich::ProgressColumn::Description,
                rich::ProgressColumn::Bar,
                rich::ProgressColumn::Percentage,
            ]);
            progress.add_task("Downloading", 100.0, 50.0);
            progress.add_task("Processing", 100.0, 100.0);
            progress.add_task("Waiting", 100.0, 0.0);
            Box::new(progress)
        }
        "table_markup_cells" | "measure_table_markup" => Box::new(markup_table()),
        "table_markup_highlight" => Box::new(highlight_table()),
        "tree_markup" | "measure_tree" => Box::new(markup_tree()),
        "tree_highlight" => Box::new(highlight_tree()),
        "columns_markup" => Box::new(Columns::from_cells(vec![
            "[b]one[/]".into(),
            "[red]two".into(),
            ":rocket: three".into(),
            Cell::Text(Text::new("[i]four")),
        ])),
        "table_nested" | "measure_table_nested" => Box::new(nested_table()),
        "table_renderable_cells" | "table_renderable_cells_w30" | "measure_table_renderables" => {
            Box::new(renderable_cells_table())
        }
        "table_tree_cell" => Box::new(tree_cell_table()),
        "columns_panels" => Box::new(Columns::from_cells(vec![
            built(|| Box::new(Panel::new(text_box("a")))),
            built(|| Box::new(Panel::new(text_box("bb")))),
            built(|| Box::new(Panel::fit(text_box("ccc")))),
            "d".into(),
        ])),
        "columns_panels_equal" => Box::new(
            Columns::from_cells(vec![
                built(|| Box::new(Panel::fit(text_box("a")))),
                built(|| Box::new(Panel::fit(text_box("bbbb")))),
                "cc".into(),
            ])
            .equal(true),
        ),
        "columns_tables" => Box::new(Columns::from_cells(
            (0..3)
                .map(|_| Cell::Renderable(Arc::new(inner_table())))
                .collect(),
        )),
        "panel_fit_table" => Box::new(Panel::new(Box::new(sample_table(SQUARE))).expand(false)),
        "panel_fit_title" => Box::new(Panel::fit(text_box("hi")).title("A longer title")),
        "panel_fit_rule" => Box::new(Panel::fit(Box::new(Rule::line()))),
        "panel_fit_rule_title" => Box::new(Panel::fit(Box::new(Rule::new("x"))).title("T")),
        "panel_width" => Box::new(Panel::new(text_box("x")).width(12)),
        "panel_fit_tree" => Box::new(Panel::fit(Box::new(markup_tree()))),
        "align_table" => Box::new(Align::center(Box::new(sample_table(SQUARE)))),
        "align_panel_fit" => Box::new(Align::right(Box::new(Panel::fit(text_box("x"))))),
        // Highlighting console (highlight_renderables.tsv).
        "columns_highlight" => Box::new(Columns::from_cells(vec![
            "n = 1".into(),
            "True".into(),
            "[b]x[/] 'y'".into(),
            Cell::Text(Text::new("2")),
        ])),
        "table_highlight_default_off" => Box::new(sample_table(SQUARE)),
        // Container measurement (measure_renderables.tsv).
        "measure_table" | "measure_table_narrow" => Box::new(sample_table(SQUARE)),
        "measure_grid_empty" => Box::new(Table::grid()),
        "measure_panel" => Box::new(Panel::new(text_box("hello world"))),
        "measure_panel_title" => Box::new(Panel::new(text_box("x")).title("Title here")),
        "measure_panel_width" => Box::new(Panel::new(text_box("x")).width(9)),
        "measure_padding" => Box::new(Padding::new(text_box("hello world"), (0, 2, 0, 3))),
        "measure_padding_tight" => Box::new(Padding::new(text_box("hello"), (0, 2, 0, 2))),
        "measure_constrain" => Box::new(Constrain::new(text_box("hello world wide"), Some(8))),
        "measure_align" => Box::new(Align::left(text_box("abc de"))),
        "measure_rule" => Box::new(Rule::new("title")),
        "measure_styled_panel" => Box::new(Styled::new(
            Box::new(Panel::new(text_box("x"))),
            Style::parse("red").unwrap(),
        )),
        "measure_hbar" => Box::new(Bar::new(10.0, 2.0, 5.0)),
        "measure_hbar_width" => Box::new(Bar::new(10.0, 2.0, 5.0).width(7)),
        other => panic!("no builder for renderable fixture {other:?}"),
    }
}

/// Renderables printed on a console with highlighting ON, checked against
/// upstream: `Columns` highlights `str` items with the console default, while
/// `Table` and `Tree` use their own `highlight` (default off).
#[test]
fn highlight_renderables_parity() {
    let data = include_str!("golden/highlight_renderables.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let width: usize = parts.next().unwrap().parse().unwrap();
        let expected = unescape(parts.next().unwrap());
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .highlight(true)
            .no_color(false)
            .build();
        let got = console.render_to_string(build_renderable(name).as_ref());
        assert!(
            expected == got || expected == format!("{got}\n"),
            "highlight renderable case {name:?} (line {}) diverged from upstream rich\n got: {got:?}\n exp: {expected:?}",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 5);
}

/// `Measurement.get` of containers that define `__rich_measure__` (`Table`,
/// `Tree`, `Panel`, `Padding`, `Constrain`, `Align`, `Rule`, `Styled`, `Bar`),
/// checked against upstream.
#[test]
fn measure_renderables_parity() {
    let data = include_str!("golden/measure_renderables.tsv");
    let mut checked = 0;
    for line in data.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [name, width, minimum, maximum] = cols[..] else {
            panic!("malformed measure row: {line:?}");
        };
        let width: usize = width.parse().unwrap();
        let console = Console::builder().width(width).highlight(false).build();
        let options = console.options().update_width(width);
        let got = Measurement::get(&console, &options, build_renderable(name).as_ref());
        assert_eq!(
            (got.minimum, got.maximum),
            (minimum.parse().unwrap(), maximum.parse().unwrap()),
            "measure case {name}"
        );
        checked += 1;
    }
    assert_eq!(checked, 18);
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

/// Highlighting and markup resolved against the *console's* theme, checked
/// against upstream. The only fixture set rendered with `highlight=true`.
///
/// A port that resolves highlighter styles eagerly against a global default
/// passes every other fixture in the corpus and fails these — which is exactly
/// what it did before style names were carried on spans.
#[test]
fn highlight_parity() {
    use serde_json::Value;

    let data = include_str!("golden/highlight.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let name = parts.next().unwrap_or("");
        let mut field = |what: &str| {
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing {what}", index + 1))
                .to_string()
        };
        let overrides: Value = serde_json::from_str(&field("theme")).expect("theme is json");
        let source = field("input");
        let expected = unescape(&field("expected"));

        let mut theme = rich::theme::Theme::default_theme();
        for (key, definition) in overrides.as_object().expect("theme is an object") {
            let definition = definition.as_str().expect("style definition is a string");
            theme.insert(
                key.clone(),
                Style::parse(definition).expect("fixture style must parse"),
            );
        }
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .highlight(true)
            .theme(theme)
            .build();

        assert_eq!(
            console.render_str_to_string(&source),
            expected,
            "highlight case {name:?} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no highlight cases were checked");
}

/// Render the question line for a `prompts.tsv` fixture `name`. Must stay in
/// sync with `PROMPT_CASES` in `scripts/capture_golden.py`.
fn render_prompt(console: &Console, name: &str) -> String {
    use rich::prompt::{Confirm, FloatPrompt, IntPrompt, Prompt};
    let text = match name {
        "prompt_plain" => Prompt::new("Name").make_prompt(console, None),
        "prompt_default" => Prompt::new("Name").make_prompt(console, Some("World")),
        "prompt_markup" => Prompt::new("[bold]Name[/]").make_prompt(console, None),
        "prompt_choices" => Prompt::new("Pick")
            .choices(["a", "b"])
            .make_prompt(console, None),
        "prompt_choices_default" => Prompt::new("Pick")
            .choices(["a", "b"])
            .make_prompt(console, Some("a")),
        "prompt_no_show_choices" => Prompt::new("Pick")
            .choices(["a", "b"])
            .show_choices(false)
            .make_prompt(console, Some("a")),
        "prompt_no_show_default" => Prompt::new("Pick")
            .choices(["a", "b"])
            .show_default(false)
            .make_prompt(console, Some("a")),
        "confirm_plain" => Confirm::new("Sure").make_prompt(console, None),
        "confirm_default_true" => Confirm::new("Sure").make_prompt(console, Some(true)),
        "confirm_default_false" => Confirm::new("Sure").make_prompt(console, Some(false)),
        "int_plain" => IntPrompt::new("Age").make_prompt(console, None),
        "int_default" => IntPrompt::new("Age").make_prompt(console, Some(42)),
        "float_default" => FloatPrompt::new("Ratio").make_prompt(console, Some(1.5)),
        other => panic!("unknown prompt fixture {other:?} — add it to render_prompt"),
    };
    console.render_to_string(&text)
}

/// The question line each prompt prints, checked against upstream — including
/// the `prompt.choices` / `prompt.default` styles and where the spaces fall.
#[test]
fn prompt_parity() {
    let data = include_str!("golden/prompts.tsv");
    let console = truecolor_console(80);
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let name = parts.next().unwrap_or("");
        let expected = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expected", index + 1)),
        );
        assert_eq!(
            render_prompt(&console, name),
            expected,
            "prompt case {name:?} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no prompt cases were checked");
}

/// Build the result of a `text_ops.tsv` fixture `name`. Must stay in sync with
/// `TEXT_OPS_CASES` in `scripts/capture_golden.py`.
fn build_text_op(name: &str) -> Vec<Text> {
    // The style strings go through as names, exactly as the capture script hands
    // them to Python's `Text.stylize` — no parse step on either side.
    let styled = |plain: &str, spans: &[(&str, usize, usize)]| {
        let mut text = Text::new(plain);
        for (style, start, end) in spans {
            text.stylize(*style, *start, *end);
        }
        text
    };
    // Run `mutate` on a text and return it as the single result piece.
    let mutated = |mut text: Text, mutate: &dyn Fn(&mut Text)| {
        mutate(&mut text);
        vec![text]
    };
    match name {
        "divide" => styled("hello world", &[("bold", 0, 5), ("red", 6, 11)]).divide(&[5]),
        "divide_span_across" => styled("abcdefgh", &[("bold", 2, 7)]).divide(&[4]),
        "split_newline" => styled("one\ntwo\nthree", &[("bold", 4, 7)]).split("\n", false, false),
        "split_include" => styled("one\ntwo\nthree", &[("bold", 4, 7)]).split("\n", true, false),
        "split_trailing" => Text::new("one\ntwo\n").split("\n", false, false),
        "split_trailing_blank" => Text::new("one\ntwo\n").split("\n", false, true),
        "split_absent" => Text::new("no separator here").split("|", false, false),
        "split_word" => styled("a-b-c", &[("bold", 2, 3)]).split("-", false, false),
        "pad" => mutated(styled("hi", &[("bold", 0, 2)]), &|t| t.pad(3, ' ')),
        "pad_left" => mutated(styled("hi", &[("bold", 0, 2)]), &|t| t.pad_left(3, '.')),
        "pad_right" => mutated(styled("hi", &[("bold", 0, 2)]), &|t| t.pad_right(3, '.')),
        "truncate_styled_ellipsis" => [1, 2, 5, 8]
            .into_iter()
            .map(|width| {
                let mut text = Text::from_markup("[red]long[/][bold blue]title[/]").unwrap();
                text.truncate(width, Some(Overflow::Ellipsis), false);
                text
            })
            .collect(),
        "truncate_wide_styled" => [
            (1, Overflow::Crop),
            (1, Overflow::Ellipsis),
            (2, Overflow::Ellipsis),
            (3, Overflow::Ellipsis),
        ]
        .into_iter()
        .map(|(width, overflow)| {
            let mut text = Text::from_markup("[red]界[/][blue]abc[/]").unwrap();
            text.truncate(width, Some(overflow), false);
            text
        })
        .collect(),
        "right_crop" => mutated(styled("hello world", &[("bold", 3, 9)]), &|t| {
            t.right_crop(4)
        }),
        "rstrip" => mutated(styled("hi there   ", &[("bold", 0, 2)]), &|t| t.rstrip()),
        "rstrip_end_partial" => mutated(Text::new("hello      "), &|t| t.rstrip_end(8)),
        "rstrip_end_noop" => mutated(Text::new("hi   "), &|t| t.rstrip_end(10)),
        "expand_tabs" => mutated(Text::new("a\tb\tc"), &|t| t.expand_tabs(4)),
        "expand_tabs_span_across_tabs" => {
            mutated(styled("a\tb\tc", &[("bold", 0, 5)]), &|t| t.expand_tabs(4))
        }
        "expand_tabs_styled" => mutated(styled("a\tb", &[("bold", 0, 2)]), &|t| t.expand_tabs(8)),
        "expand_tabs_multiline" => mutated(Text::new("ab\tc\nd\te"), &|t| t.expand_tabs(4)),
        "join" => vec![Text::new(", ").join(&[
            Text::new("a"),
            styled("b", &[("bold", 0, 1)]),
            Text::new("c"),
        ])],
        "join_styled_sep" => {
            vec![styled(" | ", &[("red", 0, 3)]).join(&[Text::new("x"), Text::new("y")])]
        }
        "join_empty_sep" => vec![Text::new("").join(&[Text::new("a"), Text::new("b")])],
        "highlight_words" => mutated(Text::new("the cat sat on the mat"), &|t| {
            t.highlight_words(&["cat", "mat"], "bold red", true)
                .expect("valid pattern");
        }),
        "highlight_words_nocase" => mutated(Text::new("Cat cat CAT"), &|t| {
            t.highlight_words(&["cat"], "bold", false)
                .expect("valid pattern");
        }),
        "highlight_regex" => mutated(Text::new("abc 123 def 456"), &|t| {
            t.highlight_regex(r"\d+", Some("bold cyan".into()), "")
                .expect("valid pattern");
        }),
        other => panic!("unknown text-op fixture {other:?} — add it to build_text_op"),
    }
}

/// `Text` manipulation — split, divide, padding, cropping, tab expansion,
/// joining and highlighting — checked against upstream. Rendering each result
/// pins the plain text, the span boundaries and the styles at once.
#[test]
fn text_ops_parity() {
    let data = include_str!("golden/text_ops.tsv");
    let console = truecolor_console(80);
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let name = parts.next().unwrap_or("");
        let expected = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expected", index + 1)),
        );
        let got = build_text_op(name)
            .iter()
            .map(|piece| console.render_to_string(piece))
            .collect::<Vec<_>>()
            .join("\x1f");
        assert_eq!(
            got,
            expected,
            "text-op case {name:?} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no text-op cases were checked");
}

/// Build the `Text` matching an `overflow.tsv` fixture `name`. Must stay in sync
/// with `OVERFLOW_CASES` in `scripts/capture_golden.py`.
fn build_overflow_text(name: &str) -> Text {
    let span = |plain: &str, style: &str, start: usize, end: usize| {
        let mut text = Text::new(plain);
        text.stylize(style, start, end);
        text
    };
    match name {
        "fold_long_word" | "crop_long_word" | "ellipsis_long_word" | "ignore_long_word" => {
            Text::new("supercalifragilistic")
        }
        "fold_sentence" | "crop_sentence" | "ellipsis_sentence" | "ignore_sentence" => {
            Text::new("the quick brown fox jumps")
        }
        "nowrap_fold" | "nowrap_crop" | "nowrap_ellipsis" | "nowrap_ignore" => {
            Text::new("the quick brown fox")
        }
        "nowrap_multiline" => Text::new("first line here\nsecond line here"),
        "wide_crop" | "wide_ellipsis" | "wide_ellipsis_exact" => Text::new("aa你好世"),
        "ellipsis_in_span" => span("abcdefgh", "bold", 2, 5),
        "ellipsis_at_span_start" => span("aaaabbbb", "bold red", 4, 8),
        "ellipsis_after_span" => span("aaaabbbb", "bold red", 0, 4),
        "crop_exact_width" | "ellipsis_exact_width" | "ellipsis_width_one" => Text::new("hello"),
        other => panic!("unknown overflow fixture {other:?} — add it to build_overflow_text"),
    }
}

/// `Text` overflow handling, checked against upstream end to end: wrapping (which
/// only folds under `fold`), justification, the per-line truncate, and the
/// console-level crop that `ignore` depends on to stay inside the terminal.
#[test]
fn overflow_parity() {
    let data = include_str!("golden/overflow.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(5, '\t');
        let name = parts.next().unwrap_or("");
        let mut field = |what: &str| {
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing {what}", index + 1))
                .to_string()
        };
        let width: usize = field("width").parse().expect("width must be a number");
        let overflow = match field("overflow").as_str() {
            "fold" => Overflow::Fold,
            "crop" => Overflow::Crop,
            "ellipsis" => Overflow::Ellipsis,
            "ignore" => Overflow::Ignore,
            other => panic!("line {}: unknown overflow {other:?}", index + 1),
        };
        let no_wrap = field("no_wrap") == "true";
        let expected = unescape(&field("expected"));

        // Captured with `Console.print(text, overflow=…, no_wrap=…)`: print-level
        // options, which a printed `Text` defers to after `Text.join`.
        let text = build_overflow_text(name);
        let console = truecolor_console(width);
        let mut options = console.options();
        options.overflow = Some(overflow);
        options.no_wrap = Some(no_wrap);
        let got = console.render_export_with(&text, &options);
        assert_eq!(
            got,
            expected,
            "overflow case {name:?} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no overflow cases were checked");
}

/// Markup edge cases, checked against upstream — including which side errors.
///
/// These pin the three places a hand-rolled tag scanner drifts from upstream's
/// `RE_TAGS`: backslash-run parity, a `[` inside a tag body, and zero-length
/// spans. Every one of them passed the old ad-hoc scanner's own unit tests while
/// diverging from real `rich`.
#[test]
fn markup_edge_parity() {
    let data = include_str!("golden/markup_edge.tsv");
    let console = truecolor_console(80);
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let markup = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing markup", index + 1)),
        );
        let expected = parts
            .next()
            .unwrap_or_else(|| panic!("line {}: missing expected", index + 1));

        match console.try_build_text(&markup) {
            Ok(text) => {
                assert_ne!(
                    expected,
                    "<ERROR>",
                    "edge case {name:?} (line {}): upstream raises MarkupError, we accepted it",
                    index + 1
                );
                assert_eq!(
                    console.render_to_string(&text),
                    unescape(expected),
                    "edge case {name:?} (line {}) diverged from upstream rich",
                    index + 1
                );
            }
            Err(error) => assert_eq!(
                expected,
                "<ERROR>",
                "edge case {name:?} (line {}): upstream renders this, we raised {error}",
                index + 1
            ),
        }
        checked += 1;
    }
    assert!(checked > 0, "no markup edge cases were checked");
}

/// The bundled terminal-theme palettes, checked against upstream. Each theme is
/// 18 colour triplets typed by hand, so this is the difference between a typo
/// failing the build and it quietly shifting every exported colour.
#[test]
fn terminal_theme_parity() {
    use rich::terminal_theme::TerminalTheme;
    use rich::{DEFAULT_TERMINAL_THEME, DIMMED_MONOKAI, MONOKAI, NIGHT_OWLISH, SVG_EXPORT_THEME};
    use serde_json::Value;

    fn lookup(name: &str) -> &'static TerminalTheme {
        match name {
            "DEFAULT_TERMINAL_THEME" => &DEFAULT_TERMINAL_THEME,
            "MONOKAI" => &MONOKAI,
            "DIMMED_MONOKAI" => &DIMMED_MONOKAI,
            "NIGHT_OWLISH" => &NIGHT_OWLISH,
            "SVG_EXPORT_THEME" => &SVG_EXPORT_THEME,
            other => panic!("no binding for theme {other:?}"),
        }
    }

    let data = include_str!("golden/themes.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, payload) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("line {}: expected name<TAB>json", index + 1));
        let expected: Value = serde_json::from_str(payload)
            .unwrap_or_else(|e| panic!("line {}: bad json: {e}", index + 1));
        let theme = lookup(name);

        let triplet = |c: &rich::color::ColorTriplet| vec![c.red, c.green, c.blue];
        let as_vec = |v: &Value| -> Vec<u8> {
            v.as_array()
                .expect("rgb array")
                .iter()
                .map(|n| n.as_u64().expect("channel") as u8)
                .collect()
        };

        assert_eq!(
            triplet(&theme.background),
            as_vec(&expected["background"]),
            "{name}: background diverged"
        );
        assert_eq!(
            triplet(&theme.foreground),
            as_vec(&expected["foreground"]),
            "{name}: foreground diverged"
        );
        let expected_ansi = expected["ansi"].as_array().expect("ansi array");
        assert_eq!(expected_ansi.len(), 16, "{name}: expected 16 ANSI colours");
        for (slot, want) in expected_ansi.iter().enumerate() {
            assert_eq!(
                triplet(&theme.ansi[slot]),
                as_vec(want),
                "{name}: ANSI colour {slot} diverged"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 5, "expected all five bundled themes");
}

/// Pure functions — cell widths and ratio resolution — checked against
/// upstream. These decide every layout decision the renderables make, so a
/// divergence here is invisible until it shows up as a mis-sized table.
#[test]
fn pure_function_parity() {
    use rich::cells::{cell_len, chop_cells, set_cell_size};
    use rich::ratio::{ratio_resolve, Edge};
    use serde_json::Value;

    let data = include_str!("golden/functions.tsv");
    let mut checked = 0;
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();

    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let args: Value = serde_json::from_str(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing args", index + 1)),
        )
        .unwrap_or_else(|e| panic!("line {}: bad args json: {e}", index + 1));
        let expected: Value = serde_json::from_str(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing result", index + 1)),
        )
        .unwrap_or_else(|e| panic!("line {}: bad result json: {e}", index + 1));

        let str_arg = |i: usize| args[i].as_str().expect("string arg").to_string();
        let usize_arg = |i: usize| args[i].as_u64().expect("integer arg") as usize;

        let got: Value = match name {
            "cell_len" => Value::from(cell_len(&str_arg(0))),
            "set_cell_size" => Value::from(set_cell_size(&str_arg(0), usize_arg(1))),
            "chop_cells" => Value::from(chop_cells(&str_arg(0), usize_arg(1))),
            "ratio_resolve" => {
                let edges: Vec<Edge> = args[1]
                    .as_array()
                    .expect("edge array")
                    .iter()
                    .map(|e| {
                        Edge::new(
                            e[0].as_u64().map(|v| v as usize),
                            e[1].as_u64().expect("ratio") as usize,
                            e[2].as_u64().expect("minimum_size") as usize,
                        )
                    })
                    .collect();
                Value::from(ratio_resolve(usize_arg(0), &edges))
            }
            other => panic!("line {}: no binding for function {other:?}", index + 1),
        };

        assert_eq!(
            got,
            expected,
            "{name}{args} (line {}) diverged from upstream rich",
            index + 1
        );
        seen.insert(name);
        checked += 1;
    }

    assert!(checked > 0, "no function cases were checked");
    // A fixture that silently loses a whole function should fail loudly.
    assert_eq!(
        seen.len(),
        4,
        "expected all four functions covered, got {seen:?}"
    );
}

/// A console pinned to one colour system, for the downgrade fixtures.
fn console_with_system(system: ColorSystem, width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(system))
        .width(width)
        .no_color(false)
        .build()
}

/// The same markup rendered under truecolor, 8-bit and standard — this is what
/// pins `Color::downgrade` to upstream's exact fall-back behaviour, rather than
/// only to our own unit tests.
#[test]
fn color_system_parity() {
    let data = include_str!("golden/colors.tsv");
    let mut checked = 0;
    let mut seen_systems = std::collections::BTreeSet::new();

    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let name = parts.next().unwrap_or("");
        let system_name = parts
            .next()
            .unwrap_or_else(|| panic!("line {}: missing color system", index + 1));
        let markup = parts
            .next()
            .unwrap_or_else(|| panic!("line {}: missing markup", index + 1));
        let expected = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expected", index + 1)),
        );

        // Names are upstream's `color_system=` strings.
        let system = match system_name {
            "truecolor" => ColorSystem::Truecolor,
            "256" => ColorSystem::EightBit,
            "standard" => ColorSystem::Standard,
            other => panic!("line {}: unknown color system {other:?}", index + 1),
        };
        seen_systems.insert(system_name.to_string());

        let console = console_with_system(system, 20);
        let got = console.render_str_to_string(markup);
        assert_eq!(
            got,
            expected,
            "colour case {name:?} under {system_name} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }

    assert!(checked > 0, "no colour cases were checked");
    // Guard against a fixture that silently loses a whole system.
    assert_eq!(
        seen_systems.len(),
        3,
        "expected truecolor/256/standard, got {seen_systems:?}"
    );
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

#[test]
fn layout_parity() {
    let data = include_str!("golden/layout.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let name = parts.next().unwrap_or("");
        let width: usize = parts
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("line {}: bad width", index + 1));
        let height: usize = parts
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("line {}: bad height", index + 1));
        let expected = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expected", index + 1)),
        );
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .height(height)
            .no_color(false)
            .build();
        let layout = build_layout(name);
        let got = console.capture(|c| c.print(&layout));
        assert_eq!(
            got,
            expected,
            "layout case {name:?} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no layout cases were checked");
}

/// A JSON value as the Python object `**fields` would carry.
fn format_value(value: &serde_json::Value) -> rich::pyformat::FormatValue {
    use rich::pyformat::FormatValue;
    match value {
        serde_json::Value::Null => FormatValue::None,
        serde_json::Value::Bool(flag) => FormatValue::Bool(*flag),
        serde_json::Value::Number(number) => match number.as_i64() {
            Some(integer) => FormatValue::Int(integer),
            None => FormatValue::Float(number.as_f64().expect("number")),
        },
        serde_json::Value::String(text) => FormatValue::Str(text.clone()),
        other => panic!("unsupported field value {other}"),
    }
}

/// Build a progress column from a `progress_time.tsv` spec. Keep in sync with
/// `progress_columns` in scripts/capture_golden.py.
fn progress_column(spec: &serde_json::Value) -> rich::ProgressColumn {
    use rich::{ProgressColumn, SpinnerColumn, TimeRemainingColumn};
    let spec = spec.as_array().expect("column spec");
    let flag = |i: usize| spec[i].as_bool().expect("bool");
    match spec[0].as_str().expect("column name") {
        "description" => ProgressColumn::Description,
        "bar" => ProgressColumn::Bar,
        "percentage" => ProgressColumn::Percentage,
        "task_progress" => ProgressColumn::TaskProgress {
            show_speed: flag(1),
        },
        "mofn" => ProgressColumn::MofN,
        "download" if flag(1) => ProgressColumn::BinaryDownload,
        "download" => ProgressColumn::Download,
        "elapsed" => ProgressColumn::TimeElapsed,
        "remaining" => ProgressColumn::TimeRemaining(TimeRemainingColumn::new(flag(1), flag(2))),
        "speed" => ProgressColumn::TransferSpeed,
        "filesize" => ProgressColumn::FileSize,
        "total_filesize" => ProgressColumn::TotalFileSize,
        "spinner" => ProgressColumn::Spinner(SpinnerColumn::new(
            spec[1].as_str().unwrap(),
            spec[2].as_str().unwrap(),
        )),
        "text" => ProgressColumn::TextFormat(
            rich::progress::TextColumn::new(spec[1].as_str().unwrap())
                .style(spec[2].as_str().unwrap().to_string())
                .justify(match spec[3].as_str().unwrap() {
                    "left" => Justify::Left,
                    "center" => Justify::Center,
                    "right" => Justify::Right,
                    other => panic!("unknown justify {other:?}"),
                })
                .markup(flag(4)),
        ),
        "renderable" => ProgressColumn::Renderable(std::sync::Arc::new(
            Text::from_markup(spec[1].as_str().unwrap()).expect("valid markup"),
        )),
        "bar_width" => ProgressColumn::BarWith(
            rich::BarColumn::new().bar_width(spec[1].as_u64().map(|width| width as usize)),
        ),
        "column" => progress_column(&spec[2]).with_table_column(column_options(&spec[1])),
        other => panic!("unknown progress column {other:?}"),
    }
}

/// `rich.table.Column(**options)` as [`rich::ColumnOptions`].
fn column_options(options: &serde_json::Value) -> rich::ColumnOptions {
    use rich::console::Overflow;
    let size = |key: &str| options[key].as_u64().map(|value| value as usize);
    rich::ColumnOptions {
        justify: match options["justify"].as_str() {
            None | Some("left") => Justify::Left,
            Some("center") => Justify::Center,
            Some("right") => Justify::Right,
            Some(other) => panic!("unknown justify {other:?}"),
        },
        width: size("width"),
        min_width: size("min_width"),
        max_width: size("max_width"),
        ratio: size("ratio"),
        no_wrap: options["no_wrap"].as_bool().unwrap_or(false),
        overflow: match options["overflow"].as_str() {
            None | Some("ellipsis") => Overflow::Ellipsis,
            Some("fold") => Overflow::Fold,
            Some("crop") => Overflow::Crop,
            Some(other) => panic!("unknown overflow {other:?}"),
        },
        style: options["style"]
            .as_str()
            .map(|style| rich::Style::parse(style).expect("valid style"))
            .unwrap_or_default(),
    }
}

/// Progress time/rate/spinner columns and the task API, driven by the same step
/// programs as upstream with a shared fake clock.
#[test]
fn progress_time_parity() {
    use rich::{Progress, TaskId, TaskUpdate};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let data = include_str!("golden/progress_time.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let case: serde_json::Value =
            serde_json::from_str(parts.next().expect("case")).expect("case json");
        let expected = unescape(parts.next().expect("expected"));

        let now = Arc::new(AtomicU64::new(0f64.to_bits()));
        let clock = now.clone();
        let mut progress =
            Progress::new().clock(move || f64::from_bits(clock.load(Ordering::SeqCst)));
        if let Some(columns) = case["columns"].as_array() {
            progress = progress.columns(columns.iter().map(progress_column).collect());
        }
        progress = progress.expand(case["expand"].as_bool().unwrap_or(false));
        let id = |v: &serde_json::Value| TaskId(v.as_u64().expect("task id") as usize);
        let mut got = String::new();
        for step in case["steps"].as_array().expect("steps") {
            let step = step.as_array().expect("step");
            match step[0].as_str().expect("op") {
                "time" => now.store(step[1].as_f64().unwrap().to_bits(), Ordering::SeqCst),
                "add" => {
                    let (description, total, completed) = (
                        step[1].as_str().unwrap(),
                        step[2].as_f64(),
                        step[3].as_f64().unwrap(),
                    );
                    let fields: Vec<(String, rich::pyformat::FormatValue)> = step
                        .get(5)
                        .and_then(|fields| fields.as_object())
                        .map(|fields| {
                            fields
                                .iter()
                                .map(|(name, value)| (name.clone(), format_value(value)))
                                .collect()
                        })
                        .unwrap_or_default();
                    progress.add_task_with(
                        description,
                        total,
                        completed,
                        step[4].as_bool().unwrap(),
                        fields,
                    );
                }
                "update" => {
                    let fields = &step[2];
                    let mut update = TaskUpdate {
                        total: fields["total"].as_f64(),
                        completed: fields["completed"].as_f64(),
                        advance: fields["advance"].as_f64(),
                        description: fields["description"].as_str().map(String::from),
                        visible: fields["visible"].as_bool(),
                        ..TaskUpdate::default()
                    };
                    // Any other keyword is a custom field (`update(**fields)`).
                    for (name, value) in fields.as_object().expect("update fields") {
                        if !["total", "completed", "advance", "description", "visible"]
                            .contains(&name.as_str())
                        {
                            update = update.field(name.clone(), format_value(value));
                        }
                    }
                    progress.update(id(&step[1]), update);
                }
                "advance" => progress.advance(id(&step[1]), step[2].as_f64().unwrap()),
                "start" => progress.start_task(id(&step[1])),
                "stop" => progress.stop_task(id(&step[1])),
                "reset" => {
                    let fields = &step[2];
                    progress.reset(
                        id(&step[1]),
                        fields["start"].as_bool().unwrap_or(true),
                        fields["total"].as_f64(),
                        fields["completed"].as_f64().unwrap_or(0.0),
                    );
                }
                "remove" => progress.remove_task(id(&step[1])),
                "render" => {
                    let console = truecolor_console(step[1].as_u64().unwrap() as usize);
                    got.push_str(&console.capture(|c| c.print(&progress)));
                }
                op => panic!("unknown progress step {op:?}"),
            }
        }
        assert_eq!(
            got,
            expected,
            "progress case {name:?} (line {}) diverged",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 21, "expected every progress time case to run");
}

/// Spinner and Status frames and LiveRender control sequences (#15): the same
/// step programs as upstream, one expected output per `render`/`position`/
/// `restore` step.
#[test]
fn live_status_parity() {
    use rich::style::StyleType;
    use rich::{LiveRender, Spinner, Status};
    use serde_json::Value;

    let data = include_str!("golden/live_status.tsv");
    let mut checked = 0;
    for line in data.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [name, case, expected] = cols[..] else {
            panic!("malformed live_status row: {line:?}");
        };
        let case: Value = serde_json::from_str(case).unwrap();
        let expected: Vec<String> = serde_json::from_str(expected).unwrap();
        let str_of = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(case["width"].as_u64().unwrap() as usize)
            .height(case["height"].as_u64().unwrap_or(25) as usize)
            .no_color(false)
            .build();
        let steps = case["steps"].as_array().unwrap();
        let mut outputs = Vec::new();
        match case["kind"].as_str().unwrap() {
            "spinner" => {
                let mut spinner = Spinner::new(case["name"].as_str().unwrap())
                    .text(case["text"].as_str().unwrap())
                    .speed(case["speed"].as_f64().unwrap());
                if let Some(style) = str_of(&case, "style") {
                    spinner = spinner.style(style);
                }
                for step in steps {
                    match step[0].as_str().unwrap() {
                        "render" => outputs.push(
                            console.render_to_string(&spinner.render(step[1].as_f64().unwrap())),
                        ),
                        _ => spinner.update(
                            str_of(&step[1], "text").as_deref(),
                            str_of(&step[1], "style").map(StyleType::from),
                            step[1].get("speed").and_then(Value::as_f64),
                        ),
                    }
                }
            }
            "status" => {
                let mut status = Status::new(case["message"].as_str().unwrap())
                    .spinner(case["spinner"].as_str().unwrap())
                    .spinner_style(case["style"].as_str().unwrap())
                    .speed(case["speed"].as_f64().unwrap());
                for step in steps {
                    match step[0].as_str().unwrap() {
                        "render" => outputs.push(console.render_to_string(
                            &status.renderable().render(step[1].as_f64().unwrap()),
                        )),
                        _ => status.update(
                            str_of(&step[1], "status").as_deref(),
                            str_of(&step[1], "spinner").as_deref(),
                            str_of(&step[1], "spinner_style").map(StyleType::from),
                            step[1].get("speed").and_then(Value::as_f64),
                        ),
                    }
                }
            }
            "clock" => {
                // Upstream's `Console(get_time=…)`: the renderable itself is
                // printed, and the spinner reads the frame time off the console.
                let now = std::sync::Arc::new(std::sync::Mutex::new(0.0_f64));
                let clock = now.clone();
                let console = Console::builder()
                    .force_terminal(true)
                    .color_system(Some(ColorSystem::Truecolor))
                    .width(case["width"].as_u64().unwrap() as usize)
                    .highlight(false)
                    .no_color(false)
                    .get_time(move || *clock.lock().unwrap())
                    .build();
                let spinner = || {
                    let mut spinner = Spinner::new(case["name"].as_str().unwrap())
                        .text(case["text"].as_str().unwrap())
                        .speed(case["speed"].as_f64().unwrap());
                    if let Some(style) = str_of(&case, "style") {
                        spinner = spinner.style(style);
                    }
                    spinner
                };
                let renderable: Box<dyn Renderable> = match case["target"].as_str().unwrap() {
                    "spinner" => Box::new(spinner()),
                    "status" => Box::new(
                        Status::new(case["text"].as_str().unwrap())
                            .spinner(case["name"].as_str().unwrap())
                            .spinner_style(case["style"].as_str().unwrap())
                            .speed(case["speed"].as_f64().unwrap()),
                    ),
                    _ => Box::new(Align::center(Box::new(spinner()))),
                };
                for step in steps {
                    *now.lock().unwrap() = step[1].as_f64().unwrap();
                    outputs.push(console.capture(|c| c.print(renderable.as_ref())));
                }
            }
            _ => {
                let text = |markup: &str| Box::new(Text::from_markup(markup).unwrap());
                let mut live = LiveRender::new(text(case["markup"].as_str().unwrap()));
                if let Some(style) = str_of(&case, "style") {
                    live = live.style(Style::parse(&style).unwrap());
                }
                for step in steps {
                    match step[0].as_str().unwrap() {
                        "render" => outputs.push(console.render_to_string(&live)),
                        "position" => {
                            outputs.push(console.render_to_string(&live.position_cursor()))
                        }
                        "restore" => outputs.push(console.render_to_string(&live.restore_cursor())),
                        _ => live.set_renderable(text(step[1].as_str().unwrap())),
                    }
                }
            }
        }
        assert_eq!(outputs, expected, "live_status case {name}");
        checked += 1;
    }
    assert_eq!(checked, 11);
}

/// No-colour mode (`Console(no_color=True)`): printed output loses its colours
/// but keeps every other attribute, `color_system` still reports the system,
/// and the exports — which read the recording, not the terminal output — keep
/// their colours.
#[test]
fn no_color_parity() {
    use serde_json::Value;

    let data = include_str!("golden/no_color.tsv");
    let mut checked = 0;
    for line in data.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [name, case, expected] = cols[..] else {
            panic!("malformed no_color row: {line:?}");
        };
        let case: Value = serde_json::from_str(case).unwrap();
        let expected: Value = serde_json::from_str(expected).unwrap();
        let (system, system_name) = match case["system"].as_str().unwrap() {
            "truecolor" => (ColorSystem::Truecolor, "truecolor"),
            "standard" => (ColorSystem::Standard, "standard"),
            other => panic!("no colour system {other:?} wired up"),
        };
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(system))
            .width(case["width"].as_u64().unwrap() as usize)
            .highlight(false)
            .no_color(true)
            .build();
        let print = |c: &Console| match case["markup"].as_str() {
            Some(markup) => c.print_str(markup),
            None => c.print(build_renderable(case["renderable"].as_str().unwrap()).as_ref()),
        };
        assert!(console.no_color(), "{name}");
        assert_eq!(
            console.color_system().map(|_| system_name),
            expected["color_system"].as_str(),
            "no_color case {name}: color_system"
        );
        assert_eq!(
            console.capture(print),
            expected["terminal"].as_str().unwrap(),
            "no_color case {name}: terminal output"
        );
        assert_eq!(
            console.export_text(print),
            expected["text"].as_str().unwrap(),
            "no_color case {name}: export_text"
        );
        assert_eq!(
            console.export_html(print),
            expected["html"].as_str().unwrap(),
            "no_color case {name}: export_html"
        );
        if let Some(svg) = expected.get("svg") {
            assert_eq!(
                console.export_svg("X", "test", print),
                svg.as_str().unwrap(),
                "no_color case {name}: export_svg"
            );
        }
        checked += 1;
    }
    assert_eq!(checked, 6);
}

/// The whole ask loop through a recording console: the question is printed
/// through the console (`Console.input`), so capture and export see it next
/// to the re-ask messages.
#[test]
fn prompt_ask_parity() {
    use rich::prompt::{Confirm, IntPrompt, Prompt, ScriptedInput};
    use serde_json::Value;

    let data = include_str!("golden/prompt_ask.tsv");
    let console = truecolor_console(80);
    let mut checked = 0;
    for line in data.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [name, case, expected] = cols[..] else {
            panic!("malformed prompt_ask row: {line:?}");
        };
        let case: Value = serde_json::from_str(case).unwrap();
        let expected: Value = serde_json::from_str(expected).unwrap();
        let ask = |c: &Console| -> Value {
            let answers: Vec<String> = case["answers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_string())
                .collect();
            let input = &mut ScriptedInput::new(answers);
            let question = case["question"].as_str().unwrap();
            match case["kind"].as_str().unwrap() {
                "prompt" => {
                    let mut prompt = Prompt::new(question);
                    if let Some(choices) = case.get("choices").and_then(Value::as_array) {
                        prompt = prompt.choices(choices.iter().map(|c| c.as_str().unwrap()));
                    }
                    let default = case.get("default").and_then(Value::as_str);
                    Value::from(prompt.ask_from(c, input, default).unwrap())
                }
                "confirm" => Value::from(Confirm::new(question).ask_from(c, input, None).unwrap()),
                _ => {
                    let default = case.get("default").and_then(Value::as_i64);
                    Value::from(
                        IntPrompt::new(question)
                            .ask_from(c, input, default)
                            .unwrap(),
                    )
                }
            }
        };
        let mut result = Value::Null;
        let terminal = console.capture(|c| result = ask(c));
        assert_eq!(
            terminal,
            expected["terminal"].as_str().unwrap(),
            "prompt_ask case {name}: terminal"
        );
        assert_eq!(result, expected["result"], "prompt_ask case {name}: result");
        assert_eq!(
            console.export_text(|c| {
                ask(c);
            }),
            expected["text"].as_str().unwrap(),
            "prompt_ask case {name}: export_text"
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
}

/// `Measurement.get` of `Syntax` and `JSON` (#149). The inputs travel in the
/// fixture as JSON, so no case list needs to stay in sync by name.
#[test]
fn measure_parity() {
    let data = include_str!("golden/measure.tsv");
    let console = Console::builder().width(80).build();
    let mut checked = 0;
    for line in data.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [name, width, spec, minimum, maximum] = cols[..] else {
            panic!("malformed measure row: {line:?}");
        };
        let spec: serde_json::Value = serde_json::from_str(spec).unwrap();
        let source = spec["source"].as_str().unwrap();
        let padding = spec["padding"].as_u64().unwrap() as usize;
        let renderable: Box<dyn Renderable> = match spec["kind"].as_str().unwrap() {
            "syntax" => Box::new(Syntax::new(source, "python").padding(padding)),
            _ => Box::new(Json::new(source).unwrap()),
        };
        let options = console.options().update_width(width.parse().unwrap());
        let got = Measurement::get(&console, &options, renderable.as_ref());
        assert_eq!(
            (got.minimum, got.maximum),
            (minimum.parse().unwrap(), maximum.parse().unwrap()),
            "measure case {name}"
        );
        checked += 1;
    }
    assert_eq!(checked, 15);
}

/// Markdown strikethrough delimiter pairing (markdown-it's rules), checked
/// against upstream. Data-driven: each fixture line carries its own source.
#[test]
fn markdown_strike_parity() {
    let data = include_str!("golden/markdown_strike.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let source: String =
            serde_json::from_str(parts.next().expect("source")).expect("source json");
        let expected = unescape(parts.next().expect("expected"));
        let console = truecolor_console(40);
        let got = console.capture(|c| c.print(&Markdown::new(&source).hyperlinks(false)));
        assert_eq!(
            got,
            expected,
            "markdown strike case {name:?} (line {}) diverged: {source:?}",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 24, "expected every markdown strike case to run");
}

/// Inline styling inside Markdown table cells (#9), checked against upstream.
/// Data-driven: each fixture line carries its own source.
#[test]
fn markdown_table_inline_parity() {
    let data = include_str!("golden/markdown_table_inline.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let source: String =
            serde_json::from_str(parts.next().expect("source")).expect("source json");
        let expected = unescape(parts.next().expect("expected"));
        let console = truecolor_console(40);
        let got = console.capture(|c| c.print(&Markdown::new(&source).hyperlinks(false)));
        assert_eq!(
            got,
            expected,
            "markdown table case {name:?} (line {}) diverged: {source:?}",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 8, "expected every markdown table case to run");
}

/// `Markdown(justify=…, style=…)` against upstream. Data-driven: each fixture
/// line carries its source and the options to apply.
#[test]
fn markdown_options_parity() {
    let data = include_str!("golden/markdown_options.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(4, '\t');
        let name = parts.next().unwrap_or("");
        let source: String =
            serde_json::from_str(parts.next().expect("source")).expect("source json");
        let options: serde_json::Value =
            serde_json::from_str(parts.next().expect("options")).expect("options json");
        let expected = unescape(parts.next().expect("expected"));
        let mut markdown = Markdown::new(&source).hyperlinks(false);
        if let Some(justify) = options.get("justify").and_then(|v| v.as_str()) {
            markdown = markdown.justify(match justify {
                "left" => Justify::Left,
                "center" => Justify::Center,
                "right" => Justify::Right,
                "full" => Justify::Full,
                other => panic!("unknown justify {other:?}"),
            });
        }
        if let Some(style) = options.get("style").and_then(|v| v.as_str()) {
            markdown = markdown.style(Style::parse(style).expect("valid style"));
        }
        let console = truecolor_console(40);
        let got = console.capture(|c| c.print(&markdown));
        assert_eq!(
            got,
            expected,
            "markdown options case {name:?} (line {}) diverged: {options}",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 8, "expected every markdown options case to run");
}

/// `ProgressBar` against upstream: determinate, pulse, ASCII and no-colour.
/// Data-driven: each fixture line carries its own case.
#[test]
fn progress_bar_parity() {
    use rich::progress_bar::ProgressBar;
    let data = include_str!("golden/progress_bar.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let case: serde_json::Value =
            serde_json::from_str(parts.next().expect("case")).expect("case json");
        let expected = unescape(parts.next().expect("expected"));
        let color_system = match case.get("color_system").and_then(|v| v.as_str()) {
            None | Some("truecolor") => ColorSystem::Truecolor,
            Some("256") => ColorSystem::EightBit,
            Some("standard") => ColorSystem::Standard,
            Some(other) => panic!("unknown color system {other:?}"),
        };
        let flag = |key: &str| case.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(color_system))
            .width(80)
            .highlight(false)
            .no_color(flag("no_color"))
            .legacy_windows(flag("legacy_windows"))
            .build();
        let completed = case["completed"].as_f64().expect("completed");
        let mut bar = match case["total"].as_f64() {
            Some(total) => ProgressBar::new(total, completed),
            None => ProgressBar::indeterminate(),
        }
        .width(case["width"].as_u64().expect("width") as usize)
        .pulse(flag("pulse"));
        if let Some(time) = case.get("animation_time").and_then(|v| v.as_f64()) {
            bar = bar.animation_time(time);
        }
        // A bar yields no newline of its own, so `print` adds none upstream.
        let got = console.render_to_string(&bar);
        assert_eq!(
            got,
            expected,
            "progress bar case {name:?} (line {}) diverged",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 15, "expected every progress bar case to run");
}

/// A live `Progress` display's byte stream (start, task changes, explicit
/// refreshes, stop) against upstream with `auto_refresh=False`. The port's
/// auto-refresh thread is given an interval long enough never to fire.
#[test]
fn progress_live_parity() {
    use rich::Progress;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let data = include_str!("golden/progress_live.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let case: serde_json::Value =
            serde_json::from_str(parts.next().expect("case")).expect("case json");
        let expected = unescape(parts.next().expect("expected"));
        let now = Arc::new(AtomicU64::new(0f64.to_bits()));
        let clock = now.clone();
        let columns = case["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .map(progress_column)
            .collect();
        let terminal = case["terminal"].as_bool().unwrap_or(true);
        let console = Console::builder()
            .force_terminal(terminal)
            .color_system(terminal.then_some(ColorSystem::Truecolor))
            .width(case["width"].as_u64().expect("width") as usize)
            .highlight(false)
            .no_color(false)
            .build();
        let live = Progress::new()
            .columns(columns)
            .transient(case["transient"].as_bool().unwrap_or(false))
            .disable(case["disable"].as_bool().unwrap_or(false))
            .clock(move || f64::from_bits(clock.load(Ordering::SeqCst)))
            .start(console, Vec::<u8>::new(), 1e-9);
        let id = |value: &serde_json::Value| rich::TaskId(value.as_u64().unwrap() as usize);
        for step in case["steps"].as_array().expect("steps") {
            match step[0].as_str().expect("op") {
                "time" => now.store(step[1].as_f64().unwrap().to_bits(), Ordering::SeqCst),
                "add" => {
                    live.add_task(
                        step[1].as_str().unwrap(),
                        step[2].as_f64(),
                        step[3].as_f64().unwrap(),
                    );
                }
                "advance" => live.advance(id(&step[1]), step[2].as_f64().unwrap()),
                "update" => {
                    let fields = &step[2];
                    let mut update = rich::TaskUpdate::default();
                    if let Some(completed) = fields["completed"].as_f64() {
                        update = update.completed(completed);
                    }
                    if let Some(description) = fields["description"].as_str() {
                        update = update.description(description);
                    }
                    live.update(id(&step[1]), update);
                }
                "refresh" => live.refresh(),
                other => panic!("unknown live progress step {other:?}"),
            }
        }
        let (_, bytes) = live.stop();
        let got = String::from_utf8(bytes).expect("utf-8");
        assert_eq!(
            got,
            expected,
            "live progress case {name:?} (line {}) diverged",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 7, "expected every live progress case to run");
}

/// `LogRender` against upstream's `_log_render.LogRender`: each case prints
/// its records through one render, so repeated times are blanked.
#[test]
fn log_render_parity() {
    use rich::{level_text, LogRender};
    let data = include_str!("golden/log_render.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let case: serde_json::Value =
            serde_json::from_str(parts.next().expect("case")).expect("case json");
        let expected = unescape(parts.next().expect("expected"));
        let options = &case["options"];
        let flag = |key: &str, default: bool| options[key].as_bool().unwrap_or(default);
        let mut render = LogRender::new()
            .show_time(flag("show_time", true))
            .show_level(flag("show_level", false))
            .show_path(flag("show_path", true))
            .omit_repeated_times(flag("omit_repeated_times", true));
        if let Some(width) = options.get("level_width") {
            render = render.level_width(width.as_u64().map(|w| w as usize));
        }
        let console = truecolor_console(case["width"].as_u64().expect("width") as usize);
        let mut got = String::new();
        for record in case["records"].as_array().expect("records") {
            let level = record[1].as_str().unwrap();
            let table = render.render(
                &console,
                Text::from_markup(record[2].as_str().unwrap()).expect("markup"),
                record[0].as_str().map(Text::new),
                if level.is_empty() {
                    Text::new("")
                } else {
                    level_text(level)
                },
                record[3].as_str(),
                record[4].as_u64().map(|line| line as u32),
                None,
            );
            got.push_str(&console.capture(|c| c.print(&table)));
        }
        assert_eq!(
            got,
            expected,
            "log render case {name:?} (line {}) diverged",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 4, "expected every log render case to run");
}

/// Run one `theme_stack.tsv` step list. Keep in sync with `run_theme_steps` in
/// scripts/capture_golden.py.
fn run_theme_steps(console: &mut Console, steps: &[serde_json::Value], out: &mut String) {
    use rich::errors::RichError;
    use rich::Theme;

    fn theme_of(styles: &serde_json::Value, inherit: bool) -> Theme {
        let pairs = styles
            .as_object()
            .expect("styles object")
            .iter()
            .map(|(name, style)| (name.clone(), style.as_str().expect("style").to_string()));
        Theme::from_styles(pairs, inherit).expect("fixture styles parse")
    }
    fn plain(console: &Console, text: &str, out: &mut String) {
        out.push_str(&console.capture(|c| c.print(&Text::new(text))));
    }

    for step in steps {
        let step = step.as_array().expect("step array");
        match step[0].as_str().expect("op") {
            "push" => console.push_theme(theme_of(&step[1], false), step[2].as_bool().unwrap()),
            "pop" => {
                if let Err(RichError::ThemeStack(message)) = console.pop_theme() {
                    plain(console, &format!("ERR:ThemeStackError:{message}"), out);
                }
            }
            "use" => {
                let mut themed = console.use_theme(theme_of(&step[1], false));
                run_theme_steps(&mut themed, step[2].as_array().expect("nested steps"), out);
            }
            "print" => {
                let markup = step[1].as_str().expect("markup");
                out.push_str(&console.capture(|c| c.print_str(markup)));
            }
            "config" => {
                let inherit = step[2].as_bool().unwrap();
                let theme = theme_of(&step[1], inherit);
                let text = if inherit {
                    theme.len().to_string()
                } else {
                    theme.config()
                };
                plain(console, &text, out);
            }
            "from_file" => {
                let text = step[1].as_str().expect("config text");
                let line = match Theme::from_file(text, step[2].as_bool().unwrap()) {
                    Ok(theme) => {
                        let mut lines: Vec<String> = theme
                            .names()
                            .map(|name| format!("{name}={}", theme.get(name).unwrap().definition()))
                            .collect();
                        lines.sort();
                        lines.join("\n")
                    }
                    Err(RichError::ThemeConfig(message)) => {
                        format!("ERR:{}", message.split(':').next().unwrap())
                    }
                    // Upstream's `Style.parse` wraps a bad colour in
                    // `StyleSyntaxError`; `Style::parse` reports it as
                    // `ColorParse`. Both are the style-parse failure here.
                    Err(RichError::StyleSyntax(_) | RichError::ColorParse(_)) => {
                        "ERR:StyleSyntaxError".to_string()
                    }
                    Err(other) => panic!("unexpected error {other:?}"),
                };
                plain(console, &line, out);
            }
            op => panic!("unknown theme step {op:?}"),
        }
    }
}

/// The theme stack (`push_theme`/`pop_theme`/`use_theme`) and theme config
/// files (`Theme.config`/`Theme.from_file`), checked against upstream.
#[test]
fn theme_stack_parity() {
    let data = include_str!("golden/theme_stack.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let steps: serde_json::Value =
            serde_json::from_str(parts.next().expect("steps")).expect("steps json");
        let expected = unescape(parts.next().expect("expected"));
        let mut console = truecolor_console(40);
        let mut got = String::new();
        run_theme_steps(&mut console, steps.as_array().unwrap(), &mut got);
        assert_eq!(
            got,
            expected,
            "theme stack case {name:?} (line {}) diverged",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 9, "expected every theme stack case to run");
}

/// A generic parser for the JSON-output fixtures: skips comments and blank
/// lines, splits each line into `fields` tab-separated columns, and hands the
/// (1-based line number, columns) pairs back.
fn tsv_rows(data: &str, fields: usize) -> Vec<(usize, Vec<&str>)> {
    data.lines()
        .enumerate()
        .map(|(index, raw)| (index + 1, raw.trim_end_matches('\r')))
        .filter(|(_, line)| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|(number, line)| {
            let columns: Vec<&str> = line.splitn(fields, '\t').collect();
            assert_eq!(columns.len(), fields, "line {number}: expected {fields} columns");
            (number, columns)
        })
        .collect()
}

/// The text matching a `print_justify.tsv` case. Must stay in sync with
/// `PRINT_JUSTIFY_CASES` in `scripts/capture_golden.py`.
fn build_print_justify_text(name: &str) -> Text {
    match name {
        "base_style_center" | "base_style_right" | "base_style_left" | "base_style_default" => {
            Text::styled("hi", "on red")
        }
        "base_style_and_spans_center" => {
            let mut text = Text::styled("ab cd", "bold");
            text.stylize("red", 0, 2);
            text
        }
        "base_style_wrapped_center" => Text::styled("hello world", "on blue"),
        "base_style_full" => Text::styled("aa bb cc dd", "on blue"),
        "base_style_multiline_right" => Text::styled("a\nbcd", "on green"),
        "markup_span_center" => Text::from_markup("[on red]hi[/]").unwrap(),
        other => panic!("no print_justify builder for {other:?}"),
    }
}

/// `Console.print(text, justify=…)`: upstream's `Text("").join([text])` makes
/// the base style a leading span, so the justify padding stays unstyled.
#[test]
fn print_justify_parity() {
    let rows = tsv_rows(include_str!("golden/print_justify.tsv"), 4);
    for (line, columns) in &rows {
        let name = columns[0];
        let width: usize = columns[1].parse().expect("width");
        let justify = match columns[2] {
            "default" => Justify::Default,
            "left" => Justify::Left,
            "center" => Justify::Center,
            "right" => Justify::Right,
            "full" => Justify::Full,
            other => panic!("line {line}: unknown justify {other:?}"),
        };
        let expected: String = serde_json::from_str(columns[3]).expect("expected json");
        let console = truecolor_console(width);
        let mut options = console.options();
        options.justify = justify;
        let got = console.render_export_with(&build_print_justify_text(name), &options);
        assert_eq!(got, expected, "print justify case {name:?} (line {line}) diverged");
    }
    assert_eq!(rows.len(), 9, "expected every print justify case to run");
}
