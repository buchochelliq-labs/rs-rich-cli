//! The meters example (examples/meters.rs): widgets of your own, driven
//! like a person would.

mod common;

#[path = "../examples/meters.rs"]
#[allow(dead_code)]
mod meters;

use common::{row_of, run, run_open, screen};
use intuituive::interact::MouseKind;
use meters::meters_app;
use rich_interact::headless::Script;

/// The board at 60x12: a title row, two rows of meters, the status row.
fn board(script: Script) -> Vec<String> {
    screen(&run_open(meters_app(true), script, 60, 12))
}

#[test]
fn four_meters_draw_a_gauge_a_sparkline_and_a_status_row() {
    let rows = board(Script::new());
    for name in ["cpu", "memory", "disk", "network"] {
        assert!(row_of(&rows, name).is_some(), "{name}: {rows:?}");
    }
    // cpu, top left: a gauge with its percentage, then a sparkline.
    assert!(rows[2].contains('%'), "{rows:?}");
    assert!(rows[3].chars().any(|c| "▁▂▃▄▅▆▇█".contains(c)), "{rows:?}");
    assert!(rows[4].contains("alarm 80%"), "{rows:?}");
}

#[test]
fn arrows_move_the_focused_meters_alarm() {
    // cpu has the focus first; ← lowers its alarm by 5.
    let rows = board(Script::new().keys("left"));
    assert!(rows[4].contains("alarm 75%"), "{rows:?}");
    // The others keep theirs.
    assert!(row_of(&rows, "alarm 85%").is_some(), "{rows:?}");
}

#[test]
fn the_board_sees_keys_first_and_keeps_them_while_paused() {
    let rows = board(Script::new().keys("space left"));
    assert!(
        rows[4].contains("alarm 80%"),
        "the meter never saw ←: {rows:?}"
    );
    assert!(
        row_of(&rows, "Paused").is_some(),
        "a toast says why: {rows:?}"
    );
    assert!(rows[11].contains("paused"), "{rows:?}");
    // Resumed, ← reaches the meter again.
    let rows = board(Script::new().keys("space space left"));
    assert!(rows[4].contains("alarm 75%"), "{rows:?}");
}

#[test]
fn the_pointer_over_a_sparkline_reads_that_sample() {
    // cpu's sparkline is row 3; the newest sample is in its last column.
    let rows = board(Script::new().mouse(MouseKind::Moved, 28, 3));
    assert!(rows[4].contains("samples ago"), "{rows:?}");
    // Moving away brings the peak and alarm back.
    let script = Script::new()
        .mouse(MouseKind::Moved, 28, 3)
        .mouse(MouseKind::Moved, 28, 0);
    let rows = board(script);
    assert!(rows[4].contains("peak"), "{rows:?}");
}

#[test]
fn r_resets_the_peak_with_a_toast() {
    let rows = board(Script::new().keys("r"));
    assert!(row_of(&rows, "cpu: peak reset").is_some(), "{rows:?}");
}

#[test]
fn a_change_redraws_inside_the_border_only() {
    // The first ← comes with the focus arriving (the border turns the
    // accent colour); the second changes the alarm alone: that frame
    // writes the meter's inside rows, not its border.
    let record = run(meters_app(true), Script::new().keys("left left q"), 60, 12);
    let frame = record.writes.iter().skip(1).rev().find(|w| w.contains('█'));
    let frame = frame.unwrap_or_else(|| panic!("{:?}", &record.writes[1..]));
    assert!(!frame.contains('╭') && !frame.contains('╰'), "{frame:?}");
}

#[test]
fn a_taller_board_gives_the_sparklines_more_rows() {
    // 20 rows: a title, two rows of meters nine tall, the status row.
    let rows = screen(&run_open(meters_app(true), Script::new(), 60, 20));
    let bars = |row: &str| row.chars().filter(|c| "▁▂▃▄▅▆▇█".contains(*c)).count();
    // cpu's sparkline is rows 3 to 7; every one of them draws bars.
    for row in &rows[3..8] {
        assert!(bars(row) > 0, "{rows:?}");
    }
    assert!(rows[8].contains("peak"), "the status row last: {rows:?}");
}
