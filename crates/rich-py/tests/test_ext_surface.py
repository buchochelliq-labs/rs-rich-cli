"""rs_rich.ext.surface: positioned drawing (rich_ext::surface).

Rich has no surface; these check the Rust crate's contract from Python: a
draw writes only the cells that changed, the surface enters and leaves the
alternate screen (or stays inline), and what it shows is readable back.
"""

from __future__ import annotations

import io

import pytest

from rs_rich.ext import surface as surface_module
from rs_rich.ext.surface import Rect, Surface
from rs_rich.panel import Panel


def take(buf: io.StringIO) -> str:
    out = buf.getvalue()
    buf.seek(0)
    buf.truncate()
    return out


def test_module_exports() -> None:
    assert set(surface_module.__all__) == {"Drawing", "Rect", "Surface"}


def test_a_second_draw_writes_only_the_changed_cell() -> None:
    buf = io.StringIO()
    surface = Surface(20, 4, file=buf, color_system=None)
    surface.draw(lambda d: d.render(Rect(0, 3, 20, 1), "tick 1"))
    first = take(buf)
    assert first.startswith("\x1b[?1049h") and "tick 1" in first
    surface.draw(lambda d: d.render(Rect(0, 3, 20, 1), "tick 2"))
    assert take(buf) == "\x1b[4;6H\x1b[0m2"
    surface.draw(lambda d: None)
    assert take(buf) == ""
    surface.finish()
    assert take(buf).endswith("\x1b[?1049l")


def test_renderables_fill_their_rectangle() -> None:
    surface = Surface(12, 3, file=io.StringIO())
    surface.draw(lambda d: d.render((0, 0, 12, 3), Panel("hi")))
    assert surface.plain() == ["╭──────────╮", "│ hi       │", "╰──────────╯"]


def test_inline_surface_keeps_its_last_frame() -> None:
    buf = io.StringIO()
    with Surface(10, 24, inline=1, file=buf) as surface:
        assert tuple(surface.area) == (0, 0, 10, 1)
        surface.draw(lambda d: d.render(d.area, "x"))
    out = buf.getvalue()
    assert "\x1b[?1049h" not in out
    assert out.endswith("\r\n\x1b[0m\x1b[?25h")


def test_resize_keeps_what_was_drawn() -> None:
    surface = Surface(10, 2, file=io.StringIO())
    surface.draw(lambda d: d.render((0, 0, 10, 1), "kept"))
    surface.resize(6, 2)
    assert surface.plain()[0] == "kept  "


def test_a_finished_surface_refuses_to_draw() -> None:
    surface = Surface(4, 1, file=io.StringIO())
    surface.finish()
    with pytest.raises(ValueError, match="finished"):
        surface.draw(lambda d: None)
