"""The pictures on docs/python/protocol.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.measure import Measurement
from rs_rich.panel import Panel
from rs_rich.segment import Segment
from rs_rich.style import Style
from rs_rich.table import Table
from rs_rich.text import Text


def rich(console):
    # --8<-- [start:rich]
    class User:
        def __init__(self, name):
            self.name = name

        def __rich__(self):
            return f"[bold]{self.name}[/] (user)"

    console.print(User("ada"))
    console.print(Panel(User("grace"), title="who"))
    # --8<-- [end:rich]


def console(console):
    # --8<-- [start:console]
    class Report:
        def __rich_console__(self, console, options):
            yield "[bold]Report[/]"
            yield Text(f"{options.max_width} columns available")
            yield Segment("raw segment", Style(italic=True))
            yield Segment.line()
            table = Table("key", "value")
            table.add_row("status", "ok")
            yield table

    console.print(Report())
    # --8<-- [end:console]


def options(console):
    # --8<-- [start:options]
    class Width:
        def __rich_console__(self, console, options):
            yield f"width {options.max_width}"

    console.print(Width())
    console.print(Panel(Width()))
    table = Table("a", "b", box=None)
    table.add_row(Width(), "x")
    console.print(table)
    # --8<-- [end:options]


def render_lines(console):
    # --8<-- [start:render-lines]
    class Framed:
        def __init__(self, child):
            self.child = child

        def __rich_console__(self, console, options):
            width = options.max_width - 2
            for line in console.render_lines(self.child, options.update_width(width)):
                yield Segment("|")
                yield from line
                yield Segment("|")
                yield Segment.line()

    console.print(Framed("some words to wrap"))
    # --8<-- [end:render-lines]


def measure(console):
    # --8<-- [start:measure]
    class Badge:
        def __rich_console__(self, console, options):
            yield "[reverse] OK [/]"

        def __rich_measure__(self, console, options):
            return Measurement(4, 4)

    class Greedy:
        def __rich_console__(self, console, options):
            yield "greedy"

    console.print(Panel.fit(Badge()))
    console.print(Panel.fit(Greedy()))
    # --8<-- [end:measure]


shots = Shots("protocol")
shots.shot("rich", rich, width=30)
shots.shot("console", console, width=24)
shots.shot("options", options, width=30)
shots.shot("render-lines", render_lines, width=12)
shots.shot("measure", measure, width=20)
shots.finish()
