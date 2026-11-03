//! The start page: Noctiluca started with no address, on intuiTUIve's
//! driver over the scripted engine. Animations move here
//! (`a11y::set_motion`); `start_still.rs` checks the still pose.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::app::{Options, app};
use noctiluca::bookmarks::{Bookmark, Bookmarks};
use noctiluca::config::Config;
use noctiluca::testing::FakeEngine;
use rich_intuituive::Driver;
use rich_intuituive::interact::{Button, Event, Mouse, MouseKind};

fn engine() -> FakeEngine {
    let dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    FakeEngine::from_dir(&dir).unwrap()
}

fn marks() -> Bookmarks {
    Bookmarks {
        path: None,
        items: vec![
            Bookmark {
                url: "https://fixture.example/reader.html".into(),
                title: "Plankton".into(),
                keyword: None,
            },
            Bookmark {
                url: "https://fixture.example/form.html".into(),
                title: "A form".into(),
                keyword: None,
            },
        ],
    }
}

fn start(
    width: u16,
    height: u16,
) -> (
    Driver,
    std::sync::Arc<std::sync::Mutex<noctiluca::testing::FakeState>>,
) {
    // Every test in this file moves, whatever the machine's variables say.
    rich_intuituive::a11y::set_motion(Some(true));
    let engine = engine();
    let state = engine.state.clone();
    let options = Options {
        config: Config::default(),
        bookmarks: marks(),
        url: None,
        view: None,
        images: Some(noctiluca::config::Images::Ansi),
        tunnel: None,
        control: None,
    };
    let mut driver = app(Box::new(engine), options).driver(width, height);
    driver.settle();
    (driver, state)
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

fn wait_for(driver: &mut Driver, text: &str) {
    let start = Instant::now();
    loop {
        driver.settle();
        if screen(driver).contains(text) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{text:?} never showed:\n{}",
            screen(driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Columns of row `y` that hold a half block of the fish.
fn fish_cells(driver: &Driver, y: u16) -> Vec<u16> {
    let row = &driver.screen().plain()[y as usize];
    row.chars()
        .enumerate()
        .filter(|(_, c)| matches!(c, '▀' | '▄' | '█'))
        .map(|(x, _)| x as u16)
        .collect()
}

/// Rows holding any of the fish.
fn fish_rows(driver: &Driver) -> Vec<u16> {
    (0..driver.screen().plain().len() as u16)
        .filter(|&y| !fish_cells(driver, y).is_empty())
        .collect()
}

#[test]
fn the_start_page_shows_the_banner_the_hints_and_the_bookmarks() {
    let (mut driver, _) = start(120, 40);
    let text = screen(&driver);
    assert!(text.contains("a web browser for the terminal"), "{text}");
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
    // The banner's letters: NOCTILUCA's N.
    assert!(text.contains(" _   _  ___"), "{text}");
    // The hints come from the keymap (the vim-like preset).
    assert!(text.contains("o open an address"), "{text}");
    assert!(text.contains("q quit"), "{text}");
    assert!(
        text.contains("Plankton  https://fixture.example/reader.html"),
        "{text}"
    );
    // Choosing a bookmark opens it and leaves the start page.
    driver.press("down enter");
    wait_for(&mut driver, "Name");
    let text = screen(&driver);
    assert!(!text.contains("a web browser for the terminal"), "{text}");
}

#[test]
fn the_fish_swims_in_and_settles_beside_the_title() {
    let (mut driver, _) = start(120, 40);
    // At the start it is off to the right.
    assert!(fish_rows(&driver).is_empty(), "{}", screen(&driver));
    driver.advance(Duration::from_millis(700));
    let swimming = fish_rows(&driver);
    assert!(!swimming.is_empty(), "{}", screen(&driver));
    let mid_x = swimming
        .iter()
        .flat_map(|&y| fish_cells(&driver, y))
        .min()
        .unwrap();
    driver.advance(Duration::from_millis(1500));
    let rows = fish_rows(&driver);
    let still_x = rows
        .iter()
        .flat_map(|&y| fish_cells(&driver, y))
        .min()
        .unwrap();
    assert!(still_x < mid_x, "{mid_x} -> {still_x}");
    // Beside the banner: its rows are the banner's rows too.
    let first = rows[0] as usize;
    let banner = driver
        .screen()
        .plain()
        .iter()
        .position(|l| l.contains("a web browser for the terminal"))
        .unwrap();
    assert!(first <= banner, "{}", screen(&driver));
    // Still: no more frames.
    let frames = driver.stats().frames;
    driver.advance(Duration::from_secs(1));
    assert_eq!(driver.stats().frames, frames);
}

#[test]
fn any_key_ends_the_animation_at_once() {
    let (mut driver, _) = start(120, 40);
    assert!(fish_rows(&driver).is_empty());
    driver.press("j");
    // The fish is where it settles, without waiting.
    assert!(!fish_rows(&driver).is_empty(), "{}", screen(&driver));
    let frames = driver.stats().frames;
    driver.advance(Duration::from_secs(1));
    assert_eq!(driver.stats().frames, frames);
}

#[test]
fn the_fish_opens_about_from_the_keyboard() {
    let (mut driver, _) = start(120, 40);
    driver.press("tab");
    driver.assert_focused("About Noctiluca");
    driver.press("enter");
    wait_for(&mut driver, "Codecs");
    // A box over the start page: Esc comes back to it.
    driver.press("esc");
    wait_for(&mut driver, "a web browser for the terminal");
    assert!(!screen(&driver).contains("Codecs"));
}

#[test]
fn the_fish_opens_about_on_a_click() {
    let (mut driver, _) = start(120, 40);
    driver.press("x");
    let rows = fish_rows(&driver);
    let y = rows[rows.len() / 2];
    let x = fish_cells(&driver, y)[0];
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        x,
        y,
    )));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), x, y)));
    wait_for(&mut driver, "Codecs");
}

#[test]
fn a_narrow_terminal_leaves_the_fish_out_but_keeps_the_title_and_list() {
    let (mut driver, _) = start(80, 24);
    driver.advance(Duration::from_secs(3));
    let text = screen(&driver);
    assert!(fish_rows(&driver).is_empty(), "{text}");
    assert!(text.contains(" _   _  ___"), "{text}");
    assert!(text.contains("Plankton"), "{text}");
    // Tall enough, it goes above the title.
    let (mut driver, _) = start(80, 50);
    driver.advance(Duration::from_secs(3));
    let rows = fish_rows(&driver);
    assert!(!rows.is_empty(), "{}", screen(&driver));
    let banner = driver
        .screen()
        .plain()
        .iter()
        .position(|l| l.contains(" _   _  ___"))
        .unwrap();
    assert!(
        (*rows.last().unwrap() as usize) < banner,
        "{}",
        screen(&driver)
    );
}

#[test]
fn an_address_typed_leaves_the_start_page() {
    let (mut driver, _) = start(100, 30);
    driver.press("o");
    driver.type_text("reader.html");
    driver.press("enter");
    wait_for(&mut driver, "Tiny drifters of the open sea");
    assert!(!screen(&driver).contains("a web browser for the terminal"));
}

#[test]
fn a_url_given_skips_the_start_page() {
    let options = Options {
        config: Config::default(),
        bookmarks: marks(),
        url: Some("reader.html".into()),
        view: None,
        images: Some(noctiluca::config::Images::Ansi),
        tunnel: None,
        control: None,
    };
    let mut driver = app(Box::new(engine()), options).driver(100, 30);
    wait_for(&mut driver, "Tiny drifters");
    assert!(!screen(&driver).contains("a web browser for the terminal"));
}
