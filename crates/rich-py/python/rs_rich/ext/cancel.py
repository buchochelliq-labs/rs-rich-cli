"""``rs_rich.ext.cancel``: one cancellation flag shared by workflows, transfers and countdowns (``rich_ext::cancel``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    CancelToken,
)

__all__ = [
    "CancelToken",
]
