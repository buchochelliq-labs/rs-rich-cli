"""``rs_rich.ext.a11y``: accessibility: policy, contrast checks and screen-reader text (``rich_ext::a11y``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    AccessibilityPolicy,
    status_marker,
    contrast_ratio,
    relative_luminance,
    delta_e,
    to_lab,
    simulate_deficiency,
    suggest_color,
    ContrastFinding,
    check_theme,
    ContrastReport,
    semantic_text,
    accessible_text,
)

__all__ = [
    "AccessibilityPolicy",
    "status_marker",
    "contrast_ratio",
    "relative_luminance",
    "delta_e",
    "to_lab",
    "simulate_deficiency",
    "suggest_color",
    "ContrastFinding",
    "check_theme",
    "ContrastReport",
    "semantic_text",
    "accessible_text",
]
