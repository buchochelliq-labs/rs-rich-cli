//! Placing the caret and selecting in an input or a text area: `.caret`,
//! `.select`, and the caret of a prefilled input (at the end).

use intuituive::prelude::*;
use intuituive::{Caret, Driver};

fn first(driver: &Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

#[test]
fn a_prefilled_input_puts_the_caret_at_the_end() {
    let app = App::new(|| input(signal("alpha".to_string())));
    let mut driver = app.driver(20, 1);
    driver.type_text("x");
    assert_eq!(first(&driver), "alphax");
    assert_eq!(driver.cursor(), Some((6, 0)));
}

#[test]
fn text_set_from_elsewhere_puts_an_inputs_caret_at_the_end_too() {
    let app = App::new(|| {
        let name = signal(String::new());
        input(name).on_key("f2", move |_| name.set("Grace".into()))
    });
    let mut driver = app.driver(20, 1);
    driver.press("f2");
    driver.type_text("!");
    assert_eq!(first(&driver), "Grace!");
}

#[test]
fn a_text_area_keeps_its_caret_at_the_start() {
    let app = App::new(|| text_area(signal("one\ntwo".to_string())));
    let mut driver = app.driver(20, 2);
    driver.type_text(">");
    assert_eq!(first(&driver), ">one");
}

#[test]
fn the_caret_goes_where_it_is_told() {
    let app = App::new(|| input(signal("report.txt".to_string())).caret(Caret::Start));
    let mut driver = app.driver(20, 1);
    driver.type_text("my-");
    assert_eq!(first(&driver), "my-report.txt");

    let app = App::new(|| input(signal("report.txt".to_string())).caret(Caret::At(6)));
    let mut driver = app.driver(20, 1);
    driver.type_text("-v2");
    assert_eq!(first(&driver), "report-v2.txt");

    // Past the end is the end; in a text area, a line break counts one.
    let app = App::new(|| input(signal("ab".to_string())).caret(Caret::At(99)));
    let mut driver = app.driver(20, 1);
    driver.type_text("c");
    assert_eq!(first(&driver), "abc");
    let app = App::new(|| text_area(signal("ab\ncd".to_string())).caret(Caret::At(4)));
    let mut driver = app.driver(20, 2);
    driver.type_text("X");
    assert_eq!(driver.screen().plain()[1].trim_end(), "cXd");
}

#[test]
fn a_selection_is_replaced_by_what_is_typed() {
    let app = App::new(|| input(signal("report.txt".to_string())).select(0..6));
    let mut driver = app.driver(20, 1);
    driver.type_text("notes");
    assert_eq!(first(&driver), "notes.txt");
}

#[test]
fn characters_are_grapheme_clusters() {
    // "é" written as e and a combining accent is one character.
    let app = App::new(|| input(signal("e\u{301}tude".to_string())).caret(Caret::At(1)));
    let mut driver = app.driver(20, 1);
    driver.type_text("-");
    assert_eq!(first(&driver), "e\u{301}-tude");
}
