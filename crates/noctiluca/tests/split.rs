//! `noctiluca --split right …` (#820) under a tmux server of the test's
//! own: the binary asks tmux for a pane beside its own, running the same
//! command line without `--split`, and returns. Skips without tmux.
#![cfg(unix)]

use std::process::Command;

#[test]
fn split_opens_the_same_command_line_in_a_pane_beside() {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("skipped: no tmux");
        return;
    }
    let socket = std::env::temp_dir().join(format!("noctiluca-split-{}", std::process::id()));
    let tmux = |args: &[&str]| {
        Command::new("tmux")
            .arg("-S")
            .arg(&socket)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(
        tmux(&[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-x",
            "100",
            "-y",
            "30",
            "sh"
        ])
        .status
        .success()
    );
    // The new pane stays once its command exits, to be read.
    tmux(&["set-option", "-g", "remain-on-exit", "on"]);
    let first = String::from_utf8_lossy(&tmux(&["display-message", "-p", "#{pane_id}"]).stdout)
        .trim()
        .to_string();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let out = Command::new(env!("CARGO_BIN_EXE_noctiluca"))
            .args([
                "--split",
                "right",
                "--dump",
                "text",
                "file:///nonexistent/split.html",
            ])
            .env("TMUX", format!("{},1,0", socket.display()))
            .env("TMUX_PANE", &first)
            .env_remove("TERMINAL_BROWSER_TERMINAL_SOCKET")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(out.status.success(), "{said}");
        assert!(said.contains("opened right in tmux (pane %"), "{said}");
        let pane = said
            .rsplit("(pane ")
            .next()
            .unwrap()
            .trim()
            .trim_end_matches(')')
            .to_string();
        // The pane runs the same line, less --split, each word quoted.
        let started = String::from_utf8_lossy(
            &tmux(&[
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{pane_start_command}",
            ])
            .stdout,
        )
        .trim()
        .to_string();
        assert!(
            started.contains("'--dump' 'text' 'file:///nonexistent/split.html'"),
            "{started}"
        );
        assert!(
            started.contains(env!("CARGO_BIN_EXE_noctiluca")),
            "{started}"
        );
        assert!(!started.contains("--split"), "{started}");
        let panes = String::from_utf8_lossy(&tmux(&["list-panes", "-F", "#{pane_id}"]).stdout)
            .lines()
            .count();
        assert_eq!(panes, 2);
    }));
    tmux(&["kill-server"]);
    let _ = std::fs::remove_file(&socket);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn split_with_no_terminal_that_can_split_says_so() {
    let out = Command::new(env!("CARGO_BIN_EXE_noctiluca"))
        .args(["--split", "down", "x.html"])
        .env_remove("TMUX")
        .env_remove("ZELLIJ")
        .env_remove("WEZTERM_PANE")
        .env_remove("KITTY_WINDOW_ID")
        .env_remove("TERMINAL_BROWSER_TERMINAL_SOCKET")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("needs a terminal that can split"), "{said}");
}
