//! Process-per-session isolation on Windows: each session's process in a
//! job object the server holds, so that ending the session, stopping the
//! server or killing it ends the process and what it started (the Unix
//! tests in `isolation.rs` and `isolation_hardening.rs` do the same with
//! process groups).

#![cfg(windows)]

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use common::{connect, read_until, running, wait_for, TOKEN};
use tungstenite::{Message, WebSocket};

/// An example, built next to the test binaries, in `examples/`.
fn example(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let p = exe
        .parent()
        .and_then(|d| d.parent())
        .unwrap()
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(p.exists(), "build the example: {p:?}");
    p
}

/// An example serving on a free port.
struct Probe {
    child: Child,
    addr: SocketAddr,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl Probe {
    fn start(name: &str, extra: &[&str]) -> Probe {
        let mut child = Command::new(example(name))
            .args(["127.0.0.1:0", "--token", TOKEN])
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let addr = line
            .split("http://")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .unwrap_or_else(|| panic!("address in {line:?}"))
            .parse()
            .unwrap();
        let stdin = child.stdin.take();
        Probe {
            child,
            addr,
            stdin,
            stdout: Some(stdout),
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Kills a stray pid when the test ends, whatever happened.
struct Reap(u32);

impl Drop for Reap {
    fn drop(&mut self) {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &self.0.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn number_after(seen: &str, word: &str) -> u32 {
    seen.rsplit(word)
        .next()
        .and_then(|r| r.trim_start().split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("a number after {word:?} in {seen:?}"))
}

/// A session, and its process id.
fn session(probe: &Probe) -> (WebSocket<TcpStream>, u32) {
    let mut socket = connect(probe.addr, 60, 4);
    let seen = read_until(&mut socket, "waiting");
    (socket, number_after(&seen, "pid "))
}

/// A session of the probe whose process has started a program (`ping`,
/// waiting a thousand seconds): the session's process id and the
/// program's.
fn session_with_a_child(probe: &Probe) -> (WebSocket<TcpStream>, u32, u32) {
    let (mut socket, pid) = session(probe);
    socket.send(Message::text("df")).unwrap();
    let seen = read_until(&mut socket, "END");
    let child = number_after(&seen, "FORKED ");
    assert!(running(child), "the session's child {child} did not start");
    (socket, pid, child)
}

/// Whether `pid` is still running after up to `within`.
fn still_running_after(pid: u32, within: Duration) -> bool {
    let until = Instant::now() + within;
    while Instant::now() < until {
        if !running(pid) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    running(pid)
}

#[test]
fn closing_the_tab_ends_the_session_and_what_it_started() {
    let probe = Probe::start("session_probe", &[]);
    let (socket, session_pid, child) = session_with_a_child(&probe);
    let _reap = Reap(child);
    drop(socket);
    wait_for("the closed tab's process to end", || !running(session_pid));
    assert!(
        !still_running_after(child, Duration::from_secs(5)),
        "the session's child {child} outlived its session"
    );
    // The server carries on.
    assert!(running(probe.child.id()));
}

#[test]
fn stopping_the_server_ends_a_sessions_child() {
    let mut probe = Probe::start("session_probe", &["--stdin-stop"]);
    let (_socket, session_pid, child) = session_with_a_child(&probe);
    let _reap = Reap(child);
    probe.stdin.take().unwrap().write_all(b"stop\n").unwrap();
    let mut line = String::new();
    probe.stdout.as_mut().unwrap().read_line(&mut line).unwrap();
    assert!(line.contains("STOPPED"), "{line:?}");
    assert!(!still_running_after(session_pid, Duration::from_secs(5)));
    assert!(!still_running_after(child, Duration::from_secs(5)));
}

/// The server is killed (`TerminateProcess`, as Task Manager's End task
/// does), so it cannot end anything itself: the system closes its handle to
/// the session's job, and the job kills the session's process and its
/// child.
#[test]
fn killing_the_server_leaves_no_orphan() {
    let mut probe = Probe::start("session_probe", &[]);
    let (_socket, session_pid, child) = session_with_a_child(&probe);
    let _reap = Reap(child);
    probe.child.kill().unwrap();
    probe.child.wait().unwrap();
    wait_for("the session's process to die with its server", || {
        !running(session_pid)
    });
    assert!(
        !still_running_after(child, Duration::from_secs(5)),
        "the session's child {child} outlived the server"
    );
}

/// A session hung in a loop dies with its server.
#[test]
fn a_server_that_dies_takes_a_hung_session_with_it() {
    let mut server = Probe::start("isolated", &[]);
    let (mut socket, pid) = session(&server);
    let _reap = Reap(pid);
    socket.send(Message::text("dh")).unwrap();
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    wait_for("the hung session's process to die with its server", || {
        !running(pid)
    });
}
