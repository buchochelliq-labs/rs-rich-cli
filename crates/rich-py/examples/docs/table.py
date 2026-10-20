"""The pictures on docs/python/table.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import box
from rs_rich.table import Table
from rs_rich.text import Text


def first(console):
    # --8<-- [start:first]
    table = Table("Planet", "Moons", title="Solar system")
    table.add_row("Earth", "1")
    table.add_row("Mars", "2")
    console.print(table)
    # --8<-- [end:first]


def columns(console):
    # --8<-- [start:columns]
    table = Table(box=box.SIMPLE, expand=True)
    table.add_column("Name", ratio=1)
    table.add_column("Score", justify="right", width=6)
    table.add_row("alpha", "97")
    table.add_row("a much longer name that wraps", "3")
    console.print(table)
    # --8<-- [end:columns]


def rows(console):
    # --8<-- [start:rows]
    table = Table("markup", "text", "none", box=None)
    table.add_row("[bold]b[/]", Text("[not markup]"), None)
    print(table.row_count)
    console.print(table)
    # --8<-- [end:rows]


def footers(console):
    # --8<-- [start:footers]
    table = Table(title="Fruit", caption="per crate", caption_justify="right",
                  show_footer=True, row_styles=["", "dim"], width=30)
    table.add_column("Name", "Total")
    table.add_column("Qty", "12", justify="right")
    table.add_row("apple", "3")
    table.add_row("banana", "4", end_section=True)
    table.add_row("cherry", "5", style="bold")
    console.print(table)
    # --8<-- [end:footers]


def leading(console):
    # --8<-- [start:leading]
    table = Table("a", "b", box=box.SIMPLE, leading=1, min_width=20)
    table.add_row("1", "2")
    table.add_row("3", "4", "extra")
    console.print(table)
    # --8<-- [end:leading]


def plain(console):
    # --8<-- [start:plain]
    table = Table(show_footer=True)
    table.add_plain_column("[file]", "[total]")
    table.add_plain_column("bytes", justify="right")
    table.add_plain_rows([("notes[1].txt", 120), ("a:smile:b", 3)])
    table.add_plain_row("[bold]x[/]", None)
    console.print(table)
    # --8<-- [end:plain]


shots = Shots("table")
shots.shot("first", first, width=40)
shots.shot("columns", columns, width=32)
shots.shot("rows", rows, width=40)
shots.shot("footers", footers, width=40)
shots.shot("plain", plain, width=40)
shots.shot("leading", leading, width=40)
shots.finish()
