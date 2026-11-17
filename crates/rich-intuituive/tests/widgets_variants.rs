//! Button variants Success and Warning in every built-in theme, and the
//! command palette's Bell.

use intuituive::prelude::*;
use intuituive::{ButtonVariant, Palette, SystemCommands, button};

fn faces(theme: Theme) -> String {
    let app = App::new(|| {
        column([
            button("Done", |_| {})
                .variant(ButtonVariant::Success)
                .fixed(1),
            button("Careful", |_| {})
                .variant(ButtonVariant::Warning)
                .fixed(1),
            label("x").focusable().autofocus(),
        ])
    })
    .theme(theme);
    let mut driver = app.driver(12, 3);
    driver.settle();
    intuituive::testing::styled(&driver)
}

#[test]
fn success_and_warning_buttons_take_their_own_faces() {
    let dark = faces(Theme::dark());
    assert!(dark.contains("[black on green3]    Done    [/]"), "{dark}");
    assert!(dark.contains("[black on yellow]  Careful   [/]"), "{dark}");
    let light = faces(Theme::light());
    assert!(light.contains("[white on green4]"), "{light}");
    assert!(light.contains("[black on dark_orange]"), "{light}");
    let mono = faces(Theme::mono());
    assert!(mono.contains("[bold reverse]    Done"), "{mono}");
}

#[test]
fn a_palette_theme_puts_them_on_its_success_and_warning_colours() {
    let palette = Palette::new(rich::Color::parse("#0178D4").unwrap())
        .success(rich::Color::parse("#4EBF71").unwrap())
        .warning(rich::Color::parse("#ffa62b").unwrap())
        .dark(true);
    let shown = faces(Theme::from_palette(palette));
    assert!(shown.contains("on #4ebf71]"), "{shown}");
    // The warning colour as the palette generates it.
    assert!(shown.contains("on #fea62b]"), "{shown}");
}

#[test]
fn the_palettes_bell_is_off_until_turned_on_and_then_rings() {
    let shown = |commands: SystemCommands| {
        let app = App::new(|| label("hi"))
            .system_commands(commands)
            .palette_key("ctrl+p");
        let mut driver = app.driver(40, 12);
        driver.press("ctrl+p");
        driver.type_text("bell");
        let shown = driver.screen().plain().join("\n");
        (driver, shown)
    };
    // Off by default, as Textual's (where it is the example of a command of
    // your own).
    let (_, plain) = shown(SystemCommands::default());
    assert!(!plain.contains("Ring the b"), "{plain}");
    let (mut driver, on) = shown(SystemCommands::default().with("bell"));
    assert!(on.contains("app Bell  Ring the b"), "{on}");
    driver.press("enter");
    assert_eq!(driver.take_bells(), 1);
}
