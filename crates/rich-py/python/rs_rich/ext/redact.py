"""``rs_rich.ext.redact``: masking secrets in text, ANSI text and rendered output (``rich_ext::redact``; experimental).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    Redactor,
    RedactMatch,
    Redacted,
    is_secret_key,
    REDACT_DETECTORS,
    RedactPatternError,
)

# The Rust names, where the flat native module needed a longer one.
DETECTORS = REDACT_DETECTORS

__all__ = [
    "Redactor",
    "RedactMatch",
    "Redacted",
    "is_secret_key",
    "REDACT_DETECTORS",
    "RedactPatternError",
    "DETECTORS",
]
