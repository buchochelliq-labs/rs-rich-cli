"""Robustness regressions: Text, renderables and code, found by differential
fuzzing against Rich 15.0.0 in the 0.0.12 audit.

Each test is one root cause. Programs are written once against Rich's API and
run under ``rich`` 15.0.0 and ``rs_rich``; output bytes (and exception types)
must match. Crashes and super-linear slowdowns run in a subprocess with a
timeout so a regression cannot take the test session down or hang it.
"""

from __future__ import annotations

import importlib
import io
import subprocess
import sys
import textwrap
from types import SimpleNamespace

import pytest

MODULES = [
    "console", "text", "style", "color", "table", "panel", "box", "rule", "padding", "align",
    "columns", "constrain", "styled", "tree", "markdown", "syntax", "pretty", "spinner", "markup",
]


def load(package: str) -> SimpleNamespace:
    return SimpleNamespace(**{name: importlib.import_module(f"{package}.{name}") for name in MODULES})


RICH = load("rich")
RS = load("rs_rich")


def run(m, program, width=40, color=False, **console_options):
    """Run program(m, console); the output, or the exception's type name."""
    console = m.console.Console(
        file=io.StringIO(),
        width=width,
        force_terminal=color,
        color_system="truecolor" if color else None,
        **console_options,
    )
    try:
        program(m, console)
    except Exception as error:  # noqa: BLE001 - the type is the result
        return f"<{type(error).__name__}>"
    return console.file.getvalue()


def compare(program, **options):
    expected = run(RICH, program, **options)
    actual = run(RS, program, **options)
    assert actual == expected
    return actual


def run_isolated(code: str, timeout: float = 60.0) -> subprocess.CompletedProcess:
    """Run code in a fresh interpreter (a crash or hang must not kill pytest)."""
    return subprocess.run(
        [sys.executable, "-c", textwrap.dedent(code)],
        capture_output=True,
        text=True,
        timeout=timeout,
    )


# ---------------------------------------------------------------------------
# High: interpreter crash / effective hang


def test_pretty_of_a_deeply_nested_list_does_not_crash_the_interpreter():
    # rs_rich/src/code/pretty.rs `walk` recurses without a depth guard: a
    # list nested ~250 deep overflows the native stack (SIGSEGV). Rich prints it.
    result = run_isolated(
        """
        import io
        from rs_rich.console import Console
        from rs_rich.pretty import traverse
        x = []
        y = x
        for _ in range(600):
            z = []; y.append(z); y = z
        try:
            traverse(x)
            Console(file=io.StringIO()).print(x)
        except RecursionError:
            pass
        print("survived")
        """
    )
    assert result.returncode == 0, f"interpreter died with {result.returncode}"
    assert "survived" in result.stdout


def test_printing_many_spans_is_not_quadratic():
    # core Text::line_segments (crates/rich/src/text.rs) scans every span for
    # every line and every cut: print(list(range(10000))) takes ~80 s against
    # Rich's 0.7 s, 20000 items ~5 minutes.
    code = """
        import io, time
        from rs_rich.console import Console
        start = time.time()
        Console(file=io.StringIO(), width=100).print(list(range(20000)))
        print(time.time() - start)
        """
    try:
        result = run_isolated(code, timeout=30)
    except subprocess.TimeoutExpired:
        pytest.fail("printing a 20000-item list took over 30 s (Rich: under 2 s)")
    assert result.returncode == 0


# ---------------------------------------------------------------------------
# Medium


def test_markup_false_leaves_strings_inside_renderables_literal():
    # Console(markup=False) is honoured for a top-level string but not for one
    # inside a Panel, Table, Rule, Columns, Padding, Align, Group or Styled:
    # rs_rich parses it (applying [link=...] and styles) and even raises
    # MarkupError at construction (rich-py/src/renderable.rs `to_renderable`).
    def program(m, c):
        table = m.table.Table("[b]h[/b]")
        table.add_row("[link=https://example.com]click[/link]")
        c.print(m.panel.Panel("[b]x[/b]"), table, m.rule.Rule("[i]t[/i]"),
                m.columns.Columns(["[b]c[/b]"]), m.console.Group("[b]g[/b]"))
        c.print(m.panel.Panel("[/] unbalanced"))

    compare(program, markup=False)


def test_print_markup_false_applies_to_container_strings():
    compare(lambda m, c: c.print(m.panel.Panel("[b]x"), markup=False))


def test_a_cropped_end_keeps_its_newline():
    # core Segment::crop_lines treats an `end` segment such as "abc\n" as one
    # run and crops its newline away, so the next print joins the same line.
    def program(m, c):
        c.print("xy", end="!!\n")
        c.print("next")

    compare(program, width=3)


def test_huge_sizes_raise_instead_of_aborting_the_process():
    # An allocation of 2**40 cells aborts the interpreter ("memory allocation
    # of ... bytes failed"); Rich raises MemoryError / OverflowError.
    for call in [
        "Text('x').pad(2**40)",
        "Text('x').extend_style(2**40)",
        "Text('x').set_length(2**40)",
        "Text('a\\tb').expand_tabs(2**40)",
        "Console(file=io.StringIO(), tab_size=2**40).print('a\\tb')",
        "Console(file=io.StringIO()).print(Syntax('a', 'python', code_width=2**40))",
    ]:
        result = run_isolated(
            f"""
            import io
            from rs_rich.console import Console
            from rs_rich.syntax import Syntax
            from rs_rich.text import Text
            try:
                {call}
            except (MemoryError, OverflowError, ValueError):
                pass
            """
        )
        assert result.returncode == 0, f"{call}: interpreter died with {result.returncode}"


def test_a_huge_panel_height_fails_fast_instead_of_eating_all_memory():
    # Panel(height=2**63) (or 2**40) has no limit: core builds the padding
    # lines one by one until the machine runs out of memory (13.5 GB before it
    # was killed during this audit). Rich raises MemoryError at once. Run under
    # a 2 GB address-space limit so a regression aborts instead of swapping.
    result = run_isolated(
        """
        import io, resource
        resource.setrlimit(resource.RLIMIT_AS, (2 << 30, 2 << 30))
        from rs_rich.console import Console
        from rs_rich.panel import Panel
        try:
            Console(file=io.StringIO(), width=30).print(Panel("x", height=2**40))
        except (MemoryError, OverflowError, ValueError):
            pass
        print("survived")
        """
    )
    assert result.returncode == 0, f"interpreter died with {result.returncode}"


def test_huge_sizes_raise_python_errors_not_panics():
    # "capacity overflow" / "attempt to subtract with overflow" surface as
    # PanicException (a BaseException); Rich raises MemoryError/OverflowError.
    for call in [
        "Text('ab').set_length(2**63)",
        "Text('ab').fit(2**63)",
        "Console(file=io.StringIO(), width=30).print(Align('x', height=2**63, vertical='middle'))",
    ]:
        result = run_isolated(
            f"""
            import io, resource
            resource.setrlimit(resource.RLIMIT_AS, (2 << 30, 2 << 30))
            from rs_rich.align import Align
            from rs_rich.console import Console
            from rs_rich.text import Text
            try:
                {call}
            except Exception:
                pass
            """
        )
        assert result.returncode == 0, f"{call}: {result.stderr.strip().splitlines()[-1:]}"


def test_columns_of_zero_width_raise_not_panic():
    # crates/rich/src/columns.rs:234 divides by `width + width_padding`, which
    # is 0 for Columns(width=0, padding=0): a PanicException (a BaseException
    # that `except Exception` does not catch). Rich raises ZeroDivisionError.
    raised = None
    console = RS.console.Console(file=io.StringIO(), width=10)
    try:
        console.print(RS.columns.Columns(["a"], width=0, padding=0))
    except BaseException as error:  # noqa: BLE001
        raised = error
    assert isinstance(raised, Exception), f"raised {type(raised).__name__}"


def test_markdown_inherits_overflow_from_its_container():
    # crates/rich/src/markdown.rs renders paragraphs with Text::render_lines,
    # which folds by default instead of reading options.overflow/no_wrap: in a
    # table cell (overflow="ellipsis") a long word folds where Rich truncates.
    def program(m, c):
        table = m.table.Table("h")
        table.add_row(m.markdown.Markdown("supercalifragilistic word"))
        c.print(table)
        c.print(m.markdown.Markdown("hello"), overflow="crop", width=3)
        c.print(m.markdown.Markdown("![alt](x)"), justify="right", width=12)

    compare(program, width=12)


def test_markdown_with_many_brackets_is_not_quadratic():
    try:
        run_isolated(
            """
            import io
            from rs_rich.console import Console
            from rs_rich.markdown import Markdown
            Console(file=io.StringIO()).print(Markdown("[" * 20000 + "x" + "]" * 20000))
            """,
            timeout=8,
        )
    except subprocess.TimeoutExpired:
        pytest.fail("Markdown with 20000 brackets took over 8 s (Rich: 0.6 s)")


def test_stylize_is_not_linear_in_the_text_length():
    # rich-py/src/text/ops.rs `stylize` recomputes every char boundary on each
    # call, so n stylize calls on an n-char text are quadratic.
    try:
        run_isolated(
            """
            from rs_rich.text import Text
            text = Text("ab" * 20000)
            for i in range(20000):
                text.stylize("bold", 2 * i, 2 * i + 1)
            """,
            timeout=10,
        )
    except subprocess.TimeoutExpired:
        pytest.fail("20000 stylize calls took over 10 s (Rich: 0.02 s)")


# ---------------------------------------------------------------------------
# Low


def test_markup_error_position_counts_characters_of_the_original_string():
    # crates/rich/src/markup.rs reports a byte offset, and the console path
    # replaces emoji before parsing: Rich says position 2 / 8.
    def program(m, c):
        for source in ["é中[/i]", ":smile: [/i]"]:
            try:
                c.print(source)
            except m.markup.MarkupError as error:
                c.file.write(str(error) + "\n")

    compare(program)


def test_grey_is_not_a_colour_name():
    # crates/rich/src/color_names.rs:245 accepts "grey"/"gray"; Rich does not.
    def program(m, c):
        c.print("[grey]x[/] [on gray]y")
        m.color.Color.parse("grey")

    compare(program, color=True)


def test_windows_color_system_uses_the_windows_palette():
    # crates/rich/src/color.rs `downgrade` maps ColorSystem::Windows onto the
    # standard palette; Rich uses WINDOWS_PALETTE (#808080 -> 90, not 37).
    def program(m, c):
        windows = m.console.Console(file=c.file, force_terminal=True, color_system="windows")
        windows.print("x", style="#808080 on #82c9b0")

    compare(program)


def test_spinner_text_expands_emoji_codes():
    # rich-py/src/renderables/spinner.rs converts str text with core's
    # Text::from_markup, which skips emoji replacement.
    compare(lambda m, c: c.print(m.spinner.Spinner("arc", ":smile: [b]x[/b]").render(0)))


def test_text_end_is_kept_inside_a_group():
    # Group(Text("ab", end="")) followed by "cd" prints "abcd" in Rich.
    def program(m, c):
        c.print(m.console.Group(m.text.Text("ab", end=""), "cd"))
        c.print(m.console.Group(m.text.Text("ab", end="!\n"), "cd"))

    compare(program)


def test_empty_renderables_still_print_their_blank_line():
    def program(m, c):
        c.print(m.tree.Tree(""))
        c.print(m.syntax.Syntax("", "python", theme="ansi_dark"))
        c.print(m.console.Group(m.text.Text("", justify="left", overflow="ignore"), "x"))

    compare(program, width=10)


def test_bad_markup_in_table_and_panel_titles_raises():
    # Table headers, cells, title and caption and Panel title/subtitle render
    # bad markup literally; Rich raises MarkupError (as rs_rich does for a
    # Panel body or a Tree label).
    for build in [
        lambda m: m.table.Table("[/]"),
        lambda m: m.table.Table("a", title="[/]"),
        lambda m: m.table.Table("a", caption="[/x]"),
        lambda m: m.panel.Panel("x", title="[/]"),
        lambda m: m.panel.Panel("x", subtitle="[/]"),
    ]:
        compare(lambda m, c: c.print(build(m)))


def test_unicode_line_separators_split_lines_when_measuring():
    # Rich measures a Text with str.splitlines(), which also splits on U+2028.
    def program(m, c):
        c.print(m.panel.Panel("a o", expand=False))

    compare(program, width=10)


def test_explicit_default_justify_is_not_overridden_by_the_column():
    def program(m, c):
        table = m.table.Table()
        table.add_column("h", justify="right", width=6)
        table.add_row(m.text.Text("x", justify="default"))
        c.print(table)

    compare(program, width=20)


def test_zero_width_panel_still_draws_its_corners():
    compare(lambda m, c: c.print(m.panel.Panel("", width=0)), width=5)


def test_crop_drops_zero_width_characters_past_the_edge():
    # core Segment::crop_lines keeps zero-width runs (U+200B, combining marks)
    # after the crop column, where they attach to the border.
    def program(m, c):
        table = m.table.Table(show_header=False, box=None, padding=0, width=19)
        table.add_column()
        table.add_column()
        table.add_row("", "​[dim]r[/dim]")
        c.print(table)

    compare(program, width=10)


def test_gfm_table_needs_a_valid_delimiter_row():
    # "-|:" is not a delimiter row (":" alone has no dash): markdown-it prints
    # a paragraph; rs_rich's pulldown-cmark draws a table.
    compare(lambda m, c: c.print(m.markdown.Markdown("a||\n-|:")), width=10)


def test_table_and_panel_attributes_are_settable():
    # Rich's Table and Panel expose their options as attributes
    # (table.show_footer = True, panel.title = ...); rs_rich has none.
    def program(m, c):
        table = m.table.Table("a")
        table.show_header = False
        table.add_row("x")
        panel = m.panel.Panel("body")
        panel.title = "t"
        c.print(table, panel)

    compare(program)
