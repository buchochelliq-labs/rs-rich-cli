//! Sharp edges a new user hits: reactive cycles, keys that never fire,
//! focus after a removed row, clock jumps, tiny screens. Each test is the
//! smallest app that hit it.

mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

use common::{row_of, run_open, screen};
use intuituive::interact::headless::Script;
use intuituive::interact::{Event, Key};
use intuituive::prelude::*;

fn press(key: &str) -> Event {
    Event::Key(Key::parse(key).expect("a key name"))
}

fn frame(driver: &mut intuituive::Driver, now: Duration) -> Vec<String> {
    driver.update(now);
    let _ = driver.render();
    driver.screen().plain()
}

/// The message of a panic in `f`.
fn panic_of(f: impl FnOnce()) -> String {
    let error = catch_unwind(AssertUnwindSafe(f)).expect_err("it panics");
    error
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[test]
fn a_memo_that_writes_a_signal_panics_with_a_reason_not_a_stack_overflow() {
    let message = panic_of(|| {
        let app = App::new(|| {
            let n = signal(0);
            let m = memo(move || {
                let v = n.get();
                n.set(v + 1);
                v
            });
            text!("{m}")
        });
        let mut driver = app.driver(10, 1);
        frame(&mut driver, Duration::ZERO);
    });
    assert!(message.contains("while a memo computes"), "{message}");
}

#[test]
fn writing_another_signal_inside_an_update_reaches_memos_afterwards() {
    let app = App::new(|| {
        let a = signal(1);
        let b = signal(0);
        let sum = memo(move || a.get() + b.get());
        text!("sum {sum}").on_key("x", move |_| {
            a.update(|v| {
                b.set(7);
                *v += 1
            })
        })
    });
    let mut driver = app.driver(10, 1);
    assert_eq!(frame(&mut driver, Duration::ZERO)[0].trim_end(), "sum 1");
    driver.event(press("x"));
    assert_eq!(frame(&mut driver, Duration::ZERO)[0].trim_end(), "sum 9");
}

#[test]
fn a_node_that_writes_what_it_reads_while_drawing_is_stopped_and_named() {
    let app = App::new(|| {
        let n = signal(0u64);
        column([
            text(move || {
                let v = n.get();
                n.set(v + 1);
                format!("n={v}")
            })
            .name("counter"),
            label(""),
            label(""),
        ])
    });
    let mut driver = app.driver(80, 6);
    for _ in 0..10 {
        frame(&mut driver, Duration::ZERO);
    }
    // It settled: nothing left to draw, and the loop may wait.
    assert!(!driver.needs_render());
    assert!(driver.timeout(Duration::ZERO) > Duration::ZERO);
    let rows = driver.screen().plain();
    assert!(
        row_of(&rows, "counter").is_some(),
        "the toast names it: {rows:?}"
    );
}

#[test]
fn a_node_that_moves_what_it_reads_once_still_draws_again() {
    // A scroll keeping the focus in view writes its offset while it
    // draws, once; that must keep working.
    let app = App::new(|| {
        let rows: Vec<Node> = (0..30)
            .map(|i| label(format!("row {i}")).focusable().focus_style("reverse"))
            .collect();
        scroll(column(rows))
    });
    let mut driver = app.driver(20, 5);
    frame(&mut driver, Duration::ZERO);
    let mut rows = Vec::new();
    for _ in 0..10 {
        driver.event(press("tab"));
        rows = frame(&mut driver, Duration::ZERO);
    }
    // The view followed the focus down, frame after frame.
    assert!(row_of(&rows, "row 0").is_none(), "{rows:?}");
    assert!(row_of(&rows, "row 9").is_some(), "{rows:?}");
}

#[test]
fn a_watch_that_sets_its_own_source_stops_and_says_so() {
    let app = App::new(|| {
        let n = signal(0u64);
        watch(move || n.get(), move |v, _| n.set(v + 1));
        column([text!("n={n}"), label(""), label(""), label("")])
    });
    let mut driver = app.driver(90, 6);
    frame(&mut driver, Duration::ZERO);
    let after_first = driver.screen().plain()[0].clone();
    let rows = frame(&mut driver, Duration::from_millis(50));
    // Not resumed on the next update: the counter stays where it stopped.
    assert_eq!(rows[0], after_first);
    let rows = frame(&mut driver, Duration::from_millis(100));
    assert_eq!(rows[0], after_first);
    assert!(
        row_of(&rows, "A watch kept setting itself off").is_some(),
        "{rows:?}"
    );
}

#[test]
fn a_binding_for_ctrl_c_runs_instead_of_quitting() {
    let app = App::new(|| {
        let copied = signal(false);
        text!("copied {copied}").on_key("ctrl+c", move |_| copied.set(true))
    });
    let mut driver = app.driver(20, 1);
    frame(&mut driver, Duration::ZERO);
    driver.event(press("ctrl+c"));
    assert!(!driver.is_done());
    assert_eq!(
        frame(&mut driver, Duration::ZERO)[0].trim_end(),
        "copied true"
    );
    // Without a binding, Ctrl+C still quits.
    let mut driver = App::new(|| label("hi")).driver(10, 1);
    frame(&mut driver, Duration::ZERO);
    driver.event(press("ctrl+c"));
    assert!(driver.is_done());
}

#[test]
fn key_names_that_could_never_fire_now_do_or_fail_loudly() {
    let app = App::new(|| {
        let last = signal(String::new());
        text!("{last}")
            .on_key("shift+a", move |_| last.set("shift+a".into()))
            .on_key("ctrl+i", move |_| last.set("ctrl+i".into()))
    });
    let mut driver = app.driver(20, 1);
    frame(&mut driver, Duration::ZERO);
    // What a legacy terminal sends for them.
    driver.event(Event::Key(Key::char('A')));
    assert_eq!(frame(&mut driver, Duration::ZERO)[0].trim_end(), "shift+a");
    driver.event(press("tab"));
    assert_eq!(frame(&mut driver, Duration::ZERO)[0].trim_end(), "ctrl+i");
    // A terminal with the kitty protocol tells Ctrl+I from Tab.
    driver.event(Event::Key(Key::char('A').exact()));
    driver.event(Event::Key(Key::parse("tab").expect("a key").exact()));
    assert_eq!(frame(&mut driver, Duration::ZERO)[0].trim_end(), "shift+a");
    driver.event(Event::Key(Key::parse("ctrl+i").expect("a key").exact()));
    assert_eq!(frame(&mut driver, Duration::ZERO)[0].trim_end(), "ctrl+i");

    let message = panic_of(|| {
        let _ = label("x").on_key(" ", |_| {});
    });
    assert!(message.contains("\"space\""), "{message}");
    let message = panic_of(|| {
        let _ = label("x").on_key("f99", |_| {});
    });
    assert!(message.contains("f99"), "{message}");
}

#[test]
fn removing_the_focused_row_focuses_the_next_one() {
    // A field above the list takes the focus first, as a to-do app's
    // entry does: the focus must not jump back to it.
    let app = App::new(|| {
        let names = signal(vec!["one", "two", "three"]);
        let picked = signal("");
        column([
            label("new").focusable(),
            each(
                move || names.get(),
                move |name| {
                    label(name)
                        .focus_style("reverse")
                        .on_key("d", move |_| names.update(|n| n.retain(|x| *x != name)))
                        .on_key("space", move |_| picked.set(name))
                },
            ),
            text!("picked {picked}"),
        ])
    });
    let mut driver = app.driver(20, 5);
    frame(&mut driver, Duration::ZERO);
    // To "two", delete it: Space reaches "three", the row after it.
    for key in ["tab", "tab", "d"] {
        driver.event(press(key));
        frame(&mut driver, Duration::ZERO);
    }
    driver.event(press("space"));
    let rows = frame(&mut driver, Duration::ZERO);
    assert!(row_of(&rows, "picked three").is_some(), "{rows:?}");
    // Deleting the last row focuses the one before it.
    driver.event(press("d"));
    frame(&mut driver, Duration::ZERO);
    driver.event(press("space"));
    let rows = frame(&mut driver, Duration::ZERO);
    assert!(row_of(&rows, "picked one").is_some(), "{rows:?}");
}

#[test]
fn a_signal_from_another_app_redraws_the_app_that_reads_it() {
    let mut shared = None;
    let a = App::new(|| {
        let n = signal(1);
        shared = Some(n);
        text!("a{n}")
    });
    let n = shared.expect("a made it");
    let b = App::new(move || text!("b{n}").on_key("+", move |_| n.update(|v| *v += 1)));
    let mut a = a.driver(10, 1);
    let mut b = b.driver(10, 1);
    frame(&mut a, Duration::ZERO);
    assert_eq!(frame(&mut b, Duration::ZERO)[0].trim_end(), "b1");
    b.event(press("+"));
    assert_eq!(frame(&mut b, Duration::ZERO)[0].trim_end(), "b2");
    assert_eq!(frame(&mut a, Duration::ZERO)[0].trim_end(), "a2");
}

#[test]
fn watch_outside_an_app_says_watch() {
    let message = panic_of(|| watch(|| 1, |_, _| {}));
    assert!(message.starts_with("watch()"), "{message}");
}

#[test]
fn a_clock_jump_ticks_a_timer_once_without_a_stall() {
    let app = App::new(|| {
        let ticks = signal(0u32);
        every(Duration::from_millis(1), move |_| ticks.update(|t| *t += 1));
        text!("{ticks}")
    });
    let mut driver = app.driver(10, 1);
    frame(&mut driver, Duration::ZERO);
    let started = std::time::Instant::now();
    let rows = frame(&mut driver, Duration::from_secs(24 * 3600));
    assert!(started.elapsed() < Duration::from_millis(100));
    assert_eq!(rows[0].trim_end(), "1");
    // And keeps in step afterwards.
    let rows = frame(
        &mut driver,
        Duration::from_secs(24 * 3600) + Duration::from_millis(1),
    );
    assert_eq!(rows[0].trim_end(), "2");
}

#[test]
fn a_toast_shows_on_a_screen_too_small_for_its_box() {
    let app = App::new(|| label("hi").on_key("t", |cx| cx.toast("saved")));
    let rows = screen(&run_open(app, Script::new().keys("t"), 10, 2));
    assert_eq!(rows[1].trim_end(), "saved", "{rows:?}");
}
