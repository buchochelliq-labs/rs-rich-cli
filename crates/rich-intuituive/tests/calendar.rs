//! The port of ratatui's `calendar-explorer` (examples/calendar.rs), on
//! 2026-10-17: the year in three of its looks as 88x26 snapshots (four
//! months of 22 cells across, three down, a line above and below), the
//! keys and the mouse, and a move that redraws only the months it touches.
//!
//! The snapshots are `calendar_app_*`: `calendar_year` is taken by
//! `tests/calendar_options.rs`.

#[path = "../examples/calendar.rs"]
#[allow(dead_code)]
mod calendar;

use calendar::{calendar_app, date_at, day_name, season, Options, LOOKS};
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::widgets::Date;
use intuituive::{assert_screen, assert_screen_styled, Driver};

const WIDTH: u16 = 88;
const HEIGHT: u16 = 26;
const TODAY: Date = Date::new(2026, 10, 17);

fn options() -> Options {
    Options {
        today: TODAY,
        linear: false,
    }
}

fn driver_at(width: u16, height: u16) -> Driver {
    let mut driver = calendar_app(options()).driver(width, height);
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

fn footer(driver: &Driver) -> String {
    rows(driver)[HEIGHT as usize - 1].clone()
}

#[test]
fn calendar_year() {
    let driver = driver();
    let rows = rows(&driver);
    assert!(rows[0].contains("2026") && rows[0].contains("look: plain"));
    assert!(rows[1].contains("January 2026") && rows[1].contains("April 2026"));
    assert!(rows[17].contains("September 2026") && rows[17].contains("December 2026"));
    assert_eq!(footer(&driver), " Sat 17 October 2026 · autumn · today");
    assert_screen_styled!(driver, "calendar_app_year");
}

#[test]
fn calendar_year_surrounding() {
    let mut driver = driver();
    driver.press("s");
    assert!(rows(&driver)[0].contains("look: surrounding days"));
    // January's first week starts on the 29th of December.
    assert!(
        rows(&driver)[3].starts_with("29 30 31  1  2  3  4"),
        "{:?}",
        rows(&driver)
    );
    assert_screen!(driver, "calendar_app_year_surrounding");
}

#[test]
fn calendar_year_headers() {
    let mut driver = driver();
    driver.press("s s s");
    assert!(rows(&driver)[0].contains("look: month header"));
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[bold reverse blue]"), "{styled}");
    assert_screen_styled!(driver, "calendar_app_year_headers");
    // On round all six, and back to plain.
    driver.press("s s s");
    assert!(rows(&driver)[0].contains(&format!("look: {}", LOOKS[0])));
}

#[test]
fn the_keys_move_the_selection() {
    let mut driver = driver();
    driver.press("l");
    assert!(footer(&driver).starts_with(" Sun 18 October"));
    driver.press("j j");
    assert!(footer(&driver).starts_with(" Sun 1 November"));
    driver.press("k h");
    assert!(footer(&driver).starts_with(" Sat 24 October"));
    driver.press("n");
    assert!(footer(&driver).starts_with(" Tue 24 November"));
    driver.press("p p p");
    assert!(footer(&driver).starts_with(" Mon 24 August"));
    driver.press("t");
    assert!(footer(&driver).contains("today"));
    // Into the next year: the title and the months follow.
    driver.press("n n n");
    assert!(rows(&driver)[0].contains("2027"));
    assert!(rows(&driver)[1].contains("January 2027"));
    // The selected day is reversed.
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[reverse]17[/]"), "{styled}");
}

#[test]
fn a_click_selects_a_day_and_the_wheel_turns_the_month() {
    let mut driver = driver();
    // March is the third month of the first row: columns 44 to 63. The
    // 1st of March 2026 is a Sunday, so the 2nd is under Mo in row 4.
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        driver.event(Event::Mouse(Mouse::new(kind, 45, 4)));
    }
    driver.settle();
    assert!(
        footer(&driver).starts_with(" Mon 2 March 2026"),
        "{}",
        footer(&driver)
    );
    driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 45, 4)));
    driver.settle();
    assert!(footer(&driver).starts_with(" Thu 2 April"));
}

#[test]
fn moving_a_day_draws_the_month_and_the_lines_only() {
    let mut driver = driver();
    driver.press("l");
    // October and the footer; not the other eleven months.
    let drawn = driver.stats().drawn;
    assert!(drawn <= 3, "{drawn} nodes drew");
    // Into November: October and November.
    driver.press("j j");
    assert!(driver.stats().drawn <= 4);
}

#[test]
fn the_days_and_the_seasons() {
    assert_eq!(day_name(Date::new(2026, 4, 22)), Some("Earth Day"));
    assert_eq!(day_name(Date::new(2026, 4, 23)), None);
    assert_eq!(season(Date::new(2026, 3, 19)).0, "winter");
    assert_eq!(season(Date::new(2026, 3, 20)).0, "spring");
    assert_eq!(season(Date::new(2026, 9, 23)).0, "autumn");
    assert_eq!(season(Date::new(2026, 12, 21)).0, "winter");
    let march = Date::new(2026, 3, 1);
    assert_eq!(date_at(march, 18, 2), Some(march));
    assert_eq!(date_at(march, 0, 2), None);
    assert_eq!(date_at(march, 2, 3), None, "a gap between days");
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = driver();
    driver.press("?");
    assert!(rows(&driver).iter().any(|r| r.contains("the next look")));
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    assert_eq!(parse("--today 2026-10-17").unwrap().today, TODAY);
    assert!(parse("--today soon").is_err());
    assert!(parse("--today").is_err());
}

#[test]
fn every_look_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        let mut driver = driver_at(width, height);
        for _ in 0..LOOKS.len() {
            driver.press("s l j n");
        }
    }
}
