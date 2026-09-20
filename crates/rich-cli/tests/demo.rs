//! The guided tour is finite and self-contained, including when piped.
use std::process::Command;

#[test]
fn guided_demo_shows_suite_without_writing_working_directory() {
    let root = tempfile::tempdir().unwrap();
    // The tour must be reproducible even beside an invalid project config.
    std::fs::write(root.path().join("rich.toml"), "[broken").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--demo", "--no-color", "--demo-delay", "60"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    for section in [
        "markup",
        "table",
        "markdown",
        "syntax",
        "progress",
        "Configuration profiles",
        "Batch planning",
        "Parallel exports",
        "JSON Lines",
        "Notebook",
        "Watch updates",
        "Tour complete",
    ] {
        assert!(text.contains(section), "missing {section}");
    }
    #[cfg(feature = "art")]
    for section in [
        "FIGlet",
        "Braille",
        "Half-block",
        "ASCII",
        "Crop anchors",
        "Image diff",
        "GIF",
    ] {
        assert!(text.contains(section), "missing {section}");
    }
    #[cfg(not(feature = "art"))]
    assert!(text.contains("art feature is disabled"));
    let profile = text
        .split("$ rich --config demo.toml --profile preview first.json")
        .nth(1)
        .unwrap();
    assert!(
        profile
            .split("Batch planning")
            .next()
            .unwrap()
            .contains('╭'),
        "profile must render a rounded panel"
    );
    assert!(!text.contains('\x1b'));
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn demo_rejects_incompatible_options_and_invalid_delays() {
    for args in [
        vec!["--demo", "file.txt"],
        vec!["--demo", "--watch"],
        vec!["--demo", "--demo-delay", "NaN"],
        vec!["--demo", "--demo-delay", "-1"],
        vec!["--demo", "--demo-delay", "61"],
        vec!["--demo-delay", "0"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rich"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn demo_as_an_operand_is_not_a_tour() {
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "--print", "--", "--demo"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "--demo");
}
