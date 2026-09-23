use std::process::Command;
#[test]
fn rich_log_view_is_opt_in_and_config_can_be_overridden() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("events.jsonl");
    std::fs::write(
        &file,
        "{\"level\":\"warn\",\"message\":\"[red]literal[/red]\",\"count\":3}\n",
    )
    .unwrap();
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rich"))
            .args(["--no-config", "--no-color", "log", file.to_str().unwrap()])
            .args(extra)
            .output()
            .unwrap()
    };
    let plain = run(&[]);
    let explicit = run(&["--log-presentation", "plain"]);
    assert!(explicit.status.success());
    assert_eq!(plain.stdout, explicit.stdout);
    let rich = run(&["--log-presentation", "rich"]);
    assert!(
        rich.status.success(),
        "{}",
        String::from_utf8_lossy(&rich.stderr)
    );
    let text = String::from_utf8(rich.stdout).unwrap();
    assert!(text.contains("count=3"));
    assert!(text.contains("[red]literal[/red]"));
    let config = temp.path().join("rich.toml");
    std::fs::write(&config, "[defaults]\nlog_presentation = 'rich'\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--no-color",
            "log",
            file.to_str().unwrap(),
            "--log-presentation",
            "plain",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, plain.stdout);
}
