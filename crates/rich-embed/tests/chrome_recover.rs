//! A ChromeEngine whose browser goes away, against a real browser: a
//! renderer that crashes, and a browser process that is killed. Runs only
//! when `RICH_EMBED_CHROME` names a Chrome or Chromium binary (see
//! `tests/chrome.rs`). A file of its own, so the browsers this process
//! starts are the only ones it can find among its children; its tests take
//! turns.

#![cfg(all(feature = "chrome", target_os = "linux"))]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use rich_embed::{ChromeEngine, WebEngine, WebFrame};
use serde_json::json;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A red page, as a data address.
const RED: &str = "data:text/html,<title>red</title><body style='margin:0;background:%23ff0000'>";

fn browser() -> Option<std::ffi::OsString> {
    let binary = std::env::var_os("RICH_EMBED_CHROME");
    if binary.is_none() {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
    }
    binary
}

/// Poll `engine` until a frame arrives, for up to `within`.
fn a_frame(engine: &mut ChromeEngine, within: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < within {
        if let Some(WebFrame::Pixels(_)) = engine.poll() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// This process's children: the browsers it started.
fn children() -> Vec<i32> {
    let me = std::process::id().to_string();
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            // pid (comm) state ppid …: the name can hold spaces.
            let after = &stat[stat.rfind(')')? + 2..];
            let parent = after.split(' ').nth(1)?;
            (parent == me).then_some(pid)
        })
        .collect()
}

#[test]
fn a_crashed_page_fails_at_once_and_a_reload_brings_it_back() {
    let Some(binary) = browser() else { return };
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut engine = ChromeEngine::new().binary(binary);
    engine.resize(20, 6).unwrap();
    engine.open(RED).unwrap();
    assert!(
        a_frame(&mut engine, Duration::from_secs(60)),
        "no first frame"
    );
    let devtools = engine.devtools().unwrap();
    let evaluate = || {
        devtools.call(
            "Runtime.evaluate",
            json!({"expression": "1 + 1", "returnByValue": true}),
            Duration::from_secs(10),
        )
    };
    assert_eq!(evaluate().unwrap()["result"]["value"], 2);
    // Chrome does not answer Page.crash: the renderer is gone first.
    let _ = devtools.call("Page.crash", json!({}), Duration::from_secs(2));
    let start = Instant::now();
    while !engine.state().crashed && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(engine.state().crashed, "{:?}", engine.state());
    let asked = Instant::now();
    let error = evaluate().unwrap_err();
    assert!(
        asked.elapsed() < Duration::from_secs(1),
        "the call waited {:?}",
        asked.elapsed()
    );
    assert_eq!(error.to_string(), "the page crashed");
    assert!(!engine.ended(), "the browser lives");
    engine.reload().unwrap();
    let start = Instant::now();
    while engine.state().crashed && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!engine.state().crashed, "never reloaded");
    let start = Instant::now();
    let answer = loop {
        match evaluate() {
            Ok(answer) => break answer,
            Err(error) if start.elapsed() > Duration::from_secs(30) => panic!("{error}"),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    assert_eq!(answer["result"]["value"], 2);
}

#[test]
fn a_killed_browser_is_started_again_by_the_next_navigation() {
    let Some(binary) = browser() else { return };
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let before = children();
    let mut engine = ChromeEngine::new().binary(binary);
    engine.resize(20, 6).unwrap();
    engine.open(RED).unwrap();
    assert!(
        a_frame(&mut engine, Duration::from_secs(60)),
        "no first frame"
    );
    // The page's history is read (its title is in it).
    let start = Instant::now();
    while engine.state().title != "red" && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let started: Vec<i32> = children()
        .into_iter()
        .filter(|pid| !before.contains(pid))
        .collect();
    assert_eq!(started.len(), 1, "{started:?}");
    let devtools = engine.devtools().unwrap();
    // SAFETY: plain integers; the pid is this process's own child.
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(started[0], libc::SIGKILL);
    }
    let start = Instant::now();
    while !engine.ended() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(engine.ended());
    let error = engine.state().error.unwrap();
    assert!(
        error.starts_with("Chrome: the browser closed (killed by signal 9 (SIGKILL))"),
        "{error}"
    );
    // Gone, and reaped.
    assert!(!std::path::Path::new(&format!("/proc/{}", started[0])).exists());
    // The next navigation starts another, and DevTools from before reach
    // it.
    engine
        .open("data:text/html,<body style='margin:0;background:%2300ff00'>")
        .unwrap();
    assert!(!engine.ended());
    assert!(
        a_frame(&mut engine, Duration::from_secs(60)),
        "no frame after"
    );
    assert_eq!(engine.state().error, None);
    // It goes on from the page the old browser was at: back is there, and
    // the new browser's own history holds this page only.
    let start = Instant::now();
    while !engine.state().can_go_back && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(engine.state().can_go_back, "{:?}", engine.state());
    let answer = devtools
        .call(
            "Runtime.evaluate",
            json!({"expression": "history.length", "returnByValue": true}),
            Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(answer["result"]["value"], 1);
    // Back goes to the red page, which only the old browser had.
    engine.back().unwrap();
    let start = Instant::now();
    while engine.state().title != "red" && start.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(engine.state().title, "red", "{:?}", engine.state());
    assert!(engine.state().can_go_forward);
}
