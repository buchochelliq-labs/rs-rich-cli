//! `rich diff` for text and patches, `rich ansi explain` and the doctor's
//! capability report (0.0.11 workstreams 8 and 9). Not upstream.
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run_in(dir: &std::path::Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "--no-color"])
        .args(args)
        .current_dir(dir)
        .env_remove("NO_COLOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The binary may exit (a usage error, say) before it reads stdin.
    if let Err(error) = child.stdin.take().unwrap().write_all(stdin.as_bytes()) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "write test stdin"
        );
    }
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn fixtures() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("old.rs"),
        "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("new.rs"),
        "fn main() {\n    let x = 2;\n    println!(\"{x}\");\n}\n",
    )
    .unwrap();
    temp
}

#[test]
fn text_diff_shows_changes_and_gates_on_the_threshold() {
    let temp = fixtures();
    let dir = temp.path();
    let out = run_in(dir, &["diff", "old.rs", "new.rs"], "");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(text.contains("@@ -1,4 +1,4 @@"), "{text}");
    assert!(
        text.contains("let x = 1;") && text.contains("let x = 2;"),
        "{text}"
    );
    assert!(
        text.contains("\nold.rs → new.rs: 1 added, 1 removed (25.0% of lines changed)"),
        "{text}"
    );

    let side = stdout(&run_in(
        dir,
        &["diff", "old.rs", "new.rs", "--side-by-side", "-w", "80"],
        "",
    ));
    assert!(side.contains('│'), "{side}");

    let within = run_in(dir, &["diff", "old.rs", "new.rs", "--threshold", "30"], "");
    assert!(within.status.success());
    assert!(stdout(&within).contains("OK 25.0% changed, within 30.0%"));
    let over = run_in(dir, &["diff", "old.rs", "new.rs", "--threshold", "10"], "");
    assert_eq!(over.status.code(), Some(5));
    assert!(stdout(&over).contains("FAIL 25.0% changed, limit 10.0%"));

    let same = stdout(&run_in(dir, &["diff", "old.rs", "old.rs"], ""));
    assert!(same.contains("no differences"), "{same}");
}

#[test]
fn a_patch_on_stdin_renders_as_files_and_hunks() {
    let temp = fixtures();
    let patch = "diff --git a/src/lib.rs b/src/lib.rs\nindex 1..2 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n pub mod a;\n-pub mod b;\n+pub mod c;\n";
    let out = run_in(temp.path(), &["diff", "-"], patch);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(
        text.contains("src/lib.rs") && text.contains("pub mod c;"),
        "{text}"
    );
    assert!(text.trim_end().ends_with("1 added, 1 removed"), "{text}");

    let prose = run_in(temp.path(), &["diff", "-"], "not a patch\n");
    assert_eq!(prose.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&prose.stderr).contains("give two files to compare"));
}

#[test]
fn ansi_explain_decodes_escapes_by_command_or_flag() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let capture = "\u{1b}[1;31mERROR\u{1b}[0m done\n";
    std::fs::write(dir.join("explain"), capture).unwrap();
    for args in [
        &["ansi", "explain"][..],
        &["ansi", "explain", "-"][..],
        &["ansi-explain", "-"][..],
        &["--ansi-explain", "-"][..],
    ] {
        let out = run_in(dir, args, capture);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = stdout(&out);
        assert!(
            text.contains("ESC[1;31m") && text.contains("bold on, fg red"),
            "{text}"
        );
        assert!(text.contains("ERROR done"), "visible text:\n{text}");
    }
    // A file literally called `explain` still works with the flag.
    let named = stdout(&run_in(dir, &["--ansi-explain", "explain"], ""));
    assert!(named.contains("ESC[0m"), "{named}");
    // --sanitize must not strip what is being explained.
    let sanitized = stdout(&run_in(dir, &["ansi", "explain", "--sanitize"], capture));
    assert!(sanitized.contains("bold on"), "{sanitized}");
    let inline = stdout(&run_in(dir, &["ansi", "explain", "--ansi-inline"], capture));
    assert!(inline.contains("ERROR"), "{inline}");
}

#[test]
fn tool_options_need_their_modes() {
    let temp = fixtures();
    let dir = temp.path();
    for (args, message) in [
        (
            &["--side-by-side", "old.rs"][..],
            "--side-by-side only has an effect with --diff",
        ),
        (
            &["--context", "2", "old.rs"][..],
            "--context only has an effect with --diff",
        ),
        (
            &["--escapes-only", "old.rs"][..],
            "--escapes-only only has an effect with --ansi-explain",
        ),
        (
            &["diff", "a.png", "b.png", "--side-by-side"][..],
            "applies to text diffs",
        ),
    ] {
        let out = run_in(dir, args, "");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(message), "{args:?}: {err}");
    }
}

#[test]
fn doctor_reports_capabilities_with_their_sources() {
    let temp = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "doctor", "--report", "json"])
        .current_dir(temp.path())
        .env("RICH_HYPERLINKS", "1")
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let caps = &report["capabilities"];
    for key in [
        "color",
        "unicode",
        "hyperlinks",
        "graphics",
        "width",
        "interactive",
        "animation",
    ] {
        assert!(!caps[key]["origin"].is_null(), "{key}: {caps}");
    }
    assert_eq!(caps["hyperlinks"]["value"], true, "{caps}");
    assert!(
        caps["hyperlinks"]["reason"]
            .as_str()
            .unwrap()
            .contains("RICH_HYPERLINKS"),
        "{caps}"
    );
    // The long-standing keys stay.
    for key in ["terminal", "image", "config", "pager"] {
        assert!(report[key].is_object(), "{key}");
    }
}

#[test]
fn bench_compare_shows_changes_and_gates_on_regressions() {
    use rich_ext::qa::bench::{BenchRun, Measurement};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let run = |render: f64, parse: f64| {
        BenchRun::new(vec![
            Measurement::from_samples("render", &[render; 20]),
            Measurement::from_samples("parse", &[parse; 20]),
        ])
    };
    run(100.0, 50.0).save(dir.join("base.json")).unwrap();
    run(101.0, 30.0).save(dir.join("fast.json")).unwrap();
    run(150.0, 50.0).save(dir.join("slow.json")).unwrap();

    let ok = run_in(dir, &["bench", "compare", "base.json", "fast.json"], "");
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let text = stdout(&ok);
    assert!(text.contains("render") && text.contains("parse"), "{text}");

    let slow = run_in(dir, &["bench", "compare", "base.json", "slow.json"], "");
    assert_eq!(slow.status.code(), Some(5), "{}", stdout(&slow));
    assert!(String::from_utf8_lossy(&slow.stderr).contains("benchmark regression"));
    // A generous threshold lets the same change through.
    let loose = run_in(
        dir,
        &[
            "bench",
            "compare",
            "base.json",
            "slow.json",
            "--threshold",
            "60",
        ],
        "",
    );
    assert!(loose.status.success(), "{}", stdout(&loose));

    let missing = run_in(dir, &["bench", "compare", "base.json", "nope.json"], "");
    assert_eq!(missing.status.code(), Some(3));
    let usage = run_in(dir, &["bench", "compare", "base.json"], "");
    assert_eq!(usage.status.code(), Some(2));
}
