//! `rich deps --sizes`, `--baseline` and `--policy` (0.0.19 workstream 16,
//! #706, #704), from the fixtures in `rich-ext/tests/fixtures/deps` and a
//! fake target directory. None runs Cargo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rich-ext/tests/fixtures")
        .join(name)
        .display()
        .to_string()
}

/// Run `rich` in `work` with a separate home (so no project config applies),
/// plain output 100 columns wide, with `stdin` on standard input.
fn run_in(work: &Path, args: &[&str], stdin: &str) -> Output {
    let home = work.join("home");
    std::fs::create_dir_all(home.join(".config/rich")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "100")
        // No cargo to fall back on: a report that tried to run it fails.
        .env("CARGO", work.join("no-cargo"))
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

fn run_with(args: &[&str], stdin: &str) -> Output {
    let root = tempfile::tempdir().unwrap();
    run_in(root.path(), args, stdin)
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
fn sizes_from_cargo_bloat_json() {
    let out = stdout(&run(&[
        "deps",
        "--sizes",
        &fixture("deps/bloat.json"),
        "--top",
        "3",
    ]));
    assert!(
        out.starts_with("17 crates: 1.9 MiB of code in a 5.2 MiB binary (2.0 MiB of .text)\n"),
        "{out}"
    );
    assert!(out.contains("regex_automata  392.0 KiB ███"), "{out}");
    assert!(out.contains("and 14 more, 862.0 KiB in all"), "{out}");
    // From stdin too.
    let json = std::fs::read_to_string(fixture("deps/bloat.json")).unwrap();
    let out = stdout(&run_with(&["deps", "--sizes", "-"], &json));
    assert!(out.starts_with("17 crates"), "{out}");
}

fn write(path: &Path, bytes: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; bytes]).unwrap();
}

/// A workspace with a target directory: `app` (a binary) depending on
/// `serde`, and on `insta` as a dev-dependency. Returns the work directory
/// and the path of its `cargo metadata` file.
fn fake_build(root: &Path) -> PathBuf {
    let work = root.join("ws");
    let target = work.join("target");
    let package = |name: &str, kind: &str| {
        format!(
            r#"{{"id": "{name} 1.0.0", "name": "{name}", "version": "1.0.0", "source": null,
                "manifest_path": "{}",
                "targets": [{{"name": "{name}", "kind": ["{kind}"]}}]}}"#,
            work.join(name).join("Cargo.toml").display()
        )
    };
    let metadata = format!(
        r#"{{"packages": [{}, {}, {}], "workspace_members": ["app 1.0.0"],
            "resolve": {{"root": "app 1.0.0", "nodes": [
              {{"id": "app 1.0.0", "deps": [
                {{"pkg": "serde 1.0.0", "dep_kinds": [{{"kind": null}}]}},
                {{"pkg": "insta 1.0.0", "dep_kinds": [{{"kind": "dev"}}]}}]}},
              {{"id": "serde 1.0.0", "deps": []}}, {{"id": "insta 1.0.0", "deps": []}}]}},
            "target_directory": "{}"}}"#,
        package("app", "bin"),
        package("serde", "lib"),
        package("insta", "lib"),
        target.display()
    );
    for name in ["app", "serde", "insta"] {
        write(&work.join(name).join("src/lib.rs"), 3000);
    }
    write(
        &target.join("debug/deps/libserde-0123456789abcdef.rlib"),
        300_000,
    );
    write(
        &target.join("debug/deps/libinsta-0123456789abcdef.rlib"),
        200_000,
    );
    write(&target.join("debug/app"), 900_000);
    write(
        &target.join("release/deps/libserde-fedcba9876543210.rlib"),
        100_000,
    );
    write(&target.join("release/app"), 400_000);
    let path = root.join("metadata.json");
    std::fs::write(&path, metadata).unwrap();
    path
}

#[test]
fn sizes_from_a_target_directory() {
    let root = tempfile::tempdir().unwrap();
    let metadata = fake_build(root.path());
    let metadata = metadata.to_str().unwrap();
    let target = root.path().join("ws/target");
    let target = target.to_str().unwrap();
    // The target directory cargo metadata names, or the one given.
    for args in [
        vec!["deps", "--metadata", metadata, "--sizes"],
        vec!["deps", "--metadata", metadata, "--sizes", target],
    ] {
        let out = stdout(&run_in(root.path(), &args, ""));
        assert!(out.starts_with("3 artifacts: 1.3 MiB in "), "{out}");
        assert!(out.contains("app (bin)"), "{out}");
        assert!(
            out.contains("│ serde │ v1.0.0  │ lib  │ 293.0 KiB │"),
            "{out}"
        );
    }
    let out = stdout(&run_in(
        root.path(),
        &[
            "deps",
            "--metadata",
            metadata,
            "--sizes",
            "--build-profile",
            "release",
        ],
        "",
    ));
    assert!(out.starts_with("2 artifacts: 488.3 KiB in "), "{out}");
    // --no-dev leaves out insta; --source-sizes adds a column.
    let out = stdout(&run_in(
        root.path(),
        &[
            "deps",
            "--metadata",
            metadata,
            "--sizes",
            "--no-dev",
            "--source-sizes",
        ],
        "",
    ));
    assert!(out.starts_with("2 artifacts: 1.1 MiB in "), "{out}");
    assert!(out.contains(", 5.9 KiB of source"), "{out}");
    assert!(!out.contains("insta"), "{out}");
    // No such profile: an input error.
    let out = run_in(
        root.path(),
        &[
            "deps",
            "--metadata",
            metadata,
            "--sizes",
            "--build-profile",
            "dist",
        ],
        "",
    );
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("no dist directory"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_baseline_flags_regressions_and_gates() {
    let bloat = fixture("deps/bloat.json");
    let baseline = fixture("deps/bloat-baseline.json");
    let out = run(&["deps", "--sizes", &bloat, "--baseline", &baseline]);
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(
        text.starts_with(
            "Sizes against the baseline (bloat-baseline.json → bloat.json)\n\
             2 regressions, 1 improvement, 13 unchanged, 1 new, 2 removed\n"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "│ regex_automata │ 236.0 KiB │ 392.0 KiB │ +156.0 KiB │  +66.1% │ regression  │"
        ),
        "{text}"
    );
    assert!(
        stderr(&out).contains(
            "3 regressions against the baseline (over 10% and over 64.0 KiB): toml_edit, \
             regex_automata, the total"
        ),
        "{}",
        stderr(&out)
    );
    // A higher floor: nothing regressed.
    let out = run(&[
        "deps",
        "--sizes",
        &bloat,
        "--baseline",
        &baseline,
        "--threshold",
        "300KiB",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    // A percentage and a floor, both given.
    let out = run(&[
        "deps",
        "--sizes",
        &bloat,
        "--baseline",
        &baseline,
        "--threshold",
        "70%",
        "--threshold",
        "0B",
    ]);
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("(over 70% and over 0 bytes): toml_edit, winnow"),
        "{}",
        stderr(&out)
    );

    // Build times, against a saved baseline.
    let timings = fixture("supply-chain/cargo-timing.html");
    let saved = fixture("deps/timings-baseline.json");
    let out = run(&["deps", "--timings", &timings, "--baseline", &saved]);
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(
            "2 regressions against the baseline (over 10% and over 0.50s): syn, thiserror-impl"
        ),
        "{}",
        stderr(&out)
    );
    let out = run(&[
        "deps",
        "--timings",
        &timings,
        "--baseline",
        &saved,
        "--threshold",
        "2s",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));

    // `--report json`: one envelope, the gate's.
    let out = run(&[
        "deps",
        "--sizes",
        &bloat,
        "--baseline",
        &baseline,
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
}

#[test]
fn a_saved_report_is_a_baseline() {
    let root = tempfile::tempdir().unwrap();
    let bloat = fixture("deps/bloat.json");
    let saved = root.path().join("sizes.json");
    let saved = saved.to_str().unwrap();
    let out = stdout(&run_in(
        root.path(),
        &["deps", "--sizes", &bloat, "--save", saved],
        "",
    ));
    assert!(out.starts_with("17 crates"), "{out}");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(saved).unwrap()).unwrap();
    assert_eq!(json["format"], "rich-deps");
    assert_eq!(json["report"], "sizes");
    // Compared with itself: unchanged.
    let out = stdout(&run_in(
        root.path(),
        &["deps", "--sizes", &bloat, "--baseline", saved],
        "",
    ));
    assert!(
        out.contains("0 regressions, 0 improvements, 17 unchanged"),
        "{out}"
    );
    // And shown again.
    let out = stdout(&run_in(
        root.path(),
        &["deps", "--sizes", saved, "--top", "2"],
        "",
    ));
    assert!(out.starts_with("17 crates: 1.9 MiB of code"), "{out}");
    // A saved timings report is read by --timings.
    let timings = root.path().join("timings.json");
    let timings = timings.to_str().unwrap();
    let html = fixture("supply-chain/cargo-timing.html");
    stdout(&run_in(
        root.path(),
        &["deps", "--timings", &html, "--save", timings],
        "",
    ));
    let out = stdout(&run_in(root.path(), &["deps", "--timings", timings], ""));
    assert!(out.starts_with("12 units: 24.82s of compile time"), "{out}");
    // But not by --sizes.
    let out = run_in(root.path(), &["deps", "--sizes", timings], "");
    assert_eq!(out.status.code(), Some(4));
    assert!(
        stderr(&out).contains("a saved timings report, not sizes"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_policy_reports_violations_and_gates() {
    let policy = fixture("deps/policy.toml");
    let cdx = fixture("deps/sbom.cdx.json");
    let out = run(&["deps", "--policy", &policy, "--sbom", &cdx]);
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(
        text.starts_with(
            "Licence policy (policy.toml): 10 crates checked, 2 violations, 1 conflict, 2 warnings\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("│ gpl-widget    │ v0.4.0  │ GPL-3.0-only           │ GPL-3.0-only is denied (GPL-3.0) │"),
        "{text}"
    );
    assert!(
        text.contains("Licences against ledger v2.1.0 (MIT)"),
        "{text}"
    );
    assert!(text.contains("a heuristic, not legal advice"), "{text}");
    assert!(
        stderr(&out).contains(
            "2 licence policy violations: gpl-widget v0.4.0 (GPL-3.0-only), mystery-codec \
             v0.1.0 (Mystery Public Licence)"
        ),
        "{}",
        stderr(&out)
    );

    // cargo-deny's deny.toml, over cargo metadata.
    let metadata = fixture("supply-chain/cargo-metadata.json");
    let out = run(&[
        "deps",
        "--metadata",
        &metadata,
        "--policy",
        &fixture("deps/deny.toml"),
    ]);
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(
        text.starts_with("Licence policy (deny.toml): 16 crates checked, 3 violations"),
        "{text}"
    );
    assert!(
        text.contains("note: `licenses.confidence-threshold` is not applied"),
        "{text}"
    );
    assert!(
        stderr(&out)
            .contains("demo-core v0.3.0 (a licence file), thiserror-impl v1.0.64 (no licence)"),
        "{}",
        stderr(&out)
    );

    // A policy everything passes; --licenses shows the groups first.
    let root = tempfile::tempdir().unwrap();
    let open = root.path().join("open.toml");
    std::fs::write(&open, "deny = [\"AGPL-3.0\"]\n").unwrap();
    let out = stdout(&run_in(
        root.path(),
        &[
            "deps",
            "--licenses",
            "--policy",
            open.to_str().unwrap(),
            "--sbom",
            &fixture("deps/sbom.spdx.json"),
        ],
        "",
    ));
    assert!(out.starts_with("6 crates under 6 licences"), "{out}");
    assert!(
        out.contains("Licence policy (open.toml): 6 crates checked, no violations"),
        "{out}"
    );
    assert!(
        out.contains("warning: mystery-codec v0.1.0: no licence"),
        "{out}"
    );
    // Conflicts fail it only when the policy says so.
    let strict = root.path().join("strict.toml");
    std::fs::write(&strict, "deny = [\"AGPL-3.0\"]\nconflicts = \"deny\"\n").unwrap();
    let out = run_in(
        root.path(),
        &[
            "deps",
            "--policy",
            strict.to_str().unwrap(),
            "--sbom",
            &fixture("deps/sbom.spdx.json"),
        ],
        "",
    );
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("1 licence conflict: gpl-widget v0.4.0 (GPL-3.0-only)"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_sbom_feeds_the_licence_report() {
    let out = stdout(&run(&[
        "deps",
        "--licenses",
        "--sbom",
        &fixture("deps/sbom.cdx.json"),
    ]));
    assert!(
        out.starts_with(
            "10 crates under 9 licences: 1 weak copyleft, 1 copyleft, 1 unknown licence"
        ),
        "{out}"
    );
    let spdx = std::fs::read_to_string(fixture("deps/sbom.spdx.json")).unwrap();
    let out = stdout(&run_with(&["deps", "--licenses", "--sbom", "-"], &spdx));
    assert!(out.starts_with("6 crates under 6 licences"), "{out}");
}

#[test]
fn malformed_input_is_a_clear_error() {
    let root = tempfile::tempdir().unwrap();
    let file = |name: &str, text: &str| {
        let path = root.path().join(name);
        std::fs::write(&path, text).unwrap();
        path.to_str().unwrap().to_string()
    };
    let truncated = file("truncated.json", "{\"crates\": [{\"name\": \"a\", \"si");
    let odd = file("odd.toml", "allow = \"MIT\"\n");
    let not_toml = file("bad.toml", "allow = [\n");
    let bom = file(
        "bom.xml",
        "<bom xmlns=\"http://cyclonedx.org/schema/bom/1.5\"/>",
    );
    let policy = fixture("deps/policy.toml");
    for (args, code, message) in [
        (
            vec!["deps", "--sizes", &truncated],
            4,
            "truncated.json: not cargo-bloat JSON",
        ),
        (
            vec!["deps", "--policy", &odd, "--sbom", &truncated],
            4,
            "odd.toml: `allow` is not a list of strings",
        ),
        (
            vec!["deps", "--policy", &not_toml, "--sbom", &truncated],
            4,
            "bad.toml: not TOML",
        ),
        (
            vec!["deps", "--policy", &policy, "--sbom", &truncated],
            4,
            "truncated.json: not SBOM JSON",
        ),
        (
            vec!["deps", "--licenses", "--sbom", &bom],
            4,
            "an XML document",
        ),
        (
            vec!["deps", "--sizes", "missing.json"],
            3,
            "cannot read missing.json",
        ),
        (
            vec!["deps", "--policy", "missing.toml", "--sbom", &bom],
            3,
            "cannot read missing.toml",
        ),
        (
            vec![
                "deps",
                "--sizes",
                &fixture("deps/bloat.json"),
                "--baseline",
                &truncated,
            ],
            4,
            "truncated.json",
        ),
    ] {
        let out = run_in(root.path(), &args, "");
        assert_eq!(out.status.code(), Some(code), "{args:?}: {}", stderr(&out));
        assert!(stderr(&out).contains(message), "{args:?}: {}", stderr(&out));
    }
}

#[test]
fn options_that_do_not_apply_are_refused() {
    let bloat = fixture("deps/bloat.json");
    let metadata = fixture("supply-chain/cargo-metadata.json");
    let cdx = fixture("deps/sbom.cdx.json");
    for (args, message) in [
        (
            vec!["deps", "--metadata", &metadata, "--sbom", &cdx],
            "--sbom only has an effect with --licenses or --policy",
        ),
        (
            vec![
                "deps",
                "--licenses",
                "--sbom",
                &cdx,
                "--metadata",
                &metadata,
            ],
            "--metadata has no effect with --sbom",
        ),
        (
            vec!["deps", "--licenses", "--sbom", &cdx, "--no-dev"],
            "--no-dev has no effect with --sbom",
        ),
        (
            vec!["deps", "--metadata", &metadata, "--baseline", &bloat],
            "--baseline only has an effect with --timings or --sizes",
        ),
        (
            vec!["deps", "--metadata", &metadata, "--save", "x.json"],
            "--save only has an effect with --timings or --sizes",
        ),
        (
            vec!["deps", "--metadata", &metadata, "--top", "3"],
            "--top only has an effect with --timings or --sizes",
        ),
        (
            vec!["deps", "--sizes", &bloat, "--threshold", "5%"],
            "--threshold only has an effect with --baseline",
        ),
        (
            vec![
                "deps",
                "--sizes",
                &bloat,
                "--baseline",
                &bloat,
                "--threshold",
                "0.5s",
            ],
            "a time threshold applies to build times, not sizes",
        ),
        (
            vec![
                "deps",
                "--sizes",
                &bloat,
                "--baseline",
                &bloat,
                "--threshold",
                "lots",
            ],
            "\"lots\" is not a threshold",
        ),
        (
            vec![
                "deps",
                "--metadata",
                &metadata,
                "--build-profile",
                "release",
            ],
            "--build-profile only has an effect with --sizes",
        ),
        (
            vec!["deps", "--metadata", &metadata, "--source-sizes"],
            "--source-sizes only has an effect with --sizes",
        ),
        (
            vec!["deps", "--sizes", &bloat, "--metadata", &metadata],
            "--metadata has no effect with a size report file",
        ),
        (
            vec!["deps", "--sizes", &bloat, "--build-profile", "release"],
            "--build-profile has no effect with a size report file",
        ),
        (
            vec!["deps", "--sizes", &bloat, "--save", "-"],
            "--save writes a file",
        ),
        (
            vec!["deps", "--sizes", &bloat, "--top", "0"],
            "--top requires a positive integer",
        ),
        (
            vec!["deps", "--sizes", &bloat, "--timings", &bloat],
            "give one of --timings, --sizes at a time",
        ),
        (
            vec![
                "deps",
                "--metadata",
                &metadata,
                "--policy",
                &bloat,
                "--features",
            ],
            "give one of --features, --policy at a time",
        ),
        (
            vec!["deps", "--sizes", "-", "--baseline", "-"],
            "only one of --sizes, --baseline can read standard input",
        ),
        (
            vec!["deps", "--policy", "-", "--sbom", "-"],
            "only one of --policy, --sbom can read standard input",
        ),
        (
            vec!["--sizes", &bloat, "README.md"],
            "--sizes only has an effect with `rich deps`",
        ),
        (
            vec!["--policy", &bloat, "README.md"],
            "--policy only has an effect with `rich deps`",
        ),
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(stderr(&out).contains(message), "{args:?}: {}", stderr(&out));
    }
    // A manifest after --sizes would be read as a report: say so.
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("Cargo.toml"), "[package]\n").unwrap();
    let out = run_in(root.path(), &["deps", "--sizes", "Cargo.toml"], "");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("give it before --sizes"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn shared_option_names_keep_their_other_meanings() {
    // `--top` is still `rich profile`'s, `--threshold` still `--diff`'s.
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("data.csv"), "a,b\nx,1\ny,2\n").unwrap();
    let out = run_in(root.path(), &["profile", "data.csv", "--top", "1"], "");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let out = run_in(root.path(), &["--threshold", "5%", "data.csv"], "");
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("invalid threshold"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn deps_help_lists_the_new_options() {
    let out = stdout(&run(&["deps", "--help"]));
    for flag in [
        "--sizes",
        "--build-profile",
        "--source-sizes",
        "--top",
        "--baseline",
        "--threshold",
        "--save",
        "--policy",
        "--sbom",
    ] {
        assert!(out.contains(flag), "{flag}: {out}");
    }
}
