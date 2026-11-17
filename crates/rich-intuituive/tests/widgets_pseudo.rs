//! The stylesheet's `:first-of-type`, `:last-of-type`, `:ansi`,
//! `:nocolor` and `:inline`, on a running app.

use intuituive::prelude::*;
use intuituive::{Palette, Stylesheet, rule};

#[test]
fn of_type_counts_only_siblings_of_the_same_kind() {
    let app = App::new(|| column([rule(), label("one"), label("two"), label("three"), rule()]))
        .stylesheet(
            "label:first-of-type { color: red; } label:last-of-type { color: blue; } \
         label:first-child { text-style: bold; }",
        );
    let mut driver = app.driver(10, 5);
    driver.settle();
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[red]one"), "{styled}");
    assert!(styled.contains("\ntwo"), "{styled}");
    assert!(styled.contains("[blue]three"), "{styled}");
    // No label is the first child: the rule is.
    assert!(!styled.contains("bold"), "{styled}");
}

#[test]
fn ansi_follows_the_theme_and_inline_the_app() {
    let css = ":ansi label { color: red; } label:inline { text-style: underline; }";
    let shown = |theme: Theme, inline: bool| {
        let mut app = App::new(|| column([label("hi")]))
            .stylesheet(css)
            .theme(theme);
        if inline {
            app = app.inline(1);
        }
        let mut driver = app.driver(4, 1);
        driver.settle();
        intuituive::testing::styled(&driver)
    };
    assert!(shown(Theme::dark(), false).contains("[red]hi"));
    let truecolour = Theme::from_palette(Palette::new(rich::Color::parse("#0178D4").unwrap()));
    assert!(!shown(truecolour, false).contains("red"));
    assert!(
        shown(Theme::dark(), true).contains("[underline red]hi"),
        "{}",
        shown(Theme::dark(), true)
    );
}

#[test]
fn the_new_states_parse_and_unknown_ones_still_fail() {
    for state in [
        ":ansi",
        ":nocolor",
        ":inline",
        ":first-of-type",
        ":last-of-type",
    ] {
        assert!(
            Stylesheet::parse(&format!("label{state} {{ color: red; }}")).is_ok(),
            "{state}"
        );
    }
    assert!(Stylesheet::parse("label:nth-of-type { color: red; }").is_err());
}
