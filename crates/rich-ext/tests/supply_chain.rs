//! Supply-chain reports (0.0.16 workstream 6), each from a fixture in
//! `tests/fixtures/supply-chain`: duplicate consolidation, feature trees,
//! build timings, advisories and licences. Snapshots live in
//! `tests/snapshots/supply_chain_*.txt`; set `UPDATE_SNAPSHOTS=1` to rewrite
//! them after a deliberate change, then review the diff.
#![cfg(feature = "data")]

use rich::cells::cell_len;
use rich::{Console, Renderable};
use rich_ext::deps::audit::{AdvisoryKind, AdvisoryReport, Severity};
use rich_ext::deps::duplicates::Consolidation;
use rich_ext::deps::features::{FeatureGraph, FeatureTree};
use rich_ext::deps::licenses::{LicenseClass, LicenseReport};
use rich_ext::deps::timings::{Timings, TimingsReport};
use rich_ext::deps::{DepGraph, DepKind, DepTree};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/supply-chain/{name}",
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
        "{}/tests/snapshots/supply_chain_{name}.txt",
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

fn metadata() -> String {
    fixture("cargo-metadata.json")
}

#[test]
fn duplicates_consolidate_onto_the_most_used_version() {
    let graph = DepGraph::from_json(&metadata()).unwrap();
    let all = [DepKind::Normal, DepKind::Build, DepKind::Dev];
    let syn = &graph.consolidation(&all)[0];
    assert_eq!(syn.name, "syn");
    assert_eq!(syn.shared_version().version, "2.0.79");
    assert_eq!(syn.to_move().len(), 1);
    check("consolidation", &plain(80, &Consolidation::new(&graph)), 80);
    // Under the tree, after the one-line summary.
    let tree = plain(
        80,
        &DepTree::new(graph)
            .duplicates_only(true)
            .summary(true)
            .consolidation(true),
    );
    assert!(
        tree.contains("duplicate: syn v1.0.109, v2.0.79\n\nconsolidation: 1 crate"),
        "{tree}"
    );
}

#[test]
fn feature_trees_show_definitions_and_requests() {
    let graph = FeatureGraph::from_json(&metadata()).unwrap();
    check(
        "features_syn",
        &plain(80, &FeatureTree::new(graph.clone(), Some("syn")).unwrap()),
        80,
    );
    let out = plain(80, &FeatureTree::new(graph.clone(), Some("serde")).unwrap());
    assert!(
        out.contains("dep:serde_derive → serde_derive v1.0.210"),
        "{out}"
    );
    assert!(
        out.contains("demo-core v0.3.0: default features, derive"),
        "{out}"
    );
    assert!(out.contains("its feature std: std"), "{out}");
    // Every crate with a feature: the members first.
    let out = plain(80, &FeatureTree::new(graph, None).unwrap());
    assert!(
        out.starts_with("demo-app v0.3.0  2 features enabled"),
        "{out}"
    );
    assert!(
        out.ends_with("other crates have no features enabled"),
        "{out}"
    );
}

#[test]
fn timings_render_as_bars_and_a_table() {
    let timings = Timings::parse(&fixture("cargo-timing.html")).unwrap();
    assert_eq!(timings.units().len(), 12);
    let syn = timings
        .units()
        .iter()
        .find(|u| u.version == "2.0.79")
        .unwrap();
    assert_eq!(syn.rmeta, Some(3.95));
    let quote = timings.units().iter().find(|u| u.name == "quote").unwrap();
    assert_eq!(quote.rmeta, Some(0.4));
    assert!((quote.codegen.unwrap() - 0.26).abs() < 1e-9);
    check(
        "timings",
        &plain(80, &TimingsReport::new(timings.clone())),
        80,
    );
    check(
        "timings_limit_40",
        &plain(40, &TimingsReport::new(timings).limit(3)),
        40,
    );
}

#[test]
fn advisories_group_by_severity() {
    let report = AdvisoryReport::from_cargo_audit(&fixture("cargo-audit.json")).unwrap();
    assert_eq!(report.vulnerabilities(), 2);
    let kinds: Vec<AdvisoryKind> = report.sorted().iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [
            AdvisoryKind::Vulnerability,
            AdvisoryKind::Vulnerability,
            AdvisoryKind::Unmaintained,
            AdvisoryKind::Yanked
        ]
    );
    assert_eq!(report.sorted()[0].severity, Severity::High);
    assert_eq!(report.sorted()[1].severity, Severity::Unknown);
    check("audit", &plain(100, &report), 100);
}

#[test]
fn licences_group_and_mark_copyleft_and_missing() {
    let report = LicenseReport::from_json(&metadata()).unwrap();
    let counts = report.counts();
    assert_eq!(counts[&LicenseClass::Copyleft], 1);
    assert_eq!(counts[&LicenseClass::File], 1);
    assert_eq!(counts[&LicenseClass::Missing], 1);
    check("licenses", &plain(100, &report), 100);
}

/// Rendered in colour with links, so a control sequence smuggled into a
/// link shows as well as one in the text.
fn coloured(renderable: &dyn Renderable) -> String {
    Console::builder()
        .width(160)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .build()
        .render_to_string(renderable)
}

/// Whether `out` holds an escape sequence other than the SGR colours and
/// the OSC 8 links the console writes itself.
fn smuggled(out: &str) -> Option<String> {
    let mut rest = out;
    while let Some(at) = rest.find('\u{1b}') {
        let tail = &rest[at..];
        let sgr = tail.strip_prefix("\u{1b}[").and_then(|t| {
            let end = t.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
            (t[end..].starts_with('m')).then_some(2 + end + 1)
        });
        let link = tail.strip_prefix("\u{1b}]8;").and_then(|t| {
            // `ESC ] 8 ; params ; URL ESC \`: the URL may hold no control.
            let end = t.find("\u{1b}\\")?;
            (!t[..end].contains(|c: char| c.is_control())).then_some(4 + end + 2)
        });
        match sgr.or(link) {
            Some(skip) => rest = &tail[skip..],
            None => return Some(tail.chars().take(24).collect()),
        }
    }
    out.contains(['\u{7}', '\u{9b}'])
        .then(|| "a BEL or CSI".to_string())
}

#[test]
fn hostile_report_text_cannot_reach_the_terminal() {
    // An advisory's id, title, package and link, a timing unit's name and
    // target, and a licence expression are text from a file; each carries
    // a title change (OSC 0), a screen clear, and a link that closes early.
    let evil = r"\u001b]0;pwned\u0007\u001b[2J\u009b31m";
    let audit = format!(
        r#"{{"vulnerabilities": {{"list": [{{
            "advisory": {{"id": "RUSTSEC-1{evil}", "title": "t{evil}",
              "url": "https://x/\u001b\\\u001b]0;evil\u0007", "aliases": ["CVE{evil}"]}},
            "versions": {{"patched": [">=1{evil}"]}},
            "package": {{"name": "p{evil}", "version": "1{evil}"}}}}]}},
          "warnings": {{}}}}"#
    );
    let report = AdvisoryReport::from_cargo_audit(&audit).unwrap();
    let timings = Timings::parse(&format!(
        r#"[{{"name": "n{evil}", "version": "1{evil}", "target": "t{evil}", "duration": 1.0}}]"#
    ))
    .unwrap();
    let metadata = format!(
        r#"{{"packages": [
            {{"id": "a 1.0.0", "name": "a{evil}", "version": "1.0.0{evil}",
              "license": "MIT AND {evil}", "source": null,
              "features": {{"f{evil}": ["dep:b{evil}"]}}, "dependencies": []}}],
          "workspace_members": ["a 1.0.0"],
          "resolve": {{"root": null, "nodes": [
            {{"id": "a 1.0.0", "features": ["f{evil}"], "deps": []}}]}}}}"#
    );
    let licenses = LicenseReport::from_json(&metadata).unwrap();
    let features = FeatureTree::new(FeatureGraph::from_json(&metadata).unwrap(), None).unwrap();
    let tree = DepTree::new(DepGraph::from_json(&metadata).unwrap());
    let renderables: [(&str, &dyn Renderable); 5] = [
        ("audit", &report),
        ("timings", &TimingsReport::new(timings)),
        ("licenses", &licenses),
        ("features", &features),
        ("tree", &tree),
    ];
    for (name, renderable) in renderables {
        let out = coloured(renderable);
        assert_eq!(smuggled(&out), None, "{name}: {out:?}");
        // What was there still shows, as visible text.
        assert!(out.contains('␛'), "{name}: {out:?}");
    }
}

/// `cargo metadata` for `count` workspace members, each with one feature
/// enabled and a dependency on the next, so every crate is a root, a
/// member, a dependent and a feature tree.
fn wide_workspace(count: usize) -> String {
    let id = |i: usize| format!("p{i} 1.0.0");
    let mut packages = Vec::new();
    let mut nodes = Vec::new();
    for i in 0..count {
        let next = (i + 1) % count;
        packages.push(serde_json::json!({
            "id": id(i), "name": format!("p{i}"), "version": "1.0.0", "source": null,
            "features": {"f": []},
            "dependencies": [{"name": format!("p{next}"), "features": ["f"]}]
        }));
        nodes.push(serde_json::json!({
            "id": id(i), "features": ["f"],
            "deps": [{"name": format!("p{next}"), "pkg": id(next),
                      "dep_kinds": [{"kind": null, "target": null}]}]
        }));
    }
    let members: Vec<String> = (0..count).map(id).collect();
    serde_json::json!({
        "packages": packages, "workspace_members": members,
        "resolve": {"root": null, "nodes": nodes}
    })
    .to_string()
}

#[test]
fn a_wide_workspace_is_read_in_near_linear_time() {
    // Root, member and dependent lookups were linear scans inside loops
    // over every package: 100,000 members took minutes. 30,000 is enough
    // to tell quadratic from linear.
    let json = wide_workspace(30_000);
    let started = std::time::Instant::now();
    let graph = DepGraph::from_json(&json).unwrap();
    assert!(graph.duplicates().is_empty());
    assert!(graph.consolidation(&[DepKind::Normal]).is_empty());
    let features = FeatureGraph::from_json(&json).unwrap();
    let tree = FeatureTree::new(features, None).unwrap();
    assert_eq!(tree.targets().len(), 30_000);
    // What drawing every tree asks of the graph: each crate's requests.
    for package in 0..30_000 {
        assert_eq!(tree.graph().requests(package).len(), 1);
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}"
    );
}
