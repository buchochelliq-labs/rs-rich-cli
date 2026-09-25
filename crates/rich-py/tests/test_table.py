"""rs_rich.table.Table."""

from __future__ import annotations

import pytest

from conftest import render
from rs_rich import box
from rs_rich.panel import Panel
from rs_rich.table import Table
from rs_rich.text import Text


def small(**options) -> Table:
    table = Table("a", "b", **options)
    table.add_row("1", "2")
    return table


def test_a_default_table():
    assert render(small()) == (
        "┏━━━┳━━━┓\n"
        "┃ a ┃ b ┃\n"
        "┡━━━╇━━━┩\n"
        "│ 1 │ 2 │\n"
        "└───┴───┘\n"
    )


def test_box_none_and_other_boxes():
    assert render(small(box=None)) == " a  b \n 1  2 \n"
    assert render(small(box=box.ASCII)).splitlines()[0] == "+-------+"


def test_a_box_must_be_a_box_constant():
    with pytest.raises(TypeError, match="rs_rich.box"):
        Table(box="rounded")


def test_title_caption_and_no_header():
    # Without a header, the default HEAVY_HEAD box draws as SQUARE, as in Rich.
    table = small(title="T", caption="C", show_header=False)
    assert render(table) == "    T    \n┌───┬───┐\n│ 1 │ 2 │\n└───┴───┘\n    C    \n"


def test_show_lines_and_edge():
    table = Table("a", show_lines=True, show_edge=False)
    table.add_row("1")
    table.add_row("2")
    assert render(table) == " a \n━━━\n 1 \n───\n 2 \n"


def test_column_options():
    table = Table(box=None, show_header=False)
    table.add_column(justify="right", width=4)
    table.add_column(no_wrap=True, max_width=3, overflow="ellipsis")
    table.add_row("7", "overflow")
    assert render(table) == "    7  ov… \n"


def test_column_styles():
    table = Table(box=None)
    table.add_column("h", style="red", header_style="blue")
    table.add_row("x")
    # `header_style` styles the whole header cell, padding included.
    assert render(table, color=True) == (
        "\x1b[1;34m \x1b[0m\x1b[1;34mh\x1b[0m\x1b[1;34m \x1b[0m\n"
        "\x1b[31m \x1b[0m\x1b[31mx\x1b[0m\x1b[31m \x1b[0m\n"
    )


@pytest.mark.parametrize("kwargs", [{"justify": "middle"}, {"overflow": "wrap"}])
def test_invalid_column_options(kwargs):
    with pytest.raises(ValueError):
        Table().add_column("x", **kwargs)


def test_cells_are_markup_text_or_empty():
    table = Table("a", "b", "c", box=None)
    table.add_row("[bold]m[/]", Text("[t]"), None)
    assert render(table, color=True).splitlines()[1] == " \x1b[1mm\x1b[0m  [t]    "
    assert table.row_count == 1


def test_too_many_cells():
    with pytest.raises(ValueError, match="too many values"):
        Table("a").add_row("1", "2")


def test_renderable_cells_are_not_supported_yet():
    with pytest.raises(NotImplementedError, match="str or Text"):
        Table("a").add_row(Panel("x"))


def test_rows_added_after_use_are_printed():
    table = Table("a", box=None)
    panel = Panel.fit(table)
    table.add_row("late")
    assert "late" in render(panel)
