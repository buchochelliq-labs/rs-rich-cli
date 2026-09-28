"""rs_rich.ext.frame: rendered results as frames (rich_ext::frame).

Rich has no frames; these check the Rust crate's contract from Python: a
frame encodes to the bytes the console writes for the same render, its
cells follow the terminal's columns, and `diff` lists the cells that
changed.
"""

from __future__ import annotations

import io

import pytest

from rs_rich.console import Console
from rs_rich.ext import frame as frame_module
from rs_rich.ext.frame import Frame
from rs_rich.ext.target import RenderTarget
from rs_rich.panel import Panel
from rs_rich.segment import Segment
from rs_rich.style import Style
from rs_rich.table import Table
from rs_rich.text import Text


def console(**kwargs):
    return Console(file=io.StringIO(), width=30, force_terminal=True, color_system="truecolor", **kwargs)


def test_the_module():
    assert frame_module.__all__ == ["Frame", "RenderFrame"]
    assert Frame.__module__ == "rs_rich.ext.frame" and Frame.__name__ == "Frame"
    import rs_rich.ext

    assert rs_rich.ext.RenderFrame is Frame


@pytest.mark.parametrize(
    "renderable",
    [
        lambda: "[bold red]Hello[/] [italic]world[/] 中文",
        lambda: Panel("[green]inside[/]", title="t"),
        lambda: Text("wrapping " * 8, style="blue"),
    ],
)
def test_to_ansi_is_what_the_console_prints(renderable):
    c = console()
    c.print(renderable())
    printed = c.file.getvalue()
    frame = Frame.from_console(c, renderable())
    assert frame.to_ansi(c) == printed
    assert frame.ends_with_newline
    for system in ("256", "standard"):
        other = Console(file=io.StringIO(), width=30, force_terminal=True, color_system=system)
        other.print(renderable())
        assert Frame.from_console(other, renderable()).to_ansi(other) == other.file.getvalue()


def test_no_color_keeps_other_attributes():
    c = console(no_color=True)
    c.print("[bold red]x[/]")
    frame = Frame.from_console(c, "[bold red]x[/]")
    assert frame.to_ansi(c) == c.file.getvalue() == "\x1b[1mx\x1b[0m\n"
    assert frame.encode(no_color=True) == "\x1b[1mx\x1b[0m\n"
    assert frame.encode(None) == "x\n"


def test_plain_size_and_rows():
    frame = Frame.from_console(console(), Panel("hi 中", width=10))
    assert frame.plain() == "╭────────╮\n│ hi 中  │\n╰────────╯\n"
    assert str(frame) == frame.plain()
    assert (frame.height, frame.width, len(frame)) == (3, 10, 3)
    assert frame.row_width(1) == 10
    runs = frame.row(1)
    assert "".join(text for text, _, _ in runs) == "│ hi 中  │"
    assert sum(cells for _, cells, _ in runs) == 10
    with pytest.raises(IndexError):
        frame.row(3)


def test_cells_follow_columns():
    frame = Frame.from_segments([Segment("a中", Style(bold=True)), Segment("b")])
    cells = frame.cells(0)
    assert [(text, width) for text, width, _ in cells] == [("a", 1), ("中", 2), ("", 0), ("b", 1)]
    assert cells[0][2] == Style(bold=True)
    assert cells[3][2] is None
    assert frame.style_count == 2


def test_control_segments_are_dropped():
    frame = Frame.from_segments([Segment("\x1b[2K", None, [(1,)]), Segment("text")])
    assert frame.plain() == "text"
    assert frame.run_count == 1


def test_diff_lists_changed_cells():
    c = console()
    before = Frame.from_console(c, "[bold]hello[/] world\nsecond line")
    after = Frame.from_console(c, "[bold]hello[/] there\nsecond line\nthird")
    assert after.diff(before) == [(0, range(6, 11)), (2, range(0, 5))]
    assert before.diff(before) == []
    restyled = Frame.from_console(c, "hello world\nsecond line")
    assert restyled.diff(before) == [(0, range(0, 5))]
    assert after.encode_span(0, 6, 11) == "there"
    assert restyled.encode_span(0, 0, 5) == "hello"
    assert before.encode_span(0, 0, 5) == "\x1b[1mhello\x1b[0m"


def test_merged_is_smaller_but_shows_the_same():
    c = console()
    table = Table("a", "b")
    table.add_row("1", "2")
    frame = Frame.from_console(c, table)
    merged = frame.merged()
    assert merged.plain() == frame.plain()
    assert merged.run_count <= frame.run_count
    assert len(frame.to_ansi_merged(c)) <= len(frame.to_ansi(c))


def test_render_target_frame():
    target = RenderTarget(width=12, color_system=None)
    frame = target.frame(Text("hello world wide"))
    assert frame.plain() == target.text(Text("hello world wide"))
    assert frame.height == 2
