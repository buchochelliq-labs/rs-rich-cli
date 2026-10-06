# Data, schemas and developer views

```python
from rs_rich import data
```

`rs_rich.data` is the `rs-rich-data` crate from Python, with the 0.0.16
views from `rs-rich-ext`: rows read from CSV, TSV or JSON Lines, type
inference, column statistics, profiles, data-quality reports, SQL result
sets and virtualised tables; the schema model and its tree, diff, timeline
and ER views; merge conflicts and the record inspector. Rich has no
counterpart. Everything renders in Rust and prints with `Console.print`.
The [data guide](https://buchochelliq-labs.github.io/rs-rich-cli/guide/data/)
describes the same API from Rust.

| Name | What it is |
|---|---|
| `read_csv`, `read_tsv`, `read_jsonl`, `read_file` | text or a file read into `Rows` |
| `Rows` | rows in memory: column names, an optional `Schema`, cells |
| `infer(rows)`, `Rows.infer()` | each column's type, with the evidence (`Inference`) |
| `Stats` | per-column statistics |
| `Profile` | a bounded profile of rows, text or a file |
| `CheckResult`, `QualityReport`, `check_not_null`, `check_unique` | data-quality results as a report |
| `ResultSet` | rows drawn as a SQL query result |
| `VirtualTable` | one window of rows, with a position line |
| `Schema`, `SchemaField` | the format-neutral schema model, from JSON Schema or SQL DDL |
| `SchemaTree`, `SchemaDiff`, `SchemaTimeline`, `ErDiagram` | the schema views |
| `ConflictView` | a file's merge conflicts |
| `RecordView` | one record as a table of fields |

`check_not_null`, `check_unique` and `DataSourceError` are also exported as
`not_null`, `unique` and `DataError`, their Rust names. `ConflictView` is
also in `rs_rich.ext.diff`, and `RecordView` in `rs_rich.ext.data`.

## Rows

```text
read_csv(text, *, delimiter=None, header=None) -> Rows
read_tsv(text, *, header=None) -> Rows
read_jsonl(text) -> Rows
read_file(path, *, format=None, header=None) -> Rows
Rows(columns, rows=None, *, schema=None)
```

CSV and TSV go through the port of Python's `csv` sniffer and reader that
`rich --csv` uses: the delimiter is sniffed unless given. `header` is
`True`, `False`, `"sniff"` (the sniffer's guess, as `rich --csv`), or
`None`: the first row is a header unless every cell in it is a number, as
`rich profile` reads it. JSON Lines rows have a column per key, in the order
first seen. `read_file` picks the format by the extension (`.csv`, `.tsv`,
`.jsonl`, `.ndjson`), then by the first character, or by `format`
(`"csv"`, `"tsv"`, `"jsonl"`). A row that cannot be read raises
`DataSourceError` with its `line`.

Cells are `None`, `int`, `float`, `str` or `Text`. A CSV's cells are all text
until inference types them. `Rows` renders as a table; `to_table_data()`
gives an [`rs_rich.ext.table.TableData`](ext/tables.md) to sort, group and
total.

```python
from rs_rich.console import Console
from rs_rich import data

console = Console(width=60, color_system=None)
rows = data.read_csv("service,p99,up\nweb,120,true\napi,35.5,false\ndb,,true\n")
print(rows.columns, rows.rows[0])
console.print(rows)
```

```text
['service', 'p99', 'up'] ['web', '120', 'true']
┏━━━━━━━━━┳━━━━━━┳━━━━━━━┓
┃ service ┃ p99  ┃ up    ┃
┡━━━━━━━━━╇━━━━━━╇━━━━━━━┩
│ web     │ 120  │ true  │
│ api     │ 35.5 │ false │
│ db      │      │ true  │
└─────────┴──────┴───────┘
```

## Inference and statistics

```text
infer(rows, *, null_tokens=None, overrides=None) -> Inference
Stats(rows, *, quantiles=None, top=3)
```

`infer` reads every cell and picks `"integer"`, `"float"`, `"boolean"`,
`"date"`, `"timestamp"`, `"null"` or `"text"` per column. Each
`ColumnInference` has its `type`, its `evidence` (how many cells parsed as
each type) and a `summary`. `null_tokens` replaces the cells that count as
null (`""`, `null`, `NULL`, `NA`, `N/A`); `overrides` maps column names to
types. Nothing changes until `Inference.apply(rows)`: then integer and float
columns become numbers, null tokens `None`, and the rows get the inferred
`schema`, so numeric columns align right.

`Stats` gives each column its `count`, `nulls`, `distinct`, `min`, `max`,
and for numbers the `mean`, `median` and `quantiles` (the quartiles unless
given), with the `top` most common values.

```python
inference = data.infer(rows)
console.print(inference)
print(inference.columns[1].summary)
inference.apply(rows)
print(rows.rows[2])
p99 = data.Stats(rows).columns[1]
print(p99.mean, p99.quantiles)
```

```text
┏━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━┳━━━━━━━┓
┃ column  ┃ type    ┃ parsed ┃ nulls ┃
┡━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━╇━━━━━━━┩
│ service │ text    │    3/3 │     0 │
│ p99     │ float   │    2/2 │     1 │
│ up      │ boolean │    3/3 │     0 │
└─────────┴─────────┴────────┴───────┘
float: 2 of 2 values, 1 null
['db', None, 'true']
77.75 [(0.25, 56.625), (0.75, 98.875)]
```

## Profiles

```text
Profile(rows, *, name=None, sample=10000, top=5, bins=10, buckets=20,
        columns=None, nulls=None)
Profile.from_text(text, *, format="csv", name=None, ...)
Profile.from_path(path, *, format=None, ...)
```

A profile says what each column holds: its type, nulls, distinct values,
statistics and a distribution (a histogram for a number column, the most
common values otherwise), with a map of where the nulls are. It keeps a
fixed-seed sample of at most `sample` rows, so the same input gives the
same profile; the row count and null counts count every row, and `sampled`
says whether there was a sample. `from_path` streams the file, so memory is
the sample whatever the file's size. `columns` profiles only those columns;
`top`, `bins` and `buckets` size the distributions and the map; `nulls`
replaces the null tokens.

`to_json()` is `rich profile --report json`'s output; `to_dict()` is the same
as Python values, and `columns` and `column(name)` read it per column.

```python
profile = data.Profile.from_text("service,p99\nweb,120\napi,35\ndb,\nweb,80\n", name="p99.csv")
print(profile.heading, profile.rows, profile.sampled)
print(profile.column("p99")["mean"], profile.column("service")["distinct"])
Console(width=72, color_system=None).print(profile)
```

```text
p99.csv: 4 rows, 2 columns 4 False
78.33333333333333 3
p99.csv: 4 rows, 2 columns

┏━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━━━┳━━━━━┳━━━━━┳━━━━━━━━━┓
┃ column  ┃ type    ┃   nulls ┃ distinct ┃ min ┃ max ┃    mean ┃
┡━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━━━╇━━━━━╇━━━━━╇━━━━━━━━━┩
│ service │ text    │       0 │        3 │ api │ web │         │
│ p99     │ integer │ 1 (25%) │        3 │  35 │ 120 │ 78.3333 │
└─────────┴─────────┴─────────┴──────────┴─────┴─────┴─────────┘

service · text · 3 distinct
  web ████████████████████████ 2
  api ████████████             1
  db  ████████████             1

p99 · integer · 1 null (25%) · median 80 (p25 57.5, p75 100)
  [30, 40)   ████████████████████████ 1
  [40, 50)                            0
  [50, 60)                            0
  [60, 70)                            0
  [70, 80)                            0
  [80, 90)   ████████████████████████ 1
  [90, 100)                           0
  [100, 110)                          0
  [110, 120)                          0
  [120, 130] ████████████████████████ 1

missing values (% null; rows top to bottom, a row each)
  service p99     
1                 
2                 
3         ████████
4                 
0 [ ░▒▓█] 100     
```

## Data quality

```text
CheckResult(check, status, *, column=None, observed=None, expected=None,
            message=None, failing_rows=None)
QualityReport(results, *, show_rows=5)
QualityReport.from_json(text, *, show_rows=5)
check_not_null(rows, column) -> CheckResult
check_unique(rows, column) -> CheckResult
```

A `CheckResult` is one check's outcome: `status` is `"pass"`, `"warn"`,
`"fail"` or `"error"`, and `failing_rows` a sample of the rows it failed on
as `(columns, rows)` or `(columns, rows, total)`, every cell text.
`QualityReport` draws results as a test report: failures, errors and
warnings first with their rows, then a table of every check and a summary.
`ok` is whether nothing failed or errored, `totals` the counts, and
`to_json()` / `from_json()` round-trip the results (`from_json` also takes a
plain list), so another tool's results can be drawn. `check_not_null` and
`check_unique` are two small checks over `Rows`; an unknown column raises
`KeyError`.

```python
report = data.QualityReport([
    data.check_not_null(rows, "p99"),
    data.check_unique(rows, "service"),
    data.CheckResult("row_count", "warn", observed="3", expected=">= 100"),
])
console.print(report)
print(report.ok, report.totals)
```

```text
FAIL not_null › p99
  observed 1 null · expected 0
  ┏━━━━━┳━━━━━━━━━┳━━━━━┳━━━━━━┓
  ┃ row ┃ service ┃ p99 ┃ up   ┃
  ┡━━━━━╇━━━━━━━━━╇━━━━━╇━━━━━━┩
  │ 3   │ db      │     │ true │
  └─────┴─────────┴─────┴──────┘

WARN row_count
  observed 3 · expected >= 100

┏━━━━━━━━┳━━━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━━━━━━━┳━━━━━━━━━━┓
┃ status ┃ check     ┃ column  ┃ observed     ┃ expected ┃
┡━━━━━━━━╇━━━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━━━━━━━╇━━━━━━━━━━┩
│ FAIL   │ not_null  │ p99     │ 1 null       │ 0        │
│ PASS   │ unique    │ service │ 0 duplicates │ 0        │
│ WARN   │ row_count │         │ 3            │ >= 100   │
└────────┴───────────┴─────────┴──────────────┴──────────┘
1 passed, 1 warned, 1 failed
False {'passed': 1, 'warned': 1, 'failed': 1, 'errored': 0}
```

## Result sets and virtualised tables

```text
ResultSet(rows, *, offset=0, limit=1000, elapsed=None, null_marker="NULL",
          title=None)
VirtualTable(rows, *, offset=0, height=20, row_numbers=False, null_marker=None,
             show_position=True, wrap=False, max_column_width=40, fit_window=False,
             title=None)
```

`ResultSet` draws rows the way a database shell does: columns aligned by
the rows' schema (numbers right), `NULL` marked apart from empty text, and a
footer with the row count and `elapsed` (seconds, or a `timedelta`). `limit`
and `offset` choose a window, and a windowed result says where it is.

`VirtualTable` lays out only `height` rows from `offset`, with column widths
from a sample (the first 100 rows) so they never jump while the window
moves; `fit_window=True` also widens them to the rows shown, for a window
printed once. `scroll_by(n)`
moves it within the rows, `offset` and `height` can be set, `window()` is
the `(start, end)` shown and `position()` the line under the table.

```python
console.print(data.ResultSet(rows, elapsed=0.012))
numbers = data.Rows(["n", "square"], [[i, i * i] for i in range(1, 1001)])
table = data.VirtualTable(numbers, offset=500, height=3)
console.print(table)
table.scroll_by(1000)
print(table.window(), table.position())
```

```text
┏━━━━━━━━━┳━━━━━━┳━━━━━━━┓
┃ service ┃  p99 ┃  up   ┃
┡━━━━━━━━━╇━━━━━━╇━━━━━━━┩
│ web     │  120 │ true  │
│ api     │ 35.5 │ false │
│ db      │ NULL │ true  │
└─────────┴──────┴───────┘
(3 rows, 12ms)
┏━━━━━┳━━━━━━━━┓
┃   n ┃ square ┃
┡━━━━━╇━━━━━━━━┩
│ 501 │ 251001 │
│ 502 │ 252004 │
│ 503 │ 253009 │
└─────┴────────┘
rows 501–503 of 1,000
(997, 1000) rows 998–1,000 of 1,000
```

## Schemas

```text
Schema.from_json_schema(schema) -> Schema
Schema.from_sql(ddl) -> Schema
SchemaTree(schema, *, title=None, max_depth=32)
SchemaDiff(old, new, *, old_name="old", new_name="new")
SchemaTimeline(versions=None, *, details=True)
ErDiagram(schema, *, direction="LR", ascii=None)
ErDiagram.from_sql(ddl, *, direction="LR", ascii=None)
```

A `Schema` is the format-neutral model every schema view reads. Read one
from a JSON Schema (a `dict`, or JSON text) or from SQL DDL, whose tables
are `tables` (each a `Schema`) and whose skipped statements are `notes`.
DDL the reader cannot follow raises `SchemaError` with its `line`. A
`SchemaField` has its `name`, `type` (as the source writes it), `required`,
`nullable`, `description`, `constraints`, `primary_key`, `unique`,
`references` and `children`. `Inference.schema` is rows' schema in the same
model.

Every view takes a `Schema`, or a JSON Schema as a `dict` or JSON text. A
`Schema` prints as its tree. `SchemaDiff` compares two JSON Schemas
directly (following `$ref`s) and anything else through the model; each
`SchemaChange` has a `kind`, `marker`, `path`, `detail` and whether it is
`breaking`. `SchemaTimeline` takes `(label, schema)` or
`(label, schema, at)` versions, or `push` adds them. `ErDiagram` draws
tables with their keys and an edge per foreign key; `entities` and
`relationships` read it back.

```python
ddl = """
CREATE TABLE users (id INT PRIMARY KEY, email TEXT UNIQUE);
CREATE TABLE posts (id INT PRIMARY KEY, author INT NOT NULL REFERENCES users (id));
"""
shop = data.Schema.from_sql(ddl)
console.print(data.SchemaTree(shop, title="shop"))
console.print(data.ErDiagram(shop))
print(data.ErDiagram(shop).relationships)

old = {"type": "object", "properties": {"id": {"type": "integer"}}}
new = {"type": "object", "required": ["id"],
       "properties": {"id": {"type": "string"}, "name": {"type": "string"}}}
diff = data.SchemaDiff(old, new, old_name="v1", new_name="v2")
console.print(diff)
print([(c.marker, c.path, c.breaking) for c in diff.changes])
```

```text
shop  2 tables
├── users  table
│   ├── id (required)  INT  primary key
│   └── email  TEXT  unique
└── posts  table
    ├── id (required)  INT  primary key
    └── author (required)  INT  → users.id
┌─────────────────┐                     ┌──────────────────┐
│      posts      │                     │      users       │
├─────────────────┤                     ├──────────────────┤
│ id      INT  PK ├──author → id (N:1)─►│ id     INT    PK │
│ author  INT  FK │                     │ email  TEXT?  UQ │
└─────────────────┘                     └──────────────────┘
[('posts', 'users', 'N:1')]
┏━━━┳━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━┓
┃   ┃ Where ┃ Change                  ┃          ┃
┡━━━╇━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━┩
│ ~ │ id    │ became required         │ breaking │
│ ~ │ id    │ type integer → string   │ breaking │
│ + │ name  │ property added (string) │          │
└───┴───────┴─────────────────────────┴──────────┘
v1 → v2: 3 changes, 2 breaking                    
[('~', 'id', True), ('~', 'id', True), ('+', 'name', False)]
```

## Conflicts and records

```text
ConflictView(text, *, path=None, language=None, layout="auto", context=3,
             line_numbers=True, wrap=True, base=True)
RecordView(record, *, depth=1, expand=None, collapse=None, max_items=20,
           max_string=200, show_types=True, title=None)
```

`ConflictView` reads the markers a merge leaves in a file and shows each
conflict numbered, with ours, base (diff3 markers) and theirs side by side
or stacked (`layout`: `"auto"`, `"side_by_side"`, `"stacked"`), highlighted
by `language` or the extension of `path`, between `context` lines. With
colour off every side line keeps a marker: `<` ours, `|` base, `>` theirs.
`conflicts` reads them back as `MergeConflict`s (`number`, `start_line`,
`end_line`, and each side's text and label). Markers out of order or a
conflict never closed raise `ConflictError` with its `line`.

`RecordView` shows one record (any Python value, or an `rs_rich.ext.data`
`DataNode` or `Document`) as a `field | type | value` table, nested values
as trees opened `depth` levels. `expand` and `collapse` open or fold
branches by path (`owner.oncall`, `ports[1]`); `branches` lists the fields
there are to open.

```python
console.print(data.ConflictView("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> topic\nd\n", layout="stacked"))
record = {"id": 7, "name": "web", "owner": {"team": "infra", "oncall": {"primary": "ana"}}}
console.print(data.RecordView(record, expand=["owner.oncall"]))
```

```text
conflict 1 of 1, lines 2-6
1   a
  ours: HEAD
3 < b
  theirs: topic
5 > c
7   d
┏━━━━━━━┳━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ field ┃ type ┃ value                  ┃
┡━━━━━━━╇━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━┩
│ id    │ int  │ 7                      │
│ name  │ str  │ web                    │
│ owner │ map  │ {…} 2 keys             │
│       │      │ ├── team: "infra"      │
│       │      │ └── oncall             │
│       │      │     └── primary: "ana" │
└───────┴──────┴────────────────────────┘
```

From the shell, `rich profile FILE` profiles a data file, `rich schema`
draws schemas and `rich diff --conflicts FILE` a file's conflicts
([the command line](cli.md)).
