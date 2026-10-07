//! The tutorial's to-do app, driven like a person would.

mod common;

#[path = "../examples/todo.rs"]
#[allow(dead_code)]
mod todo;

use common::{row_of, run, screen};
use rich_interact::headless::Script;
use todo::{todo_app, Todo};

#[test]
fn typing_and_enter_adds_a_todo_and_the_entry_clears() {
    let script = Script::new()
        .text("milk")
        .keys("enter")
        .text("eggs")
        .keys("enter ctrl+q");
    let rows = screen(&run(todo_app(Vec::new()), script, 50, 12));
    assert_eq!(row_of(&rows, "• milk"), Some(4), "{rows:?}");
    assert_eq!(row_of(&rows, "• eggs"), Some(5), "{rows:?}");
    // The entry is empty again, ready for the next one.
    assert!(!rows[1].contains("eggs"), "{rows:?}");
    assert!(row_of(&rows, "2 left").is_some(), "{rows:?}");
}

#[test]
fn space_ticks_off_the_focused_todo() {
    let start = vec![Todo::new(1, "milk"), Todo::new(2, "eggs")];
    // Tab from the entry to the first row, then to the second.
    let script = Script::new().keys("tab tab space ctrl+q");
    let rows = screen(&run(todo_app(start), script, 50, 12));
    assert!(rows[4].contains("• milk"), "{rows:?}");
    assert!(rows[5].contains("✓ eggs"), "{rows:?}");
    assert!(row_of(&rows, "1 left").is_some(), "{rows:?}");
}

#[test]
fn d_asks_before_deleting() {
    let start = vec![Todo::new(1, "milk"), Todo::new(2, "eggs")];
    let rows = screen(&run(
        todo_app(start.clone()),
        Script::new().keys("tab d n ctrl+q"),
        50,
        12,
    ));
    assert!(row_of(&rows, "milk").is_some(), "n keeps it: {rows:?}");

    let rows = screen(&run(
        todo_app(start),
        Script::new().keys("tab d y ctrl+q"),
        50,
        12,
    ));
    assert!(row_of(&rows, "milk").is_none(), "y deletes it: {rows:?}");
    assert_eq!(row_of(&rows, "• eggs"), Some(4), "{rows:?}");
}

#[test]
fn the_focused_row_is_highlighted() {
    let start = vec![Todo::new(1, "milk")];
    let record = run(todo_app(start), Script::new().keys("tab ctrl+q"), 50, 12);
    // Reverse video (7) on the row, filled to the panel's inner width.
    let out = record.output();
    assert!(out.contains(";7m"), "{out:?}");
}

#[test]
fn ticking_a_todo_draws_only_its_row_and_the_footer() {
    let start = vec![
        Todo::new(1, "milk"),
        Todo::new(2, "eggs"),
        Todo::new(3, "tea"),
    ];
    let app = todo_app(start).inspector(true);
    let rows = screen(&run(app, Script::new().keys("tab space ctrl+q"), 100, 14));
    let text = rows.join("\n");
    assert!(text.contains("drew 2 of"), "{text}");
}
