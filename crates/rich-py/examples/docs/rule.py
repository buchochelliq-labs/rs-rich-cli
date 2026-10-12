"""The pictures on docs/python/rule.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.align import Align
from rs_rich.bar import Bar
from rs_rich.constrain import Constrain
from rs_rich.padding import Padding
from rs_rich.panel import Panel
from rs_rich.rule import Rule


def rules(console):
    # --8<-- [start:rule]
    console.print(Rule())
    console.print(Rule("[b]Section[/]"))
    console.print(Rule("left", align="left", characters="="))
    # --8<-- [end:rule]


def padded(console):
    # --8<-- [start:padding]
    console.print(Padding("padded", (1, 4), style="on blue"))
    console.print(Padding.indent("indented", 4))
    # --8<-- [end:padding]


def aligned(console):
    # --8<-- [start:align]
    console.print(Align.center(Panel.fit("centred")))
    console.print(Align.right("[i]right[/]", vertical="bottom", height=2))
    # --8<-- [end:align]


def constrained(console):
    # --8<-- [start:constrain]
    console.print(Constrain(Panel("at most 16 cells wide"), 16))
    # --8<-- [end:constrain]


def solid_bar(console):
    # --8<-- [start:bar]
    console.print(Bar(100, 25, 70))
    # --8<-- [end:bar]


shots = Shots("rule")
shots.shot("rule", rules, width=30)
shots.shot("padding", padded, width=20)
shots.shot("align", aligned, width=24)
shots.shot("constrain", constrained, width=40)
shots.shot("bar", solid_bar, width=20)
shots.finish()
