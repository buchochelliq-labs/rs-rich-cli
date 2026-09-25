"""``rs_rich.ext.size_bar``: a size against a total or a limit (``rich_ext::size_bar``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    SizeBar,
    SIZE_BAR_STYLES,
)

# The Rust names, where the flat native module needed a longer one.
STYLES = SIZE_BAR_STYLES

__all__ = [
    "SizeBar",
    "SIZE_BAR_STYLES",
    "STYLES",
]
