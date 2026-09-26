"""``rich.markup``: ``escape``, ``render`` and ``Tag``."""

from ._native import MarkupError, Tag, escape
from ._native import render_markup as render

__all__ = ["MarkupError", "Tag", "escape", "render"]
