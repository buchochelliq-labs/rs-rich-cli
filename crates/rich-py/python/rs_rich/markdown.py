"""``rich.markdown``: ``Markdown``.

``Markdown(..., highlighter="lumis")`` picks the code highlighter for code
blocks by name (see ``rs_rich.syntax.code_highlighters()``); not in Rich.
"""

from ._native import Markdown

__all__ = ["Markdown"]
