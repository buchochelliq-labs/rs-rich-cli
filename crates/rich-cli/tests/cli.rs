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
    // Tracks the crate version, which is independent SemVer — deliberately NOT
    // the upstream rich-cli version (see AGENTS.md → Versioning).
    let expected = format!("rich (rs-rich-cli) {}", env!("CARGO_PKG_VERSION"));
    assert!(out.contains(&expected), "got: {out:?}, want: {expected:?}");
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

// --- Regressions found by UAT ------------------------------------------------
//
// Every test below reproduces a defect that shipped while the whole gate was
// green. They exist because the type checker, the linter and 274 other tests
// could not see any of them.

/// The image pair the diff tests run against, as absolute paths.
fn diff_fixtures() -> (String, String) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("rich-art/tests/fixtures");
    (
        dir.join("halo-before.png").display().to_string(),
        dir.join("halo-after.png").display().to_string(),
    )
}

/// `"NaN"` parses as an f32 and every comparison against it is false, so this
/// silently switched the CI gate off and reported a pass. A gate that can be
/// disabled by an empty template variable is worse than no gate at all.
#[test]
fn a_nonsense_threshold_is_rejected_rather_than_disabling_the_gate() {
    let (before, after) = diff_fixtures();
    for bad in ["NaN", "inf", "-1", "101", "1e3"] {
        let (_out, ok) = run(
            &[
                "--diff",
                &before,
                &after,
                "--image-mode",
                "none",
                "--threshold",
                bad,
            ],
            "",
        );
        assert!(
            !ok,
            "--threshold {bad} should be rejected, but the run succeeded"
        );
    }
}

/// A threshold inside the range must still gate normally.
#[test]
fn a_valid_threshold_still_gates() {
    let (before, after) = diff_fixtures();
    let (_out, over) = run(
        &[
            "--diff",
            &before,
            &after,
            "--image-mode",
            "none",
            "--threshold",
            "2",
        ],
        "",
    );
    assert!(!over, "5.4% change against a 2% limit must fail");
    let (_out, under) = run(
        &[
            "--diff",
            &before,
            &after,
            "--image-mode",
            "none",
            "--threshold",
            "90",
        ],
        "",
    );
    assert!(under, "5.4% change against a 90% limit must pass");
}

/// The gate compared full precision against a one-decimal display, so a limit
/// equal to the reported figure printed "FAIL 5.4% changed, limit 5.4%" — a
/// verdict contradicting itself, and unexplainable from the output.
#[test]
fn the_threshold_matches_the_percentage_it_prints() {
    let (before, after) = diff_fixtures();
    let (out, ok) = run(
        &[
            "--diff",
            &before,
            &after,
            "--image-mode",
            "none",
            "--threshold",
            "5.4",
        ],
        "",
    );
    assert!(
        ok,
        "a limit equal to the reported figure must not fail; got: {out}"
    );
}

/// Redirected output has no colour, and half-blocks without colour are a
/// rectangle of identical characters — 30 rows carrying no information.
#[test]
fn a_colourless_diff_renders_something_readable() {
    let (before, after) = diff_fixtures();
    let (out, ok) = run(&["--diff", &before, &after, "-w", "40", "--no-color"], "");
    assert!(ok);
    let picture: Vec<&str> = out
        .lines()
        .take_while(|line| !line.contains("of the canvas"))
        .filter(|line| !line.trim().is_empty())
        .collect();
    let distinct: std::collections::HashSet<_> = picture.iter().collect();
    assert!(
        distinct.len() > 1,
        "the picture collapsed to {} distinct row(s) — that is a solid block, not an image",
        distinct.len()
    );
}

/// Flags that decorate a single rendered resource were accepted and silently
/// dropped by `--diff`, which reads as the flag having no effect.
#[test]
fn decoration_flags_are_refused_with_diff_rather_than_ignored() {
    let (before, after) = diff_fixtures();
    for flag in [
        vec!["--panel", "rounded"],
        vec!["--padding", "2"],
        vec!["--center"],
    ] {
        let mut args = vec![
            "--diff",
            before.as_str(),
            after.as_str(),
            "--image-mode",
            "none",
        ];
        args.extend(flag.iter().copied());
        let (_out, ok) = run(&args, "");
        assert!(
            !ok,
            "{flag:?} with --diff should be an error, not a silent no-op"
        );
    }
}

/// Width 0 rendered nothing at all and exited 0, while a negative width was
/// correctly refused.
#[test]
fn a_zero_width_is_refused() {
    let (_out, ok) = run(&["--width", "0", "-p", "hello"], "");
    assert!(!ok, "--width 0 should be rejected");
}

/// Windows editors write a UTF-8 BOM by default. It made valid JSON fail to
/// parse "at column 1", and Markdown render its first heading as literal text
/// at exit 0 — wrong output with no diagnostic, which is the worse of the two.
#[test]
fn a_utf8_bom_does_not_break_parsing() {
    let dir = std::env::temp_dir().join("rich-bom-test");
    std::fs::create_dir_all(&dir).unwrap();

    let json = dir.join("bom.json");
    std::fs::write(&json, "\u{feff}{\"ok\": true}").unwrap();
    let (_out, ok) = run(&["-j", json.to_str().unwrap()], "");
    assert!(ok, "BOM-prefixed JSON should parse");

    let md = dir.join("bom.md");
    std::fs::write(&md, "\u{feff}# Heading\n").unwrap();
    let (out, ok) = run(&["-m", md.to_str().unwrap(), "--no-color"], "");
    assert!(ok);
    assert!(
        !out.contains("# Heading"),
        "the heading rendered literally, so the BOM survived: {out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A directory reached `read_to_string` as "Access is denied" on Windows,
/// sending the reader hunting for a permissions problem.
#[test]
fn a_directory_says_it_is_a_directory() {
    let dir = std::env::temp_dir();
    let (_out, ok) = run(&[dir.to_str().unwrap()], "");
    assert!(!ok, "a directory is not a renderable resource");
}
