//! Pilot-style test helpers: clicks and hovers by node name (#810).

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{Easing, button};

fn app(moved: bool) -> App {
    App::new(move || {
        let presses = signal(0u32);
        let ok = button("OK", move |_| presses.update(|n| *n += 1))
            .name("ok")
            .tooltip("Accept");
        let mut rows = vec![text!("presses {presses}"), label("filler")];
        if moved {
            rows.insert(1, label("pushed down"));
            rows.push(row([label("left").fixed(8), ok]));
        } else {
            rows.insert(1, ok);
        }
        column(rows)
    })
}

#[test]
fn click_on_runs_the_handler_wherever_the_button_is() {
    for moved in [false, true] {
        let mut driver = app(moved).driver(30, 8);
        driver.click_on("ok");
        assert_eq!(
            driver.screen().plain()[0].trim_end(),
            "presses 1",
            "moved: {moved}"
        );
        assert!(driver.rect_of("ok").is_some());
    }
}

#[test]
fn hover_on_shows_the_tooltip_after_the_delay() {
    let mut driver = app(false).driver(30, 8);
    driver.hover_on("ok");
    assert!(!driver.screen().plain().join("\n").contains("Accept"));
    driver.advance(Duration::from_secs(1));
    assert!(driver.screen().plain().join("\n").contains("Accept"));
}

#[test]
fn an_unknown_name_lists_the_named_nodes() {
    let message = std::panic::catch_unwind(|| {
        let mut driver = app(false).driver(30, 8);
        driver.click_on("cancel");
    })
    .expect_err("an unknown name panics");
    let message = message
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(message.contains("`cancel`"), "{message}");
    assert!(message.contains("ok"), "{message}");
    assert!(app(false).driver(30, 8).rect_of("cancel").is_none());
}

#[test]
fn accessible_names_find_nodes_too() {
    let app = App::new(|| {
        let n = signal(0u32);
        column([text!("{n}"), button("Save", move |_| n.update(|n| *n += 1))])
    });
    let mut driver = app.driver(20, 4);
    driver.click_on("Save");
    assert_eq!(driver.screen().plain()[0].trim_end(), "1");
}

#[test]
fn scroll_and_double_click_on() {
    let app = App::new(|| {
        let (wheel, clicks) = (signal(0i32), signal(0u32));
        column([
            text!("{wheel} {clicks}"),
            label("pad")
                .name("pad")
                .on_click(move |_| clicks.update(|n| *n += 1))
                .on_mouse(move |_, mouse| {
                    use intuituive::interact::MouseKind;
                    match mouse.kind {
                        MouseKind::ScrollDown => wheel.update(|w| *w += 1),
                        MouseKind::ScrollUp => wheel.update(|w| *w -= 1),
                        _ => return false,
                    }
                    true
                }),
        ])
    });
    let mut driver = app.driver(10, 3);
    driver.scroll_on("pad", 3);
    driver.scroll_on("pad", -1);
    driver.double_click_on("pad");
    assert_eq!(driver.screen().plain()[0].trim_end(), "2 2");
}

#[test]
fn wait_for_animations_runs_an_animation_to_its_end() {
    let app = App::new(|| {
        let value = signal(0.0f64);
        text!("{value}").on_key("a", move |cx| {
            cx.animate(value, 10.0, Duration::from_millis(300), Easing::Linear)
        })
    });
    let mut driver = app.driver(10, 1);
    driver.press("a");
    driver.wait_for_animations();
    assert_eq!(driver.screen().plain()[0].trim_end(), "10");
}
