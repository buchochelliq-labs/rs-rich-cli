"""``rs_rich.ext.env_inspect``: environment variables and PATH-like lists (``rich_ext::env_inspect``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    EnvView,
    PathView,
    is_secret_name,
    redact_value,
    name_matches,
    is_path_like,
    OS_PATH_SEPARATOR,
)

__all__ = [
    "EnvView",
    "PathView",
    "is_secret_name",
    "redact_value",
    "name_matches",
    "is_path_like",
    "OS_PATH_SEPARATOR",
]
