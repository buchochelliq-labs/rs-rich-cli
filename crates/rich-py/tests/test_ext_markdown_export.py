"""``rs_rich.ext.markdown_export``: GitHub-flavoured Markdown from renderables.

Rich has no Markdown export, so each case is the Rust crate's own test
(``crates/rich-ext/src/markdown_export/tests.rs``) or doc example, built with
the Python classes, and must give the same Markdown.
"""

from __future__ import annotations

import pytest

from rs_rich.columns import Columns
from rs_rich.console import Group
from rs_rich.errors import MarkupError
from rs_rich.ext import markdown_export as gfm
from rs_rich.markdown import Markdown
from rs_rich.panel import Panel
from rs_rich.rule import Rule
from rs_rich.syntax import Syntax
from rs_rich.table import Table
from rs_rich.text import Text
from rs_rich.tree import Tree


def test_the_module_doc_example():
    table = Table()
    table.add_column("check")
    table.add_column("ms", justify="right")
    table.add_row("[bold]lint[/]", "1200")
    table.add_row("a|b", "35")
    tree = Tree("crates")
    tree.add("rich").add("table.rs")
    doc = gfm.GfmDocument()
    doc.push_heading(2, "Report").push(table).push(Rule()).push(tree)
    assert gfm.fit_github_comment(doc.finish()) == (
        "## Report\n\n"
        "| check | ms |\n"
        "| --- | ---: |\n"
        "| **lint** | 1200 |\n"
        "| a\\|b | 35 |\n\n"
        "---\n\n"
        "- crates\n  - rich\n    - table.rs\n"
    )


def test_table_with_header_alignment_styles_and_escapes():
    table = Table()
    table.add_column("name")
    table.add_column("size", justify="right")
    table.add_column("state", justify="center")
    table.add_row("[bold]a|b[/]", "10", "ok")
    table.add_row("[link=https://x.test/a(1)]docs[/] and [i]more[/]", "2", "failing")
    table.add_row("[s]old[/] `x`", "300", "-")
    assert gfm.to_gfm(table) == (
        "| name | size | state |\n"
        "| --- | ---: | :---: |\n"
        "| **a\\|b** | 10 | ok |\n"
        "| [docs](https://x.test/a%281%29) and *more* | 2 | failing |\n"
        "| ~~old~~ \\`x\\` | 300 | - |"
    )


def test_table_title_caption_and_footer():
    table = Table(title="Build [b]times[/]", caption="from CI", show_footer=True)
    table.add_column("job", "total")
    table.add_column("s", "12", justify="right")
    table.add_row("lint", "2")
    table.add_row("test", "10")
    assert gfm.to_gfm(table) == (
        "**Build times**\n\n"
        "| job | s |\n"
        "| --- | ---: |\n"
        "| lint | 2 |\n"
        "| test | 10 |\n"
        "| **total** | **12** |\n\n"
        "*from CI*"
    )


def test_tables_without_headers_and_grids():
    table = Table(show_header=False)
    table.add_column("a")
    table.add_column("b")
    table.add_row("1", "2")
    assert gfm.to_gfm(table) == "|  |  |\n| --- | --- |\n| 1 | 2 |"
    grid = Table.grid()
    grid.add_column("a")
    grid.add_column("b", justify="right")
    grid.add_row("x")
    assert gfm.to_gfm(grid) == "|  |  |\n| --- | ---: |\n| x |  |"


def test_multi_line_cells_and_plain_cells():
    table = Table(show_lines=True)
    table.add_column("k")
    table.add_column("v")
    table.add_row("a", "one\ntwo")
    table.add_plain_row("[b]", "three")
    assert gfm.to_gfm(table) == "| k | v |\n| --- | --- |\n| a | one<br>two |\n| \\[b\\] | three |"


def test_a_renderable_cell_is_its_plain_rendering():
    table = Table(row_styles=["bold", "none"], title=Text("[not markup]"))
    table.add_column("long")
    table.add_column("cell")
    table.add_row(("word " * 40).strip(), "a")
    table.add_row(Text("[b]"), Rule())
    out = gfm.to_gfm(table, gfm.GfmOptions(width=20)).splitlines()
    assert out[0] == "**\\[not markup\\]**"
    assert out[4] == "| **{}** | **a** |".format(("word " * 40).strip())
    assert out[5] == "| \\[b\\] | {} |".format("─" * 20)


def test_tree_as_nested_lists():
    tree = Tree("[bold]root[/]")
    child = tree.add("child [code]x()[/]")
    child.add(Text("leaf [not markup]"))
    child.add("- dash")
    hidden = tree.add("closed", expanded=False)
    hidden.add("unseen")
    assert gfm.to_gfm(tree) == (
        "- **root**\n"
        "  - child `x()`\n"
        "    - leaf \\[not markup\\]\n"
        "    - \\- dash\n"
        "  - closed"
    )
    hidden_root = Tree("root", hide_root=True)
    hidden_root.add("a")
    hidden_root.add_plain("[b]")
    assert gfm.to_gfm(hidden_root) == "- a\n- \\[b\\]"


def panel():
    return Panel(Text.from_markup("hello [i]world[/]\nsecond\n\nnew paragraph"), title="Status", subtitle="v1")


def test_panel_as_blockquote_and_details():
    assert gfm.to_gfm(panel()) == (
        "> **Status**\n>\n> hello *world*\\\n> second\n>\n> new paragraph\n>\n> *v1*"
    )
    details = gfm.GfmExporter(gfm.GfmOptions(panel=gfm.PanelStyle.details(open=True)))
    assert details.export(panel()) == (
        "<details open><summary>Status</summary>\n\n"
        "hello *world*\\\nsecond\n\nnew paragraph\n\n*v1*\n\n</details>"
    )
    untitled = gfm.GfmOptions(panel="details", untitled_summary="More <info>")
    assert gfm.to_gfm(Panel(Text("x")), untitled) == (
        "<details><summary>More &lt;info&gt;</summary>\n\nx\n\n</details>"
    )


def test_a_markup_panel_body_and_a_drawing():
    assert gfm.to_gfm(Panel("all [b]green[/]", title="CI")) == "> **CI**\n>\n> all **green**"
    inner = Table()
    inner.add_column("a")
    inner.add_row("1")
    assert gfm.to_gfm(Panel(inner, title="T")) == (
        "> **T**\n>\n> ```text\n> ┏━━━┓\n> ┃ a ┃\n> ┡━━━┩\n> │ 1 │\n> └───┘\n> ```"
    )
    columns = Columns(["a", "b"])
    assert gfm.to_gfm(Panel(columns, title=Text("[x]"))) == "> **\\[x\\]**\n>\n> a b"


def test_rules():
    assert gfm.to_gfm(Rule()) == "---"
    assert gfm.to_gfm(Rule("Next [b]part[/]")) == "---\n\n**Next part**"
    assert gfm.to_gfm(Rule("x", characters="=")) == "---\n\n**x**"
    assert gfm.to_gfm(Rule(Text("[b]"))) == "---\n\n**\\[b\\]**"


def test_markdown_syntax_text_and_markup():
    assert gfm.to_gfm(Markdown("# Title\n\n- *item*\n")) == "# Title\n\n- *item*"
    syntax = Syntax("select 1;\n\tx", "sql", line_numbers=True, theme="monokai")
    assert gfm.to_gfm(syntax) == "```sql\nselect 1;\n    x\n```"
    text = Text("bold  code`tick` end")
    text.stylize("bold", 0, 6)
    text.stylize("code", 6, 16)
    assert gfm.to_gfm(text) == "**bold**  `` code`tick` `` end"
    assert gfm.to_gfm("[bold]x[/] <y> & z_") == "**x** \\<y\\> \\& z\\_"


def test_groups_write_each_item_as_a_block():
    table = Table()
    table.add_column("a")
    table.add_row("1")
    out = gfm.to_gfm(Group(Text("first"), table, "[i]last[/]"))
    assert out == "first\n\n| a |\n| --- |\n| 1 |\n\n*last*"
    # What the exporter cannot read is its plain rendering.
    assert gfm.to_gfm(Columns(["a", "b"])) == "```text\na b\n```"


def test_bad_markup_raises_as_printing_does():
    with pytest.raises(MarkupError):
        gfm.to_gfm("[/bold]")
    table = Table()
    table.add_column("a")
    table.add_row("[/x]")
    with pytest.raises(MarkupError):
        gfm.to_gfm(table)
    # The exporter's own markup method takes it literally, as Rust's does.
    assert gfm.GfmExporter().markup("[/x]") == "\\[/x\\]"


def test_python_renderables_are_read_back():
    class Custom:
        def __rich_console__(self, console, options):
            yield "custom output"

    assert gfm.to_gfm(Custom()) == "```text\ncustom output\n```"

    class Cast:
        def __rich__(self):
            return Rule("cast")

    assert gfm.to_gfm(Cast()) == "---\n\n**cast**"


def test_documents_join_blocks():
    doc = gfm.GfmDocument()
    doc.push_heading(9, "Big [i]deal[/]").push_markdown("- already *markdown*\n").push_code("a ``` b").push(Text(""))
    assert doc.finish() == "###### Big *deal*\n\n- already *markdown*\n\n````\na ``` b\n````\n"
    assert str(doc) == doc.finish() and len(doc) == 3
    assert doc.blocks[0] == "###### Big *deal*"
    assert gfm.GfmDocument().finish() == ""
    plain = gfm.GfmDocument().push_plain(Text("**"))
    assert plain.finish() == "```text\n**\n```\n"


def test_exporter_helpers():
    exporter = gfm.GfmExporter()
    assert exporter.markup("[bold]ok[/] and [link=https://example.com]docs[/]") == "**ok** and [docs](https://example.com)"
    assert exporter.text(Text("2 * 3")) == "2 \\* 3"
    assert exporter.text("2 * 3") == "2 \\* 3"
    assert exporter.code("SELECT 1", "sql") == "```sql\nSELECT 1\n```"
    assert exporter.plain(Text("**")) == "```text\n**\n```"


def test_truncation():
    assert gfm.truncate_markdown("short", 10) == "short"
    long = "```text\n" + "line\n" * 100 + "```\n"
    cut = gfm.truncate_markdown(long, 200)
    assert len(cut) <= 200
    assert "\n```\n" in cut
    assert cut.endswith("characters shown.*\n")
    assert gfm.truncate_markdown("abcdef", 3) == "abc"
    text = "é" * gfm.GITHUB_COMMENT_LIMIT
    assert gfm.fit_github_comment(text) == text
    cut = gfm.fit_github_comment(text + "\nmore")
    assert len(cut) <= gfm.GITHUB_COMMENT_LIMIT and "*Truncated: " in cut


def test_options():
    options = gfm.GfmOptions()
    assert (options.width, options.panel, options.untitled_summary) == (100, gfm.PanelStyle(), "Details")
    assert options.code_styles == ["code", "markdown.code", "inline_code"]
    assert repr(options) == (
        "GfmOptions(width=100, panel=PanelStyle('blockquote'), "
        "code_styles=['code', 'markdown.code', 'inline_code'], untitled_summary='Details')"
    )
    styled = Text("call()")
    styled.stylize("mine", 0, 6)
    assert gfm.to_gfm(styled, gfm.GfmOptions(code_styles=["mine"])) == "`call()`"
    assert gfm.PanelStyle("details", open=True) == gfm.PanelStyle.details(True)
    assert gfm.PanelStyle.details().kind == "details" and not gfm.PanelStyle.details().open
    with pytest.raises(ValueError, match="panel style"):
        gfm.PanelStyle("fold")
    with pytest.raises(ValueError, match="open applies"):
        gfm.PanelStyle("blockquote", open=True)
    with pytest.raises(ValueError, match="width"):
        gfm.GfmOptions(width=0)
    with pytest.raises(TypeError, match="panel"):
        gfm.GfmOptions(panel=3)
    assert gfm.GITHUB_COMMENT_LIMIT == 65536
