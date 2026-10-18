//! Inputs bound to signals (gap batch B5): `input` and `number_input`.

use intuituive::a11y::Role;
use intuituive::interact::Event;
use intuituive::prelude::*;
use intuituive::{assert_screen, Driver};

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

#[test]
fn typing_sets_the_signal_and_setting_it_replaces_the_text() {
    let app = App::new(|| {
        let name = signal(String::from("Ad"));
        column([input(name).fixed(1), text!("[{name}]").fixed(1)])
            .on_key("ctrl+r", move |_| name.set("Grace".into()))
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    // The caret starts at the end of what the signal held.
    driver.press("end");
    driver.type_text("a");
    assert_eq!(rows(&driver), ["Ada", "[Ada]"]);
    driver.press("ctrl+r");
    assert_eq!(rows(&driver), ["Grace", "[Grace]"]);
}

#[test]
fn enter_submits_and_inserts_nothing() {
    let app = App::new(|| {
        let name = signal(String::new());
        let sent = signal(Vec::<String>::new());
        column([
            input(name)
                .on_submit(move |value, _| sent.update(|v| v.push(value.to_string())))
                .fixed(1),
            text(move || sent.get().join(",")).fixed(1),
        ])
    });
    let mut driver = app.driver(20, 2);
    driver.type_text("hi");
    driver.press("enter");
    assert_eq!(rows(&driver), ["hi", "hi"]);
    // Pasted lines join on one line.
    driver.event(Event::Paste(" a\nb".into()));
    driver.settle();
    assert_eq!(rows(&driver)[0], "hi a b");
}

#[test]
fn without_a_handler_enter_goes_on_to_the_app() {
    let app = App::new(|| {
        let name = signal(String::new());
        let seen = signal(0);
        column([input(name).fixed(1), text!("{seen}").fixed(1)])
            .on_key("enter", move |_| seen.update(|n| *n += 1))
    });
    let mut driver = app.driver(20, 2);
    driver.type_text("x");
    driver.press("enter");
    assert_eq!(rows(&driver), ["x", "1"]);
}

#[test]
fn a_mask_shows_bullets_while_the_signal_holds_the_text() {
    let app = App::new(|| {
        let secret = signal(String::new());
        column([
            input(secret).mask('•').placeholder("Password").fixed(1),
            text!("{secret}").fixed(1),
        ])
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    assert_eq!(rows(&driver)[0], "Password");
    driver.type_text("hunter2");
    assert_eq!(rows(&driver), ["•••••••", "hunter2"]);
    assert_eq!(driver.cursor(), Some((7, 0)));
}

#[test]
fn max_length_stops_typing() {
    let app = App::new(|| input(signal(String::new())).max_length(3));
    let mut driver = app.driver(10, 1);
    driver.type_text("abcdef");
    assert_eq!(rows(&driver)[0], "abc");
}

#[test]
fn input_scrolled() {
    let app = App::new(|| input(signal(String::new())));
    let mut driver = app.driver(10, 1);
    driver.type_text("the quick brown fox");
    assert_screen!(driver, "input_scrolled");
    // The end of the line, and the caret after it.
    assert_eq!(rows(&driver)[0], "brown fox");
    assert_eq!(driver.cursor(), Some((9, 0)));
}

#[test]
fn number_input_clamps() {
    let app = App::new(|| {
        let age = signal(30i64);
        column([
            number_input(age, 0..=150).fixed(1),
            text!("age {age}").fixed(1),
            label("next").focusable().fixed(1),
        ])
    });
    let mut driver = app.driver(12, 3);
    driver.settle();
    driver.press("up down down");
    assert_eq!(rows(&driver)[1], "age 29");
    // Typing replaces the number; out of range, the value stays in it.
    driver.type_text("200");
    assert_eq!(rows(&driver)[..2], ["200", "age 150"]);
    // Leaving shows the value.
    driver.press("tab");
    assert_eq!(rows(&driver)[0], "150");
    driver.press("shift+tab end pageup");
    assert_eq!(rows(&driver)[1], "age 150");
    driver.press("home");
    assert_eq!(rows(&driver)[1], "age 0");
    driver.press("pageup");
    assert_eq!(rows(&driver)[1], "age 10");
}

#[test]
fn number_input_steps_and_takes_negatives() {
    let app = App::new(|| {
        let n = signal(0i64);
        column([
            number_input(n, -50..=50).step(5).fixed(1),
            text!("{n}").fixed(1),
        ])
    });
    let mut driver = app.driver(12, 2);
    driver.press("down");
    assert_eq!(rows(&driver)[1], "-5");
    driver.type_text("-12");
    assert_eq!(rows(&driver)[1], "-12");
    driver.press("backspace");
    assert_eq!(rows(&driver), ["-1", "-1"]);
    driver.press("esc");
    assert_eq!(rows(&driver)[0], "-1");
}

#[test]
fn tab_goes_through_three_inputs() {
    let app = App::new(|| {
        column([
            input(signal(String::new())).name("first").fixed(1),
            input(signal(String::new())).name("last").fixed(1),
            number_input(signal(0), 0..=150).name("age").fixed(1),
        ])
    });
    let mut driver = app.driver(20, 3);
    driver.settle();
    driver.assert_focused("first");
    driver.type_text("Ada");
    driver.press("tab");
    driver.assert_focused("last");
    driver.press("tab");
    driver.assert_focused("age");
    driver.press("shift+tab shift+tab");
    driver.assert_focused("first");
    assert_eq!(rows(&driver)[0], "Ada");
}

#[test]
fn the_role_is_a_text_box_with_the_text_as_its_value() {
    let app = App::new(|| {
        column([
            input(signal(String::from("Ada"))).label("Name").fixed(1),
            number_input(signal(42), 0..=99).label("Age").fixed(1),
        ])
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    let tree = driver.accessibility();
    assert_eq!(tree[0].role, Role::TextBox);
    assert_eq!(tree[0].name, "Name");
    assert_eq!(tree[0].value.as_deref().map(str::trim_end), Some("Ada"));
    assert_eq!(tree[1].role, Role::TextBox);
    assert_eq!(tree[1].value.as_deref().map(str::trim_end), Some("42"));
}

#[test]
fn errors_show_with_error_when() {
    let app = App::new(|| {
        let name = signal(String::new());
        input(name).error_when(move || name.with(|n| n.len() == 1))
    });
    let mut driver = app.driver(10, 1);
    driver.type_text("a");
    assert!(driver.accessibility()[0].state.invalid);
    driver.type_text("b");
    assert!(!driver.accessibility()[0].state.invalid);
}
