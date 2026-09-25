"""``rs_rich.ext.testing``: render snapshots and rendered-diff assertions (``rich_ext::testing``; needs the ``testing`` feature).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    render_snapshot,
    assert_render_eq,
    assert_str_eq,
    assert_json_eq,
    highlighter_conformance,
)

__all__ = [
    "render_snapshot",
    "assert_render_eq",
    "assert_str_eq",
    "assert_json_eq",
    "highlighter_conformance",
]
