//! The port of ratatui's `demo` (examples/demo.rs): each tab at 100x32
//! as snapshots, the keys and the mouse, the clock, and a tick that
//! redraws only what moves.

#[path = "../examples/demo.rs"]
#[allow(dead_code)]
mod demo;

use std::time::Duration;

use demo::{demo_app, Options, FROZEN_TICKS, TASKS};
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::{assert_screen, assert_screen_styled, Driver};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 32;

fn driver_at(options: Options, width: u16, height: u16) -> Driver {
    let mut driver = demo_app(options).driver(width, height);
    driver.settle();
    driver
}

fn driver(options: Options) -> Driver {
    driver_at(options, WIDTH, HEIGHT)
}

/// Move the virtual clock on `n` ticks of 250 ms, drawing after each.
fn tick(driver: &mut Driver, from: u32, n: u32) {
    for k in from + 1..=from + n {
        driver.update(Duration::from_millis(250) * k);
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

fn click(driver: &mut Driver, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        column,
        row,
    )));
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Up(Button::Left),
        column,
        row,
    )));
    driver.settle();
}

#[test]
fn demo_graphs() {
    let mut driver = driver(Options::default());
    tick(&mut driver, 0, 10);
    let rows = rows(&driver);
    // Ten ticks: the gauge is at a hundredth, the chart's window 10 to 30.
    assert!(row_of(&rows, "1.00%").is_some(), "{rows:?}");
    assert!(row_of(&rows, "  10 ").is_some(), "{rows:?}");
    assert_screen!(driver, "demo_graphs");
}

#[test]
fn demo_graphs_no_chart() {
    let mut driver = driver(Options::default());
    tick(&mut driver, 0, 10);
    driver.press("t");
    let rows = rows(&driver);
    assert_eq!(row_of(&rows, "Chart"), None, "{rows:?}");
    // The lists and the bars take the width.
    assert!(rows[13].ends_with('╮'), "{rows:?}");
    assert_screen!(driver, "demo_graphs_no_chart");
    // And back.
    driver.press("t");
    assert!(row_of(&self::rows(&driver), "Chart").is_some());
}

#[test]
fn demo_world() {
    let mut driver = driver(Options::default());
    driver.press("right");
    let rows = rows(&driver);
    assert!(
        row_of(&rows, "Europe-1        Lisbon").is_some(),
        "{rows:?}"
    );
    assert_screen!(driver, "demo_world");
}

#[test]
fn demo_colours() {
    let mut driver = driver(Options::default());
    driver.press("right right");
    assert_screen_styled!(driver, "demo_colours");
}

#[test]
fn demo_plain() {
    let unicode = Options {
        unicode: false,
        ..Options::default()
    };
    let mut driver = driver(unicode);
    tick(&mut driver, 0, 10);
    let rows = rows(&driver);
    // Dots, not Braille, and a thin line gauge.
    let braille = |c: char| ('\u{2801}'..='\u{28ff}').contains(&c);
    assert!(!rows.iter().any(|r| r.chars().any(braille)), "{rows:?}");
    assert!(row_of(&rows, "1% ───").is_some(), "{rows:?}");
    assert_screen!(driver, "demo_plain");
}

#[test]
fn a_tick_redraws_only_what_moves() {
    let mut driver = demo_app(Options::default())
        .inspector(true)
        .driver(140, HEIGHT);
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
    let before: Vec<u64> = ["footer", "tasks", "tabs", "gauge", "chart"]
        .iter()
        .map(|name| draws(&driver, name))
        .collect();
    driver.update(Duration::from_millis(750));
    let frame = driver.render().expect("a tick draws");
    let stats = driver.stats();
    // The two gauges, the sparkline, the log, the bars and the chart.
    assert!(
        (6..=8).contains(&stats.drawn),
        "{} nodes drew: {:?}",
        stats.drawn,
        driver.inspector_timings()
    );
    assert_eq!(
        draws(&driver, "footer"),
        before[0],
        "the footer did not draw"
    );
    assert_eq!(draws(&driver, "tasks"), before[1], "the tasks did not draw");
    assert_eq!(
        draws(&driver, "tabs"),
        before[2],
        "the tab strip did not draw"
    );
    assert_eq!(draws(&driver, "gauge"), before[3] + 1, "the gauge drew");
    assert_eq!(draws(&driver, "chart"), before[4] + 1, "the chart drew");
    // Nothing of theirs was sent.
    for unchanged in ["rainbow", "Rotate the API keys", "Colours"] {
        assert!(
            !frame.contains(unchanged),
            "{unchanged} was sent: {frame:?}"
        );
    }
}

#[test]
fn down_and_up_move_the_task_selection_round() {
    let mut driver = driver(Options::default());
    // Nothing is selected at first; ↓ selects the first task.
    assert_eq!(row_of(&rows(&driver), "> "), None);
    driver.press("down");
    assert!(rows(&driver)[14].contains(&format!("> {}", TASKS[0])));
    driver.press("j");
    assert!(rows(&driver)[15].contains("> Renew the"));
    // ↑ from the first goes round to the last.
    driver.press("up k");
    let rows = rows(&driver);
    // The list scrolls to it: four rows show, the last of them selected.
    assert!(rows[17].contains("> Plan the next drill"), "{rows:?}");
}

#[test]
fn left_and_right_go_round_the_tabs() {
    let mut driver = driver(Options::default());
    driver.press("left");
    assert!(row_of(&rows(&driver), "╭─ Colours").is_some());
    driver.press("l l");
    assert!(row_of(&rows(&driver), "╭─ Servers").is_some());
    driver.press("h");
    assert!(row_of(&rows(&driver), "╭─ Graphs").is_some());
}

#[test]
fn the_mouse_picks_tabs_tasks_and_servers() {
    let mut driver = driver(Options::default());
    // " Graphs │ World │ Colours": World is at columns 11 to 17.
    click(&mut driver, 13, 1);
    assert!(row_of(&rows(&driver), "╭─ Servers").is_some());
    // The third server's row: the header, its gap, then the rows.
    click(&mut driver, 5, 8);
    let screen = driver.screen();
    let style = screen.style(screen.cell(2, 8).style).cloned();
    let definition = style.map(|s| s.definition()).unwrap_or_default();
    assert!(
        definition.contains("reverse"),
        "the clicked server is highlighted: {definition}"
    );
    // Back to Graphs, and a click on the third task.
    click(&mut driver, 4, 1);
    click(&mut driver, 5, 16);
    assert!(rows(&driver)[16].contains("> Patch the build hosts"));
    // The wheel moves the selection on, three tasks a turn.
    driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 5, 16)));
    driver.settle();
    assert!(rows(&driver)[17].contains("> Review the alert"));
}

#[test]
fn frozen_moves_the_data_on_and_stops_the_clock() {
    let frozen = Options {
        frozen: true,
        ..Options::default()
    };
    let mut driver = driver(frozen);
    let first = rows(&driver);
    let percent = format!("{:.2}%", FROZEN_TICKS as f64 / 10.0);
    assert!(row_of(&first, &percent).is_some(), "{first:?}");
    driver.update(Duration::from_secs(10));
    assert!(driver.render().is_none(), "nothing moves");
    assert_eq!(rows(&driver), first);
}

#[test]
fn the_help_and_the_palette_list_the_keys() {
    let mut driver = driver(Options::default());
    driver.press("?");
    let rows = rows(&driver);
    let at = row_of(&rows, "show or hide the chart").expect("the help lists t");
    assert!(rows[at].contains('t'), "{rows:?}");
    driver.press("esc ctrl+p");
    assert!(row_of(&self::rows(&driver), "the next tab").is_some());
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    let options = parse("--tick-rate 100 --unicode false --frozen").unwrap();
    assert_eq!(options.tick_rate, Duration::from_millis(100));
    assert!(!options.unicode && options.frozen);
    assert!(parse("--unicode=true").unwrap().unicode);
    assert!(parse("--tick-rate").is_err());
    assert!(parse("--unicode maybe").is_err());
    assert!(parse("--colour").is_err());
}

#[test]
fn linear_mode_reads_the_charts_by_their_labels() {
    let linear = Options {
        linear: true,
        ..Options::default()
    };
    let mut driver = demo_app(linear).driver(WIDTH, HEIGHT);
    driver.settle();
    let names: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|node| node.name.clone())
        .collect();
    for label in [
        "Tasks",
        "Logs",
        "Two sine waves, a chart",
        "Random values, a sparkline",
    ] {
        assert!(names.iter().any(|n| n == label), "{label}: {names:?}");
    }
}

#[test]
fn every_tab_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5)] {
        for unicode in [true, false] {
            let options = Options {
                unicode,
                ..Options::default()
            };
            let mut driver = driver_at(options, width, height);
            tick(&mut driver, 0, 2);
            driver.press("t t right right right");
        }
    }
}
