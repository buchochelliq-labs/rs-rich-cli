# Sorting, grouping and streaming tables

The core `Table` is a faithful port of upstream's: it lays out whatever rows
you give it, in the order you give them. `rich_ext::table` adds the data
operations upstream leaves to you, and a table for rows that keep changing:

- **`TableData`**: typed rows under column definitions, with a stable
  multi-column sort, grouping with per-group aggregates, and a totals row.
- **`StreamingTable`**: rows under stable keys for append/update workloads
  under a live display. A frame re-renders only the rows that changed.
- **`VirtualTable`**: one window of a source too large to hold, with column
  widths that stay put while it scrolls.
- **`sort`** and **`group`**: the same operations as functions over plain
  rows, for building a core `Table` yourself.

`rs-rich-data` adds a SQL-shaped result set on top (see
[SQL result sets](#sql-result-sets)).

Every view here *builds* core tables; none has its own table renderer. So the
output looks exactly like a core `Table`, box styles, column options, ASCII
fallback and all. No feature flag is needed.

## Values and columns

A cell is a `Value`: `Null`, `Int`, `Float`, `Str` (literal text, never
markup) or `Text` (styled). The type decides how the cell sorts and
aggregates. The column decides how it looks:

```rust
--8<-- "crates/rich-ext/examples/guide_tables_ext.rs:data"
```

- `Column::new(header)` is left-justified. `.justify(..)` sets the
  justification and `.options(ColumnOptions { .. })` sets any core column
  option (width, ratio, `no_wrap`, style, …).
- `.format(|value| Text)` controls how a column displays its values, as here
  for milliseconds or with `rich_ext::format` for byte sizes. Aggregates are
  displayed through the same formatter, except counts.
- `Value` converts from `&str`, `String`, `Text`, the integer types, `f64`
  and `Option<T>` (`None` is `Null`). A row shorter than the columns is
  padded with `Null`, and extra cells are dropped.

## Sorting

`sort_by` takes `SortKey`s in priority order. Later keys break ties between
rows that are equal on the earlier ones:

```rust
--8<-- "crates/rich-ext/examples/guide_tables_ext.rs:sort"
```

![Sorted by region, then errors descending](../../media/guide/guide_tables_ext-sort.svg)

- **Stable.** Rows that are equal on every key keep the order they were added
  in.
- **Empty cells last.** A `Null` or empty string sorts after every other value
  in either direction, like the missing region above.
- **Natural by default.** Numbers compare numerically, and so do strings that
  are numbers (`"10"` after `"9"`). Other strings compare case-insensitively,
  with runs of digits compared by value, so `worker-9` sorts before
  `worker-10`. Numbers sort before text. `SortKey::asc(i).lexical()` compares
  the plain strings byte by byte instead.
- **Header indicators.** Each sorted column's header gets `▲` or `▼` (styled
  `table.sort_indicator`). When there is more than one key, the indicator also
  shows the key's priority (`▲1`, `▼2`).

```rust
--8<-- "crates/rich-ext/examples/guide_tables_ext.rs:natural"
```

![Natural order: worker-9 before worker-10](../../media/guide/guide_tables_ext-natural.svg)

On a console that can only render ASCII, the indicators are `^` and `v`, and
the core table switches to an ASCII box:

```rust
--8<-- "crates/rich-ext/examples/guide_tables_ext.rs:ascii"
```

![ASCII indicators and box](../../media/guide/guide_tables_ext-ascii.svg)

## Grouping and aggregates

`GroupBy::new(column)` splits the rows into groups by the value in that
column. Each group renders as three parts:

1. A header row. It holds the group's key in the first column, styled
   `table.group`. When the grouped column is not the first column, the
   column's name is prefixed (`region: eu`). Empty cells form one group,
   shown as `(empty)`.
2. The group's rows.
3. A summary row with the group's aggregates, styled `table.aggregate`. This
   row appears only when the grouping has aggregates.

Groups appear in the order of their first row. To get groups in key order,
sort by the grouped column first.

```rust
--8<-- "crates/rich-ext/examples/guide_tables_ext.rs:group"
```

![Grouped by region with subtotals and a totals row](../../media/guide/guide_tables_ext-group.svg)

| Aggregate | Result |
|---|---|
| `Aggregate::count(col)` | The number of non-empty cells, shown as a plain integer |
| `Aggregate::sum(col)` | The sum of the numeric cells: an `Int` if every cell is an integer, otherwise a `Float` |
| `Aggregate::min(col)`, `max(col)` | The smallest or largest non-empty cell, in natural order |
| `Aggregate::mean(col)` | The mean of the numeric cells |
| `Aggregate::custom(col, f)` | `f(&[&Value]) -> Value` over the group's cells |

Summary and totals rows:

- Each aggregate is placed in its own column. If a column has more than one
  aggregate, they are joined with `, `.
- The summary label (`subtotal` by default, set with `GroupBy::label`) goes
  in the first cell. If the first column has its own aggregate, the label is
  prefixed to it (`all: 6`).
- `TableData::totals(label, aggregates)` adds one final row computed over
  every row.

To compute groups without rendering them, call
`GroupBy::groups(&rows, &order)`. It returns each group's key, its row
indices and its aggregate values.

### Plain rows and a core `Table`

`sort::sort_rows`, `sort::sorted_indices` and `sort::compare_rows` work on any
rows that are `AsRef<[Value]>`, such as `Vec<Vec<Value>>`. They let you keep
building the core `Table` yourself:

```rust
use rich::Table;
use rich_ext::table::{sort::sort_rows, SortKey, Value};

let mut rows = vec![
    vec![Value::from("v1.10"), Value::Int(2)],
    vec![Value::from("v1.9"), Value::Int(5)],
];
sort_rows(&mut rows, &[SortKey::desc(1), SortKey::asc(0)]);
let mut table = Table::new();
table.add_column("version").add_column("n");
for row in &rows {
    table.add_row_text(row.iter().map(Value::to_text).collect());
}
```

## Streaming tables

`StreamingTable<K>` keeps its rows under stable keys of type `K`, such as a
job name, a path or a sequence number:

| Method | Effect |
|---|---|
| `upsert(key, row)` | Adds a new key at the end. For an existing key, replaces the row in place, keeping its position |
| `update_cell(&key, col, value)` | Changes one cell |
| `remove(&key)` | Removes the row and returns its cells |
| `set_sort(keys)` | Shows the rows sorted (stable over insertion order), with indicators |
| `window(Window::Tail(n))` | Shows the last `n` rows, below an `… N earlier rows` line |
| `window(Window::Head(n))` | Shows the first `n` rows, above an `… N more rows` line |
| `capacity(n)` | Evicts the oldest rows beyond `n`. Evicted rows count as earlier rows |

```rust
--8<-- "crates/rich-ext/examples/guide_tables_ext.rs:stream"
```

![A tail window over a pipeline](../../media/guide/guide_tables_ext-stream.svg)

`StreamingTable` implements `Renderable`, so you can put it anywhere a
renderable goes, including a `LiveCoordinator` region (see
[Live and layout](live-and-layout.md#coordinated-live-regions)):

```rust
--8<-- "crates/rich-ext/examples/guide_tables_ext.rs:live"
```

```rust
--8<-- "crates/rich-ext/examples/guide_tables_ext.rs:live-run"
```

### What a frame renders again

Each row caches three things: its formatted cells, their widths and its
rendered lines. Rendering a frame works like this:

- Only rows written since the last frame are formatted again.
- If no column's width changed, only those rows are laid out again. The rest
  are copied from the cache.
- If a column's width did change, every visible row is laid out again. That
  happens when a new row widens a column, when the widest row is removed, or
  when the available width changes.
- Writing a value equal to the current one is not a change. `Value::Text`
  cells are the exception: they always count as changed, because their base
  style cannot be compared.
- Sorting only reorders cached rows. A change to the window only drops or adds
  rows.

`stats()` returns counters for this (`frames`, `rows_prepared`,
`rows_rendered`, `relayouts`), so tests can assert what a frame re-rendered.
The streamed output is byte for byte what `to_table()` (a core `Table` of the
same rows, built from scratch) renders, apart from the window's indicator
line. The test suite checks that equality after every step of a mixed
workload.

How this stays exact: the core sizes a column from the widest cell in it and
nothing else. Each changed row is rendered by a core table that holds the row
plus one extra row made of each column's widest cell. So the row gets exactly
the widths the full table would give it.

Gotchas:

- Cached lines keep the styles they were rendered with. If you render the
  same table on consoles with different themes, call `invalidate()` between
  them.
- Row separators (`show_lines`) are not supported. A frame can have a title,
  a caption, a box style, and `show_edge`, `expand` and `border_style`.
- `to_data()` takes a snapshot of the rows in display order as a `TableData`,
  for grouping and aggregates.

## Virtualised tables

`VirtualTable<S>` renders the rows `offset..offset + height` of a source and
fetches nothing else, so a million-row source costs one window of memory.
The source implements `VirtualRows`:

| Method | Required | What it does |
|---|---|---|
| `row(index)` | yes | The row at a 0-based index, or `None` past the end |
| `row_count()` | no | The number of rows, when the source knows |
| `rows(start, len)` | no | A range at once, for sources that fetch ranges cheaply (the default calls `row`) |

`Vec<Vec<Value>>`, slices of rows, references, `Box` and `Arc` implement it,
and `FnRows` wraps a closure:

```rust
use rich::{Console, Justify};
use rich_ext::table::{Column, FnRows, Value, VirtualTable};

// Ten million rows, computed on demand.
let rows = FnRows::new(Some(10_000_000), |i| {
    Some(vec![Value::Int(i as i64 + 1), format!("item {i}").into()])
});
let table = VirtualTable::new(
    [Column::new("id").justify(Justify::Right), Column::new("name")],
    rows,
)
.widths([8, 10])
.offset(1_000)
.height(3);
Console::new().print(&table);
```

```text
┏━━━━━━━━━━┳━━━━━━━━━━━━┓
┃       id ┃ name       ┃
┡━━━━━━━━━━╇━━━━━━━━━━━━┩
│     1001 │ item 1000  │
│     1002 │ item 1001  │
│     1003 │ item 1002  │
└──────────┴────────────┘
rows 1,001–1,003 of 10,000,000
```

### Column widths

Widths don't follow the window, so columns never jump while you scroll. Each
column's width comes from the first of these that is set:

1. `widths([…])`, in column order (`None` skips a column).
2. The column's own `ColumnOptions::width`.
3. A sample: the header and the first `sample(n)` rows (default 100, at most
   10,000), capped at `max_column_width` (default 40). The sample is taken
   once and cached; `resample()` takes it again.

Cells don't wrap, so a row is one line and longer text ends in `…`.
`wrap(true)` wraps instead. `fit_window(true)` also widens sampled columns
to fit the rows shown, for a window rendered once rather than scrolled.

### The position line

Under the table, a `table.position` line says where the window is:

| Source | Line |
|---|---|
| Knows its count | `rows 1,001–1,040 of 1,000,000` |
| Can't count, more rows follow | `rows 1–40 of 41+` |
| Can't count, end reached | `rows 961–1,000 of 1,000` |
| Empty | `no rows` |

An offset past the end of a counted source shows the last full window.
`show_position(false)` hides the line, and `footnote(text)` adds one more
line below it. `row_numbers(true)` adds a `#` column, and
`null_marker("NULL")` shows null cells as a dim italic `NULL` instead of
empty, the way an empty string shows.

### In a viewport

`VirtualTable` is a static renderer. Interactive scrolling belongs to
`rs-rich-interact`'s viewport, which drives the window through these methods:

| Method | Effect |
|---|---|
| `set_offset(n)` / `scroll_by(±n)` | Moves the window. `scroll_by` stops at the top and at the last full window |
| `set_height(n)` | Shows `n` rows (at most 10,000) |
| `max_offset()` | The offset of the last full window, when the count is known |
| `page()` | The rows the window shows, its start, the total and whether more follow |
| `chrome_lines(console, width)` | The lines around the rows: title, edges, header, position and footnote |

A viewport of `lines` lines holds `lines - chrome_lines(…)` rows:

```rust
use rich::Console;
use rich_ext::table::{Column, FnRows, Value, VirtualTable};

let rows = FnRows::new(Some(1_000), |i| Some(vec![Value::from(i)]));
let mut table = VirtualTable::new([Column::new("i")], rows);
let console = Console::builder().width(30).build();
table.set_height(12 - table.chrome_lines(&console, 30));
assert_eq!(console.render_export(&table).lines().count(), 12);
```

### Rows from data files

With `rs-rich-data`, `Rows` implements `VirtualRows`. A forward-only
`RowSource` (a CSV or JSON Lines reader, an Arrow batch stream) can't fetch
by index, so `rich_data::window::RowWindow` reads it once. It keeps the
window, the first rows (for the width sample) and the count:

```rust
use rich_data::window::RowWindow;

let window = RowWindow::reader()
    .offset(50_000)
    .len(40)
    .count(true) // read to the end for the total (the default)
    .read(source)?;
console.print(&window.to_virtual_table());
```

With `count(false)`, reading stops one row past the window, and the position
line reads `of 50,041+`.

## SQL result sets

`rich_data::sql::ResultSet` shows a query result the way a database shell
does. It renders through `VirtualTable` and needs no database connection:
rows come from `Rows`, a `RowWindow` of any adapter, or any `VirtualRows`
(`ResultSet::from_parts(columns, schema, source)`).

- **Typed alignment.** Each column's type comes from the schema (by name,
  else by position). Integers, floats and decimals are right-justified,
  booleans centred, and text, dates and timestamps left. A column without a
  type is right-justified when every sampled non-null cell is a number.
- **NULL.** A null cell reads `NULL` in `table.null` (dim italic). An empty
  string stays empty. `null_marker` changes the text.
- **The row count.** `(3 rows)`, `(1 row)`, or `(1,001+ rows)` when the source
  can't count, with the elapsed time if you give one: `(3 rows, 12ms)`.
- **Large results.** `limit` (default 1,000, at most 10,000) and `offset`
  choose the window. A windowed result shows its position line above the
  count.

```rust
use std::time::Duration;

use rich_data::sql::ResultSet;
use rich_data::{DataType, Field, Rows, Schema, Value};

let schema = Schema::new([
    Field::new("id", DataType::Integer),
    Field::new("name", DataType::String),
    Field::new("active", DataType::Boolean),
    Field::new("balance", DataType::decimal()),
]);
let mut rows = Rows::new(["id", "name", "active", "balance"]).with_schema(schema);
rows.push([Value::Int(1), "ada".into(), "true".into(), Value::Float(12.5)]);
rows.push([Value::Int(2), "".into(), "false".into(), Value::Null]);
rows.push([Value::Int(10), Value::Null, "true".into(), Value::Float(-3.0)]);
console.print(&ResultSet::new(rows).elapsed(Duration::from_millis(12)));
```

```text
┏━━━━┳━━━━━━┳━━━━━━━━┳━━━━━━━━━┓
┃ id ┃ name ┃ active ┃ balance ┃
┡━━━━╇━━━━━━╇━━━━━━━━╇━━━━━━━━━┩
│  1 │ ada  │  true  │    12.5 │
│  2 │      │ false  │    NULL │
│ 10 │ NULL │  true  │      -3 │
└────┴──────┴────────┴─────────┘
(3 rows, 12ms)
```

For a file, `ResultSet::read(source, offset, limit)` reads one pass of any
`RowSource`. Run `infer::Inferrer` over CSV rows first so their columns get
types.

## Styles

| Key | Default | Used for |
|---|---|---|
| `table.group` | `bold` | Group header rows |
| `table.aggregate` | `italic` | Summary and totals rows |
| `table.sort_indicator` | `cyan` | `▲`/`▼` in headers |
| `table.more` | `dim` | The window's `… N more rows` line |
| `table.position` | `dim` | A virtualised table's position line and footnote |
| `table.null` | `dim italic` | The `NULL` marker |
| `table.row_number` | `dim` | The `#` column of a virtualised table |

These keys are listed in `table::STYLES` and included in `extended_theme()`.
If a theme lacks a key, the renderers fall back to the default in this table.
The names don't clash with upstream's `table.header`, `table.footer`,
`table.cell`, `table.title` and `table.caption`. Everything reads the same
without colour.

## Accessibility

`TableData`, `StreamingTable` and `VirtualTable` implement
`a11y::AccessibleText` through their core `Table` (for `VirtualTable`, the
rows of the window). The result is the same `Table with N rows, columns: …`
summary that a core table produces.
