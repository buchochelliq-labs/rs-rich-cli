"""The pictures on docs/python/layout.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

import io

from rs_rich.columns import Columns
from rs_rich.console import Console, Group, group
from rs_rich.layout import Layout
from rs_rich.panel import Panel


def in_columns(console):
    # --8<-- [start:columns]
    words = "alpha beta gamma delta epsilon zeta eta theta".split()
    console.print(Columns(words))
    console.print(Columns(words, equal=True, column_first=True))
    # --8<-- [end:columns]


def groups(console):
    # --8<-- [start:group]
    console.print(Panel.fit(Group("first", Panel("second"))))


    @group()
    def lines():
        yield "one"
        yield "two"


    console.print(Panel.fit(lines()))
    # --8<-- [end:group]


def regions(console):
    # --8<-- [start:layout]
    layout = Layout()
    layout.split_column(Layout(name="header", size=3), Layout(name="body"))
    layout["body"].split_row(Layout(name="left"), Layout(name="right", ratio=2))
    layout["header"].update(Panel("header"))
    layout["left"].update("left side")
    console.print(layout)
    # --8<-- [end:layout]
    return layout


def structure(console):
    layout = regions(Console(file=io.StringIO(), width=40, height=10))
    # --8<-- [start:tree]
    console.print(layout.tree)
    # --8<-- [end:tree]


shots = Shots("layout")
shots.shot("columns", in_columns, width=30)
shots.shot("group", groups, width=30)
shots.shot("layout", regions, width=40, height=10)
shots.shot("tree", structure, width=40)
shots.finish()
