//! The app shell (gap batch B6): modal shadows and backdrops, printing
//! above an inline app, exit values and errors, the mouse switch and the
//! frame count.

mod common;

use std::io;

use common::{run, screen};
use intuituive::interact::headless::{Headless, Script};
use intuituive::interact::BackendKind;
use intuituive::prelude::*;
use intuituive::{assert_screen_styled, ModalOptions};

fn with_modal(look: ModalOptions) -> intuituive::Driver {
    let app = App::new(move || {
        column([
            label("first row of the screen"),
            label("second row"),
            label("third"),
        ])
        .on_key("o", move |cx| {
            let look = look.clone();
            cx.modal_with(Size::Fixed(10), Size::Fixed(3), look, || {
                label("hi").panel("")
            })
        })
    });
    let mut driver = app.driver(20, 6);
    driver.press("o");
    driver
}

#[test]
fn modal_shadow() {
    let driver = with_modal(ModalOptions::new().shadow("on blue"));
    assert_screen_styled!(driver, "modal_shadow");
    let styled = intuituive::testing::styled(&driver);
    let lines: Vec<&str> = styled.lines().collect();
    // The box is rows 1-3, columns 5-14: the shadow is the column after it
    // from row 2, and the row under it from column 6.
    assert!(lines[2].contains("[on blue]"), "{styled}");
    assert!(lines[4].contains("[on blue]"), "{styled}");
    assert!(!lines[1].contains("[on blue]"), "{styled}");
}

#[test]
fn modal_backdrop() {
    let driver = with_modal(ModalOptions::new().backdrop("dim"));
    assert_screen_styled!(driver, "modal_backdrop");
    let styled = intuituive::testing::styled(&driver);
    // What is below is dimmed; the box itself is not.
    assert!(
        styled.lines().next().unwrap().starts_with("[dim]"),
        "{styled}"
    );
    assert!(styled.contains("hi"), "{styled}");
    assert!(!styled.contains("[dim]hi"), "{styled}");
}

#[test]
fn a_backdrop_follows_what_changes_below() {
    let app = App::new(|| {
        let count = signal(0);
        every(std::time::Duration::from_millis(10), move |_| {
            count.update(|n| *n += 1)
        });
        text!("count {count}").on_key("o", |cx| {
            let look = ModalOptions::new().backdrop("dim");
            cx.modal_with(Size::Fixed(6), Size::Fixed(3), look, || {
                label("x").panel("")
            })
        })
    });
    let mut driver = app.driver(20, 5);
    driver.press("o");
    driver.update(std::time::Duration::from_millis(15));
    let _ = driver.render();
    let styled = intuituive::testing::styled(&driver);
    // The count changed under the modal, and is dimmed still.
    assert!(styled.starts_with("[dim]count 1"), "{styled}");
}

fn printing_inline() -> App {
    App::new(|| {
        label("region")
            .on_key("p", |cx| {
                cx.print_above("[bold]done[/] step one");
                cx.print_above("step two");
            })
            .on_key("q", |cx| cx.quit())
    })
    .inline(2)
}

#[test]
fn printed_lines_come_before_the_region_redraws() {
    let record = run(printing_inline(), Script::new().keys("p q"), 30, 24);
    let out = record.output();
    let one = out.find("step one").expect("the first line");
    let two = out.find("step two").expect("the second line");
    // After both lines, the region is made again and drawn below them.
    let region = out.rfind("region").expect("the region");
    assert!(one < two && two < region, "{out:?}");
    assert!(out[two..].contains("\r\n\x1b[1A\x1b[J") || out[two..].contains("\x1b[J"));
    assert_eq!(screen(&record), ["region", ""]);
}

#[test]
fn a_full_screen_app_prints_after_it_ends() {
    let app = App::new(|| {
        label("full")
            .on_key("p", |cx| cx.print_above("afterwards"))
            .on_key("q", |cx| cx.quit())
    });
    let mut driver = app.driver(20, 2);
    driver.press("p");
    // A loop of your own takes them, to print once the terminal is back.
    assert_eq!(driver.take_printed(), ["afterwards"]);
    assert!(driver.take_printed().is_empty());
    // `run_on` writes them after the last frame.
    let app = App::new(|| {
        label("full")
            .on_key("p", |cx| cx.print_above("afterwards"))
            .on_key("q", |cx| cx.quit())
    });
    let record = run(app, Script::new().keys("p q"), 20, 2);
    let last = record.writes.last().expect("a write");
    assert!(last.contains("afterwards"), "{:?}", record.writes);
}

#[test]
fn a_proxy_prints_above_too() {
    let app = App::new(|| label("region")).inline(1);
    let proxy = app.proxy();
    let mut driver = app.driver(20, 5);
    driver.settle();
    std::thread::spawn(move || proxy.print_above("from a thread"))
        .join()
        .unwrap();
    driver.update(std::time::Duration::from_millis(1));
    let out = driver.render().expect("a frame");
    assert!(out.contains("from a thread"), "{out:?}");
    assert!(driver.take_printed().is_empty());
}

#[test]
fn quit_with_reaches_run_for() {
    let app = App::new(|| {
        let selected = signal(0usize);
        let items = || vec!["red".to_string(), "green".into()];
        list(items, selected).on_key("enter", move |cx| cx.quit_with(selected.get()))
    });
    let mut backend = Headless::new(Script::new().keys("down enter"), 20, 3);
    assert_eq!(app.run_on_for::<usize>(&mut backend).unwrap(), Some(1));
    // Another type is no value; a plain quit neither.
    let app = App::new(|| label("x").on_key("y", |cx| cx.quit_with("yes")));
    let mut backend = Headless::new(Script::new().keys("y"), 10, 1);
    assert_eq!(app.run_on_for::<usize>(&mut backend).unwrap(), None);
    let app = App::new(|| label("x").on_key("q", |cx| cx.quit()));
    let mut backend = Headless::new(Script::new().keys("q"), 10, 1);
    assert_eq!(app.run_on_for::<usize>(&mut backend).unwrap(), None);
    // A loop of your own takes it.
    let app = App::new(|| label("x").on_key("y", |cx| cx.quit_with(7u8)));
    let mut driver = app.driver(10, 1);
    driver.press("y");
    assert!(driver.is_done());
    assert_eq!(driver.take_exit::<String>(), None);
    assert_eq!(driver.take_exit::<u8>(), Some(7));
    assert_eq!(driver.take_exit::<u8>(), None);
}

#[test]
fn fail_makes_run_on_return_the_error() {
    let app = App::new(|| {
        label("x").on_key("f", |cx| {
            cx.fail(io::Error::new(io::ErrorKind::PermissionDenied, "no access"))
        })
    });
    let mut backend = Headless::new(Script::new().keys("f"), 10, 1);
    let error = app.run_on(&mut backend).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "no access");
    // Any error, as `Other`.
    let app = App::new(|| label("x").on_key("f", |cx| cx.fail("bad config")));
    let mut backend = Headless::new(Script::new().keys("f"), 10, 1);
    let error = app.run_on(&mut backend).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "bad config");
}

#[test]
fn the_mouse_switch_reaches_the_session() {
    let full = App::new(|| label("x"));
    assert!(full.session_options(BackendKind::Crossterm).mouse);
    let full = App::new(|| label("x")).mouse(Some(false));
    let options = full.session_options(BackendKind::Crossterm);
    assert!(!options.mouse);
    assert!(options.alternate_screen);
    let inline = App::new(|| label("x")).inline(3);
    assert!(!inline.session_options(BackendKind::Crossterm).mouse);
    let inline = App::new(|| label("x")).inline(3).mouse(Some(true));
    let options = inline.session_options(BackendKind::Crossterm);
    assert!(options.mouse);
    assert!(!options.alternate_screen);
}

#[test]
fn frames_are_counted() {
    let app = App::new(|| {
        let count = signal(0);
        text!("{count}").on_key("+", move |_| count.update(|n| *n += 1))
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    let first = driver.stats().frames;
    assert!(first >= 1);
    driver.press("+");
    driver.press("+");
    assert_eq!(driver.stats().frames, first + 2);
}
