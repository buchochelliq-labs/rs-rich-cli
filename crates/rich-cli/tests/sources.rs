//! Diagrams from real sources (0.0.15 workstream 4): `rich dot` (and `.dot`
//! detection, ```dot fences, `--dot-backend`, and the rule that a project's
//! `rich.toml` cannot run Graphviz), `rich deps` and `rich schema`, each from
//! a fixture.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path)
}

fn dot(name: &str) -> String {
    fixture(&format!("rich-diagram/tests/fixtures/dot/{name}"))
        .display()
        .to_string()
}

fn source(name: &str) -> String {
    fixture(&format!("rich-ext/tests/fixtures/sources/{name}"))
        .display()
        .to_string()
}

/// Run `rich` in `dir` with a separate home, plain output 80 columns wide.
fn run_in(dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(dir)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "80")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let home = root.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(home.join(".config/rich")).unwrap();
    (root, work, home)
}

fn run(args: &[&str]) -> Output {
    let (_root, work, home) = setup();
    run_in(&work, &home, args)
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn dot_files_draw_through_the_command_its_alias_and_detection() {
    let file = dot("services.dot");
    for args in [
        &["dot", file.as_str()][..],
        &["graphviz", file.as_str()],
        &[file.as_str()],
    ] {
        let out = stdout(&run(args));
        assert!(out.contains("│ Browser ├─┘"), "{args:?}: {out}");
        assert!(
            out.contains("DOT: clusters are drawn without their frames: Backend (api, db)"),
            "{args:?}: {out}"
        );
    }
}

#[test]
fn unsupported_dot_fails_naming_the_construct_and_line() {
    for (file, expected) in [
        ("port.dot", "line 3: a node port (`a:…`) is not supported"),
        (
            "html_label.dot",
            "line 2: an HTML-like label is not supported",
        ),
        ("record.dot", "line 2: the `record` shape is not supported"),
    ] {
        let out = run(&["dot", &dot(file)]);
        assert_eq!(out.status.code(), Some(4), "{file}");
        assert!(out.stdout.is_empty(), "{file} drew something");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(expected), "{file}: {stderr}");
    }
    let out = run(&["dot", &dot("port.dot"), "--report", "json"]);
    let report = String::from_utf8_lossy(&out.stderr);
    assert!(
        report.contains("\"code\":\"data\"") && report.contains("line 3"),
        "{report}"
    );
}

#[test]
fn dot_fences_in_markdown_draw_unless_turned_off() {
    let (_root, work, home) = setup();
    std::fs::write(
        work.join("doc.md"),
        "# Graph\n\n```dot\ndigraph { rankdir=LR; a -> b }\n```\n",
    )
    .unwrap();
    let out = stdout(&run_in(&work, &home, &["doc.md"]));
    assert!(out.contains("│ a ├─►│ b │"), "{out}");
    let off = stdout(&run_in(&work, &home, &["doc.md", "--dot-backend", "off"]));
    assert!(off.contains("rankdir=LR"), "{off}");
    assert!(!off.contains("├─►"), "{off}");
}

#[test]
fn a_project_config_cannot_run_graphviz() {
    let (_root, work, home) = setup();
    std::fs::write(
        work.join("rich.toml"),
        "[defaults]\ndot_backend = \"graphviz\"\n",
    )
    .unwrap();
    let out = run_in(&work, &home, &["dot", &dot("undirected.dot")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("dot_backend = \"graphviz\" in ./rich.toml is ignored"),
        "{stderr}"
    );
    assert!(out.status.success(), "{stderr}");
    let explain = run_in(&work, &home, &["config", "explain", "dot_backend"]);
    let text = String::from_utf8_lossy(&explain.stdout);
    assert!(text.contains("note: dot_backend"), "{text}");
}

/// `--dot-backend graphviz` writes Graphviz's own SVG on `--export-svg`; a
/// missing `dot` falls back to the text drawing's SVG with a warning.
#[test]
fn graphviz_svg_export_falls_back_without_dot() {
    let (_root, work, home) = setup();
    let svg = work.join("out.svg");
    let out = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("PATH", work.join("no-bin"))
        .env("COLUMNS", "80")
        .args([
            "dot",
            &dot("undirected.dot"),
            "--dot-backend",
            "graphviz",
            "--export-svg",
            svg.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("`dot` is not installed"), "{stderr}");
    let written = std::fs::read_to_string(&svg).unwrap();
    assert!(written.contains("<svg"), "{written}");
}

#[cfg(unix)]
#[test]
fn graphviz_svg_export_uses_the_dot_program() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, work, home) = setup();
    let bin = work.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("dot"),
        "#!/bin/sh\ncat >/dev/null\nprintf '<svg id=\"graphviz\"/>'\n",
    )
    .unwrap();
    std::fs::set_permissions(bin.join("dot"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let svg = work.join("out.svg");
    // Graphviz draws ports the native parser refuses: the SVG is written,
    // and the terminal says the text drawing was not.
    let out = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("PATH", &bin)
        .args([
            "dot",
            &dot("port.dot"),
            "--dot-backend",
            "graphviz",
            "--export-svg",
            svg.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains("only Graphviz's SVG was written"),
        "{stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(&svg).unwrap(),
        "<svg id=\"graphviz\"/>"
    );
}

#[test]
fn deps_draws_the_tree_from_metadata() {
    let metadata = source("cargo-metadata.json");
    let out = stdout(&run(&["deps", "--metadata", &metadata]));
    assert!(
        out.starts_with("demo-app v0.3.0\n├── demo-core v0.3.0\n"),
        "{out}"
    );
    assert!(out.contains("syn v1.0.109 (duplicate)"), "{out}");
    assert!(out.contains("[build-dependencies]"), "{out}");
    assert!(out.contains("serde v1.0.210 (*)"), "{out}");
    assert!(out.contains("duplicate: syn v1.0.109, v2.0.79"), "{out}");

    let out = stdout(&run(&[
        "deps",
        "--metadata",
        &metadata,
        "--depth",
        "1",
        "--no-dev",
    ]));
    assert!(
        !out.contains("serde_derive") && !out.contains("insta"),
        "{out}"
    );
}

#[test]
fn deps_why_and_graph() {
    let metadata = source("cargo-metadata.json");
    let out = stdout(&run(&[
        "deps",
        "--metadata",
        &metadata,
        "--why",
        "syn@1.0.109",
    ]));
    assert_eq!(
        out,
        "syn v1.0.109 (duplicate)\n└── strum_macros v0.25.3\n    └── demo-core v0.3.0\n"
    );
    let out = stdout(&run(&[
        "deps",
        "--metadata",
        &metadata,
        "--why",
        "syn@1.0.109",
        "--graph",
    ]));
    assert!(out.contains("│ strum_macros v0.25.3  │"), "{out}");
    assert!(
        out.contains("╱───────────────╲"),
        "duplicates are hexagons: {out}"
    );
    let out = stdout(&run(&[
        "deps",
        "--metadata",
        &metadata,
        "--graph",
        "--depth",
        "1",
        "-w",
        "200",
    ]));
    assert!(
        out.contains("demo-app v0.3.0") && out.contains("┄build┄"),
        "{out}"
    );

    let missing = run(&["deps", "--metadata", &metadata, "--why", "tokio"]);
    assert_eq!(missing.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no package \"tokio\""));
}

#[test]
fn deps_why_no_dev_and_graph_duplicates_honour_their_flags() {
    let metadata = source("cargo-metadata.json");
    // `insta` reaches serde only as demo-app's dev dependency.
    let why = ["deps", "--metadata", &metadata, "--why", "serde"];
    let out = stdout(&run(&why));
    assert!(out.contains("insta v1.40.0"), "{out}");
    let out = stdout(&run(&[&why[..], &["--no-dev"]].concat()));
    assert!(
        out.contains("demo-app v0.3.0") && !out.contains("insta"),
        "{out}"
    );
    let out = stdout(&run(
        &[&why[..], &["--no-dev", "--graph", "-w", "200"]].concat()
    ));
    assert!(
        out.contains("demo-app v0.3.0") && !out.contains("insta"),
        "{out}"
    );

    // `--graph --duplicates`: only what leads to syn's two versions.
    let out = stdout(&run(&[
        "deps",
        "--metadata",
        &metadata,
        "--graph",
        "--duplicates",
        "-w",
        "200",
    ]));
    assert!(
        out.contains("strum_macros v0.25.3") && out.contains("insta v1.40.0"),
        "{out}"
    );
    assert!(
        !out.contains("log v0.4.22") && !out.contains("cc v1.1.28") && !out.contains("similar"),
        "{out}"
    );
}

#[test]
fn deps_runs_cargo_metadata_and_reports_its_failure() {
    let (_root, work, home) = setup();
    // No Cargo.toml here: cargo fails, and rich says so.
    let out = run_in(&work, &home, &["deps"]);
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cargo metadata"), "{stderr}");
    // A real manifest: this crate's.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("rich-plugin-api");
    let out = run_in(
        &work,
        &home,
        &["deps", manifest.to_str().unwrap(), "--depth", "1"],
    );
    let text = stdout(&out);
    assert!(text.starts_with("rs-rich-plugin-api v"), "{text}");
}

#[test]
fn deps_options_are_for_deps_only() {
    let out = run(&["--why", "x", "file.txt"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--why only has an effect with `rich deps`")
    );
}

#[test]
fn schema_draws_a_tree_and_a_diff() {
    let v1 = source("order-v1.schema.json");
    let v2 = source("order-v2.schema.json");
    let out = stdout(&run(&["schema", &v1]));
    assert!(
        out.starts_with("Order  object  no other properties"),
        "{out}"
    );
    assert!(
        out.contains("├── id (required)  string  pattern=/^ord_[a-z0-9]+$/"),
        "{out}"
    );
    assert!(
        out.contains("referrer  object  → #/$defs/customer (recursive)"),
        "{out}"
    );

    let out = stdout(&run(&["schema", &v1, &v2]));
    assert!(
        out.contains("│ ~ │ currency      │ became required"),
        "{out}"
    );
    assert!(
        out.contains("order-v1.schema.json → order-v2.schema.json: 9 changes, 5 breaking"),
        "{out}"
    );

    let bad = run(&["schema", &dot("services.dot")]);
    assert_eq!(bad.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("not JSON"));
    let three = run(&["schema", &v1, &v2, &v1]);
    assert_eq!(three.status.code(), Some(2));
}

#[test]
fn the_new_commands_have_help() {
    for command in ["dot", "deps", "schema"] {
        let out = stdout(&run(&[command, "--help"]));
        assert!(out.contains(&format!("rich {command}")), "{command}: {out}");
    }
    let out = stdout(&run(&["deps", "--help"]));
    assert!(out.contains("--why") && out.contains("--metadata"), "{out}");
}

#[test]
fn deps_why_says_when_the_chosen_kinds_never_reach_the_crate() {
    let metadata = source("cargo-metadata.json");
    // `insta` is only demo-app's dev dependency.
    let out = run(&[
        "deps",
        "--metadata",
        &metadata,
        "--why",
        "insta",
        "--no-dev",
    ]);
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("insta is reached only through dev-dependencies"),
        "{stderr}"
    );
    assert!(out.stdout.is_empty(), "{out:?}");
    // With dev dependencies it is found.
    let out = stdout(&run(&["deps", "--metadata", &metadata, "--why", "insta"]));
    assert!(out.starts_with("insta v1.40.0"), "{out}");
}

/// `cargo metadata` honours the project's `.cargo/config.toml`; `rich deps`
/// turns off its `rustc` wrappers, which would otherwise run.
#[cfg(unix)]
#[test]
fn deps_does_not_run_the_projects_rustc_wrappers() {
    use std::os::unix::fs::PermissionsExt;
    let (root, work, home) = setup();
    let project = root.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join(".cargo")).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"wrapped\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    let marker = root.path().join("wrapper-ran");
    let wrapper = project.join("wrapper.sh");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\ntouch '{}'\nexec \"$@\"\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        project.join(".cargo/config.toml"),
        format!(
            "[build]\nrustc-wrapper = '{0}'\nrustc-workspace-wrapper = '{0}'\n",
            wrapper.display()
        ),
    )
    .unwrap();
    // Cargo reads `.cargo/config.toml` from the directory it runs in.
    let _ = work;
    let out = run_in(&project, &home, &["deps"]);
    let text = stdout(&out);
    assert!(text.starts_with("wrapped v0.1.0"), "{text}");
    assert!(!marker.exists(), "the project's rustc wrapper ran");
}
