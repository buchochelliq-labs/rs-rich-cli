//! `rich view`, `rich hex`, `rich unicode`, `rich env` and `rich capture`
//! (0.0.11 workstream 11). Not upstream.
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(dir: &std::path::Path, args: &[&str], stdin: &[u8], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rich"));
    command
        .args(["--no-config", "--no-color", "--width", "80"])
        .args(args)
        .current_dir(dir)
        .env_remove("NO_COLOR")
        .env("COLUMNS", "80")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    // The binary may exit (a usage error, say) before it reads stdin.
    if let Err(error) = child.stdin.take().unwrap().write_all(stdin) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "write test stdin"
        );
    }
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn ok(output: &Output) -> String {
    assert!(output.status.success(), "{output:?}");
    text(&output.stdout)
}

fn usage(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    text(&output.stderr)
}

fn temp() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.rs"),
        "fn main() {\n    let total = 1;\n    println!(\"{total}\");\n}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("notes.md"), "# Title\n\nSome *text*.\n").unwrap();
    std::fs::write(
        dir.path().join("fix.patch"),
        "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("blob.bin"), b"\x7fELF\x00\x01binary").unwrap();
    dir
}

#[test]
fn view_numbers_source_and_routes_other_formats() {
    let dir = temp();
    let out = ok(&run(dir.path(), &["view", "main.rs"], b"", &[]));
    assert!(out.starts_with("1 │ fn main() {"), "{out}");
    assert!(out.contains("4 │ }"), "{out}");
    let bare = ok(&run(
        dir.path(),
        &["view", "--no-line-numbers", "main.rs"],
        b"",
        &[],
    ));
    assert!(bare.starts_with("fn main() {"), "{bare}");
    // Markdown renders, a patch is drawn as one, binary becomes hex.
    let md = ok(&run(dir.path(), &["view", "notes.md"], b"", &[]));
    assert!(md.contains("Title") && !md.contains("# Title"), "{md}");
    let patch = ok(&run(dir.path(), &["view", "fix.patch"], b"", &[]));
    assert!(patch.contains("new") && !patch.contains("1 │"), "{patch}");
    let hex = ok(&run(dir.path(), &["view", "blob.bin"], b"", &[]));
    assert!(hex.starts_with("00000000  7f 45 4c 46 00 01"), "{hex}");
    // Stdin is read once and its format detected from the content.
    let json = ok(&run(dir.path(), &["view", "-"], b"{\"a\": 1}\n", &[]));
    assert!(json.starts_with("1 │ {\"a\": 1}"), "{json}");
}

#[test]
fn view_search_highlights_and_reports_matches_on_stderr() {
    let dir = temp();
    let output = run(
        dir.path(),
        &["view", "--search", "TOTAL", "main.rs"],
        b"",
        &[],
    );
    assert!(ok(&output).contains("let total"));
    assert_eq!(
        text(&output.stderr).trim(),
        "2 matches for \"TOTAL\" on lines 2, 3"
    );
}

#[test]
fn hex_slices_groups_and_searches() {
    let dir = temp();
    let input = b"hello\x00world\n";
    let out = ok(&run(dir.path(), &["hex", "-"], input, &[]));
    assert_eq!(
        out,
        "00000000  68 65 6c 6c 6f 00 77 6f  72 6c 64 0a              │hello.world.│\n0000000c\n"
    );
    let sliced = ok(&run(
        dir.path(),
        &["hex", "--offset", "0x6", "--length", "3", "-"],
        input,
        &[],
    ));
    assert!(sliced.starts_with("00000006  77 6f 72 "), "{sliced}");
    assert!(
        ok(&run(
            dir.path(),
            &["hexdump", "--search", "\"wor\"", "-"],
            input,
            &[]
        ))
        .contains("77 6f  72"),
        "the match crosses a group boundary"
    );
    let error = usage(&run(
        dir.path(),
        &["hex", "--search", "zz", "-"],
        input,
        &[],
    ));
    assert!(error.contains("not a hex digit"), "{error}");
}

#[test]
fn unicode_shows_graphemes_and_invalid_bytes() {
    let dir = temp();
    let out = ok(&run(
        dir.path(),
        &["unicode", "-"],
        b"e\xcc\x81\xff!\n",
        &[],
    ));
    assert!(out.contains("U+0065 U+0301"), "{out}");
    assert!(out.contains("invalid"), "{out}");
    // The trailing newline is not inspected.
    assert!(out.contains("5 bytes, 3 code points, 2 graphemes"), "{out}");
    assert!(!out.ends_with("\n\n"), "{out:?}");
}

#[test]
fn env_redacts_filters_and_checks_path_entries() {
    let dir = temp();
    let env = [("RSRICH_TEST_TOKEN", "abc"), ("RSRICH_TEST_AUTHOR", "me")];
    let out = ok(&run(dir.path(), &["env", "RSRICH_TEST"], b"", &env));
    assert!(
        out.contains("RSRICH_TEST_AUTHOR") && out.contains("me"),
        "{out}"
    );
    assert!(out.contains("(3 chars)") && !out.contains("abc"), "{out}");
    let shown = ok(&run(
        dir.path(),
        &["env", "--show-secrets", "RSRICH_TEST_TOKEN"],
        b"",
        &env,
    ));
    assert!(shown.contains("abc"), "{shown}");
    let exists = dir.path().to_string_lossy().into_owned();
    let path = format!(
        "{exists}{sep}/definitely/missing{sep}{exists}",
        sep = if cfg!(windows) { ';' } else { ':' }
    );
    let table = ok(&run(
        dir.path(),
        &["env", "RSRICH_TEST_PATH"],
        b"",
        &[("RSRICH_TEST_PATH", path.as_str())],
    ));
    assert!(
        table.contains("missing") && table.contains("duplicate of #1"),
        "{table}"
    );
}

#[cfg(unix)]
#[test]
fn capture_runs_the_command_and_records_a_cast() {
    let dir = temp();
    let out = ok(&run(
        dir.path(),
        &[
            "capture",
            "--cast",
            "run.cast",
            "--",
            "sh",
            "-c",
            "echo hi; echo err >&2; exit 2",
        ],
        b"",
        &[],
    ));
    let hi = out.find("hi").expect(&out);
    let err = out.find("err").expect(&out);
    assert!(hi < err, "{out}");
    assert!(out.contains("exit 2"), "{out}");
    let cast = std::fs::read_to_string(dir.path().join("run.cast")).unwrap();
    assert!(
        cast.lines().next().unwrap().contains("\"version\":2"),
        "{cast}"
    );
    // SVG export works like every other mode.
    ok(&run(
        dir.path(),
        &["capture", "--export-svg", "run.svg", "--", "echo", "done"],
        b"",
        &[],
    ));
    assert!(std::fs::read_to_string(dir.path().join("run.svg"))
        .unwrap()
        .contains("done"));
}

#[test]
fn viewer_options_are_rejected_elsewhere_and_capture_needs_a_command() {
    let dir = temp();
    let error = usage(&run(
        dir.path(),
        &["view", "--offset", "3", "main.rs"],
        b"",
        &[],
    ));
    assert!(
        error.contains("--offset only has an effect with `rich hex`"),
        "{error}"
    );
    let error = usage(&run(dir.path(), &["--search", "x", "main.rs"], b"", &[]));
    assert!(error.contains("`rich view` or `rich hex`"), "{error}");
    let error = usage(&run(dir.path(), &["capture"], b"", &[]));
    assert!(error.contains("capture needs a command"), "{error}");
    let missing = run(
        dir.path(),
        &["capture", "--", "definitely-not-a-command-xyz"],
        b"",
        &[],
    );
    assert_eq!(missing.status.code(), Some(3), "{missing:?}");
}
