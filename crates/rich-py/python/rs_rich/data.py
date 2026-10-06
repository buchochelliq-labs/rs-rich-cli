"""``rs_rich.data``: tabular data, profiles and data views (``rs-rich-data``; no Rich counterpart).

Rows come from CSV, TSV or JSON Lines (``read_csv``, ``read_tsv``,
``read_jsonl``, ``read_file``) or from Python (``Rows``). ``infer`` types
their columns, with the evidence; ``Stats`` summarises each column;
``Profile`` profiles rows, text or a file in one bounded pass; checks become
a ``QualityReport``. ``ResultSet`` draws rows as a SQL result and
``VirtualTable`` one window of them.

The schema views read a format-neutral ``Schema`` (from JSON Schema or SQL
DDL): ``SchemaTree``, ``SchemaDiff``, ``SchemaTimeline`` and ``ErDiagram``.
``ConflictView`` shows a file's merge conflicts and ``RecordView`` one record
as a table of fields.
"""

from ._native import (
    CheckResult,
    ColumnInference,
    ColumnStats,
    ConflictError,
    ConflictView,
    DataSourceError,
    ErDiagram,
    Inference,
    MergeConflict,
    Profile,
    QualityReport,
    RecordView,
    ResultSet,
    Rows,
    Schema,
    SchemaChange,
    SchemaDiff,
    SchemaError,
    SchemaField,
    SchemaTimeline,
    SchemaTree,
    Stats,
    VirtualTable,
    check_not_null,
    check_unique,
    infer,
    read_csv,
    read_file,
    read_jsonl,
    read_tsv,
)

# The crate's names for the same things.
DataError = DataSourceError
not_null = check_not_null
unique = check_unique

__all__ = [
    "CheckResult",
    "ColumnInference",
    "ColumnStats",
    "ConflictError",
    "ConflictView",
    "DataSourceError",
    "ErDiagram",
    "Inference",
    "MergeConflict",
    "Profile",
    "QualityReport",
    "RecordView",
    "ResultSet",
    "Rows",
    "Schema",
    "SchemaChange",
    "SchemaDiff",
    "SchemaError",
    "SchemaField",
    "SchemaTimeline",
    "SchemaTree",
    "Stats",
    "VirtualTable",
    "check_not_null",
    "check_unique",
    "infer",
    "read_csv",
    "read_file",
    "read_jsonl",
    "read_tsv",
]
