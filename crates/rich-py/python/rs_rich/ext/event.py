"""``rs_rich.ext.event``: typed, structured log events (``rich_ext::event``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    StructuredEvent,
)

__all__ = [
    "StructuredEvent",
]
