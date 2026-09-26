"""``rs_rich.ext.format``: sizes, rates, durations, times and numbers as people read them (``rich_ext::format``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    format_size,
    format_rate,
    format_duration,
    format_clock,
    format_relative,
    format_timestamp,
    format_percent,
    format_number,
    format_compact,
)

# The Rust names, where the flat native module needed a longer one.
rate = format_rate
duration = format_duration
clock = format_clock
relative = format_relative
timestamp = format_timestamp
percent = format_percent
number = format_number
compact = format_compact

__all__ = [
    "format_size",
    "format_rate",
    "format_duration",
    "format_clock",
    "format_relative",
    "format_timestamp",
    "format_percent",
    "format_number",
    "format_compact",
    "rate",
    "duration",
    "clock",
    "relative",
    "timestamp",
    "percent",
    "number",
    "compact",
]
