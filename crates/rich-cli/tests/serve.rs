//! `rich serve`: only in a build with the `serve` feature. There, its help,
//! its usage errors, and a program served on 127.0.0.1 (the page refused
//! without the printed token, and given with it); without the feature, no
//! `serve` at all.

use std::process::{Command, Stdio};

/// Whether `--help` lists a `serve` command (its row starts with the word).
fn lists_serve(help: &[u8]) -> bool {
    String::from_utf8_lossy(help)
        .lines()
        .any(|line| line.trim_start().starts_with("serve "))
}

fn rich() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rich"));
    command.env("NO_COLOR", "1").env("COLUMNS", "100");
    command
}

#[cfg(not(feature = "serve"))]
#[test]
fn a_default_build_has_no_serve() {
    let help = rich().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(!lists_serve(&help.stdout));
    // The word is not a command: nothing is served.
    let run = rich()
        .args(["serve", "--", "true"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(!String::from_utf8_lossy(&run.stdout).contains("Serving"));
}

#[cfg(feature = "serve")]
#[test]
fn serve_has_help_and_usage_errors() {
    let help = rich().args(["serve", "--help"]).output().unwrap();
    assert!(help.status.success(), "{help:?}");
    let text = String::from_utf8_lossy(&help.stdout);
    for flag in [
        "--bind",
        "--port",
        "--max-sessions",
        "--allow-origin",
        "PROGRAM",
    ] {
        assert!(text.contains(flag), "{flag} in {text}");
    }
    let root = rich().arg("--help").output().unwrap();
    assert!(lists_serve(&root.stdout));

    for args in [
        &["serve"][..],
        &["serve", "--port", "nope", "--", "true"],
        &["serve", "--bogus", "--", "true"],
    ] {
        let out = rich().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("rich serve"));
    }
}

#[cfg(all(unix, feature = "serve"))]
#[test]
fn serve_prints_the_address_and_serves_the_page() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;

    let mut child = rich()
        .args([
            "serve",
            "--port",
            "0",
            "--max-sessions",
            "1",
            "--",
            "sh",
            "-c",
            "true",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let get = |target: &str, host: &str| {
        let mut stream = TcpStream::connect(host).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    };
    let result = std::panic::catch_unwind(|| {
        // "Serving sh at http://127.0.0.1:PORT/?token=…"
        let url = line
            .trim()
            .strip_prefix("Serving sh at http://")
            .unwrap_or_else(|| panic!("the address: {line:?}"));
        let (host, target) = url.split_once('/').unwrap();
        assert!(
            host.starts_with("127.0.0.1:"),
            "loopback by default: {host}"
        );
        let page = get(&format!("/{target}"), host);
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert!(page.contains("<title>sh</title>"));
        let refused = get("/", host);
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    });
    let _ = child.kill();
    let _ = child.wait();
    result.unwrap();
}

/// A program that ignores SIGHUP does not outlive `rich serve`: SIGTERM
/// ends every session, and each program is killed once it has had a
/// moment, before `rich` exits.
#[cfg(all(unix, feature = "serve"))]
#[test]
fn serve_ends_a_program_that_ignores_hangups_when_it_stops() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    let running = |pid: &str| {
        let out = Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
        !stat.is_empty() && !stat.starts_with('Z')
    };
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let script = format!(
        "trap '' HUP; echo $$ > '{}'; exec sleep 60",
        pid_file.display()
    );
    let mut child = rich()
        .args(["serve", "--port", "0", "--", "sh", "-c", &script])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let result = std::panic::catch_unwind(|| {
        let url = line
            .trim()
            .strip_prefix("Serving sh at http://")
            .unwrap_or_else(|| panic!("the address: {line:?}"));
        let (host, target) = url.split_once('/').unwrap();
        // Open a session, as the page does: the program starts.
        let mut stream = TcpStream::connect(host).unwrap();
        let token = target.trim_start_matches("?token=");
        write!(
            stream,
            "GET /ws?token={token}&cols=80&rows=24 HTTP/1.1\r\nHost: {host}\r\n\
             Origin: http://{host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        )
        .unwrap();
        let mut answer = [0u8; 12];
        stream.read_exact(&mut answer).unwrap();
        assert_eq!(&answer, b"HTTP/1.1 101");
        let start = Instant::now();
        let pid = loop {
            let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
            if !pid.trim().is_empty() {
                break pid.trim().to_string();
            }
            assert!(start.elapsed() < Duration::from_secs(10), "no program");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(running(&pid));
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap();
        (pid, stream)
    });
    // `rich` exits once its program has ended.
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if start.elapsed() > Duration::from_secs(20) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let (pid, _stream) = result.unwrap();
    let alive = running(&pid);
    if alive {
        let _ = Command::new("kill").args(["-KILL", &pid]).status();
    }
    assert_eq!(status.and_then(|s| s.code()), Some(128 + 15), "rich exited");
    assert!(!alive, "the program outlived `rich serve`");
}
