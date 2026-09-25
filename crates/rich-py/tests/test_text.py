"""rs_rich.text.Text."""

from __future__ import annotations

import pytest

from conftest import render
from rs_rich.panel import Panel
from rs_rich.errors import MarkupError
from rs_rich.style import Style
from rs_rich.text import Text


def test_plain_len_str_and_repr():
    text = Text("héllo")
    assert (text.plain, len(text), str(text), repr(text)) == ("héllo", 5, "héllo", "<text 'héllo' [] ''>")


def test_a_style_applies_to_the_whole_text():
    assert render(Text("x", style="bold"), color=True) == "\x1b[1mx\x1b[0m\n"
    assert render(Text("x", Style(italic=True)), color=True) == "\x1b[3mx\x1b[0m\n"


def test_justify_applies_inside_a_container():
    # Printed on its own, a Text takes the console's justification, as in Rich.
    assert render(Text("hi", justify="right"), width=6) == "hi\n"
    assert render(Panel(Text("hi", justify="right"), width=8)) == "╭──────╮\n│   hi │\n╰──────╯\n"


@pytest.mark.parametrize(
    "options, panel",
    [
        ({"overflow": "ellipsis", "no_wrap": True}, "│ abc… │"),
        ({"overflow": "crop", "no_wrap": True}, "│ abcd │"),
        ({"overflow": "fold"}, "│ abcd │\n│ efgh │\n│ ij   │"),
    ],
)
def test_overflow_and_no_wrap(options, panel):
    text = Text("abcdefghij", **options)
    # On its own a Text folds at the console width, as in Rich.
    assert render(text, width=8) == "abcdefgh\nij\n"
    body = "\n".join(render(Panel(text), width=8).splitlines()[1:-1])
    assert body == panel


@pytest.mark.parametrize("kwargs", [{"justify": "middle"}, {"overflow": "wrap"}])
def test_invalid_options_are_value_errors(kwargs):
    with pytest.raises(ValueError):
        Text("x", **kwargs)


def test_a_bad_style_type_is_a_type_error():
    with pytest.raises(TypeError, match="style must be a str or a Style"):
        Text("x", style=42)


class TestFromMarkup:
    def test_parses_markup(self):
        text = Text.from_markup("[bold]b[/] plain")
        assert text.plain == "b plain"
        assert render(text, color=True) == "\x1b[1mb\x1b[0m plain\n"

    def test_base_style_and_justify(self):
        text = Text.from_markup("[bold]b[/]", style="red", justify="center")
        assert render(Panel(text, width=7), color=True).splitlines()[1] == (
            "│ \x1b[31m \x1b[0m\x1b[1;31mb\x1b[0m\x1b[31m \x1b[0m │"
        )

    def test_bad_markup(self):
        with pytest.raises(MarkupError):
            Text.from_markup("[/i]")


class TestAppend:
    def test_appends_strings_with_a_style_and_returns_the_text(self):
        text = Text("a")
        assert text.append("b", style="bold") is text
        assert text.append("c").plain == "abc"
        assert render(text, color=True) == "a\x1b[1mb\x1b[0mc\n"

    def test_appends_a_text_keeping_its_spans(self):
        text = Text("a").append(Text("b", style="italic"))
        assert render(text, color=True) == "a\x1b[3mb\x1b[0m\n"

    def test_a_text_takes_no_style_argument(self):
        with pytest.raises(ValueError, match="style must not be set"):
            Text("a").append(Text("b"), style="bold")

    def test_a_text_can_append_itself(self):
        # rich 15.0.0: "abab".
        text = Text("ab")
        assert text.append(text) is text
        assert (text.plain, len(text)) == ("abab", 4)

    def test_only_str_or_text(self):
        with pytest.raises(TypeError, match="Only str or Text"):
            Text("a").append(1)


class TestStylize:
    def test_character_offsets(self):
        text = Text("日本語 text")
        text.stylize("bold", 0, 2)
        assert render(text, color=True) == "\x1b[1m日本\x1b[0m語 text\n"

    def test_negative_and_open_ended_ranges(self):
        text = Text("abcdef")
        text.stylize("red", -2)
        assert render(text, color=True) == "abcd\x1b[31mef\x1b[0m\n"
        text = Text("abcdef")
        text.stylize("red", 1, -1)
        assert render(text, color=True) == "a\x1b[31mbcde\x1b[0mf\n"

    def test_out_of_range_offsets_are_clamped(self):
        text = Text("ab")
        text.stylize("red", -10, 10)
        assert render(text, color=True) == "\x1b[31mab\x1b[0m\n"

    def test_a_style_object(self):
        text = Text("ab")
        text.stylize(Style(underline=True), 1)
        assert render(text, color=True) == "a\x1b[4mb\x1b[0m\n"

    @pytest.mark.parametrize(
        "start, end, expected",
        [
            # rich 15.0.0 takes any int and clamps what is past the text.
            (0, 2**70, "\x1b[1mabc\x1b[0m\n"),
            (-(2**70), 2**70, "\x1b[1mabc\x1b[0m\n"),
            (1, -(2**70), "abc\n"),
            (2**70, None, "abc\n"),
        ],
    )
    def test_offsets_beyond_a_machine_integer_are_clamped(self, start, end, expected):
        text = Text("abc")
        text.stylize("bold", start, end)
        assert render(text, color=True) == expected

    def test_offsets_must_be_integers(self):
        with pytest.raises(TypeError):
            Text("abc").stylize("bold", "1")  # type: ignore[arg-type]
