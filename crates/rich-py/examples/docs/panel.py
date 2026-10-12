"""The pictures on docs/python/panel.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import box
from rs_rich.panel import Panel
from rs_rich.table import Table
from rs_rich.text import Text


def titles(console):
    # --8<-- [start:titles]
    console.print(Panel("Hello, [bold]World[/]!", title="Greeting", subtitle="rs_rich"))
    # --8<-- [end:titles]


def text_titles(console):
    # --8<-- [start:text-titles]
    console.print(Panel("body", title=Text("Title", style="bold"),
                        subtitle=Text("page 1"), subtitle_align="right"))
    # --8<-- [end:text-titles]


def fit(console):
    # --8<-- [start:fit]
    table = Table("key", "value", box=box.SIMPLE)
    table.add_row("answer", "42")
    console.print(Panel.fit(table, title="inner", box=box.DOUBLE, padding=(0, 2)))
    # --8<-- [end:fit]


shots = Shots("panel")
shots.shot("titles", titles, width=30)
shots.shot("text-titles", text_titles, width=30)
shots.shot("fit", fit, width=40)
shots.finish()
