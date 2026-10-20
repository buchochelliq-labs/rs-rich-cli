"""The pictures on docs/python/ext/markdown-export.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.console import Console
from rs_rich.ext.markdown_export import to_gfm
from rs_rich.table import Table


def table_and_markdown(console):
    # --8<-- [start:table]
    table = Table(title="Checks")
    table.add_column("check")
    table.add_column("ms", justify="right")
    table.add_row("[bold]lint[/]", "1200")
    table.add_row("a|b", "35")
    Console(width=40).print(table)
    print(to_gfm(table))
    # --8<-- [end:table]


shots = Shots("ext/markdown-export")
shots.shot("table", table_and_markdown, width=40)
shots.finish()
