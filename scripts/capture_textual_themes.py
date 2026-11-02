"""Capture Textual's generated theme colours for intuiTUIve's golden test.

intuiTUIve's `Palette` (crates/rich-intuituive/src/palette.rs) ports
Textual's `ColorSystem` (textual/design.py) and its built-in themes
(textual/theme.py; Textual is MIT licensed). This script writes the colours
Textual generates for each built-in theme, for the names the port makes, to
crates/rich-intuituive/tests/fixtures/textual_themes.txt, one
`theme name #RRGGBB[AA]` per line. `tests/palette.rs` compares them.

Run it in a virtualenv of its own (Textual pulls in a `rich` of its own,
which must not replace the pinned one this repository's parity tests use):

    python -m venv .venv-textual
    .venv-textual/bin/pip install "textual==8.2.8"
    .venv-textual/bin/python scripts/capture_textual_themes.py
"""

from __future__ import annotations

import sys
from importlib.metadata import version
from pathlib import Path

from textual.color import Color
from textual.design import ColorSystem
from textual.theme import BUILTIN_THEMES

# The names intuiTUIve's palette generates, in Textual's order.
SHADED = [
    "primary",
    "secondary",
    "primary-background",
    "secondary-background",
    "background",
    "foreground",
    "panel",
    "boost",
    "surface",
    "warning",
    "error",
    "success",
    "accent",
]
SUFFIXES = ["-darken-3", "-darken-2", "-darken-1", "", "-lighten-1", "-lighten-2", "-lighten-3"]
ROLES = ["primary", "secondary", "warning", "error", "success", "accent"]
NAMES = (
    [f"text-{r}" for r in ROLES]
    + [f"{name}{suffix}" for name in SHADED for suffix in SUFFIXES]
    + ["text", "text-muted", "text-disabled"]
    + [f"{r}-muted" for r in ROLES]
    + [
        "foreground-muted",
        "foreground-disabled",
        "border",
        "border-blurred",
        "surface-active",
        "scrollbar",
        "scrollbar-hover",
        "scrollbar-background",
        "block-cursor-background",
    ]
)


def resolve(value: str, background: Color) -> str:
    """`auto N%` is the contrast text over the background; the rest parse."""
    if value.startswith("auto"):
        alpha = float(value.split()[1].rstrip("%")) / 100
        return background.get_contrast_text(alpha).hex
    return Color.parse(value).hex


def main() -> int:
    out = Path(__file__).resolve().parent.parent / (
        "crates/rich-intuituive/tests/fixtures/textual_themes.txt"
    )
    lines = [f"# Textual {version('textual')}: scripts/capture_textual_themes.py"]
    for name, theme in BUILTIN_THEMES.items():
        if theme.primary.startswith("ansi_"):
            continue
        system: ColorSystem = theme.to_color_system()
        colors = system.generate()
        background = Color.parse(colors["background"])
        for key in NAMES:
            lines.append(f"{name} {key} {resolve(colors[key], background)}")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {len(lines) - 1} colours to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
