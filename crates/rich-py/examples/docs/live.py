"""The pictures on docs/python/live.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.live import Live
from rs_rich.panel import Panel
from rs_rich.screen import Screen


def final_frame(console):
    # --8<-- [start:live]
    with Live(console=console, auto_refresh=False) as live:
        for step in range(3):
            live.update(Panel(f"step {step}"), refresh=True)
    console.print()
    # --8<-- [end:live]


def full_screen(console):
    # --8<-- [start:screen]
    console.print(Screen("top", "next"))
    console.print()
    # --8<-- [end:screen]


shots = Shots("live")
shots.shot("live", final_frame, width=30, terminal=False)
shots.shot("screen", full_screen, width=12, height=3)
shots.finish()
