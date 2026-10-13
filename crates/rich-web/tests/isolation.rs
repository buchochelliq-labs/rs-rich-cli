//! One process per session: the `isolated` example run as a server of its
//! own, its tabs crashing, hanging and running out of memory on purpose
//! while the others, and the server, carry on.

#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};

use common::{connect, read_close_reason, read_until, running, wait_for, TOKEN};
use tungstenite::{Message, WebSocket};

/// The `isolated` example, serving on a free port.
struct Server {
    child: Child,
    addr: SocketAddr,
}

impl Server {
    fn start() -> Server {
        Server::start_with(&[])
    }

    fn start_with(extra: &[&str]) -> Server {
        // Examples are built next to the test binaries, in `examples/`.
        let exe = std::env::current_exe().unwrap();
        let example = exe
            .parent()
            .and_then(|deps| deps.parent())
            .unwrap()
            .join("examples")
            .join("isolated");
        assert!(
            example.exists(),
            "the isolated example is built: {example:?}"
        );
        let mut child = Command::new(example)
            .args(["127.0.0.1:0", "--token", TOKEN, "--memory"])
            .arg((512u64 << 20).to_string())
            .args(["--cpu", "20"])
            .args(extra)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        // "Serving … at http://127.0.0.1:PORT/?token=…"
        let addr = line
            .split("http://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or_else(|| panic!("the address in {line:?}"))
            .parse()
            .unwrap();
        Server { child, addr }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A session, and the id of the process it runs in.
fn session(server: &Server) -> (WebSocket<TcpStream>, u32) {
    let mut socket = connect(server.addr, 60, 4);
    let seen = read_until(&mut socket, "waiting");
    let pid = seen
        .split("pid ")
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("a pid in {seen:?}"));
    (socket, pid)
}

/// The session still answers `+`.
fn answers(socket: &mut WebSocket<TcpStream>) {
    socket.send(Message::text("d+")).unwrap();
    read_until(socket, "ANSWERED");
}

#[test]
fn each_tab_runs_in_a_process_of_its_own() {
    let server = Server::start();
    let (mut a, pid_a) = session(&server);
    let (mut b, pid_b) = session(&server);
    assert_ne!(pid_a, pid_b);
    assert_ne!(pid_a, server.pid());
    answers(&mut a);
    answers(&mut b);
    // The tab closes: its process ends.
    drop(a);
    wait_for("the closed tab's process to end", || !running(pid_a));
    assert!(running(pid_b));
}

#[test]
fn a_crash_ends_its_own_session_only() {
    let server = Server::start();
    let (mut a, pid_a) = session(&server);
    let (mut b, _) = session(&server);
    a.send(Message::text("dc")).unwrap();
    let (_, reason) = read_close_reason(&mut a);
    assert!(reason.contains("ended unexpectedly"), "{reason:?}");
    wait_for("the crashed process to be gone", || !running(pid_a));
    answers(&mut b);
    // And a new tab still starts.
    let (mut c, _) = session(&server);
    answers(&mut c);
}

#[test]
fn a_hang_holds_up_its_own_session_only() {
    let server = Server::start();
    let (mut a, pid_a) = session(&server);
    let (mut b, _) = session(&server);
    a.send(Message::text("dh")).unwrap();
    answers(&mut b);
    // Closing the hung tab kills its process.
    drop(a);
    wait_for("the hung process to be killed", || !running(pid_a));
    answers(&mut session(&server).0);
}

// macOS has no address-space limit.
#[cfg(not(target_vendor = "apple"))]
#[test]
fn running_out_of_memory_ends_its_own_session_only() {
    let server = Server::start();
    let (mut a, pid_a) = session(&server);
    let (mut b, _) = session(&server);
    a.send(Message::text("dm")).unwrap();
    let (_, reason) = read_close_reason(&mut a);
    assert!(reason.contains("ended unexpectedly"), "{reason:?}");
    wait_for("the process to be gone", || !running(pid_a));
    answers(&mut b);
}

#[cfg(target_os = "linux")]
#[test]
fn a_server_that_dies_takes_its_sessions_with_it() {
    let mut server = Server::start();
    let (mut a, pid_a) = session(&server);
    a.send(Message::text("dh")).unwrap();
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    wait_for("the hung session's process to die with its server", || {
        !running(pid_a)
    });
}

#[cfg(target_os = "linux")]
#[test]
fn the_limits_hold_from_the_start_of_a_sessions_process() {
    // Each session's process allocates 1 GiB before it reaches `serve`,
    // past its 512 MiB limit: it never starts the app, and the server
    // says so.
    let server = Server::start_with(&["--startup-memory", &(1u64 << 30).to_string()]);
    let mut socket = connect(server.addr, 60, 4);
    let (_, reason) = read_close_reason(&mut socket);
    assert!(reason.contains("before it started the app"), "{reason:?}");
}
