"""``rich.errors``: the exceptions rs_rich raises, with Rich's hierarchy."""

from ._native import (
    ConsoleError,
    LiveError,
    MarkupError,
    MissingStyle,
    NoAltScreen,
    NotRenderableError,
    StyleError,
    StyleStackError,
    StyleSyntaxError,
)

__all__ = [
    "ConsoleError",
    "StyleError",
    "StyleSyntaxError",
    "MissingStyle",
    "StyleStackError",
    "NotRenderableError",
    "MarkupError",
    "LiveError",
    "NoAltScreen",
]
