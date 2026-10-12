//! Guide: Schemas — run: cargo run -p rs-rich-data --example guide_schemas --features er [-- --svg docs/media/guide]
//!
//! The snippets and pictures on `docs/guide/data/schemas.md` (and the SQL
//! sections of `docs/guide/ext/sources.md`) come from this file.

use std::error::Error;
use std::path::PathBuf;

use rich::ColorSystem;

// --8<-- [start:imports]
use rich::Console;
use rich_data::er;
use rich_ext::schema::{
    sql, Constraint, DataType, Field, Schema, SchemaDiff, SchemaTimeline, SchemaTree,
};
// --8<-- [end:imports]

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

// --8<-- [start:ddl]
const V1: &str = "
-- The shop, version 1.
CREATE TABLE customers (
    id      BIGINT PRIMARY KEY,
    email   VARCHAR(255) NOT NULL UNIQUE,
    name    TEXT
);

CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers (id),
    status      VARCHAR(16) DEFAULT 'new',
    total       NUMERIC(10, 2) CHECK (total >= 0)
);
";

const V2: &str = "
CREATE TABLE customers (
    id      BIGINT PRIMARY KEY,
    email   VARCHAR(320) NOT NULL UNIQUE,
    name    TEXT NOT NULL
);

CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES customers (id),
    status      VARCHAR(16) DEFAULT 'pending',
    total       NUMERIC(12, 2),
    placed_at   TIMESTAMP WITH TIME ZONE
);

CREATE TABLE order_lines (
    order_id BIGINT REFERENCES orders (id),
    line     INT,
    sku      TEXT NOT NULL,
    PRIMARY KEY (order_id, line)
);
";
// --8<-- [end:ddl]

// --8<-- [start:model]
fn model(console: &Console) -> Result {
    let users = Schema::new([
        Field::new("id", DataType::Integer).required(true),
        Field::new("email", DataType::String)
            .with_native_type("VARCHAR(320)")
            .with_constraint(Constraint::Format("email".into())),
        Field::new("joined", DataType::Date).with_description("First sign-in."),
    ])
    .named("users");
    console.print(&SchemaTree::from_model(users));
    Ok(())
}
// --8<-- [end:model]

// --8<-- [start:sql]
fn sql_tree(console: &Console) -> Result {
    let parsed = sql::parse(V1)?;
    for note in &parsed.notes {
        eprintln!("{note}"); // line 13: orders.total: CHECK constraint skipped
    }
    console.print(&SchemaTree::from_model(parsed.schema).title("shop"));
    Ok(())
}
// --8<-- [end:sql]

// --8<-- [start:diff]
fn diff(console: &Console) -> Result {
    let v1 = sql::parse(V1)?.schema;
    let v2 = sql::parse(V2)?.schema;
    console.print(&SchemaDiff::models(&v1, &v2).names("v1", "v2"));
    Ok(())
}
// --8<-- [end:diff]

// --8<-- [start:timeline]
fn timeline(console: &Console) -> Result {
    let timeline = SchemaTimeline::new()
        .push("v1", sql::parse(V1)?.schema)
        .push("v2", sql::parse(V2)?.schema);
    console.print(&timeline);
    Ok(())
}
// --8<-- [end:timeline]

// --8<-- [start:er]
const DDL: &str = "
CREATE TABLE customers (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE, name TEXT);
CREATE TABLE orders (
    id          INT PRIMARY KEY,
    customer_id INT NOT NULL REFERENCES customers (id),
    placed_at   TIMESTAMP NOT NULL
);
";

fn er_diagram(console: &Console) -> Result {
    let (diagram, notes) = er::from_sql(DDL)?;
    assert!(notes.is_empty());
    console.print(&diagram);
    Ok(())
}
// --8<-- [end:er]

fn main() {
    let shots = Shots::from_args("guide_schemas");
    shots.shot("model", 64, model);
    shots.shot("sql", 64, sql_tree);
    shots.shot("diff", 80, diff);
    shots.shot("timeline", 84, timeline);
    shots.shot("er", 84, er_diagram);
}

/// `--svg DIR` writes each shot as `DIR/<topic>-<shot>.svg` from a pinned
/// console; without it, shots print to the terminal.
struct Shots {
    topic: &'static str,
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args(topic: &'static str) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { topic, dir }
    }

    fn shot(&self, name: &str, width: usize, f: impl FnOnce(&Console) -> Result) {
        let Some(dir) = &self.dir else {
            let console = Console::builder().theme(rich_ext::extended_theme()).build();
            return f(&console).expect("the shot renders");
        };
        let console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .theme(rich_ext::extended_theme())
            .build();
        let id = format!("{}-{name}", self.topic);
        let svg = console.export_svg(&id, &id, |console| {
            f(console).expect("the shot renders");
        });
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        let path = dir.join(format!("{id}.svg"));
        std::fs::write(&path, svg).expect("write the SVG");
        eprintln!("wrote {}", path.display());
    }
}
