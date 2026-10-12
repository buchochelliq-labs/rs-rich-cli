"""The pictures on docs/python/ext/terminals.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import fidelity
from rs_rich.panel import Panel
from rs_rich.text import Text


def degraded(console):
    # --8<-- [start:fidelity]
    print(fidelity.select_fidelity(unicode=True, color=False, interactive=True, animation=True))
    panel = Panel(Text.from_markup("[green]✔ ok[/]  [red]✖ failed[/]  → log"), title="CI", width=30)
    console.print(fidelity.Degrade(panel, level="ascii"))
    print(fidelity.ascii_text("“quoted” → ok"))
    # --8<-- [end:fidelity]


shots = Shots("ext/terminals")
shots.shot("fidelity", degraded, width=80)
shots.finish()
