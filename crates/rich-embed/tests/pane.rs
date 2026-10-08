//! The terminal pane against a host that plays bytes back: output draws,
//! keys reach the program as the bytes a terminal sends, resizes reach the
//! host, and the exit is reported.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use rich_embed::{terminal_with, ExitStatus, ReplayHandle, ReplayHost};
use rich_intuituive::interact::{Button, Event, Key, Mouse, MouseKind};
use rich_intuituive::prelude::*;
use rich_intuituive::Driver;

/// Turn the app's loop once: results from other threads, then a frame.
fn turn(driver: &mut Driver) {
    driver.update(Duration::ZERO);
    driver.render();
    driver.update(Duration::ZERO);
    driver.render();
}

fn key(driver: &mut Driver, name: &str) {
    driver.event(Event::Key(Key::parse(name).unwrap()));
    turn(driver);
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

/// An app with one pane over `host`, `width` x `height`.
fn pane(host: ReplayHost, width: u16, height: u16) -> (Driver, ReplayHandle) {
    let handle = host.handle();
    let app = App::new(move || terminal_with(host).node());
    let mut driver = app.driver(width, height);
    turn(&mut driver);
    (driver, handle)
}

#[test]
fn output_draws_in_the_pane() {
    let host = ReplayHost::new().output("hello\r\n\x1b[1;32mworld\x1b[0m 漢字!");
    let (mut driver, handle) = pane(host, 20, 4);
    assert_eq!(handle.started(), Some((20, 4)));
    let screen = rows(&driver);
    assert_eq!(screen[0], "hello");
    assert_eq!(screen[1], "world 漢字!");
    // The program's colours come through.
    let cell = driver.screen().cell(0, 1);
    let style = driver.screen().style(cell.style).unwrap();
    assert_eq!(style.color(), Some(&rich::Color::from_ansi(2)));

    // Output that comes later, from another thread, wakes the app.
    let feeder = handle.clone();
    std::thread::spawn(move || feeder.feed("\r\nlater"))
        .join()
        .unwrap();
    turn(&mut driver);
    assert_eq!(rows(&driver)[2], "later");
}

#[test]
fn keys_and_pastes_reach_the_program_as_terminal_bytes() {
    let (mut driver, handle) = pane(ReplayHost::new(), 20, 4);
    for name in ["l", "s", "enter", "up", "ctrl+c", "tab", "alt+b", "f5"] {
        key(&mut driver, name);
    }
    assert_eq!(handle.take_written(), b"ls\r\x1b[A\x03\t\x1bb\x1b[15~".to_vec());
    // A program that turned on application cursor keys and bracketed
    // paste gets those.
    handle.feed("\x1b[?1h\x1b[?2004h");
    turn(&mut driver);
    key(&mut driver, "up");
    driver.event(Event::Paste("a\nb".into()));
    turn(&mut driver);
    assert_eq!(
        handle.take_written(),
        b"\x1bOA\x1b[200~a\nb\x1b[201~".to_vec()
    );
}

#[test]
fn the_mouse_reaches_a_program_that_asked_for_it() {
    let (mut driver, handle) = pane(ReplayHost::new(), 20, 4);
    // Not asked for: nothing is sent (the press is left to the app, whose
    // selection may take it).
    driver.event(Event::Mouse(Mouse::new(MouseKind::Down(Button::Left), 3, 1)));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 3, 1)));
    turn(&mut driver);
    assert!(handle.take_written().is_empty());
    // SGR reports of presses and releases.
    handle.feed("\x1b[?1000h\x1b[?1006h");
    turn(&mut driver);
    driver.event(Event::Mouse(Mouse::new(MouseKind::Down(Button::Left), 3, 1)));
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 3, 1)));
    turn(&mut driver);
    assert_eq!(
        handle.take_written(),
        b"\x1b[<0;4;2M\x1b[<0;4;2m".to_vec()
    );
}

#[test]
fn a_resize_reaches_the_host() {
    let host = ReplayHost::new().output("x");
    let handle = host.handle();
    let app = App::new(move || {
        row([
            terminal_with(host).node(),
            label("side").fixed(4),
        ])
    });
    let mut driver = app.driver(24, 5);
    turn(&mut driver);
    assert_eq!(handle.started(), Some((20, 5)));
    driver.event(Event::Resize {
        columns: 34,
        rows: 8,
    });
    turn(&mut driver);
    assert_eq!(handle.sizes(), vec![(30, 8)]);
    assert_eq!(rows(&driver)[0], "x                             side");
}

#[test]
fn the_exit_is_a_signal_and_a_callback() {
    let host = ReplayHost::new()
        .output("bye")
        .exit(ExitStatus::with_code(3));
    let handle = host.handle();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let app = App::new(move || {
        let pane = terminal_with(host).on_exit(move |status, _| {
            log.borrow_mut().push(status.code());
        });
        let status = pane.status();
        column([
            pane.node(),
            text(move || match status.get() {
                Some(status) => format!("{status}"),
                None => "running".into(),
            })
            .fixed(1),
        ])
    });
    let mut driver = app.driver(30, 4);
    turn(&mut driver);
    let screen = rows(&driver);
    assert_eq!(screen[0], "bye");
    assert_eq!(screen[3], "exited with code 3");
    assert_eq!(*seen.borrow(), vec![3]);
    // After the exit, keys are no longer sent.
    key(&mut driver, "x");
    assert!(handle.written().is_empty());
}

#[test]
fn the_view_scrolls_back() {
    let lines: String = (1..=10).map(|n| format!("line {n}\r\n")).collect();
    let (mut driver, handle) = pane(ReplayHost::new().output(lines), 12, 3);
    assert_eq!(rows(&driver)[0], "line 9");
    key(&mut driver, "shift+pageup");
    assert!(rows(&driver)[0].starts_with("line 6"), "{:?}", rows(&driver));
    // The wheel scrolls back down; nothing is sent to the program.
    driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 0, 0)));
    turn(&mut driver);
    assert_eq!(rows(&driver)[0], "line 9");
    assert!(handle.written().is_empty());
}

#[test]
fn released_keys_go_to_the_app_and_the_program_ends_with_its_pane() {
    let host = ReplayHost::new().output("x");
    let handle = host.handle();
    let app = App::new(move || {
        let panes = signal(vec![1]);
        let host = RefCell::new(Some(host));
        column([
            each(
                move || panes.get(),
                move |_| match host.borrow_mut().take() {
                    Some(host) => terminal_with(host).release_keys("f10").node(),
                    None => label("again"),
                },
            ),
            label("app").fixed(1),
        ])
        .on_key("f10", move |_| panes.set(Vec::new()))
    });
    let mut driver = app.driver(12, 3);
    turn(&mut driver);
    assert_eq!(rows(&driver)[0], "x");
    key(&mut driver, "f10");
    assert!(handle.written().is_empty());
    assert_eq!(rows(&driver)[0], "");
    assert!(handle.killed());
}
