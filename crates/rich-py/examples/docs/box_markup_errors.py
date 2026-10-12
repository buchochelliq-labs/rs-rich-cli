"""The pictures on docs/python/box-markup-errors.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import box
from rs_rich.columns import Columns
from rs_rich.markup import escape
from rs_rich.table import Table


def boxes(console):
    # --8<-- [start:boxes]
    for style in [box.ASCII, box.MARKDOWN]:
        table = Table("a", "b", box=style)
        table.add_row("1", "2")
        console.print(table)
    print(repr(box.ROUNDED))
    # --8<-- [end:boxes]


def escaped(console):
    # --8<-- [start:escape]
    user_input = "[bold]not a tag[/bold]"
    print(escape(user_input))
    console.print(escape(user_input))
    # --8<-- [end:escape]


def gallery(console):
    # --8<-- [start:gallery]
    tables = []
    for name in ["SQUARE", "ROUNDED", "HEAVY_HEAD", "DOUBLE_EDGE",
                 "SIMPLE", "MINIMAL", "HORIZONTALS", "ASCII2"]:
        table = Table("Crate", "Version", box=getattr(box, name), title=name)
        table.add_row("rs-rich", "0.0.8")
        tables.append(table)
    console.print(Columns(tables))
    # --8<-- [end:gallery]


shots = Shots("box-markup-errors")
shots.shot("boxes", boxes, width=30)
shots.shot("gallery", gallery, width=60)
shots.shot("escape", escaped, width=40)
shots.finish()
