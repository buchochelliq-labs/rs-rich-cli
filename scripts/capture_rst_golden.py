#!/usr/bin/env python3
"""Capture the `--rst` parity fixtures from the real `rich-rst`.

`crates/rich-ext/src/rst` ports rich-rst's `RestructuredText`, which
rich-cli's `--rst` prints. This renders every document under
`crates/rich-ext/tests/fixtures/rst/` the way rich-cli does, on the `rich`
pinned in UPSTREAM.toml, and writes what it printed beside the document:
`<name>.<width>.txt` without colour, and `<name>.<width>.ansi` in truecolor.
`crates/rich-ext/tests/rst.rs` asserts the port prints the same bytes.

Install the pins in a virtualenv of their own (never beside rich-cli, which
would downgrade rich; see AGENTS.md):

    pip install "rich==15.0.0" "rich-rst==1.3.2" "docutils==0.23"
    python scripts/capture_rst_golden.py

Only documents without code blocks are captured in colour: code is
highlighted by Pygments upstream and by syntect here, as `--syntax` is.
OSC 8 link ids are random upstream and absent here, so they are dropped.
"""

from __future__ import annotations

import importlib.metadata
import io
import pathlib
import re
import sys
import tomllib

from rich.console import Console
from rich_rst import RestructuredText

ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "crates" / "rich-ext" / "tests" / "fixtures" / "rst"
WIDTHS = (80, 40)
# Documents with no code blocks, whose colours are comparable.
COLOUR = {"inline", "lists", "defs", "tables", "blocks_nocode", "line_blocks", "wide_tables"}


def check_versions() -> None:
    pins = tomllib.loads((ROOT / "UPSTREAM.toml").read_text())
    for section in ("rich", "rich-rst"):
        expected = pins[section]["version"]
        installed = importlib.metadata.version(pins[section]["pypi"])
        if installed != expected:
            sys.exit(f"{section}: installed {installed}, expected {expected} from UPSTREAM.toml")
    expected = pins["rich-rst"]["docutils"]
    installed = importlib.metadata.version("docutils")
    if installed != expected:
        sys.exit(f"docutils: installed {installed}, expected {expected} from UPSTREAM.toml")


def render(source: str, width: int, colour: bool) -> str:
    # As rich-cli 1.8.1 builds it: `RestructuredText(data, code_theme=theme,
    # default_lexer=lexer or "python", show_errors=False)` on a console with
    # emoji off.
    file = io.StringIO()
    console = Console(
        file=file,
        width=width,
        force_terminal=colour,
        color_system="truecolor" if colour else None,
        emoji=False,
        legacy_windows=False,
    )
    console.print(
        RestructuredText(
            source, code_theme="ansi_dark", default_lexer="python", show_errors=False
        )
    )
    return re.sub(r"\x1b\]8;id=\d+;", "\x1b]8;;", file.getvalue())


def main() -> None:
    check_versions()
    for path in sorted(FIXTURES.glob("*.rst")):
        source = path.read_text()
        for width in WIDTHS:
            out = path.with_name(f"{path.stem}.{width}.txt")
            out.write_text(render(source, width, colour=False), newline="\n")
            if path.stem in COLOUR:
                out = path.with_name(f"{path.stem}.{width}.ansi")
                out.write_text(render(source, width, colour=True), newline="\n")
    print(f"captured {FIXTURES.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
