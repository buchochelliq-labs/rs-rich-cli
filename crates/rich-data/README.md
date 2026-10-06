# rs-rich-data

Tabular data for [rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli),
the Rust port of Python's `rich`: one row source contract with adapters
behind it, so a file, a stream of records or a query result becomes a table
without hand-written glue. This crate is an rs-rich addition, not a port:
`rich` reads no data files. It builds on
[`rs-rich-ext`](https://crates.io/crates/rs-rich-ext)'s public API.

- **Row sources.** `RowSource` yields column names, an optional schema (in
  `rich_ext::schema`'s format-neutral model) and rows of
  `rich_ext::table::Value`, one at a time. `Rows` holds them in memory and
  becomes a `rich_ext::table::TableData` to sort, group, total or style.
- **Adapters.** CSV and TSV through the port of Python's `csv` sniffer and
  reader that `rich --csv` uses, JSON Lines, any `serde::Serialize` rows,
  and Arrow `RecordBatch`es behind the off-by-default `arrow` feature.
- **Type inference**, on request: integers, floats, booleans, dates,
  timestamps, nulls and text per column, with the evidence (how many cells
  parsed as each type) and per-column overrides.
- **Column statistics**: count, nulls, distinct values, min, max, mean,
  median, quantiles and the most common values, as a table or as a summary
  under each column heading.

```rust
use rich::Console;
use rich_data::csv::CsvReader;
use rich_data::infer::Inferrer;
use rich_data::stats::Stats;

let mut rows = CsvReader::new()
    .read("service,p99,up\nweb,120,true\napi,35.5,false\ndb,8,true\n")
    .unwrap();
let inference = Inferrer::new().infer(&rows);
inference.apply(&mut rows); // p99 becomes numbers; `--csv` never does this

let console = Console::new();
console.print(&inference); // column, type, how many cells parsed, nulls
console.print(&rows.to_table_data());
console.print(&Stats::of(&rows));
```

Conditional styles for these tables (rules that style a cell, row or
column by value, from Rust predicates or TOML rule tables) are
`rich_ext::table::rules`.

## Features

| Feature | Default | What it adds |
|---|---|---|
| `arrow` | no | `rich_data::arrow`: `RecordBatch` rows, a streaming `BatchSource`, Arrow schemas in the model, and the schema explorer (`tree`, `diff`) (`arrow-array` and `arrow-schema`) |
| `er` | no | `rich_data::er`: ER diagrams of a schema or of SQL DDL (`model`, `from_sql`), through `rs-rich-diagram` |

## Limits

JSON Lines refuses a line longer than 64 MiB, and every adapter describes
nested values to 32 levels. Streaming sources choose their columns from
the first 1,000 records (configurable) and report keys seen later instead
of growing new columns.

Independent SemVer from 0.0.1; see the repository's `AGENTS.md`.
