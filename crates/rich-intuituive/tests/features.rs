//! The command palette and help built from bindings, toasts, animations,
//! text selection with copying, and menus.

mod common;

use std::time::Duration;

use common::{row_of, run, run_open, screen};
use intuituive::interact::headless::Script;
use intuituive::interact::{Button, MouseKind};
use intuituive::menu::{context_menu, menu_bar, open_menu, Menu, MenuItem};
use intuituive::prelude::*;
use intuituive::{Easing, Placement};

fn counter() -> App {
    App::new(|| {
        let count = signal(0);
        column([text!("count {count}").fixed(1), label("body")])
            .bind("+", "Add one", move |_| count.update(|n| *n += 1))
            .bind("r", "Reset the count", move |_| count.set(0))
            .on_key("q", |cx| cx.quit())
    })
    .palette_key("ctrl+p")
    .help_key("?")
}

#[test]
fn the_palette_runs_a_described_binding_by_name() {
    let script = Script::new().keys("+ ctrl+p").text("add").keys("enter q");
    let rows = screen(&run(counter(), script, 40, 10));
    assert_eq!(rows[0], "count 2", "{rows:?}");
}

#[test]
fn the_palette_lists_only_described_bindings() {
    let rows = screen(&run_open(counter(), Script::new().keys("ctrl+p"), 60, 12));
    assert!(row_of(&rows, "Add one").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Reset the count").is_some(), "{rows:?}");
    // `q` has no description.
    assert!(!rows.iter().any(|r| r.contains("Quit")), "{rows:?}");
}

#[test]
fn the_help_shows_bindings_with_their_keys_and_esc_closes_it() {
    let rows = screen(&run_open(counter(), Script::new().keys("?"), 40, 10));
    let at = row_of(&rows, "Reset the count").expect("the help lists the binding");
    assert!(rows[at].contains('r'), "{rows:?}");
    let rows = screen(&run(counter(), Script::new().keys("? esc q"), 40, 10));
    assert_eq!(row_of(&rows, "Reset the count"), None, "{rows:?}");
}

fn toaster() -> App {
    App::new(|| {
        label("main")
            .on_key("t", |cx| {
                cx.toast_for("[b]Saved[/]", Duration::from_secs(2))
            })
            .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn a_toast_shows_at_the_bottom_right_and_expires() {
    let rows = screen(&run_open(toaster(), Script::new().keys("t"), 30, 8));
    let at = row_of(&rows, "Saved").expect("the toast shows");
    assert!(at >= 4, "{rows:?}");
    assert!(rows[at].trim_end().ends_with('│'), "{rows:?}");
    let script = Script::new().keys("t").wait(Duration::from_secs(3));
    let rows = screen(&run_open(toaster(), script, 30, 8));
    assert_eq!(row_of(&rows, "Saved"), None, "{rows:?}");
}

#[test]
fn an_animation_moves_a_signal_to_its_target_over_time() {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = seen.clone();
    let app = App::new(move || {
        let x = signal(0.0f64);
        let log = log.clone();
        column([text!("x {:.0}", x.get()).fixed(1)])
            .on_key("a", move |cx| {
                cx.animate(x, 100.0, Duration::from_millis(200), Easing::Linear)
            })
            .on_key("q", move |cx| {
                log.borrow_mut().push(x.get());
                cx.quit()
            })
    });
    let script = Script::new()
        .keys("a")
        .wait(Duration::from_millis(100))
        .keys("q");
    run(app, script, 20, 3);
    let halfway = seen.borrow()[0];
    assert!(halfway > 20.0 && halfway < 80.0, "{halfway}");

    let app = App::new(|| {
        let x = signal(0.0f64);
        text!("x {:.0}", x.get())
            .on_key("a", move |cx| {
                cx.animate(x, 100.0, Duration::from_millis(200), Easing::EaseInOut)
            })
            .on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .keys("a")
        .wait(Duration::from_millis(500))
        .keys("q");
    assert_eq!(screen(&run(app, script, 20, 3))[0], "x 100");
}

#[test]
fn easings_start_at_zero_and_end_at_one() {
    for easing in [
        Easing::Linear,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
    ] {
        assert_eq!(easing.at(0.0), 0.0);
        assert!((easing.at(1.0) - 1.0).abs() < 1e-9);
        assert!(easing.at(0.5) > 0.0 && easing.at(0.5) < 1.0);
    }
}

fn text_app() -> App {
    App::new(|| {
        column([label("hello world").fixed(1), label("second line").fixed(1)])
            .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn dragging_over_text_selects_it_and_copies_it() {
    let script = Script::new().drag((0, 0), (4, 0)).keys("q");
    let record = run(text_app(), script, 20, 4);
    assert_eq!(record.copies, ["hello"]);

    // Across lines: the rest of the first and the start of the second.
    let script = Script::new().drag((6, 0), (5, 1)).keys("q");
    let record = run(text_app(), script, 20, 4);
    assert_eq!(record.copies, ["world\nsecond"]);
}

#[test]
fn selection_can_be_turned_off_and_a_click_selects_nothing() {
    let script = Script::new().drag((0, 0), (4, 0)).keys("q");
    let record = run(text_app().selectable(false), script, 20, 4);
    assert!(record.copies.is_empty());
    let record = run(text_app(), Script::new().click(2, 0).keys("q"), 20, 4);
    assert!(record.copies.is_empty());
}

fn menu_app() -> App {
    App::new(|| {
        let picked = signal(String::from("none"));
        let item =
            move |name: &'static str| MenuItem::new(name, move |_| picked.set(name.to_string()));
        column([
            menu_bar(vec![
                Menu::new(
                    "File",
                    vec![item("New"), MenuItem::separator(), item("Open")],
                ),
                Menu::new("Edit", vec![item("Copy").hint("ctrl+c"), item("Paste")]),
            ])
            .fixed(1),
            text!("picked {picked}")
                .fixed(1)
                .on_mouse(move |cx, mouse| {
                    if mouse.kind != MouseKind::Down(Button::Right) {
                        return false;
                    }
                    context_menu(cx, vec![item("Cut"), item("Delete")]);
                    true
                }),
            label(""),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn the_menu_bar_opens_a_menu_below_the_title_and_runs_an_item() {
    // " File  Edit ": Edit starts at column 6.
    let rows = screen(&run_open(
        menu_app(),
        Script::new().keys("right enter"),
        30,
        8,
    ));
    let at = row_of(&rows, "Copy").expect("the Edit menu is open");
    assert_eq!(at, 2, "{rows:?}");
    assert!(rows[at].contains("ctrl+c"), "{rows:?}");
    assert_eq!(rows[at].find('│'), Some(6), "{rows:?}");

    let rows = screen(&run(
        menu_app(),
        Script::new().keys("enter down enter q"),
        30,
        8,
    ));
    // Down skips the separator.
    assert_eq!(rows[1], "picked Open", "{rows:?}");

    let script = Script::new().click(7, 0).click(8, 3).keys("q");
    let rows = screen(&run(menu_app(), script, 30, 8));
    assert_eq!(rows[1], "picked Paste", "{rows:?}");
}

#[test]
fn a_context_menu_opens_at_the_pointer() {
    let script = Script::new().mouse(MouseKind::Down(Button::Right), 4, 1);
    let rows = screen(&run_open(menu_app(), script, 30, 8));
    let at = row_of(&rows, "Cut").expect("the context menu is open");
    assert_eq!(at, 3, "{rows:?}");
    assert_eq!(rows[at].find('│'), Some(4), "{rows:?}");

    let script = Script::new()
        .mouse(MouseKind::Down(Button::Right), 4, 1)
        .keys("down enter q");
    assert_eq!(screen(&run(menu_app(), script, 30, 8))[1], "picked Delete");
}

#[test]
fn a_popup_can_anchor_to_a_rectangle_of_the_screen() {
    let app = App::new(|| {
        label("").on_key("o", |cx| {
            open_menu(
                cx,
                intuituive::screen::Rect::new(10, 5, 4, 1),
                Placement::Above,
                vec![MenuItem::new("Up", |_| {})],
            )
        })
    });
    let rows = screen(&run_open(app, Script::new().keys("o"), 30, 8));
    assert_eq!(row_of(&rows, "Up"), Some(3), "{rows:?}");
    assert_eq!(rows[3].find('│'), Some(10), "{rows:?}");
}

#[test]
fn a_copy_from_a_timer_reaches_the_clipboard_before_the_app_quits() {
    let app = App::new(|| {
        every(Duration::from_millis(5), |cx| {
            cx.copy("from a timer");
            cx.quit();
        });
        label("waiting")
    });
    // The timer quits in the same turn: no input follows it.
    let record = run(app, Script::new().wait(Duration::from_millis(20)), 20, 2);
    assert_eq!(record.copies, ["from a timer"]);
}
