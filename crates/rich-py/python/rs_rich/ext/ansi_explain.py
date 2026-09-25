"""``rs_rich.ext.ansi_explain``: escape sequences decoded into words (``rich_ext::ansi_explain``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    AnsiToken,
    Explanation,
    explain,
    ExplanationView,
    sgr_effects,
    csi_meaning,
    osc_meaning,
    control_name,
    escape_visible,
)

__all__ = [
    "AnsiToken",
    "Explanation",
    "explain",
    "ExplanationView",
    "sgr_effects",
    "csi_meaning",
    "osc_meaning",
    "control_name",
    "escape_visible",
]
