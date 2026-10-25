//! Controls that change in place: buttons and check boxes whose labels
//! read signals (`button_of`, `checkbox_of`), the three-state check box
//! (`checkbox_tri`, `CheckState`), and select fields and radio groups whose
//! options are known by key (`select_field_keyed`, `radio_keyed`).

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::{
    button_of, checkbox, checkbox_of, checkbox_tri, radio, radio_keyed, select_field,
    select_field_keyed, CheckState, Driver, SelectOption,
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

fn read(driver: &Driver) -> Vec<String> {
    driver
        .accessibility()
        .iter()
        .map(|n| n.describe())
        .collect()
}

// C1: labels that change in place.

/// A Play/Pause button, a count of presses, and `x` that flips the label
/// from outside.
fn player() -> App {
    App::new(|| {
        let playing = signal(false);
        let presses = signal(0);
        column([
            button_of(
                move || match playing.get() {
                    true => "Pause".into(),
                    false => "Play".into(),
                },
                move |_| {
                    presses.update(|n| *n += 1);
                    playing.update(|p| *p = !*p);
                },
            )
            .name("play")
            .fixed(1),
            button_of(|| "Other".into(), |_| {}).name("other").fixed(1),
            text!("{presses}").fixed(1),
        ])
        .on_key("x", move |_| playing.update(|p| *p = !*p))
    })
}

#[test]
fn a_button_label_changes_while_it_has_the_focus() {
    let mut driver = player().driver(12, 3);
    driver.settle();
    driver.assert_focused("play");
    let id = driver.accessibility()[0].id;
    assert_eq!(rows(&driver)[0], "    Play");
    driver.press("enter");
    assert_eq!(rows(&driver)[0], "   Pause");
    assert_eq!(rows(&driver)[2], "1");
    // From outside, too: the same node, still focused, named anew.
    driver.press("x");
    assert_eq!(rows(&driver)[0], "    Play");
    driver.assert_focused("play");
    let node = &driver.accessibility()[0];
    assert_eq!(node.id, id);
    assert!(node.focused);
    assert_eq!(node.describe(), "Play, button");
    driver.press("x");
    assert_eq!(read(&driver)[0], "Pause, button");
}

#[test]
fn a_button_label_changes_while_the_mouse_holds_it() {
    let mut driver = player().driver(12, 3);
    driver.settle();
    mouse(&mut driver, MouseKind::Down(Button::Left), 4, 0);
    assert_eq!(rows(&driver)[0], "   [Play]");
    // The label changes under the held mouse: still held.
    driver.press("x");
    assert_eq!(rows(&driver)[0], "  [Pause]");
    // Let go over it: one press, which flips it back.
    mouse(&mut driver, MouseKind::Up(Button::Left), 4, 0);
    assert_eq!(rows(&driver)[2], "1");
    assert_eq!(rows(&driver)[0], "    Play");
}

#[test]
fn a_wider_label_lays_the_row_out_again() {
    let app = App::new(|| {
        let long = signal(false);
        row([
            button_of(
                move || match long.get() {
                    true => "Much longer".into(),
                    false => "Go".into(),
                },
                |_| {},
            ),
            label("|"),
        ])
        .on_key("l", move |_| long.set(true))
    });
    let mut driver = app.driver(20, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], " Go |");
    driver.press("l");
    assert_eq!(rows(&driver)[0], " Much longer |");
}

#[test]
fn a_check_box_label_changes_and_keeps_its_state_and_focus() {
    let app = App::new(|| {
        let state = signal(CheckState::Off);
        let count = signal(3);
        column([
            checkbox_of(move || format!("{} files", count.get()), state).name("files"),
            checkbox("Other", signal(false)).name("other"),
        ])
        .on_key("+", move |_| count.update(|n| *n += 1))
    });
    let mut driver = app.driver(16, 2);
    driver.press("space +");
    assert_eq!(rows(&driver)[0], "[x] 4 files");
    driver.assert_focused("files");
    assert_eq!(read(&driver)[0], "4 files, check box, checked");
}

// C2: the three-state check box.

#[test]
fn three_states_are_drawn_and_described() {
    let app = App::new(|| {
        column([
            checkbox_tri("Off", signal(CheckState::Off)),
            checkbox_tri("On", signal(CheckState::On)),
            checkbox_tri("Some", signal(CheckState::Mixed)),
        ])
    });
    let mut driver = app.driver(12, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["[ ] Off", "[x] On", "[-] Some"]);
    assert_eq!(
        read(&driver),
        [
            "Off, check box, not checked",
            "On, check box, checked",
            "Some, check box, mixed"
        ]
    );
    let mixed = &driver.accessibility()[2];
    assert!(mixed.state.mixed);
    assert_eq!(mixed.state.checked, None);
    assert!(mixed
        .aria_attributes()
        .contains(&("aria-checked", "mixed".to_string())));
}

/// A three-state box and what it holds, with `m` to make it mixed and `d`
/// to disable it.
fn tri(next: Option<CheckState>) -> App {
    App::new(move || {
        let state = signal(CheckState::Mixed);
        let locked = signal(false);
        let mut boxed = checkbox_tri("All", state).disabled_when(move || locked.get());
        if let Some(next) = next {
            boxed = boxed.on_mixed(next);
        }
        column([boxed.fixed(1), text!("{:?}", state.get()).fixed(1)])
            .on_key("m", move |_| state.set(CheckState::Mixed))
            .on_key("d", move |_| locked.update(|l| *l = !*l))
    })
}

#[test]
fn keys_and_clicks_move_between_the_states() {
    let mut driver = tri(None).driver(12, 2);
    driver.settle();
    assert_eq!(rows(&driver)[0], "[-] All");
    // Mixed goes on, then off, then on.
    driver.press("space");
    assert_eq!(rows(&driver)[1], "On");
    driver.press("enter");
    assert_eq!(rows(&driver)[1], "Off");
    click(&mut driver, 1, 0);
    assert_eq!(rows(&driver)[1], "On");
    // Made mixed from outside while focused: drawn so.
    driver.press("m");
    assert_eq!(rows(&driver)[0], "[-] All");
    assert_eq!(read(&driver)[0], "All, check box, mixed");
    click(&mut driver, 1, 0);
    assert_eq!(rows(&driver)[0], "[x] All");
}

#[test]
fn on_mixed_says_where_a_press_goes_from_mixed() {
    let mut driver = tri(Some(CheckState::Off)).driver(12, 2);
    driver.press("space");
    assert_eq!(rows(&driver), ["[ ] All", "Off"]);
    driver.press("space");
    assert_eq!(rows(&driver)[1], "On");
}

#[test]
fn a_disabled_mixed_box_stays_mixed() {
    let mut driver = tri(None).driver(12, 2);
    driver.press("d space enter");
    click(&mut driver, 1, 0);
    assert_eq!(rows(&driver), ["[-] All", "Mixed"]);
    assert_eq!(read(&driver)[0], "All, check box, mixed, disabled");
}

// C3: keyed options.

#[derive(Clone, Copy, Debug, PartialEq)]
enum Dev {
    Usb,
    Hdmi,
    Jack,
}

/// Three devices, reordered by `r`, the jack removed by `x`, HDMI renamed
/// by `n`, USB disabled unless `u` plugs it in.
fn devices(radio_group: bool) -> (App, impl Fn(&Driver) -> String, impl Fn(&Driver) -> String) {
    let app = App::new(move || {
        let chosen = signal(Some(Dev::Hdmi));
        let reversed = signal(false);
        let removed = signal(false);
        let renamed = signal(false);
        let plugged = signal(false);
        let options = move || {
            let usb = SelectOption::new(Dev::Usb, "USB");
            let mut all = vec![
                match plugged.get() {
                    true => usb,
                    false => usb.disabled("unplugged"),
                },
                SelectOption::new(
                    Dev::Hdmi,
                    match renamed.get() {
                        true => "TV",
                        false => "HDMI",
                    },
                ),
            ];
            if !removed.get() {
                all.push(SelectOption::new(Dev::Jack, "Jack"));
            }
            if reversed.get() {
                all.reverse();
            }
            all
        };
        let field = match radio_group {
            true => radio_keyed(options, chosen),
            false => select_field_keyed(options, chosen),
        };
        let height = if radio_group { 3 } else { 1 };
        column([
            field.label("Output").name("field").fixed(height),
            text!("{:?}", chosen.get()).fixed(1),
        ])
        .on_key("r", move |_| reversed.update(|r| *r = !*r))
        .on_key("x", move |_| removed.set(true))
        .on_key("n", move |_| renamed.set(true))
        .on_key("u", move |_| plugged.set(true))
    });
    let height = if radio_group { 3 } else { 1 };
    let chosen = move |driver: &Driver| rows(driver)[height].clone();
    let shown = move |driver: &Driver| driver.accessibility()[0].describe();
    (app, chosen, shown)
}

#[test]
fn a_reorder_keeps_the_selection() {
    for radio_group in [false, true] {
        let (app, chosen, shown) = devices(radio_group);
        let mut driver = app.driver(30, 8);
        driver.settle();
        assert_eq!(chosen(&driver), "Some(Hdmi)");
        driver.press("r");
        assert_eq!(chosen(&driver), "Some(Hdmi)");
        let read = shown(&driver);
        assert!(read.contains("2 of 3: HDMI"), "{read}");
        // Now Jack, HDMI, USB; USB is disabled.
        driver.press("up");
        assert_eq!(chosen(&driver), "Some(Jack)");
        driver.press("down");
        assert_eq!(chosen(&driver), "Some(Hdmi)");
        // Down passes USB by: a field stays at its last, a group goes round.
        driver.press("down");
        let after = if radio_group {
            "Some(Jack)"
        } else {
            "Some(Hdmi)"
        };
        assert_eq!(chosen(&driver), after);
    }
}

#[test]
fn a_relabel_keeps_the_key() {
    for radio_group in [false, true] {
        let (app, chosen, shown) = devices(radio_group);
        let mut driver = app.driver(30, 8);
        driver.press("n");
        assert_eq!(chosen(&driver), "Some(Hdmi)");
        assert!(shown(&driver).contains(": TV"));
        assert!(rows(&driver).iter().any(|row| row.contains("TV")));
    }
}

#[test]
fn a_removed_choice_leaves_nothing_chosen() {
    for radio_group in [false, true] {
        let (app, chosen, shown) = devices(radio_group);
        let mut driver = app.driver(30, 8);
        driver.press("down");
        assert_eq!(chosen(&driver), "Some(Jack)");
        driver.press("x");
        assert_eq!(chosen(&driver), "None");
        if radio_group {
            assert_eq!(shown(&driver), "Output, radio group: none");
            assert!(!rows(&driver).iter().any(|row| row.contains('•')));
        } else {
            assert!(rows(&driver)[0].starts_with("(none) "));
            assert_eq!(shown(&driver), "Output, combo box: none, collapsed");
        }
        // Down from nothing: the first that may be chosen.
        driver.press("down");
        assert_eq!(chosen(&driver), "Some(Hdmi)");
    }
}

#[test]
fn a_disabled_option_cannot_be_chosen_by_keys() {
    for radio_group in [false, true] {
        let (app, chosen, _) = devices(radio_group);
        let mut driver = app.driver(30, 8);
        // Up from HDMI: a field stays, as USB above it is disabled; a
        // group goes round to the jack.
        driver.press("up");
        let after = if radio_group {
            "Some(Jack)"
        } else {
            "Some(Hdmi)"
        };
        assert_eq!(chosen(&driver), after);
        if radio_group {
            // Home: the first that may be chosen.
            driver.press("home");
            assert_eq!(chosen(&driver), "Some(Hdmi)");
        }
        // Typing its name passes it by (and the key goes on up, here to
        // the binding that plugs it in).
        driver.press("u");
        assert_eq!(chosen(&driver), "Some(Hdmi)");
    }
}

#[test]
fn a_disabled_radio_option_cannot_be_clicked() {
    let (app, chosen, _) = devices(true);
    let mut driver = app.driver(30, 8);
    driver.settle();
    assert_eq!(rows(&driver)[0], "( ) USB (unplugged)");
    click(&mut driver, 1, 0);
    assert_eq!(chosen(&driver), "Some(Hdmi)");
    click(&mut driver, 1, 2);
    assert_eq!(chosen(&driver), "Some(Jack)");
}

#[test]
fn a_disabled_menu_option_cannot_be_chosen() {
    let (app, chosen, _) = devices(false);
    let mut driver = app.driver(30, 8);
    driver.settle();
    // Open the menu: USB is listed, greyed with why.
    driver.press("enter");
    let menu = rows(&driver).join("\n");
    assert!(menu.contains("USB") && menu.contains("unplugged"), "{menu}");
    // The arrows pass it by: up from HDMI goes round to the jack.
    driver.press("up enter");
    assert_eq!(chosen(&driver), "Some(Jack)");
    // A click on it does nothing; the menu stays open.
    driver.press("enter");
    let at = rows(&driver)
        .iter()
        .position(|row| row.contains("USB"))
        .expect("the menu lists USB") as u16;
    click(&mut driver, 4, at);
    assert!(rows(&driver).join("\n").contains("unplugged"));
    // Typing its first letter in the menu passes it by.
    driver.press("u enter");
    assert_eq!(chosen(&driver), "Some(Jack)");
}

#[test]
fn a_typed_letter_reaches_an_option_once_it_is_enabled() {
    let (app, chosen, _) = devices(false);
    let mut driver = app.driver(30, 8);
    // `u` is bound on the column to plug USB in, but the field uses
    // letters first: it does nothing while USB is disabled, so it bubbles.
    driver.press("u");
    assert_eq!(chosen(&driver), "Some(Hdmi)");
    driver.press("u");
    assert_eq!(chosen(&driver), "Some(Usb)");
}

#[test]
fn the_string_forms_still_work() {
    let app = App::new(|| {
        let theme = signal(1usize);
        let size = signal(0usize);
        column([
            select_field(|| vec!["Dark".into(), "Light".into()], theme).fixed(1),
            radio(|| vec!["S".into(), "M".into()], size).fixed(2),
            text!("{theme} {size}").fixed(1),
        ])
    });
    let mut driver = app.driver(20, 4);
    driver.press("up tab down");
    assert_eq!(rows(&driver)[3], "0 1");
}

#[test]
fn an_open_menu_chooses_the_option_it_shows_after_a_reorder() {
    let app = App::new(|| {
        let chosen = signal(Some(Dev::Hdmi));
        let reversed = signal(false);
        // The list turns round a second in, under the open menu.
        intuituive::after(std::time::Duration::from_secs(1), move |_| {
            reversed.set(true)
        });
        let options = move || {
            let mut all = vec![
                SelectOption::new(Dev::Usb, "USB").disabled("unplugged"),
                SelectOption::new(Dev::Hdmi, "HDMI"),
                SelectOption::new(Dev::Jack, "Jack"),
            ];
            if reversed.get() {
                all.reverse();
            }
            all
        };
        column([
            select_field_keyed(options, chosen).fixed(1),
            text!("{:?}", chosen.get()).fixed(1),
        ])
    });
    let mut driver = app.driver(30, 8);
    driver.settle();
    // The menu shows USB, HDMI, Jack; down moves from HDMI to Jack.
    driver.press("enter");
    driver.press("down");
    // Now Jack, HDMI, USB: the menu's third item is USB by index.
    driver.advance(std::time::Duration::from_millis(1100));
    driver.press("enter");
    assert_eq!(rows(&driver)[1], "Some(Jack)");
}
