//! `rich deps`' supply-chain reports (0.0.16 workstream 6): the duplicate
//! consolidation under `--duplicates`, `--features`, `--timings FILE`,
//! `--audit FILE` (exit 5 on a vulnerability) and `--licenses`, each from a
//! fixture in `rich-ext/tests/fixtures/supply-chain`. None runs Cargo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rich-ext/tests/fixtures/supply-chain")
        .join(name)
        .display()
        .to_string()
}

/// Run `rich` in an empty directory with a separate home (so no project
/// config applies, and no Cargo.toml is there), plain output 100 columns
/// wide, with `stdin` on standard input.
fn run_with(args: &[&str], stdin: &str) -> Output {
    let root = tempfile::tempdir().unwrap();
    let work: PathBuf = root.path().join("work");
    let home = root.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(home.join(".config/rich")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "100")
        // No cargo to fall back on: a report that tried to run it fails.
        .env("CARGO", root.path().join("no-cargo"))
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let mut input = child.stdin.take().unwrap();
        input.write_all(stdin.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn run(args: &[&str]) -> Output {
    run_with(args, "")
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn duplicates_add_the_consolidation_summary() {
    let metadata = fixture("cargo-metadata.json");
    let out = stdout(&run(&["deps", "--metadata", &metadata, "--duplicates"]));
    assert!(
        out.contains(
            "duplicate: syn v1.0.109, v2.0.79\n\n\
             consolidation: 1 crate at several versions\n\
             syn: 2 versions, 3 dependents; 2 use v2.0.79 (newest)\n"
        ),
        "{out}"
    );
    assert!(
        out.contains("└── v1.0.109 ← strum_macros v0.25.3 (could move to v2.0.79)"),
        "{out}"
    );
    // Without --duplicates the tree ends with the one-line summary, as before.
    let out = stdout(&run(&["deps", "--metadata", &metadata]));
    assert!(out.ends_with("duplicate: syn v1.0.109, v2.0.79\n"), "{out}");
}

#[test]
fn features_show_one_crate_or_every_crate_with_one() {
    let metadata = fixture("cargo-metadata.json");
    let out = stdout(&run(&[
        "deps",
        "--metadata",
        &metadata,
        "--features",
        "--package",
        "syn@2.0.79",
    ]));
    assert!(
        out.starts_with("syn v2.0.79  5 features enabled\n"),
        "{out}"
    );
    assert!(
        out.contains("│   └── quote?/proc-macro → quote v1.0.37"),
        "{out}"
    );
    assert!(
        out.contains("serde_derive v1.0.210: derive, parsing, printing, proc-macro"),
        "{out}"
    );
    let out = stdout(&run(&["deps", "--metadata", &metadata, "--features"]));
    assert!(
        out.starts_with("demo-app v0.3.0  2 features enabled"),
        "{out}"
    );
    assert!(out.contains("\nsyn v1.0.109  7 features enabled"), "{out}");

    let missing = run(&[
        "deps",
        "--metadata",
        &metadata,
        "--features",
        "--package",
        "tokio",
    ]);
    assert_eq!(missing.status.code(), Some(4));
    assert!(stderr(&missing).contains("no package \"tokio\""));
    let alone = run(&["deps", "--metadata", &metadata, "--package", "syn"]);
    assert_eq!(alone.status.code(), Some(2));
    assert!(stderr(&alone).contains("--package only has an effect with --features"));
}

#[test]
fn timings_read_the_html_report_without_cargo() {
    let out = stdout(&run(&["deps", "--timings", &fixture("cargo-timing.html")]));
    assert!(
        out.starts_with("12 units: 24.82s of compile time, 16.50s wall clock\n"),
        "{out}"
    );
    assert!(out.contains("syn v2.0.79 "), "{out}");
    assert!(
        out.contains("│ syn                            │ v2.0.79  │ 6.84s │    3.95s │   2.89s │"),
        "{out}"
    );
    // From stdin too.
    let html = std::fs::read_to_string(fixture("cargo-timing.html")).unwrap();
    let out = stdout(&run_with(&["deps", "--timings", "-"], &html));
    assert!(out.starts_with("12 units"), "{out}");

    let bad = run_with(&["deps", "--timings", "-"], "not a report");
    assert_eq!(bad.status.code(), Some(4));
    assert!(stderr(&bad).contains("no timing data"), "{}", stderr(&bad));
    let missing = run(&["deps", "--timings", "missing.html"]);
    assert_eq!(missing.status.code(), Some(3));
}

#[test]
fn audit_groups_by_severity_and_gates_on_vulnerabilities() {
    let out = run(&["deps", "--audit", &fixture("cargo-audit.json")]);
    // The report is shown, then the gate fails: exit 5.
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(
        text.starts_with(
            "2 vulnerabilities, 2 warnings in 16 dependencies\n\
             1 high · 1 unknown · 2 informational\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("│ high 7.5     │ RUSTSEC-2099-0001 │"),
        "{text}"
    );
    assert!(
        stderr(&out).contains("2 vulnerabilities found: RUSTSEC-2099-0001, RUSTSEC-2099-0002"),
        "{}",
        stderr(&out)
    );

    // `--report json`: one envelope, the gate's.
    let out = run(&[
        "deps",
        "--audit",
        &fixture("cargo-audit.json"),
        "--report",
        "json",
    ]);
    assert_eq!(out.status.code(), Some(5));
    let envelopes: Vec<serde_json::Value> = stderr(&out)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    assert_eq!(envelopes.len(), 1, "{}", stderr(&out));
    assert_eq!(envelopes[0]["code"], "gate");

    // Warnings alone do not fail.
    let warnings_only = r#"{"lockfile": {"dependency-count": 3},
        "vulnerabilities": {"found": false, "count": 0, "list": []},
        "warnings": {"yanked": [{"kind": "yanked",
            "package": {"name": "cc", "version": "1.1.28"}, "advisory": null}]}}"#;
    let out = stdout(&run_with(&["deps", "--audit", "-"], warnings_only));
    assert!(
        out.starts_with("0 vulnerabilities, 1 warning in 3 dependencies"),
        "{out}"
    );

    let bad = run_with(&["deps", "--audit", "-"], "[]");
    assert_eq!(bad.status.code(), Some(4));
}

#[test]
fn licenses_group_and_mark_and_honour_no_dev() {
    let metadata = fixture("cargo-metadata.json");
    let out = stdout(&run(&["deps", "--metadata", &metadata, "--licenses"]));
    assert!(
        out.starts_with(
            "16 crates under 7 licences: 1 copyleft, 1 licence file only, 1 no licence\n"
        ),
        "{out}"
    );
    assert!(out.contains("│ GPL-3.0-or-later "), "{out}");
    assert!(out.contains("copyleft"), "{out}");
    // insta and similar are only reached as a dev-dependency.
    let out = stdout(&run(&[
        "deps",
        "--metadata",
        &metadata,
        "--licenses",
        "--no-dev",
    ]));
    assert!(out.starts_with("14 crates under 6 licences"), "{out}");
    assert!(!out.contains("insta"), "{out}");
}

#[test]
fn report_options_refuse_what_does_not_apply() {
    let metadata = fixture("cargo-metadata.json");
    let timings = fixture("cargo-timing.html");
    for (args, message) in [
        (
            vec!["deps", "--metadata", &metadata, "--licenses", "--features"],
            "give one of --features, --licenses at a time",
        ),
        (
            vec!["deps", "--timings", &timings, "--metadata", &metadata],
            "--metadata has no effect with --timings",
        ),
        (
            vec!["deps", "--metadata", &metadata, "--licenses", "--graph"],
            "--graph has no effect with --licenses",
        ),
        (
            vec!["deps", "--metadata", &metadata, "--features", "--no-dev"],
            "--no-dev has no effect with --features",
        ),
        (
            vec!["--audit", &timings, "README.md"],
            "--audit only has an effect with `rich deps`",
        ),
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(stderr(&out).contains(message), "{args:?}: {}", stderr(&out));
    }
}

#[test]
fn deps_help_lists_the_reports() {
    let out = stdout(&run(&["deps", "--help"]));
    for flag in [
        "--features",
        "--package",
        "--timings",
        "--audit",
        "--licenses",
    ] {
        assert!(out.contains(flag), "{flag}: {out}");
    }
}
