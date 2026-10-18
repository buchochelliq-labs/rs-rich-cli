//! The port of ratatui's `input-form` and `user-input` (examples/form.rs):
//! the form empty, filled and in error, the chat's messages, as 80x24
//! snapshots; Tab and Enter between the fields; sending the form hands it
//! to `Driver::take_exit`.

#[path = "../examples/form.rs"]
#[allow(dead_code)]
mod form;

use form::{age_problem, form_app, name_problem, Person};
use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::{assert_screen, assert_screen_styled, Driver};

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

fn driver_at(width: u16, height: u16) -> Driver {
    let mut driver = form_app().driver(width, height);
    driver.settle();
    driver
}

fn driver() -> Driver {
    driver_at(WIDTH, HEIGHT)
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn has(driver: &Driver, needle: &str) -> bool {
    rows(driver).iter().any(|r| r.contains(needle))
}

#[test]
fn form_empty() {
    let driver = driver();
    driver.assert_focused("first-name");
    assert!(has(&driver, "Enter on the age sends the form"));
    assert_screen!(driver, "form_empty");
}

#[test]
fn form_filled() {
    let mut driver = driver();
    driver.type_text("Grace");
    driver.press("tab");
    driver.assert_focused("last-name");
    driver.type_text("Hopper");
    // Enter moves on too.
    driver.press("enter");
    driver.assert_focused("age");
    driver.type_text("85");
    driver.press("up");
    assert!(has(&driver, "Grace"), "{:?}", rows(&driver));
    assert!(has(&driver, "86"), "{:?}", rows(&driver));
    assert_screen!(driver, "form_filled");
    // Enter on the age sends it, and the app ends with it.
    driver.press("enter");
    assert!(driver.is_done());
    assert_eq!(
        driver.take_exit::<Person>(),
        Some(Person {
            first_name: "Grace".into(),
            last_name: "Hopper".into(),
            age: 86,
        })
    );
}

#[test]
fn form_error() {
    let mut driver = driver();
    // A digit in a name is wrong at once.
    driver.type_text("Ada1");
    assert!(has(&driver, "The first name takes letters"));
    // Sending the form shows the empty fields as wrong too, and does not
    // end the app.
    driver.press("tab tab enter");
    assert!(!driver.is_done());
    assert!(has(&driver, "The last name is empty."));
    assert!(has(&driver, "The age is not given."));
    assert_screen_styled!(driver, "form_error");
    let invalid: Vec<String> = driver
        .accessibility()
        .iter()
        .filter(|n| n.state.invalid)
        .map(|n| n.name.clone())
        .collect();
    assert!(invalid.len() >= 3, "{invalid:?}");
    // Fixed, the errors go and the form is sent.
    driver.press("shift+tab shift+tab");
    driver.assert_focused("first-name");
    driver.press("backspace");
    driver.press("tab");
    driver.type_text("King");
    driver.press("tab");
    driver.type_text("36");
    driver.press("enter");
    assert_eq!(
        driver.take_exit::<Person>().map(|p| p.to_string()),
        Some("Ada King, 36 years old".to_string())
    );
}

#[test]
fn form_messages() {
    let mut driver = driver();
    driver.press("f2");
    assert!(has(&driver, "Press q to quit, e to start editing"));
    // Not editing: typing goes to the app, not the input.
    driver.press("x");
    assert!(!has(&driver, "│x"));
    driver.press("e");
    driver.assert_focused("message");
    assert!(has(&driver, "Press Esc to stop editing"));
    driver.type_text("Hello there");
    driver.press("enter");
    driver.type_text("q is typed while editing");
    driver.press("enter");
    driver.press("esc");
    assert!(has(&driver, "1: Hello there"));
    assert!(has(&driver, "2: q is typed while editing"));
    assert!(has(&driver, "Press q to quit"));
    assert_screen!(driver, "form_messages");
    // Not editing, q quits.
    driver.press("q");
    assert!(driver.is_done());
    assert_eq!(driver.take_exit::<Person>(), None);
}

#[test]
fn a_click_changes_tabs_and_esc_quits_the_form() {
    let mut driver = driver();
    // " Form │ Chat ": Chat is at columns 8 to 11.
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        driver.event(Event::Mouse(Mouse::new(kind, 9, 0)));
    }
    driver.settle();
    assert!(has(&driver, "Messages"));
    driver.press("f1");
    driver.assert_focused("first-name");
    driver.press("esc");
    assert!(driver.is_done());
}

#[test]
fn the_rules() {
    assert_eq!(name_problem("", false), None);
    assert_eq!(name_problem(" ", true), Some("is empty"));
    assert_eq!(name_problem("Mary-Jane O'Neil", true), None);
    assert!(name_problem("R2D2", false).is_some());
    assert_eq!(age_problem(0, false), None);
    assert_eq!(age_problem(0, true), Some("is not given"));
    assert_eq!(age_problem(30, true), None);
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = driver();
    driver.press("f2 ?");
    assert!(has(&driver, "start editing the message"));
    driver.press("esc ctrl+p");
    assert!(has(&driver, "the Form tab"));
}

#[test]
fn both_tabs_draw_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        let mut driver = driver_at(width, height);
        driver.type_text("Ann");
        driver.press("tab tab enter f2 e");
        driver.type_text("hi");
        driver.press("enter esc f1");
    }
}
