//! BrowshEngine against a stand-in for Browsh: a script that writes down
//! how it was started, starts a child of its own (as Browsh starts
//! Firefox), and idles.

#![cfg(all(feature = "browsh", unix))]

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use rich_embed::{BrowshEngine, WebEngine};

/// Whether process `pid` is still running (and not just waiting to be
/// reaped).
fn running(pid: &str) -> bool {
    std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .map(|out| {
            let stat = String::from_utf8_lossy(&out.stdout);
            let stat = stat.trim();
            !stat.is_empty() && !stat.starts_with('Z')
        })
        .unwrap_or(false)
}

#[test]
fn browsh_serves_this_computer_only_and_ends_with_its_group() {
    let dir = std::env::temp_dir().join(format!("rich-embed-fake-browsh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("browsh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             out='{dir}'\n\
             printf '%s\\n' \"$@\" > \"$out/argv\"\n\
             for home in \"$XDG_CONFIG_HOME\" \"$HOME/Library/Application Support\"; do\n\
             if [ -f \"$home/browsh/config.toml\" ]; then\n\
             printf '%s' \"$home\" > \"$out/home\"; cat \"$home/browsh/config.toml\" > \"$out/config\"\n\
             fi\n\
             done\n\
             sleep 60 &\n\
             printf '%s' $! > \"$out/child\"\n\
             touch \"$out/ready\"\n\
             wait\n",
            dir = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut engine = BrowshEngine::new().program(script.display().to_string());
    engine.resize(80, 24).unwrap();
    engine.open("https://example.com").unwrap();
    let start = Instant::now();
    while !dir.join("ready").exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "Browsh did not start"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    let (argv, home, config, child) = (read("argv"), read("home"), read("config"), read("child"));
    let config_existed = std::path::Path::new(&home).is_dir();
    let child_ran = running(&child);
    drop(engine);
    // Its whole group ends with the engine: Browsh and what it started.
    let start = Instant::now();
    while running(&child) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let child_left = running(&child);
    let config_left = !home.is_empty() && std::path::Path::new(&home).exists();
    let _ = std::fs::remove_dir_all(&dir);
    if child_left {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &child])
            .status();
    }

    assert_eq!(argv.trim(), "--http-server-mode");
    assert!(
        config_existed,
        "a configuration of the engine's own: {home:?}"
    );
    assert!(config.contains("bind = \"127.0.0.1\""), "{config}");
    assert!(child_ran);
    assert!(!child_left, "Browsh's child outlived the engine");
    assert!(!config_left, "the configuration outlived the engine");
}
