"""rs_rich.panel.Panel."""

from __future__ import annotations

import gc
import subprocess
import sys
import weakref

import pytest

from conftest import in_thread, render
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


@pytest.mark.parametrize("padding", [2**16 + 1, 2**40, (0, 2**16 + 1), (0, 0, 0, 2**70)])
@pytest.mark.parametrize("make", [Panel, Panel.fit])
def test_padding_past_65536_is_a_value_error(make, padding):
    # rich 15.0.0 renders it, slowly; core never finishes, so it is refused.
    with pytest.raises(ValueError, match="padding must be between 0 and 65536"):
        make("x", padding=padding)


def test_the_largest_padding_renders_as_in_rich():
    # rich 15.0.0: the horizontal padding squeezes the content out entirely.
    assert render(Panel("x", padding=(0, 2**16)), width=6) == "╭────╮\n╰────╯\n"


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


def test_a_child_that_is_not_renderable_fails_when_printed():
    # As in Rich: a panel's child must be a str or a renderable.
    from rs_rich.errors import NotRenderableError

    with pytest.raises(NotRenderableError, match="Unable to render"):
        render(Panel([1, 2]))


def test_a_hundred_nested_panels_render_as_in_rich():
    panel = "x"
    for _ in range(100):
        panel = Panel(panel)
    lines = render(panel, width=450).splitlines()
    # rich 15.0.0 renders this too: 100 borders above and below the "x".
    assert len(lines) == 201
    assert lines[100][190:260] == "│ │ │ │ │ x" + " " * 50 + "│ │ │ │ │"


def test_deeper_nesting_is_a_recursion_error():
    # rich 15.0.0 raises RecursionError before 150 levels.
    panel = "x"
    for _ in range(101):
        panel = Panel(panel)
    with pytest.raises(RecursionError, match="at most 100 nested"):
        render(panel)


def test_very_deep_nesting_does_not_crash_the_interpreter():
    # Once a native stack overflow: run it where a crash cannot take pytest down.
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            "import io\n"
            "from rs_rich.console import Console\n"
            "from rs_rich.panel import Panel\n"
            "p = 'x'\n"
            "for _ in range(3000):\n"
            "    p = Panel(p)\n"
            "try:\n"
            "    Console(file=io.StringIO(), width=40).print(p)\n"
            "except RecursionError:\n"
            "    print('RecursionError')\n",
        ],
        capture_output=True,
        text=True,
        timeout=120,
    )
    assert (result.returncode, result.stdout) == (0, "RecursionError\n"), result.stderr


def test_a_width_0_console_prints_nothing():
    # rich 15.0.0 prints nothing at all for a panel with no room.
    assert render(Panel("hi"), width=0) == ""


def test_usable_from_another_thread():
    panel = Panel.fit("x")
    box = {}

    def work():
        box["out"] = render(panel)

    assert in_thread(work) is None
    assert box["out"] == "╭───╮\n│ x │\n╰───╯\n"


def test_a_cycle_through_the_child_is_collected():
    class Holder:
        pass

    holder = Holder()
    holder.panel = Panel(holder)
    alive = weakref.ref(holder)
    del holder
    gc.collect()
    assert alive() is None


def test_attributes_can_be_read_and_set_as_in_rich():
    panel = Panel("body")
    panel.title = "t"
    panel.subtitle = "s"
    panel.expand = False
    assert (panel.title, panel.subtitle, panel.padding, panel.style) == ("t", "s", (0, 1), "none")
    assert render(panel, width=30) == "╭─ t ──╮\n│ body │\n╰─ s ──╯\n"


def test_a_huge_height_is_a_memory_error_not_an_exhausted_machine():
    with pytest.raises(MemoryError):
        render(Panel("x", height=2**40))


def test_bad_markup_in_a_title_raises():
    from rs_rich.errors import MarkupError

    for panel in [Panel("x", title="[/]"), Panel("x", subtitle="[/]")]:
        with pytest.raises(MarkupError):
            render(panel)


def test_markup_off_applies_to_the_body_but_not_the_title():
    # Rich parses a title with `Text.from_markup` whatever `markup` says.
    out = render(Panel("[/] [b]x", title="[b]t[/b]"), width=20, markup=False)
    assert out.splitlines()[:2] == ["╭─────── t ────────╮", "│ [/] [b]x         │"]
