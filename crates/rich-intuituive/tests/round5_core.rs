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

// 3. Bindings: scaled steps, any modifiers, falling through, hidden.

#[derive(Default)]
struct Scope {
    samples: u32,
    threshold: f64,
    average: u32,
}

fn samples(s: &mut Scope) -> &mut u32 {
    &mut s.samples
}

fn threshold(s: &mut Scope) -> &mut f64 {
    &mut s.threshold
}

const MAGNITUDES: [(&str, f64, &str); 4] = [
    ("", 1.0, "×1"),
    ("shift", 10.0, "×10"),
    ("ctrl", 5.0, "×5"),
    ("alt", 0.2, "⅕"),
];

#[test]
fn scaled_steps_move_a_clamped_field_by_the_modifiers_factor() {
    let app = App::new(|| {
        let scope = signal(Scope {
            samples: 1000,
            threshold: 0.0,
            average: 1,
        });
        text(move || scope.with(|s| format!("{} {:.1}", s.samples, s.threshold)))
            .bind_step_scaled(
                "right",
                &MAGNITUDES,
                "More samples",
                scope,
                samples,
                10.0,
                10..=2000,
            )
            .bind_step_scaled(
                "left",
                &MAGNITUDES,
                "Fewer samples",
                scope,
                samples,
                -10.0,
                10..=2000,
            )
            .bind_step_scaled(
                "up",
                &MAGNITUDES,
                "Threshold up",
                scope,
                threshold,
                0.5,
                -1.0..=8.0,
            )
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(60, 12);
    driver.press("right shift+right ctrl+right alt+right");
    // 1000 + 10 + 100 + 50 + 2.
    assert_eq!(rows(&driver)[0], "1162 0.0");
    driver.press("shift+left shift+left shift+left shift+left shift+left shift+left shift+left shift+left shift+left shift+left shift+left shift+left");
    assert_eq!(rows(&driver)[0], "10 0.0");
    driver.press("up alt+up shift+up shift+up");
    assert_eq!(rows(&driver)[0], "10 8.0");
    driver.press("?");
    let help = rows(&driver).join("\n");
    assert!(
        help.contains("More samples (shift ×10, ctrl ×5,") && help.contains("alt ⅕)"),
        "{help}"
    );
}

#[test]
fn a_modifier_blind_binding_takes_its_key_with_anything_held_but_yields_to_an_exact_one() {
    let app = App::new(|| {
        let scope = signal(Scope::default());
        text(move || scope.with(|s| s.average.to_string()))
            .bind_step(
                "pageup",
                "Average more",
                scope,
                |s| &mut s.average,
                1.0,
                1..=100,
            )
            .any_modifiers()
            .bind_step(
                "ctrl+alt+pageup",
                "Average much more",
                scope,
                |s| &mut s.average,
                50.0,
                1..=100,
            )
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(60, 12);
    driver.press("pageup shift+pageup ctrl+pageup alt+pageup");
    assert_eq!(rows(&driver)[0], "4");
    driver.press("ctrl+alt+pageup");
    assert_eq!(rows(&driver)[0], "54");
    // Listed once, with its own key.
    driver.press("?");
    let help = rows(&driver).join("\n");
    assert_eq!(help.matches("Average more").count(), 1, "{help}");
}

#[test]
fn a_binding_that_falls_through_runs_and_then_the_next_handler_of_its_key_does() {
    let app = App::new(|| {
        let said = signal(String::new());
        let triggered = signal(true);
        column([
            text!("{said}")
                .bind("esc", "Stop the trigger", move |_| {
                    triggered.set(false);
                    said.update(|s| s.push_str("trigger "));
                })
                .fall_through()
                .bind("esc", "Then this one", move |cx| {
                    said.update(|s| s.push_str("second "));
                    cx.pass_key();
                })
                .autofocus(),
            label("x"),
        ])
        .bind("esc", "Reset", move |_| {
            said.update(|s| s.push_str("reset"))
        })
    });
    let mut driver = app.driver(30, 2);
    driver.press("esc");
    assert_eq!(rows(&driver)[0], "trigger second reset");
}

#[test]
fn a_hidden_binding_works_is_left_out_of_the_help_and_runs_by_name() {
    let app = App::new(|| {
        let n = signal(0);
        text!("{n}")
            .bind("+", "Add one", move |_| n.update(|n| *n += 1))
            .bind_hidden("shift+pageup ctrl+pageup", "Add one, any way", move |_| {
                n.update(|n| *n += 1)
            })
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(50, 12);
    driver.press("+ shift+pageup ctrl+pageup");
    assert_eq!(rows(&driver)[0], "3");
    assert!(driver.run_named("Add one, any way"));
    assert_eq!(rows(&driver)[0], "4");
    driver.press("?");
    let help = rows(&driver).join("\n");
    assert!(
        help.contains("Add one") && !help.contains("any way"),
        "{help}"
    );
}

#[allow(dead_code)]
fn unused(_: Event) {}
