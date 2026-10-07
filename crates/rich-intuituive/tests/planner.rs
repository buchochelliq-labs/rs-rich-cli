//! The planner example (examples/planner.rs): a menu bar, a tree, split
//! panes, a calendar and a virtual list, driven like a person would.

mod common;

#[path = "../examples/planner.rs"]
#[allow(dead_code)]
mod planner;

use common::{row_of, run_open, screen};
use planner::planner_app;
use rich_interact::headless::Script;

fn planner(script: Script) -> Vec<String> {
    screen(&run_open(planner_app(), script, 90, 24))
}

/// Where `needle` is on `row`, in cells.
fn column_of(row: &str, needle: &str) -> u16 {
    let at = row.find(needle).expect("on the row");
    row[..at].chars().count() as u16
}

#[test]
fn it_shows_the_projects_the_due_date_and_the_end_of_the_log() {
    let rows = planner(Script::new());
    assert!(rows[0].starts_with(" File  Task  Help"), "{rows:?}");
    for item in ["▾ intuiTUIve 2", "✓ Widget trait v2", "· Water the plants"] {
        assert!(row_of(&rows, item).is_some(), "{item}: {rows:?}");
    }
    assert!(row_of(&rows, "October 2026").is_some(), "{rows:?}");
    // The selected task's details, beside the calendar.
    assert!(rows[2].contains("Examples"), "{rows:?}");
    // A hundred thousand rows, and only the end of them drawn.
    assert!(row_of(&rows, "#99999").is_some(), "{rows:?}");
    assert!(rows[23].contains("5 open · 100000 log rows"), "{rows:?}");
}

#[test]
fn space_marks_the_task_done_with_a_toast_and_a_log_row() {
    let rows = planner(Script::new().keys("space"));
    assert!(rows[23].contains("4 open · 100001 log rows"), "{rows:?}");
    assert!(row_of(&rows, "✓ Examples").is_some(), "{rows:?}");
    assert!(
        row_of(&rows, "Examples done").is_some(),
        "the log: {rows:?}"
    );
}

#[test]
fn picking_a_day_on_the_calendar_moves_the_due_date() {
    let rows = planner(Script::new());
    let row = row_of(&rows, "12 13 14 15").expect("the calendar's third week");
    let x = column_of(&rows[row], "15");
    let rows = planner(Script::new().click(x, row as u16));
    assert!(
        row_of(&rows, "due date of Examples moved to 15 October").is_some(),
        "{rows:?}"
    );
}

#[test]
fn n_adds_a_task_to_the_project_and_selects_it() {
    let rows = planner(Script::new().keys("n"));
    assert!(row_of(&rows, "· New task 5").is_some(), "{rows:?}");
    assert!(rows[23].contains("6 open"), "{rows:?}");
    assert!(row_of(&rows, "Added to intuiTUIve").is_some(), "{rows:?}");
}

#[test]
fn f10_opens_the_menus_and_the_help_lists_the_bindings() {
    let rows = planner(Script::new().keys("f10 enter"));
    assert!(row_of(&rows, "New task").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Quit").is_some(), "{rows:?}");
    let rows = planner(Script::new().keys("?"));
    assert!(
        row_of(&rows, "add a task to the project").is_some(),
        "{rows:?}"
    );
}
