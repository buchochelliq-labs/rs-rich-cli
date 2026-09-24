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
    let failed = run(
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
    );
    // The command's status passes through, after the panel and the cast.
    assert_eq!(failed.status.code(), Some(2), "{failed:?}");
    let out = text(&failed.stdout);
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
    // `--report json` describes the command's failure, not a success.
    let reported = run(
        dir.path(),
        &["--report", "json", "capture", "--", "sh", "-c", "exit 7"],
        b"",
        &[],
    );
    assert_eq!(reported.status.code(), Some(7), "{reported:?}");
    let report: serde_json::Value = serde_json::from_str(text(&reported.stderr).trim()).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["code"], "command");
    assert_eq!(report["exit_code"], 7);
    assert_eq!(report["result"]["status"], 7);
    // A command killed by a signal exits 128 + the signal, as in a shell.
    let killed = run(
        dir.path(),
        &["capture", "--", "sh", "-c", "kill -TERM $$"],
        b"",
        &[],
    );
    assert_eq!(killed.status.code(), Some(128 + 15), "{killed:?}");
    assert!(
        text(&killed.stdout).contains("killed by signal 15"),
        "{killed:?}"
    );
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

#[cfg(unix)]
#[test]
fn capture_redacts_before_showing_exporting_or_recording() {
    let dir = temp();
    let token = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
    let script = format!(
        "echo 'GITHUB_TOKEN={token}'; printf 'pass\\033[31mword=hun\\033[0mter2\\n'; \
         echo 'order 1234-5678'; echo 'plain line'"
    );
    // From a file, so the secrets are not on the (also redacted) command line.
    std::fs::write(dir.path().join("script.sh"), script).unwrap();
    let out = ok(&run(
        dir.path(),
        &[
            "capture",
            "--redact",
            "--redact-pattern",
            r"order (?P<secret>\d{4})",
            "--cast",
            "run.cast",
            "--export-svg",
            "run.svg",
            "--export-html",
            "run.html",
            "--",
            "sh",
            "script.sh",
        ],
        b"",
        &[],
    ));
    let cast = std::fs::read_to_string(dir.path().join("run.cast")).unwrap();
    let svg = std::fs::read_to_string(dir.path().join("run.svg")).unwrap();
    let html = std::fs::read_to_string(dir.path().join("run.html")).unwrap();
    for (name, body) in [
        ("stdout", &out),
        ("cast", &cast),
        ("svg", &svg),
        ("html", &html),
    ] {
        for secret in [token, "hunter2", "ter2", "1234"] {
            assert!(!body.contains(secret), "{name} leaks {secret:?}:\n{body}");
        }
    }
    // Masks keep the width of what they replace, so the panel stays aligned.
    let stars = "*".repeat(token.len());
    assert!(out.contains(&format!("GITHUB_TOKEN={stars}")), "{out}");
    assert!(out.contains("password=*******"), "{out}");
    assert!(out.contains("order ****-5678"), "{out}");
    assert!(out.contains("plain line"), "{out}");
    // The recording keeps its escapes around the masked cells.
    assert!(
        cast.contains(r"pass\u001b[31mword=***\u001b[0m****"),
        "{cast}"
    );
}

#[cfg(unix)]
#[test]
fn capture_redacts_the_command_line_too() {
    let dir = temp();
    let out = ok(&run(
        dir.path(),
        &[
            "capture",
            "--redact",
            "--",
            "sh",
            "-c",
            "true",
            "password=hunter2",
        ],
        b"",
        &[],
    ));
    assert!(!out.contains("hunter2"), "{out}");
    assert!(out.contains("password=*******"), "{out}");
    // Without the flag nothing is masked.
    let out = ok(&run(
        dir.path(),
        &["capture", "--", "echo", "token=abc"],
        b"",
        &[],
    ));
    assert!(out.contains("token=abc"), "{out}");
}

#[test]
fn redact_options_are_checked() {
    let dir = temp();
    let error = usage(&run(
        dir.path(),
        &["capture", "--redact-pattern", "(", "--", "true"],
        b"",
        &[],
    ));
    assert!(
        error.contains("--redact-pattern: invalid pattern \"(\""),
        "{error}"
    );
    let error = usage(&run(
        dir.path(),
        &["view", "--redact-pattern", "x", "main.rs"],
        b"",
        &[],
    ));
    assert!(
        error.contains("--redact-pattern only has an effect with `rich capture`"),
        "{error}"
    );
    let error = usage(&run(dir.path(), &["--redact", "main.rs"], b"", &[]));
    assert!(
        error.contains("--redact only has an effect with --inspect or `rich capture`"),
        "{error}"
    );
}
