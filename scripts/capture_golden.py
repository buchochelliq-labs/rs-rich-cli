#!/usr/bin/env python3
"""Regenerate golden parity fixtures from the real Python `rich` library.

This is the source of truth for the byte-parity tests in
`crates/rich/tests/golden.rs`. When syncing a new upstream release (see the
`sync-upstream` skill), install the matching `rich` version and re-run this to
refresh the expected output:

    pip install "rich==<version-from-UPSTREAM.toml>"
    python scripts/capture_golden.py

Only add cases here whose behavior the Rust core is expected to reproduce
exactly. Features that are *our* additions (e.g. the [error]/[warning]/[info]
theme conveniences) are NOT upstream and must not be captured here.
"""

from __future__ import annotations

import pathlib

from rich import box
from rich.align import Align
from rich.columns import Columns
from rich.console import Console
from rich.constrain import Constrain
from rich.padding import Padding
from rich.panel import Panel
from rich.rule import Rule
from rich.table import Table
from rich.text import Text
from rich.tree import Tree


def _tree() -> Tree:
    tree = Tree("root")
    child_a = tree.add("child A")
    child_a.add("leaf A1")
    child_a.add("leaf A2")
    tree.add("child B")
    return tree


def _table(box_set) -> Table:
    table = Table(box=box_set)
    table.add_column("Name")
    table.add_column("Age")
    table.add_row("Alice", "30")
    table.add_row("Bob", "7")
    return table

# (name, console-markup) — keep in sync with the Rust test's expectations.
CASES: list[tuple[str, str]] = [
    ("bold_red", "[bold red]hello[/]"),
    ("two_tags", "[bold]Hello[/] [red]World[/]"),
    ("fg_on_bg", "[white on blue]bg[/]"),
    ("nested_inner_wins", "[red]a[blue]x[/]b[/]"),
    ("hex_truecolor", "[#ff8800]x[/]"),
    ("plain_text", "no styles here"),
]

# (name, width, renderable) — the Rust test builds a matching renderable per name.
# `legacy_windows`/`safe_box` are pinned OFF for deterministic, platform-neutral
# box glyphs (the port does not yet do platform box substitution).
RENDERABLE_CASES = [
    ("rule_plain", 20, Rule()),
    ("rule_title", 20, Rule("Hi")),
    ("rule_title_odd", 21, Rule("Hi")),
    ("panel_plain", 20, Panel("hello")),
    ("panel_title", 20, Panel("hello", title="T")),
    ("panel_square", 20, Panel("hi", box=box.SQUARE)),
    ("padding_1_2", 10, Padding("hi", (1, 2))),
    ("padding_0_1", 10, Padding("hi", (0, 1))),
    ("wrap_words", 10, Text("The quick brown fox")),
    ("wrap_fold", 6, Text("abcdefghij")),
    ("panel_wrap", 14, Panel("The quick brown fox", box=box.SQUARE)),
    ("table_square", 40, _table(box.SQUARE)),
    ("table_default", 40, _table(box.HEAVY_HEAD)),
    ("tree_nested", 40, _tree()),
    ("align_center", 20, Align.center("hi")),
    ("align_right", 20, Align.right("hi")),
    ("align_center_odd", 21, Align.center("hi")),
    ("constrain_panel", 20, Constrain(Panel("hi", box=box.SQUARE), width=10)),
    ("columns_two_rows", 20, Columns(["one", "two", "three", "four", "five", "six"])),
    ("columns_one_row", 30, Columns(["alpha", "beta", "gamma", "delta"])),
]

RENDERABLE_HEADER = """\
# Golden parity fixtures for RENDERABLES — captured from real Python `rich`.
# Regenerate with: python scripts/capture_golden.py
#
# Format: <name>\\t<width>\\t<expected-ansi>
# `\\x1b` = ESC byte, `\\n` = newline (both unescaped by the test).
# Console: force_terminal=True, color_system="truecolor", highlight=False,
#          safe_box=False, legacy_windows=False.
# The Rust test builds the renderable matching each <name>; keep them in sync.
"""

HEADER = """\
# Golden parity fixtures — captured from the real Python `rich` library.
# Regenerate with: python scripts/capture_golden.py  (see AGENTS.md → Parity)
#
# Format: <name>\\t<console-markup>\\t<expected-ansi>
# `\\x1b` in the expected column is the ESC byte (unescaped by the test).
# Console: force_terminal=True, color_system="truecolor", highlight=False.
#
# Only cases whose output is byte-identical to upstream belong here. Our theme
# conveniences ([error]/[warning]/[info]) are NOT upstream and live in unit tests.
"""


def escape(text: str) -> str:
    """Render ESC and newline as the literal markers the Rust test unescapes."""
    return text.replace("\x1b", "\\x1b").replace("\n", "\\n")


def golden_dir() -> pathlib.Path:
    return (
        pathlib.Path(__file__).resolve().parent.parent
        / "crates"
        / "rich"
        / "tests"
        / "golden"
    )


def main() -> None:
    # highlight=False so no ReprHighlighter styling leaks in — the Rust core
    # ships no default highlighter.
    console = Console(
        force_terminal=True, color_system="truecolor", width=80, highlight=False
    )
    markup_path = golden_dir() / "truecolor.tsv"
    lines = [HEADER.rstrip("\n")]
    for name, markup in CASES:
        with console.capture() as capture:
            console.print(markup, end="")
        lines.append(f"{name}\t{markup}\t{escape(capture.get())}")
    markup_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {len(CASES)} markup cases to {markup_path}")

    renderable_path = golden_dir() / "renderables.tsv"
    rlines = [RENDERABLE_HEADER.rstrip("\n")]
    for name, width, renderable in RENDERABLE_CASES:
        rconsole = Console(
            force_terminal=True,
            color_system="truecolor",
            width=width,
            highlight=False,
            safe_box=False,
            legacy_windows=False,
        )
        with rconsole.capture() as capture:
            rconsole.print(renderable)
        output = capture.get()
        # Guard: if the capture console's encoding isn't UTF-8, rich substitutes
        # box-drawing glyphs with ASCII, producing non-deterministic fixtures.
        # Fail loudly instead of writing a bad fixture.
        if name == "rule_plain" and "─" not in output:
            raise SystemExit(
                "box glyphs were ASCII-substituted — run with UTF-8 mode:\n"
                "    PYTHONUTF8=1 python scripts/capture_golden.py"
            )
        rlines.append(f"{name}\t{width}\t{escape(output)}")
    renderable_path.write_text("\n".join(rlines) + "\n", encoding="utf-8")
    print(f"wrote {len(RENDERABLE_CASES)} renderable cases to {renderable_path}")


if __name__ == "__main__":
    main()
