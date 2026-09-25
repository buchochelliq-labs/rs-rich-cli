"""Shared fixtures for the rs_rich tests."""

from __future__ import annotations

import io

import pytest

from rs_rich.console import Console


@pytest.fixture(autouse=True)
def _no_ambient_terminal_settings(monkeypatch):
    # Width and colour must come from each test's own arguments.
    for name in ("COLUMNS", "NO_COLOR", "FORCE_COLOR", "COLORTERM"):
        monkeypatch.delenv(name, raising=False)


def render(*objects, width: int = 40, color: bool = False, **print_options) -> str:
    """Print objects to a fresh in-memory console and return the output."""
    out = io.StringIO()
    console = Console(
        file=out,
        width=width,
        force_terminal=color,
        color_system="truecolor" if color else None,
    )
    console.print(*objects, **print_options)
    return out.getvalue()
