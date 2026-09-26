"""``rs_rich.ext.live``: several live regions and printed lines through one writer (``rich_ext::live``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    LiveCoordinator,
    RegionId,
    LiveCoordinatorError,
)

__all__ = [
    "LiveCoordinator",
    "RegionId",
    "LiveCoordinatorError",
]
