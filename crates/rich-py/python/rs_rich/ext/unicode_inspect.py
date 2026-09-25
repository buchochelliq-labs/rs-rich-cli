"""``rs_rich.ext.unicode_inspect``: graphemes, code points and widths (``rich_ext::unicode_inspect``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    GraphemeCluster,
    UnicodeView,
    classify_cluster,
    control_picture,
)

# The Rust names, where the flat native module needed a longer one.
Cluster = GraphemeCluster
classify = classify_cluster

__all__ = [
    "GraphemeCluster",
    "UnicodeView",
    "classify_cluster",
    "control_picture",
    "Cluster",
    "classify",
]
