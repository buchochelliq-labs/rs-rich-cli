"""``rs_rich.ext.capabilities``: what the terminal supports, and why (``rich_ext::capabilities``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    CapabilityReport,
    detect_capabilities,
    CAPABILITY_OVERRIDE_VARS,
)

# The Rust names, where the flat native module needed a longer one.
detect = detect_capabilities
OVERRIDE_VARS = CAPABILITY_OVERRIDE_VARS

__all__ = [
    "CapabilityReport",
    "detect_capabilities",
    "CAPABILITY_OVERRIDE_VARS",
    "detect",
    "OVERRIDE_VARS",
]
