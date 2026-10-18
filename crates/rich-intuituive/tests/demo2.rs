//! The port of ratatui's `demo2` (examples/demo2.rs): each tab at 81x18,
//! the size of ratatui's recording, as snapshots; destroy on the virtual
//! clock; the narrow layout; the keys, the mouse, the stylesheet, and a
//! key that redraws only what it changes.

#[path = "../examples/demo2.rs"]
#[allow(dead_code)]
mod demo2;

use std::time::Duration;

use demo2::{demo2_app, Options, EMAILS, HOPS, INGREDIENTS, SHEET};
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::widgets::Date;
use intuituive::{assert_screen, assert_screen_styled, Driver, Stylesheet};

const WIDTH: u16 = 81;
const HEIGHT: u16 = 18;

fn options() -> Options {
    Options {
        today: Date::new(2026, 10, 18),
        ..Options::default()
    }
}

fn driver_at(width: u16, height: u16) -> Driver {
    let mut driver = demo2_app(options()).driver(width, height);
    driver.settle();
    driver
}

fn driver() -> Driver {
    driver_at(WIDTH, HEIGHT)
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn row_of(rows: &[String], needle: &str) -> Option<usize> {
    rows.iter().position(|r| r.contains(needle))
}

fn press_at(driver: &mut Driver, kind: MouseKind, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(kind, column, row)));
    driver.settle();
}

fn click(driver: &mut Driver, column: u16, row: u16) {
    press_at(driver, MouseKind::Down(Button::Left), column, row);
    press_at(driver, MouseKind::Up(Button::Left), column, row);
}

/// The style of the cell at `x`, `y`, as a definition.
fn style_at(driver: &Driver, x: u16, y: u16) -> String {
    let screen = driver.screen();
    screen
        .style(screen.cell(x, y).style)
        .map(|s| s.definition())
        .unwrap_or_default()
}

#[test]
fn demo2_about() {
    let driver = driver();
    let rows = rows(&driver);
    assert!(rows[0].starts_with("intuiTUIve"), "{rows:?}");
    assert!(row_of(&rows, "─ intuiTUIve ─").is_some(), "{rows:?}");
    assert_screen!(driver, "demo2_about");
}

#[test]
fn the_mark_is_drawn_in_half_blocks_and_j_recolours_its_prompt() {
    let mut driver = driver();
    // The mark is 26 by 7 cells, four in, in the middle of the 16 rows
    // under the title bar: its frame from row 5, its prompt's first
    // stroke on row 7, the lower half of the cell at column 7.
    assert_eq!(style_at(&driver, 5, 5), "#c8c8c8 on #c8c8c8");
    assert_eq!(style_at(&driver, 7, 7), "#080808 on #40c060");
    driver.press("j");
    assert_eq!(style_at(&driver, 7, 7), "#080808 on #c0c060");
    driver.press("k k");
    assert_eq!(
        style_at(&driver, 7, 7),
        "#080808 on #40c060",
        "k stops at the first"
    );
}

#[test]
fn demo2_recipe() {
    let mut driver = driver();
    driver.press("l");
    let rows = rows(&driver);
    assert!(
        row_of(&rows, "Tomato and lentil soup").is_some(),
        "{rows:?}"
    );
    assert!(row_of(&rows, "Qty     Ingredient").is_some(), "{rows:?}");
    assert_screen_styled!(driver, "demo2_recipe");
}

#[test]
fn demo2_email() {
    let mut driver = driver();
    driver.press("l l");
    assert!(row_of(&rows(&driver), ">>Priya").is_some());
    assert_screen!(driver, "demo2_email");
}

#[test]
fn demo2_traceroute() {
    let mut driver = driver();
    driver.press("l l l");
    let rows = rows(&driver);
    assert!(
        row_of(&rows, "Traceroute cdn.ath.example.net").is_some(),
        "{rows:?}"
    );
    assert!(row_of(&rows, "━ Ping ━").is_some(), "{rows:?}");
    assert_screen!(driver, "demo2_traceroute");
}

#[test]
fn demo2_weather() {
    let mut driver = driver();
    driver.press("l l l l j j");
    let rows = rows(&driver);
    assert!(row_of(&rows, "October 2026").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Downloading: 6%").is_some(), "{rows:?}");
    assert!(row_of(&rows, "77°").is_some(), "{rows:?}");
    // Today is bold red.
    let y = row_of(&rows, "12 13 14 15 16 17 18").expect("the week of the 18th");
    let x = rows[y].chars().position(|c| c == '8').expect("the 18th") as u16;
    let style = style_at(&driver, x, y as u16);
    assert!(style.contains("bold") && style.contains("red"), "{style}");
    assert_screen!(driver, "demo2_weather");
}

#[test]
fn demo2_destroy() {
    let mut driver = driver();
    driver.press("l l");
    driver.press("d");
    // Two seconds, a frame every 20 ms, as the app's loop would.
    for ms in (20..=2000).step_by(20) {
        driver.update(Duration::from_millis(ms));
        driver.render();
    }
    driver.settle();
    let rows = rows(&driver);
    // The cells have dripped, and the wordmark has begun to fade in.
    assert!(row_of(&rows, ">>Priya <priya@example.com> Lunch on Friday?").is_none());
    let blocks = demo2::wordmark().iter().flatten().filter(|on| **on).count();
    let shown = rows
        .iter()
        .flat_map(|r| r.chars())
        .filter(|c| *c == '█')
        .count();
    assert!(shown >= blocks, "{rows:?}");
    assert_screen!(driver, "demo2_destroy");
    // Seeded: the same two seconds come out the same.
    let mut again = self::driver();
    again.press("l l d");
    again.update(Duration::from_millis(2000));
    again.settle();
    assert_eq!(self::rows(&again), rows);
    // And d brings the app back.
    driver.press("d");
    assert!(row_of(&self::rows(&driver), ">>Priya <priya@example.com>").is_some());
}

#[test]
fn demo2_narrow() {
    let mut driver = driver_at(58, HEIGHT);
    driver.press("l l l");
    let rows = rows(&driver);
    // No margins: the hops start at the left edge, the swatch does not
    // show, and there is no map.
    assert!(rows[3].starts_with("  Host"), "{rows:?}");
    assert!(!rows.iter().any(|r| r.contains('▀')), "{rows:?}");
    // Without the map the hops take the width.
    assert!(
        rows[3].find("Address").is_some_and(|at| at > 40),
        "{rows:?}"
    );
    assert_screen!(driver, "demo2_narrow");
}

#[test]
fn the_keys_move_between_tabs_and_inside_them() {
    let mut driver = driver();
    // h at the first tab stays; Tab moves on, as l does.
    driver.press("h tab");
    assert!(row_of(&rows(&driver), "Tomato and lentil soup").is_some());
    driver.press("right");
    assert!(row_of(&rows(&driver), ">>Priya").is_some());
    // j and k go round the emails.
    driver.press("j");
    assert!(row_of(&rows(&driver), "From: Sam").is_some());
    driver.press("k k");
    assert!(row_of(&rows(&driver), "From: Tom").is_some());
    // l at the last tab stays.
    driver.press("l l l l");
    assert!(row_of(&rows(&driver), "October 2026").is_some());
    assert_eq!(EMAILS.len(), 5);
}

#[test]
fn the_mouse_picks_tabs_emails_ingredients_and_hops() {
    let mut driver = driver();
    // "intuiTUIve" then the strip at column 38: " Recipe " " Email " …
    click(&mut driver, 48, 0);
    assert!(row_of(&rows(&driver), ">>Priya").is_some());
    // The third email: the folders are on row 2, the emails below.
    click(&mut driver, 10, 5);
    assert!(row_of(&rows(&driver), "From: Lena").is_some());
    // The wheel: three emails on.
    press_at(&mut driver, MouseKind::ScrollDown, 10, 5);
    assert!(row_of(&rows(&driver), "From: Tom").is_some());
    // The Recipe tab, and its second ingredient, under the header.
    click(&mut driver, 41, 0);
    click(&mut driver, 52, 6);
    assert!(
        style_at(&driver, 52, 6).contains("#c0c060"),
        "{}",
        style_at(&driver, 52, 6)
    );
    // The Traceroute tab, and a hop: the map draws its path.
    click(&mut driver, 60, 0);
    let before = rows(&driver);
    click(&mut driver, 8, 6);
    assert!(
        style_at(&driver, 8, 6).contains("#c0c060"),
        "{}",
        style_at(&driver, 8, 6)
    );
    assert_ne!(rows(&driver), before, "the ping and the map follow");
    assert_eq!(INGREDIENTS.len(), 12);
    assert_eq!(HOPS.len(), 16);
}

#[test]
fn a_key_redraws_only_what_it_changes() {
    let mut driver = demo2_app(options()).inspector(true).driver(140, HEIGHT);
    driver.settle();
    driver.press("l l");
    let draws = |driver: &Driver, name: &str| {
        driver
            .inspector_timings()
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.draws)
            .unwrap_or_else(|| panic!("no node called {name}"))
    };
    let names = ["app-title", "tabs", "keys", "folders", "inbox", "headers"];
    let before: Vec<u64> = names.iter().map(|n| draws(&driver, n)).collect();
    driver.event(Event::Key(intuituive::interact::Key::char('j')));
    driver.update(Duration::ZERO);
    let frame = driver.render().expect("j draws");
    assert!(
        driver.stats().drawn <= 4,
        "{} nodes drew",
        driver.stats().drawn
    );
    for (name, before) in names.iter().zip(before) {
        let changed = matches!(*name, "inbox" | "headers");
        assert_eq!(draws(&driver, name), before + changed as u64, "{name}");
    }
    // The swatch is not sent again.
    assert!(!frame.contains('▀'), "{frame:?}");
}

#[test]
fn the_stylesheet_colours_the_frame() {
    assert!(Stylesheet::parse(SHEET).is_ok());
    let driver = driver();
    assert!(
        style_at(&driver, 0, 0).contains("#eeeeee"),
        "{}",
        style_at(&driver, 0, 0)
    );
    assert!(style_at(&driver, 0, 0).contains("on #101830"));
    // A sheet of your own, from a file.
    let dir = std::env::temp_dir().join(format!("demo2-sheet-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("demo2.tcss");
    std::fs::write(
        &path,
        SHEET.replace(
            "#app-title { color: #eeeeee; }",
            "#app-title { color: red; }",
        ),
    )
    .unwrap();
    let mut driver = demo2_app(Options {
        sheet: Some(path),
        ..options()
    })
    .driver(WIDTH, HEIGHT);
    driver.settle();
    assert!(
        style_at(&driver, 0, 0).contains("red"),
        "{}",
        style_at(&driver, 0, 0)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    let options = parse("--today 2026-10-18 my.tcss").unwrap();
    assert_eq!(options.today, Date::new(2026, 10, 18));
    assert_eq!(options.sheet, Some("my.tcss".into()));
    assert_eq!(
        parse("--today=2027-01-02").unwrap().today,
        Date::new(2027, 1, 2)
    );
    assert!(parse("--today").is_err());
    assert!(parse("--today soon").is_err());
    assert!(parse("--colour").is_err());
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = driver();
    driver.press("?");
    assert!(row_of(&rows(&driver), "destroy, or bring back").is_some());
}

#[test]
fn every_tab_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (10, 4)] {
        let mut driver = driver_at(width, height);
        for _ in 0..5 {
            driver.press("j k j l");
        }
        driver.press("d");
        driver.update(Duration::from_secs(3));
        driver.settle();
        driver.update(Duration::from_secs(20));
        driver.settle();
    }
}
