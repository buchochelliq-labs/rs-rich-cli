//! The notice bar: folded to one row, opened, its actions, and dismissed.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::prelude::*;
use intuituive::{Driver, Level, notice_bar};

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

type Signals = (Signal<bool>, Signal<bool>, Signal<u32>);

/// The bar over a count of its first action's runs, and the signals:
/// shown, open and the count.
fn app(closes: bool, width: u16, height: u16) -> (Driver, Signals) {
    let out: Rc<Cell<Option<Signals>>> = Rc::default();
    let set = out.clone();
    let app = App::new(move || {
        let (shown, open, runs) = (signal(true), signal(false), signal(0u32));
        set.set(Some((shown, open, runs)));
        column([
            notice_bar(
                shown,
                || "Update".into(),
                || "Version 2 is ready to install; it restarts the app".into(),
            )
            .notice_level(Level::Success)
            .notice_open(open)
            .notice_action("Install", move |_| runs.update(|n| *n += 1))
            .notice_action("Later", |_| {})
            .notice_closes(closes),
            text!("runs {runs}"),
            label("below").focusable(),
        ])
    });
    let mut driver = app.driver(width, height);
    driver.settle();
    (driver, out.get().expect("built"))
}

#[test]
fn folded_it_is_one_row_cut_with_an_ellipsis() {
    let (driver, _) = app(false, 48, 5);
    let rows = rows(&driver);
    assert_eq!(rows[0], "▸ ✓ Update Version 2 is rea…  Install  Later  ×");
    assert_eq!(rows[1], "runs 0");
    assert_eq!(
        driver.accessibility()[0].describe(),
        "Update: Version 2 is ready to install; it restarts the app, collapsed"
    );
}

#[test]
fn enter_or_a_click_opens_it_and_the_app_can_open_it() {
    let (mut driver, (_, open, _)) = app(false, 30, 6);
    driver.press("enter");
    assert!(open.get_untracked());
    let rows = rows(&driver);
    assert!(rows[0].starts_with("▾ ✓ Update"), "{rows:?}");
    assert_eq!(rows[1], "  Version 2 is ready to");
    assert_eq!(rows[2], "  install; it restarts the app");
    assert_eq!(rows[3], "runs 0");
    // A click on the text folds it again.
    assert!(driver.click_text("install;"));
    assert!(!open.get_untracked());
    assert_eq!(self::rows(&driver)[1], "runs 0");
    open.set(true);
    driver.settle();
    assert_eq!(self::rows(&driver)[1], "  Version 2 is ready to");
}

#[test]
fn actions_run_from_the_keys_or_a_click_and_may_close_it() {
    let (mut driver, (shown, _, runs)) = app(false, 48, 5);
    driver.press("right enter");
    assert_eq!(runs.get_untracked(), 1);
    assert!(driver.click_text("Install"));
    assert_eq!(runs.get_untracked(), 2);
    assert!(shown.get_untracked());
    // With notice_closes, an action dismisses it.
    let (mut driver, (shown, _, runs)) = app(true, 48, 5);
    assert!(driver.click_text("Install"));
    assert_eq!(runs.get_untracked(), 1);
    assert!(!shown.get_untracked());
    assert_eq!(rows(&driver)[0], "runs 1");
}

#[test]
fn the_close_mark_or_escape_dismisses_it() {
    let (mut driver, (shown, _, _)) = app(false, 48, 5);
    assert!(driver.click_text("×"));
    assert!(!shown.get_untracked());
    assert_eq!(rows(&driver)[0], "runs 0");
    shown.set(true);
    driver.settle();
    driver.press("shift+tab");
    assert!(driver.focused_name().unwrap().starts_with("Update"));
    driver.press("escape");
    assert!(!shown.get_untracked());
    assert_eq!(rows(&driver)[0], "runs 0");
}
