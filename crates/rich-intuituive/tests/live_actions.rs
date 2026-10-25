//! A live action list: the help and the palette follow the bindings while
//! they are open, a binding may be listed as unavailable with a reason
//! (`bind_unless`), the palette checks a binding again as it runs it, and
//! `action_overlay` lists actions of the app's own.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{action_overlay, input, Action, Driver, Size};

fn shown(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

/// The focused row of a palette: the one with the pointer.
fn focused_row(driver: &Driver) -> String {
    driver
        .screen()
        .plain()
        .into_iter()
        .find(|row| row.contains('❯') || row.contains('>'))
        .unwrap_or_default()
}

const SECOND: Duration = Duration::from_secs(1);

/// `Delete` bound while `on`; a timer turns it off after a second (`off`)
/// or on (`!off`). `ran` counts its runs.
fn conditional(start: bool) -> App {
    App::new(move || {
        let (on, ran) = (signal(start), signal(0));
        every(SECOND, move |_| on.update(|on| *on = !*on));
        text!("ran {ran}")
            .bind_when(
                move || on.get(),
                "x",
                "Delete",
                move |_| ran.update(|n| *n += 1),
            )
            .bind("y", "Yank", |_| {})
    })
    .palette_key("ctrl+p")
    .help_key("f1")
}

#[test]
fn the_palette_lists_a_binding_once_it_applies() {
    let mut driver = conditional(false).driver(50, 10);
    driver.press("ctrl+p");
    assert!(!shown(&driver).contains("Delete"));
    driver.advance(SECOND);
    assert!(shown(&driver).contains("Delete"), "{}", shown(&driver));
    driver.advance(SECOND);
    assert!(!shown(&driver).contains("Delete"));
}

#[test]
fn the_help_follows_the_bindings_while_open() {
    let mut driver = conditional(false).driver(50, 10);
    driver.press("f1");
    assert!(shown(&driver).contains("Yank") && !shown(&driver).contains("Delete"));
    driver.advance(SECOND);
    assert!(shown(&driver).contains("Delete"));
}

#[test]
fn a_command_that_stopped_applying_does_not_run() {
    let mut driver = conditional(true).driver(50, 10);
    driver.press("ctrl+p");
    // Searched for, so it is the one focused.
    driver.press("D e l");
    assert!(focused_row(&driver).contains("Delete"));
    driver.advance(SECOND);
    driver.press("enter");
    driver.press("esc");
    assert_eq!(driver.screen().plain()[0].trim_end(), "ran 0");
}

#[test]
fn a_command_that_applies_runs_from_the_palette() {
    let mut driver = conditional(true).driver(50, 10);
    driver.press("ctrl+p D e l enter");
    assert_eq!(driver.screen().plain()[0].trim_end(), "ran 1");
}

/// Save, unavailable until `e` edits; `n` counts the saves.
fn editor() -> App {
    App::new(|| {
        let (dirty, saved) = (signal(false), signal(0));
        every(SECOND, move |_| dirty.update(|d| *d = !*d));
        text!("saved {saved}")
            .bind_unless(
                move || (!dirty.get()).then(|| "nothing to save".to_string()),
                "ctrl+s",
                "Save",
                move |_| saved.update(|n| *n += 1),
            )
            .bind("q", "Quit", |cx| cx.quit())
    })
    .palette_key("ctrl+p")
    .help_key("f1")
}

#[test]
fn an_unavailable_binding_is_listed_with_its_reason_and_does_not_run() {
    let mut driver = editor().driver(60, 10);
    // Its key passes it by.
    driver.press("ctrl+s");
    assert_eq!(driver.screen().plain()[0].trim_end(), "saved 0");
    driver.press("f1");
    assert!(shown(&driver).contains("Save (nothing to save)"));
    driver.press("esc ctrl+p S a v e");
    assert!(focused_row(&driver).contains("Save (nothing to save)"));
    // Enter on it: listed still, and nothing runs.
    driver.press("enter");
    assert!(shown(&driver).contains("nothing to save"));
    // Available now, while the palette is open: the reason goes.
    driver.advance(SECOND);
    assert!(!shown(&driver).contains("nothing to save"));
    driver.press("enter");
    assert_eq!(driver.screen().plain()[0].trim_end(), "saved 1");
    // And its key works.
    driver.press("ctrl+s");
    assert_eq!(driver.screen().plain()[0].trim_end(), "saved 2");
}

#[test]
fn the_selection_stays_on_its_binding_as_the_list_changes() {
    let mut driver = conditional(false).driver(50, 10);
    driver.press("ctrl+p");
    // Only Yank listed; focus it, then Delete comes in before it.
    assert!(focused_row(&driver).contains("Yank"));
    driver.advance(SECOND);
    assert!(shown(&driver).contains("Delete"));
    assert!(focused_row(&driver).contains("Yank"), "{}", shown(&driver));
}

// `action_overlay`: actions of the app's own.

/// A player: its actions change label with `playing`, Next is unavailable
/// until a track is queued (a timer, each second), and Stop comes and goes
/// with it. An editor beneath takes typed keys.
fn player() -> App {
    App::new(|| {
        let (playing, queued, ran) = (signal(false), signal(false), signal(String::new()));
        let typed = signal(String::new());
        every(SECOND, move |_| queued.update(|q| *q = !*q));
        let actions = move || {
            let mut all = vec![
                Action::new("toggle", if playing.get() { "Pause" } else { "Play" }).keys("space"),
            ];
            let next = Action::new("next", "Next track");
            all.push(match queued.get() {
                true => next,
                false => next.unavailable("queue is empty"),
            });
            if queued.get() {
                all.insert(0, Action::new("stop", "Stop"));
            }
            all
        };
        column([
            input(typed).name("editor").fixed(1),
            text!("ran {ran}").fixed(1),
        ])
        .on_key("f2", move |cx| {
            cx.action_palette(actions, move |id, _| {
                if id == "toggle" {
                    playing.update(|p| *p = !*p);
                }
                ran.set(id.to_string());
            })
        })
    })
}

#[test]
fn actions_come_go_and_relabel_while_open() {
    let mut driver = player().driver(90, 14);
    driver.press("f2");
    let open = shown(&driver);
    assert!(
        open.contains("Play") && open.contains("Next track (queue is empty)"),
        "{open}"
    );
    assert!(!open.contains("Stop"));
    driver.advance(SECOND);
    let now = shown(&driver);
    assert!(
        now.contains("Stop") && !now.contains("queue is empty"),
        "{now}"
    );
}

#[test]
fn the_focus_stays_on_the_same_action() {
    let mut driver = player().driver(90, 14);
    driver.press("f2 down");
    assert!(focused_row(&driver).contains("Next track"));
    // Stop comes in first: the focus stays on Next.
    driver.advance(SECOND);
    assert!(focused_row(&driver).contains("Next track"));
    driver.press("enter");
    assert_eq!(driver.screen().plain()[1].trim_end(), "ran next");
}

#[test]
fn an_unavailable_action_does_not_run_and_the_focus_comes_back() {
    let mut driver = player().driver(90, 14);
    driver.settle();
    driver.assert_focused("editor");
    driver.press("f2 down enter");
    // Still open, nothing run.
    assert!(shown(&driver).contains("queue is empty"));
    assert_eq!(driver.screen().plain()[1].trim_end(), "ran");
    // Typed keys go to the palette's search, not the editor beneath.
    driver.press("p l");
    driver.press("enter");
    assert_eq!(driver.screen().plain()[1].trim_end(), "ran toggle");
    driver.assert_focused("editor");
    assert_eq!(driver.screen().plain()[0].trim_end(), "");
}

#[test]
fn an_action_that_went_while_chosen_does_not_run() {
    let mut driver = player().driver(90, 14);
    driver.advance(SECOND);
    driver.press("f2");
    // Stop is listed now, first.
    assert!(focused_row(&driver).contains("Stop"));
    // It goes as the queue empties: the focus moves, and Stop never runs.
    driver.advance(SECOND);
    assert!(!shown(&driver).contains("Stop"));
    driver.press("enter");
    assert_ne!(driver.screen().plain()[1].trim_end(), "ran stop");
}

#[test]
fn action_overlay_is_a_node_of_its_own() {
    let app = App::new(|| {
        let said = signal(String::new());
        text!("{said}").on_key("a", move |cx| {
            cx.modal(Size::Percent(90), Size::Percent(90), move || {
                action_overlay(
                    || vec![Action::new("one", "First"), Action::new("two", "Second")],
                    move |id, _| said.set(id.to_string()),
                )
            })
        })
    });
    let mut driver = app.driver(40, 8);
    driver.press("a down enter");
    assert_eq!(driver.screen().plain()[0].trim_end(), "two");
}
