"""rs_rich.console.Console."""

from __future__ import annotations

import io

import pytest

from conftest import render
from rs_rich.console import Console
from rs_rich.errors import ConsoleError, MarkupError
from rs_rich.panel import Panel
from rs_rich.style import Style
from rs_rich.text import Text


class TestConstruction:
    def test_a_file_that_is_not_a_terminal_is_80_columns_and_colourless(self):
        console = Console(file=io.StringIO())
        assert console.is_terminal is False
        assert console.color_system is None
        assert console.width == 80

    def test_columns_sets_the_width_of_a_non_terminal_file(self, monkeypatch):
        monkeypatch.setenv("COLUMNS", "33")
        assert Console(file=io.StringIO()).width == 33

    def test_explicit_width_and_height(self):
        console = Console(file=io.StringIO(), width=12, height=7)
        assert (console.width, console.height) == (12, 7)

    @pytest.mark.parametrize(
        "name, expected",
        [("standard", "standard"), ("256", "256"), ("truecolor", "truecolor"), (None, None)],
    )
    def test_colour_systems(self, name, expected):
        console = Console(file=io.StringIO(), force_terminal=True, color_system=name)
        assert console.is_terminal is True
        assert console.color_system == expected

    def test_an_unknown_colour_system_is_a_value_error(self):
        with pytest.raises(ValueError, match="not a valid color system"):
            Console(color_system="16bit")

    def test_file_is_the_given_file_or_stdout(self, capsys):
        out = io.StringIO()
        assert Console(file=out).file is out
        import sys

        assert Console().file is sys.stdout


class TestPrint:
    def test_markup_is_rendered(self):
        assert render("[bold]hi[/] there") == "hi there\n"
        assert render("[bold]hi[/]", color=True) == "\x1b[1mhi\x1b[0m\n"

    def test_strings_are_joined_with_sep(self):
        assert render("a", "b", "c") == "a b c\n"
        assert render("a", "b", sep="-") == "a-b\n"

    def test_numbers_bools_and_none_print_as_their_str(self):
        assert render(1, 2.5, True, None) == "1 2.5 True None\n"

    def test_numbers_are_highlighted_unless_disabled(self):
        assert render("n=42", color=True) == "\x1b[33mn\x1b[0m=\x1b[1;36m42\x1b[0m\n"
        out = io.StringIO()
        Console(file=out, force_terminal=True, color_system="truecolor", highlight=False).print("n=42")
        assert out.getvalue() == "n=42\n"

    def test_emoji_codes(self):
        assert render(":thumbs_up:") == "👍\n"
        out = io.StringIO()
        Console(file=out, emoji=False).print(":thumbs_up:")
        assert out.getvalue() == ":thumbs_up:\n"

    def test_no_color_keeps_other_attributes(self):
        out = io.StringIO()
        console = Console(file=out, force_terminal=True, color_system="truecolor", no_color=True)
        console.print("[bold red]x[/]")
        assert out.getvalue() == "\x1b[1mx\x1b[0m\n"

    def test_an_empty_print_is_a_blank_line(self):
        assert render() == "\n"

    def test_renderables_print_on_their_own_lines(self):
        assert render("before", Text("text"), "after") == "before\ntext\nafter\n"

    def test_long_strings_wrap_at_the_width(self):
        assert render("one two three four", width=9) == "one two \nthree \nfour\n"

    def test_justify_strings(self):
        assert render("mid", width=9, justify="center") == "   mid   \n"
        assert render("end", width=9, justify="right") == "      end\n"

    def test_writes_to_stdout_when_no_file_is_given(self, capsys):
        Console(width=20).print("to stdout")
        assert capsys.readouterr().out == "to stdout\n"

    def test_bad_markup_raises_markup_error(self):
        with pytest.raises(MarkupError):
            render("[/bold]")
        assert issubclass(MarkupError, ConsoleError) and not issubclass(MarkupError, ValueError)

    @pytest.mark.parametrize(
        "args, kwargs, message",
        [
            (({"a": 1},), {}, "cannot render dict"),
            (("x",), {"end": ""}, 'end="\\\\n" only'),
            ((Panel("x"),), {"justify": "center"}, "justifies str objects only"),
        ],
    )
    def test_unsupported_input_raises_instead_of_rendering_differently(self, args, kwargs, message):
        with pytest.raises(NotImplementedError, match=message):
            render(*args, **kwargs)

    def test_an_invalid_justify_is_a_value_error(self):
        with pytest.raises(ValueError, match="invalid justify"):
            render("x", justify="middle")


class TestRule:
    def rule(self, *args, **kwargs):
        out = io.StringIO()
        Console(file=out, width=20).rule(*args, **kwargs)
        return out.getvalue()

    def test_a_plain_rule_spans_the_width(self):
        assert self.rule() == "─" * 20 + "\n"

    def test_a_titled_rule(self):
        assert self.rule("Title") == "────── Title ───────\n"

    def test_characters(self):
        assert self.rule(characters="=") == "=" * 20 + "\n"

    def test_style(self):
        out = io.StringIO()
        Console(file=out, width=4, force_terminal=True, color_system="truecolor").rule(style="red")
        assert out.getvalue() == "\x1b[31m────\x1b[0m\n"
        out = io.StringIO()
        Console(file=out, width=4, force_terminal=True, color_system="truecolor").rule(
            style=Style(color="blue")
        )
        assert out.getvalue() == "\x1b[34m────\x1b[0m\n"


class TestExportText:
    def test_exports_what_was_printed_and_clears(self):
        out = io.StringIO()
        console = Console(file=out, width=20, record=True, force_terminal=True, color_system="truecolor")
        console.print("[bold]one[/]")
        console.rule("two")
        assert console.export_text() == "one\n" + "─────── two ────────\n"
        assert console.export_text() == ""

    def test_styles_and_keeping_the_record(self):
        console = Console(file=io.StringIO(), record=True, force_terminal=True, color_system="truecolor")
        console.print("[bold]x[/]")
        assert console.export_text(clear=False, styles=True) == "\x1b[1mx\x1b[0m\n"
        assert console.export_text() == "x\n"

    def test_requires_record(self):
        with pytest.raises(RuntimeError, match="record=True"):
            Console(file=io.StringIO()).export_text()
