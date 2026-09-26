"""``rs_rich.ext.hex``: hex dumps (``rich_ext::hex``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    HexView,
    find_all,
    byte_class,
    MAX_BYTES_PER_LINE,
)

__all__ = [
    "HexView",
    "find_all",
    "byte_class",
    "MAX_BYTES_PER_LINE",
]
