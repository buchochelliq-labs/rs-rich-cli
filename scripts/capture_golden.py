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

from rich.console import Console

# (name, console-markup) — keep in sync with the Rust test's expectations.
CASES: list[tuple[str, str]] = [
    ("bold_red", "[bold red]hello[/]"),
    ("two_tags", "[bold]Hello[/] [red]World[/]"),
    ("fg_on_bg", "[white on blue]bg[/]"),
    ("nested_inner_wins", "[red]a[blue]x[/]b[/]"),
    ("hex_truecolor", "[#ff8800]x[/]"),
    ("plain_text", "no styles here"),
]

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
    """Render the ESC byte as the literal marker the Rust test unescapes."""
    return text.replace("\x1b", "\\x1b")


def main() -> None:
    # highlight=False so no ReprHighlighter styling leaks in — the Rust core
    # ships no default highlighter.
    console = Console(
        force_terminal=True, color_system="truecolor", width=80, highlight=False
    )
    out_path = (
        pathlib.Path(__file__).resolve().parent.parent
        / "crates"
        / "rich"
        / "tests"
        / "golden"
        / "truecolor.tsv"
    )
    lines = [HEADER.rstrip("\n")]
    for name, markup in CASES:
        with console.capture() as capture:
            console.print(markup, end="")
        lines.append(f"{name}\t{markup}\t{escape(capture.get())}")
    out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {len(CASES)} cases to {out_path}")


if __name__ == "__main__":
    main()
