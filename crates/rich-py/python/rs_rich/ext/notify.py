"""``rs_rich.ext.notify``: transient notifications (``rich_ext::notify``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    Notification,
    Notifications,
)

__all__ = [
    "Notification",
    "Notifications",
]
