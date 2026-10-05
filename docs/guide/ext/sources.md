# Dependency graphs and JSON Schemas

Two sources the terminal is often asked about, drawn as trees (0.0.15):
Cargo's resolved dependency graph and JSON Schemas. Both live in `rs-rich-ext`
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
