"""``rs_rich.ext.stacktrace``: stack traces from Rust, Python, Java and JavaScript (``rich_ext::stacktrace``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    StackTrace,
    TraceFrame,
    parse_stacktrace,
    MAX_TRACE_CAUSES,
)

# The Rust names, where the flat native module needed a longer one.
parse = parse_stacktrace
MAX_CAUSES = MAX_TRACE_CAUSES

__all__ = [
    "StackTrace",
    "TraceFrame",
    "parse_stacktrace",
    "MAX_TRACE_CAUSES",
    "parse",
    "MAX_CAUSES",
]
