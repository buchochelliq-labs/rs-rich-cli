//! `--help`, `rich completions`, `rich docs` and `rich config explain|reference`
//! at the process boundary: exit codes, stdout/stderr and reports.
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .arg("--no-config")
        .args(args)
        .env_remove("NO_COLOR")
        .env("COLUMNS", "80")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn help_is_plain_when_piped_and_fits_the_console_width() {
    let output = run(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let help = stdout(&output);
    assert!(
        help.starts_with("Usage: rich [OPTIONS] [RESOURCE]\n"),
        "{help}"
    );
    assert!(!help.contains('\x1b'), "piped help carries escapes");
    assert!(help.lines().all(|line| line.chars().count() <= 80));
    for heading in [
        "Render mode:",
        "Image:",
        "Batch:",
        "Commands:",
        "Exit codes:",
    ] {
        assert!(help.lines().any(|line| line == heading), "{heading}");
    }
    assert_eq!(stdout(&run(&["-h"])), help);
    let version = stdout(&run(&["-V"]));
    assert!(version.starts_with("rich (rs-rich-cli) "), "{version}");
}

#[test]
fn subcommand_help_shows_the_command_path() {
    for (args, usage) in [
        (
            &["completions", "--help"][..],
            "Usage: rich completions <SHELL>",
        ),
        (&["docs", "man", "-h"], "Usage: rich docs man [OPTIONS]"),
        (
            &["config", "explain", "--help"],
            "Usage: rich config explain",
        ),
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert!(stdout(&output).starts_with(usage), "{}", stdout(&output));
    }
}

#[test]
fn completions_print_a_script_and_reject_unknown_shells() {
    let output = run(&["completions", "bash"]);
    assert_eq!(output.status.code(), Some(0));
    let script = stdout(&output);
    assert!(script.contains("complete -F _rich"), "{script}");
    for word in ["--image-mode", "--no-watch", "completions", "explain"] {
        assert!(script.contains(word), "{word}");
    }
    for shell in ["zsh", "fish", "powershell", "pwsh"] {
        assert_eq!(
            run(&["completions", shell]).status.code(),
            Some(0),
            "{shell}"
        );
    }

    let output = run(&["completions", "tcsh", "--report", "json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(report["code"], "usage");
    assert!(report["message"]
        .as_str()
        .unwrap()
        .contains("unknown shell"));
}

#[test]
fn docs_print_markdown_man_and_the_config_reference() {
    let markdown = stdout(&run(&["docs", "markdown"]));
    assert!(markdown.starts_with("# rich\n"), "{markdown}");
    assert!(markdown.contains("## rich config explain"));
    assert!(markdown.contains("`--jobs <N>`"));

    let man = stdout(&run(&["docs", "man"]));
    assert!(man.starts_with(".TH \"RICH\" \"1\""), "{man}");

    let config = stdout(&run(&["docs", "config"]));
    assert!(config.starts_with("# rich configuration\n"), "{config}");
    assert!(config.contains("| `watch_interval` |"));

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("man");
    let output = run(&["docs", "man", "--output", target.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    for page in ["rich.1", "rich-config-explain.1", "rich-completions.1"] {
        assert!(target.join(page).is_file(), "{page}");
        assert!(stdout(&output).contains(page));
    }

    let blocked = dir.path().join("file");
    std::fs::write(&blocked, "").unwrap();
    let output = run(&["docs", "man", "--output", blocked.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "a write failure is an input error"
    );

    let output = run(&["docs", "mark"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("did you mean markdown"));
}

#[test]
fn a_render_mode_keeps_command_words_as_text() {
    let output = run(&["-p", "docs"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), "docs\n");
}

#[test]
fn config_explain_and_reference_run_without_a_config() {
    let output = run(&["config", "explain", "jobs", "--jobs", "3"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.starts_with("jobs = 3\n"), "{text}");
    assert!(text.contains("overridden by command line"), "{text}");

    let output = run(&["config", "explain", "jbos", "--report", "json"]);
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(report["message"]
        .as_str()
        .unwrap()
        .contains("did you mean jobs"));

    let output = run(&["config", "reference"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("command line"));
}
