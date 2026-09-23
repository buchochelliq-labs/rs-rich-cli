//! Doctor is a read-only diagnostic, including when external programs are configured.
use std::process::Command;

#[test]
fn doctor_json_reports_redirected_capabilities_without_environment_secrets() {
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["doctor", "--no-config", "--report", "json"])
        .env("NO_COLOR", "1")
        .env("MANPAGER", "missing-pager secret-argument")
        .env("DOCTOR_SECRET_TOKEN", "never-disclose-this")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["terminal"]["stdout_tty"], false);
    assert_eq!(report["terminal"]["no_color"], true);
    assert_eq!(report["terminal"]["color"], "none");
    assert_eq!(report["features"]["art"], cfg!(feature = "art"));
    assert_eq!(report["config"]["disabled"], true);
    assert_eq!(report["pager"]["source"], "MANPAGER");
    assert_eq!(report["pager"]["program"], "missing-pager");
    assert_eq!(report["pager"]["availability"], "not checked");
    assert!(!text.contains("secret-argument"));
    assert!(!text.contains("never-disclose-this"));
}

#[test]
fn doctor_validates_selected_config_and_profile() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("settings.toml");
    std::fs::write(&config, "[profiles.preview]\nwidth = 42\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "preview",
            "doctor",
            "--report",
            "json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["config"]["source"], config.to_str().unwrap());
    assert_eq!(report["config"]["profile"], "preview");
    assert_eq!(report["terminal"]["width"], 42);
    std::fs::write(&config, "[broken").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "doctor",
            "--config",
            config.to_str().unwrap(),
            "--report",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let _: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
}

#[test]
fn doctor_as_value_or_trailing_operand_is_not_a_command() {
    for args in [
        vec!["--no-config", "--print", "--", "doctor"],
        vec!["--no-config", "--print", "--title", "doctor", "hello"],
        vec!["--no-config", "--print", "hello", "doctor"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rich"))
            .args(args)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(!text.contains("stdout_tty"));
        assert!(!text.contains("Rich doctor"));
    }
}

#[test]
fn doctor_human_reports_inference_and_default_pager_without_starting_it() {
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["doctor", "--no-config", "--no-color"])
        .env_remove("MANPAGER")
        .env_remove("PAGER")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Rich doctor"));
    assert!(text.contains("inferred"));
    assert!(text.contains("not checked"));
    assert!(!text.contains('\x1b'));
}

#[test]
fn doctor_image_choice_is_validated_without_rendering() {
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "doctor",
            "--no-config",
            "--image-mode",
            "braille",
            "--report",
            "json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["image"]["requested_mode"], "braille");
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["doctor", "--no-config", "--image-mode", "bogus"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn doctor_color_overrides_match_rendering_precedence() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("settings.toml");
    std::fs::write(&config, "[defaults]\nno_color = false\n").unwrap();
    for flags in [
        vec!["--no-config", "--color"],
        vec!["--config", config.to_str().unwrap()],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rich"))
            .args(["doctor", "--report", "json"])
            .args(&flags)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["terminal"]["no_color"], false, "{flags:?}");
        // Color preference never forces terminal controls into redirected output.
        assert_eq!(report["terminal"]["color"], "none");
    }
    // A rich.toml found in the working directory cannot undo NO_COLOR.
    std::fs::write(
        root.path().join("rich.toml"),
        "[defaults]\nno_color = false\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["doctor", "--report", "json"])
        .current_dir(root.path())
        .env("NO_COLOR", "1")
        .env("HOME", root.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["terminal"]["no_color"], true);
}
