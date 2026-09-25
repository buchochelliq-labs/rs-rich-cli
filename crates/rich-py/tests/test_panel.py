"""rs_rich.panel.Panel."""

from __future__ import annotations

import pytest

from conftest import render
from rs_rich import box
from rs_rich.panel import Panel
from rs_rich.table import Table
from rs_rich.text import Text


def test_a_panel_fills_the_width():
    assert render(Panel("hi"), width=8) == "╭──────╮\n│ hi   │\n╰──────╯\n"


def test_fit_wraps_the_content():
    assert render(Panel.fit("hi"), width=20) == "╭────╮\n│ hi │\n╰────╯\n"


def test_titles_and_alignment():
    panel = Panel("x", title="T", subtitle="S", subtitle_align="right", width=12)
    assert render(panel) == "╭─── T ────╮\n│ x        │\n╰────── S ─╯\n"


@pytest.mark.parametrize(
    "padding, expected",
    [
        (0, "╭─╮\n│x│\n╰─╯\n"),
        ((0, 2), "╭─────╮\n│  x  │\n╰─────╯\n"),
        ((1, 0, 0, 0), "╭─╮\n│ │\n│x│\n╰─╯\n"),
    ],
)
def test_padding(padding, expected):
    assert render(Panel.fit("x", padding=padding)) == expected


def test_bad_arguments():
    with pytest.raises(ValueError, match="padding"):
        Panel("x", padding=(1, 2, 3))
    with pytest.raises(ValueError, match="needs a box"):
        Panel("x", box=None)
    with pytest.raises(ValueError, match="invalid align"):
        Panel("x", title_align="middle")


def test_boxes_and_border_style():
    assert render(Panel.fit("x", box=box.ASCII)) == "+---+\n| x |\n+---+\n"
    colored = render(Panel.fit("x", border_style="red"), color=True)
    assert colored.startswith("\x1b[31m╭─")


def test_a_string_child_is_markup_without_highlighting():
    assert render(Panel.fit("[bold]b[/] 42"), color=True).splitlines()[1] == (
        "│ \x1b[1mb\x1b[0m 42 │"
    )


def test_children_can_be_text_tables_and_panels():
    table = Table("k", box=None)
    table.add_row("v")
    inner = Panel.fit(Text("t"))
    assert "t" in render(Panel(inner)) and " k " in render(Panel.fit(table))


def test_an_unsupported_child_fails_when_printed():
    with pytest.raises(NotImplementedError, match="cannot render list"):
        render(Panel([1, 2]))
