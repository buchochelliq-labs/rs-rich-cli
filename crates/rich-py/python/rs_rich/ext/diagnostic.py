"""``rs_rich.ext.diagnostic``: compiler-style diagnostics (``rich_ext::diagnostic``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    Diagnostic,
    Location,
    SourceSnippet,
    Suggestion,
    DiagnosticSpanError,
)

__all__ = [
    "Diagnostic",
    "Location",
    "SourceSnippet",
    "Suggestion",
    "DiagnosticSpanError",
]
