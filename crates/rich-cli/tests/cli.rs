//! Integration tests for the `rich` CLI: exercise each render mode end to end.
//!
//! These drive the built binary (via `CARGO_BIN_EXE_rich`) and assert on plain
//! (`--no-color`) output, so they check argument routing and library wiring
//! without depending on exact ANSI bytes (that parity lives in the `rich` crate).

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rich"))
}

/// Run the CLI with `args`, feeding `stdin`, returning `(stdout, success)`.
fn run(args: &[&str], stdin: &str) -> (String, bool) {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rich");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait rich");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.success(),
    )
}

#[test]
fn version_flag() {
    let (out, ok) = run(&["--version"], "");
    assert!(ok);
    assert!(out.contains("rich (rs-rich-cli) 1.8.1"), "got: {out:?}");
}

#[test]
fn print_mode_renders_markup() {
    let (out, ok) = run(&["--no-color", "-p", "[bold]hi[/] there"], "");
    assert!(ok);
    assert_eq!(out, "hi there\n");
}

#[test]
fn markdown_mode_from_stdin() {
    let (out, ok) = run(
        &["--no-color", "--width", "20", "-m", "-"],
        "# Heading\n\ntext",
    );
    assert!(ok);
    assert!(out.contains("Heading"), "got: {out:?}");
    assert!(out.contains("text"), "got: {out:?}");
}

#[test]
fn json_mode_pretty_prints_from_stdin() {
    let (out, ok) = run(&["--no-color", "-j", "-"], r#"{"a": 1, "b": [2, 3]}"#);
    assert!(ok);
    // Pretty-printed with 2-space indent.
    assert!(out.contains("{\n  \"a\": 1"), "got: {out:?}");
}

#[test]
fn rule_mode_draws_title() {
    let (out, ok) = run(&["--no-color", "--width", "12", "--rule", "hi"], "");
    assert!(ok);
    assert!(out.contains("hi"), "got: {out:?}");
    assert!(out.contains('─'), "expected box rule chars, got: {out:?}");
}

#[test]
fn center_justifies_print_output() {
    let (out, ok) = run(&["--no-color", "-w", "9", "--center", "-p", "mid"], "");
    assert!(ok);
    assert_eq!(out, "   mid   \n");
}

#[test]
fn conflicting_modes_error() {
    let (_out, ok) = run(&["-m", "-j", "x"], "");
    assert!(!ok, "two mode flags should fail");
}
