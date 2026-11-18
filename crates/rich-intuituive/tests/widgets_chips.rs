//! Chips (rs-rich-ext's badges as nodes), removable ones, and a tag input.

use intuituive::prelude::*;
use intuituive::{Driver, chip, chip_of, tag_input};
use rich_ext::a11y::Status;
use rich_ext::badge::Badge;

fn row0(driver: &Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

#[test]
fn a_chip_is_a_badge_in_its_theme_style() {
    let app = App::new(|| {
        row([
            chip("beta"),
            chip_of(|| Badge::status(Status::Ok, "build")),
            chip_of(|| Badge::meta("v", "1.2")),
        ])
    });
    let mut driver = app.driver(30, 1);
    driver.settle();
    assert_eq!(row0(&driver), " beta  ✔ build  v  1.2");
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[black on white] beta [/]"), "{styled}");
    assert!(
        styled.contains("[bold black on green] ✔ build [/]"),
        "{styled}"
    );
}

#[test]
fn a_removable_chip_goes_by_a_click_on_its_cross_or_a_key() {
    let app = App::new(|| {
        let tags = signal(vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        each(
            move || tags.get(),
            move |tag: String| {
                let gone = tag.clone();
                chip(tag).on_remove(move |_| tags.update(|t| t.retain(|x| *x != gone)))
            },
        )
    });
    let mut driver = app.driver(10, 3);
    driver.settle();
    let rows = |driver: &Driver| -> Vec<String> {
        driver
            .screen()
            .plain()
            .iter()
            .map(|r| r.trim_end().to_string())
            .collect()
    };
    assert_eq!(rows(&driver), [" a ×", " b ×", " c ×"]);
    // A click on the chip's text: nothing.
    driver.click(1, 1);
    assert_eq!(rows(&driver), [" a ×", " b ×", " c ×"]);
    // On its cross: gone.
    driver.click(3, 1);
    assert_eq!(rows(&driver), [" a ×", " c ×", ""]);
    // The focus is on the first: Delete removes it.
    driver.press("delete");
    // The focus went from the chip removed to the next one, `c`.
    assert_eq!(rows(&driver), [" a ×", "", ""]);
    assert_eq!(driver.accessibility()[0].describe(), "[a], button");
}

#[test]
fn a_tag_input_adds_once_removes_and_takes_a_paste() {
    let app = App::new(|| {
        let tags = signal(Vec::<String>::new());
        column([
            tag_input(tags),
            text(move || format!("({})", tags.get().join("|"))),
        ])
    });
    let mut driver = app.driver(40, 2);
    driver.settle();
    driver.type_text(" rust ");
    driver.press("tab");
    driver.type_text("rust,tui");
    driver.press("enter");
    assert_eq!(
        driver.screen().plain()[1].trim_end(),
        "(rust|tui)",
        "{:?}",
        driver.screen().plain()
    );
    // Backspace: the typed text first, then the last tag.
    driver.type_text("x");
    driver.press("backspace backspace");
    assert_eq!(driver.screen().plain()[1].trim_end(), "(rust)");
    driver.send(intuituive::interact::Event::Paste("a,b,".into()));
    driver.settle();
    assert_eq!(driver.screen().plain()[1].trim_end(), "(rust|a|b)");
    // A click on a chip's ×.
    driver.click(6, 0);
    assert_eq!(driver.screen().plain()[1].trim_end(), "(a|b)");
    assert!(
        driver.accessibility()[0]
            .describe()
            .ends_with("text box: a, b")
    );
}

#[test]
fn a_tag_input_wraps_its_chips_and_grows_from_min_to_max() {
    let app = App::new(|| {
        let tags = signal(Vec::<String>::new());
        column([tag_input(tags).grow(1, 2), label("below")])
    });
    let mut driver = app.driver(16, 4);
    driver.settle();
    let rows = |driver: &Driver| -> Vec<String> {
        driver
            .screen()
            .plain()
            .iter()
            .map(|r| r.trim_end().to_string())
            .collect()
    };
    assert_eq!(rows(&driver)[1], "below");
    driver.type_text("alpha,beta,");
    // ` alpha × ` and ` beta × ` do not both fit in 16: two rows.
    assert_eq!(rows(&driver)[..3], [" alpha ×", " beta ×", "below"]);
    // Typed text that does not fit after the last chip goes under it, but
    // the input grows no taller than 2: the last two rows are shown.
    driver.type_text("gamma-delta");
    assert_eq!(rows(&driver)[..3], [" beta ×", "gamma-delta", "below"]);
    driver.press("enter");
    // The caret has a row of its own under the new chip.
    assert_eq!(rows(&driver)[..3], [" gamma-delta ×", "", "below"]);
    // A click on the `×` of a chip on a row below the first removes it.
    driver.click(13, 0);
    assert_eq!(rows(&driver)[..3], [" alpha ×", " beta ×", "below"]);
}

#[test]
fn a_tag_input_moves_its_caret_in_what_is_typed() {
    let app = App::new(|| {
        let tags = signal(Vec::<String>::new());
        column([
            tag_input(tags),
            text(move || format!("({})", tags.get().join("|"))),
        ])
    });
    let mut driver = app.driver(20, 2);
    driver.settle();
    driver.type_text("ac");
    driver.press("left");
    driver.type_text("b");
    // The caret is on `c`, reversed.
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("ab[reverse]c[/]"), "{styled}");
    driver.press("home");
    driver.type_text("x");
    driver.press("end");
    driver.type_text("y");
    driver.press("home delete right right backspace");
    assert_eq!(driver.screen().plain()[0].trim_end(), "acy");
    driver.press("enter");
    assert_eq!(driver.screen().plain()[1].trim_end(), "(acy)");
}
