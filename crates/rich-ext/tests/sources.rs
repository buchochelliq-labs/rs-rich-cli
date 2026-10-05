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

// ------------------------------------------------ workspaces and hostile input

/// Two workspace members: `member` has the dev dependency `x` (which uses
/// `y` 1.0.0), and `bbb` depends on `member` and on `y` 2.0.0. Which member
/// sorts first decides which root the walk meets first.
fn workspace(member: &str) -> DepGraph {
    let crates_io = "registry+https://github.com/rust-lang/crates.io-index";
    let pkg = |name: &str, version: &str, source: Option<&str>| {
        serde_json::json!({"id": format!("{name} {version}"), "name": name,
            "version": version, "source": source})
    };
    let dep = |id: &str, kind: Option<&str>| {
        serde_json::json!({"name": id.split(' ').next(), "pkg": id,
            "dep_kinds": [{"kind": kind, "target": null}]})
    };
    let member_id = format!("{member} 0.1.0");
    let json = serde_json::json!({
        "packages": [
            pkg(member, "0.1.0", None), pkg("bbb", "0.1.0", None),
            pkg("x", "1.0.0", Some(crates_io)), pkg("y", "1.0.0", Some(crates_io)),
            pkg("y", "2.0.0", Some(crates_io)),
        ],
        "workspace_members": [member_id, "bbb 0.1.0"],
        "resolve": {"root": null, "nodes": [
            {"id": member_id, "deps": [dep("x 1.0.0", Some("dev"))]},
            {"id": "bbb 0.1.0", "deps": [dep(&member_id, None), dep("y 2.0.0", None)]},
            {"id": "x 1.0.0", "deps": [dep("y 1.0.0", None)]},
            {"id": "y 1.0.0", "deps": []},
            {"id": "y 2.0.0", "deps": []},
        ]}
    });
    DepGraph::from_value(&json).unwrap()
}

#[test]
fn a_root_reached_through_another_root_keeps_its_dev_dependencies() {
    for member in ["aaa", "zzz"] {
        let graph = workspace(member);
        let all = [DepKind::Normal, DepKind::Build, DepKind::Dev];
        let x = graph.find("x")[0];
        assert!(graph.reachable(&all)[x], "{member}: x unreachable");
        assert!(graph.duplicates().contains_key("y"), "{member}");
        let why = WhyTree::new(graph.clone(), "x").unwrap();
        assert_eq!(why.targets().len(), 1, "{member}");
        assert_eq!(
            plain(60, &why),
            format!("x v1.0.0\n└── {member} v0.1.0 [dev]"),
            "{member}"
        );
        // Each member at the top level is expanded in full, dev section
        // included, however it was reached before.
        let tree = plain(60, &DepTree::new(graph.clone()).summary(true));
        let expanded = format!("{member} v0.1.0\n└── [dev-dependencies]\n    └── x v1.0.0");
        let expanded_in_list =
            format!("{member} v0.1.0\n│   └── [dev-dependencies]\n│       └── x v1.0.0");
        let expanded_last =
            format!("{member} v0.1.0\n    └── [dev-dependencies]\n        └── x v1.0.0");
        assert!(
            tree.contains(&expanded)
                || tree.contains(&expanded_in_list)
                || tree.contains(&expanded_last),
            "{member}:\n{tree}"
        );
        let top = format!("{member} v0.1.0 (*)");
        assert!(!tree.lines().any(|line| line == top), "{tree}");
        assert!(tree.contains("duplicate: y v1.0.0, v2.0.0"), "{tree}");
        // Without dev dependencies, y 1.0.0 is not reached: no duplicate.
        let no_dev = [DepKind::Normal, DepKind::Build];
        let tree = plain(60, &DepTree::new(graph).kinds(&no_dev).summary(true));
        assert!(!tree.contains("x v1.0.0"), "{tree}");
        assert!(
            tree.contains("no crate is resolved at more than one version"),
            "{tree}"
        );
    }
}

/// A chain of `n` packages, `p0` (the root) to `p{n-1}`.
fn chain(n: usize) -> DepGraph {
    let crates_io = "registry+https://github.com/rust-lang/crates.io-index";
    let packages: Vec<serde_json::Value> = (0..n)
        .map(|i| {
            serde_json::json!({"id": format!("p{i} 1.0.0"), "name": format!("p{i}"),
                "version": "1.0.0", "source": if i == 0 { None } else { Some(crates_io) }})
        })
        .collect();
    let nodes: Vec<serde_json::Value> = (0..n)
        .map(|i| {
            let deps: Vec<serde_json::Value> = if i + 1 < n {
                vec![serde_json::json!({"name": format!("p{}", i + 1),
                    "pkg": format!("p{} 1.0.0", i + 1),
                    "dep_kinds": [{"kind": null, "target": null}]})]
            } else {
                Vec::new()
            };
            serde_json::json!({"id": format!("p{i} 1.0.0"), "deps": deps})
        })
        .collect();
    let json = serde_json::json!({
        "packages": packages,
        "workspace_members": ["p0 1.0.0"],
        "resolve": {"root": "p0 1.0.0", "nodes": nodes}
    });
    DepGraph::from_value(&json).unwrap()
}

#[test]
fn very_deep_dependency_chains_are_cut_off_not_overflowed() {
    // On a thread with the default (2 MiB) stack.
    std::thread::Builder::new()
        .spawn(|| {
            let graph = chain(3000);
            let tree = plain(700, &DepTree::new(graph.clone()));
            assert!(tree.contains("levels not shown"), "{tree}");
            let tree = plain(700, &DepTree::new(graph.clone()).duplicates_only(true));
            assert_eq!(tree, "");
            let why = plain(700, &WhyTree::new(graph, "p2999").unwrap());
            assert!(why.starts_with("p2999 v1.0.0"), "{why}");
            assert!(why.contains("levels not shown"), "{why}");
        })
        .unwrap()
        .join()
        .unwrap();
}

/// `$defs` `a0` … `a{levels}`, each an object whose properties `x` and `y`
/// both refer to the next: 2^levels paths through shared definitions.
/// `description` is set on every definition when given.
fn shared_refs(levels: usize, description: Option<&str>) -> serde_json::Value {
    let mut defs = serde_json::Map::new();
    for i in 0..levels {
        let next = serde_json::json!({"$ref": format!("#/$defs/a{}", i + 1)});
        let mut def = serde_json::json!({"type": "object",
            "properties": {"x": next.clone(), "y": next}});
        if let Some(description) = description {
            def["description"] = description.into();
        }
        defs.insert(format!("a{i}"), def);
    }
    defs.insert(format!("a{levels}"), serde_json::json!({"type": "string"}));
    serde_json::json!({"$defs": defs, "$ref": "#/$defs/a0"})
}

#[test]
fn shared_refs_do_not_blow_up_the_tree_or_the_diff() {
    let (tree, changes, truncated) = std::thread::spawn(|| {
        let schema = shared_refs(18, None);
        let tree = plain(100, &SchemaTree::new(schema.clone()));
        let diff = SchemaDiff::new(&schema, &shared_refs(18, Some("v2")));
        (tree, diff.changes().len(), diff.is_truncated())
    })
    .join()
    .unwrap();
    assert!(tree.lines().count() <= 10_001, "{}", tree.lines().count());
    assert!(
        tree.ends_with("(the tree stops at 10000 entries)"),
        "{}",
        &tree[tree.len() - 200..]
    );
    assert!(changes <= 10_000, "{changes} changes");
    assert!(truncated);
    let diff = SchemaDiff::new(&shared_refs(18, None), &shared_refs(18, Some("v2")));
    let out = plain(100, &diff);
    assert!(out.contains("stopped after"), "{}", &out[out.len() - 300..]);
}

#[test]
fn a_ref_spelled_another_way_is_still_recursive() {
    let schema = serde_json::json!({
        "$defs": {"node": {"type": "object",
            "properties": {"next": {"$ref": "#/%24defs/node"}}}},
        "$ref": "#/$defs/node"
    });
    let tree = plain(100, &SchemaTree::new(schema));
    assert_eq!(tree.lines().count(), 2, "{tree}");
    assert!(tree.contains("(recursive)"), "{tree}");
}

#[test]
fn additional_properties_chains_keep_the_depth_guard() {
    // `a0` … `a20000` (`b…` in the new version), each one's
    // `additionalProperties` the next, and a `minLength` that changes at
    // every level.
    let chain = |prefix: &str, min: u64| {
        let mut defs = serde_json::Map::new();
        for i in 0..20_000 {
            defs.insert(
                format!("{prefix}{i}"),
                serde_json::json!({"type": "object", "minLength": min,
                    "additionalProperties": {"$ref": format!("#/$defs/{prefix}{}", i + 1)}}),
            );
        }
        serde_json::json!({"$defs": defs, "$ref": format!("#/$defs/{prefix}0")})
    };
    let changes = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            SchemaDiff::new(&chain("a", 1), &chain("b", 2))
                .changes()
                .len()
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(changes < 100, "{changes}");
}
