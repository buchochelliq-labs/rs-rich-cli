"""``rs_rich.ext.table``: typed, sortable, groupable tables, streaming tables and conditional styles (``rich_ext::table``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    DataColumn,
    SortKey,
    Aggregate,
    GroupBy,
    TableData,
    TableSort,
    TableGroup,
    StreamingTable,
    StyleRule,
    StyleRules,
    TABLE_STYLES,
)

# The Rust names, where the flat native module needed a longer one.
Column = DataColumn
Sort = TableSort
Group = TableGroup
STYLES = TABLE_STYLES

__all__ = [
    "DataColumn",
    "SortKey",
    "Aggregate",
    "GroupBy",
    "TableData",
    "TableSort",
    "TableGroup",
    "StreamingTable",
    "StyleRule",
    "StyleRules",
    "TABLE_STYLES",
    "Column",
    "Sort",
    "Group",
    "STYLES",
]
