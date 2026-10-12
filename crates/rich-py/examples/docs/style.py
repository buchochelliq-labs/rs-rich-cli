"""The pictures on docs/python/style.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.text import Text


def definitions(console):
    # --8<-- [start:definitions]
    for definition in ["bold", "dim", "italic", "underline", "strike", "reverse",
                       "bold red", "black on bright_yellow", "#ff8800 on #102030",
                       "bold not dim cyan on bright_black"]:
        console.print(Text(definition, style=definition))
    # --8<-- [end:definitions]


shots = Shots("style")
shots.shot("definitions", definitions, width=40)
shots.finish()
