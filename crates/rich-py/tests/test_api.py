"""The rs_rich API: the import-only migration, errors and unsupported input."""

from __future__ import annotations

import io
import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

import rs_rich
from rs_rich.console import Console
from rs_rich.errors import MarkupError, StyleSyntaxError
from rs_rich.panel import Panel
from rs_rich.style import Style
from rs_rich.table import Table
from rs_rich.text import Text

EXAMPLE = Path(__file__).resolve().parent.parent / "examples" / "star_wars.py"


def run(source: str) -> bytes:
    """Run a program with its output on a pipe, 70 columns wide."""
    env = dict(os.environ, COLUMNS="70")
    env.pop("NO_COLOR", None)
    return subprocess.run(
        [sys.executable, "-"], input=source.encode(), env=env, capture_output=True, check=True
    ).stdout


def test_a_rich_example_runs_with_only_its_imports_changed():
    source = EXAMPLE.read_text(encoding="utf-8")
    ported = re.sub(r"^from rich\b", "from rs_rich", source, flags=re.MULTILINE)
    changed = [
        (old, new)
        for old, new in zip(source.splitlines(), ported.splitlines())
        if old != new
    ]
    assert changed and all(old.startswith("from rich.") for old, _ in changed)
    output = run(source)
    assert b"Star Wars Movies" in output
    assert run(ported) == output


def test_print_writes_to_stdout_and_the_global_console():
    out = io.StringIO()
    console = Console(file=out, width=20)
    console.print("[bold]hi[/]", 1, None)
    assert out.getvalue() == "hi 1 None\n"
    assert console.file is out
    assert rs_rich.get_console() is rs_rich.get_console()


def test_console_properties():
    console = Console(file=io.StringIO(), width=33, force_terminal=True, color_system="256")
    assert (console.width, console.is_terminal, console.color_system) == (33, True, "256")
    plain = Console(file=io.StringIO())
    assert plain.is_terminal is False and plain.color_system is None
    with pytest.raises(ValueError, match="color system"):
        Console(color_system="16")


def test_errors():
    with pytest.raises(MarkupError):
        Text.from_markup("[/bold]")
    with pytest.raises(MarkupError):
        Console(file=io.StringIO()).print("[/nope]")
    with pytest.raises(StyleSyntaxError):
        Style.parse("bold not-a-colour")
    with pytest.raises(RuntimeError, match="record=True"):
        Console(file=io.StringIO()).export_text()
    with pytest.raises(ValueError, match="too many values"):
        table = Table("one")
        table.add_row("a", "b")
    with pytest.raises(TypeError):
        Panel("x", box="rounded")


def test_what_is_not_implemented_yet_is_refused_not_rendered_differently():
    console = Console(file=io.StringIO())
    # Rich pretty-prints containers; that comes with rs_rich.pretty.
    with pytest.raises(NotImplementedError, match="pretty printing"):
        console.print({"a": 1})
    with pytest.raises(NotImplementedError, match="tab_size=8"):
        Console(tab_size=4)


def test_renderables_nest_anywhere():
    table = Table("a")
    table.add_row(Panel("nested"))
    out = io.StringIO()
    Console(file=out, width=20).print(table, Panel("x"), justify="center")
    assert "nested" in out.getvalue()


def test_text_uses_python_character_offsets():
    text = Text("日本語 text")
    text.stylize("bold", 0, 2)
    text.stylize("red", -4)
    assert len(text) == 8 and text.plain == "日本語 text" and str(text) == "日本語 text"
    out = io.StringIO()
    Console(file=out, force_terminal=True, color_system="truecolor").print(text)
    assert out.getvalue() == "\x1b[1m日本\x1b[0m語 \x1b[31mtext\x1b[0m\n"


def test_style_repr_and_equality():
    assert str(Style(bold=True, color="red")) == "bold red"
    assert Style(bold=True) + Style(italic=True) == Style.parse("bold italic")
    assert repr(Style()) == 'Style.parse("none")'
    assert rs_rich.__version__ == "0.0.1"
