//! Diagram sources drawn as trees: Cargo dependency graphs (`deps`) and JSON
//! Schemas (`schema`), each from a fixture in `tests/fixtures/sources`.
//! Snapshots live in `tests/snapshots/sources_*.txt`; set
//! `UPDATE_SNAPSHOTS=1` to rewrite them after a deliberate change, then
//! review the diff.
#![cfg(feature = "data")]

use rich::cells::cell_len;
use rich::{ColorSystem, Console, Renderable};
use rich_ext::deps::{DepGraph, DepKind, DepTree, WhyTree};
use rich_ext::schema::{self, SchemaDiff, SchemaTree};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/sources/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn plain(width: usize, renderable: &dyn Renderable) -> String {
    Console::builder()
        .width(width)
        .color_system(None)
        .build()
        .render_to_string(renderable)
}

fn check(name: &str, actual: &str, width: usize) {
    for line in actual.lines() {
        assert!(
            cell_len(line) <= width,
            "{name}: wider than {width}: {line:?}"
        );
    }
    let path = format!(
        "{}/tests/snapshots/sources_{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {path}; run with UPDATE_SNAPSHOTS=1"));
    assert_eq!(actual, expected, "{name} changed:\n{actual}");
}

fn graph() -> DepGraph {
    DepGraph::from_json(&fixture("cargo-metadata.json")).unwrap()
}

#[test]
fn the_dependency_tree_renders_from_cargo_metadata() {
    check(
        "deps_tree",
        &plain(80, &DepTree::new(graph()).summary(true)),
        80,
    );
    check(
        "deps_depth_1",
        &plain(80, &DepTree::new(graph()).max_depth(1)),
        80,
    );
}

#[test]
fn duplicate_versions_are_found_and_marked() {
    let graph = graph();
    let duplicates = graph.duplicates();
    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates["syn"], ["1.0.109", "2.0.79"]);
    let out = plain(80, &DepTree::new(graph.clone()).duplicates_only(true));
    assert!(out.contains("syn v1.0.109 (duplicate)"), "{out}");
    assert!(!out.contains("log v0.4.22"), "{out}");
    // Normal dependencies only: no build or dev sections.
    let out = plain(80, &DepTree::new(graph).kinds(&[DepKind::Normal]));
    assert!(
        !out.contains("[dev-dependencies]") && !out.contains("cc v"),
        "{out}"
    );
}

#[test]
fn why_shows_every_path_that_pulls_a_crate_in() {
    let graph = graph();
    check(
        "deps_why_syn",
        &plain(80, &WhyTree::new(graph.clone(), "syn@1.0.109").unwrap()),
        80,
    );
    let target = graph.find("unicode-ident")[0];
    let paths = graph.paths_to(
        target,
        100,
        &[DepKind::Normal, DepKind::Build, DepKind::Dev],
    );
    assert!(paths.iter().all(|path| graph.roots().contains(&path[0])));
    assert!(paths.iter().all(|path| *path.last().unwrap() == target));
    let shortest: Vec<String> = paths[0].iter().map(|&p| graph.display(p)).collect();
    assert_eq!(shortest.len(), 4, "{shortest:?}");
    assert!(WhyTree::new(graph, "tokio").is_err());
}

#[test]
fn colour_marks_duplicates_but_words_carry_them_too() {
    let console = Console::builder()
        .width(80)
        .color_system(Some(ColorSystem::Standard))
        .force_terminal(true)
        .no_color(false)
        .build();
    let out = console.render_to_string(&DepTree::new(graph()));
    assert!(out.contains("\u{1b}[1;33msyn"), "{out}");
    assert!(out.contains("(duplicate)"), "{out}");
}

#[test]
fn the_schema_tree_renders_from_a_fixture() {
    let tree = SchemaTree::from_json(&fixture("order-v1.schema.json")).unwrap();
    check("schema_tree", &plain(100, &tree), 100);
    let narrow = plain(40, &tree);
    for line in narrow.lines() {
        assert!(cell_len(line) <= 40, "{line:?}");
    }
}

#[test]
fn the_schema_diff_renders_from_two_fixtures() {
    let old = schema::parse(&fixture("order-v1.schema.json")).unwrap();
    let new = schema::parse(&fixture("order-v2.schema.json")).unwrap();
    let diff = SchemaDiff::new(&old, &new).names("order-v1", "order-v2");
    check("schema_diff", &plain(100, &diff), 100);
    assert!(diff.breaking() > 0);
}
