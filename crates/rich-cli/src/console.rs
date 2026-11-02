//! `rich console [--socket PATH] [--trace] [-v] [-x TARGET]...` (0.0.24,
//! after `textual console`): listen on a Unix domain socket and print the
//! `tracing` events an intuiTUIve app sends there (the app calls
//! `intuituive::log::install_console()` and runs with
//! `INTUITUIVE_CONSOLE=1`), so a full-screen app is debugged from a second
//! terminal. Not upstream: a binary-boundary convenience (see
//! `docs/PORTING.md`) that reads the JSON lines `rich log` reads and shows
//! them with rs-rich-ext's event presentation.
//!
//! - The socket is made in a directory only the user can enter (made 0700
//!   when it is missing) and set to 0600; it is never a TCP port.
//! - The default socket is intuiTUIve's: `console.sock` in its state
//!   directory, duplicated here (the CLI does not depend on intuiTUIve) and
//!   kept the same by a test.
//! - Unix only: elsewhere it says so and exits with a usage error.
use super::*;

use rich_ext::cli_doc::{ArgSpec, CommandSpec};

/// The target the app's inspector trace comes with.
const TRACE_TARGET: &str = "intuituive::trace";

/// Whether the command line is `rich console …`.
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("console")
}

/// The command's help: registered with the root spec in `cli_spec`.
pub(super) fn command() -> CommandSpec {
    CommandSpec::new("console")
        .about(
            "Print the tracing events an intuiTUIve app sends from another terminal (like \
             textual console): run the app with INTUITUIVE_CONSOLE=1",
        )
        .usage("console [--socket PATH] [--trace] [-v] [-x TARGET]...")
        .arg(ArgSpec::option("socket").value_name("PATH").help(
            "The Unix socket to listen on (default console.sock in intuiTUIve's state directory)",
        ))
        .arg(
            ArgSpec::flag("trace")
                .help("Show the app's inspector trace too: keys, focus, screens and signal writes"),
        )
        .arg(
            ArgSpec::flag("verbose")
                .short('v')
                .help("Show every event's fields on lines of their own"),
        )
        .arg(
            ArgSpec::option("exclude")
                .short('x')
                .value_name("TARGET")
                .multiple(true)
                .help("Leave out events whose target starts with TARGET (repeatable)"),
        )
        .example(
            "rich console",
            "Listen on the default socket; then INTUITUIVE_CONSOLE=1 cargo run --example demo",
        )
        .example(
            "rich console --trace -x hyper",
            "The app's trace too, without hyper's events",
        )
}

/// `rich console`'s arguments.
#[derive(Debug, Default, PartialEq, Eq)]
struct ConsoleArgs {
    socket: Option<std::path::PathBuf>,
    trace: bool,
    verbose: bool,
    exclude: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Help,
    Run(ConsoleArgs),
}

/// Parse the words after (and the global options before) `console`.
fn parse_args(args: &[String]) -> Result<Parsed, String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "console" {
            break;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            iter.next();
        }
    }
    let mut parsed = ConsoleArgs::default();
    while let Some(arg) = iter.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || -> Result<String, String> {
            match inline.clone() {
                Some(value) => Ok(value),
                None => iter
                    .next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value")),
            }
        };
        match name {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--no-color" | "--color" | "--no-config" => {}
            "--config" | "--profile" | "--report" => {
                value()?;
            }
            "--socket" => parsed.socket = Some(value()?.into()),
            "--trace" => parsed.trace = true,
            "--verbose" | "-v" => parsed.verbose = true,
            "--exclude" | "-x" => parsed.exclude.push(value()?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Parsed::Run(parsed))
}

/// intuiTUIve's state directory, as its `state_dir("intuituive")` makes
/// it (`crates/rich-intuituive/src/state.rs`; a test keeps them the same).
fn state_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let base = if cfg!(windows) {
        var("LOCALAPPDATA").map(PathBuf::from)
    } else {
        let home = var("HOME").map(PathBuf::from);
        if cfg!(target_os = "macos") {
            home.map(|h| h.join("Library/Application Support"))
        } else {
            var("XDG_STATE_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| home.map(|h| h.join(".local/state")))
        }
    };
    base.map(|dir| dir.join("intuituive"))
}

/// Where the app connects unless `INTUITUIVE_CONSOLE` names a path.
fn default_socket() -> Option<std::path::PathBuf> {
    state_dir().map(|dir| dir.join("console.sock"))
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    let no_color = cli_spec::no_color_requested(args);
    let parsed = match parse_args(args) {
        Ok(Parsed::Run(parsed)) => parsed,
        Ok(Parsed::Help) => {
            if let Some(help) = cli_spec::subcommand_help(&["console"], no_color) {
                authoring::out(&format!("{help}\n"));
            }
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            return emit_error(
                json,
                ExitClass::Usage,
                &format!("{message} (try `rich console --help`)"),
            );
        }
    };
    let Some(path) = parsed.socket.clone().or_else(default_socket) else {
        return emit_error(
            json,
            ExitClass::Usage,
            "no state directory for the default socket: give --socket PATH",
        );
    };
    match listen(&path, &parsed, no_color) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => emit_error(json, ExitClass::Usage, &message),
    }
}

/// Whether to show an event read from the socket.
fn shown(event: &serde_json::Value, args: &ConsoleArgs) -> bool {
    let target = event
        .get("target")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if target == TRACE_TARGET && !args.trace {
        return false;
    }
    !args
        .exclude
        .iter()
        .any(|excluded| target.starts_with(excluded.as_str()))
}

/// Bind the socket: its directory made 0700 when missing, a stale socket
/// (nothing listening) replaced, the socket set to 0600.
#[cfg(unix)]
fn bind(path: &std::path::Path) -> Result<std::os::unix::net::UnixListener, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty())
        && !dir.exists()
    {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(format!(
                "a console is listening on {} already",
                path.display()
            ));
        }
        std::fs::remove_file(path)
            .map_err(|e| format!("cannot replace {}: {e}", path.display()))?;
    }
    let listener = UnixListener::bind(path)
        .map_err(|e| format!("cannot listen on {}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("cannot restrict {}: {e}", path.display()))?;
    Ok(listener)
}

/// Listen until interrupted, printing each connection's events as they
/// come, one connection a thread.
#[cfg(unix)]
fn listen(path: &std::path::Path, args: &ConsoleArgs, no_color: bool) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::{Arc, Mutex};
    let listener = bind(path)?;
    eprintln!(
        "rich console: listening on {} (run the app with INTUITUIVE_CONSOLE=1; Ctrl+C ends)",
        path.display()
    );
    let console = Arc::new(Mutex::new(
        crate::console_builder().no_color(no_color).build(),
    ));
    let args = Arc::new(ConsoleArgs {
        socket: None,
        trace: args.trace,
        verbose: args.verbose,
        exclude: args.exclude.clone(),
    });
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let (console, args) = (console.clone(), args.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                if !shown(&json, &args) {
                    continue;
                }
                let mut event = super::structured_log::event(&json);
                if args.verbose {
                    event = event.view(rich_ext::event::EventView::Expanded);
                }
                let console = console.lock().unwrap_or_else(|e| e.into_inner());
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(console.render_export(&event).as_bytes());
                let _ = out.flush();
            }
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn listen(_path: &std::path::Path, _args: &ConsoleArgs, _no_color: bool) -> Result<(), String> {
    Err("rich console needs a Unix domain socket, which this platform's std does not have".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn arguments_parse() {
        assert_eq!(
            parse_args(&words(
                "console --socket /tmp/s --trace -v -x hyper --exclude=h2"
            )),
            Ok(Parsed::Run(ConsoleArgs {
                socket: Some("/tmp/s".into()),
                trace: true,
                verbose: true,
                exclude: vec!["hyper".into(), "h2".into()],
            }))
        );
        assert_eq!(parse_args(&words("console --help")), Ok(Parsed::Help));
        assert!(parse_args(&words("console --nope")).is_err());
        assert!(requested(&words("console")));
    }

    #[test]
    fn the_trace_and_excluded_targets_are_left_out() {
        let event = |target: &str| serde_json::json!({"level": "info", "target": target});
        let plain = ConsoleArgs::default();
        assert!(shown(&event("app"), &plain));
        assert!(!shown(&event(TRACE_TARGET), &plain));
        let traced = ConsoleArgs {
            trace: true,
            exclude: vec!["hyper".into()],
            ..ConsoleArgs::default()
        };
        assert!(shown(&event(TRACE_TARGET), &traced));
        assert!(!shown(&event("hyper::client"), &traced));
    }

    /// The default socket is intuiTUIve's: the same state directory rules
    /// and file name as `state_dir` and `default_console_socket` there.
    #[test]
    fn the_default_socket_is_intuituives() {
        const STATE: &str = include_str!("../../rich-intuituive/src/state.rs");
        const CONSOLE: &str = include_str!("../../rich-intuituive/src/log/console.rs");
        for rule in [
            "var(\"LOCALAPPDATA\")",
            "h.join(\"Library/Application Support\")",
            "var(\"XDG_STATE_HOME\")",
            ".filter(|p| p.is_absolute())",
            "h.join(\".local/state\")",
        ] {
            assert!(
                STATE.contains(rule),
                "intuiTUIve's state_dir changed: {rule}"
            );
        }
        assert!(CONSOLE.contains("state_file(\"intuituive\", \"console.sock\")"));
        assert!(CONSOLE.contains("pub const TRACE_TARGET: &str = \"intuituive::trace\";"));
        assert!(default_socket().is_none_or(|path| path.ends_with("intuituive/console.sock")));
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_is_the_users_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rich-console-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("state/console.sock");
        let listener = bind(&path).unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir.join("state")), 0o700);
        // A second console finds the first listening.
        assert!(bind(&path).unwrap_err().contains("already"));
        drop(listener);
        // A stale socket is replaced.
        assert!(bind(&path).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
