//! A real program on a real PTY, in a pane.

#![cfg(unix)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rich_embed::{terminal, ExitStatus, LocalPty, PtyHost};
use rich_intuituive::prelude::*;

#[test]
fn a_program_runs_in_the_pane_and_its_exit_is_reported() {
    let exited: Rc<RefCell<Option<ExitStatus>>> = Rc::default();
    let seen = exited.clone();
    let app = App::new(move || {
        terminal(["sh", "-c", "printf hi; exit 3"])
            .on_exit(move |status, _| *seen.borrow_mut() = Some(status))
            .node()
    });
    let mut driver = app.driver(20, 3);
    let start = Instant::now();
    // Output and the exit come from the PTY's threads, through the app's
    // proxy, picked up by `update`.
    while exited.borrow().is_none() && start.elapsed() < Duration::from_secs(10) {
        driver.update(start.elapsed());
        driver.render();
        std::thread::sleep(Duration::from_millis(10));
    }
    driver.update(start.elapsed());
    driver.render();
    assert_eq!(exited.borrow().as_ref().map(ExitStatus::code), Some(3));
    assert_eq!(driver.screen().plain()[0].trim_end(), "hi");
}

#[test]
fn a_local_pty_carries_bytes_both_ways() {
    let mut pty = LocalPty::new(["cat"]);
    pty.start(20, 4).unwrap();
    pty.write(b"ping\r").unwrap();
    let start = Instant::now();
    let mut out = Vec::new();
    while !String::from_utf8_lossy(&out).contains("ping\r\n")
        && start.elapsed() < Duration::from_secs(10)
    {
        out.extend(pty.read());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(String::from_utf8_lossy(&out).contains("ping"), "{out:?}");
    pty.resize(30, 5).unwrap();
    pty.kill().unwrap();
    let start = Instant::now();
    while pty.exit_status().is_none() && start.elapsed() < Duration::from_secs(10) {
        pty.read();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(pty.exit_status().is_some_and(|status| !status.success()));
}

#[test]
fn backpressure_holds_the_program_until_its_output_is_read() {
    let script = "head -c 300000 /dev/zero | tr '\\0' x; echo; echo END";
    let mut pty = LocalPty::new(["sh", "-c", script]).backpressure(4096);
    pty.start(80, 24).unwrap();
    // Nothing is read: the program fills the little that is held, then
    // waits, so it has not finished, and nothing was dropped.
    std::thread::sleep(Duration::from_millis(500));
    assert!(pty.exit_status().is_none());
    let mut out = pty.read();
    assert!(
        !out.is_empty() && out.len() <= 4096 + 65536,
        "{} bytes held",
        out.len()
    );
    let start = Instant::now();
    while pty.exit_status().is_none() && start.elapsed() < Duration::from_secs(20) {
        out.extend(pty.read());
        std::thread::sleep(Duration::from_millis(2));
    }
    out.extend(pty.read());
    assert_eq!(pty.exit_status().map(|status| status.code()), Some(0));
    let text = String::from_utf8_lossy(&out);
    assert_eq!(text.matches('x').count(), 300_000);
    assert!(text.contains("END"), "the end of the output arrived");
}

/// Whether process `pid` is still running (and not just waiting to be
/// reaped).
fn running(pid: u32) -> bool {
    std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .map(|out| {
            let stat = String::from_utf8_lossy(&out.stdout);
            let stat = stat.trim();
            !stat.is_empty() && !stat.starts_with('Z')
        })
        .unwrap_or(false)
}

/// Read until `pid:N` arrives: the program's process id.
fn read_pid(pty: &mut LocalPty) -> u32 {
    let start = Instant::now();
    let mut out = String::new();
    while start.elapsed() < Duration::from_secs(10) {
        out.push_str(&String::from_utf8_lossy(&pty.read()));
        if let Some(pid) = out
            .split("pid:")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|pid| pid.parse().ok())
        {
            return pid;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no pid in {out:?}");
}

#[test]
fn a_program_that_ignores_hangups_is_killed() {
    let script = "trap '' HUP; echo pid:$$; exec sleep 60";
    // Killed: hung up on, then, a moment later, killed.
    let mut pty = LocalPty::new(["sh", "-c", script]);
    pty.start(20, 4).unwrap();
    let pid = read_pid(&mut pty);
    pty.kill().unwrap();
    let start = Instant::now();
    while pty.exit_status().is_none() && start.elapsed() < Duration::from_secs(10) {
        pty.read();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(pty.exit_status().is_some_and(|status| !status.success()));
    assert!(!running(pid));

    // Dropped, as a pane that leaves the tree is: the same.
    let mut pty = LocalPty::new(["sh", "-c", script]);
    pty.start(20, 4).unwrap();
    let pid = read_pid(&mut pty);
    drop(pty);
    let start = Instant::now();
    while running(pid) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!running(pid), "the program outlived its host");
}

#[test]
fn input_a_program_does_not_read_is_bounded() {
    // Raw mode: the PTY holds a little of what is not read, then the
    // writes to it wait.
    let mut pty = LocalPty::new(["sh", "-c", "stty raw -echo; echo ready; exec sleep 60"]);
    pty.start(20, 4).unwrap();
    let start = Instant::now();
    let mut out = Vec::new();
    while !String::from_utf8_lossy(&out).contains("ready") {
        assert!(start.elapsed() < Duration::from_secs(10), "{out:?}");
        out.extend(pty.read());
        std::thread::sleep(Duration::from_millis(10));
    }
    let chunk = vec![b'x'; 64 * 1024];
    let mut taken = 0;
    let refused = loop {
        match pty.write(&chunk) {
            Ok(()) => taken += chunk.len(),
            Err(error) => break error,
        }
        assert!(taken <= 64 << 20, "{taken} bytes taken, none read");
    };
    assert_eq!(refused.kind(), std::io::ErrorKind::WouldBlock);
    // 1 MiB held, one write more, and what the PTY itself holds.
    assert!(taken <= (1 << 20) + (512 << 10), "{taken} bytes taken");
    // Still refused a moment later: the program reads none of it.
    std::thread::sleep(Duration::from_millis(100));
    assert!(pty.write(b"y").is_err());
}
