//! Toasts with buttons, dismissed by a click or a key, the Success level,
//! and the notification history.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::prelude::*;
use intuituive::{Driver, Level, Notification, Notified, notifications};

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

/// An app whose `n` shows a notification with two buttons, `t` a plain
/// toast, `s` a success and `x` dismisses them; the count of each
/// button's runs, and the history.
fn app() -> (
    Driver,
    Rc<Cell<Option<(Signal<u32>, Signal<u32>, Signal<Vec<Notified>>)>>>,
) {
    let out = Rc::new(Cell::new(None));
    let set = out.clone();
    let app = App::new(move || {
        let (undo, view) = (signal(0u32), signal(0u32));
        set.set(Some((undo, view, notifications())));
        column([text!("undo {undo} view {view}")])
            .on_key("n", move |cx| {
                cx.notify_with(
                    Notification::new(Level::Warn, "Trash", "3 files moved")
                        .action("Undo", move |_| undo.update(|n| *n += 1))
                        .action("View", move |_| view.update(|n| *n += 1)),
                )
            })
            .on_key("t", |cx| cx.toast("Copied"))
            .on_key("s", |cx| cx.notify(Level::Success, "Saved", "notes.txt"))
            .on_key("x", |cx| cx.dismiss_toasts())
    });
    let mut driver = app.driver(40, 10);
    driver.settle();
    (driver, out)
}

#[test]
fn a_notification_shows_its_buttons_under_its_body() {
    let (mut driver, _) = app();
    driver.press("n");
    let rows = rows(&driver);
    assert!(rows[6].ends_with("╭─ Trash ───────╮"), "{rows:#?}");
    assert!(rows[7].ends_with("│ 3 files moved │"), "{rows:#?}");
    assert!(rows[8].ends_with("│  Undo   View  │"), "{rows:#?}");
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[white on grey30] Undo [/]"), "{styled}");
}

#[test]
fn a_button_runs_its_action_and_dismisses_the_notification() {
    let (mut driver, out) = app();
    let (undo, view, _) = out.get().unwrap();
    driver.press("n");
    assert!(driver.click_text("View"));
    assert_eq!((undo.get_untracked(), view.get_untracked()), (0, 1));
    assert!(driver.find_text("Trash").is_none());
    assert_eq!(rows(&driver)[0], "undo 0 view 1");
    // A second one; its other button.
    driver.press("n");
    assert!(driver.click_text("Undo"));
    assert_eq!((undo.get_untracked(), view.get_untracked()), (1, 1));
}

#[test]
fn a_click_elsewhere_on_a_toast_dismisses_it_and_nothing_runs() {
    let (mut driver, out) = app();
    let (undo, view, _) = out.get().unwrap();
    driver.press("n t");
    assert!(driver.click_text("3 files"));
    assert!(driver.find_text("Trash").is_none());
    assert!(driver.find_text("Copied").is_some());
    assert_eq!((undo.get_untracked(), view.get_untracked()), (0, 0));
    assert!(driver.click_text("Copied"));
    assert!(driver.find_text("Copied").is_none());
}

#[test]
fn a_key_dismisses_them_all() {
    let (mut driver, _) = app();
    driver.press("n t s");
    assert!(driver.find_text("Saved").is_some());
    driver.press("x");
    for gone in ["Trash", "Copied", "Saved"] {
        assert!(driver.find_text(gone).is_none(), "{gone}");
    }
}

#[test]
fn success_is_a_level_in_the_good_style() {
    let (mut driver, _) = app();
    driver.press("s");
    let styled = intuituive::testing::styled(&driver);
    assert!(
        styled.contains("[green]╭─[/][bold green] Saved [/]"),
        "{styled}"
    );
    assert!(
        driver
            .take_announcements()
            .iter()
            .any(|a| a.text == "Saved: notes.txt")
    );
}

#[test]
fn the_history_keeps_what_was_shown_after_it_goes() {
    let (mut driver, out) = app();
    let (_, _, history) = out.get().unwrap();
    driver.press("n t s x");
    let seen = history.get_untracked();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].level, Some(Level::Warn));
    assert_eq!(seen[0].title, "Trash");
    assert_eq!(seen[1].level, None);
    assert_eq!(seen[1].body, "Copied");
    assert_eq!(seen[2].level, Some(Level::Success));
    // At most 100 are kept.
    for _ in 0..120 {
        driver.press("t");
    }
    assert_eq!(history.get_untracked().len(), 100);
}
