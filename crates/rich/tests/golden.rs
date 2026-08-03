//! Golden parity tests.
//!
//! Each fixture line is a `(name, markup, expected-ansi)` triple whose expected
//! column was captured from the **real Python `rich`** library (see
//! `scripts/capture_golden.py`). We assert byte-for-byte equality so that any
//! drift from upstream fails loudly. This is the backbone of the "stay in sync"
//! guarantee described in AGENTS.md.

use rich::markdown::Markdown;
use rich::r#box::{Box as BoxSet, DOUBLE_EDGE, HEAVY_HEAD, SIMPLE, SQUARE};
use rich::{
    Align, AnsiDecoder, Bar, ColorSystem, Columns, Console, Constrain, Control, HorizontalAlign,
    Json, Justify, Layout, Padding, Panel, ProgressBar, Renderable, Rule, Style, Styled, Table,
    Text, Tree,
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
        "table_lines" => Box::new(lines_table()),
        "table_col_width" => Box::new(width_table()),
        "table_col_style" => Box::new(style_table()),
        "table_nowrap" => Box::new(nowrap_table()),
        "table_pad_edge" => Box::new(edge_table(false, true)),
        "table_no_edge" => Box::new(edge_table(true, false)),
        "table_collapse" => Box::new(collapse_table()),
        "table_style" => Box::new(table_style_table()),
        "table_csv_style" => Box::new(csv_style_table()),
        "table_ratio" => Box::new(table_ratio_table()),
        "table_min_width" => Box::new(table_min_width_table()),
        "table_max_width" => Box::new(table_max_width_table()),
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
        "json_unicode" => Box::new(
            Json::new("{\"name\": \"caf\u{e9}\", \"emoji\": \"\u{2764}\"}").expect("valid JSON"),
        ),
        "markdown_doc" => Box::new(Markdown::new(
            "# Title\n\nHello **bold** and *italic* and `code`.",
        )),
        "markdown_list" => Box::new(Markdown::new("Items:\n\n- one\n- two\n\n1. a\n2. b")),
        "markdown_quote_hr" => Box::new(Markdown::new("Note:\n\n> important\n\n---\n\ndone")),
        "markdown_hr_end" => Box::new(Markdown::new("a\n\n---")),
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
            let mut progress = rich::Progress::new();
            progress.add_task("Downloading", 100.0, 50.0);
            progress.add_task("Processing", 100.0, 100.0);
            progress.add_task("Waiting", 100.0, 0.0);
            Box::new(progress)
        }
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
