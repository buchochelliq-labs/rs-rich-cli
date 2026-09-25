"""``rs_rich.ext.target``: explicit, deterministic render targets (``rich_ext::target``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    RenderTarget,
    resolve_target_capabilities,
)

# The Rust names, where the flat native module needed a longer one.
resolve_capabilities = resolve_target_capabilities

__all__ = [
    "RenderTarget",
    "resolve_target_capabilities",
    "resolve_capabilities",
]
