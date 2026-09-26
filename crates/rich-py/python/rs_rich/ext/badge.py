"""``rs_rich.ext.badge``: status, label, link and metadata chips (``rich_ext::badge``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    Badge,
    Badges,
    BADGE_STYLES,
)

# The Rust names, where the flat native module needed a longer one.
STYLES = BADGE_STYLES

__all__ = [
    "Badge",
    "Badges",
    "BADGE_STYLES",
    "STYLES",
]
