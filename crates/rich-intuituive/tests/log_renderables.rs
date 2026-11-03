//! Renderables in a `Log` (#802): laid out at the view's width and again
//! when it changes, filtered by their text, followed when they arrive.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::log::{LogLine, Severity};
use intuituive::prelude::*;

/// A log, kept where the test can write to it after the app is built.
fn app(build: impl Fn(Log) + 'static) -> (App, Rc<Cell<Option<Log>>>) {
    let out = Rc::new(Cell::new(None));
    let seen = out.clone();
    let app = App::new(move || {
        let log = Log::new(100);
        build(log);
        seen.set(Some(log));
        log.view().focusable()
    });
    (app, out)
}

fn panel(text: &str) -> rich::Panel {
    rich::Panel::new(Box::new(rich::Text::new(text.to_string())))
}

fn table() -> rich::Table {
    let mut table = rich::Table::new();
    table.add_column("file");
    table.add_column("size");
    table.add_row(&["Cargo.toml", "1 KB"]);
    table.add_row(&["README.md", "4 KB"]);
    table
}

#[test]
fn a_renderable_is_drawn_at_the_views_width_and_again_when_it_changes() {
    let (app, _) = app(|log| log.write(panel("tool output")));
    let mut driver = app.driver(40, 5);
    driver.settle();
    let rows = driver.screen().plain();
    assert_eq!(rows[0].chars().count(), 40);
    assert!(
        rows[0].starts_with('╭') && rows[0].trim_end().ends_with('╮'),
        "{rows:?}"
    );
    assert_eq!(rows[0].trim_end().chars().count(), 40);
    driver.resize(60, 5);
    driver.settle();
    let rows = driver.screen().plain();
    assert_eq!(rows[0].trim_end().chars().count(), 60, "{rows:?}");
    assert!(rows[1].contains("tool output"));
}

#[test]
fn a_filter_matches_a_word_inside_a_written_table() {
    let (app, log) = app(|log| {
        log.push("one line");
        log.write(table());
        log.push("another line");
    });
    let mut driver = app.driver(40, 8);
    driver.settle();
    let log = log.take().unwrap();
    log.set_filter("README").unwrap();
    driver.settle();
    let rows = driver.screen().plain();
    assert!(rows.iter().any(|r| r.contains("README.md")), "{rows:?}");
    assert!(!rows.iter().any(|r| r.contains("line")), "{rows:?}");
    assert_eq!(log.shown(), 1);
}

#[test]
fn following_holds_when_a_tall_entry_arrives() {
    let (app, log) = app(|log| {
        for i in 0..20 {
            log.push(format!("line {i}"));
        }
    });
    let mut driver = app.driver(30, 6);
    driver.settle();
    let log = log.take().unwrap();
    assert_eq!(driver.screen().plain()[5].trim_end(), "line 19");
    log.write(table());
    driver.settle();
    let rows = driver.screen().plain();
    // The table's last row at the bottom, the lines above it pushed up.
    assert!(rows[5].starts_with('└'), "{rows:?}");
    assert!(
        rows[0].contains("line 19") || rows[0].contains('┏'),
        "{rows:?}"
    );
    assert!(log.is_following());
    log.push("after");
    driver.settle();
    assert_eq!(driver.screen().plain()[5].trim_end(), "after");
}

#[test]
fn scrolling_moves_by_entries_and_end_follows_again() {
    let (app, log) = app(|log| {
        log.write(panel("first"));
        log.write(panel("second"));
        log.write(panel("third"));
    });
    let mut driver = app.driver(20, 3);
    driver.settle();
    let log = log.take().unwrap();
    assert!(driver.screen().plain()[1].contains("third"));
    driver.press("up");
    assert!(
        driver.screen().plain()[1].contains("second"),
        "{:?}",
        driver.screen().plain()
    );
    assert!(!log.is_following());
    driver.press("home");
    assert!(driver.screen().plain()[1].contains("first"));
    driver.press("end");
    assert!(driver.screen().plain()[1].contains("third"));
    assert!(log.is_following());
}

#[test]
fn a_written_entry_has_a_level_and_a_target_and_counts_as_one_line() {
    let (app, log) = app(|log| {
        log.set_capacity(2);
        log.push("dropped");
        log.write_with(
            panel("out"),
            LogLine::new("").level(Severity::Info).target("tool"),
        );
        log.push("last");
    });
    let mut driver = app.driver(20, 6);
    driver.settle();
    let log = log.take().unwrap();
    assert_eq!(log.len(), 2);
    let rows = driver.screen().plain();
    assert_eq!(rows[0].trim_end(), "INFO  tool");
    assert!(rows[2].contains("out"), "{rows:?}");
    assert_eq!(rows[4].trim_end(), "last");
}
