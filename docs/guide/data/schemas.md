# Schemas

Schemas draw, compare and evolve the same way whatever format they come
from. Every schema view in rs-rich reads one format-neutral model,
`rich_ext::schema::Schema`; readers map JSON Schema, SQL DDL and Arrow into
it, and the views (a tree, a diff, a timeline and an ER diagram) read only
the model. This page is the map; the details, with full outputs, are in the
[schema sections of the sources page](../ext/sources.md#json-schema).

| Want | Use | Detail |
|---|---|---|
| Read a JSON Schema | `schema::json::to_model`, or `SchemaTree::new` directly | [JSON Schema](../ext/sources.md#json-schema) |
| Read SQL DDL | `schema::sql::parse` | [SQL DDL](../ext/sources.md#sql-ddl) |
| Read an Arrow schema | `rich_data::arrow::schema` (`arrow` feature) | [Arrow schemas](../ext/sources.md#arrow-schemas) |
| Draw it as a tree | `SchemaTree` | [JSON Schema](../ext/sources.md#json-schema), [The schema model](../ext/sources.md#the-schema-model) |
| See what changed | `SchemaDiff::new` (JSON Schema), `SchemaDiff::models` (anything) | [What changed](../ext/sources.md#what-changed), [Comparing any two schemas](../ext/sources.md#comparing-any-two-schemas) |
| See how it evolved | `SchemaTimeline` | [Schema evolution](../ext/sources.md#schema-evolution) |
| Draw tables and keys | `rich_data::er` (`er` feature) | [ER diagrams](../ext/sources.md#er-diagrams) |

The schema views are in `rs-rich-ext` behind its `data` feature; Arrow and
ER diagrams are in `rs-rich-data` behind its `arrow` and `er` features. The
`rich schema` command draws them from the command line (see the
[CLI guide](../../cli.md)).

## The model

A `Schema` is a list of `Field`s, optionally named and described, and, for
a SQL file, further named tables, each a `Schema` of its columns. A `Field`
has:

- a model type (`DataType`): `integer`, `float`, `decimal(10,2)`, `string`,
  `boolean`, `date`, `timestamp[UTC]`, `binary`, `list<…>`, `struct`,
  `map<…, …>`, `any` or `unknown`;
- the source's own type name as written (`VARCHAR(255)`, `Int32`,
  `string | null`), which the tree shows and diffs compare;
- whether it may be null and whether a value must be present;
- constraints (`Constraint`): enum values, bounds, length, pattern, format,
  a default, primary and unique keys, and a `ForeignKey` to `table.column`;
- a description and metadata.

A key across several columns is the table's own constraint
(`Schema::primary_key`, `Schema::foreign_keys`). `rich_data`'s row sources
carry the same model, and `infer::Inference::schema` builds one from rows,
so a CSV file's inferred columns draw like any other schema.

```rust
use rich_ext::schema::{DataType, Field, Schema};

let users = Schema::new([
    Field::new("id", DataType::Integer).required(true),
    Field::new("email", DataType::String).with_native_type("VARCHAR(320)"),
])
.named("users");
```

## Readers

- **JSON Schema.** `schema::json::to_model` keeps JSON Schema's structure
  (pattern-named properties, tuple items, `oneOf` branches, `$ref`s) with
  each child's `FieldKind`. `SchemaTree::new` and `SchemaDiff::new` read a
  JSON Schema directly, following `$ref`s, and draw byte for byte what they
  drew before the model existed.
- **SQL DDL.** `schema::sql::parse` reads a `CREATE TABLE` subset: types,
  `NOT NULL`, `DEFAULT`, `PRIMARY KEY`, `UNIQUE`, `REFERENCES`, `CONSTRAINT`,
  `ENUM` and `COMMENT`. What it skips (`CHECK`, indexes, other statements)
  comes back as notes with their lines; what it cannot read is an error with
  its line. Input is bounded (16 MiB, 100,000 statements).
- **Arrow.** `rich_data::arrow::schema` maps nested structs, lists, maps,
  dictionaries, timestamps with unit and zone, decimals, nullability and
  metadata.

## Views

```rust
use rich::Console;
use rich_ext::schema::{sql, SchemaDiff, SchemaTimeline, SchemaTree};

let v1 = sql::parse(V1)?.schema;
let v2 = sql::parse(V2)?.schema;
let console = Console::new();
console.print(&SchemaTree::from_model(v2.clone()).title("shop"));
console.print(&SchemaDiff::models(&v1, &v2).names("v1", "v2"));
console.print(&SchemaTimeline::new().push("v1", v1).push("v2", v2));
```

- **`SchemaTree`** draws each field with its type, `(required)`, its
  constraints and the first line of its description; a schema with tables
  draws each table under the root.
- **`SchemaDiff`** lists fields added (`+`), removed (`-`) and changed
  (`~`): type changes, required, enum values, constraints and keys, marking
  `breaking` where data the old schema accepted may now be refused. The
  rule is the same for every format.
- **`SchemaTimeline`** lays a series of versions on a `chart::Timeline`: a
  row per field spanning the versions it is in, a milestone per version
  with its counts, and the changes listed under it.
- **ER diagrams** (`rich_data::er`): a box per table with its columns,
  types and keys (`PK`, `FK`, `UQ`, `?` for nullable), and an edge per
  foreign key with its cardinality, laid out by `rs-rich-diagram`.
  `er::model` takes any `Schema`; `er::from_sql` reads DDL and returns the
  diagram with the reader's notes.

```rust
use rich_data::er;

let (diagram, notes) = er::from_sql(DDL)?;
console.print(&diagram);
```

None of the views reads colour alone: markers and words carry the meaning,
and the styles come from the `schema.*` theme keys (`schema::STYLES`).

## From Python

[`rs_rich.data`](https://buchochelliq-labs.github.io/rs-rich-cli/python/data/#schemas)
has the same views: `Schema.from_json_schema`, `Schema.from_sql`,
`SchemaTree`, `SchemaDiff`, `SchemaTimeline` and `ErDiagram`. Each takes a
`Schema`, or a JSON Schema as a `dict` or JSON text.
