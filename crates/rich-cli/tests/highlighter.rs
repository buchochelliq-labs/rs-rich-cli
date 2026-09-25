//! Choosing the code highlighter (#525): `--highlighter`, `--code-theme`, the
//! `highlighter` / `code_theme` config keys, the commands that honour them and
//! the usage errors. Exported HTML carries the styles; stdout here is plain.
use std::path::Path;
use std::process::{Command, Output};

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(dir)
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("home/.config"))
        .env("COLUMNS", "60")
        .env_remove("NO_COLOR")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

/// Render `args` and return the exported HTML.
fn html(dir: &Path, args: &[&str]) -> String {
    let out_path = dir.join("out.html");
    let _ = std::fs::remove_file(&out_path);
    let mut args = args.to_vec();
    args.extend(["--export-html", "out.html"]);
    let out = run_in(dir, &args);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read_to_string(out_path).unwrap()
}

fn setup() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("home/.config/rich")).unwrap();
    std::fs::write(
        root.path().join("a.py"),
        "def f(x):\n    return x + 1  # hi\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("b.py"),
        "def f(x):\n    return x + 2  # hi\n",
    )
    .unwrap();
    std::fs::write(root.path().join("doc.md"), "# D\n\n```python\nx = 1\n```\n").unwrap();
    root
}

#[test]
fn the_code_theme_reaches_every_command_that_highlights() {
    let root = setup();
    let dir = root.path();
    for command in [
        &["a.py"][..],
        &["--syntax", "a.py"],
        &["view", "a.py"],
        &["diff", "a.py", "b.py"],
        &["doc.md"],
    ] {
        let default = html(dir, command);
        let mut themed_args = vec!["--no-config", "--code-theme", "ansi_dark"];
        themed_args.extend(command);
        let themed = html(dir, &themed_args);
        let mut plain_args = vec!["--no-config"];
        plain_args.extend(command);
        assert_eq!(
            default,
            html(dir, &plain_args),
            "{command:?}: --no-config changed it"
        );
        assert_ne!(default, themed, "{command:?}: the theme had no effect");
        // Choosing the default explicitly changes nothing.
        let mut syntect_args = vec!["--no-config", "--highlighter", "syntect"];
        syntect_args.extend(command);
        assert_eq!(default, html(dir, &syntect_args), "{command:?}");
    }
}

#[test]
fn config_keys_match_the_flags_and_a_project_config_may_set_them() {
    let root = setup();
    let dir = root.path();
    let flagged = html(
        dir,
        &[
            "--no-config",
            "--highlighter",
            "syntect",
            "--code-theme",
            "ansi_light",
            "a.py",
        ],
    );
    // The working-directory config is trusted with this plain choice.
    std::fs::write(
        dir.join("rich.toml"),
        "[defaults]\nhighlighter = \"syntect\"\ncode_theme = \"ansi_light\"\n",
    )
    .unwrap();
    let out = run_in(dir, &["a.py", "--export-html", "out.html"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "no warning: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("out.html")).unwrap(),
        flagged
    );
    // The flag overrides the config.
    let dark = html(dir, &["--code-theme", "ansi_dark", "a.py"]);
    assert_eq!(
        dark,
        html(dir, &["--no-config", "--code-theme", "ansi_dark", "a.py"])
    );
}

#[test]
fn unknown_highlighters_and_themes_are_usage_errors_listing_the_choices() {
    let root = setup();
    let dir = root.path();
    let out = run_in(dir, &["--no-config", "--highlighter", "nope", "a.py"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("unknown code highlighter \"nope\"; available: "),
        "{err}"
    );
    assert!(err.contains("syntect"), "{err}");

    let out = run_in(dir, &["--no-config", "--code-theme", "nope", "a.py"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("has no theme \"nope\""), "{err}");
    assert!(err.contains("ansi_dark"), "the themes are listed: {err}");

    // From a config file too.
    std::fs::write(
        dir.join("rich.toml"),
        "[defaults]\nhighlighter = \"nope\"\n",
    )
    .unwrap();
    let out = run_in(dir, &["a.py"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(not(feature = "lumis"))]
#[test]
fn lumis_needs_the_feature() {
    let root = setup();
    let out = run_in(
        root.path(),
        &["--no-config", "--highlighter", "lumis", "a.py"],
    );
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("lumis needs a build with the lumis feature"),
        "{err}"
    );
}

#[cfg(feature = "lumis")]
#[test]
fn lumis_highlights_when_built_in() {
    let root = setup();
    let dir = root.path();
    let syntect = html(dir, &["--no-config", "a.py"]);
    let lumis = html(dir, &["--no-config", "--highlighter", "lumis", "a.py"]);
    assert_ne!(syntect, lumis);
    // Its own theme names, and the ANSI themes both engines share.
    html(
        dir,
        &[
            "--no-config",
            "--highlighter",
            "lumis",
            "--code-theme",
            "dracula",
            "a.py",
        ],
    );
    let ansi = html(
        dir,
        &[
            "--no-config",
            "--highlighter",
            "lumis",
            "--code-theme",
            "ansi_dark",
            "doc.md",
        ],
    );
    assert!(ansi.contains("x"), "{ansi}");
    // A syntect theme is not a lumis theme.
    let out = run_in(
        dir,
        &[
            "--no-config",
            "--highlighter",
            "lumis",
            "--code-theme",
            "base16-ocean.dark",
            "a.py",
        ],
    );
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn doctor_reports_the_highlighters_and_the_active_one() {
    let root = setup();
    let out = run_in(
        root.path(),
        &[
            "doctor",
            "--no-config",
            "--code-theme",
            "ansi_dark",
            "--report",
            "json",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let highlighters = &report["code_highlighters"];
    assert_eq!(highlighters["active"]["name"], "syntect");
    assert_eq!(highlighters["active"]["theme"], "ansi_dark");
    let names: Vec<&str> = highlighters["available"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"syntect"));
    assert_eq!(names.contains(&"lumis"), cfg!(feature = "lumis"));
    let syntect = &highlighters["available"][names.iter().position(|n| *n == "syntect").unwrap()];
    assert!(syntect["themes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t == "ansi_light"));

    let text = run_in(root.path(), &["doctor", "--no-config"]);
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(
        text.lines()
            .any(|l| l.starts_with("Code highlighter: syntect (theme ")),
        "{text}"
    );
}
