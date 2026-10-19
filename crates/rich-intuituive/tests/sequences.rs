//! Keys beyond one-key bindings: key sequences and the which-key pop-up,
//! `on_any_key`, `previews_keys`, a pop-up's inner bindings, and how the
//! palette and the help show them.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{Driver, Theme};

fn row(driver: &Driver, y: usize) -> String {
    driver.screen().plain()[y].trim_end().to_string()
}

/// A node that says what ran, with `g g`, `g h` and `j`.
fn files() -> App {
    App::new(|| {
        let ran = signal(String::from("-"));
        text!("{ran}")
            .bind_sequence("g g", "Go to the top", move |_| ran.set("top".into()))
            .bind_sequence("g h", "Go home", move |_| ran.set("home".into()))
            .bind_sequence("\",\" m t", "Sort by mtime", move |_| {
                ran.set("mtime".into())
            })
            .bind("j", "Next file", move |_| ran.set("down".into()))
    })
}

#[test]
fn a_sequence_runs_once_its_last_key_is_pressed() {
    let mut driver = files().driver(30, 6);
    driver.press("g");
    assert_eq!(row(&driver, 0), "-");
    driver.press("h");
    assert_eq!(row(&driver, 0), "home");
    driver.press("\",\" m t");
    assert_eq!(row(&driver, 0), "mtime");
}

#[test]
fn another_key_cancels_the_sequence_and_is_handled_as_usual() {
    let mut driver = files().driver(30, 6);
    driver.press("g j");
    assert_eq!(row(&driver, 0), "down");
    // The sequence is over: `g` then `g` starts afresh.
    driver.press("g g");
    assert_eq!(row(&driver, 0), "top");
    // A key that goes on with nothing, partway through a longer one.
    driver.press("\",\" j");
    assert_eq!(row(&driver, 0), "down");
}

#[test]
fn escape_cancels_the_sequence_and_goes_no_further() {
    let app = App::new(|| {
        let ran = signal(String::from("-"));
        text!("{ran}")
            .on_sequence("g g", move |_| ran.set("top".into()))
            .on_key("esc", move |_| ran.set("escape".into()))
    });
    let mut driver = app.driver(20, 2);
    driver.press("g esc");
    assert_eq!(row(&driver, 0), "-");
    driver.press("g");
    driver.press("g");
    assert_eq!(row(&driver, 0), "top");
    driver.press("esc");
    assert_eq!(row(&driver, 0), "escape");
}

#[test]
fn a_deeper_binding_of_the_first_key_wins_over_an_ancestors_sequence() {
    let app = App::new(|| {
        let ran = signal(String::from("-"));
        column([
            label("list")
                .focusable()
                .on_key("g", move |_| ran.set("list g".into())),
            text!("{ran}"),
        ])
        .on_sequence("g g", move |_| ran.set("top".into()))
    });
    let mut driver = app.driver(20, 2);
    driver.press("g g");
    assert_eq!(row(&driver, 1), "list g");
}

#[test]
fn a_previewing_container_starts_its_sequence_before_the_focused_node() {
    let app = App::new(|| {
        let ran = signal(String::from("-"));
        column([
            label("list")
                .focusable()
                .on_key("g", move |_| ran.set("list g".into())),
            text!("{ran}"),
        ])
        .previews_keys()
        .on_sequence("g g", move |_| ran.set("top".into()))
    });
    let mut driver = app.driver(20, 2);
    driver.press("g g");
    assert_eq!(row(&driver, 1), "top");
}

#[test]
fn which_key_lists_what_can_follow_and_goes_when_the_sequence_ends() {
    let mut driver = files().which_key(true).driver(60, 8);
    driver.settle();
    let before = driver.screen().plain();
    driver.press("g");
    let rows = driver.screen().plain();
    // Bottom, a cell in from the edges: a box of one row of entries.
    assert!(
        rows[4].starts_with(" ╭") && rows[4].ends_with("╮ "),
        "{rows:#?}"
    );
    assert!(rows[5].contains("g  Go to the top"), "{rows:#?}");
    assert!(rows[5].contains("h  Go home"), "{rows:#?}");
    assert!(rows[6].starts_with(" ╰"), "{rows:#?}");
    assert!(rows[7].trim().is_empty());
    driver.press("h");
    assert_eq!(row(&driver, 0), "home");
    let after = driver.screen().plain();
    assert_eq!(after[4..], before[4..]);
    // Three levels: what is left of the keys shows.
    driver.press("\",\"");
    assert!(driver.screen().plain()[5].contains("m t  Sort by mtime"));
    driver.press("esc");
    assert!(!driver.screen().plain()[5].contains("Sort by mtime"));
}

#[test]
fn which_key_fills_three_columns_top_to_bottom() {
    let app = App::new(|| {
        let mut node = label("x");
        for (key, what) in [("a", "one"), ("b", "two"), ("c", "three"), ("d", "four")] {
            node = node.bind_sequence(&format!("g {key}"), what, |_| {});
        }
        node
    })
    .which_key(true);
    let mut driver = app.driver(62, 8);
    driver.press("g");
    let rows = driver.screen().plain();
    // Two rows, three columns of twenty cells: a b | c d.
    assert!(
        rows[4].contains("a  one") && rows[5].contains("b  two"),
        "{rows:#?}"
    );
    let c = rows[4].find("c  three").expect("c in the first row");
    let d = rows[5].find("d  four").expect("d in the second row");
    assert_eq!(c, d);
}

#[test]
fn which_key_can_wait_before_it_shows() {
    let mut driver = files()
        .which_key_delay(Duration::from_millis(300))
        .driver(60, 8);
    driver.update(Duration::ZERO);
    driver.render();
    driver.event(intuituive::interact::Event::Key(
        intuituive::interact::Key::char('g'),
    ));
    driver.update(Duration::from_millis(100));
    driver.render();
    assert!(!driver.screen().plain()[5].contains("Go home"));
    assert!(driver.timeout(Duration::from_millis(100)) <= Duration::from_millis(200));
    driver.update(Duration::from_millis(400));
    driver.render();
    assert!(driver.screen().plain()[5].contains("Go home"));
}

#[test]
fn the_help_and_the_palette_list_sequences() {
    let mut driver = files().help_key("f1").palette_key("ctrl+p").driver(60, 20);
    driver.press("f1");
    let help = driver.screen().plain();
    let line = |text: &str| {
        help.iter()
            .find(|r| r.contains(text))
            .cloned()
            .unwrap_or_default()
    };
    assert!(
        line("Go to the top").contains("  g g    Go to the top"),
        "{help:#?}"
    );
    assert!(
        line("Sort by mtime").contains("  , m t  Sort by mtime"),
        "{help:#?}"
    );
    driver.press("esc esc");
    driver.press("ctrl+p");
    let palette = driver.screen().plain().join("\n");
    assert!(
        palette.contains("Go home") && palette.contains("g h"),
        "{palette}"
    );
    // Run from the palette, as its keys would.
    driver.type_text("home");
    driver.press("enter");
    assert_eq!(row(&driver, 0), "home");
}

#[test]
fn the_palette_cuts_a_long_description_to_show_its_key() {
    let app = App::new(|| {
        label("x").bind(
            "esc",
            "Exit visual mode, clear selection, or cancel search",
            |_| {},
        )
    })
    .palette_key("ctrl+p");
    let mut driver = app.driver(60, 10);
    driver.press("ctrl+p");
    let rows = driver.screen().plain();
    let line = rows
        .iter()
        .find(|r| r.contains("Exit visual"))
        .expect("listed");
    assert!(line.contains("… esc"), "{line}");
}

#[test]
fn on_any_key_takes_what_the_bindings_leave() {
    let app = App::new(|| {
        let seen = signal(Vec::<String>::new());
        column([
            text(move || seen.get().join(" ")),
            label("inner")
                .focusable()
                .on_key("x", move |_| seen.update(|s| s.push("x!".into()))),
        ])
        .on_any_key(move |_, key| {
            seen.update(|s| s.push(key.to_string()));
            key.code != intuituive::interact::KeyCode::Char('n')
        })
        .on_key("n", move |_| seen.update(|s| s.push("never".into())))
    });
    let mut driver = app.driver(30, 2);
    // `x` is the inner node's; `y` and `ctrl+a` reach the catch-all, which
    // lets `n` go (its own binding came first anyway).
    driver.press("x y ctrl+a n");
    assert_eq!(row(&driver, 0), "x! y ctrl+a never");
}

#[test]
fn bindings_inside_a_pop_ups_panel_fire() {
    let app = App::new(|| {
        let said = signal(String::from("-"));
        text!("{said}").on_key("o", move |cx| {
            cx.modal(Size::Fixed(12), Size::Fixed(3), move || {
                label("y or n")
                    .on_key("y", move |cx| {
                        said.set("yes".into());
                        cx.pop();
                    })
                    .panel("Sure?")
            })
        })
    });
    let mut driver = app.driver(20, 5);
    driver.press("o");
    assert!(driver.screen().plain()[1].contains("Sure?"));
    driver.press("y");
    assert_eq!(row(&driver, 0), "yes");
    assert!(!driver.screen().plain()[1].contains("Sure?"));
}

#[test]
fn a_pop_ups_title_takes_the_themes_popup_title_style() {
    let theme = Theme::dark().style("popup.title", rich::Style::parse("bold magenta").unwrap());
    let app = App::new(|| {
        column([label("x").panel("Plain")]).on_key("o", |cx| {
            cx.modal(Size::Fixed(12), Size::Fixed(3), || {
                label("hi").panel("Popup")
            })
        })
    })
    .theme(theme);
    let mut driver = app.driver(30, 8);
    driver.press("o");
    let screen = driver.screen();
    let magenta = rich::Style::parse("bold magenta").unwrap();
    let find = |word: &str| {
        let rows = screen.plain();
        let y = rows.iter().position(|r| r.contains(word)).unwrap();
        let x = rows[y][..rows[y].find(word).unwrap()].chars().count();
        screen.cell(x as u16, y as u16).style
    };
    let popup = find("Popup");
    assert_eq!(
        screen.style(popup).map(|s| s.color().cloned()),
        Some(magenta.color().cloned())
    );
    let plain = find("Plain");
    assert_ne!(
        screen.style(plain).map(|s| s.color().cloned()),
        Some(magenta.color().cloned())
    );
}

#[test]
fn which_key_keeps_a_gap_between_its_columns() {
    // Yazi's which: three columns, descriptions longer than a column.
    let app = App::new(|| {
        let mut node = label("x");
        for (key, what) in [
            ("m", "Sort by modified time"),
            ("M", "Sort by modified time (reverse)"),
            ("a", "Sort alphabetically"),
            ("A", "Sort alphabetically (reverse)"),
            ("n", "Sort naturally"),
            ("N", "Sort naturally (reverse)"),
        ] {
            node = node.bind_sequence(&format!("comma {key}"), what, |_| {});
        }
        node
    })
    .which_key(true);
    let mut driver = app.driver(62, 8);
    driver.press("comma");
    let rows = driver.screen().plain();
    // Columns of 20 cells: a key, two cells, a description cut with `…`,
    // and at least two blank cells before the next column's key.
    let first = &rows[4];
    assert!(first.contains(" m  Sort by modi… "), "{rows:#?}");
    let next = first.find(" a  Sort").expect("the second column");
    assert!(first[..next].ends_with("…  "), "{first:?}");
    assert!(!first.contains("(rever a"), "{first:?}");
}

#[test]
fn which_key_styles_its_keys_separator_and_descriptions_from_the_theme() {
    let theme = Theme::dark()
        .with_config(
            "[styles]\nwhich_key.key = bright_cyan\nwhich_key.sep = on blue\n\
             which_key.desc = bright_magenta\n",
        )
        .unwrap();
    let mut driver = files().which_key(true).theme(theme).driver(60, 8);
    driver.press("g");
    let styled = intuituive::testing::styled(&driver);
    assert!(
        styled.contains("[bright_cyan] h[/][on blue]  [/][bright_magenta]Go home[/]"),
        "{styled}"
    );
}

#[test]
#[should_panic(expected = "bind_sequence(\"g\") has one key")]
fn a_sequence_of_one_key_is_refused() {
    let _ = App::new(|| label("x").bind_sequence("g", "Go", |_| {}));
}

#[test]
fn the_comma_key_has_a_name() {
    let app = App::new(|| {
        let ran = signal(String::from("-"));
        text!("{ran}")
            .bind_sequence("comma s", "Sort by size", move |_| ran.set("size".into()))
            .on_key("ctrl+comma", move |_| ran.set("settings".into()))
    });
    let mut driver = app.driver(20, 1);
    driver.press("comma s");
    assert_eq!(row(&driver, 0), "size");
    driver.press("ctrl+comma");
    assert_eq!(row(&driver, 0), "settings");
    // The quoted form is the same key.
    driver.press("\",\" s");
    assert_eq!(row(&driver, 0), "size");
}

#[test]
fn the_help_groups_bindings_by_section() {
    let app = App::new(|| {
        label("scope")
            .name("scope")
            .bind("q", "Quit", |cx| cx.quit())
            .bind_in("oscilloscope", "t", "Toggle the trigger", |_| {})
            .bind_in("spectroscope", "l", "Logarithmic scale", |_| {})
            .bind_in("oscilloscope", "e", "Falling edge", |_| {})
    })
    .help_key("?");
    let mut driver = app.driver(60, 24);
    driver.press("?");
    let rows = driver.screen().plain();
    let at = |text: &str| {
        rows.iter()
            .position(|r| r.contains(text))
            .unwrap_or_else(|| panic!("{text} in {rows:#?}"))
    };
    // Each section's heading, then its keys; the unsectioned under the
    // node's name.
    assert!(at("Quit") < at("oscilloscope"));
    assert!(at("oscilloscope") < at("Toggle the trigger"));
    assert_eq!(at("Falling edge"), at("Toggle the trigger") + 1);
    assert!(at("Falling edge") < at("spectroscope"));
    assert!(at("spectroscope") < at("Logarithmic scale"));
}

#[test]
fn scaled_variants_run_with_their_value_and_show_as_one_row() {
    let app = App::new(|| {
        let scale = signal(0.0f64);
        text!("{scale}")
            .bind_scaled(
                "up k",
                &[("", 1.0), ("shift", 10.0), ("ctrl", 5.0), ("alt", 0.5)],
                "Scale up",
                move |_, by| scale.update(|s| *s += by),
            )
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(90, 20);
    driver.press("up shift+up ctrl+up alt+up K alt+k");
    assert_eq!(row(&driver, 0), "27");
    driver.press("?");
    let rows = driver.screen().plain();
    let lines: Vec<&String> = rows.iter().filter(|r| r.contains("Scale up")).collect();
    assert_eq!(lines.len(), 1, "{rows:#?}");
    assert!(
        lines[0].contains("Scale up (shift ×10, ctrl ×5, alt ×0.5)"),
        "{rows:#?}"
    );
    assert!(!lines[0].contains("shift+"), "{rows:#?}");
}

#[test]
fn a_cycle_steps_through_the_modes_and_round() {
    let app = App::new(|| {
        let mode = signal(String::from("oscilloscope"));
        let modes = ["oscilloscope", "vectorscope", "spectroscope"].map(String::from);
        text!("{mode}").bind_cycle("tab", "Next mode", mode, &modes)
    });
    let mut driver = app.driver(20, 1);
    driver.press("tab");
    assert_eq!(row(&driver, 0), "vectorscope");
    driver.press("tab tab");
    assert_eq!(row(&driver, 0), "oscilloscope");
}
