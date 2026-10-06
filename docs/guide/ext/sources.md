# Dependency graphs and JSON Schemas

Two sources the terminal is often asked about, drawn as trees (0.0.15):
Cargo's resolved dependency graph and JSON Schemas (and, since 0.0.16, SQL
DDL and Arrow schemas through [the schema model](#the-schema-model)); beside the graph, the
[supply-chain reports](#supply-chain-reports) (0.0.16) read feature,
build-time, advisory and licence data Cargo and its tools already wrote. Both live in `rs-rich-ext`
behind its `data` feature, as `rich_ext::deps` and `rich_ext::schema`; the
`rich` CLI shows them with `rich deps` and `rich schema`. Neither reads
colour alone: markers and words carry the meaning, and colour repeats it.

The examples below use the fixtures in `crates/rich-ext/tests/fixtures/sources`,
which CI renders on every change. [A recording](../../recordings.md#dependency-trees)
shows `rich deps` on the same fixture in a terminal.

## Cargo dependency graphs

`DepGraph::from_json` reads the output of `cargo metadata --format-version 1`:
every package, the workspace members, and which depend on which (normal,
build and dev). Running Cargo is the caller's business; the library only
reads its output.

```rust
use rich::Console;
use rich_ext::deps::{DepGraph, DepTree, WhyTree};

let json = std::fs::read_to_string("metadata.json").unwrap();
let graph = DepGraph::from_json(&json).unwrap();
let console = Console::new();
console.print(&DepTree::new(graph.clone()).summary(true));
console.print(&WhyTree::new(graph, "syn@1.0.109").unwrap());
```

`DepTree` prints what `cargo tree` prints: each root (the package
`cargo metadata` ran in, else every workspace member) and what it depends on.
A package already shown is marked `(*)` instead of repeated, build and dev
dependencies sit under `[build-dependencies]` and `[dev-dependencies]`, and a
crate resolved at more than one version is marked `(duplicate)` and coloured.
Dev dependencies are a root's only, as Cargo resolves them; a root is
expanded in full at the top level (its dev dependencies included) even when
it was already shown as another root's dependency, so `demo-core` above is
listed again with what it uses. `max_depth`, `kinds` and `duplicates_only`
trim it; `summary(true)` lists the duplicates underneath, counting only the
versions reachable through the kinds followed. A branch deeper than 128
levels (`deps::MAX_TREE_DEPTH`, far past any real graph) ends in a
`… (deeper levels not shown …)` note.

```bash
rich deps                         # runs cargo metadata in the working directory
rich deps path/to/Cargo.toml      # ... or for that manifest (or its directory)
rich deps --metadata meta.json    # ... or reads saved output (`-` for stdin)
```

```text
demo-app v0.3.0
├── demo-core v0.3.0
│   ├── serde v1.0.210
│   │   └── serde_derive v1.0.210
│   │       ├── proc-macro2 v1.0.86
│   │       │   └── unicode-ident v1.0.13
│   │       ├── quote v1.0.37
│   │       │   └── proc-macro2 v1.0.86 (*)
│   │       └── syn v2.0.79 (duplicate)
│   │           ├── proc-macro2 v1.0.86 (*)
│   │           ├── quote v1.0.37 (*)
│   │           └── unicode-ident v1.0.13
│   ├── strum_macros v0.25.3
│   │   ├── proc-macro2 v1.0.86 (*)
│   │   ├── quote v1.0.37 (*)
│   │   └── syn v1.0.109 (duplicate)
│   │       ├── proc-macro2 v1.0.86 (*)
│   │       ├── quote v1.0.37 (*)
│   │       └── unicode-ident v1.0.13
│   └── thiserror v1.0.64
│       └── thiserror-impl v1.0.64
│           ├── proc-macro2 v1.0.86 (*)
│           ├── quote v1.0.37 (*)
│           └── syn v2.0.79 (duplicate) (*)
├── log v0.4.22
├── serde v1.0.210 (*)
├── [build-dependencies]
│   └── cc v1.1.28
└── [dev-dependencies]
    └── insta v1.40.0
        ├── serde v1.0.210 (*)
        └── similar v2.6.0
demo-core v0.3.0
├── serde v1.0.210 (*)
├── strum_macros v0.25.3 (*)
└── thiserror v1.0.64 (*)
duplicate: syn v1.0.109, v2.0.79
```

`--depth N` keeps N levels below each root, `--no-dev` leaves dev
dependencies out, and `--duplicates` keeps only the branches that lead to a
duplicated crate:

```text
demo-app v0.3.0
├── demo-core v0.3.0
│   ├── serde v1.0.210
│   │   └── serde_derive v1.0.210
│   │       └── syn v2.0.79 (duplicate)
│   ├── strum_macros v0.25.3
│   │   └── syn v1.0.109 (duplicate)
│   └── thiserror v1.0.64
│       └── thiserror-impl v1.0.64
│           └── syn v2.0.79 (duplicate)
├── serde v1.0.210 (*)
└── [dev-dependencies]
    └── insta v1.40.0
        └── serde v1.0.210 (*)
demo-core v0.3.0
├── serde v1.0.210 (*)
├── strum_macros v0.25.3 (*)
└── thiserror v1.0.64 (*)
duplicate: syn v1.0.109, v2.0.79
```

### Running Cargo

Without `--metadata`, `rich deps` runs `cargo metadata --format-version 1`
(`$CARGO`, else `cargo` on the `PATH`) in the working directory. Cargo runs
there with **that directory's Cargo configuration**: every
`.cargo/config.toml` from it up to the root, and the `rust-toolchain.toml`
rustup honours. `cargo metadata` compiles nothing, but it does run `rustc`
to learn the target, and it may fetch the registry index and git
dependencies as Cargo always does (`rich deps` does not pass `--offline` or
`--locked`; Cargo's own settings and `CARGO_NET_OFFLINE` apply).

A configuration can name programs. `rich deps` turns off the ones `cargo
metadata` would run without needing them: it sets `RUSTC_WRAPPER`,
`CARGO_BUILD_RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER` and
`CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER` to empty for Cargo, which overrides a
`build.rustc-wrapper` or `build.rustc-workspace-wrapper` in the project's
configuration. What it cannot turn off is the rest of Cargo's behaviour: a
`build.rustc` naming another program still runs as the compiler, and a
`rust-toolchain.toml` still selects (and rustup may install) a toolchain.
Note that Cargo reads the configuration of the directory it is started in,
not the manifest's: `rich deps path/to/Cargo.toml` uses the working
directory's configuration (and that of the directories above it).

So in a checkout you do not trust, do not run `rich deps` there. Make the
metadata where running Cargo is safe (`cargo metadata --format-version 1 >
meta.json`, in a sandbox or a container) and read it with `rich deps
--metadata meta.json`, which runs nothing.

### What pulls a crate in

`--why CRATE` (`name`, or `name@version`) prints `WhyTree`: the crate and
everything that depends on it, up to the workspace, which is every path that
pulls it in (what `cargo tree -i` prints). A dependent that uses it only as a
build or dev dependency is marked `[build]` or `[dev]`. `DepGraph::paths_to`
lists the paths one by one, shortest first.

```bash
rich deps --why syn@1.0.109
```

```text
syn v1.0.109 (duplicate)
└── strum_macros v0.25.3
    └── demo-core v0.3.0
```

With `--no-dev`, a crate only dev dependencies pull in is not reached, and
`rich deps --why` says so and exits 4 rather than print nothing:

```text
rich: insta is reached only through dev-dependencies (drop --no-dev)
```

### As a diagram

`--graph` draws the same through the [diagram layout](../diagram/index.md)
instead: dependencies left to right, or for `--why` from the workspace down to
the crate. Workspace members are rounded, duplicated crates are hexagons, and
build and dev edges are dotted and labelled. A large graph is wider than the
terminal; `--depth` and `-w` help, and the drawing is cropped, never wrapped.

```bash
rich deps --why syn@1.0.109 --graph
```

```text
  ╭───────────────────╮
  │ demo-core v0.3.0  │
  ╰─────────┬─────────╯
            │
            ▼
┌───────────────────────┐
│ strum_macros v0.25.3  │
└───────────┬───────────┘
            │
            ▼
    ╱───────────────╲
    │ syn v1.0.109  │
    ╲───────────────╱
```

## Supply-chain reports

Five reports sit beside the tree (0.0.16), in `rich_ext::deps`'s submodules
`duplicates`, `features`, `timings`, `audit` and `licenses`. Each reads what
Cargo or its tools already wrote: `cargo metadata`, a `cargo build
--timings` report or `cargo audit --json` output. None runs a scanner or
reaches the network, and `--timings` and `--audit` do not run Cargo at all.
The examples use the fixtures in `crates/rich-ext/tests/fixtures/supply-chain`.

| Flag | Reads | Shows |
|---|---|---|
| `--duplicates` | `cargo metadata` | the duplicated branches, then a consolidation summary |
| `--features [--package CRATE]` | `cargo metadata` | each crate's enabled features, what they turn on, who asked |
| `--licenses` | `cargo metadata` | crates grouped by licence; copyleft, unknown and missing marked |
| `--timings FILE` | `cargo build --timings` | the slowest units as bars and a table |
| `--audit FILE` | `cargo audit --json` | advisories grouped by severity; exit 5 on a vulnerability |

Give one report at a time. An option that does not apply to it (`--graph`,
`--depth` or `--duplicates` with any of them, `--no-dev` with anything but
`--licenses`, `--metadata` or a manifest with `--timings` and `--audit`) is
refused with exit 2 rather than ignored.

The parsers are bounded: `--timings` and `--audit` read at most 64 MiB
(`deps::MAX_INPUT`), every reader takes at most 100,000 packages, units or
advisories (`deps::MAX_RECORDS`), JSON nests at most 128 levels (serde_json's
limit), and a licence expression is at most 1,024 bytes and 32 parentheses
deep. Past a limit, or on malformed input, the reader returns an error, and
`rich deps` exits 4 naming it.

### Consolidating duplicates

`--duplicates` keeps only the branches that lead to a duplicated crate, as
before, and now adds `Consolidation` under the tree: for each crate resolved
at more than one version, every version with what depends on it, and the
version most dependents already use (the newest by SemVer, on a tie). It does
not say the others could move onto it: that depends on their version
requirements, which the resolved graph does not record. `DepGraph::consolidation(kinds)` returns the same as data
(`Duplicate::shared_version`, `Duplicate::to_move`), and
`DepTree::consolidation(true)` adds it under a tree. Without `--duplicates`,
`rich deps` prints what it always did.

```text
duplicate: syn v1.0.109, v2.0.79

consolidation: 1 crate at several versions
syn: 2 versions, 3 dependents; 2 use v2.0.79 (newest)
├── v2.0.79 ← serde_derive v1.0.210, thiserror-impl v1.0.64
└── v1.0.109 ← strum_macros v0.25.3
```

### Feature trees

`--features` reads the features Cargo resolved (`resolve.nodes[].features`,
unified across the build), each package's `[features]` table and the
features its dependents ask for. `FeatureTree` draws a crate: each enabled
feature with its definition underneath (another feature, an optional
dependency `dep:name` and the package it resolved to, or a dependency's
feature `name/feature`, with `name?/feature` applying only when that
dependency is on for another reason), and under "requested by" each
dependent with the features its manifest names and the features of its own
that turn on more. An entry Cargo did not act on is marked `(off)`.

Without `--package`, every crate with a feature enabled is shown, the
workspace members first, and the rest are counted.

```bash
rich deps --features --package syn@2.0.79
```

```text
syn v2.0.79  5 features enabled
├── default
│   ├── derive
│   ├── parsing
│   ├── printing
│   └── proc-macro
├── derive
├── parsing
├── printing
│   └── dep:quote → quote v1.0.37
├── proc-macro
│   ├── proc-macro2/proc-macro → proc-macro2 v1.0.86
│   └── quote?/proc-macro → quote v1.0.37
└── requested by
    ├── serde_derive v1.0.210: derive, parsing, printing, proc-macro
    └── thiserror-impl v1.0.64: default features
```

### Build times

`--timings FILE` reads the report `cargo build --timings` writes to
`target/cargo-timings/cargo-timing.html` (its script holds every unit as
JSON), that `UNIT_DATA` array on its own, or the `timing-info` JSON lines
older nightly toolchains wrote with `--timings=json -Zunstable-options`.
`TimingsReport` shows the totals, the 20 slowest units as bars
(`TimingsReport::limit` changes the number) and a table that splits each
unit's time into the frontend (until its `.rmeta` was ready, which lets
dependents start) and codegen. A crate built at two versions has the version
in its bar's label. Artifact sizes are not in the report and are not shown.

```bash
cargo build --timings
rich deps --timings target/cargo-timings/cargo-timing.html
```

```text
12 units: 24.82s of compile time, 16.50s wall clock

syn v2.0.79                    ████████████████████████████████████████ 6.84
syn v1.0.109                   ██████████████████████████████           5.12
serde_derive                   ██████████████████████▉                  3.91
…

┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━┳━━━━━━━┳━━━━━━━━━━┳━━━━━━━━━┓
┃ Unit                           ┃ Version  ┃  Time ┃ Frontend ┃ Codegen ┃
┡━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━╇━━━━━━━╇━━━━━━━━━━╇━━━━━━━━━┩
│ syn                            │ v2.0.79  │ 6.84s │    3.95s │   2.89s │
│ syn                            │ v1.0.109 │ 5.12s │    2.90s │   2.22s │
…
```

### Advisories

`audit::Advisory` is a generic model of one finding: id, kind
(vulnerability, unsound, unmaintained, yanked or notice), package and the
version in use, the patched and unaffected version requirements, severity
and CVSS score, title, link, aliases and date. `AdvisoryReport::from_cargo_audit`
reads `cargo audit --json`; a plugin reading another scanner builds the same
values and passes them to `AdvisoryReport::new`. `cargo audit` gives a CVSS
vector rather than a severity: a CVSS 3.x vector is scored with the
specification's formula (`audit::cvss3_score`) and banded as CVSS does; a
vulnerability without one (or with a CVSS 4 vector) is `unknown`, and a
warning without one is `informational`.

The report lists the counts, then a table grouped by severity, most severe
first. The ID links to the advisory in a terminal that shows links.

```bash
cargo audit --json > audit.json
rich deps --audit audit.json
```

```text
2 vulnerabilities, 2 warnings in 16 dependencies
1 high · 1 unknown · 2 informational

┏━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ Severity     ┃ ID                ┃ Crate                ┃ Patched ┃ Title                        ┃
┡━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┩
│ high 7.5     │ RUSTSEC-2099-0001 │ similar v2.6.0       │ >=2.6.1 │ Unbounded recursion when     │
│              │                   │                      │         │ diffing deeply nested input  │
├──────────────┼───────────────────┼──────────────────────┼─────────┼──────────────────────────────┤
│ unknown      │ RUSTSEC-2099-0002 │ log v0.4.22          │ no fix  │ Format string handling may   │
│              │                   │                      │         │ read uninitialised memory    │
├──────────────┼───────────────────┼──────────────────────┼─────────┼──────────────────────────────┤
│ unmaintained │ RUSTSEC-2099-0003 │ strum_macros v0.25.3 │ no fix  │ strum_macros 0.25 is no      │
│              │                   │                      │         │ longer maintained            │
│ yanked       │ –                 │ cc v1.1.28           │ no fix  │ this version was yanked from │
│              │                   │                      │         │ its registry                 │
└──────────────┴───────────────────┴──────────────────────┴─────────┴──────────────────────────────┘
rich: 2 vulnerabilities found: RUSTSEC-2099-0001, RUSTSEC-2099-0002
```

**Exit code.** Like `cargo audit` itself, and like the CLI's other gates
(`diff --threshold`, `bench compare`), `rich deps --audit` exits 5 when the
report lists a vulnerability, after showing it in full; with `--report json`
the one envelope on stderr has `"code": "gate"`. Warnings alone (unmaintained,
unsound, yanked) exit 0. A file that is not `cargo audit` JSON exits 4.

### Licences

`--licenses` groups the packages the tree reaches (`--no-dev` leaves out
those only dev dependencies pull in) by their `license` expression, or notes
a `license-file` or no licence at all. `licenses::classify` reads SPDX
expressions (`AND`, `OR`, `WITH`, parentheses, and the old `MIT/Apache-2.0`
form): an `OR` is as permissive as its most permissive choice, an `AND` as
strict as its strictest part. Weak copyleft (LGPL, MPL, EPL, CDDL, …),
copyleft (GPL, AGPL, EUPL, OSL, SSPL, CC-BY-SA), unknown identifiers,
unreadable expressions, licence files and missing licences are marked; the
operands of an expression are sorted, so `MIT OR Apache-2.0` and
`Apache-2.0 OR MIT` are one group. It is a reading aid, not legal advice.

```text
16 crates under 7 licences: 1 copyleft, 1 licence file only, 1 no licence

┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━┓
┃ Licence                          ┃ Crates ┃ Which                            ┃ Note              ┃
┡━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━┩
│ Apache-2.0 OR MIT                │      9 │ cc v1.1.28, log v0.4.22,         │                   │
…
│ (licence file)                   │      1 │ demo-core v0.3.0                 │ licence file only │
│ (none)                           │      1 │ thiserror-impl v1.0.64           │ no licence        │
│ GPL-3.0-or-later                 │      1 │ demo-app v0.3.0                  │ copyleft          │
│ MIT                              │      1 │ strum_macros v0.25.3             │                   │
└──────────────────────────────────┴────────┴──────────────────────────────────┴───────────────────┘
```

## JSON Schema

`SchemaTree` draws a schema as a tree of its properties: each with its type, a
`(required)` marker, its constraints (`minLength=1`, `format=email`,
`one of "a", "b"`, `no other properties`, …) and the first line of its
description. Array items (`[items]`, or `[0]`, `[1]` for tuples),
`patternProperties` (`/pattern/`), `additionalProperties`
(`[other properties]`), `oneOf` / `anyOf` / `allOf` branches, `not` and
`if` / `then` / `else` are branches of their own.

A `$ref` within the document (`#/$defs/…`, `#/definitions/…`, any JSON
pointer, or a `$anchor`) is resolved and drawn in place, marked `→ #/$defs/…`;
one that refers back to a schema it is already inside (however the reference
is spelled: `#/%24defs/x` is `#/$defs/x`) is marked `(recursive)` instead of
drawn again, and one to another document is shown, not fetched. A definition
shared through `$ref`s is drawn wherever it is used, which a small schema can
make exponentially many places: the tree stops at 10,000 entries
(`schema::MAX_ENTRIES`) and ends with a `… (the tree stops at 10000
entries)` note.

```rust
use rich::Console;
use rich_ext::schema::SchemaTree;

let json = std::fs::read_to_string("order.schema.json").unwrap();
Console::new().print(&SchemaTree::from_json(&json).unwrap());
```

```bash
rich schema order-v1.schema.json
```

```text
Order  object  no other properties  A customer's order.
├── id (required)  string  pattern=/^ord_[a-z0-9]+$/  Order id.
├── status  enum  one of "pending", "paid", "shipped", default="pending"
├── customer  object  → #/$defs/customer
│   ├── email (required)  string  format=email
│   └── referrer  object  → #/$defs/customer (recursive)  Who referred them.
├── items (required)  array  minItems=1
│   └── [items]  object  → #/$defs/item
│       ├── sku (required)  string
│       ├── quantity (required)  integer  minimum=1
│       └── price  number  exclusiveMinimum=0
├── payment  one of
│   ├── [1] Card  object
│   │   └── last4 (required)  string  minLength=4, maxLength=4
│   └── [2] Invoice  object
│       └── due  string  format=date
└── notes  string | null  maxLength=500
```

### What changed

`SchemaDiff::new(&old, &new)` compares two versions: properties added and
removed, type changes, properties that became (or stopped being) required,
enum values, constraints and branch counts, each marked `+`, `-` or `~`.
`breaking` marks a change that may refuse a document the old version
accepted: a new requirement, a removed enum value, a narrower type, a tighter
bound, or a removed property the new version may refuse (through
`additionalProperties: false` or a schema, or a `patternProperties` schema it
matches; `true`, `{}` or no `additionalProperties` still accepts it). Keywords
beside a local `$ref` are compared along with the schema it names. Shared
definitions are compared wherever they are used, so the diff has a budget
too: it stops after 10,000 changes (`schema::MAX_CHANGES`) or 100,000
comparisons of differing schemas (`MAX_COMPARISONS`), and then says so in its
summary (`(stopped after …: more may differ)`; `SchemaDiff::is_truncated`).
`rich schema OLD NEW` prints it:

```bash
rich schema order-v1.schema.json order-v2.schema.json
```

```text
┏━━━┳━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━┓
┃   ┃ Where         ┃ Change                             ┃          ┃
┡━━━╇━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━┩
│ ~ │ currency      │ became required                    │ breaking │
│ - │ notes         │ property removed (string | null)   │ breaking │
│ + │ status        │ enum value "refunded" added        │          │
│ + │ items         │ maxItems 100 added                 │ breaking │
│ ~ │ items[].price │ type number → string               │ breaking │
│ + │ items[].price │ pattern /^[0-9]+\.[0-9]{2}$/ added │ breaking │
│ - │ items[].price │ exclusiveMinimum 0 removed         │          │
│ ~ │ payment       │ one of: 2 → 3 branches             │          │
│ + │ currency      │ property added (string)            │          │
└───┴───────────────┴────────────────────────────────────┴──────────┘
order-v1.schema.json → order-v2.schema.json: 9 changes, 5 breaking
```

Both read a file, an http(s) URL or `-` (stdin, for one of the two); a file
that is not a JSON Schema (an object, `true` or `false`) is an error, exit 4.

## The schema model

`SchemaTree` and `SchemaDiff` read a format-neutral model,
`rich_ext::schema::{Schema, Field, DataType, Constraint}` (0.0.16), so JSON
Schema, SQL DDL and Arrow schemas draw and compare the same way. A `Schema`
is a list of `Field`s, optionally named, and (for a SQL file) further named
tables. A field has a model type (`integer`, `decimal(10,2)`,
`timestamp[UTC]`, `list<…>`, `struct`, `map<…, …>`), the source's own type
name as written (`VARCHAR(255)`, `Int32`, `string | null`), which the tree
shows, whether it may be null and whether a value must be present, and its
constraints: enum values, bounds, length, pattern, format, a default, primary
and unique keys and a `ForeignKey` to `table.column`. A key across several
columns is the table's own constraint.

`schema::json::to_model` maps a JSON Schema into the model; `SchemaTree::new`
draws a JSON Schema through the same mapping, and the output above is byte
for byte what it was before the model existed. JSON Schema's own structure
(pattern-named properties, tuple items, `oneOf` branches, `$ref`s) is kept
with each child's `FieldKind` and the reference its type came through.

### SQL DDL

`schema::sql::parse` reads a small `CREATE TABLE` subset: column names and
types (quoted or not, with parameters, several words such as
`DOUBLE PRECISION`, and arrays), `NOT NULL`, `DEFAULT`, `PRIMARY KEY`,
`UNIQUE` and `REFERENCES` on a column or for several columns on the table,
`CONSTRAINT name`, MySQL's `ENUM(…)` and `COMMENT`, `--` and `/* */`
comments, and any number of statements. `CHECK` constraints, indexes and
every statement but `CREATE TABLE` are skipped, each with a note giving its
line; text it cannot read (an unclosed string or parenthesis, a table with
no column list) is an error with its line. Input is bounded: 16 MiB, 100,000
statements, 4,096 columns a table and parentheses 64 deep.

```rust
use rich::Console;
use rich_ext::schema::{sql, SchemaTree};

let parsed = sql::parse(&std::fs::read_to_string("shop.sql").unwrap()).unwrap();
for note in &parsed.notes {
    eprintln!("{note}"); // line 13: orders.total: CHECK constraint skipped
}
Console::new().print(&SchemaTree::from_model(parsed.schema).title("shop"));
```

```text
shop  2 tables
├── customers  table
│   ├── id (required)  BIGINT  primary key
│   ├── email (required)  VARCHAR(255)  unique
│   └── name  TEXT
└── orders  table
    ├── id (required)  BIGINT  primary key
    ├── customer_id (required)  BIGINT  → customers.id
    ├── status  VARCHAR(16)  default='new'
    └── total  NUMERIC(10, 2)
```

### Comparing any two schemas

`SchemaDiff::models(&old, &new)` compares two model schemas: fields by name
(nested ones by path, a list's items as `field[]`), then tables by name. It
reports the same kinds of change as the JSON Schema diff, plus keys and
metadata, with the same `breaking` rule: a new requirement (a `NOT NULL`, a
non-nullable Arrow field), a changed type, a new key or reference, a tighter
bound, a removed enum value, and a removed field or table (rows that have it
no longer fit). A changed default or metadata entry is informational. Types
are compared by the source's own name, ignoring case, so `INT → BIGINT` and
`Int32 → Int64` are changes; comparing schemas from two formats works, but
reports every type whose names differ.

```text
┏━━━┳━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━┓
┃   ┃ Where            ┃ Change                                 ┃          ┃
┡━━━╇━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━┩
│ ~ │ customers.name   │ became required                        │ breaking │
│ ~ │ customers.email  │ type VARCHAR(255) → VARCHAR(320)       │ breaking │
│ ~ │ orders.status    │ default 'new' → 'pending'              │          │
│ ~ │ orders.total     │ type NUMERIC(10, 2) → NUMERIC(12, 2)   │ breaking │
│ + │ orders.placed_at │ field added (TIMESTAMP WITH TIME ZONE) │          │
│ + │ order_lines      │ table added (3 fields)                 │          │
└───┴──────────────────┴────────────────────────────────────────┴──────────┘
v1 → v2: 6 changes, 3 breaking
```

`SchemaDiff::new` stays the way to compare two JSON Schemas: it follows
`$ref`s as it goes, so shared definitions are compared only where they
differ.

### Arrow schemas

With `rs-rich-data`'s `arrow` feature, `rich_data::arrow::schema` maps an
Arrow schema into the model: nested structs, lists and maps, dictionaries
(as their value type), timestamps with their unit and time zone, decimals
with precision and scale, nullability, and field and schema metadata.
`arrow::tree` draws it and `arrow::diff` compares two (#342):

```text
events  8 fields  version=1
├── id (required)  Int32
├── at (required)  Timestamp(µs, "Europe/Paris")
├── kind  Dictionary(Int8, Utf8)  description=kind
├── amount  Decimal128(12, 2)
├── tags  List
│   └── item  Utf8
├── address  Struct
│   ├── city (required)  Utf8
│   └── zip  Utf8
├── scores  Map
│   ├── key (required)  Utf8
│   └── value  Float64
└── debug  Boolean
```

### Schema evolution

`SchemaTimeline` lays a series of versions on a `chart::Timeline` (#347): a
row per field (`table.column` for DDL) spanning the versions it is in, a
milestone per version with its counts (`v2: +2 ~4, 3 breaking`), and, unless
`details(false)`, every change listed under it. A span takes the added,
changed or breaking style in the version its field changed. Versions are at
their index unless placed with `at`; consecutive JSON Schemas compare as
JSON Schema, anything else through the model.

```rust
use rich_ext::schema::{sql, SchemaTimeline};

let timeline = SchemaTimeline::new()
    .push("v1", sql::parse(V1).unwrap().schema)
    .push("v2", sql::parse(V2).unwrap().schema);
```

```text
customers.id         #############################==============================
customers.email      #############################==============================
customers.name       #############################==============================
orders.id            #############################==============================
orders.customer_id   #############################==============================
orders.status        #############################==============================
orders.total         #############################==============================
orders.placed_at                                  ##############################
order_lines.order_id                              ##############################
order_lines.line                                  ##############################
order_lines.sku                                   ##############################
                     * v1                         * v2: +2 ~4, 3 breaking
                     +----------------------------+----------------------------+
                     0                            1                            2
v1 → v2: 6 changes, 3 breaking
├── ~ customers.name  became required  breaking
├── ~ customers.email  type VARCHAR(255) → VARCHAR(320)  breaking
├── ~ orders.status  default 'new' → 'pending'
├── ~ orders.total  type NUMERIC(10, 2) → NUMERIC(12, 2)  breaking
├── + orders.placed_at  field added (TIMESTAMP WITH TIME ZONE)
└── + order_lines  table added (3 fields)
```

The `rich schema` command still reads JSON Schema only; DDL and Arrow input
and an ER view come to the CLI later in 0.0.16.
