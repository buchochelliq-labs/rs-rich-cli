"""``rich.highlighter``: ``Highlighter`` and the built-in highlighters.

Subclass ``Highlighter`` (override ``highlight(text)``) or ``RegexHighlighter``
(set ``highlights`` and ``base_style``), as with Rich.
"""

from ._native import (
    Highlighter,
    ISO8601Highlighter,
    JSONHighlighter,
    NullHighlighter,
    RegexHighlighter,
    ReprHighlighter,
)

__all__ = [
    "Highlighter",
    "ISO8601Highlighter",
    "JSONHighlighter",
    "NullHighlighter",
    "RegexHighlighter",
    "ReprHighlighter",
]
