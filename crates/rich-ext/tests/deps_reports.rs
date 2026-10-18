//! `rich deps`' sizes, regression baselines and licence policy (0.0.19
//! workstream 16, #706, #704), from the fixtures in `tests/fixtures/deps`
//! and a fake target directory built under `CARGO_TARGET_TMPDIR`.
//! Snapshots live in `tests/snapshots/deps_reports_*.txt`; set
//! `UPDATE_SNAPSHOTS=1` to rewrite them after a deliberate change, then
//! review the diff.
#![cfg(feature = "data")]

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rich::cells::cell_len;
use rich::{Console, Renderable};
use rich_ext::deps::compare::{Comparison, Quantity, Threshold, Thresholds, Verdict};
use rich_ext::deps::licenses::{LicenseReport, LicensedPackage};
use rich_ext::deps::policy::{self, Policy, PolicyReport};
use rich_ext::deps::sbom::Sbom;
use rich_ext::deps::sizes::{ArtifactKind, SizeOptions, SizeReport, SizeSource, Sizes};
use rich_ext::deps::timings::Timings;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
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
        "{}/tests/snapshots/deps_reports_{name}.txt",
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

/// A fresh directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("deps-reports-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, bytes: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn age(path: &Path, seconds: u64) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(seconds))
        .unwrap();
}

/// A workspace and its target directory: `app` (a member with a binary)
/// depending on `syn` at two versions and on `serde`, with `serde_derive`
/// a proc-macro and `rmeta-only` checked but not built. Returns the
/// target directory and `cargo metadata` for it.
fn fake_build(name: &str) -> (PathBuf, String) {
    let root = scratch(name);
    let registry = root.join("registry");
    let workspace = root.join("work");
    let package = |name: &str, version: &str, dir: &Path, targets: &str| {
        format!(
            r#"{{"id": "{name} {version}", "name": "{name}", "version": "{version}",
                "source": null, "manifest_path": "{}", "targets": {targets}}}"#,
            dir.join("Cargo.toml").display()
        )
    };
    let syn1 = registry.join("syn-1.0.109");
    let syn2 = registry.join("syn-2.0.79");
    let serde = registry.join("serde-1.0.210");
    let derive = registry.join("serde_derive-1.0.210");
    let rmeta = registry.join("rmeta-only-0.1.0");
    let app = workspace.join("app");
    // Sources: what `--source-sizes` adds up.
    write(&app.join("src/main.rs"), 1000);
    write(&app.join("Cargo.toml"), 200);
    write(&app.join("target/debug/huge"), 50_000);
    write(&app.join("nested/Cargo.toml"), 10);
    write(&app.join("nested/src/lib.rs"), 70_000);
    for dir in [&syn1, &syn2, &serde, &derive, &rmeta] {
        write(&dir.join("src/lib.rs"), 2000);
    }
    let lib = |name: &str| format!(r#"[{{"name": "{name}", "kind": ["lib"]}}]"#);
    let packages = [
        package(
            "app",
            "0.1.0",
            &app,
            r#"[{"name": "app", "kind": ["bin"]}, {"name": "smoke-test", "kind": ["test"]}]"#,
        ),
        package("syn", "1.0.109", &syn1, &lib("syn")),
        package("syn", "2.0.79", &syn2, &lib("syn")),
        package("serde", "1.0.210", &serde, &lib("serde")),
        package(
            "serde_derive",
            "1.0.210",
            &derive,
            r#"[{"name": "serde_derive", "kind": ["proc-macro"]}]"#,
        ),
        package("rmeta-only", "0.1.0", &rmeta, &lib("rmeta-only")),
    ];
    let target = workspace.join("target");
    let metadata = format!(
        r#"{{"packages": [{}], "workspace_members": ["app 0.1.0"],
            "resolve": {{"root": "app 0.1.0", "nodes": [
              {{"id": "app 0.1.0", "deps": [
                {{"pkg": "syn 1.0.109", "dep_kinds": [{{"kind": null}}]}},
                {{"pkg": "syn 2.0.79", "dep_kinds": [{{"kind": null}}]}},
                {{"pkg": "serde 1.0.210", "dep_kinds": [{{"kind": null}}]}},
                {{"pkg": "serde_derive 1.0.210", "dep_kinds": [{{"kind": null}}]}},
                {{"pkg": "rmeta-only 0.1.0", "dep_kinds": [{{"kind": "dev"}}]}}]}},
              {{"id": "syn 1.0.109", "deps": []}}, {{"id": "syn 2.0.79", "deps": []}},
              {{"id": "serde 1.0.210", "deps": []}}, {{"id": "serde_derive 1.0.210", "deps": []}},
              {{"id": "rmeta-only 0.1.0", "deps": []}}]}},
            "target_directory": "{}"}}"#,
        packages.join(", "),
        target.display()
    );
    let deps = target.join("debug/deps");
    let dep_info = |crate_name: &str, hash: &str, source: &Path| {
        let artifact = deps.join(format!("lib{crate_name}-{hash}.rlib"));
        std::fs::write(
            deps.join(format!("{crate_name}-{hash}.d")),
            format!(
                "{}: {}\n\n{}:\n",
                artifact.display(),
                source.join("src/lib.rs").display(),
                source.join("src/lib.rs").display()
            ),
        )
        .unwrap();
    };
    // syn at two versions: told apart by their dep-info files.
    write(&deps.join("libsyn-1111111111111111.rlib"), 300_000);
    write(&deps.join("libsyn-2222222222222222.rlib"), 500_000);
    // An older build of syn 2 (other features): left behind, not counted.
    write(&deps.join("libsyn-3333333333333333.rlib"), 900_000);
    age(&deps.join("libsyn-3333333333333333.rlib"), 3600);
    dep_info("syn", "1111111111111111", &syn1);
    dep_info("syn", "2222222222222222", &syn2);
    dep_info("syn", "3333333333333333", &syn2);
    // serde: the rlib wins over the rmeta beside it.
    write(&deps.join("libserde-4444444444444444.rlib"), 200_000);
    write(&deps.join("libserde-4444444444444444.rmeta"), 50_000);
    write(&deps.join("libserde_derive-5555555555555555.so"), 120_000);
    write(&deps.join("librmeta_only-6666666666666666.rmeta"), 4_000);
    // No package claims these.
    write(&deps.join("libgone-7777777777777777.rlib"), 10_000);
    // A test harness, the uplifted copy of a library, and other files.
    write(&deps.join("app-8888888888888888"), 99_000);
    write(&deps.join("app-8888888888888888.d"), 10);
    // What `cargo check` leaves for a test and a binary: not libraries.
    write(&deps.join("libsmoke_test-aaaaaaaaaaaaaaaa.rmeta"), 5_000);
    write(&deps.join("libapp-bbbbbbbbbbbbbbbb.rmeta"), 6_000);
    std::fs::write(
        deps.join("app-bbbbbbbbbbbbbbbb.d"),
        format!("x.rmeta: {}\n", app.join("src/main.rs").display()),
    )
    .unwrap();
    write(&target.join("debug/libserde.rlib"), 200_000);
    write(&target.join("debug/app.d"), 100);
    write(&target.join("debug/app"), 700_000);
    write(
        &target.join("release/deps/libsyn-9999999999999999.rlib"),
        123,
    );
    (target, metadata)
}

#[test]
fn sizes_come_from_the_newest_artifact_of_each_package() {
    let (target, metadata) = fake_build("target");
    let sizes = Sizes::from_target_dir(&target, &metadata, &SizeOptions::default()).unwrap();
    let rows: Vec<(String, Option<&str>, ArtifactKind, u64)> = sizes
        .entries()
        .iter()
        .map(|e| (e.name.clone(), e.version.as_deref(), e.kind, e.bytes))
        .collect();
    assert_eq!(
        rows,
        [
            ("app".to_string(), Some("0.1.0"), ArtifactKind::Bin, 700_000),
            ("syn".into(), Some("2.0.79"), ArtifactKind::Lib, 500_000),
            ("syn".into(), Some("1.0.109"), ArtifactKind::Lib, 300_000),
            ("serde".into(), Some("1.0.210"), ArtifactKind::Lib, 200_000),
            (
                "serde_derive".into(),
                Some("1.0.210"),
                ArtifactKind::ProcMacro,
                120_000
            ),
            (
                "rmeta-only".into(),
                Some("0.1.0"),
                ArtifactKind::Rmeta,
                4_000
            ),
        ]
    );
    assert_eq!(sizes.total(), 1_824_000);
    assert!(sizes.entries().iter().all(|e| e.source_bytes.is_none()));
    let SizeSource::TargetDir {
        profile, unmatched, ..
    } = sizes.source()
    else {
        panic!("{:?}", sizes.source());
    };
    assert_eq!((profile.as_str(), *unmatched), ("debug", 1));
    assert_eq!(sizes.entries()[0].key(), "app (bin)");
    assert_eq!(sizes.entries()[1].id.as_deref(), Some("syn 2.0.79"));

    // The release profile, by name, or given as the directory itself.
    let release = SizeOptions::default().profile("bench");
    let sizes = Sizes::from_target_dir(&target, &metadata, &release).unwrap();
    assert_eq!(sizes.total(), 123);
    let sizes = Sizes::from_target_dir(&target.join("release"), &metadata, &SizeOptions::default())
        .unwrap();
    assert_eq!(sizes.total(), 123);
    let wrong = SizeOptions::default().profile("release");
    assert!(Sizes::from_target_dir(&target.join("debug"), &metadata, &wrong).is_err());
    let missing = SizeOptions::default().profile("dist");
    let err = Sizes::from_target_dir(&target, &metadata, &missing).unwrap_err();
    assert!(err.to_string().contains("no dist directory"), "{err}");
}

#[test]
fn source_sizes_leave_out_target_and_nested_packages() {
    let (target, metadata) = fake_build("sources");
    let options = SizeOptions::default().sources(true);
    let sizes = Sizes::from_target_dir(&target, &metadata, &options).unwrap();
    let source = |name: &str| {
        sizes
            .entries()
            .iter()
            .find(|e| e.name == name)
            .and_then(|e| e.source_bytes)
    };
    // app's main.rs and Cargo.toml; not its target/ or its nested package.
    assert_eq!(source("app"), Some(1200));
    assert_eq!(source("serde"), Some(2000));
    assert_eq!(source("serde_derive"), Some(2000));
    assert_eq!(sizes.source_total(), Some(1200 + 5 * 2000));
    let out = plain(100, &SizeReport::new(sizes));
    assert!(out.contains(", 10.9 KiB of source"), "{out}");
    assert!(out.contains("Source"), "{out}");
}

#[test]
fn a_size_report_renders_and_reads_back() {
    let (target, metadata) = fake_build("report");
    let sizes = Sizes::from_target_dir(&target, &metadata, &SizeOptions::default()).unwrap();
    let out = plain(80, &SizeReport::new(sizes.clone()).limit(4));
    assert!(out.starts_with("6 artifacts: 1.7 MiB in "), "{out}");
    assert!(
        out.contains("1 crate in the directory matched no package"),
        "{out}"
    );
    assert!(out.contains("syn v2.0.79   488.3 KiB"), "{out}");
    assert!(out.contains("and 2 more, 121.1 KiB in all"), "{out}");
    let saved = sizes.to_json();
    assert_eq!(Sizes::parse(&saved).unwrap(), sizes);
}

#[test]
fn cargo_bloat_json_is_read() {
    let sizes = Sizes::parse(&fixture("deps/bloat.json")).unwrap();
    assert_eq!(sizes.entries().len(), 17);
    assert_eq!(sizes.entries()[0].name, "std");
    assert_eq!(sizes.total(), 2_039_296);
    assert_eq!(
        sizes.source(),
        &SizeSource::Bloat {
            file_size: Some(5_505_024),
            text_size: Some(2_097_152)
        }
    );
    check("bloat", &plain(80, &SizeReport::new(sizes).limit(8)), 80);
    // Build output captured with it (`2>&1`) is skipped.
    let noisy = format!("   Compiling ledger v2.1.0\n{}", fixture("deps/bloat.json"));
    assert_eq!(Sizes::parse(&noisy).unwrap().entries().len(), 17);
}

#[test]
fn comparisons_flag_regressions_new_and_removed_units() {
    let old = Sizes::parse(&fixture("deps/bloat-baseline.json")).unwrap();
    let new = Sizes::parse(&fixture("deps/bloat.json")).unwrap();
    let comparison = Comparison::sizes(&old, &new, Thresholds::SIZES);
    let regressed: Vec<&str> = comparison
        .regressions()
        .iter()
        .map(|r| r.key.as_str())
        .collect();
    assert_eq!(regressed, ["toml_edit", "regex_automata"]);
    assert_eq!(comparison.count(Verdict::New), 1);
    assert_eq!(comparison.count(Verdict::Removed), 2);
    assert_eq!(comparison.count(Verdict::Improvement), 1);
    assert_eq!(comparison.total().verdict, Verdict::Regression);
    assert!(comparison.failed());
    let automata = comparison
        .rows()
        .iter()
        .find(|r| r.key == "regex_automata")
        .unwrap();
    assert_eq!(automata.change(), 159_744.0);
    assert_eq!(automata.percent().map(|p| p.round()), Some(66.0));
    check(
        "bloat_comparison",
        &plain(
            90,
            &comparison
                .clone()
                .names("bloat-baseline.json", "bloat.json")
                .limit(8),
        ),
        90,
    );

    // A higher floor: no unit, nor the total, grew by 300 KiB.
    let loose = Thresholds::SIZES
        .with(Threshold::parse("300KiB").unwrap(), Quantity::Bytes)
        .unwrap();
    let comparison = Comparison::sizes(&old, &new, loose);
    assert!(comparison.regressions().is_empty());
    assert!(!comparison.failed());
    // A percentage alone (floor 0): memchr's +12.5% now counts.
    let strict = Thresholds::SIZES
        .with(Threshold::Bytes(0), Quantity::Bytes)
        .unwrap();
    let comparison = Comparison::sizes(&old, &new, strict);
    assert!(comparison.regressions().iter().any(|r| r.key == "memchr"));
}

#[test]
fn timings_compare_with_a_saved_baseline() {
    let current = Timings::parse(&fixture("supply-chain/cargo-timing.html")).unwrap();
    let baseline = Timings::parse(&fixture("deps/timings-baseline.json")).unwrap();
    let comparison = Comparison::timings(&baseline, &current, Thresholds::TIMINGS);
    let regressed: Vec<&str> = comparison
        .regressions()
        .iter()
        .map(|r| r.key.as_str())
        .collect();
    // syn is two units (two versions), compared as one.
    assert_eq!(regressed, ["syn", "thiserror-impl"]);
    assert_eq!(comparison.total().verdict, Verdict::Unchanged);
    check("timings_comparison", &plain(100, &comparison), 100);
    // Saved and read back, a report compares equal to itself.
    let saved = Timings::parse(&current.to_json()).unwrap();
    let same = Comparison::timings(&saved, &current, Thresholds::TIMINGS);
    assert_eq!(same.count(Verdict::Unchanged), same.rows().len());
}

#[cfg(feature = "toml")]
#[test]
fn a_rich_policy_from_toml() {
    use rich_ext::deps::policy::{Finding, Severity};
    use rich_ext::deps::sbom::SbomFormat;
    let policy = Policy::from_toml(&fixture("deps/policy.toml")).unwrap();
    assert!(policy.notes().is_empty(), "{:?}", policy.notes());
    let sbom = Sbom::parse(&fixture("deps/sbom.cdx.json")).unwrap();
    assert_eq!(sbom.format(), SbomFormat::CycloneDx);
    assert_eq!(sbom.spec_version(), "1.5");
    let report = PolicyReport::new(sbom.report(), &policy)
        .named("policy.toml")
        .root(sbom.root().unwrap());
    let violations: Vec<(&str, &Finding)> = report
        .violations()
        .iter()
        .map(|v| (v.name.as_str(), &v.finding))
        .collect();
    assert_eq!(
        violations,
        [
            (
                "gpl-widget",
                &Finding::Violation("GPL-3.0-only is denied (GPL-3.0)".into())
            ),
            (
                "mystery-codec",
                &Finding::Violation("not an SPDX expression".into())
            ),
        ]
    );
    let conflicts: Vec<(&str, Severity)> = report
        .conflicts()
        .iter()
        .map(|c| (c.name.as_str(), c.severity))
        .collect();
    assert_eq!(
        conflicts,
        [
            ("gpl-widget", Severity::Conflict),
            ("acme-telemetry", Severity::Check),
            ("option-ext", Severity::Check),
        ]
    );
    assert!(report.failed());
    check("policy", &plain(100, &report), 100);
}

#[cfg(feature = "toml")]
#[test]
fn cargo_deny_licenses_are_read() {
    use rich_ext::deps::sbom::SbomFormat;
    let policy = Policy::from_toml(&fixture("deps/deny.toml")).unwrap();
    assert_eq!(policy.format(), policy::PolicyFormat::CargoDeny);
    assert!(policy.ignores_private());
    assert_eq!(policy.notes().len(), 1, "{:?}", policy.notes());
    let sbom = Sbom::parse(&fixture("deps/sbom.spdx.json")).unwrap();
    assert_eq!(sbom.format(), SbomFormat::Spdx);
    assert_eq!(sbom.root().map(|r| r.name.as_str()), Some("ledger"));
    let report = PolicyReport::new(sbom.report(), &policy);
    let names: Vec<&str> = report
        .violations()
        .iter()
        .map(|v| v.name.as_str())
        .collect();
    // option-ext's MPL-2.0 is allowed by its exception; GPL is not allowed
    // (cargo-deny refuses what it is not told to allow).
    assert_eq!(names, ["gpl-widget", "mystery-codec"]);
}

#[cfg(feature = "toml")]
#[test]
fn odd_policies_are_errors_or_notes() {
    use rich_ext::deps::policy::Finding;
    for (text, expected) in [
        ("allow = [", "not TOML"),
        ("title = \"x\"", "not a licence policy"),
        ("allow = \"MIT\"", "not a list of strings"),
        ("allow = [1]", "not a list of strings"),
        ("licenses = 3", "not a table"),
        ("exceptions = [{ allow = [\"MIT\"] }]", "names no crate"),
        ("exceptions = [{ name = \"a\" }]", "has no `allow` list"),
        ("exceptions = [\"a\"]", "is not a table"),
        ("allow = []\nconflicts = \"maybe\"", "warn, deny or ignore"),
        ("[licenses]\nprivate = 1", "not a table"),
    ] {
        let err = Policy::from_toml(text).unwrap_err().to_string();
        assert!(err.contains(expected), "{text:?}: {err}");
    }
    let policy = Policy::from_toml(
        "allow = [\"MIT\", \"MIT OR Apache-2.0\", \"Not A Licence\"]\n\
         colour = \"blue\"\n\
         exceptions = [{ name = \"x\", version = \"banana\", allow = [\"ISC\"] }]",
    )
    .unwrap();
    assert_eq!(policy.notes().len(), 4, "{:?}", policy.notes());
    // A deny list alone allows the rest; cargo-deny's shape allows nothing
    // it is not told to.
    let deny_only = Policy::from_toml("deny = [\"GPL-3.0\"]").unwrap();
    let cargo_deny = Policy::from_toml("[licenses]\ndeny = [\"GPL-3.0\"]").unwrap();
    let zlib = LicensedPackage {
        id: "z 1.0.0".into(),
        name: "z".into(),
        version: "1.0.0".into(),
        license: Some("Zlib".into()),
        license_file: None,
    };
    assert_eq!(deny_only.check(&zlib), Finding::Allowed);
    assert!(matches!(cargo_deny.check(&zlib), Finding::Violation(_)));
}

#[test]
fn conflicts_need_a_readable_licence() {
    let packages = vec![
        LicensedPackage {
            id: "root".into(),
            name: "root".into(),
            version: "1.0.0".into(),
            license: Some("Apache-2.0 OR MIT".into()),
            license_file: None,
        },
        LicensedPackage {
            id: "a".into(),
            name: "a".into(),
            version: "1.0.0".into(),
            license: None,
            license_file: Some("COPYING".into()),
        },
        LicensedPackage {
            id: "b".into(),
            name: "b".into(),
            version: "1.0.0".into(),
            license: Some("AGPL-3.0-or-later".into()),
            license_file: None,
        },
    ];
    let found = policy::conflicts(&packages[0], &packages);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "b");
    assert_eq!(found[0].root, "root v1.0.0");
    let report =
        PolicyReport::new(&LicenseReport::new(packages.clone()), &Policy::new()).root(&packages[0]);
    // Conflicts warn by default: the check passes.
    assert!(!report.failed());
    let strict = Policy::new().conflicts(policy::ConflictMode::Deny);
    let report =
        PolicyReport::new(&LicenseReport::new(packages.clone()), &strict).root(&packages[0]);
    assert!(report.failed());
}

#[test]
fn hostile_text_in_reports_is_shown_not_run() {
    let bloat = r#"{"crates": [{"name": "evil\u001b]0;pwned\u0007", "size": 100000}]}"#;
    let out = plain(80, &SizeReport::new(Sizes::parse(bloat).unwrap()));
    assert!(!out.contains('\u{1b}'), "{out:?}");
    let sbom = r#"{"bomFormat": "CycloneDX", "components": [
        {"name": "x\u001b[2J", "version": "1", "licenses": [{"expression": "GPL-3.0\u001b[2J"}]}]}"#;
    let sbom = Sbom::parse(sbom).unwrap();
    let report = PolicyReport::new(sbom.report(), &Policy::new().allow(["MIT"]));
    let out = plain(100, &report);
    assert!(!out.contains('\u{1b}'), "{out:?}");
    let old = Sizes::parse(bloat).unwrap();
    let out = plain(
        100,
        &Comparison::sizes(&old, &Sizes::new(Vec::new()), Thresholds::SIZES),
    );
    assert!(!out.contains('\u{1b}'), "{out:?}");
}
