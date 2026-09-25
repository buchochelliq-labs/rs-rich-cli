"""Rich-compatible terminal rendering, backed by the rs-rich Rust port.

Every class here is implemented in Rust (``rs_rich._native``); these modules
only re-export them under Rich's module paths, so a program can move from
``rich`` by changing its imports. See the supported surface in the README.
"""

from typing import Any, Optional

from ._native import __version__
from .console import Console

__all__ = ["Console", "__version__", "get_console", "print"]

_console: Optional[Console] = None


def get_console() -> Console:
    """The global console ``rs_rich.print`` writes to, created on first use."""
    global _console
    if _console is None:
        _console = Console()
    return _console


def print(*objects: Any, sep: str = " ", end: str = "\n") -> None:  # noqa: A001
    """``rich.print``: print objects to the global console."""
    get_console().print(*objects, sep=sep, end=end)
