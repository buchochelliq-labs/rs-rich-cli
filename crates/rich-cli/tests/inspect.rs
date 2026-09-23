//! `rich inspect` and `--format` (0.0.11 workstream 4): not upstream, so these
//! pin our own behaviour, and that piped input without `--format` is unchanged.
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

const YAML: &str = "defaults: &defaults\n  retries: 3\nservers:\n  - name: alpha\n    port: 8080\n  - name: beta\n    port: 8081\n    password: hunter2\n";

#[test]
fn inspect_draws_a_tree_and_its_views() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("hosts.yaml"), YAML).unwrap();
    let dir = temp.path();
    let args = |extra: &[&str]| {
        let mut args = vec!["--no-color", "inspect", "hosts.yaml"];
        args.extend_from_slice(extra);
        args.into_iter().map(String::from).collect::<Vec<_>>()
    };
    let run = |extra: &[&str]| {
        let args = args(extra);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        text(&run_in(dir, &args, ""))
    };

    let tree = run(&[]);
    assert!(tree.starts_with("hosts.yaml\n"), "{tree}");
    assert!(tree.contains("defaults &defaults"), "{tree}");
    assert!(tree.contains("port: 8081"), "{tree}");

    assert_eq!(
        run(&["--select", "$.servers[*].name"]),
        "hosts.yaml\n├── servers[0].name: \"alpha\"\n└── servers[1].name: \"beta\"\n"
    );
    let found = run(&["--find", "PORT"]);
    assert!(found.contains("servers[0].port: 8080"), "{found}");
    assert!(found.ends_with("2 matches\n"), "{found}");

    let flat = run(&["--flatten", "--redact"]);
    assert!(flat.contains("servers[1].password"), "{flat}");
    assert!(!flat.contains("hunter2"), "{flat}");

    let folded = run(&["--max-depth", "1"]);
    assert!(folded.contains("{…}") || folded.contains("[…]"), "{folded}");
}

#[test]
fn inspect_compares_two_documents() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("old.json"), r#"{"a": 1, "b": 2}"#).unwrap();
    std::fs::write(temp.path().join("new.toml"), "a = 1\nb = 3\nc = 4\n").unwrap();
    let out = text(&run_in(
        temp.path(),
        &["--no-color", "inspect", "old.json", "--compare", "new.toml"],
        "",
    ));
    assert!(
        out.contains('b') && out.contains('3') && out.contains('c'),
        "{out}"
    );
    assert!(!out.contains("a:"), "unchanged keys are not listed:\n{out}");
}

#[test]
fn inspect_reads_stdin_and_reports_errors_with_exit_codes() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let out = text(&run_in(
        dir,
        &["--no-color", "inspect"],
        "[database]\nhost = \"db\"\n",
    ));
    assert!(out.contains("database") && out.contains("\"db\""), "{out}");

    let bad = run_in(
        dir,
        &["--no-color", "inspect", "--format", "json"],
        "{\n \"a\": \n}",
    );
    assert_eq!(bad.status.code(), Some(4));
    let err = String::from_utf8_lossy(&bad.stderr);
    assert!(err.contains("<stdin>:3:1: invalid json"), "{err}");

    let unknown = run_in(dir, &["--no-color", "inspect"], "just words\n");
    assert_eq!(unknown.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("cannot detect the format"));

    for args in [
        &["--select", "$.a", "-"][..],
        &["--json", "--format", "yaml", "-"][..],
        &["inspect", "--flatten", "--table", "-"][..],
        &["inspect", "--format", "csv", "-"][..],
    ] {
        let out = run_in(dir, args, "{}");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn format_routes_piped_input_and_leaves_plain_text_alone() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    // Without --format nothing changes: piped JSON is text, as upstream prints it.
    let json = "{\"a\": 1}\n";
    let plain = text(&run_in(dir, &["--no-color", "-"], json));
    assert_eq!(plain, "{\"a\": 1}\n\n");
    // --format auto reads stdin without `-` and sends JSON to the JSON renderer.
    let routed = text(&run_in(dir, &["--no-color", "--format", "auto"], json));
    let rendered = text(&run_in(dir, &["--no-color", "--json", "-"], json));
    assert_eq!(routed, rendered);
    // Anything detection cannot place is printed exactly as before.
    let prose = "plain words here\n";
    assert_eq!(
        text(&run_in(dir, &["--no-color", "--format", "auto"], prose)),
        text(&run_in(dir, &["--no-color", "-"], prose))
    );
    // A named format overrides the extension.
    std::fs::write(dir.join("data.txt"), json).unwrap();
    let named = text(&run_in(
        dir,
        &["--no-color", "--format", "json", "data.txt"],
        "",
    ));
    assert_eq!(named, rendered);
}

#[test]
fn configured_format_applies_only_where_it_means_something() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    std::fs::write(dir.join("rich.toml"), "[defaults]\nformat = \"auto\"\n").unwrap();
    let json = "{\"a\": 1}\n";
    let routed = text(&run_in(dir, &["--no-color", "-"], json));
    assert!(
        routed.contains("\"a\": 1") && routed.starts_with('{'),
        "{routed}"
    );
    assert_ne!(routed, "{\"a\": 1}\n\n");
    // An explicit mode keeps working: config never makes it a usage error.
    let explicit = run_in(dir, &["--no-color", "--markdown", "-"], "# hi\n");
    assert!(
        explicit.status.success(),
        "{}",
        String::from_utf8_lossy(&explicit.stderr)
    );

    std::fs::write(dir.join("rich.toml"), "[defaults]\nformat = \"csv\"\n").unwrap();
    let invalid = run_in(dir, &["config", "validate"], "");
    assert_eq!(invalid.status.code(), Some(2));
}
