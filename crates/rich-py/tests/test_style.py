"""rs_rich.style.Style."""

from __future__ import annotations

import pytest

from conftest import render
from rs_rich.errors import ConsoleError, StyleSyntaxError
from rs_rich.style import Style
from rs_rich.text import Text


@pytest.mark.parametrize(
    "kwargs, definition",
    [
        ({}, "none"),
        ({"bold": True}, "bold"),
        ({"italic": False}, "not italic"),
        ({"color": "red", "bgcolor": "#102030"}, "red on #102030"),
        ({"underline": True, "strike": True, "color": "blue"}, "underline strike blue"),
        ({"link": "https://example.com"}, "link https://example.com"),
    ],
)
def test_keyword_arguments_build_a_style_definition(kwargs, definition):
    assert str(Style(**kwargs)) == definition


def test_parse_and_equality():
    assert Style.parse("bold red") == Style(bold=True, color="red")
    assert Style.parse("bold") != Style.parse("italic")


def test_adding_combines_and_the_right_side_wins():
    combined = Style(color="red", bold=True) + Style(color="blue")
    assert combined == Style.parse("bold blue")
    assert render(Text("x", style=combined), color=True) == "\x1b[1;34mx\x1b[0m\n"


def test_repr():
    assert repr(Style(dim=True)) == 'Style.parse("dim")'


def test_styles_are_immutable():
    with pytest.raises(AttributeError):
        Style().bold = True  # type: ignore[attr-defined]


@pytest.mark.parametrize("definition", ["bold not-a-colour", "on", "link"])
def test_bad_definitions(definition):
    with pytest.raises(StyleSyntaxError):
        Style.parse(definition)
    assert issubclass(StyleSyntaxError, ConsoleError)


def test_a_bad_colour_keyword_is_a_style_syntax_error():
    with pytest.raises(StyleSyntaxError):
        Style(color="no-such-colour")


def test_styles_are_hashable_and_equal_styles_hash_alike():
    # rich 15.0.0's Style is hashable, so it can key a dict or join a set.
    assert {Style.parse("bold"): 1}[Style(bold=True)] == 1
    assert hash(Style.parse("bold red")) == hash(Style(color="red", bold=True))
    assert len({Style.parse("bold"), Style(bold=True), Style.parse("italic")}) == 2
