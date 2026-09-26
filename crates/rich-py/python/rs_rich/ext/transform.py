"""``rs_rich.ext.transform``: composable transforms and pipelines (``rich_ext::transform``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    Pipeline,
    KeepLines,
    HighlightMatches,
    TransformError,
    PipelineError,
)

__all__ = [
    "Pipeline",
    "KeepLines",
    "HighlightMatches",
    "TransformError",
    "PipelineError",
]
