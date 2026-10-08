//! Linear mode: the accessibility tree written as lines of text, then only
//! what changed, the focus moving, and what was announced.

use std::time::Duration;

use intuituive::interact::{Event, Key};
use intuituive::prelude::*;
use intuituive::widgets::{tree, TreeItem};

fn step(driver: &mut intuituive::Driver, key: Option<&str>) -> Vec<String> {
    if let Some(key) = key {
        driver.event(Event::Key(Key::parse(key).expect("a key")));
    }
    driver.update(Duration::ZERO);
    let out = driver.render().unwrap_or_default();
    assert!(!out.contains('\x1b'), "no escape sequences: {out:?}");
    assert!(out.is_empty() || out.ends_with("\r\n"), "{out:?}");
    out.split_terminator("\r\n").map(str::to_string).collect()
}

fn app() -> App {
    App::new(|| {
        let picked = signal(0usize);
        let saved = signal(0);
        column([
            label("[b]Files[/]").fixed(1),
            label("~~~~").access_hidden(true).fixed(1),
            list(|| (1..=10).map(|n| format!("file{n}.rs")).collect(), picked)
                .label("Files")
                .fixed(3),
            text!("{saved} saved").live().fixed(1),
            label("Save")
                .on_click(|_| {})
                .focusable()
                .on_key("enter", move |cx| {
                    saved.update(|n| *n += 1);
                    cx.toast("Saved");
                })
                .fixed(1),
            tree(
                || vec![TreeItem::new("src").child(TreeItem::new("main.rs"))],
                signal(vec![0usize]),
            )
            .fixed(2),
        ])
        .on_key("a", |cx| cx.announce("Two files changed", false))
    })
    .linear(true)
}

#[test]
fn the_first_frame_is_the_whole_tree_in_reading_order() {
    let mut driver = app().driver(30, 12);
    assert!(driver.is_linear());
    assert_eq!(
        step(&mut driver, None),
        [
            "Files",
            "→ Files, list, 1 of 10: file1.rs, selected",
            "0 saved",
            "Save, button",
            "tree, 1 of 1: src, selected, collapsed",
        ]
    );
    // Nothing changed: nothing to write.
    assert_eq!(step(&mut driver, None), Vec::<String>::new());
}

#[test]
fn only_what_changed_is_written_with_the_focus_and_announcements() {
    let mut driver = app().driver(30, 12);
    step(&mut driver, None);
    assert_eq!(
        step(&mut driver, Some("down")),
        ["Files, list, 2 of 10: file2.rs, selected"]
    );
    assert_eq!(step(&mut driver, Some("tab")), ["→ Save, button"]);
    // The live line changes (written once, not announced again) and the
    // toast is announced.
    assert_eq!(step(&mut driver, Some("enter")), ["1 saved", "Saved"]);
    assert_eq!(step(&mut driver, Some("a")), ["Two files changed"]);
    assert_eq!(
        step(&mut driver, Some("tab")),
        ["→ tree, 1 of 1: src, selected, collapsed"]
    );
    assert_eq!(
        step(&mut driver, Some("right")),
        ["tree, 1 of 2: src, selected, expanded"]
    );
}

#[test]
fn a_dialog_is_written_and_the_screen_again_when_it_closes() {
    let app = App::new(|| {
        label("Delete")
            .focusable()
            .on_key("enter", |cx| {
                cx.modal(Size::Fixed(20), Size::Fixed(3), || {
                    label("Sure? y/n").focusable().on_key("n", |cx| cx.pop())
                })
            })
            .role(intuituive::a11y::Role::Button)
    })
    .linear(true);
    let mut driver = app.driver(30, 6);
    assert_eq!(step(&mut driver, None), ["→ Delete, button"]);
    let opened = step(&mut driver, Some("enter"));
    assert_eq!(opened, ["→ Sure? y/n, dialog"], "{opened:?}");
    assert_eq!(step(&mut driver, Some("n")), ["→ Delete, button"]);
}

#[test]
fn linear_mode_leaves_nothing_to_put_back() {
    let mut driver = app().driver(30, 12);
    step(&mut driver, None);
    assert_eq!(driver.finish(), "");
}

#[test]
fn keys_still_work_in_linear_mode() {
    let app = App::new(|| {
        let n = signal(0);
        text!("count {n}")
            .on_key("+", move |_| n.update(|n| *n += 1))
            .on_key("q", |cx| cx.quit())
    })
    .linear(true);
    let mut backend = intuituive::interact::headless::Headless::new(
        intuituive::interact::headless::Script::new().keys("+ + q"),
        20,
        2,
    );
    let record = backend.record();
    app.run_on(&mut backend).expect("the app runs");
    let written = record.borrow().output();
    assert_eq!(written, "count 0\r\ncount 1\r\ncount 2\r\n");
}
