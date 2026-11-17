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
