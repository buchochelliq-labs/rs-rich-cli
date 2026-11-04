//! The log console (#815): an app's `tracing` events as JSON lines on a
//! local socket (a Unix socket, a named pipe on Windows), where `rich
//! console` listens.
#![cfg(all(feature = "tracing", any(unix, windows)))]

use std::io::{BufRead, BufReader};
use std::time::Duration;

use intuituive::log::{console_layer, install_console};
use intuituive::prelude::*;
use rich_ext::local_socket::{LocalListener, private_dir};
use tracing_subscriber::prelude::*;

/// A socket's path in a private directory of its own.
fn socket(name: &str) -> std::path::PathBuf {
    let base =
        std::env::temp_dir().join(format!("intuituive-console-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let dir = base.join("private");
    private_dir(&dir).unwrap();
    dir.join("console.sock")
}

#[test]
fn a_handlers_event_arrives_with_its_level_and_target() {
    let path = socket("events");
    let listener = LocalListener::bind(&path).unwrap();
    let subscriber = tracing_subscriber::registry().with(console_layer(&path).unwrap());
    let stream = listener.accept().unwrap();
    stream.peer_allowed().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    tracing::subscriber::with_default(subscriber, || {
        let app = App::new(|| {
            label("app")
                .on_key("i", |_| tracing::info!(target: "myapp", rows = 3, "loaded"))
                .on_key("l", |cx| cx.log("from cx.log"))
        });
        let mut driver = app.driver(10, 1);
        driver.press("i l");
    });
    let all: Vec<String> = BufReader::new(stream)
        .lines()
        .map_while(Result::ok)
        .collect();
    // The inspector's trace comes too, at TRACE: the keys pressed.
    assert!(
        all.iter()
            .any(|line| line.contains(r#""target":"intuituive::trace""#) && line.contains("key i")),
        "{all:#?}"
    );
    let mut lines = all
        .into_iter()
        .filter(|line| !line.contains("intuituive::trace"))
        .map(Ok::<_, ()>);
    let first = lines.next().unwrap().unwrap();
    assert!(first.contains(r#""level":"info""#), "{first}");
    assert!(first.contains(r#""target":"myapp""#), "{first}");
    assert!(first.contains(r#""message":"loaded""#), "{first}");
    assert!(first.contains(r#""rows":3"#), "{first}");
    let second = lines.next().unwrap().unwrap();
    assert!(second.contains(r#""level":"debug""#), "{second}");
    assert!(second.contains(r#""target":"app""#), "{second}");
    assert!(second.contains("from cx.log"), "{second}");
    drop(listener);
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn without_the_variable_nothing_is_opened() {
    // Tests run without it (setting it here would need `unsafe`).
    if std::env::var_os(intuituive::log::CONSOLE_VAR).is_some() {
        return;
    }
    assert!(intuituive::log::console_socket().is_none());
    assert!(!install_console().unwrap());
}

#[test]
fn the_variable_names_the_socket() {
    assert!(
        intuituive::log::default_console_socket()
            .is_none_or(|path| path.ends_with("intuituive/console/console.sock"))
    );
}
