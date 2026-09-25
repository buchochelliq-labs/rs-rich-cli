"""Rich-compatible terminal rendering, backed by the rs-rich Rust port.

Every class here is implemented in Rust (``rs_rich._native``); these modules
only re-export them under Rich's module paths, so a program can move from
``rich`` by changing its imports. See the supported surface in the README.
"""

from typing import IO, Any, Optional

from . import _native
from ._native import __version__
from .console import Console

__all__ = ["Console", "__version__", "get_console", "inspect", "print", "print_json", "reconfigure"]

_console: Optional[Console] = None


def get_console() -> Console:
    """The global console ``rs_rich.print`` writes to, created on first use."""
    global _console
    if _console is None:
        _console = Console()
    return _console


def reconfigure(**kwargs: Any) -> None:
    """``rich.reconfigure``: replace the global console with ``Console(**kwargs)``.

    Rich changes the existing console in place; rs_rich's native console
    cannot be, so ``get_console()`` returns a new object afterwards.
    """
    global _console
    _console = Console(**kwargs)


def print(  # noqa: A001
    *objects: Any,
    sep: str = " ",
    end: str = "\n",
    file: Optional[IO[str]] = None,
    flush: bool = False,
) -> None:
    """``rich.print``: print objects to the global console (or to ``file``)."""
    write_console = get_console() if file is None else Console(file=file)
    write_console.print(*objects, sep=sep, end=end)


def print_json(json: Optional[str] = None, *, data: Any = None, **kwargs: Any) -> None:
    """``rich.print_json``: pretty-print JSON on the global console."""
    get_console().print_json(json, data=data, **kwargs)


def inspect(obj: Any, **kwargs: Any) -> None:
    """``rich.inspect``: provided by ``_native.inspect`` once the code area adds it."""
    implementation = getattr(_native, "inspect", None)
    if implementation is None:
        raise NotImplementedError("rs_rich.inspect is not implemented yet")
    kwargs.setdefault("console", get_console())
    implementation(obj, **kwargs)
