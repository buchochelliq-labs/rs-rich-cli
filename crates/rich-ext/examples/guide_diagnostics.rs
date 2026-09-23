//! Guide: Diagnostics — run: cargo run -p rs-rich-ext --example guide_diagnostics --features anyhow [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/diagnostics.md` come from this file. With
//! `--svg DIR` every shot is written as `DIR/guide_diagnostics-<shot>.svg`.
//! `--panic` installs the rich panic hook and panics, to show it for real.

use std::path::PathBuf;

use rich::{ColorSystem, Console};
use rich_ext::diagnostic::{
    Diagnostic, DiagnosticInfo, Level, Location, SourceSnippet, Suggestion,
};
use rich_ext::event::EventView;
use rich_ext::hyperlink::Hyperlinker;
use rich_ext::stacktrace;

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

    fn shot(&self, name: &str, width: usize, body: impl FnOnce(&Console)) {
        match &self.dir {
            None => {
                let console = Console::new();
                console.print_str(&format!("[dim]── {name} ──[/]"));
                body(&console);
            }
            Some(dir) => {
                let console = Console::builder()
                    .width(width)
                    .force_terminal(true)
                    .color_system(Some(ColorSystem::Truecolor))
                    .build();
                let id = format!("guide_diagnostics-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

// --8<-- [start:info]
#[derive(Debug, thiserror::Error)]
enum ConfigError {
    #[error("port {0:?} is not a number")]
    BadPort(String),
    #[error("cannot read the config")]
    Read(#[from] std::io::Error),
}

impl DiagnosticInfo for ConfigError {
    fn code(&self) -> Option<String> {
        Some(match self {
            ConfigError::BadPort(_) => "C001".into(),
            ConfigError::Read(_) => "C002".into(),
        })
    }
    fn code_url(&self) -> Option<String> {
        self.code()
            .map(|code| format!("https://acme.dev/errors/{code}"))
    }
    fn help(&self) -> Option<String> {
        matches!(self, ConfigError::BadPort(_)).then(|| "use a number such as 8080".into())
    }
    fn location(&self) -> Option<Location> {
        Some(Location::new("app.toml", Some(2), None))
    }
}
// --8<-- [end:info]

// --8<-- [start:parser]
use rich_ext::stacktrace::{Frame, Language, Parsers, StackTrace, TraceParser};

/// Traces of the form `tiny error: message`, then `  at function (path:line)` lines.
struct TinyParser;

impl TraceParser for TinyParser {
    fn detect(&self, text: &str) -> bool {
        text.starts_with("tiny error: ")
    }

    fn parse(&self, text: &str) -> Option<StackTrace> {
        let mut lines = text.lines();
        let mut trace = StackTrace::new(Language::Other("tiny".into()));
        trace.kind = Some("tiny error".into());
        trace.message = lines.next()?.strip_prefix("tiny error: ").map(Into::into);
        for line in lines {
            let (function, place) = line.trim().strip_prefix("at ")?.split_once(" (")?;
            let (path, line) = place.trim_end_matches(')').rsplit_once(':')?;
            trace.frames.push(Frame {
                function: Some(function.into()),
                path: Some(path.into()),
                line: line.parse().ok(),
                ..Frame::default()
            });
        }
        trace.frames.reverse(); // most recent call last, like every parser
        Some(trace)
    }
}
// --8<-- [end:parser]

// --8<-- [start:panic-hook]
fn install_panic_hook() {
    let console = Console::builder()
        .force_terminal(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .build();
    std::panic::set_hook(Box::new(stacktrace::panic_hook(console)));
}
// --8<-- [end:panic-hook]

const PYTHON_TRACE: &str = r#"Traceback (most recent call last):
  File "/srv/app/app.py", line 6, in main
    load()
  File "/srv/app/app.py", line 3, in load
    return {}["port"]
           ~~^^^^^^^^
KeyError: 'port'

The above exception was the direct cause of the following exception:

Traceback (most recent call last):
  File "/srv/app/app.py", line 11, in outer
    main()
  File "/srv/app/app.py", line 8, in main
    raise ValueError("bad config") from e
ValueError: bad config
"#;

const RUST_PANIC: &str = "thread 'main' panicked at src/main.rs:14:10:
port must be a number: ParseIntError { kind: InvalidDigit }
stack backtrace:
   0: __rustc::rust_begin_unwind
             at /rustc/0000000000000000000000000000000000000000/library/std/src/panicking.rs:679:5
   1: core::panicking::panic_fmt
             at /rustc/0000000000000000000000000000000000000000/library/core/src/panicking.rs:80:14
   2: core::result::unwrap_failed
             at /rustc/0000000000000000000000000000000000000000/library/core/src/result.rs:1870:5
   3: app::parse_port
             at ./src/main.rs:14:10
   4: app::main
             at ./src/main.rs:3:16
   5: core::ops::function::FnOnce::call_once
             at /rustc/0000000000000000000000000000000000000000/library/core/src/ops/function.rs:250:5
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
";

fn main() {
    if std::env::args().any(|arg| arg == "--panic") {
        install_panic_hook();
        let port: u16 = "eighty".parse().expect("port must be a number");
        println!("{port}");
    }
    let shots = Shots::from_args();

    shots.shot("quickstart", 70, |console| {
        // --8<-- [start:quickstart]
        let diagnostic = Diagnostic::error("mismatched types")
            .code("E0308")
            .location(Location::new("src/main.rs", Some(12), Some(5)))
            .cause("expected `u16`, found `&str`");
        console.print(&diagnostic);
        // --8<-- [end:quickstart]
    });

    shots.shot("levels", 70, |console| {
        // --8<-- [start:levels]
        console.print(&Diagnostic::error("cannot open `app.toml`"));
        console.print(&Diagnostic::warning("`timeout` is deprecated"));
        console.print(&Diagnostic::new("using 4 worker threads").level(Level::Info));
        console.print(&Diagnostic::new("defaults came from /etc/acme").level(Level::Note));
        console.print(&Diagnostic::new("run `acme check` to validate").level(Level::Help));
        // No level: the message alone, styled `diagnostic.message`.
        console.print(&Diagnostic::new("config rejected").code("CFG"));
        // --8<-- [end:levels]
    });

    shots.shot("snippet", 70, |console| {
        // --8<-- [start:snippet]
        let source = "[server]\nport = invalid\nhost = \"localhost\"\n";
        let value = source.find("invalid").unwrap();
        let key = source.find("port").unwrap();

        let snippet = SourceSnippet::new("config.toml".into(), source.into(), value..value + 7, 1)
            .expect("span on UTF-8 boundaries")
            .primary_label("not a number")
            .secondary(key..key + 4, "for this key")
            .expect("span on UTF-8 boundaries");
        let fix = Suggestion::replace("for example", source, value..value + 7, "8080")
            .expect("span on UTF-8 boundaries");

        let diagnostic = Diagnostic::error("invalid endpoint")
            .code("CFG001")
            .code_url("https://acme.dev/errors/CFG001")
            .location(snippet.location())
            .cause("port must be numeric")
            .snippet(snippet)
            .note("ports below 1024 need privileges")
            .help("use a port from 1 to 65535")
            .suggestion(fix)
            .hyperlinker(Hyperlinker::new())
            .view(EventView::Expanded); // show everything below the header
        console.print(&diagnostic);
        // --8<-- [end:snippet]
    });

    shots.shot("info", 70, |console| {
        // --8<-- [start:info-use]
        let error = ConfigError::BadPort("eighty".into());
        console.print(&error.to_diagnostic().view(EventView::Expanded));

        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        console.print(&ConfigError::from(io).to_diagnostic());
        // --8<-- [end:info-use]
    });

    shots.shot("anyhow", 70, |console| {
        // --8<-- [start:anyhow]
        use anyhow::Context;

        let error = std::fs::read_to_string("/etc/acme/missing.toml")
            .context("reading the config")
            .context("starting the server")
            .unwrap_err();
        console.print(&Diagnostic::from_anyhow(&error, 8));
        // --8<-- [end:anyhow]
    });

    shots.shot("trace-python", 70, |console| {
        // --8<-- [start:trace-python]
        let trace = stacktrace::parse(PYTHON_TRACE).expect("a Python traceback");
        assert_eq!(trace.chain().count(), 2); // ValueError, caused by KeyError
        let origin = trace.origin().expect("an application frame");
        assert_eq!(origin.function.as_deref(), Some("main"));
        console.print(&trace);
        // --8<-- [end:trace-python]
    });

    shots.shot("trace-rust", 70, |console| {
        // --8<-- [start:trace-rust]
        let trace = stacktrace::parse(RUST_PANIC).expect("a Rust panic");
        console.print(&trace);
        // Every frame, and plain paths instead of links:
        let _all = trace
            .render_options()
            .show_library(true)
            .hyperlinker(Hyperlinker::disabled());
        // --8<-- [end:trace-rust]
    });

    shots.shot("trace-custom", 70, |console| {
        // --8<-- [start:parser-use]
        let parsers = Parsers::new().with_parser(TinyParser);
        let text =
            "tiny error: out of cheese\n  at brew (src/pot.tiny:9)\n  at main (src/main.tiny:2)";
        let trace = parsers.parse(text).expect("TinyParser detects it");
        let diagnostic = Diagnostic::error("worker crashed")
            .trace(trace)
            .view(EventView::Expanded); // traces show in the expanded view
        console.print(&diagnostic);
        // --8<-- [end:parser-use]
    });

    shots.shot("dashboard", 70, |console| {
        // --8<-- [start:dashboard]
        use rich_ext::dashboard::DiagnosticsDashboard;

        let at = |path: &str, line, column| Location::new(path, Some(line), Some(column));
        let mut dashboard = DiagnosticsDashboard::new().top_codes(3);
        dashboard
            .push(
                Diagnostic::error("mismatched types")
                    .code("E0308")
                    .location(at("src/main.rs", 12, 5)),
            )
            .push(
                Diagnostic::warning("unused variable `x`")
                    .code("W1")
                    .location(at("src/main.rs", 3, 9)),
            )
            .push(
                Diagnostic::warning("unused import")
                    .code("W1")
                    .location(at("src/lib.rs", 1, 5)),
            )
            .push(Diagnostic::new("see the migration guide").level(Level::Note));
        console.print(&dashboard);
        // --8<-- [end:dashboard]
    });
}
