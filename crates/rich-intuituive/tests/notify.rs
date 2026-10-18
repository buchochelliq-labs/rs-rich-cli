//! Notifications with a level and a title, the corner toasts stack in, and
//! the confirm dialog.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{Corner, Level};

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver.screen().plain()
}

#[test]
fn a_plain_toast_still_sits_at_the_bottom_right() {
    let app = App::new(|| label("").on_key("t", |cx| cx.toast("saved")));
    let mut driver = app.driver(20, 5);
    driver.press("t");
    assert_eq!(
        rows(&driver)[2..],
        [
            "          ╭───────╮ ",
            "          │ saved │ ",
            "          ╰───────╯ "
        ]
    );
}

#[test]
fn a_notification_has_its_title_on_the_edge_and_its_body_wrapped() {
    let app = App::new(|| {
        label("").on_key("e", |cx| {
            cx.notify(
                Level::Error,
                "Copy failed",
                "permission denied: /etc/shadow is not readable by you",
            )
        })
    })
    .toasts_at(Corner::TopRight);
    let mut driver = app.driver(40, 8);
    driver.press("e");
    let shown = rows(&driver);
    assert_eq!(shown[0], " ╭─ Copy failed ──────────────────────╮ ");
    assert_eq!(shown[1], " │ permission denied: /etc/shadow is  │ ");
    assert_eq!(shown[2], " │ not readable by you                │ ");
    assert_eq!(shown[3], " ╰────────────────────────────────────╯ ");
    // In the error style, and said as an error.
    let title = driver.screen().cell(4, 0).style;
    let bad = Theme::dark()
        .styles
        .iter()
        .find(|(n, _)| n == "bad")
        .unwrap()
        .1
        .clone();
    assert_eq!(
        driver
            .screen()
            .style(title)
            .and_then(|s| s.color().cloned()),
        bad.color().cloned()
    );
    let said: Vec<String> = driver
        .take_announcements()
        .into_iter()
        .map(|a| a.text)
        .collect();
    assert!(said.contains(
        &"Error: Copy failed: permission denied: /etc/shadow is not readable by you".to_string()
    ));
}

#[test]
fn toasts_stack_from_their_corner_newest_nearest_it() {
    let app = App::new(|| {
        label("").on_key("t", |cx| {
            cx.notify(Level::Info, "One", "first");
            cx.notify(Level::Warn, "Two", "second");
        })
    })
    .toasts_at(Corner::TopLeft);
    let mut driver = app.driver(30, 8);
    driver.press("t");
    let shown = rows(&driver);
    assert!(shown[0].starts_with(" ╭─ Two"), "{shown:#?}");
    assert!(shown[3].starts_with(" ╭─ One"), "{shown:#?}");

    let app = App::new(|| {
        label("").on_key("t", |cx| {
            cx.toast("first");
            cx.toast("second");
        })
    })
    .toasts_at(Corner::BottomLeft);
    let mut driver = app.driver(30, 8);
    driver.press("t");
    let shown = rows(&driver);
    assert_eq!(shown[6].trim_end(), " │ second │");
    assert_eq!(shown[3].trim_end(), " │ first │");
}

#[test]
fn a_notification_goes_after_five_seconds() {
    let app = App::new(|| label("").on_key("n", |cx| cx.notify(Level::Info, "Hi", "there")));
    let mut driver = app.driver(30, 6);
    driver.press("n");
    assert!(rows(&driver).iter().any(|r| r.contains("Hi")));
    driver.update(Duration::from_millis(4900));
    driver.render();
    assert!(rows(&driver).iter().any(|r| r.contains("Hi")));
    driver.update(Duration::from_millis(5100));
    driver.render();
    assert!(!rows(&driver).iter().any(|r| r.contains("Hi")));
}

/// An app whose `d` asks before it trashes three files.
fn trash() -> App {
    App::new(|| {
        let gone = signal(String::from("kept"));
        text!("{gone}").on_key("d", move |cx| {
            let items = vec!["notes.txt".into(), "todo.md".into(), "[odd].rs".into()];
            cx.confirm(
                "Trash 3 selected files?",
                "They go to the trash.",
                items,
                move |_| gone.set("trashed".into()),
            )
        })
    })
}

#[test]
fn confirm_shows_the_title_the_body_the_items_and_the_answers() {
    let mut driver = trash().driver(60, 14);
    driver.press("d");
    let shown = rows(&driver);
    let top = shown
        .iter()
        .position(|r| r.contains("Trash 3 selected files?"))
        .unwrap();
    // Centred, the title in the middle of the top edge.
    assert!(
        shown[top].starts_with("     ╭─") && shown[top].ends_with("╮     "),
        "{shown:#?}"
    );
    assert!(shown[top + 1].contains("They go to the trash."));
    assert!(shown[top + 2].contains("├────"));
    assert!(shown[top + 3].contains("│ notes.txt"));
    assert!(shown[top + 5].contains("│ [odd].rs"));
    assert!(shown[top + 6].contains("├────"));
    assert!(shown[top + 7].contains("[Y]es") && shown[top + 7].contains("(N)o"));
}

#[test]
fn y_and_enter_say_yes_n_and_escape_say_no() {
    for (keys, after) in [
        ("d y", "trashed"),
        ("d enter", "trashed"),
        ("d n", "kept"),
        ("d esc", "kept"),
        ("d tab enter", "kept"),
        ("d right enter", "kept"),
    ] {
        let mut driver = trash().driver(60, 14);
        driver.press(keys);
        assert_eq!(rows(&driver)[0].trim_end(), after, "{keys}");
        assert!(
            !rows(&driver).iter().any(|r| r.contains("Trash 3")),
            "{keys}"
        );
    }
}

#[test]
fn confirm_counts_the_items_it_does_not_list() {
    let app = App::new(|| {
        label("").on_key("d", |cx| {
            let items = (0..14).map(|i| format!("file {i}")).collect();
            cx.confirm("Delete?", "", items, |_| {})
        })
    });
    let mut driver = app.driver(60, 20);
    driver.press("d");
    let shown = rows(&driver).join("\n");
    assert!(
        shown.contains("file 9") && !shown.contains("file 10"),
        "{shown}"
    );
    assert!(shown.contains("… and 4 more"), "{shown}");
}
