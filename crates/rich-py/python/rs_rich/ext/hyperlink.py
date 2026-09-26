"""``rs_rich.ext.hyperlink``: oSC 8 links for URLs, paths and issue references (``rich_ext::hyperlink``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    Hyperlinker,
    FoundLink,
)

__all__ = [
    "Hyperlinker",
    "FoundLink",
]
