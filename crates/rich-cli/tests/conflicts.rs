//! `rich diff --conflicts FILE` (0.0.16 workstream 5, #339). Not upstream.
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

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

const MERGE: &str = "\
fn main() {
    let a = 1;
<<<<<<< HEAD
    let b = 2;
||||||| base
    let b = 0;
=======
    let b = 3;
>>>>>>> feature
    println!(\"{a}\");
}
";

fn fixtures() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("merge.rs"), MERGE).unwrap();
    std::fs::write(temp.path().join("clean.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        temp.path().join("broken.rs"),
        "a\n<<<<<<< HEAD\nb\n=======\n",
    )
    .unwrap();
    temp
}

#[test]
fn conflicts_render_side_by_side_when_wide() {
    let temp = fixtures();
    let out = run_in(
        temp.path(),
        &["diff", "--conflicts", "merge.rs", "--width", "100"],
        "",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "\
conflict 1 of 1, lines 3-9
 1   fn main() {
 2       let a = 1;
   ours: HEAD                    │    base: base                   │    theirs: feature
 4 <     let b = 2;              │  6 |     let b = 0;             │  8 >     let b = 3;
10       println!(\"{a}\");
11   }
merge.rs: 1 conflict (1 with a base)
"
    );
}

#[test]
fn conflicts_stack_when_narrow_and_take_the_diff_options() {
    let temp = fixtures();
    let out = run_in(
        temp.path(),
        &[
            "diff",
            "--conflicts",
            "merge.rs",
            "--width",
            "40",
            "--context",
            "0",
        ],
        "",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "\
conflict 1 of 1, lines 3-9
   ours: HEAD
 4 <     let b = 2;
   base: base
 6 |     let b = 0;
   theirs: feature
 8 >     let b = 3;
merge.rs: 1 conflict (1 with a base)
"
    );
    // `--side-by-side` forces columns, and the `--diff` flag form works too.
    let out = run_in(
        temp.path(),
        &[
            "--diff",
            "--conflicts",
            "--side-by-side",
            "merge.rs",
            "--width",
            "40",
        ],
        "",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains(" │ "), "{}", stdout(&out));
}

#[test]
fn conflicts_read_stdin_and_report_none() {
    let temp = fixtures();
    let out = run_in(temp.path(), &["diff", "--conflicts", "clean.rs"], "");
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "no conflicts\nclean.rs: 0 conflicts\n");

    let out = run_in(temp.path(), &["diff", "--conflicts", "-"], MERGE);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("<stdin>: 1 conflict"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn conflict_errors_have_exit_codes() {
    let temp = fixtures();
    let dir = temp.path();
    // Markers that do not parse: a data error naming the line.
    let out = run_in(dir, &["diff", "--conflicts", "broken.rs"], "");
    assert_eq!(out.status.code(), Some(4));
    assert!(
        stderr(&out).contains("broken.rs: line 2: this conflict is never closed"),
        "{}",
        stderr(&out)
    );
    // A missing file is an input error.
    let out = run_in(dir, &["diff", "--conflicts", "missing.rs"], "");
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    // Usage errors.
    for (args, message) in [
        (
            &["diff", "--conflicts", "merge.rs", "clean.rs"][..],
            "--conflicts needs exactly one file",
        ),
        (
            &["diff", "--conflicts", "merge.rs", "--threshold", "1"][..],
            "--threshold cannot be combined with --conflicts",
        ),
        (
            &["--conflicts", "merge.rs"][..],
            "--conflicts only has an effect with --diff",
        ),
    ] {
        let out = run_in(dir, args, "");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(stderr(&out).contains(message), "{args:?}: {}", stderr(&out));
    }
}

#[test]
fn diff_help_lists_conflicts() {
    let temp = fixtures();
    let out = run_in(temp.path(), &["diff", "--help"], "");
    assert!(out.status.success());
    assert!(stdout(&out).contains("--conflicts"), "{}", stdout(&out));
}
