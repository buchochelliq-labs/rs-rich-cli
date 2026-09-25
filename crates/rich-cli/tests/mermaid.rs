//! Mermaid diagrams (#222): `rich mermaid`, `.mmd` detection, ```mermaid
//! fences in Markdown, `--mermaid-backend`, and the rule that a project's
//! `rich.toml` cannot start a browser.
#![cfg(feature = "mermaid")]

use std::path::Path;
use std::process::{Command, Output};

/// Run `rich` in `dir` with a separate home, plain output 60 columns wide.
fn run_in(dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(dir)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "60")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let home = root.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(home.join(".config/rich")).unwrap();
    std::fs::write(work.join("flow.mmd"), "graph LR\n  A --> B\n").unwrap();
    std::fs::write(
        work.join("doc.md"),
        "# Doc\n\n```mermaid\ngraph LR\n  A --> B\n```\n",
    )
    .unwrap();
    (root, work, home)
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

const DRAWN: &str = "│ A ├─►│ B │";

#[test]
fn the_command_its_alias_and_mmd_files_draw_the_diagram() {
    let (_root, work, home) = setup();
    for args in [
        &["mermaid", "flow.mmd"][..],
        &["mmd", "flow.mmd"],
        &["flow.mmd"],
    ] {
        let out = stdout(&run_in(&work, &home, args));
        assert!(out.contains(DRAWN), "{args:?}: {out}");
    }
    // A diagram the text renderer cannot draw shows its source under a note.
    std::fs::write(work.join("seq.mmd"), "sequenceDiagram\n  A->>B: hi\n").unwrap();
    let out = stdout(&run_in(
        &work,
        &home,
        &["seq.mmd", "--mermaid-backend", "text"],
    ));
    assert!(
        out.starts_with("Mermaid: sequence diagrams are not drawn as text"),
        "{out}"
    );
    assert!(out.contains("A->>B: hi"), "{out}");
}

#[test]
fn markdown_fences_draw_unless_the_backend_is_off() {
    let (_root, work, home) = setup();
    let drawn = stdout(&run_in(&work, &home, &["doc.md"]));
    assert!(drawn.contains(DRAWN), "{drawn}");
    let off = stdout(&run_in(
        &work,
        &home,
        &["doc.md", "--mermaid-backend", "off"],
    ));
    assert!(!off.contains(DRAWN) && off.contains("A --> B"), "{off}");
    // A document without a Mermaid fence renders the same either way.
    std::fs::write(work.join("plain.md"), "# T\n\n```rust\nfn x() {}\n```\n").unwrap();
    assert_eq!(
        stdout(&run_in(&work, &home, &["plain.md"])),
        stdout(&run_in(
            &work,
            &home,
            &["plain.md", "--mermaid-backend", "off"]
        ))
    );
}

#[test]
fn a_bad_backend_is_a_usage_error() {
    let (_root, work, home) = setup();
    let out = run_in(&work, &home, &["flow.mmd", "--mermaid-backend", "dot"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unknown Mermaid backend \"dot\""),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(not(feature = "mmdc"))]
#[test]
fn mmdc_needs_a_build_with_it() {
    let (_root, work, home) = setup();
    let out = run_in(&work, &home, &["flow.mmd", "--mermaid-backend", "mmdc"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("needs a build with the mmdc feature"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A project's `rich.toml` cannot choose mmdc: every Markdown file would start
/// a browser. The setting is dropped with a warning and explained; other
/// values are allowed, and the user's own config may choose it.
#[test]
fn a_project_config_cannot_choose_mmdc() {
    let (_root, work, home) = setup();
    std::fs::write(
        work.join("rich.toml"),
        "[defaults]\nmermaid_backend = \"mmdc\"\n",
    )
    .unwrap();
    let out = run_in(&work, &home, &["doc.md"]);
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        err.contains("mermaid_backend = \"mmdc\" in ./rich.toml is ignored"),
        "{err}"
    );
    assert!(stdout(&out).contains(DRAWN));
    let explain = stdout(&run_in(
        &work,
        &home,
        &["config", "explain", "mermaid_backend"],
    ));
    assert!(explain.contains("may not start a browser"), "{explain}");

    std::fs::write(
        work.join("rich.toml"),
        "[defaults]\nmermaid_backend = \"off\"\n",
    )
    .unwrap();
    let off = run_in(&work, &home, &["doc.md"]);
    assert!(
        off.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&off.stderr)
    );
    assert!(!stdout(&off).contains(DRAWN));
}

#[cfg(not(feature = "mmdc"))]
#[test]
fn the_users_own_config_may_choose_mmdc() {
    // Trusted, so it is honoured, and this build reports that it lacks mmdc.
    let (_root, work, home) = setup();
    std::fs::write(
        home.join(".config/rich/config.toml"),
        "[defaults]\nmermaid_backend = \"mmdc\"\n",
    )
    .unwrap();
    let out = run_in(&work, &home, &["doc.md"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("needs a build with the mmdc feature"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
