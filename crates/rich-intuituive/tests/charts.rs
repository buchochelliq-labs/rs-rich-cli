//! The chart gallery (examples/charts.rs), the port of ratatui's chart
//! examples: each tab at 100x30 as a snapshot with the clock stopped, the
//! keys and the mouse, and a tick that redraws only what moves.

#[path = "../examples/charts.rs"]
#[allow(dead_code)]
mod charts;

use std::time::Duration;

use charts::{chart_window, charts_app, gauge_ratios, Options, FROZEN_TICKS, MARKERS, TABS};
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::{assert_screen, assert_screen_styled, Driver};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;

fn driver_at(options: Options, width: u16, height: u16) -> Driver {
    let mut driver = charts_app(options).driver(width, height);
    driver.settle();
    driver
}

/// Tab `tab`, the clock stopped at [`FROZEN_TICKS`].
fn frozen(tab: usize) -> Driver {
    let options = Options {
        tab,
        frozen: true,
        ..Options::default()
    };
    driver_at(options, WIDTH, HEIGHT)
}

/// Move the virtual clock on `n` ticks of 200 ms, drawing after each.
fn tick(driver: &mut Driver, from: u32, n: u32) {
    for k in from + 1..=from + n {
        driver.update(Duration::from_millis(200) * k);
        driver.settle();
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

fn row_of(rows: &[String], needle: &str) -> Option<usize> {
    rows.iter().position(|r| r.contains(needle))
}

#[test]
fn charts_gauge() {
    let driver = frozen(0);
    let rows = rows(&driver);
    assert_eq!(gauge_ratios(FROZEN_TICKS)[0], 0.42);
    assert!(row_of(&rows, "42%").is_some(), "{rows:?}");
    assert!(row_of(&rows, "of 1000 MB").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Thin  :  42% ───").is_some(), "{rows:?}");
    assert!(row_of(&rows, "▰▰▰").is_some(), "{rows:?}");
    assert_screen_styled!(driver, "charts_gauge");
}

#[test]
fn charts_sparkline() {
    let driver = frozen(1);
    let rows = rows(&driver);
    // The gaps are marked, and the Braille line is there.
    assert!(row_of(&rows, "×").is_some(), "{rows:?}");
    let braille = |c: char| ('\u{2801}'..='\u{28ff}').contains(&c);
    assert!(rows.iter().any(|r| r.chars().any(braille)), "{rows:?}");
    assert_screen!(driver, "charts_sparkline");
}

#[test]
fn charts_bars() {
    let driver = frozen(2);
    let rows = rows(&driver);
    assert!(row_of(&rows, "26°").is_some(), "{rows:?}");
    assert!(row_of(&rows, "00  01  02").is_some(), "{rows:?}");
    assert!(row_of(&rows, "23 commits").is_some(), "{rows:?}");
    assert_screen_styled!(driver, "charts_bars");
}

#[test]
fn charts_groups() {
    let driver = frozen(3);
    let rows = rows(&driver);
    assert!(row_of(&rows, "Q1           Q2").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Fern ").is_some(), "{rows:?}");
    assert!(row_of(&rows, "10.6M").is_some(), "{rows:?}");
    assert_screen!(driver, "charts_groups");
}

#[test]
fn charts_chart() {
    let driver = frozen(4);
    let rows = rows(&driver);
    let (low, high) = chart_window(FROZEN_TICKS);
    assert!(row_of(&rows, &format!("{low:.1}")).is_some(), "{rows:?}");
    assert!(row_of(&rows, &format!("{high:.1}")).is_some(), "{rows:?}");
    // The legends: top right, top left and bottom right.
    assert!(row_of(&rows, "│sin│").is_some(), "{rows:?}");
    assert!(row_of(&rows, "│plan│").is_some(), "{rows:?}");
    assert!(row_of(&rows, "│braille│").is_some(), "{rows:?}");
    assert_screen!(driver, "charts_chart");
}

#[test]
fn charts_canvas() {
    let mut driver = frozen(5);
    let rows = rows(&driver);
    assert!(row_of(&rows, "You are here").is_some(), "{rows:?}");
    assert!(row_of(&rows, "a rectangle").is_some(), "{rows:?}");
    assert_screen!(driver, "charts_canvas");
    // m goes through every marker, and round.
    for (name, _) in MARKERS.iter().skip(1) {
        driver.press("m");
        let rows = self::rows(&driver);
        assert!(rows[1].contains(&format!("Marker: {name}")), "{rows:?}");
    }
    driver.press("m");
    assert!(self::rows(&driver)[1].contains("Marker: Braille"));
}

#[test]
fn the_moving_window_moves_with_the_clock() {
    let options = Options {
        tab: 4,
        ..Options::default()
    };
    let mut driver = driver_at(options, WIDTH, HEIGHT);
    tick(&mut driver, 0, 8);
    let (low, _) = chart_window(8);
    assert!(row_of(&rows(&driver), &format!("{low:.1} ")).is_some());
    // Space stops the clock: the window stays.
    driver.press("space");
    let before = rows(&driver);
    tick(&mut driver, 8, 5);
    assert_eq!(rows(&driver), before);
    driver.press("space");
    tick(&mut driver, 13, 1);
    assert_ne!(rows(&driver), before);
}

#[test]
fn a_tick_redraws_only_what_moves() {
    let mut driver = charts_app(Options::default())
        .inspector(true)
        .driver(WIDTH, HEIGHT);
    driver.settle();
    tick(&mut driver, 0, 2);
    let draws = |driver: &Driver, name: &str| {
        driver
            .inspector_timings()
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.draws)
            .unwrap_or_else(|| panic!("no node called {name}"))
    };
    let before: Vec<u64> = ["tabs", "footer", "gauge-1", "line-gauges"]
        .iter()
        .map(|name| draws(&driver, name))
        .collect();
    driver.update(Duration::from_millis(600));
    let frame = driver.render().expect("a tick draws");
    let stats = driver.stats();
    // The four gauges and the line gauges.
    assert_eq!(stats.drawn, 5, "{:?}", driver.inspector_timings());
    assert_eq!(
        draws(&driver, "tabs"),
        before[0],
        "the tab strip did not draw"
    );
    assert_eq!(
        draws(&driver, "footer"),
        before[1],
        "the footer did not draw"
    );
    assert_eq!(draws(&driver, "gauge-1"), before[2] + 1, "the gauge drew");
    assert_eq!(draws(&driver, "line-gauges"), before[3] + 1);
    assert!(!frame.contains("Percentage"), "the panels were not sent");
    // On a tab that does not move, a tick draws no node (the inspector's
    // own panel still counts its frames).
    driver.press("3");
    driver.update(Duration::from_millis(800));
    let _ = driver.render();
    assert_eq!(driver.stats().drawn, 0, "the bars do not move");
}

#[test]
fn frozen_stops_the_clock() {
    let mut driver = frozen(0);
    let first = rows(&driver);
    driver.update(Duration::from_secs(10));
    assert!(driver.render().is_none(), "nothing moves");
    assert_eq!(rows(&driver), first);
}

#[test]
fn the_keys_and_clicks_change_tabs() {
    let mut driver = frozen(0);
    driver.press("right");
    assert!(row_of(&rows(&driver), "─ Left to right ─").is_some());
    driver.press("h h");
    assert!(row_of(&rows(&driver), "Marker: Braille").is_some());
    driver.press("5");
    assert!(row_of(&rows(&driver), "─ A moving window ─").is_some());
    // " Gauge │ Sparkline │ Bars": a click on Bars.
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        driver.event(Event::Mouse(Mouse::new(kind, 22, 0)));
        driver.settle();
    }
    assert!(row_of(&rows(&driver), "Weather").is_some());
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = frozen(0);
    driver.press("?");
    let rows = rows(&driver);
    assert!(
        row_of(&rows, "stop or start the clock").is_some(),
        "{rows:?}"
    );
    assert!(
        row_of(&rows, "the canvas's next marker").is_some(),
        "{rows:?}"
    );
}

#[test]
fn linear_mode_reads_the_charts_by_their_labels() {
    for (tab, label) in [
        (0, "Percentage, a gauge"),
        (1, "Random values in Braille, two to a cell"),
        (4, "A sine and a cosine in a moving window, a chart"),
        (5, "A map of the world"),
    ] {
        let options = Options {
            tab,
            linear: true,
            frozen: true,
            ..Options::default()
        };
        let driver = driver_at(options, WIDTH, HEIGHT);
        let names: Vec<String> = driver
            .accessibility()
            .iter()
            .map(|node| node.name.clone())
            .collect();
        assert!(names.iter().any(|n| n == label), "{label}: {names:?}");
    }
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    let options = parse("--tick-rate 50 --frozen --tab 5").unwrap();
    assert_eq!(options.tick_rate, Duration::from_millis(50));
    assert!(options.frozen);
    assert_eq!(options.tab, 4);
    assert!(parse("--tab=7").is_err());
    assert!(parse("--tick-rate fast").is_err());
    assert!(parse("--unicode").is_err());
}

#[test]
fn every_tab_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        for tab in 0..TABS.len() {
            let options = Options {
                tab,
                ..Options::default()
            };
            let mut driver = driver_at(options, width, height);
            tick(&mut driver, 0, 3);
            driver.press("m");
        }
    }
}
