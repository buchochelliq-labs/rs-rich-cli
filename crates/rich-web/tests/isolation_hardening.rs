//! Process-per-session isolation against a hostile or broken neighbourhood
//! (the 0.0.19 release test): a killed server, a program the app runs, a
//! stranger on the session's port, a startup that outruns its limits.

#![cfg(target_os = "linux")]

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use common::{connect, read_close_reason_within, read_until, running, wait_for, TOKEN};
use tungstenite::{Message, WebSocket};

fn probe_exe() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap();
    let p = exe
        .parent()
        .and_then(|d| d.parent())
        .unwrap()
        .join("examples")
        .join("session_probe");
    assert!(p.exists(), "build the example: {p:?}");
    p
}

struct Probe {
    child: Child,
    addr: SocketAddr,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl Probe {
    fn start(extra: &[&str]) -> Probe {
        let mut child = Command::new(probe_exe())
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
        let _ = Command::new("kill")
            .args(["-KILL", &self.0.to_string()])
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

/// Press `key`, read up to `END`, give the number after `word`.
fn press(socket: &mut WebSocket<TcpStream>, key: &str, word: &str) -> u32 {
    socket.send(Message::text(format!("d{key}"))).unwrap();
    let seen = read_until(socket, "END");
    number_after(&seen, word)
}

fn kill(pid: u32, signal: &str) {
    Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .unwrap();
}

/// Whether `pid` is still alive (not a zombie) after up to `within`.
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

// ---------------------------------------------------------------------------
// Orphans

/// The server is killed (SIGKILL, and below SIGTERM): its session's process
/// sees its parent go and ends its whole process group, so what it started
/// goes too.
fn server_signal_leaves_no_orphan(signal: &str) {
    let probe = Probe::start(&[]);
    let (mut socket, session_pid) = session(&probe);
    let grandchild = press(&mut socket, "f", "FORKED ");
    let _reap = Reap(grandchild);
    assert!(running(grandchild));
    kill(probe.child.id(), signal);
    wait_for("the session's process to die with its server", || {
        !running(session_pid)
    });
    assert!(
        !still_running_after(grandchild, Duration::from_secs(5)),
        "{signal}: the session's child {grandchild} outlived the server"
    );
}

#[test]
fn sigkill_of_the_server_leaves_no_orphan() {
    server_signal_leaves_no_orphan("-KILL");
}

#[test]
fn sigterm_of_the_server_leaves_no_orphan() {
    server_signal_leaves_no_orphan("-TERM");
}

// ---------------------------------------------------------------------------
// The session variable leaks into what the app runs

/// A program the app runs inherits `RS_RICH_WEB_SESSION`; when it serves
/// too, it binds as it would anywhere (it once took the variable for its
/// own, tried to reach a port that was gone, and exited 1).
#[test]
fn a_program_the_app_runs_can_still_serve() {
    let probe = Probe::start(&[]);
    let (mut socket, _) = session(&probe);
    socket.send(Message::text("dn")).unwrap();
    let seen = read_until(&mut socket, "END");
    // Only the changed cells are sent: the words come in pieces.
    assert!(
        seen.contains("code=0") && seen.contains("bound=true"),
        "the nested server did not bind: {seen:?}"
    );
}

/// The same variable with another bind number: `Server::bind` still binds
/// the address asked for, not an ephemeral loopback port.
#[test]
fn an_inherited_variable_does_not_move_the_address() {
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let want = free.local_addr().unwrap();
    drop(free);
    let out = Command::new(probe_exe())
        .args(["--nested", &want.to_string()])
        .env(
            rich_web::SESSION_ENV,
            "1 1 00000000000000000000000000000000 7 80 24 xterm - -",
        )
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(&format!("BOUND {want}")),
        "asked for {want}, got {text:?}"
    );
}

// ---------------------------------------------------------------------------
// The handshake

/// The loopback listeners `pid` holds, from /proc (what any local user can
/// read of /proc/net/tcp, narrowed here to the server's own sockets).
fn listening_ports(pid: u32) -> Vec<u16> {
    let mut inodes = Vec::new();
    if let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
        for fd in fds.flatten() {
            if let Ok(link) = std::fs::read_link(fd.path()) {
                let link = link.to_string_lossy().to_string();
                if let Some(inode) = link
                    .strip_prefix("socket:[")
                    .and_then(|s| s.strip_suffix(']'))
                {
                    inodes.push(inode.to_string());
                }
            }
        }
    }
    let table = std::fs::read_to_string("/proc/net/tcp").unwrap();
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            // local_address, st == 0A (LISTEN), inode
            let (local, state, inode) = (f[1], f[3], f[9]);
            (state == "0A" && inodes.iter().any(|i| i == inode))
                .then(|| u16::from_str_radix(local.split(':').nth(1).unwrap(), 16).unwrap())
        })
        .collect()
}

/// The session's own loopback listener, once the server opens it.
fn session_port(probe: &Probe) -> u16 {
    let main = probe.addr.port();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(p) = listening_ports(probe.child.id())
            .into_iter()
            .find(|p| *p != main)
        {
            return p;
        }
        assert!(Instant::now() < until, "no session listener appeared");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Another local process (any user can connect to a loopback port) opens a
/// few idle connections to the session's port before the session's process
/// does. They are read side by side, so the real process is not held up
/// behind them.
#[test]
fn idle_connections_to_the_session_port_do_not_stop_a_session() {
    // A program whose start takes 1.5 s (an interpreter, say).
    let probe = Probe::start(&["--startup-sleep", "1500"]);
    let mut page = connect(probe.addr, 60, 4);
    let port = session_port(&probe);
    let squatters: Vec<TcpStream> = (0..8)
        .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
        .collect();
    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(25);
    let mut seen = String::new();
    let mut outcome = None;
    while Instant::now() < deadline {
        match page.read() {
            Ok(Message::Text(t)) => {
                seen.push_str(t.as_str());
                if seen.contains("waiting") {
                    outcome = Some("the app started".to_string());
                    break;
                }
            }
            Ok(Message::Close(f)) => {
                outcome = Some(format!("closed: {:?}", f.map(|f| f.reason.to_string())));
                break;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                outcome = Some(format!("error {e}"));
                break;
            }
        }
    }
    drop(squatters);
    assert_eq!(
        outcome.as_deref(),
        Some("the app started"),
        "after {:?}",
        started.elapsed()
    );
}

/// One connection that sends a byte every 1.5 s: its hello has 2 s in all,
/// and stopping the server does not wait for the trickle.
#[test]
fn a_trickling_connection_does_not_hold_up_stopping_the_server() {
    let mut probe = Probe::start(&["--startup-sleep", "1500", "--stdin-stop"]);
    let _page = connect(probe.addr, 60, 4);
    let port = session_port(&probe);
    let mut squatter = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let trickle = std::thread::spawn(move || {
        // ~12 s of one byte each 1.5 s, then go away.
        for _ in 0..8 {
            if squatter.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
    std::thread::sleep(Duration::from_millis(500));
    // Stop the server.
    probe.stdin.take().unwrap().write_all(b"stop\n").unwrap();
    let mut line = String::new();
    probe.stdout.as_mut().unwrap().read_line(&mut line).unwrap();
    trickle.join().unwrap();
    let took = number_after(&line, "STOPPED ");
    assert!(took < 3000, "stopping the server took {took} ms");
}

// ---------------------------------------------------------------------------
// Keep the helper used even when a test above is filtered out.
#[allow(dead_code)]
fn _unused(s: &mut TcpStream) {
    let _ = s.read(&mut [0u8; 1]);
    let _ = read_close_reason_within;
}

// ---------------------------------------------------------------------------
// The limits from the start

/// `taskset -c 0 isolated --startup-memory 1GiB --memory 512MiB`: every session's process
/// allocates 1 GiB before `serve`, which the docs say the limit covers on
/// Linux. Ten tabs: none reaches the app, even on one CPU, where the child
/// can allocate before `prlimit` (after `spawn`) sets its limit: the server
/// then refuses a process that already took more.
#[test]
fn the_memory_limit_holds_for_every_sessions_startup() {
    let exe = probe_exe().with_file_name("isolated");
    // On one CPU (a one-vCPU VM or container, or `taskset`), the child gets
    // to run before the server's thread returns from `spawn` to `prlimit`.
    let mut child = Command::new("taskset")
        .args(["-c", "0"])
        .arg(exe)
        .args(["127.0.0.1:0", "--token", TOKEN, "--memory"])
        .arg((512u64 << 20).to_string())
        .args(["--startup-memory", &(1u64 << 30).to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let addr: SocketAddr = line
        .split("http://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap()
        .parse()
        .unwrap();
    let mut started = Vec::new();
    for _ in 0..10 {
        let mut page = connect(addr, 60, 4);
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            match page.read() {
                Ok(Message::Text(t)) if t.as_str().contains("waiting") => {
                    let pid = number_after(t.as_str(), "pid ");
                    let status =
                        std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
                    let vm = status
                        .lines()
                        .find(|l| l.starts_with("VmSize"))
                        .unwrap_or("")
                        .to_string();
                    let limits =
                        std::fs::read_to_string(format!("/proc/{pid}/limits")).unwrap_or_default();
                    let cap = limits
                        .lines()
                        .find(|l| l.starts_with("Max address space"))
                        .unwrap_or("")
                        .to_string();
                    started.push(format!("pid {pid}: {vm} / {cap}"));
                    break;
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        started.is_empty(),
        "{} of 10 sessions ran the app past their limit: {started:#?}",
        started.len()
    );
}

/// Sound: stopping the server (`Handle::stop`) ends a session's child that
/// stayed in its process group.
#[test]
fn stopping_the_server_ends_a_sessions_child() {
    let mut probe = Probe::start(&["--stdin-stop"]);
    let (mut socket, session_pid) = session(&probe);
    let grandchild = press(&mut socket, "f", "FORKED ");
    let _reap = Reap(grandchild);
    probe.stdin.take().unwrap().write_all(b"stop\n").unwrap();
    let mut line = String::new();
    probe.stdout.as_mut().unwrap().read_line(&mut line).unwrap();
    assert!(line.contains("STOPPED"), "{line:?}");
    assert!(!running(session_pid));
    assert!(!still_running_after(grandchild, Duration::from_secs(2)));
}

/// The server dies while a session's process is still starting, and
/// another local process binds the session's port. The session's process
/// gives it nothing: it sees its server has gone, and a stranger on the
/// port could not answer with the server's reply anyway.
#[test]
fn a_session_process_does_not_serve_whoever_takes_its_port() {
    let probe = Probe::start(&["--startup-sleep", "2000"]);
    let _page = connect(probe.addr, 60, 4);
    let port = session_port(&probe);
    let server_pid = probe.child.id();
    kill(server_pid, "-KILL");
    // Another process takes the port the server listened on, once it can.
    let until = Instant::now() + Duration::from_secs(2);
    let squatter = loop {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => break l,
            Err(e) if Instant::now() < until => {
                let _ = e;
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("binding {port}: {e}"),
        }
    };
    squatter.set_nonblocking(true).unwrap();
    let until = Instant::now() + Duration::from_secs(8);
    let stolen = loop {
        match squatter.accept() {
            Ok((s, _)) => break Some(s),
            Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let Some(stream) = stolen else { return };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut secret = String::new();
    reader.read_line(&mut secret).unwrap();
    // Speak WebSocket to it as the server's relay would: frames straight
    // away, no HTTP. Read what the app draws.
    let mut seen = Vec::new();
    let mut buf = [0u8; 4096];
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until && !String::from_utf8_lossy(&seen).contains("waiting") {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => seen.extend_from_slice(&buf[..n]),
        }
    }
    let seen = String::from_utf8_lossy(&seen).to_string();
    if seen.contains("pid ") {
        let _reap = Reap(number_after(&seen, "pid "));
    }
    panic!(
        "a process that took the port got secret {:?} and the app: {:?}",
        secret.trim(),
        seen.chars().filter(|c| !c.is_control()).collect::<String>()
    );
}
