//! `--theme-file` and the `theme_file` config key (#499): upstream `[styles]`
//! theme files, their precedence and their diagnostics.
use std::path::Path;
use std::process::{Command, Output};

fn run_in(dir: &Path, args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(dir)
        .env("HOME", dir)
        .env("FORCE_COLOR", "1")
        .env("COLUMNS", "40")
        .env_remove("NO_COLOR")
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// Render with `--export-html` and return the exported page, which carries
/// the resolved styles (redirected stdout is always plain).
fn html(dir: &Path, args: &[&str], stdin: &str) -> String {
    let mut args = args.to_vec();
    args.extend(["--export-html", "out.html"]);
    let out = run_in(dir, &args, stdin);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read_to_string(dir.join("out.html")).unwrap()
}

#[test]
fn a_theme_file_styles_markup_and_renderer_output() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("theme.ini"),
        "[styles]\nalert = bold magenta\nrepr.number = underline red\n",
    )
    .unwrap();
    let markup = html(
        root.path(),
        &["--print", "[alert]hi[/]", "--theme-file", "theme.ini"],
        "",
    );
    assert!(
        markup.contains("color: #800080; text-decoration-color: #800080; font-weight: bold"),
        "{markup}"
    );
    // The repr highlighter resolves `repr.number` through the theme.
    let themed = html(
        root.path(),
        &["--print", "total 42", "--theme-file", "theme.ini"],
        "",
    );
    assert!(themed.contains("color: #800000"), "{themed}");
    assert!(themed.contains("text-decoration: underline"), "{themed}");
    let plain = html(root.path(), &["--print", "total 42"], "");
    assert!(!plain.contains("color: #800000"), "{plain}");
    assert!(plain.contains("color: #008080"), "{plain}");
}

#[test]
fn config_themes_and_theme_style_override_the_theme_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("theme.ini"), "[styles]\nalert = red\n").unwrap();
    std::fs::write(
        root.path().join("rich.toml"),
        "[themes.cool]\nalert = 'blue'\n",
    )
    .unwrap();
    let render = |extra: &[&str]| {
        let mut args = vec!["--print", "[alert]x[/]", "--theme-file", "theme.ini"];
        args.extend(extra);
        html(root.path(), &args, "")
    };
    assert!(render(&[]).contains("color: #800000"));
    assert!(render(&["--theme", "cool"]).contains("color: #000080"));
    assert!(render(&["--theme", "cool", "--theme-style", "alert=green"]).contains("color: #008000"));
    assert!(render(&["--theme-style", "alert=green"]).contains("color: #008000"));
}

#[test]
fn the_config_key_is_relative_to_the_config_file() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("cfg");
    std::fs::create_dir_all(config.join("themes")).unwrap();
    std::fs::write(config.join("themes/t.ini"), "[styles]\nalert = italic\n").unwrap();
    std::fs::write(
        config.join("rich.toml"),
        "[defaults]\ntheme_file = 'themes/t.ini'\n",
    )
    .unwrap();
    let page = html(
        root.path(),
        &["--config", "cfg/rich.toml", "--print", "[alert]x[/]"],
        "",
    );
    assert!(page.contains("font-style: italic"), "{page}");
    // An explicit flag replaces the configured file.
    std::fs::write(root.path().join("bold.ini"), "[styles]\nalert = bold\n").unwrap();
    let page = html(
        root.path(),
        &[
            "--config",
            "cfg/rich.toml",
            "--theme-file",
            "bold.ini",
            "--print",
            "[alert]x[/]",
        ],
        "",
    );
    assert!(page.contains("font-weight: bold"), "{page}");
    assert!(!page.contains("font-style: italic"), "{page}");
}

#[test]
fn invalid_theme_files_name_the_file_and_line() {
    let root = tempfile::tempdir().unwrap();
    for (text, messages) in [
        ("[colors]\nalert = red\n", vec!["NoSectionError"]),
        ("[styles]\nalert = red\nnot an option\n", vec!["line 3"]),
        ("alert = red\n", vec!["line 1", "no section headers"]),
        (
            "[styles]\n# note\nalert = red\nwarn = not-a-colour\n",
            vec!["line 4", "not-a-colour"],
        ),
    ] {
        std::fs::write(root.path().join("bad.ini"), text).unwrap();
        let out = run_in(
            root.path(),
            &["--print", "x", "--theme-file", "bad.ini"],
            "",
        );
        assert_eq!(out.status.code(), Some(2), "{text:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("--theme-file bad.ini"), "{stderr}");
        for message in messages {
            assert!(stderr.contains(message), "{text:?}: {stderr}");
        }
    }
    let missing = run_in(
        root.path(),
        &["--print", "x", "--theme-file", "missing.ini"],
        "",
    );
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--theme-file missing.ini"));
}

#[test]
fn the_config_key_is_validated() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("rich.toml"),
        "[defaults]\ntheme_file = 42\n",
    )
    .unwrap();
    let out = run_in(root.path(), &["config", "validate"], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("theme_file"));
}
