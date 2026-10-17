//! Snapshot assertions and the driver's test helpers: the guide's
//! Testing section, run. Snapshots are in `tests/snapshots`; set
//! `UPDATE_SNAPSHOTS=1` to write them again after a deliberate change, then
//! review the diff.

#[path = "../examples/todo.rs"]
#[allow(dead_code)]
mod todo;

use intuituive::prelude::*;
use intuituive::{assert_screen, assert_screen_styled};
use todo::{todo_app, Todo};

// --8<-- [start:component]
/// A component: a node and the state behind it, built by a function.
fn counter() -> Node {
    let count = signal(0);
    text!("[b]Count:[/] {count}")
        .name("counter")
        .focusable()
        .on_key("+", move |_| count.update(|c| *c += 1))
        .on_key("-", move |_| count.update(|c| *c -= 1))
}

#[test]
fn the_counter_counts() {
    let mut driver = App::new(counter).driver(20, 1);
    driver.press("+ + + -");
    driver.assert_focused("counter");
    assert_screen!(driver, "counter_two");
}
// --8<-- [end:component]

// --8<-- [start:app]
#[test]
fn a_new_todo_goes_to_the_end_of_the_list() {
    let mut driver = todo_app(vec![Todo::new(1, "milk")]).driver(40, 9);
    driver.type_text("eggs");
    driver.press("enter");
    assert_screen!(driver, "todo_added");
    // Tab leaves the entry for the list; space ticks the row off.
    driver.press("tab");
    driver.assert_focused("• milk");
    driver.press("space");
    assert_screen_styled!(driver, "todo_ticked");
    driver.press("ctrl+q");
    assert!(driver.is_done());
}
// --8<-- [end:app]

#[test]
fn render_with_rows_snapshot_too() {
    let rows = App::new(|| counter().on_key("q", |cx| cx.quit()))
        .render_with(&["+", "q"], 20, 1)
        .unwrap();
    assert_screen!(rows, "counter_one");
}

#[test]
fn styled_rows_are_markup() {
    let mut driver =
        App::new(|| label("[bold red]Error[/] in [u]3[/] fields  ").focusable()).driver(24, 2);
    driver.settle();
    assert_eq!(
        intuituive::testing::styled(&driver),
        "[bold red]Error[/] in [underline]3[/] fields\n\n"
    );
    assert_eq!(intuituive::testing::plain(&driver), "Error in 3 fields\n\n");
}

#[test]
fn a_mismatch_fails_with_a_diff() {
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        // Updating, every snapshot is written, not compared.
        return;
    }
    let mut driver = App::new(counter).driver(20, 1);
    driver.press("+");
    let dir = std::env::temp_dir().join(format!("intuituive-snapshots-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("counter.txt");
    std::fs::write(&path, "Count: 5\n").unwrap();
    let error = intuituive::testing::check_snapshot(&path, &intuituive::testing::plain(&driver))
        .unwrap_err();
    assert!(error.contains("-Count: 5\n+Count: 1\n"), "{error}");
    assert!(error.contains("UPDATE_SNAPSHOTS=1"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn focus_assertions_name_where_the_focus_is() {
    let app = App::new(|| {
        column([
            label("Save").focusable(),
            label("Quit").name("quit").focusable(),
        ])
    });
    let mut driver = app.driver(10, 2);
    driver.settle();
    // By its accessible name (the text it shows), or by its name.
    driver.assert_focused("Save");
    assert_eq!(driver.focused_name().as_deref(), Some("Save"));
    driver.press("tab");
    driver.assert_focused("quit");
    assert_eq!(driver.focused_name().as_deref(), Some("quit"));
    let id = driver.focused().expect("something has the focus");
    let message = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        driver.assert_focused("Save")
    }))
    .unwrap_err();
    let message = message.downcast_ref::<String>().unwrap();
    assert_eq!(
        message,
        "expected the focus on `Save`, but it is on `quit` (Quit)"
    );
    assert_eq!(driver.focused(), Some(id));
}

#[test]
fn nothing_focused_says_so() {
    let mut driver = App::new(|| label("plain")).driver(10, 1);
    driver.settle();
    assert_eq!(driver.focused(), None);
    let message = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        driver.assert_focused("plain")
    }))
    .unwrap_err();
    assert_eq!(
        message.downcast_ref::<String>().unwrap(),
        "expected the focus on `plain`, but nothing has the focus"
    );
}

#[test]
fn keys_after_quitting_are_dropped() {
    let app = App::new(|| {
        let count = signal(0);
        text!("{count}")
            .on_key("+", move |_| count.update(|c| *c += 1))
            .on_key("q", |cx| cx.quit())
    });
    let mut driver = app.driver(5, 1);
    driver.press("+ q +");
    assert!(driver.is_done());
    driver.type_text("more");
    assert_eq!(driver.screen().plain()[0].trim_end(), "1");
}
