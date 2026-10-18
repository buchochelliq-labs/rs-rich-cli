//! The port of ratatui's `panic` (examples/panic.rs): in a real PTY, with
//! rs-rich-interact's harness running this binary's `child` test as the
//! app, `p` gives the terminal back before the panic's message prints;
//! headless, `e` makes `run_on` return the error, and the screen as an
//! 80x24 snapshot.

#[path = "../examples/panic.rs"]
#[allow(dead_code)]
mod panic;

use intuituive::assert_screen;
use intuituive::interact::headless::{Headless, Script};
use panic::{panic_app, ERROR, PANIC};

#[test]
fn panic_screen() {
    let mut driver = panic_app().driver(80, 24);
    driver.settle();
    assert_screen!(driver, "panic_screen");
}

#[test]
fn e_makes_run_on_return_the_error() {
    let mut backend = Headless::new(Script::new().keys("e"), 80, 24);
    let error = panic_app()
        .run_on(&mut backend)
        .expect_err("e ends the app with an error");
    assert_eq!(error.to_string(), ERROR);
}

#[test]
fn q_quits_without_one() {
    let mut backend = Headless::new(Script::new().keys("q"), 80, 24);
    assert!(panic_app().run_on(&mut backend).is_ok());
}

#[test]
fn p_panics_with_the_message() {
    let mut driver = panic_app().driver(80, 24);
    driver.settle();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| driver.press("p")))
        .expect_err("p panics");
    let message = caught.downcast_ref::<String>().cloned().unwrap_or_default();
    assert_eq!(message, PANIC);
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = panic_app().driver(80, 24);
    driver.press("?");
    let rows = driver.screen().plain();
    assert!(rows.iter().any(|r| r.contains("panic in a key handler")));
}

// rs-rich-interact's PTY harness: the child is this binary's `child`
// test.
#[cfg(unix)]
#[path = "../../rich-interact/tests/support/mod.rs"]
mod support;

/// The app the PTY test runs, in this binary: idle unless the harness
/// started it.
#[test]
fn child() {
    if std::env::var_os("INTERACT_CHILD").is_none() {
        return;
    }
    let result = panic_app().run();
    println!("OUTCOME {result:?}");
}

#[cfg(unix)]
mod pty {
    use super::panic::PANIC;
    use super::support::{assert_restored, Pty};

    #[test]
    fn p_gives_the_terminal_back_before_the_message_prints() {
        let mut pty = Pty::start("panic");
        pty.wait_for("Panics and errors");
        pty.send("p");
        let (output, parser) = pty.finish();
        // The terminal is as it was: the main screen, the cursor shown,
        // the mouse off, the line discipline restored.
        assert_restored(&output, &parser);
        // And the message is on it, written after the alternate screen
        // was left.
        let left = output
            .rfind("\x1b[?1049l")
            .expect("the alternate screen was left");
        let message = output.rfind(PANIC).expect("the message printed");
        assert!(left < message, "{output:?}");
        // On the screen too, wherever its line wrapped (macOS's terminal
        // driver can leave the cursor mid-line when the message prints).
        let squeeze = |text: &str| text.split_whitespace().collect::<String>();
        let screen = parser.screen().contents();
        assert!(squeeze(&screen).contains(&squeeze(PANIC)), "{screen}");
        assert!(!output.contains("OUTCOME"), "the app did not return");
    }
}
