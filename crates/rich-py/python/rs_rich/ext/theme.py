"""``rs_rich.ext.theme``: the extended theme: Rich's styles plus rich-ext's (``rich_ext::theme``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    extended_theme,
    EXTRA_STYLES,
    STYLE_TABLES,
)

__all__ = [
    "extended_theme",
    "EXTRA_STYLES",
    "STYLE_TABLES",
]
