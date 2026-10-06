# Developer views

Four views added in 0.0.16 show the things a developer reads every day:
a file's merge conflicts, one record of a data set, a crate's supply chain,
and what changed between two schemas. Each is a renderable in a library
crate and a flag of an existing `rich` command, so it reads the same in a
program, a script and a terminal. This page says what each is for and links
to its detailed section.

| View | Library | Command | Detail |
|---|---|---|---|
| Merge conflicts | `rich_ext::diff::ConflictView` | `rich diff --conflicts FILE` | [Merge conflicts](../ext/diffs-and-test-reports.md#merge-conflicts) |
| One record | `rich_ext::data::RecordView` | | [The record inspector](../ext/structured-data.md#the-record-inspector) |
| The supply chain | `rich_ext::deps::{duplicates, features, timings, audit, licenses}` | `rich deps --duplicates`, `--features`, `--timings`, `--audit`, `--licenses` | [Supply-chain reports](../ext/sources.md#supply-chain-reports) |
| Schema changes | `rich_ext::schema::{SchemaDiff, SchemaTimeline}` | `rich schema OLD NEW` | [Schemas](schemas.md) |

None of them needs colour to be read: markers (`<` `|` `>` for conflict
sides, `+` `-` `~` for changes) and words (`breaking`, `FAIL`, a severity)
carry the meaning, and every style comes from a theme key a theme can
override.

## Merge conflicts

`ConflictFile::parse` reads the markers `git merge` leaves in a file,
including diff3 bases and the longer markers of nested conflicts and
`conflict-marker-size`, and `ConflictView` shows each conflict numbered,
with ours, base and theirs side by side or stacked, syntax highlighted, and
a few lines of context. Line numbers are the file's, so they lead back to
the markers. Malformed markers are an error naming the line.

```rust
use rich_ext::diff::{ConflictLayout, ConflictView};

let view = ConflictView::parse(&std::fs::read_to_string("src/config.rs")?)?
    .path("src/config.rs")
    .layout(ConflictLayout::Auto);
console.print(&view);
```

```bash
git diff --name-only --diff-filter=U | xargs -n1 rich diff --conflicts
```

## The record inspector

`RecordView` shows one record (a parsed JSON, YAML or TOML document, or any
`serde` value) as a `field | type | value` table, with nested values as
trees opened a few levels deep, long strings and containers cut, and
`expand(path)` / `collapse(path)` to open or fold a branch by path. Redact
before display with `redact(…)`. It is the static half of
`rich_interact`'s data explorer: both implement `OpenBranch`.

## The supply chain

Five reports beside `rich deps`' dependency tree read what Cargo already
wrote, and never run a scanner or reach the network:

- **Duplicates**: the crates in the graph at more than one version, the
  branches that pull each in, and a consolidation summary
  ([detail](../ext/sources.md#consolidating-duplicates)).
- **Features**: each crate's enabled features, what they turn on and who
  asked for them ([detail](../ext/sources.md#feature-trees)).
- **Build times**: the slowest units of a `cargo build --timings` report as
  bars and a table ([detail](../ext/sources.md#build-times)).
- **Advisories**: `cargo audit --json` output grouped by severity
  ([detail](../ext/sources.md#advisories)).
- **Licences**: crates grouped by licence, with copyleft, unknown and
  missing licences marked ([detail](../ext/sources.md#licences)).

## Schema changes

`SchemaDiff` lists what changed between two schemas (JSON Schema, SQL DDL
or Arrow, through one model), marking `breaking` the changes that may
refuse data the old schema accepted; `SchemaTimeline` lays several versions
side by side. The [schema guide](schemas.md) covers both, with the tree and
ER views.

## From Python

`ConflictView` and `RecordView` are in
[`rs_rich.data`](https://buchochelliq-labs.github.io/rs-rich-cli/python/data/#conflicts-and-records)
(and in `rs_rich.ext.diff` and `rs_rich.ext.data`), with the schema views.
The Cargo reports are the `rich deps` command, which `python -m rs_rich`
runs.
