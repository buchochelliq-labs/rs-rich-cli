"""``rs_rich.ext.frame``: rendered results as frames (``rich_ext::frame``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate. A
``Frame`` holds a render as rows of styled runs: its plain text, its cells
(one per terminal column), the ANSI bytes the console writes for it, and a
cell-level ``diff`` against a previous frame.
"""

from .._native import RenderFrame

# The Rust name; the flat native module's `Frame` is Rich's traceback frame.
Frame = RenderFrame

__all__ = [
    "Frame",
    "RenderFrame",
]
