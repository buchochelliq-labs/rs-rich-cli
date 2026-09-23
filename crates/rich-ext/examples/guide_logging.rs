//! Guide: Logging — run: cargo run -p rs-rich-ext --example guide_logging --features log,tracing [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/logging.md` come from this file. With
//! `--svg DIR` every shot is written as `DIR/guide_logging-<shot>.svg`.
//! Records then go to a recording sink first, so the handler can lay them out
//! on the SVG console; without it they print through `RichHandler` directly.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rich::{ColorSystem, Console};
use rich_ext::adapters::{EventLayer, EventSink, LogAdapter};
use rich_ext::event::{
    EventContext, EventView, Message, Severity, SourceLocation, StructuredEvent, Value,
};
use rich_ext::RichHandler;

const WIDTH: usize = 90;

/// Where shots go: the terminal, or one SVG per shot.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let dir = args
            .iter()
            .position(|arg| arg == "--svg")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        Shots { dir }
    }

    fn svg(&self) -> bool {
        self.dir.is_some()
    }

    fn console(&self) -> Console {
        match self.dir {
            None => Console::new(),
            Some(_) => Console::builder()
                .width(WIDTH)
                .force_terminal(true)
                .color_system(Some(ColorSystem::Truecolor))
                .build(),
        }
    }

    fn shot(&self, name: &str, body: impl FnOnce(&Console)) {
        let console = self.console();
        match &self.dir {
            None => {
                console.print_str(&format!("[dim]── {name} ──[/]"));
                body(&console);
            }
            Some(dir) => {
                let id = format!("guide_logging-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

// --8<-- [start:sink]
/// Keeps events instead of printing them: handy in tests.
#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<StructuredEvent>>,
}

impl EventSink for Recorder {
    fn emit(&self, event: StructuredEvent) -> std::io::Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}
// --8<-- [end:sink]

impl Recorder {
    fn take(&self) -> Vec<StructuredEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

// --8<-- [start:install-log]
/// Route the `log` facade to `sink`. The application owns this decision:
/// constructing a `LogAdapter` never installs anything by itself.
fn install_logger(sink: Arc<dyn EventSink>) {
    let logger = Box::leak(Box::new(LogAdapter::new(sink, log::LevelFilter::Debug)));
    log::set_logger(logger).expect("no other logger is installed");
    log::set_max_level(log::LevelFilter::Debug);
}
// --8<-- [end:install-log]

fn log_lines() {
    // --8<-- [start:log-lines]
    log::info!("Server starting on 127.0.0.1:8080");
    log::debug!("GET /index.html 200 in 0.4ms");
    log::warn!("cache miss for key {:?}", "user:42");
    log::error!("POST /upload failed: disk full");
    // --8<-- [end:log-lines]
}

fn tracing_lines() {
    // --8<-- [start:tracing-lines]
    tracing::info!(port = 8080u16, tls = true, "listening");
    tracing::warn!(retries = 3i64, elapsed_ms = 1250.5, "upstream slow");
    tracing::error!(path = "/upload", "request failed");
    // --8<-- [end:tracing-lines]
}

fn main() {
    let shots = Shots::from_args();
    let recorder = Arc::new(Recorder::default());

    // --8<-- [start:handler]
    // `time_format` pins the clock so this output is reproducible; the
    // default is the current UTC time as `[HH:MM:SS]`.
    let handler = RichHandler::new(Console::new()).time_format(|| "[12:00:00]".into());
    // --8<-- [end:handler]

    if shots.svg() {
        install_logger(recorder.clone());
        log_lines();
        shots.shot("log", |console| {
            for event in recorder.take() {
                console.print(&handler.render(&event));
            }
        });
    } else {
        // --8<-- [start:handler-install]
        let handler = Arc::new(handler);
        install_logger(handler.clone());
        // --8<-- [end:handler-install]
        println!("── log ──");
        log_lines();

        println!("── tracing ──");
        // --8<-- [start:tracing]
        use tracing_subscriber::prelude::*;

        let subscriber = tracing_subscriber::registry().with(EventLayer::new(handler.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing_lines();
        });
        // --8<-- [end:tracing]
    }

    if shots.svg() {
        use tracing_subscriber::prelude::*;
        let subscriber = tracing_subscriber::registry().with(EventLayer::new(recorder.clone()));
        tracing::subscriber::with_default(subscriber, tracing_lines);
        let handler = RichHandler::new(Console::new()).time_format(|| "[12:00:01]".into());
        shots.shot("tracing", |console| {
            for event in recorder.take() {
                console.print(&handler.render(&event));
            }
        });
    }

    shots.shot("options", |console| {
        // --8<-- [start:options]
        let handler = RichHandler::new(Console::new())
            .show_time(false)
            .show_path(false)
            .markup(true) // messages are console markup, not literal text
            .keywords(["DEPLOY", "ROLLBACK"]); // replaces the HTTP-method list
        let event =
            StructuredEvent::new(Message::Literal("DEPLOY [bold]v2.4.1[/] to 3 hosts".into()))
                .context(EventContext {
                    severity: Some(Severity::Warn),
                    ..Default::default()
                });
        console.print(&handler.render(&event));
        // --8<-- [end:options]
    });

    shots.shot("links", |console| {
        // --8<-- [start:links]
        use rich_ext::hyperlink::Hyperlinker;

        // Any `Highlighter + Send + Sync` replaces the default ReprHighlighter.
        let linker = Hyperlinker::new().repository("https://github.com/acme/app");
        let handler = RichHandler::new(Console::new())
            .show_time(false)
            .highlighter(Some(Box::new(linker)));
        let event = StructuredEvent::new(Message::Literal(
            "retrying after #42; see src/net.rs:88 and https://acme.dev/status".into(),
        ));
        console.print(&handler.render(&event));
        // --8<-- [end:links]
    });

    shots.shot("event", |console| {
        // --8<-- [start:event]
        let event = StructuredEvent::new(Message::Literal("request finished".into()))
            .field("status", Value::Unsigned(200))
            .field("route", Value::String("/api/items".into()))
            .field("elapsed_ms", Value::Float(12.5))
            .field(
                "tags",
                Value::List(vec![
                    Value::String("cache".into()),
                    Value::String("gzip".into()),
                ]),
            )
            .context(EventContext {
                timestamp: Some("2026-09-23T12:00:00Z".into()),
                severity: Some(Severity::Info),
                target: Some("http".into()),
                source: Some(SourceLocation {
                    path: "src/server.rs".into(),
                    line: 88,
                    column: None,
                }),
                ..Default::default()
            });

        console.print(&event); // compact: one line, fields as key=value
        console.print(
            &event
                .clone()
                .field_order(vec!["route".into()]) // this field first
                .hide_fields(vec!["tags".into()])
                .view(EventView::Expanded), // one field per line
        );
        // --8<-- [end:event]
    });

    shots.shot("event-diagnostic", |console| {
        // --8<-- [start:event-diagnostic]
        use rich_ext::diagnostic::{Diagnostic, Location};

        let event = StructuredEvent::new(Message::Markup("[bold]config reload[/] rejected".into()))
            .context(EventContext {
                severity: Some(Severity::Error),
                ..Default::default()
            })
            .diagnostic(
                Diagnostic::error("unknown key `prot`")
                    .code("CFG002")
                    .location(Location::new("app.toml", Some(4), Some(1))),
            );
        console.print(&event);
        // --8<-- [end:event-diagnostic]
    });
}
