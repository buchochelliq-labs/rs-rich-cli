use std::process::Command;

#[test]
fn redirected_output_never_launches_pager() {
    for flags in [
        vec!["--pager"],
        vec!["--auto-pager"],
        vec!["--pager", "--no-pager"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rich"))
            .arg("--no-config")
            .args(flags)
            .args(["--print", "hello"])
            .env("MANPAGER", "rich-pager-that-does-not-exist")
            .env("PAGER", "rich-pager-that-does-not-exist")
            .env_remove("NO_COLOR")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));
    }
}

#[test]
fn negative_flags_override_profile_booleans() {
    let root = std::env::temp_dir().join(format!("rich-pager-v8-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let config = root.join("rich.toml");
    std::fs::write(
        &config,
        "[defaults]\npager = true\nbatch = true\nsanitize = true\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .arg("--config")
        .arg(config)
        .args([
            "--no-pager",
            "--no-batch",
            "--no-sanitize",
            "--print",
            "hello",
        ])
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(root);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("hello"));
}

#[test]
fn interactive_modes_cannot_be_combined_with_batch_or_paging() {
    for flags in [
        vec!["--watch", "--batch", "--dry-run"],
        vec!["--watch", "--pager"],
        vec!["--watch", "--auto-pager"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rich"))
            .arg("--no-config")
            .args(flags)
            .arg("file.json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("--watch cannot"));
    }
}
