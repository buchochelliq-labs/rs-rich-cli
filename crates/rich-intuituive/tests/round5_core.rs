//! Round 5's generic core pieces: the focus a wrapper hands to the list it
//! builds, a help that wraps its keys and filters after `/`, bindings that
//! scale a step, ignore modifiers, fall through or hide, played feeds with
//! a state and a tail at their own pace, waits the test clock counts as
//! asleep, recent writes with their times, falling values, a level overlay
//! on Miller columns, marks by key, find from the hovered item, numbered
//! tabs, optional status segments, styled facts, text edited in
//! `$EDITOR`, state files, a frecency store and jobs running a process.
//! Nothing about files, sound or one app is in them.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use intuituive::interact::Event;
use intuituive::miller::MillerSource;
use intuituive::prelude::*;
use intuituive::{miller_of, Driver, Notifier};

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

// 4. Feeds: a played feed's state, a tail at its own pace, waits the
// test clock counts as asleep, recent writes with their times.

#[test]
fn a_played_feed_ends_when_its_values_run_out_while_its_tail_plays_on() {
    use intuituive::{latest_iter, Pace};
    let app = App::new(|| {
        let (frame, state) = latest_iter(vec![1, 2, 3], Pace::Free)
            .then_at(Pace::Every(Duration::from_millis(100)), 10..)
            .with_state();
        let (last, done) = latest_iter(vec![7], Pace::Free).with_state();
        text(move || {
            format!(
                "{:?} {:?} {:?} {:?}",
                frame.get(),
                state.get(),
                last.get(),
                done.get()
            )
        })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(50, 1);
    driver.settle();
    // All three at once; the tail a beat after the last.
    assert_eq!(rows(&driver)[0], "Some(3) Ended Some(7) Ended");
    driver.advance(Duration::from_millis(350));
    assert_eq!(rows(&driver)[0], "Some(12) Ended Some(7) Ended");
}

#[test]
fn a_played_feed_stalls_between_slow_values_and_runs_again() {
    use intuituive::{latest_iter, Pace};
    let app = App::new(|| {
        let (frame, state) = latest_iter(1..=3, Pace::Every(Duration::from_millis(500)))
            .stall_after(Duration::from_millis(300))
            .with_state();
        text(move || format!("{:?} {:?}", frame.get(), state.get()))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(40, 1);
    driver.advance(Duration::from_millis(400));
    assert_eq!(rows(&driver)[0], "Some(1) Stalled(0ns)");
    driver.advance(Duration::from_millis(200));
    assert_eq!(rows(&driver)[0], "Some(2) Running");
    driver.advance(Duration::from_millis(500));
    assert_eq!(rows(&driver)[0], "Some(3) Ended");
}

/// What a test sees of a thread waiting on a condition another thread
/// makes true.
struct Device {
    filled: Arc<AtomicBool>,
    notifier: Arc<Mutex<Option<Notifier>>>,
}

#[test]
fn a_thread_waiting_for_a_condition_does_not_hold_the_test_clocks_turns() {
    let device = Rc::new(Device {
        filled: Arc::new(AtomicBool::new(false)),
        notifier: Arc::new(Mutex::new(None)),
    });
    let shared = device.clone();
    let app = App::new(move || {
        let (filled, slot) = (shared.filled.clone(), shared.notifier.clone());
        let said = latest(move |tx| {
            *slot.lock().unwrap() = Some(tx.notifier());
            let ready = tx.wait_for(|| filled.load(Ordering::SeqCst), None);
            tx.send(format!("ready {ready}"));
            // Then one with a timeout, on the app's clock.
            let ready = tx.wait_for(|| false, Duration::from_millis(300));
            tx.send(format!("timed out {}", !ready));
        });
        text(move || said.get().unwrap_or_else(|| "waiting".into()))
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(20, 1);
    let start = Instant::now();
    driver.advance(Duration::from_secs(1));
    // Asleep to the clock: the second went by without a five-second hold.
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(rows(&driver)[0], "waiting");
    device.filled.store(true, Ordering::SeqCst);
    let notifier = device.notifier.lock().unwrap().clone().unwrap();
    notifier.notify();
    driver.settle();
    assert_eq!(rows(&driver)[0], "ready true");
    driver.advance(Duration::from_millis(250));
    assert_eq!(rows(&driver)[0], "ready true");
    driver.advance(Duration::from_millis(100));
    assert_eq!(rows(&driver)[0], "timed out true");
}

#[test]
fn recent_keeps_a_part_of_each_write_and_its_time() {
    use intuituive::{recent_of, recent_timed};
    let app = App::new(|| {
        let frame = signal(vec![0u32; 1024]);
        let sums = recent_of(frame, 2, |f: &Vec<u32>| f.iter().sum::<u32>());
        let times = recent_timed(frame, 2);
        text(move || {
            let at: Vec<u128> = times.with(|t| t.iter().map(|(at, _)| at.as_millis()).collect());
            format!("{:?} {at:?}", sums.get())
        })
        .on_key("w", move |_| frame.update(|f| f[1] += 1))
    });
    let mut driver = app.driver(30, 1);
    driver.press("w");
    driver.advance(Duration::from_millis(40));
    driver.press("w");
    driver.advance(Duration::from_millis(40));
    driver.press("w");
    assert_eq!(rows(&driver)[0], "[2, 3] [40, 80]");
}

#[test]
fn a_falling_value_rises_at_once_and_never_falls_below_its_source() {
    use intuituive::falling;
    let app = App::new(|| {
        let level = signal(0.0);
        let bar = falling(level, 4.0);
        text(move || format!("{:.2}", bar.get()))
            .on_key("h", move |_| level.set(1.0))
            .on_key("m", move |_| level.set(0.5))
    });
    let mut driver = app.driver(10, 1);
    driver.press("h m");
    driver.advance(Duration::from_millis(200));
    // 1 - ½·4·0.2² = 0.92.
    let shown: f64 = rows(&driver)[0].parse().unwrap();
    assert!((0.90..0.94).contains(&shown), "{shown}");
    driver.advance(Duration::from_secs(2));
    assert_eq!(rows(&driver)[0], "0.50");
    driver.press("h");
    assert_eq!(rows(&driver)[0], "1.00");
}

// 5. Lists and Miller columns: a level overlay, marks by key, find from
// the hovered item, numbered tabs, optional segments, styled facts.

/// A two-level tree: `a` and `b` at the top, each with two leaves.
struct Tree;

impl MillerSource for Tree {
    type Key = String;
    fn items(&self, at: &String) -> Vec<String> {
        match at.as_str() {
            "" => names(&["a", "b"]),
            "a" => names(&["a1", "a2"]),
            _ => names(&["b1", "b2"]),
        }
    }
    fn child(&self, at: &String, name: &str) -> Option<String> {
        at.is_empty().then(|| name.to_string())
    }
    fn parent(&self, at: &String) -> Option<(String, String)> {
        (!at.is_empty()).then(|| (String::new(), at.clone()))
    }
}

#[test]
fn an_overlay_lists_other_items_at_a_level_until_the_columns_leave_it() {
    let app = App::new(|| {
        let m = miller_of(Tree, String::new());
        column([
            m.navigation(m.view()).flex(1),
            text(move || format!("{:?} {:?}", m.overlay_at(), m.hovered().get())).fixed(1),
        ])
        .on_key("s", move |_| {
            m.overlay(
                "a".into(),
                vec![("found b2".into(), None), ("in b".into(), Some("b".into()))],
            )
        })
    });
    let mut driver = app.driver(60, 3);
    // Into `a`, on `a2`, then the overlay at `a`.
    driver.press("l j s");
    assert_eq!(rows(&driver)[2], r#"Some("a") Some("found b2")"#);
    // Its second item previews the level it opens.
    driver.press("j");
    assert!(rows(&driver)[0].contains("b1"), "{:?}", rows(&driver));
    // `h` goes back to `a`'s own items, on the one hovered before.
    driver.press("h");
    assert_eq!(rows(&driver)[2], r#"None Some("a2")"#);
    // Entering an item goes to its level, and the overlay is over.
    driver.press("s j l");
    assert_eq!(rows(&driver)[2], r#"None Some("b1")"#);
    driver.press("h");
    assert_eq!(rows(&driver)[2], r#"None Some("b")"#);
}

#[test]
fn marks_by_key_show_a_selection_in_another_list_visual_range_too() {
    use intuituive::{multi_select, Mark};
    let app = App::new(|| {
        let items = signal(names(&["a", "b", "c"]));
        let cursor = signal(0usize);
        let picked = multi_select::<String>()
            .over(move || items.get())
            .range_from(cursor);
        // The same items, the other way round.
        let reversed = memo(move || items.with(|i| i.iter().rev().cloned().collect::<Vec<_>>()));
        row([
            list(move || items.get(), cursor).fixed(4),
            list(move || reversed.get(), signal(0))
                .marks_by(picked, move |i| reversed.with(|r| r[i].clone()))
                .no_focus()
                .fixed(4),
            text(move || format!("{:?}", picked.mark(&"b".to_string()))),
        ])
        .on_key("v", move |_| picked.visual(false))
        .on_key("space", move |_| picked.toggle("c".into()))
    });
    let mut driver = app.driver(30, 3);
    driver.press("space v down");
    let styled = intuituive::testing::styled(&driver);
    // c (row 0 of the reversed list) selected; b (row 1) in the range.
    let lines: Vec<&str> = styled.lines().collect();
    assert!(lines[0].contains("on yellow"), "{styled}");
    assert!(lines[1].contains("on cyan"), "{styled}");
    assert!(rows(&driver)[0].ends_with(&format!("{:?}", Mark::Visual)));
}

#[test]
fn find_from_the_hovered_item_stays_on_a_match() {
    use intuituive::list_search;
    let app = App::new(|| {
        let items = || names(&["main.rs", "lib.rs", "mod.rs"]);
        let cursor = signal(0usize);
        let find = list_search(|| "m".to_string(), items, cursor);
        let (here, next) = (find.clone(), find.clone());
        text!("{cursor}")
            .on_key("h", move |_| {
                here.next_from(true);
            })
            .on_key("n", move |_| {
                next.next_from(false);
            })
            .on_key("p", move |_| {
                find.previous_from(true);
            })
    });
    let mut driver = app.driver(5, 1);
    driver.press("h");
    assert_eq!(rows(&driver)[0], "0");
    driver.press("n");
    assert_eq!(rows(&driver)[0], "2");
    driver.press("p");
    assert_eq!(rows(&driver)[0], "2");
}

#[test]
fn numbered_tabs_and_a_segment_that_shows_only_while_it_should() {
    use intuituive::widgets::{seg, status_bar, Caps};
    use intuituive::{tab_set, tabs_for_indexed};
    let app = App::new(|| {
        let set = tab_set(|| "src".to_string());
        set.open_with("docs".to_string());
        let visual = signal(false);
        column([
            tabs_for_indexed(&set, |i, name| format!("{} {name}", i + 1)).fixed(1),
            status_bar(
                [
                    seg(|| " NOR ".into()).style("white on blue"),
                    seg(|| " VIS ".into())
                        .style("white on red")
                        .when(move || visual.get()),
                    seg(|| " notes".into()),
                ],
                [],
            )
            .caps(Caps::Powerline)
            .fixed(1),
        ])
        .on_key("v", move |_| visual.set(true))
    });
    let mut driver = app.driver(30, 2);
    driver.settle();
    let strip = rows(&driver)[0].clone();
    assert!(
        strip.contains("1 src") && strip.contains("2 docs"),
        "{strip}"
    );
    assert_eq!(rows(&driver)[1], " NOR \u{e0b0} notes");
    driver.press("v");
    assert_eq!(rows(&driver)[1], " NOR \u{e0b0} VIS \u{e0b0} notes");
}

#[allow(dead_code)]
fn unused(_: Event) {}
