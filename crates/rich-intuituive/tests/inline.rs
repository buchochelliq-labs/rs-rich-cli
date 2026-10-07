//! Inline apps: a region below the cursor instead of the alternate screen.

mod common;

use common::{run, screen};
use intuituive::prelude::*;
use rich_interact::headless::Script;

fn counter() -> App {
    App::new(|| {
        let count = signal(0);
        column([text!("count {count}").fixed(1), label("[dim]+ adds")])
            .on_key("+", move |_| count.update(|c| *c += 1))
            .on_key("q", |cx| cx.quit())
    })
    .inline(3)
}

#[test]
fn an_inline_app_draws_in_its_region_with_relative_moves() {
    let record = run(counter(), Script::new().keys("+ q"), 30, 24);
    assert_eq!(screen(&record), ["count 1", "+ adds", ""]);
    let out = record.output();
    // The region is made by scrolling room below the cursor, then cleared;
    // nothing is placed absolutely and the screen is never wiped.
    assert!(out.contains("\r\n\n\x1b[2A\x1b[J"), "{out:?}");
    assert!(!out.contains("\x1b[2J"), "{out:?}");
    assert!(!out.contains('H'), "no absolute cursor moves: {out:?}");
    // The update moves within the region: up from where the cursor was
    // left (the region's last row), to the changed cell.
    let update = record
        .writes
        .iter()
        .find(|w| w.ends_with('1') && w.contains("\x1b[0m"))
        .expect("the update");
    assert_eq!(update, "\x1b[2A\r\x1b[6C\x1b[0m1", "{update:?}");
}

#[test]
fn an_inline_app_leaves_the_cursor_below_its_last_frame() {
    let record = run(counter(), Script::new().keys("q"), 30, 24);
    let last = record.writes.last().expect("the finish");
    // To the region's last row, then a new line under it.
    assert!(last.ends_with("\x1b[0m\r\n\x1b[0m\x1b[?25h"), "{last:?}");
}

#[test]
fn an_inline_region_fits_a_short_terminal() {
    let record = run(counter(), Script::new().keys("q"), 30, 2);
    assert_eq!(screen(&record), ["count 0", "+ adds"]);
}
