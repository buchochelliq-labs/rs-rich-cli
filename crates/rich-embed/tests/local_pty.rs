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
