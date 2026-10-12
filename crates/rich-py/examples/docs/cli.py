"""The pictures on docs/python/cli.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

import subprocess
import sys

from rs_rich.text import Text


def json_colour(console):
    # --8<-- [start:json-run]
    result = subprocess.run(
        [sys.executable, "-m", "rs_rich", "--json", "-", "--width", "30", "--force-terminal"],
        input='{"name": "rich", "values": [1, 2.5, true, null]}',
        capture_output=True,
        text=True,
    )
    # --8<-- [end:json-run]
    # --8<-- [start:json-print]
    console.print(Text.from_ansi(result.stdout), end="")
    # --8<-- [end:json-print]


shots = Shots("cli")
shots.shot("json", json_colour, width=30)
shots.finish()
