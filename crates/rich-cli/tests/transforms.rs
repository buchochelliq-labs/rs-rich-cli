//! `--filter` and `--highlight` (0.0.12 workstream 4, #216): not upstream, so
//! these pin our own behaviour and the documented pipeline order.
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run_in(dir: &std::path::Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
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

fn text(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn error(output: &Output) -> (i32, String) {
    assert!(!output.status.success());
    (
        output.status.code().unwrap(),
        String::from_utf8(output.stderr.clone()).unwrap(),
    )
}

const LOG: &str = "INFO start\nWARN slow disk\nERROR failed\nINFO done\n";
const JSON: &str = r#"{"name": "api", "servers": [{"host": "a", "port": 1}, {"host": "b", "password": "hunter2"}]}"#;

#[test]
fn filter_keeps_matching_lines_of_text_print_and_syntax() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    std::fs::write(dir.join("app.log"), LOG).unwrap();

    let piped = text(&run_in(
        dir,
        &["--no-color", "-", "--filter", "WARN|ERROR"],
        LOG,
    ));
    assert_eq!(piped, "WARN slow disk\nERROR failed\n\n");

    let print = text(&run_in(
        dir,
        &[
            "--no-color",
            "--print",
            "[bold]WARN[/] a\nINFO b",
            "--filter",
            "WARN",
        ],
        "",
    ));
    assert_eq!(print, "WARN a\n");

    // `.log` renders as syntax; the filter runs on the highlighted text.
    let syntax = text(&run_in(
        dir,
        &["--no-color", "app.log", "--filter", "^INFO"],
        "",
    ));
    assert_eq!(syntax, "INFO start\nINFO done\n\n");
}

#[test]
fn inspect_runs_redact_select_filter_highlight_in_order() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    std::fs::write(dir.join("svc.json"), JSON).unwrap();
    let run = |extra: &[&str]| {
        let mut args = vec!["--no-color", "inspect", "svc.json"];
        args.extend_from_slice(extra);
        text(&run_in(dir, &args, ""))
    };

    // --filter keeps the shape above what it selects.
    assert_eq!(
        run(&["--filter", "$..host"]),
        "svc.json\n└── servers\n    ├── [0]\n    │   └── host: \"a\"\n    └── [1]\n        └── host: \"b\"\n"
    );
    // --select runs before --filter: the filter's path is relative to the
    // selection, and the label is the selected path.
    assert_eq!(
        run(&[
            "--select",
            "$.servers[1]",
            "--filter",
            "$.password",
            "--redact"
        ]),
        "servers[1]\n└── password: \"********\"\n"
    );
    // --highlight changes styles only.
    assert_eq!(run(&["--highlight", "$.name"]), run(&[]));
}

#[test]
fn transforms_are_refused_where_they_do_not_apply() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    std::fs::write(dir.join("svc.json"), JSON).unwrap();
    std::fs::write(dir.join("notes.md"), "# hi\n").unwrap();

    let (code, message) = error(&run_in(dir, &["--json", "svc.json", "--filter", "a"], ""));
    assert_eq!(code, 2);
    assert!(
        message.contains("--filter only has an effect on text"),
        "{message}"
    );

    // Found by extension at run time.
    let (code, message) = error(&run_in(dir, &["notes.md", "--highlight", "hi"], ""));
    assert_eq!(code, 2);
    assert!(message.contains("not markdown"), "{message}");

    let (code, message) = error(&run_in(dir, &["-", "--filter", "("], "x"));
    assert_eq!(code, 2);
    assert!(message.contains("--filter: invalid pattern"), "{message}");

    let (code, message) = error(&run_in(dir, &["inspect", "svc.json", "--filter", "$["], ""));
    assert_eq!(code, 2);
    assert!(message.contains("invalid --filter expression"), "{message}");

    for (extra, expected) in [
        (
            &["--highlight", "$.name", "--flatten"][..],
            "--highlight cannot be combined with --flatten",
        ),
        (
            &["--filter", "$.name", "--highlight", "$.name", "--table"],
            "--highlight cannot be combined with --table",
        ),
        (
            &["--filter", "$.name", "--compare", "svc.json"],
            "--filter cannot be combined with --compare",
        ),
    ] {
        let mut args = vec!["inspect", "svc.json"];
        args.extend_from_slice(extra);
        let (code, message) = error(&run_in(dir, &args, ""));
        assert_eq!(code, 2);
        assert!(message.contains(expected), "{message}");
    }
    // --filter works with the other views.
    let flat = text(&run_in(
        dir,
        &[
            "--no-color",
            "inspect",
            "svc.json",
            "--filter",
            "$.name",
            "--flatten",
        ],
        "",
    ));
    assert!(flat.contains("name") && !flat.contains("servers"), "{flat}");
}
