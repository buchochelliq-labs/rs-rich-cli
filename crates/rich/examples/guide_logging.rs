//! Guide: Logging and errors — run: cargo run -p rs-rich --example guide_logging [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/logging-and-errors.md are cut from this file.

#[path = "guide_support/mod.rs"]
mod guide_support;

use std::fmt;

use guide_support::Shots;

// --8<-- [start:imports]
use rich::{level_text, Console, LogLevel, LogRecord, LogRender, Text, Traceback};
// --8<-- [end:imports]

fn main() {
    let shots = Shots::from_args("guide_logging");
    shots.shot("records", 72, records);
    shots.shot("render", 72, log_render);
    shots.shot("traceback", 72, traceback);
    shots.shot("panic", 72, panic_message);
    if !shots.is_svg() {
        panic_hook();
        let _ = std::panic::catch_unwind(|| panic!("demo panic, caught"));
        let _ = std::panic::take_hook(); // back to the default hook
    }
}

// --8<-- [start:records]
fn records(console: &Console) {
    // One record at a time; the time is whatever string you format.
    console.print(&LogRecord::new(LogLevel::Info, "Server starting").time("[12:00:01]"));
    console.print(
        &LogRecord::new(LogLevel::Warn, "Config file not found, using defaults")
            .time("[12:00:01]")
            .path("main.rs")
            .line(42),
    );
    console.print(
        &LogRecord::new(
            LogLevel::Error,
            "Could not bind to port 80: permission denied",
        )
        .time("[12:00:02]")
        .path("net.rs")
        .line(118),
    );
    console.print(&LogRecord::new(
        LogLevel::Debug,
        "no time column when there is no time",
    ));
}
// --8<-- [end:records]

// --8<-- [start:render]
fn log_render(console: &Console) {
    // Share one LogRender across a stream so a repeated time is blanked.
    let render = LogRender::new().show_level(true).level_width(Some(8));

    let lines = [
        (
            "[12:00:01]",
            "INFO",
            "Request 1 served in 3 ms",
            "http.rs",
            20,
        ),
        (
            "[12:00:01]",
            "INFO",
            "Request 2 served in 5 ms",
            "http.rs",
            20,
        ),
        ("[12:00:02]", "CRITICAL", "Worker 3 exited", "pool.rs", 97),
    ];
    for (time, level, message, path, line) in lines {
        let table = render.render(
            console,
            Text::new(message),
            Some(Text::new(time)),
            level_text(level), // styled with logging.level.<name>
            Some(path),
            Some(line),
            None, // or Some("/abs/path/http.rs") to hyperlink the path
        );
        console.print(&table);
    }
}
// --8<-- [end:render]

// --8<-- [start:traceback]
#[derive(Debug)]
struct ConfigError {
    path: String,
    source: std::io::Error,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "could not load config from {}", self.path)
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

fn load_config() -> Result<String, ConfigError> {
    Err(ConfigError {
        path: "/etc/app/config.toml".into(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory"),
    })
}

fn traceback(console: &Console) {
    if let Err(error) = load_config() {
        // The message, then each `source()` as a "Caused by:" line.
        console.print(&Traceback::new(&error));
    }
}
// --8<-- [end:traceback]

// --8<-- [start:panic]
fn panic_message(console: &Console) {
    // Any string works, e.g. a message captured by a panic hook.
    console.print(&Traceback::from_message(
        "index out of bounds: the len is 3 but the index is 7",
    ));
}
// --8<-- [end:panic]

// --8<-- [start:hook]
fn panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        let location = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        Console::new().print(&Traceback::from_message(format!("{message}{location}")));
    }));
}
// --8<-- [end:hook]
