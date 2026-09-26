"""``rs_rich.ext.layout``: bounded layouts: constraints, allocation, nodes and overflow (``rich_ext::layout``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    Constraint,
    allocate,
    LayoutNode,
    Overflowing,
    fit_segments,
    ConstraintError,
)

__all__ = [
    "Constraint",
    "allocate",
    "LayoutNode",
    "Overflowing",
    "fit_segments",
    "ConstraintError",
]
