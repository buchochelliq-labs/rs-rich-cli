"""``rs_rich.ext.sanitize``: making untrusted text inert (``rich_ext::sanitize``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    sanitize_terminal_controls,
    sanitize_terminal_and_bidi_controls,
    sanitize_single_line,
    is_bidi_control,
)

__all__ = [
    "sanitize_terminal_controls",
    "sanitize_terminal_and_bidi_controls",
    "sanitize_single_line",
    "is_bidi_control",
]
