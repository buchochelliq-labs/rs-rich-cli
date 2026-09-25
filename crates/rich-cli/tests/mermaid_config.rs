#![cfg(all(feature = "mermaid", not(feature = "mmdc")))]
//! A configured Mermaid backend the build lacks (0.0.12 release test).

use std::process::{Command, Stdio};

/// In a build without mmdc (the default `cargo install`), a user config with
/// `mermaid_backend = "mmdc"`, perhaps shared from a machine with an mmdc
/// build, warns and draws Mermaid as text instead of failing every command.
#[test]
fn user_config_mmdc_does_not_break_unrelated_commands() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let home = root.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(home.join(".config/rich")).unwrap();
    std::fs::write(
        home.join(".config/rich/config.toml"),
        "[defaults]\nmermaid_backend = \"mmdc\"\n",
    )
    .unwrap();
    std::fs::write(work.join("a.txt"), "hello\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "60")
        .arg("a.txt")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "plain text failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
