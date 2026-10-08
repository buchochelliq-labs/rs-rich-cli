//! An app run with [`App::run_with`] on every backend this build has
//! (crossterm, and termion and termwiz behind the features of those names),
//! in a real PTY: keys reach its bindings, Ctrl+Z suspends it and `fg`
//! draws it again, a SIGTSTP from outside resumes it without it drawing
//! over and over, and the terminal is given back (`stty -a` afterwards,
//! and a vt100 emulator) when it quits, on Ctrl+C and inline.
#![cfg(unix)]

// rs-rich-interact's PTY harness: the child is this binary's `child` test.
#[path = "../../rich-interact/tests/support/mod.rs"]
mod support;

use std::process::Command;
use std::time::{Duration, Instant};

use intuituive::interact::BackendKind;
use intuituive::prelude::*;
use support::{assert_restored, suspends_and_resumes, Pty};

fn app() -> App {
    let head = format!("child ready pid {}.", std::process::id());
    App::new(move || {
        let count = signal(0);
        column([label(head), text!("count {count}!")])
            .on_key("+", move |_| count.update(|n| *n += 1))
            .on_key("q", |cx| cx.quit())
    })
}

#[test]
fn child() {
    let Some(mode) = std::env::var_os("INTERACT_CHILD") else {
        return;
    };
    let backend = std::env::var("INTERACT_BACKEND")
        .ok()
        .and_then(|name| BackendKind::from_name(&name))
        .unwrap_or_default();
    let app = if mode == "inline" {
        app().inline(3)
    } else {
        app()
    };
    let result = app.run_with(backend);
    println!("OUTCOME {result:?}");
}

fn backends() -> impl Iterator<Item = &'static str> {
    BackendKind::ALL.iter().map(|kind| kind.name())
}

/// Wait until the terminal's screen, as an emulator shows it, has `text`
/// (the app draws only the cells that change, so the bytes may not).
fn wait_for_screen(pty: &Pty, text: &str) {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(pty.text().as_bytes());
        let screen = parser.screen().contents();
        if screen.contains(text) {
            return;
        }
        assert!(Instant::now() < end, "no {text:?} on the screen:\n{screen}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Quit with `q`, and check the terminal was given back.
fn quit(mut pty: Pty, backend: &str) {
    pty.send("q");
    let (output, parser) = pty.finish();
    assert!(output.contains("OUTCOME Ok(())"), "{backend}: {output}");
    assert_restored(&output, &parser);
}

#[test]
fn an_app_runs_on_every_backend() {
    for backend in backends() {
        let mut pty = Pty::start_on(backend, "keys");
        pty.wait_for("child ready");
        pty.send("+");
        pty.send("+");
        wait_for_screen(&pty, "count 2!");
        quit(pty, backend);

        let mut pty = Pty::start_on(backend, "interrupt");
        pty.wait_for("child ready");
        pty.send("\x03");
        let (output, parser) = pty.finish();
        assert!(output.contains("OUTCOME Ok(())"), "{backend}: {output}");
        assert_restored(&output, &parser);
    }
}

#[test]
fn an_inline_app_runs_on_every_backend() {
    for backend in backends() {
        let mut pty = Pty::start_on(backend, "inline");
        pty.wait_for("child ready");
        assert!(!pty.text().contains("\x1b[?1049h"), "{backend}");
        pty.send("+");
        wait_for_screen(&pty, "count 1!");
        quit(pty, backend);
    }
}

#[test]
fn ctrl_z_suspends_an_app_and_fg_draws_it_again() {
    for backend in backends() {
        let mut pty = Pty::start_on(backend, "ctrl-z");
        suspends_and_resumes(&mut pty, |pty, _| pty.send("\x1a"));
        pty.send("+");
        wait_for_screen(&pty, "count 1!");
        quit(pty, backend);
    }
}

#[test]
fn an_app_resumed_from_outside_draws_once() {
    for backend in backends() {
        let mut pty = Pty::start_on(backend, "sigtstp");
        suspends_and_resumes(&mut pty, |_, pid| {
            Command::new("kill").args(["-TSTP", pid]).status().unwrap();
        });
        // Drawn again once, then idle: nothing more is written.
        std::thread::sleep(Duration::from_millis(300));
        let drawn = pty.text().len();
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(pty.text().len(), drawn, "{backend}: still drawing");
        pty.send("+");
        wait_for_screen(&pty, "count 1!");
        quit(pty, backend);
    }
}
