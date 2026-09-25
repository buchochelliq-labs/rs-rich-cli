"""Byte-for-byte compatibility with Python ``rich`` 15.0.0.

Each program is written once against Rich's API and run twice: with ``rich``'s
modules and with ``rs_rich``'s. The two outputs must be identical, in colour
and without.
"""

from __future__ import annotations

import importlib
import importlib.metadata
import io
import re
from types import SimpleNamespace

import pytest

RICH_VERSION = "15.0.0"


def modules(package: str) -> SimpleNamespace:
    load = lambda name: importlib.import_module(f"{package}.{name}")  # noqa: E731
    return SimpleNamespace(
        console=load("console"),
        text=load("text"),
        style=load("style"),
        table=load("table"),
        panel=load("panel"),
        box=load("box"),
        markup=load("markup"),
    )


def console(m: SimpleNamespace, color: bool, **options):
    return m.console.Console(
        file=io.StringIO(),
        width=options.pop("width", 60),
        force_terminal=color,
        color_system="truecolor" if color else None,
        **options,
    )


# Every program takes the modules and a console, and prints to it.


def markup_and_highlighting(m, c):
    c.print("[bold red]Error:[/] disk [italic]full[/] at 99% on /dev/sda1")
    c.print("numbers 123 4.5 and True, None, 'strings' and https://example.com")
    c.print("several", "strings", "joined", 42)
    c.print()
    c.print(m.markup.escape("[not markup] stays"))


def text_objects(m, c):
    text = m.text.Text("Hello, World!")
    text.stylize("bold magenta", 0, 6)
    text.stylize(m.style.Style(color="green", underline=True), -6, -1)
    text.append(" appended", style="on blue")
    text.append(m.text.Text.from_markup(" and [i]markup[/i]"))
    c.print(text)
    c.print(m.text.Text("centred", justify="center"))
    c.print(m.text.Text("x" * 90, overflow="ellipsis", no_wrap=True))
    c.print(m.text.Text("wrapped words " * 8, style="yellow"))


def styles(m, c):
    for style in [
        m.style.Style(bold=True, color="red"),
        m.style.Style(italic=False, bgcolor="#102030", underline=True),
        m.style.Style.parse("bold not dim cyan on bright_black"),
        m.style.Style(color="color(208)") + m.style.Style(strike=True),
    ]:
        text = m.text.Text("styled", style=style)
        c.print(text)


def star_wars_table(m, c):
    # The README example of Rich itself.
    table = m.table.Table(title="Star Wars Movies")
    table.add_column("Released", justify="right", style="cyan", no_wrap=True)
    table.add_column("Title", style="magenta")
    table.add_column("Box Office", justify="right", style="green")
    table.add_row("Dec 20, 2019", "Star Wars: The Rise of Skywalker", "$952,110,690")
    table.add_row("May 25, 2018", "Solo: A Star Wars Story", "$393,151,347")
    table.add_row("Dec 15, 2017", "Star Wars Ep. V111: The Last Jedi", "$1,332,539,889")
    table.add_row("Dec 16, 2016", "Rogue One: A Star Wars Story", "$1,332,439,889")
    c.print(table)


def table_options(m, c):
    table = m.table.Table(
        "Name",
        "Score",
        box=m.box.SIMPLE,
        show_lines=True,
        caption="a caption",
        border_style="blue",
    )
    table.add_row("[b]alpha[/]", "1")
    table.add_row(m.text.Text("beta", style="red"), None)
    table.add_row("gamma is a long name that wraps", "3")
    c.print(table)
    grid = m.table.Table(box=None, show_header=False, expand=True)
    grid.add_column(ratio=1)
    grid.add_column(ratio=2, justify="center", header_style="bold")
    grid.add_column(width=6, overflow="fold")
    grid.add_row("left", "middle", "overflowing")
    c.print(grid)
    boxed = m.table.Table(box=m.box.ROUNDED, show_edge=False, title="[i]no edge[/]")
    boxed.add_column("a", min_width=8)
    boxed.add_column("b", max_width=5)
    boxed.add_row("1", "long value here")
    c.print(boxed)


def panels(m, c):
    c.print(m.panel.Panel("Hello, [bold]World[/]! 42"))
    c.print(
        m.panel.Panel.fit(
            m.text.Text("fits its text", style="italic"),
            title="Title",
            subtitle="[dim]sub[/]",
            subtitle_align="right",
            border_style="green",
        )
    )
    c.print(m.panel.Panel("padded", box=m.box.DOUBLE, padding=(1, 4), width=30))
    table = m.table.Table("k", "v")
    table.add_row("a", "1")
    c.print(m.panel.Panel(m.panel.Panel(table, title="inner"), title="outer", title_align="left"))


def rules(m, c):
    c.rule()
    c.rule("[bold]Section[/]")
    c.rule("Stars", characters="*", style="red")


def justified_prints(m, c):
    c.print("left")
    c.print("centre", justify="center")
    c.print("right [b]bold[/]", justify="right")


PROGRAMS = [
    markup_and_highlighting,
    text_objects,
    styles,
    star_wars_table,
    table_options,
    panels,
    rules,
    justified_prints,
]


def test_the_reference_is_rich_15():
    assert importlib.metadata.version("rich") == RICH_VERSION


@pytest.mark.parametrize("color", [True, False], ids=["truecolor", "plain"])
@pytest.mark.parametrize("program", PROGRAMS, ids=lambda p: p.__name__)
def test_output_matches_rich_byte_for_byte(program, color):
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, color)
        program(m, c)
        outputs.append(c.file.getvalue())
    expected, actual = outputs
    assert actual == expected


@pytest.mark.parametrize("styles", [False, True])
def test_export_text_matches_rich(styles):
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, True, record=True)
        star_wars_table(m, c)
        c.rule("done")
        outputs.append(c.export_text(styles=styles))
        assert c.export_text() == ""  # cleared
    assert outputs[1] == outputs[0]


def test_links_match_except_for_upstreams_random_ids():
    # Upstream tags each OSC 8 link with a random `id=`; the port leaves it
    # out so output is reproducible (docs/DIVERGENCES.md #20).
    outputs = []
    for package in ["rich", "rs_rich"]:
        m = modules(package)
        c = console(m, True)
        c.print(m.text.Text("site", style=m.style.Style(link="https://example.com")))
        c.print("[link=https://example.org]markup link[/link]")
        outputs.append(c.file.getvalue())
    expected = re.sub(r"\x1b\]8;id=[^;]*;", "\x1b]8;;", outputs[0])
    assert outputs[1] == expected
