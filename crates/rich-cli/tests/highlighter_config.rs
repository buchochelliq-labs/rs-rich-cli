//! `--highlighter` / `--code-theme` choices in `doctor` and `config validate`,
//! and `--filter` / `--highlight` rendering (0.0.12 release test).
use std::path::Path;
use std::process::{Command, Output};

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(dir)
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("home/.config"))
        .env("COLUMNS", "40")
        .env_remove("NO_COLOR")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

fn setup(project_config: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("home/.config/rich")).unwrap();
    std::fs::write(root.path().join("rich.toml"), project_config).unwrap();
    std::fs::write(
        root.path().join("a.py"),
        "def f(x):\n    return x + 1  # hi\n",
    )
    .unwrap();
    std::fs::write(root.path().join("t.txt"), "abc\ndef\n").unwrap();
    root
}

/// `--code-theme`'s help and its error point at `rich doctor --report json`
/// for the list of themes, so a config naming an unknown theme is reported
/// there, not fatal.
#[test]
fn doctor_reports_an_unknown_configured_code_theme_instead_of_failing() {
    let root = setup("[defaults]\ncode_theme = \"no-such-theme\"\n");
    let out = run_in(root.path(), &["doctor", "--report", "json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("code_highlighters"),
        "doctor failed: status {:?}\nstdout: {stdout}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn doctor_reports_an_unknown_configured_highlighter_instead_of_failing() {
    let root = setup("[defaults]\nhighlighter = \"no-such-engine\"\n");
    let out = run_in(root.path(), &["doctor", "--report", "json"]);
    assert!(
        out.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// `config validate` calls a config valid that makes every render exit 2.
#[test]
fn config_validate_rejects_a_code_theme_every_run_rejects() {
    let root = setup("[defaults]\ncode_theme = \"no-such-theme\"\n");
    let run = run_in(root.path(), &["t.txt"]);
    assert_eq!(run.status.code(), Some(2), "precondition: rendering fails");
    let validate = run_in(root.path(), &["config", "validate"]);
    let stdout = String::from_utf8_lossy(&validate.stdout);
    assert!(
        !validate.status.success() || stdout.contains("\"valid\": false"),
        "config validate accepted a config every run rejects:\n{stdout}"
    );
}

/// `--highlight` marks matches in reverse video only: a named group in the
/// regular expression is not looked up as a theme style.
#[test]
fn highlight_named_groups_add_no_theme_styles() {
    let root = setup("");
    std::fs::remove_file(root.path().join("rich.toml")).unwrap();
    let html = |pattern: &str| {
        let out = run_in(
            root.path(),
            &["--highlight", pattern, "t.txt", "--export-html", "out.html"],
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::read_to_string(root.path().join("out.html")).unwrap()
    };
    let plain = html("(b)");
    let named = html("(?P<bold>b)");
    assert_eq!(plain, named, "a named group changed the highlight style");
}

/// A `--highlight` that matches nothing leaves `--syntax` output as it was:
/// the transformed text keeps the full-width background rows of `Syntax`.
#[test]
fn a_non_matching_highlight_leaves_syntax_output_unchanged() {
    let root = setup("");
    std::fs::remove_file(root.path().join("rich.toml")).unwrap();
    let without = run_in(root.path(), &["--width", "30", "--syntax", "a.py"]);
    let with = run_in(
        root.path(),
        &["--width", "30", "--syntax", "--highlight", "zzz", "a.py"],
    );
    assert!(with.status.success());
    assert_eq!(
        String::from_utf8_lossy(&without.stdout),
        String::from_utf8_lossy(&with.stdout)
    );
}
