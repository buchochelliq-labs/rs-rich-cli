//! The port of ratatui's `async-github` (examples/pulls.rs): the table
//! while it loads, once it has, and when the load failed, as 80x24
//! snapshots; j/k and the mouse; r loads again.

#[path = "../examples/pulls.rs"]
#[allow(dead_code)]
mod pulls;

use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::{assert_screen, Driver};
use pulls::{fixture, parse, pulls_app, Fetch, PULLS};

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

/// The app, waiting for its load before each frame.
fn driver_at(fetch: Fetch, width: u16, height: u16) -> Driver {
    let mut driver = pulls_app(fetch).wait_for_tasks(true).driver(width, height);
    driver.settle();
    driver
}

fn driver(fetch: Fetch) -> Driver {
    driver_at(fetch, WIDTH, HEIGHT)
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn has(driver: &Driver, needle: &str) -> bool {
    rows(driver).iter().any(|r| r.contains(needle))
}

/// A load that waits until the test lets it go.
fn gated() -> (Fetch, mpsc::Sender<()>) {
    let (go, wait) = mpsc::channel::<()>();
    let wait = Arc::new(Mutex::new(wait));
    let fetch: Fetch = Arc::new(move || {
        let _ = wait.lock().unwrap().recv_timeout(Duration::from_secs(30));
        Ok(PULLS.to_vec())
    });
    (fetch, go)
}

#[test]
fn pulls_loading() {
    let (fetch, go) = gated();
    // Not waiting for the load: the first frames show it under way.
    let mut driver = pulls_app(fetch).driver(WIDTH, HEIGHT);
    driver.settle();
    assert!(rows(&driver)[0].contains("Loading…"), "{:?}", rows(&driver));
    assert!(!has(&driver, "#212"));
    assert!(driver.accessibility().iter().any(|n| n.state.busy));
    assert_screen!(driver, "pulls_loading");
    go.send(()).unwrap();
    let mut now = Duration::ZERO;
    while !has(&driver, "#212") {
        now += Duration::from_millis(10);
        assert!(now < Duration::from_secs(30), "the load never came");
        std::thread::sleep(Duration::from_millis(10));
        driver.update(now);
        driver.settle();
    }
    assert!(rows(&driver)[0].contains("Loaded 12"));
}

#[test]
fn pulls_loaded() {
    let driver = driver(fixture(Duration::ZERO, false));
    assert!(
        rows(&driver)[0].contains("Loaded 12"),
        "{:?}",
        rows(&driver)
    );
    assert!(has(&driver, "Add a dark theme to the settings page"));
    assert_screen!(driver, "pulls_loaded");
}

#[test]
fn pulls_failed() {
    let mut driver = driver(fixture(Duration::ZERO, true));
    assert!(rows(&driver)[0].contains("Failed"), "{:?}", rows(&driver));
    assert!(has(&driver, "the server said 503"));
    assert_screen!(driver, "pulls_failed");
    // r tries again, and fails again.
    driver.press("r");
    assert!(rows(&driver)[0].contains("Failed"));
}

#[test]
fn j_and_k_move_and_a_click_picks() {
    let mut driver = driver(fixture(Duration::ZERO, false));
    assert!(!has(&driver, "> "));
    driver.press("j");
    assert!(has(&driver, "> #212"), "{:?}", rows(&driver));
    driver.press("j j k");
    assert!(has(&driver, "> #211"), "{:?}", rows(&driver));
    // The header, its gap, then the rows: the fifth pull request.
    let at = rows(&driver)
        .iter()
        .position(|r| r.contains("#208"))
        .unwrap() as u16;
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        driver.event(Event::Mouse(Mouse::new(kind, 10, at)));
    }
    driver.settle();
    assert!(has(&driver, "> #208"), "{:?}", rows(&driver));
}

#[test]
fn r_loads_again() {
    let calls = Arc::new(Mutex::new(0));
    let counted = calls.clone();
    let fetch: Fetch = Arc::new(move || {
        *counted.lock().unwrap() += 1;
        Ok(PULLS[..3].to_vec())
    });
    let mut driver = driver(fetch);
    assert!(rows(&driver)[0].contains("Loaded 3"));
    driver.press("j r");
    assert_eq!(*calls.lock().unwrap(), 2);
    assert!(rows(&driver)[0].contains("Loaded 3"));
    assert!(!has(&driver, "> "), "the selection goes with the old rows");
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = driver(fixture(Duration::ZERO, false));
    driver.press("?");
    assert!(has(&driver, "load them again"));
}

#[test]
fn the_command_line() {
    let parse = |line: &str| parse(line.split_whitespace().map(String::from));
    assert!(parse("--delay 0 --fail").is_ok());
    assert!(parse("--delay soon").is_err());
    assert!(parse("--from gh").is_err());
}

#[test]
fn it_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        let mut driver = driver_at(fixture(Duration::ZERO, false), width, height);
        driver.press("j j r k");
    }
}
