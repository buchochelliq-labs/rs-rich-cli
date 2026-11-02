//! The terminal host client (#820) against a fake terminal on a Unix
//! socket, speaking terminal-browser's protocol, and against a tmux
//! server of the test's own, where tmux is installed.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

use rich_interact::host::{
    Direction, HostError, Method, Pane, SocketHost, Split, TerminalHost, Tmux,
};
use serde_json::{Value, json};

/// A fake terminal: it lists `methods` in `hello`, answers each request
/// with `answer`, and records every request it got.
fn fake_terminal(
    methods: &'static [&'static str],
    answer: fn(&Value) -> Value,
) -> (PathBuf, Arc<Mutex<Vec<Value>>>) {
    let dir = std::env::temp_dir().join(format!(
        "rich-host-{}-{}",
        std::process::id(),
        methods.len()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("terminal.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let request: Value = serde_json::from_str(&line).unwrap();
            record.lock().unwrap().push(request.clone());
            let reply = if request["method"] == "hello" {
                json!({"id": request["id"], "result": {"name": "fake", "methods": methods}})
            } else {
                answer(&request)
            };
            let _ = writeln!(stream, "{reply}");
        }
    });
    (path, seen)
}

#[test]
fn hello_lists_the_methods_and_only_those_are_sent() {
    let (path, seen) = fake_terminal(&["split", "sendText"], |request| {
        match request["method"].as_str() {
            Some("split") => json!({"id": 1, "result": {"pane": {"id": "p2"}}}),
            _ => json!({"id": 1, "result": {}}),
        }
    });
    let host = SocketHost::connect(&path).unwrap();
    assert_eq!(host.name(), "fake");
    assert!(host.supports(Method::Split) && host.supports(Method::SendText));
    assert!(!host.supports(Method::FocusPane));
    let pane = host
        .split(&Split::new(
            Direction::Right,
            vec!["noctiluca".into(), "https://example.com".into()],
        ))
        .unwrap();
    assert_eq!(pane.id, "p2");
    host.send_text(&pane, "hello agent").unwrap();
    // Not listed: refused here, nothing sent.
    assert!(matches!(
        host.focus_pane(&pane),
        Err(HostError::Unsupported("focusPane"))
    ));
    let seen = seen.lock().unwrap();
    let methods: Vec<&str> = seen.iter().filter_map(|r| r["method"].as_str()).collect();
    assert_eq!(methods, ["hello", "split", "sendText"]);
    assert_eq!(seen[1]["params"]["direction"], "right");
    assert_eq!(seen[1]["params"]["command"][1], "https://example.com");
    assert_eq!(
        seen[2]["params"],
        json!({"paneId": "p2", "text": "hello agent"})
    );
}

#[test]
fn an_error_reply_is_a_refusal_with_its_reason() {
    let (path, _) = fake_terminal(
        &["split", "focusPane", "getCurrentPane"],
        |_| json!({"id": 1, "error": {"message": "no room for a split"}}),
    );
    let host = SocketHost::connect(&path).unwrap();
    match host.split(&Split::new(Direction::Down, Vec::new())) {
        Err(HostError::Refused(why)) => assert_eq!(why, "no room for a split"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(host.current_pane(), Err(HostError::Refused(_))));
}

#[test]
fn a_path_that_is_not_a_socket_is_refused() {
    let file = std::env::temp_dir().join(format!("rich-host-file-{}", std::process::id()));
    std::fs::write(&file, "").unwrap();
    match SocketHost::connect(&file) {
        Err(HostError::Refused(why)) => assert!(why.contains("not a socket"), "{why}"),
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_file(file);
}

/// A tmux server of the test's own: split beside its first pane, list the
/// panes, find the neighbour, type into the new one and focus it.
#[test]
fn tmux_splits_types_and_focuses() {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("skipped: no tmux");
        return;
    }
    let socket = std::env::temp_dir().join(format!("rich-host-tmux-{}", std::process::id()));
    let tmux = |args: &[&str]| {
        Command::new("tmux")
            .arg("-S")
            .arg(&socket)
            .args(args)
            .output()
            .unwrap()
    };
    let started = tmux(&[
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-x",
        "80",
        "-y",
        "24",
        "sh",
    ]);
    assert!(started.status.success(), "{started:?}");
    let first = String::from_utf8_lossy(&tmux(&["display-message", "-p", "#{pane_id}"]).stdout)
        .trim()
        .to_string();
    let host = Tmux::new(Some(first.clone())).socket(&socket);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_eq!(host.current_pane().unwrap().id, first);
        let pane = host
            .split(&Split {
                direction: Direction::Right,
                command: vec!["sh".into()],
                size: Some(40),
            })
            .unwrap();
        assert!(pane.id.starts_with('%') && pane.id != first, "{pane:?}");
        let panes = host.list_panes().unwrap();
        assert_eq!(panes.len(), 2, "{panes:?}");
        assert_eq!(
            host.neighbor(Direction::Right).unwrap().map(|p| p.id),
            Some(pane.id.clone())
        );
        assert_eq!(host.neighbor(Direction::Left).unwrap(), None);
        host.send_text(&pane, "echo typed-by-the-host").unwrap();
        host.focus_pane(&Pane::new(first.clone())).unwrap();
        let active =
            String::from_utf8_lossy(&tmux(&["display-message", "-p", "#{pane_id}"]).stdout)
                .trim()
                .to_string();
        assert_eq!(active, first);
        // The text arrived in the new pane, as typed (no Enter).
        let shown = String::from_utf8_lossy(&tmux(&["capture-pane", "-p", "-t", &pane.id]).stdout)
            .to_string();
        assert!(shown.contains("echo typed-by-the-host"), "{shown}");
    }));
    tmux(&["kill-server"]);
    let _ = std::fs::remove_file(&socket);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
