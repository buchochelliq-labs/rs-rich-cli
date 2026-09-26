"""``rs_rich.ext.data``: structured data: one document tree for JSON, YAML, TOML, XML, INI and dotenv (``rich_ext::data``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    DataNode,
    parse_data,
    detect_format,
    format_for,
    Explorer,
    TableView,
    FlatView,
    flatten,
    unflatten,
    SearchQuery,
    SearchMatch,
    search,
    SearchResults,
    select,
    selector_backends,
    DataChange,
    diff_data,
    DataDiffView,
    Redaction,
    ConfigFileView,
    DataJson,
    Document,
    Select,
    Filter,
    Highlight,
    Redact,
    DataError,
    SelectError,
    DATA_FORMATS,
    SECRET_KEYS,
)

# The Rust names, where the flat native module needed a longer one.
parse = parse_data
Node = DataNode
diff = diff_data
DiffView = DataDiffView
Change = DataChange
json = DataJson
FORMATS = DATA_FORMATS

__all__ = [
    "DataNode",
    "parse_data",
    "detect_format",
    "format_for",
    "Explorer",
    "TableView",
    "FlatView",
    "flatten",
    "unflatten",
    "SearchQuery",
    "SearchMatch",
    "search",
    "SearchResults",
    "select",
    "selector_backends",
    "DataChange",
    "diff_data",
    "DataDiffView",
    "Redaction",
    "ConfigFileView",
    "DataJson",
    "Document",
    "Select",
    "Filter",
    "Highlight",
    "Redact",
    "DataError",
    "SelectError",
    "DATA_FORMATS",
    "SECRET_KEYS",
    "parse",
    "Node",
    "diff",
    "DiffView",
    "Change",
    "json",
    "FORMATS",
]
