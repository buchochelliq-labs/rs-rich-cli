"""``rs_rich.ext.countdown``: retry backoff, retry and rate-limit status, countdown bars and waits (``rich_ext::countdown``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    remaining_label,
    Backoff,
    CountdownBar,
    RetryStatus,
    RateLimit,
    countdown_wait,
)

# The Rust names, where the flat native module needed a longer one.
wait = countdown_wait

__all__ = [
    "remaining_label",
    "Backoff",
    "CountdownBar",
    "RetryStatus",
    "RateLimit",
    "countdown_wait",
    "wait",
]
