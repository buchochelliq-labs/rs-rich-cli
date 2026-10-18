//! A port of ratatui's `panic` example
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps/panic>),
//! rebuilt on intuiTUIve. It reuses none of ratatui's code: it rebuilds the
//! behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs.
//!
//!     cargo run -p rs-rich-intuituive --example panic
//!
//! p panics in a key handler: the terminal is given back (the main screen,
//! the cursor, the keyboard as it was) before the panic's message prints,
//! so the message is readable and the shell works. e ends the app with an
//! error, which `run` returns once the terminal is back, and `main` prints.
//! q quits.
//!
//! What intuiTUIve adds: nothing to set up. ratatui's example installs a
//! panic hook that restores the terminal; here the session's own hook does
//! it for every app, and `cx.fail` hands an error to `run`'s caller as an
//! `io::Error`.

use intuituive::prelude::*;

/// The message the panic carries.
pub const PANIC: &str = "p was pressed: a key handler panicked";

/// The error `e` ends the app with.
pub const ERROR: &str = "e was pressed: the app ended with an error";

/// The app.
pub fn panic_app() -> App {
    App::new(|| {
        let lines = [
            "[bold]p[/]  panic in a key handler. The terminal is given back first,",
            "   then the panic's message prints below the shell's prompt.",
            "",
            "[bold]e[/]  end the app with an error. [italic]run[/] returns it once the",
            "   terminal is back, and [italic]main[/] prints it.",
            "",
            "[bold]q[/]  quit, with no error.",
        ];
        label(lines.join("\n"))
            .name("keys")
            .padding(1, 2)
            .panel("Panics and errors")
            .title_bottom("[dim]? lists the keys[/]")
            .bind("p", "panic in a key handler", |_| panic!("{PANIC}"))
            .bind("e", "end with an error", |cx| cx.fail(ERROR))
            .bind("q", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p")
}

#[allow(dead_code)]
fn main() {
    if let Err(error) = panic_app().run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
