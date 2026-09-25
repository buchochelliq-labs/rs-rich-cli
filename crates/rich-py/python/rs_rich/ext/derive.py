"""``rs_rich.ext.derive``: records as field grids and tables, the runtime behind ``#[derive(Rich)]`` (``rich_ext::derive``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate.
"""

from .._native import (
    RecordField,
    Record,
    RecordsTable,
    records_table,
)

# The Rust names, where the flat native module needed a longer one.
Field = RecordField
table = records_table

__all__ = [
    "RecordField",
    "Record",
    "RecordsTable",
    "records_table",
    "Field",
    "table",
]
