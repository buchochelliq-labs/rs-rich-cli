//! Guide: Dependency graphs and JSON Schemas — run: cargo run -p rs-rich-ext --example guide_sources --features data [-- --svg docs/media/guide]
//!
//! The pictures on `docs/guide/ext/sources.md` come from this file. It reads
//! the fixtures the crate's tests read, in `tests/fixtures/sources` and
//! `tests/fixtures/supply-chain`.

use std::error::Error;
use std::path::PathBuf;

use rich::{ColorSystem, Console};
use rich_ext::deps::audit::AdvisoryReport;
use rich_ext::deps::duplicates::Consolidation;
use rich_ext::deps::features::{FeatureGraph, FeatureTree};
use rich_ext::deps::licenses::LicenseReport;
use rich_ext::deps::timings::{Timings, TimingsReport};
use rich_ext::deps::{DepGraph, DepTree, WhyTree};
use rich_ext::schema::{self, SchemaDiff, SchemaTree};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

/// A fixture from `tests/fixtures`, standing in for a file you would read.
fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

// --8<-- [start:tree]
fn dep_tree(console: &Console) -> Result {
    // The output of `cargo metadata --format-version 1`.
    let json = fixture("sources/cargo-metadata.json");
    let graph = DepGraph::from_json(&json)?;
    console.print(&DepTree::new(graph).summary(true));
    Ok(())
}
// --8<-- [end:tree]

// --8<-- [start:duplicates]
fn duplicates(console: &Console) -> Result {
    let graph = DepGraph::from_json(&fixture("sources/cargo-metadata.json"))?;
    console.print(&DepTree::new(graph).duplicates_only(true).summary(true));
    Ok(())
}
// --8<-- [end:duplicates]

// --8<-- [start:why]
fn why(console: &Console) -> Result {
    let graph = DepGraph::from_json(&fixture("sources/cargo-metadata.json"))?;
    console.print(&WhyTree::new(graph, "syn@1.0.109")?);
    Ok(())
}
// --8<-- [end:why]

// --8<-- [start:consolidation]
fn consolidation(console: &Console) -> Result {
    let graph = DepGraph::from_json(&fixture("supply-chain/cargo-metadata.json"))?;
    // What `--duplicates` adds under the tree; `DepTree::consolidation(true)`
    // puts it there.
    console.print(&Consolidation::new(&graph));
    Ok(())
}
// --8<-- [end:consolidation]

// --8<-- [start:features]
fn features(console: &Console) -> Result {
    let graph = FeatureGraph::from_json(&fixture("supply-chain/cargo-metadata.json"))?;
    console.print(&FeatureTree::new(graph, Some("syn@2.0.79"))?);
    Ok(())
}
// --8<-- [end:features]

// --8<-- [start:timings]
fn timings(console: &Console) -> Result {
    // target/cargo-timings/cargo-timing.html, from `cargo build --timings`.
    let timings = Timings::parse(&fixture("supply-chain/cargo-timing.html"))?;
    console.print(&TimingsReport::new(timings).limit(6));
    Ok(())
}
// --8<-- [end:timings]

// --8<-- [start:audit]
fn audit(console: &Console) -> Result {
    // `cargo audit --json > audit.json`
    let report = AdvisoryReport::from_cargo_audit(&fixture("supply-chain/cargo-audit.json"))?;
    console.print(&report);
    Ok(())
}
// --8<-- [end:audit]

// --8<-- [start:licenses]
fn licenses(console: &Console) -> Result {
    let report = LicenseReport::from_json(&fixture("supply-chain/cargo-metadata.json"))?;
    console.print(&report);
    Ok(())
}
// --8<-- [end:licenses]

// --8<-- [start:schema]
fn schema_tree(console: &Console) -> Result {
    let json = fixture("sources/order-v1.schema.json");
    console.print(&SchemaTree::from_json(&json)?);
    Ok(())
}
// --8<-- [end:schema]

// --8<-- [start:schema-diff]
fn schema_diff(console: &Console) -> Result {
    let old = schema::parse(&fixture("sources/order-v1.schema.json"))?;
    let new = schema::parse(&fixture("sources/order-v2.schema.json"))?;
    let diff = SchemaDiff::new(&old, &new).names("order-v1.schema.json", "order-v2.schema.json");
    console.print(&diff);
    Ok(())
}
// --8<-- [end:schema-diff]

fn main() {
    let shots = Shots::from_args("guide_sources");
    shots.shot("tree", 64, dep_tree);
    shots.shot("duplicates", 64, duplicates);
    shots.shot("why", 64, why);
    shots.shot("consolidation", 72, consolidation);
    shots.shot("features", 72, features);
    shots.shot("timings", 80, timings);
    shots.shot("audit", 100, audit);
    shots.shot("licenses", 100, licenses);
    shots.shot("schema", 90, schema_tree);
    shots.shot("schema-diff", 80, schema_diff);
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
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
        eprintln!("wrote {}", dir.join(format!("{id}.svg")).display());
    }
}
