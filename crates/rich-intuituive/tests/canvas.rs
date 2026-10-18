//! The port of ratatui's `canvas` and `mouse-drawing` (examples/canvas.rs):
//! the map, each marker, a drawing made with the mouse and the ball after a
//! number of frames, as 80x24 snapshots, and a frame of pong that redraws
//! the pong panel alone.

#[path = "../examples/canvas.rs"]
#[allow(dead_code)]
mod canvas;

use std::time::Duration;

use canvas::{canvas_app, Ball, Options, FRAME, MARKERS};
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::{assert_screen, Driver};

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

fn frozen() -> Options {
    Options {
        frozen: true,
        ..Options::default()
    }
}

fn driver_at(options: Options, width: u16, height: u16) -> Driver {
    let mut driver = canvas_app(options).driver(width, height);
    driver.settle();
    driver
}

fn driver(options: Options) -> Driver {
    driver_at(options, WIDTH, HEIGHT)
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

fn mouse(driver: &mut Driver, kind: MouseKind, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(kind, column, row)));
}

/// The column of `needle` in `row`, in cells.
fn column_of(row: &str, needle: &str) -> Option<usize> {
    row.find(needle).map(|at| row[..at].chars().count())
}

#[test]
fn canvas_map() {
    let mut driver = driver(frozen());
    let start = rows(&driver);
    let (row, col) = start
        .iter()
        .enumerate()
        .find_map(|(i, r)| column_of(r, "You are here").map(|c| (i, c)))
        .expect("you are on the map");
    // Two steps east and one north move you right and up.
    driver.press("l l k");
    let moved = rows(&driver);
    let (row2, col2) = moved
        .iter()
        .enumerate()
        .find_map(|(i, r)| column_of(r, "You are here").map(|c| (i, c)))
        .expect("you are still on the map");
    assert!(col2 > col && row2 <= row, "{start:?}\n{moved:?}");
    assert_screen!(driver, "canvas_map");
}

#[test]
fn canvas_markers() {
    let mut driver = driver(frozen());
    for (i, (_, name)) in MARKERS.iter().enumerate() {
        if i > 0 {
            driver.press("enter");
        }
        assert!(has(&driver, &format!("Marker: {name}")), "{name}");
        let kind = name.replace(' ', "_").to_lowercase();
        let snapshot = format!("canvas_marker_{kind}");
        assert_screen!(driver, &snapshot);
    }
    // Round to the first again.
    driver.press("enter");
    assert!(has(&driver, "Marker: Braille"));
}

#[test]
fn canvas_drawn() {
    let mut driver = driver(frozen());
    // The pad's inside starts at column 1, row 13 (below a 12-row top
    // half and the pad's border).
    assert!(rows(&driver)[12].starts_with("╭─ Draw"));
    mouse(&mut driver, MouseKind::Down(Button::Left), 4, 14);
    for x in 5..=30 {
        mouse(&mut driver, MouseKind::Drag(Button::Left), x, 14);
    }
    for y in 15..=20 {
        mouse(&mut driver, MouseKind::Drag(Button::Left), 30, y);
    }
    mouse(&mut driver, MouseKind::Up(Button::Left), 30, 20);
    driver.settle();
    // A second stroke in the next colour.
    driver.press("space");
    mouse(&mut driver, MouseKind::Down(Button::Left), 6, 18);
    mouse(&mut driver, MouseKind::Drag(Button::Left), 12, 18);
    mouse(&mut driver, MouseKind::Up(Button::Left), 12, 18);
    driver.settle();
    let rows = rows(&driver);
    let line: String = rows[14].chars().skip(5).take(20).collect();
    assert!(line.chars().all(|c| c != ' '), "the stroke: {line:?}");
    assert!(has(&driver, "■ cyan"));
    let styled = intuituive::testing::styled(&driver);
    let pad_row = styled.lines().nth(18).unwrap();
    assert!(pad_row.contains("[cyan]"), "{pad_row}");
    assert_screen!(driver, "canvas_drawn");
    // c clears it.
    driver.press("c");
    let blank: String = self::rows(&driver)[14].chars().skip(5).take(20).collect();
    assert_eq!(blank.trim(), "");
}

#[test]
fn canvas_pong() {
    let mut driver = driver(Options::default());
    let first = rows(&driver);
    let mut now = Duration::ZERO;
    for _ in 0..90 {
        now += FRAME;
        driver.update(now);
        driver.settle();
    }
    assert_ne!(rows(&driver), first, "the ball moved");
    assert_screen!(driver, "canvas_pong");
    // The model: after 90 frames the ball is where the steps put it.
    let mut ball = Ball::default();
    for _ in 0..90 {
        ball.step();
    }
    assert!(ball.x > 4.0 && ball.x < 96.0 && ball.y > 4.0 && ball.y < 46.0);
}

#[test]
fn a_frame_of_pong_redraws_the_pong_panel_alone() {
    let mut driver = driver(Options::default());
    driver.update(FRAME);
    driver.render().expect("a frame draws");
    assert_eq!(driver.stats().drawn, 1, "only the ball's canvas drew");
    // A key that moves you draws the map, not pong or the boxes.
    driver.press("h");
    let stats = driver.stats();
    assert!(stats.drawn <= 2, "{stats:?}");
}

#[test]
fn a_stroke_draws_the_pad_alone() {
    let mut driver = driver(frozen());
    mouse(&mut driver, MouseKind::Down(Button::Left), 4, 14);
    driver.render().expect("the press draws");
    assert_eq!(driver.stats().drawn, 1);
    mouse(&mut driver, MouseKind::Drag(Button::Left), 9, 15);
    driver.render().expect("the drag draws");
    assert_eq!(driver.stats().drawn, 1);
}

#[test]
fn the_ball_stays_in_its_court() {
    let mut ball = Ball::default();
    for _ in 0..10_000 {
        ball.step();
        assert!((0.0..=100.0).contains(&ball.x) && (0.0..=50.0).contains(&ball.y));
    }
}

#[test]
fn frozen_stops_the_clock() {
    let mut driver = driver(frozen());
    driver.update(Duration::from_secs(5));
    assert!(driver.render().is_none(), "nothing moves");
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = driver(frozen());
    driver.press("?");
    assert!(has(&driver, "the next marker"));
    driver.press("esc ctrl+p");
    assert!(has(&driver, "the next pen colour"));
}

#[test]
fn linear_mode_names_the_canvases() {
    let options = Options {
        linear: true,
        frozen: true,
    };
    let mut driver = canvas_app(options).driver(WIDTH, HEIGHT);
    driver.settle();
    let names: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|n| n.name.clone())
        .collect();
    for label in [
        "A map of the world, with you on it",
        "A ball bouncing round a box",
        "A pad to draw on with the mouse",
    ] {
        assert!(names.iter().any(|n| n == label), "{label}: {names:?}");
    }
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    assert!(parse("--frozen").unwrap().frozen);
    assert!(parse("--wide").is_err());
}

#[test]
fn every_marker_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        let mut driver = driver_at(Options::default(), width, height);
        for _ in 0..MARKERS.len() {
            driver.press("enter h j");
            mouse(
                &mut driver,
                MouseKind::Down(Button::Left),
                2,
                height.saturating_sub(2),
            );
            mouse(
                &mut driver,
                MouseKind::Drag(Button::Left),
                3,
                height.saturating_sub(2),
            );
            driver.update(FRAME * 2);
            driver.settle();
        }
    }
}
