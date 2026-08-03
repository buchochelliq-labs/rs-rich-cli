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
    Json, Justify, Layout, Overflow, Padding, Panel, ProgressBar, Renderable, Rule, Style, Styled,
    Table, Text, Tree,
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

/// Build the `Text` matching an `overflow.tsv` fixture `name`. Must stay in sync
/// with `OVERFLOW_CASES` in `scripts/capture_golden.py`.
fn build_overflow_text(name: &str) -> Text {
    let span = |plain: &str, style: &str, start: usize, end: usize| {
        let mut text = Text::new(plain);
        text.stylize(
            start,
            end,
            Style::parse(style).expect("test style must parse"),
        );
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

        let text = build_overflow_text(name)
            .overflow(overflow)
            .no_wrap(no_wrap);
        let got = truecolor_console(width).render_export(&text);
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
