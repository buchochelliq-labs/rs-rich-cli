"""``rs_rich.ext.fidelity``: rendering anything at a lower fidelity (``rich_ext::fidelity``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    select_fidelity,
    Degrade,
    degrade_segments,
    ascii_text,
    FIDELITY_LEVELS,
)

# The Rust names, where the flat native module needed a longer one.
LEVELS = FIDELITY_LEVELS

__all__ = [
    "select_fidelity",
    "Degrade",
    "degrade_segments",
    "ascii_text",
    "FIDELITY_LEVELS",
    "LEVELS",
]
