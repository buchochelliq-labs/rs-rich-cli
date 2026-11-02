//! Screen modes and screens that hand back a value (#808).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use intuituive::prelude::*;

fn first_line(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

#[test]
fn a_mode_keeps_its_stack_while_another_shows() {
    let app = App::new(|| {
        label("chat home")
            .on_key("p", |cx| {
                cx.push(|| {
                    let n = signal(0u32);
                    text!("pushed {n}")
                        .on_key("+", move |_| n.update(|n| *n += 1))
                        .on_key("s", |cx| cx.switch_mode("settings"))
                })
            })
            .on_key("s", |cx| cx.switch_mode("settings"))
    })
    .mode("chat", || label("unused"))
    .mode("settings", || {
        text!("settings").on_key("c", |cx| {
            assert_eq!(cx.mode(), "settings");
            cx.switch_mode(intuituive::DEFAULT_MODE)
        })
    });
    let mut driver = app.driver(20, 1);
    driver.settle();
    driver.press("p + +");
    assert_eq!(first_line(&driver), "pushed 2");
    driver.press("s");
    assert_eq!(first_line(&driver), "settings");
    driver.press("c");
    // The pushed screen is still on top, with its state.
    assert_eq!(first_line(&driver), "pushed 2");
    driver.press("+");
    assert_eq!(first_line(&driver), "pushed 3");
}

#[test]
fn an_unknown_mode_does_nothing() {
    let app = App::new(|| label("home").on_key("x", |cx| cx.switch_mode("nowhere")));
    let mut driver = app.driver(10, 1);
    driver.press("x");
    assert_eq!(first_line(&driver), "home");
}

#[test]
fn push_for_hands_back_the_dismissed_value_or_none() {
    let got: Rc<RefCell<Vec<Option<u32>>>> = Rc::default();
    let seen = got.clone();
    let app = App::new(move || {
        let got = got.clone();
        label("home").on_key("p", move |cx| {
            let got = got.clone();
            cx.push_for::<u32>(
                || {
                    label("picker")
                        .focusable()
                        .on_key("7", |cx| cx.dismiss(7u32))
                        .on_key("esc", |cx| cx.pop())
                },
                move |value, _| got.borrow_mut().push(value),
            )
        })
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    driver.press("p");
    assert_eq!(first_line(&driver), "picker");
    driver.press("7");
    assert_eq!(first_line(&driver), "home");
    driver.press("p esc");
    assert_eq!(first_line(&driver), "home");
    assert_eq!(*seen.borrow(), vec![Some(7), None]);
}

#[test]
fn modal_for_hands_back_a_value_and_can_open_another_screen() {
    let app = App::new(|| {
        let picked = signal(String::from("none"));
        text!("picked {picked}").on_key("m", move |cx| {
            cx.modal_for::<String>(
                Size::Auto,
                Size::Auto,
                || label("a or b").on_key("a", |cx| cx.dismiss(String::from("a"))),
                move |value, cx| {
                    picked.set(value.unwrap_or_default());
                    cx.toast("done");
                },
            )
        })
    });
    let mut driver = app.driver(20, 3);
    driver.press("m a");
    assert!(first_line(&driver).starts_with("picked a"));
    assert!(driver.screen().plain().join("\n").contains("done"));
}

#[test]
fn a_stashed_modes_timers_do_not_fire_until_it_shows_again() {
    let ticks = Rc::new(Cell::new(0u32));
    let seen = ticks.clone();
    let app = App::new(move || {
        let ticks = ticks.clone();
        every(Duration::from_millis(100), move |_| {
            ticks.set(ticks.get() + 1)
        });
        label("home").on_key("s", |cx| cx.switch_mode("other"))
    })
    .mode("other", || {
        label("other").on_key("h", |cx| cx.switch_mode(intuituive::DEFAULT_MODE))
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    driver.advance(Duration::from_millis(250));
    let before = seen.get();
    assert_eq!(before, 2);
    driver.press("s");
    driver.advance(Duration::from_secs(1));
    assert_eq!(seen.get(), before, "no ticks while another mode shows");
    driver.press("h");
    driver.advance(Duration::from_millis(1));
    // One tick for the time it was due, then on in step.
    assert_eq!(seen.get(), before + 1);
}
