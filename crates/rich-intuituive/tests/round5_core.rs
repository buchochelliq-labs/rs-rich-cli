//! Round 5's generic core pieces: the focus a wrapper hands to the list it
//! builds, a help that wraps its keys and filters after `/`, bindings that
//! scale a step, ignore modifiers, fall through or hide, played feeds with
//! a state and a tail at their own pace, waits the test clock counts as
//! asleep, recent writes with their times, falling values, a level overlay
//! on Miller columns, marks by key, find from the hovered item, numbered
//! tabs, optional status segments, styled facts, text edited in
//! `$EDITOR`, state files, a frecency store and jobs running a process.
//! Nothing about files, sound or one app is in them.

use intuituive::interact::Event;
use intuituive::prelude::*;
use intuituive::Driver;

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// 1. `autofocus` on a wrapper focuses what it builds.

#[test]
fn autofocus_on_a_switch_focuses_its_list_which_gets_its_keys_and_the_menu_key() {
    let app = App::new(|| {
        let items = signal(names(&["one", "two", "three"]));
        let cursor = signal(0usize);
        let menu = signal(String::from("none"));
        column([
            switch(
                move || items.with(Vec::is_empty),
                move |empty| {
                    if empty {
                        label("No items")
                    } else {
                        list(move || items.get(), cursor).name("items")
                    }
                },
            )
            .on_context(move |i, _| menu.set(format!("menu on {i}")))
            .autofocus()
            .fixed(3),
            text!("{menu}"),
        ])
    });
    let mut driver = app.driver(20, 4);
    driver.settle();
    // The list, not the switch, holds the focus: its own Down moves it.
    assert_eq!(driver.focused_name().as_deref(), Some("items"));
    driver.press("down");
    driver.press("menu");
    assert_eq!(rows(&driver)[3], "menu on 1");
}

#[test]
fn autofocus_on_a_switch_built_later_passes_the_focus_once_it_is_built() {
    let app = App::new(|| {
        let items = signal(Vec::<String>::new());
        let cursor = signal(0usize);
        switch(
            move || items.with(Vec::is_empty),
            move |empty| {
                if empty {
                    label("No items")
                } else {
                    list(move || items.get(), cursor).name("items")
                }
            },
        )
        .autofocus()
        .on_key("f", move |_| items.set(names(&["a", "b"])))
    });
    let mut driver = app.driver(10, 2);
    driver.settle();
    driver.press("f");
    driver.settle();
    assert_eq!(driver.focused_name().as_deref(), Some("items"));
    driver.press("down");
    assert!(intuituive::testing::styled(&driver).contains("[reverse]b"));
}

// 2. The help wraps a long key list; `/` starts its search.

#[test]
fn the_help_wraps_a_long_key_list_instead_of_cutting_it() {
    let app = App::new(|| {
        label("scope")
            .bind(
                "pageup shift+pageup ctrl+pageup alt+pageup",
                "Average more",
                |_| {},
            )
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(70, 14);
    driver.press("?");
    let help = rows(&driver).join("\n");
    assert!(help.contains("pgup/shift+pgup/"), "{help}");
    assert!(help.contains("ctrl+pgup/alt+pgup"), "{help}");
}

#[test]
fn a_filter_first_help_moves_with_j_and_k_and_searches_after_slash() {
    use intuituive::{HelpOptions, KeyFormat};
    let app = App::new(|| {
        label("files")
            .bind("y", "Yank", |_| {})
            .bind("x", "Cut", |_| {})
            .bind("p", "Paste", |_| {})
            .on_key("f2", |cx| {
                cx.help_with(
                    HelpOptions::new()
                        .key_format(KeyFormat::Vim)
                        .filter_first(true)
                        .filter_key("/"),
                )
            })
    });
    let mut driver = app.driver(40, 14);
    driver.press("f2");
    driver.press("j k x");
    let help = rows(&driver).join("\n");
    // Letters moved; none was searched for.
    assert!(help.contains("Yank") && help.contains("Paste"), "{help}");
    assert!(help.contains("/ search"), "{help}");
    driver.press("/");
    driver.type_text("cut");
    let help = rows(&driver).join("\n");
    assert!(help.contains("Cut") && !help.contains("Yank"), "{help}");
    // Esc ends the search, then closes the help.
    driver.press("esc");
    assert!(rows(&driver).join("\n").contains("Yank"));
    driver.press("esc");
    assert!(!rows(&driver).join("\n").contains("Yank"));
}

#[allow(dead_code)]
fn unused(_: Event) {}
