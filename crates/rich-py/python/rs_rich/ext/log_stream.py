"""``rs_rich.ext.log_stream``: log records read from JSON Lines or text logs, filtered, folded and grouped (``rich_ext::log_stream``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate, the library
behind ``rich log``.
"""

from .._native import (
    LogRecord,
    LogAssembler,
    LevelFilter,
    LogPredicate,
    LogFilter,
    LogRepeat,
    Collapser,
    LogGroupHeader,
    LogSummary,
    parse_log_line,
    log_parser_names,
    log_group_key,
    parse_log_timestamp,
    format_log_offset,
    LOG_STYLES,
)

# The Rust names, where the flat native module needed a longer one.
Record = LogRecord
Assembler = LogAssembler
Predicate = LogPredicate
Filter = LogFilter
Repeat = LogRepeat
GroupHeader = LogGroupHeader
Summary = LogSummary
parse_line = parse_log_line
parser_names = log_parser_names
group_key = log_group_key
parse_timestamp = parse_log_timestamp
format_offset = format_log_offset
STYLES = LOG_STYLES

__all__ = [
    "LogRecord",
    "LogAssembler",
    "LevelFilter",
    "LogPredicate",
    "LogFilter",
    "LogRepeat",
    "Collapser",
    "LogGroupHeader",
    "LogSummary",
    "parse_log_line",
    "log_parser_names",
    "log_group_key",
    "parse_log_timestamp",
    "format_log_offset",
    "LOG_STYLES",
    "Record",
    "Assembler",
    "Predicate",
    "Filter",
    "Repeat",
    "GroupHeader",
    "Summary",
    "parse_line",
    "parser_names",
    "group_key",
    "parse_timestamp",
    "format_offset",
    "STYLES",
]
