"""``rich.syntax``: ``Syntax``, plus the port's code-highlighter choice.

``Syntax(..., highlighter="syntect")`` (or ``"lumis"`` in a lumis build)
picks the engine by name; ``code_highlighters()`` lists the names and
``code_themes(name)`` an engine's themes. Rich highlights with Pygments, so
token colours differ from Rich's (see docs/python/syntax.md).
"""

from ._native import Syntax, code_highlighters, code_themes

DEFAULT_THEME = "monokai"

__all__ = ["Syntax", "code_highlighters", "code_themes"]
