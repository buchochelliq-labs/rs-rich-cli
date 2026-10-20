use super::*;
use rich::{Justify, Text};
use std::sync::Arc;

fn gfm(value: &(impl ToGfm + ?Sized)) -> String {
    GfmExporter::new().export(value)
}

#[test]
fn table_with_header_alignment_styles_and_escapes() {
    let mut table = Table::new();
    table.add_column("name");
    table.add_column_justify("size", Justify::Right);
    table.add_column_justify("state", Justify::Center);
    table.add_row(&["[bold]a|b[/]", "10", "ok"]);
    table.add_row(&[
        "[link=https://x.test/a(1)]docs[/] and [i]more[/]",
        "2",
        "failing",
    ]);
    table.add_row(&["[s]old[/] `x`", "300", "-"]);
    assert_eq!(
        gfm(&table),
        "| name | size | state |\n\
         | --- | ---: | :---: |\n\
         | **a\\|b** | 10 | ok |\n\
         | [docs](https://x.test/a%281%29) and *more* | 2 | failing |\n\
         | ~~old~~ \\`x\\` | 300 | - |"
    );
}

#[test]
fn table_title_caption_and_footer() {
    let mut table = Table::new()
        .title("Build [b]times[/]")
        .caption("from CI")
        .show_footer(true);
    table.add_column("job");
    table.column_footer("total");
    table.add_column_justify("s", Justify::Right);
    table.column_footer("12");
    table.add_row(&["lint", "2"]);
    table.add_row(&["test", "10"]);
    assert_eq!(
        gfm(&table),
        "**Build times**\n\n\
         | job | s |\n\
         | --- | ---: |\n\
         | lint | 2 |\n\
         | test | 10 |\n\
         | **total** | **12** |\n\n\
         *from CI*"
    );
}

#[test]
fn table_without_header_gets_an_empty_one() {
    let mut table = Table::new().show_header(false);
    table.add_column("a");
    table.add_column("b");
    table.add_row(&["1", "2"]);
    assert_eq!(gfm(&table), "|  |  |\n| --- | --- |\n| 1 | 2 |");
}

#[test]
fn table_rows_with_lines_keep_multi_line_cells() {
    let mut table = Table::new().show_lines(true);
    table.add_column("k");
    table.add_column("v");
    table.add_row(&["a", "one\ntwo"]);
    table.add_row(&["b", "three"]);
    assert_eq!(
        gfm(&table),
        "| k | v |\n| --- | --- |\n| a | one<br>two |\n| b | three |"
    );
}

#[test]
fn tables_are_read_from_their_cells_not_their_rendering() {
    // A wrapped cell without `show_lines`, which a rendering cannot tell
    // from two rows, and a grid, which has no borders to read.
    let mut table = Table::new();
    table.add_column("v");
    table.add_row(&["one\ntwo"]);
    table.add_row(&["three"]);
    assert_eq!(gfm(&table), "| v |\n| --- |\n| one<br>two |\n| three |");
    let mut grid = Table::grid();
    grid.add_column("a");
    grid.add_column_justify("b", Justify::Right);
    grid.add_row(&["x"]);
    assert_eq!(gfm(&grid), "|  |  |\n| --- | ---: |\n| x |  |");
    // Wider than the export width, styled rows, literal text, a renderable.
    let mut wide = Table::new()
        .row_styles(vec!["bold".into(), "none".into()])
        .title_text(Text::new("[not markup]"));
    wide.add_column("long");
    wide.add_column("cell");
    wide.add_row(&["word ".repeat(40).trim_end(), "a"]);
    wide.add_row_cells(vec![
        Cell::Text(Text::new("[b]")),
        Cell::Renderable(Arc::new(Rule::line())),
    ]);
    let out = GfmExporter::with_options(GfmOptions::new().width(20)).export(&wide);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "**\\[not markup\\]**", "{out}");
    assert_eq!(
        lines[4],
        format!("| **{}** | **a** |", "word ".repeat(40).trim_end())
    );
    assert_eq!(lines[5], format!("| \\[b\\] | {} |", "─".repeat(20)));
}

#[test]
fn columns_fall_back_to_their_rendering() {
    let columns = Columns::new(vec!["a".into(), "b".into()]);
    let out = gfm(&Panel::new(Box::new(columns)).title_as_text(Text::new("[x]")));
    assert_eq!(out, "> **\\[x\\]**\n>\n> a b");
}

#[test]
fn tree_as_nested_lists() {
    let mut tree = Tree::new("[bold]root[/]");
    let child = tree.add("child [code]x()[/]");
    child.add(Text::new("leaf [not markup]"));
    child.add("- dash");
    let mut hidden = Tree::new("closed").expanded(false);
    hidden.add("unseen");
    tree.add_tree(hidden);
    assert_eq!(
        gfm(&tree),
        "- **root**\n\
         \x20 - child `x()`\n\
         \x20   - leaf \\[not markup\\]\n\
         \x20   - \\- dash\n\
         \x20 - closed"
    );
}

#[test]
fn tree_with_hidden_root_lists_its_children() {
    let mut tree = Tree::new("root").hide_root(true);
    tree.add("a");
    tree.add("b");
    assert_eq!(gfm(&tree), "- a\n- b");
}

#[test]
fn panel_as_blockquote_and_details() {
    let panel = || {
        Panel::new(Box::new(
            Text::from_markup("hello [i]world[/]\nsecond\n\nnew paragraph").unwrap(),
        ))
        .title("Status")
        .subtitle("v1")
    };
    assert_eq!(
        gfm(&panel()),
        "> **Status**\n>\n> hello *world*\\\n> second\n>\n> new paragraph\n>\n> *v1*"
    );
    let details =
        GfmExporter::with_options(GfmOptions::new().panel(PanelStyle::Details { open: true }));
    assert_eq!(
        details.export(&panel()),
        "<details open><summary>Status</summary>\n\n\
         hello *world*\\\nsecond\n\nnew paragraph\n\n*v1*\n\n</details>"
    );
    let untitled = GfmExporter::with_options(
        GfmOptions::new()
            .panel(PanelStyle::Details { open: false })
            .untitled_summary("More <info>"),
    );
    assert_eq!(
        untitled.export(&Panel::new(Box::new(Text::new("x")))),
        "<details><summary>More &lt;info&gt;</summary>\n\nx\n\n</details>"
    );
}

#[test]
fn panel_around_a_drawing_is_fenced() {
    let mut inner = Table::new();
    inner.add_column("a");
    inner.add_row(&["1"]);
    let out = gfm(&Panel::new(Box::new(inner)).title("T"));
    assert_eq!(
        out,
        "> **T**\n>\n> ```text\n> ┏━━━┓\n> ┃ a ┃\n> ┡━━━┩\n> │ 1 │\n> └───┘\n> ```"
    );
}

#[test]
fn rules() {
    assert_eq!(gfm(&Rule::line()), "---");
    assert_eq!(gfm(&Rule::new("Next [b]part[/]")), "---\n\n**Next part**");
    assert_eq!(gfm(&Rule::new("x").characters("=")), "---\n\n**x**");
    // A title wider than the export width, and a literal one.
    let long = "word ".repeat(30);
    let out = GfmExporter::with_options(GfmOptions::new().width(20)).export(&Rule::new(&long));
    assert_eq!(out, format!("---\n\n**{}**", long.trim_end()));
    assert_eq!(
        gfm(&Rule::with_title_text(Text::new("[b]"))),
        "---\n\n**\\[b\\]**"
    );
}

#[cfg(feature = "markdown")]
#[test]
fn markdown_is_written_as_its_source() {
    let markdown = rich::markdown::Markdown::new("# Title\n\n- *item*\n");
    assert_eq!(gfm(&markdown), "# Title\n\n- *item*");
}

#[test]
fn text_paragraphs_and_escapes() {
    let text = Text::from_markup(
        "# not a heading\n1. not a list\n    indented *star*\n\n[bold]a [italic]b[/italic][/bold] c",
    )
    .unwrap();
    assert_eq!(
        gfm(&text),
        "\\# not a heading\\\n1\\. not a list\\\n\u{a0}\u{a0}\u{a0}\u{a0}indented \\*star\\*\n\n**a *b*** c"
    );
}

#[test]
fn markers_hug_text_and_code_spans_grow_their_fences() {
    let mut text = Text::new("bold  code`tick` end");
    text.stylize("bold", 0, 6);
    text.stylize("code", 6, 16);
    assert_eq!(gfm(&text), "**bold**  `` code`tick` `` end");
    let mut strike = Text::new("gone");
    strike.stylize("strike", 0, 4);
    assert_eq!(gfm(&strike), "~~gone~~");
}

#[test]
fn markup_strings_and_groups() {
    assert_eq!(gfm("[bold]x[/] <y> & z_"), "**x** \\<y\\> \\& z\\_");
    let group = Renderables::new(vec![Arc::new(Text::new("first")), Arc::new(Rule::line())]);
    let out = gfm(&group);
    assert!(out.starts_with("first\n\n```text\n───"), "{out}");
}

#[test]
fn columns_are_plain_text() {
    let columns = Columns::new(vec!["a".into(), "b".into()]);
    assert_eq!(gfm(&columns), "```text\na b\n```");
}

#[test]
fn documents_join_blocks() {
    let mut doc = GfmDocument::new();
    doc.push_heading(9, "Big [i]deal[/]")
        .push_markdown("- already *markdown*\n")
        .push_code("a ``` b", "")
        .push(&Text::new(""));
    assert_eq!(
        doc.finish(),
        "###### Big *deal*\n\n- already *markdown*\n\n````\na ``` b\n````\n"
    );
    assert_eq!(GfmDocument::new().finish(), "");
}

#[cfg(feature = "syntax")]
#[test]
fn syntax_is_fenced_with_its_language() {
    let syntax = rich::Syntax::new("select 1;\n\tx", "sql")
        .line_numbers(true)
        .theme("monokai");
    assert_eq!(gfm(&syntax), "```sql\nselect 1;\n    x\n```");
    assert_eq!(gfm(&rich::Syntax::new("plain", "")), "```\nplain\n```");
}

#[test]
fn truncation_closes_fences_and_details() {
    assert_eq!(truncate_markdown("short", 10), "short");
    let fenced_body = format!(
        "<details><summary>s</summary>\n\n```text\n{}```\n\n</details>\n",
        "line\n".repeat(50)
    );
    let cut = truncate_markdown(&fenced_body, 150);
    assert!(cut.chars().count() <= 150, "{}", cut.chars().count());
    let kept = cut
        .split("\n```\n\n</details>\n\n*Truncated: ")
        .next()
        .unwrap();
    assert!(
        kept.starts_with("<details><summary>s</summary>\n\n```text\nline\n"),
        "{cut}"
    );
    assert!(
        cut.ends_with(&format!(
            " of {} characters shown.*\n",
            fenced_body.chars().count()
        )),
        "{cut}"
    );
    // Blockquoted fences close with their prefix.
    let quoted = format!("> ```text\n{}", "> x\n".repeat(40));
    let cut = truncate_markdown(&quoted, 100);
    assert!(cut.contains("> x\n> ```\n\n*Truncated"), "{cut}");
    // A limit too small for the note keeps a prefix.
    assert_eq!(truncate_markdown("abcdef", 3), "abc");
}

#[test]
fn github_limit_counts_characters() {
    let text = "é".repeat(GITHUB_COMMENT_LIMIT);
    assert_eq!(fit_github_comment(&text), text);
    let long = format!("{text}\nmore");
    let cut = fit_github_comment(&long);
    assert!(cut.chars().count() <= GITHUB_COMMENT_LIMIT);
    assert!(cut.contains("*Truncated: "));
}
