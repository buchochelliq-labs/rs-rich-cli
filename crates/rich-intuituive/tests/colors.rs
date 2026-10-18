//! The port of ratatui's `color-explorer`, `colors-rgb` and `modifiers`
//! (examples/colors.rs): each tab at 100x32 as snapshots, the keys and the
//! mouse, the swatch on the virtual clock, and a frame that redraws only
//! the swatch.

#[path = "../examples/colors.rs"]
#[allow(dead_code)]
mod colors;

use std::time::Duration;

use colors::{colors_app, Options, FRAME};
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::{assert_screen, assert_screen_styled, Driver};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 32;

fn driver_at(options: Options, width: u16, height: u16) -> Driver {
    let mut driver = colors_app(options).driver(width, height);
    driver.settle();
    driver
}

fn driver(options: Options) -> Driver {
    driver_at(options, WIDTH, HEIGHT)
}

fn frozen() -> Options {
    Options {
        frozen: true,
        ..Options::default()
    }
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

#[test]
fn colors_named() {
    let driver = driver(frozen());
    assert!(has(&driver, "bright_magenta"));
    assert_screen_styled!(driver, "colors_named");
}

#[test]
fn colors_indexed() {
    let mut driver = driver(frozen());
    driver.press("2");
    let rows = rows(&driver);
    assert!(rows.iter().any(|r| r.contains("  0   1   2")), "{rows:?}");
    assert!(rows.iter().any(|r| r.contains(" 231")), "{rows:?}");
    assert!(rows.iter().any(|r| r.contains(" 255")), "{rows:?}");
    assert_screen!(driver, "colors_indexed");
}

#[test]
fn colors_rgb() {
    let mut driver = driver(frozen());
    driver.press("3");
    assert!(has(&driver, "stopped (--frozen)"));
    assert!(has(&driver, "▀▀▀▀"));
    assert_screen!(driver, "colors_rgb");
}

#[test]
fn colors_modifiers() {
    let mut driver = driver(frozen());
    driver.press("4");
    assert!(has(&driver, "Crossed out"));
    assert_screen!(driver, "colors_modifiers");
    // Each modifier is the style of its row's cells.
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[strike red] Crossed out [/]"), "{styled}");
    assert!(styled.contains("[italic blue]   Italic    [/]"), "{styled}");
}

#[test]
fn colors_palettes() {
    let mut driver = driver(frozen());
    driver.press("5");
    assert!(has(&driver, "fuchsia"));
    assert!(has(&driver, "deep orange"));
    assert_screen!(driver, "colors_palettes");
}

#[test]
fn the_swatch_turns_and_counts_its_frames() {
    let mut driver = driver(Options::default());
    driver.press("3");
    let first = intuituive::testing::styled(&driver);
    // A second of frames on the virtual clock: the hue has turned, and the
    // read-out counts the frames drawn.
    let mut now = Duration::ZERO;
    while now < Duration::from_millis(1010) {
        now += FRAME;
        driver.update(now);
        driver.settle();
    }
    assert_ne!(intuituive::testing::styled(&driver), first, "it turned");
    let rows = rows(&driver);
    let readout = rows
        .iter()
        .find(|r| r.contains("frames a second"))
        .expect("the read-out");
    let count: u64 = readout
        .split_whitespace()
        .find_map(|w| w.trim_matches('│').parse().ok())
        .expect("a number");
    assert!((55..=62).contains(&count), "{readout}");
}

#[test]
fn a_frame_redraws_only_the_swatch() {
    let mut driver = driver(Options::default());
    driver.press("3");
    driver.update(FRAME);
    driver.render().expect("a frame draws");
    assert_eq!(driver.stats().drawn, 1, "only the swatch drew");
    // On another tab, the clock moves and nothing draws.
    driver.press("1");
    driver.update(FRAME * 2);
    driver.render();
    let stats = driver.stats();
    assert_eq!(
        (stats.drawn, stats.bytes),
        (0, 0),
        "nothing drew on the Named tab"
    );
}

#[test]
fn keys_and_clicks_change_tabs() {
    let mut driver = driver(frozen());
    driver.press("right");
    assert!(has(&driver, "Indexed colours"));
    driver.press("l l");
    assert!(has(&driver, "─ Modifiers"));
    driver.press("h");
    assert!(has(&driver, "─ RGB"));
    driver.press("left left left");
    assert!(has(&driver, "─ Palettes"));
    // " Named │ Indexed │ RGB │ ...": RGB is near column 22.
    let at = rows(&driver)[1]
        .chars()
        .collect::<String>()
        .split("RGB")
        .next()
        .unwrap()
        .chars()
        .count() as u16;
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        driver.event(Event::Mouse(Mouse::new(kind, at + 1, 1)));
    }
    driver.settle();
    assert!(has(&driver, "─ RGB"));
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = driver(frozen());
    driver.press("?");
    assert!(has(&driver, "the Palettes tab"));
    driver.press("esc ctrl+p");
    assert!(has(&driver, "the next tab"));
}

#[test]
fn linear_mode_names_the_blocks() {
    let options = Options {
        linear: true,
        frozen: true,
    };
    let mut driver = colors_app(options).driver(WIDTH, HEIGHT);
    driver.settle();
    driver.press("2");
    let names: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|n| n.name.clone())
        .collect();
    assert!(
        names.iter().any(|n| n.contains("the colour cube")),
        "{names:?}"
    );
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    assert!(parse("--frozen").unwrap().frozen);
    assert!(parse("--linear").unwrap().linear);
    assert!(parse("--fast").is_err());
}

#[test]
fn every_tab_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        let mut driver = driver_at(Options::default(), width, height);
        for key in ["2", "3", "4", "5", "1"] {
            driver.press(key);
            driver.update(FRAME * 3);
            driver.settle();
        }
    }
}
