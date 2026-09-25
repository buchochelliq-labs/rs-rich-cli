"""``rs_rich.ext.log_handler``: structured events laid out like Rich's logging handler (``rich_ext::log_handler``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    EventHandler,
    HandledEvent,
    EVENT_KEYWORDS,
)

# The Rust names, where the flat native module needed a longer one.
KEYWORDS = EVENT_KEYWORDS

__all__ = [
    "EventHandler",
    "HandledEvent",
    "EVENT_KEYWORDS",
    "KEYWORDS",
]
