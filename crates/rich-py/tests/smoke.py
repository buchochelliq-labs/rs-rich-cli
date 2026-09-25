"""Install the wheel from DIST into this interpreter and render with it.

    python crates/rich-py/tests/smoke.py dist

Used by CI on every wheel target, with the oldest and newest supported Python.
It needs no Rich and no test framework.
"""

import io
import subprocess
import sys
from pathlib import Path


def main() -> None:
    dist = Path(sys.argv[1])
    wheels = sorted(dist.glob("rs_rich-*.whl"))
    if not wheels:
        sys.exit(f"no rs_rich wheel in {dist}")
    subprocess.run(
        [sys.executable, "-m", "pip", "install", "--force-reinstall", "--no-deps", str(wheels[-1])],
        check=True,
    )
    from rs_rich.console import Console
    from rs_rich.panel import Panel
    from rs_rich.table import Table

    out = io.StringIO()
    console = Console(file=out, width=30, force_terminal=True, color_system="truecolor")
    table = Table("name", "value")
    table.add_row("[bold]pi[/]", "3.14")
    console.print(Panel(table, title="smoke"))
    rendered = out.getvalue()
    assert "╭" in rendered and "pi" in rendered and "\x1b[1m" in rendered, rendered
    print(f"rs_rich {__import__('rs_rich').__version__} on Python {sys.version.split()[0]}: ok")


if __name__ == "__main__":
    main()
