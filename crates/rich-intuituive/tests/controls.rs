//! The stock form controls: button, checkbox, radio group, switch, slider,
//! select field, spinner, `loading_when` and collapsible. Keys, the mouse,
//! what assistive technology is told, and their theme styles.

use std::time::Duration;

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::{
    button, checkbox, collapsible, radio, select_field, slider, spinner, switch_toggle,
    ButtonVariant, Driver,
};

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn mouse(driver: &mut Driver, kind: MouseKind, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(kind, column, row)));
    driver.settle();
}

fn click(driver: &mut Driver, column: u16, row: u16) {
    mouse(driver, MouseKind::Down(Button::Left), column, row);
    mouse(driver, MouseKind::Up(Button::Left), column, row);
}

/// The definition of the style at a cell.
fn style_at(driver: &Driver, column: u16, row: u16) -> String {
    let screen = driver.screen();
    screen
        .style(screen.cell(column, row).style)
        .map(|s| s.definition())
        .unwrap_or_default()
}

/// What assistive technology reads for each node, in order.
fn read(driver: &Driver) -> Vec<String> {
    driver
        .accessibility()
        .iter()
        .map(|n| n.describe())
        .collect()
}

/// A button that counts its presses, beside another, over the count.
fn buttons() -> App {
    App::new(|| {
        let presses = signal(0);
        let locked = signal(false);
        column([
            row([
                button("Save", move |_| presses.update(|n| *n += 1))
                    .variant(ButtonVariant::Primary)
                    .disabled_when(move || locked.get()),
                button("Other", |_| {}),
            ])
            .gap(1)
            .fixed(1),
            text!("{presses}").fixed(1),
        ])
        .on_key("l", move |_| locked.update(|l| *l = !*l))
    })
}

#[test]
fn a_button_is_pressed_by_enter_space_and_a_click() {
    let mut driver = buttons().driver(20, 2);
    driver.settle();
    assert_eq!(rows(&driver)[0], " Save   Other");
    driver.press("enter space");
    assert_eq!(rows(&driver)[1], "2");
    // Held: pressed, and not counted until it is let go over it.
    mouse(&mut driver, MouseKind::Down(Button::Left), 2, 0);
    assert_eq!(rows(&driver)[0], "[Save]  Other");
    assert_eq!(rows(&driver)[1], "2");
    mouse(&mut driver, MouseKind::Up(Button::Left), 2, 0);
    assert_eq!(rows(&driver)[0], " Save   Other");
    assert_eq!(rows(&driver)[1], "3");
    // Let go elsewhere: no press.
    mouse(&mut driver, MouseKind::Down(Button::Left), 2, 0);
    mouse(&mut driver, MouseKind::Up(Button::Left), 2, 1);
    assert_eq!(rows(&driver)[1], "3");
}

#[test]
fn a_disabled_button_is_greyed_out_and_does_nothing() {
    let mut driver = buttons().driver(20, 2);
    driver.press("l");
    // The focus leaves it: the other button has it.
    driver.press("enter");
    click(&mut driver, 2, 0);
    assert_eq!(rows(&driver)[1], "0");
    assert!(
        style_at(&driver, 1, 0).contains("dim"),
        "{}",
        style_at(&driver, 1, 0)
    );
    assert_eq!(read(&driver)[0], "Save, button, disabled");
    driver.press("l shift+tab enter");
    assert_eq!(rows(&driver)[1], "1");
    assert_eq!(read(&driver)[0], "Save, button");
}

#[test]
fn a_buttons_face_and_focus_come_from_the_theme() {
    let mut driver = buttons().driver(20, 2);
    driver.settle();
    // Save, primary, has the focus: the dark theme's primary face with
    // its focus look over it; Other its plain face.
    assert_eq!(style_at(&driver, 1, 0), "bold reverse black on cyan");
    assert_eq!(style_at(&driver, 8, 0), "white on grey30");
    driver.press("tab");
    assert_eq!(style_at(&driver, 1, 0), "black on cyan");
    assert_eq!(style_at(&driver, 8, 0), "bold reverse white on grey30");
    // A theme style given to one node restyles it alone.
    let app = App::new(|| {
        row([
            label("x").focusable().fixed(1),
            button("Go", |_| {}).theme_styles([("button", "red on white")]),
        ])
    });
    let mut driver = app.driver(8, 1);
    driver.settle();
    assert_eq!(style_at(&driver, 3, 0), "red on white");
}

#[test]
fn a_tall_button_draws_the_edges_the_theme_names() {
    let app = App::new(|| {
        row([
            label("x").focusable().fixed(1),
            button("Go", |_| {})
                .theme_styles([
                    ("button", "white on #802020"),
                    ("button.light", "#c04040"),
                    ("button.shade", "#401010"),
                    ("button.focus", "on #b03030"),
                ])
                .fixed(6),
        ])
    });
    let mut driver = app.driver(7, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["x▔▔▔▔▔▔", "   Go", " ▁▁▁▁▁▁"]);
    assert_eq!(style_at(&driver, 1, 0), "#c04040 on #802020");
    assert_eq!(style_at(&driver, 1, 2), "#401010 on #802020");
    // Held, the edges swap: pressed in.
    mouse(&mut driver, MouseKind::Down(Button::Left), 3, 1);
    assert_eq!(style_at(&driver, 1, 0), "#401010 on #b03030");
    assert_eq!(rows(&driver)[1], "  [Go]");
    // Without edge colours, a tall button is its face alone.
    let app = App::new(|| button("Go", |_| {}).fixed(3));
    let mut driver = app.driver(6, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["", "  Go", ""]);
}

#[test]
fn a_checkbox_and_a_switch_follow_their_signals() {
    let app = App::new(|| {
        let wrap = signal(false);
        let dark = signal(true);
        column([
            checkbox("Wrap lines", wrap).fixed(1),
            switch_toggle("Dark theme", dark).fixed(1),
            text!("{wrap} {dark}").fixed(1),
        ])
    });
    let mut driver = app.driver(20, 3);
    driver.settle();
    assert_eq!(
        rows(&driver),
        ["[ ] Wrap lines", "──● Dark theme", "false true"]
    );
    assert_eq!(
        read(&driver)[..2],
        [
            "Wrap lines, check box, not checked",
            "Dark theme, switch, checked"
        ]
    );
    // Focused, its text is reversed.
    assert!(style_at(&driver, 5, 0).contains("reverse"));
    assert!(!style_at(&driver, 5, 0 + 1).contains("reverse"));
    driver.press("space");
    assert_eq!(rows(&driver)[0], "[x] Wrap lines");
    driver.press("enter tab left");
    assert_eq!(rows(&driver)[1], "●── Dark theme");
    driver.press("left right");
    assert_eq!(rows(&driver)[2], "false true");
    // A click on either.
    click(&mut driver, 6, 0);
    click(&mut driver, 0, 1);
    assert_eq!(rows(&driver)[2], "true false");
    assert_eq!(read(&driver)[1], "Dark theme, switch, not checked");
}

#[test]
fn in_text_mode_the_controls_say_what_they_are() {
    let app = App::new(|| {
        column([
            switch_toggle("Dark", signal(true)).fixed(1),
            button("Save", |_| {}).fixed(1),
            collapsible("More", signal(false), label("hidden")),
        ])
    })
    .accessible(true);
    let mut driver = app.driver(12, 4);
    driver.settle();
    assert_eq!(rows(&driver)[..3], ["[on ] Dark", "   [Save]", "[+] More"]);
}

/// A radio group over the option it holds.
fn sizes() -> App {
    App::new(|| {
        let size = signal(1usize);
        column([
            radio(
                || vec!["Small".into(), "Medium".into(), "Large".into()],
                size,
            )
            .label("Size")
            .fixed(3),
            text!("{size}").fixed(1),
        ])
    })
}

#[test]
fn the_arrows_move_the_choice_within_a_radio_group() {
    let mut driver = sizes().driver(12, 4);
    driver.settle();
    assert_eq!(rows(&driver)[..3], ["( ) Small", "(•) Medium", "( ) Large"]);
    assert_eq!(
        read(&driver)[0],
        "Size, radio group, 2 of 3: Medium, checked"
    );
    driver.press("down");
    assert_eq!(rows(&driver)[3], "2");
    // Round from the last to the first, and back.
    driver.press("down");
    assert_eq!(rows(&driver)[3], "0");
    driver.press("up");
    assert_eq!(rows(&driver)[3], "2");
    driver.press("home");
    assert_eq!(rows(&driver)[3], "0");
    driver.press("end left");
    assert_eq!(rows(&driver)[3], "1");
    // A click chooses an option.
    click(&mut driver, 3, 2);
    assert_eq!(rows(&driver)[3], "2");
    assert_eq!(
        read(&driver)[0],
        "Size, radio group, 3 of 3: Large, checked"
    );
    // The chosen option is the one marked with the focus.
    assert!(style_at(&driver, 4, 2).contains("reverse"));
    assert!(!style_at(&driver, 4, 0).contains("reverse"));
}

/// A slider eleven cells wide over its value.
fn volume(start: f64) -> App {
    App::new(move || {
        let volume = signal(start);
        column([
            slider(volume, 0.0..=100.0, 10.0).label("Volume").fixed(1),
            text!("{volume}").fixed(1),
        ])
    })
}

#[test]
fn the_keys_move_a_slider_in_steps_within_its_range() {
    let mut driver = volume(50.0).driver(11, 2);
    driver.settle();
    assert_eq!(rows(&driver)[0], "━━━━━●─────");
    assert_eq!(read(&driver)[0], "Volume, slider: 50");
    driver.press("right up");
    assert_eq!(rows(&driver)[1], "70");
    driver.press("left down down");
    assert_eq!(rows(&driver)[1], "40");
    driver.press("end right");
    assert_eq!(rows(&driver)[1], "100");
    driver.press("pagedown");
    assert_eq!(rows(&driver)[1], "90");
    driver.press("home left");
    assert_eq!(rows(&driver)[1], "0");
    assert_eq!(rows(&driver)[0], "●──────────");
    driver.press("pageup");
    assert_eq!(read(&driver)[0], "Volume, slider: 10");
    let attributes = driver.accessibility()[0].aria_attributes();
    assert!(attributes.contains(&("aria-valuenow", "10".to_string())));
}

#[test]
fn a_click_and_a_drag_move_a_sliders_knob() {
    let mut driver = volume(0.0).driver(11, 2);
    driver.settle();
    mouse(&mut driver, MouseKind::Down(Button::Left), 3, 0);
    assert_eq!(rows(&driver)[1], "30");
    // Dragged on, past the end: captured, it keeps following.
    mouse(&mut driver, MouseKind::Drag(Button::Left), 7, 0);
    assert_eq!(rows(&driver)[1], "70");
    mouse(&mut driver, MouseKind::Drag(Button::Left), 15, 1);
    assert_eq!(rows(&driver)[1], "100");
    mouse(&mut driver, MouseKind::Up(Button::Left), 15, 1);
    mouse(&mut driver, MouseKind::ScrollDown, 4, 0);
    assert_eq!(rows(&driver)[1], "90");
}

#[test]
fn a_slider_reads_its_step_s_decimals() {
    let app = App::new(|| slider(signal(0.25), 0.0..=1.0, 0.05).label("Gain"));
    let mut driver = app.driver(10, 1);
    driver.press("right");
    assert_eq!(read(&driver)[0], "Gain, slider: 0.30");
}

/// A select field over the option it holds.
fn themes() -> App {
    App::new(|| {
        let theme = signal(0usize);
        let names = || vec!["Dark".into(), "Light".into(), "Blue".into(), "Black".into()];
        column([
            select_field(names, theme).label("Theme").fixed(1),
            text!("{theme}").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_select_field_opens_a_menu_under_it_at_its_choice() {
    let mut driver = themes().driver(14, 8);
    driver.settle();
    assert_eq!(rows(&driver)[0], "Dark         ▾");
    assert_eq!(
        read(&driver)[0],
        "Theme, combo box, 1 of 4: Dark, collapsed"
    );
    driver.press("down");
    assert_eq!(rows(&driver)[1], "1");
    click(&mut driver, 3, 0);
    let shown = rows(&driver);
    assert!(shown[1].starts_with("╭"), "{shown:?}");
    assert!(shown[3].contains("Light"), "{shown:?}");
    // The menu opens at the choice: down goes to the next.
    driver.press("down enter");
    assert_eq!(rows(&driver)[0], "Blue         ▾");
    assert_eq!(rows(&driver)[1], "2");
    // Esc closes it with nothing chosen, and the field says it is closed.
    driver.press("enter");
    assert!(read(&driver).iter().any(|line| line.contains("menu")));
    driver.press("esc");
    assert_eq!(rows(&driver)[1], "2");
    assert_eq!(
        read(&driver)[0],
        "Theme, combo box, 3 of 4: Blue, collapsed"
    );
}

#[test]
fn typing_jumps_to_the_option_it_starts() {
    let mut driver = themes().driver(14, 8);
    driver.settle();
    // Closed: `b` goes to Blue, `b` again round to Black, `l` to Light.
    driver.press("b");
    assert_eq!(rows(&driver)[1], "2");
    driver.press("b");
    assert_eq!(rows(&driver)[1], "3");
    driver.update(Duration::from_secs(2));
    driver.press("l");
    assert_eq!(rows(&driver)[1], "1");
    // Letters typed together: `bla` is Black.
    driver.update(Duration::from_secs(4));
    driver.press("b l a");
    assert_eq!(rows(&driver)[1], "3");
    // In the open menu too.
    driver.update(Duration::from_secs(6));
    driver.press("enter d enter");
    assert_eq!(rows(&driver)[1], "0");
    // A letter no option starts goes on up: q quits.
    driver.press("q");
    assert!(driver.is_done());
}

#[test]
fn a_spinner_turns_only_while_it_spins() {
    let app = App::new(|| {
        let busy = signal(true);
        row([
            spinner("line").spinning_when(move || busy.get()).fixed(2),
            label("Working"),
        ])
        .on_key("s", move |_| busy.update(|b| *b = !*b))
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "- Working");
    // Asleep until the next frame is due (`line` turns every 130 ms).
    assert_eq!(driver.timeout(Duration::ZERO), Duration::from_millis(50));
    driver.advance(Duration::from_millis(130));
    assert_eq!(rows(&driver)[0], "\\ Working");
    driver.advance(Duration::from_millis(260));
    assert_eq!(rows(&driver)[0], "/ Working");
    // The glyph is decoration: only the text is read.
    assert_eq!(read(&driver), ["Working"]);
    // Stopped: blank, and nothing more to draw however long it waits.
    driver.press("s");
    assert_eq!(rows(&driver)[0], "  Working");
    driver.advance(Duration::from_millis(130));
    driver.update(Duration::from_secs(5));
    assert!(!driver.needs_render());
    // Started again, it turns again.
    driver.press("s");
    let now = rows(&driver)[0].clone();
    driver.advance(Duration::from_millis(130));
    assert_ne!(rows(&driver)[0], now);
}

#[test]
fn an_unknown_spinner_is_dots() {
    let app = App::new(|| spinner("no such spinner"));
    let mut driver = app.driver(2, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "⠋");
}

#[test]
fn loading_when_dims_a_node_under_a_spinner_and_marks_it_busy() {
    let app = App::new(|| {
        let loading = signal(true);
        column([list(
            || vec!["alpha".into(), "beta".into(), "gamma".into()],
            signal(0),
        )
        .label("Files")
        .loading_when(move || loading.get())
        .fixed(3)])
        .on_key("l", move |_| loading.update(|l| *l = !*l))
    });
    let mut driver = app.driver(9, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["alpha", "bet ⠋", "gamma"]);
    assert!(style_at(&driver, 0, 2).contains("dim"));
    assert_eq!(
        read(&driver)[0],
        "Files, list, 1 of 3: alpha, selected, busy"
    );
    driver.advance(Duration::from_millis(80));
    assert_eq!(rows(&driver)[1], "bet ⠙");
    driver.press("l");
    assert_eq!(rows(&driver), ["alpha", "beta", "gamma"]);
    assert!(!style_at(&driver, 0, 2).contains("dim"));
    assert_eq!(read(&driver)[0], "Files, list, 1 of 3: alpha, selected");
}

#[test]
fn a_collapsible_shows_its_child_while_open() {
    let app = App::new(|| {
        let open = signal(false);
        column([
            collapsible("Advanced", open, checkbox("Cache", signal(true))),
            button("Done", |_| {}).fixed(1),
        ])
    });
    let mut driver = app.driver(12, 3);
    driver.settle();
    assert_eq!(rows(&driver)[..2], ["▸ Advanced", "    Done"]);
    // Closed, the child is out of the Tab order: Tab goes to Done.
    driver.press("tab");
    assert_eq!(read(&driver)[1], "Done, button");
    driver.press("shift+tab right");
    assert_eq!(rows(&driver)[..3], ["▾ Advanced", "[x] Cache", "    Done"]);
    assert_eq!(read(&driver)[0], "Advanced, button, expanded");
    driver.press("tab");
    driver.press("space");
    assert_eq!(rows(&driver)[1], "[ ] Cache");
    // A click on the header closes it.
    click(&mut driver, 3, 0);
    assert_eq!(rows(&driver)[..2], ["▸ Advanced", "    Done"]);
    assert_eq!(read(&driver)[0], "Advanced, button, collapsed");
}

#[path = "../examples/controls.rs"]
#[allow(dead_code)]
mod controls_example;

#[test]
fn the_controls_example_is_a_settings_form() {
    let mut driver = controls_example::controls_app().driver(60, 20);
    driver.settle();
    intuituive::assert_screen!(driver, "controls_form");
}

#[test]
fn the_settings_form_changes_saves_and_reads_out() {
    let mut driver = controls_example::controls_app().driver(60, 20);
    driver.settle();
    // The theme by its first letter, two points bigger, the boxes flipped,
    // tabs for spaces, and Advanced open.
    driver.press("s tab right right tab space tab space tab down tab enter");
    intuituive::assert_screen!(driver, "controls_form_changed");
    assert_eq!(
        read(&driver),
        [
            "Settings, region",
            "Appearance",
            "Theme, combo box, 3 of 4: Solarized, collapsed",
            "Font size, slider: 16",
            "16pt",
            "Show line numbers, check box, not checked",
            "Dark title bar, switch, checked",
            "Editor",
            "Indent with, radio group, 2 of 2: Tabs, checked",
            "Advanced, button, expanded",
            "Send anonymous usage data, check box, not checked",
            "Autosave every, slider: 30",
            "every 30s",
            "Not saved yet",
            "Save, button",
            "Reset, button",
            "Delete profile, button",
            "tab moves · enter opens · space ticks · ←/→ slide · q quits",
        ]
    );
    // Save: busy for a second, then saved.
    driver.press("tab tab tab enter");
    assert!(rows(&driver).iter().any(|r| r.contains("Saving…")));
    assert!(driver.accessibility()[0].state.busy);
    driver.advance(Duration::from_secs(1));
    assert!(rows(&driver).iter().any(|r| r.contains("Saved")));
    assert!(!driver.accessibility()[0].state.busy);
    // Reset puts it all back.
    driver.press("tab enter");
    assert!(rows(&driver)[2].contains("Dark "), "{:?}", rows(&driver));
}
