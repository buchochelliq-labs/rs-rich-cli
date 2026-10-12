"""The pictures on docs/python/ext/layout.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import layout
from rs_rich.panel import Panel
from rs_rich.text import Text


def bounded(console):
    # --8<-- [start:bounded]
    header = layout.LayoutNode(Text("acme deploy"), height=1)
    sidebar = layout.LayoutNode(Text("api\nworker\nweb"), width=layout.Constraint(min=6, max=12), content_width=True)
    body = layout.LayoutNode(Panel("v2.4.1 on 3 of 4 services", title="status"))
    footer = layout.LayoutNode(Text("q quit"), height=1, align=("end", "start"))
    screen = layout.LayoutNode.split("vertical", [header, layout.LayoutNode.split("horizontal", [sidebar, body]), footer])
    screen.validate()
    console.print(screen)
    # --8<-- [end:bounded]


def overflow(console):
    # --8<-- [start:overflow]
    code = Text("let answer = compute_the_answer(universe, everything, 42);")
    console.print(layout.Overflowing(code, "ellipsis"))
    # --8<-- [end:overflow]


shots = Shots("ext/layout")
shots.shot("bounded", bounded, width=40, height=7)
shots.shot("overflow", overflow, width=30)
shots.finish()
