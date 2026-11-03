//! `rich console [--socket PATH] [--trace] [-v] [-x TARGET]...` (0.0.24,
//! after `textual console`): listen on a Unix domain socket and print the
//! `tracing` events an intuiTUIve app sends there (the app calls
//! `intuituive::log::install_console()` and runs with
//! `INTUITUIVE_CONSOLE=1`), so a full-screen app is debugged from a second
//! terminal. Not upstream: a binary-boundary convenience (see
//! `docs/PORTING.md`) that reads the JSON lines `rich log` reads and shows
//! them with rs-rich-ext's event presentation.
//!
//! - The socket is only ever the user's: its directory must be the user's
//!   alone (made 0700 when it is missing, refused otherwise), the socket is
//!   created 0600, a symbolic link or anything but a stale socket at the
//!   path is refused, and a connection from another user is dropped
//!   (checked by the peer's credentials where the platform has them). It
//!   is never a TCP port.
//! - The default socket is intuiTUIve's: `console/console.sock` in its
//!   state directory, duplicated here (the CLI does not depend on
//!   intuiTUIve) and kept the same by a test.
//! - Unix only for now: elsewhere it says so and exits with a usage error
//!   (a Windows named pipe is a follow-up).
use super::*;

use rich_ext::cli_doc::{ArgSpec, CommandSpec};

/// The target the app's inspector trace comes with.
#[cfg_attr(not(unix), allow(dead_code))]
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
            "The Unix socket to listen on, in a directory only you can use (default \
             console/console.sock in intuiTUIve's state directory)",
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

/// Where the app connects unless `INTUITUIVE_CONSOLE` names a path: in a
/// directory of its own, which only the user can use.
fn default_socket() -> Option<std::path::PathBuf> {
    state_dir().map(|dir| dir.join("console").join("console.sock"))
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
#[cfg_attr(not(unix), allow(dead_code))]
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

/// This process's effective user.
#[cfg(unix)]
#[allow(unsafe_code)] // geteuid has no preconditions and cannot fail.
fn current_uid() -> u32 {
    // SAFETY: as above.
    unsafe { libc::geteuid() }
}

/// The user at the other end of `stream`, where the platform says:
/// `SO_PEERCRED` on Linux and Android, `getpeereid` on the BSDs and macOS;
/// `None` elsewhere (the directory's mode is then what keeps others out).
#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(unsafe_code)] // getsockopt into a ucred of the size it is told.
fn peer_uid(stream: &std::os::unix::net::UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the fd is open for the call; cred and len point to writable
    // memory of the size len says.
    let done = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (done == 0 && len as usize == std::mem::size_of::<libc::ucred>()).then_some(cred.uid)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
#[allow(unsafe_code)] // getpeereid into two writable ids.
fn peer_uid(stream: &std::os::unix::net::UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (libc::uid_t::MAX, libc::gid_t::MAX);
    // SAFETY: the fd is open for the call; both pointers are writable.
    let done = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    (done == 0).then_some(uid)
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
))]
fn peer_uid(_stream: &std::os::unix::net::UnixStream) -> Option<u32> {
    None
}

/// The socket's directory: made (with any missing parents) 0700 when it is
/// missing; else it must be a real directory, the user's, that nobody else
/// can enter or write. Nothing is loosened or tightened: a directory that
/// fails is refused, with how to fix it.
#[cfg(unix)]
fn private_dir(dir: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    if std::fs::symlink_metadata(dir).is_err() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let meta = std::fs::symlink_metadata(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let shown = dir.display();
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "{shown} is not a directory (a symbolic link is refused): give --socket a path in a \
             directory only you can use"
        ));
    }
    if meta.uid() != current_uid() || meta.mode() & 0o077 != 0 {
        return Err(format!(
            "{shown} must be a directory only you can use (yours, mode 700): run `chmod 700 \
             {shown}`, or give --socket a path in such a directory"
        ));
    }
    Ok(())
}

/// Bind the socket in a [private directory](private_dir): a symbolic link
/// at the path is refused, and so is anything but a stale socket of the
/// user's (which is replaced); the socket is made 0600 and checked. The
/// directory is checked to be the user's alone first, so no one else can
/// reach the socket before its mode is set. (The umask is not changed: it
/// is the whole process's, and other threads create files meanwhile.)
#[cfg(unix)]
fn bind(path: &std::path::Path) -> Result<std::os::unix::net::UnixListener, String> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    private_dir(dir)?;
    let shown = path.display();
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(format!("{shown} is a symbolic link: refused"));
        }
        if !meta.file_type().is_socket() || meta.uid() != current_uid() {
            return Err(format!(
                "{shown} is not a socket of yours: refused (remove it, or give another --socket)"
            ));
        }
        if UnixStream::connect(path).is_ok() {
            return Err(format!("a console is listening on {shown} already"));
        }
        std::fs::remove_file(path).map_err(|e| format!("cannot replace {shown}: {e}"))?;
    }
    let listener =
        UnixListener::bind(path).map_err(|e| format!("cannot listen on {shown}: {e}"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("cannot make {shown} private: {e}"))?;
    let mode = std::fs::symlink_metadata(path)
        .map(|meta| meta.permissions().mode() & 0o777)
        .map_err(|e| format!("cannot read {shown}: {e}"))?;
    if mode != 0o600 {
        drop(listener);
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "{shown} was created with mode {mode:o}, not 600: refused"
        ));
    }
    Ok(listener)
}

/// Whether a connection is the user's own: its peer's user, where the
/// platform says, is this process's.
#[cfg(unix)]
fn accepted(stream: &std::os::unix::net::UnixStream) -> Result<(), u32> {
    peer_allowed(peer_uid(stream), current_uid())
}

/// A peer of user `peer` (`None`: the platform cannot say) is let in by a
/// console of user `me` when they are the same.
#[cfg(unix)]
fn peer_allowed(peer: Option<u32>, me: u32) -> Result<(), u32> {
    match peer {
        Some(uid) if uid != me => Err(uid),
        _ => Ok(()),
    }
}

/// Listen until interrupted, printing each connection's events as they
/// come, one connection a thread; another user's connection is dropped.
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
        if accepted(&stream).is_err() {
            eprintln!("rich console: refused a connection from another user");
            continue;
        }
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
    Err(
        "rich console is Unix only for now: it listens on a Unix domain socket (a Windows \
         named pipe is planned)"
            .into(),
    )
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
        assert!(CONSOLE.contains("state_file(\"intuituive\", \"console\")"));
        assert!(CONSOLE.contains(".map(|dir| dir.join(\"console.sock\"))"));
        assert!(CONSOLE.contains("pub const TRACE_TARGET: &str = \"intuituive::trace\";"));
        assert!(
            default_socket().is_none_or(|path| path.ends_with("intuituive/console/console.sock"))
        );
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
        // The user's own connection is let in, by its credentials.
        let client = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let (server, _) = listener.accept().unwrap();
        assert_eq!(peer_uid(&server).unwrap_or(current_uid()), current_uid());
        assert!(accepted(&server).is_ok());
        drop((client, server));
        // A second console finds the first listening.
        assert!(bind(&path).unwrap_err().contains("already"));
        drop(listener);
        // A stale socket is replaced.
        assert!(bind(&path).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn another_users_connection_is_refused() {
        assert_eq!(peer_allowed(Some(1000), 1000), Ok(()));
        assert_eq!(peer_allowed(Some(1001), 1000), Err(1001));
        // Where the platform cannot say, the directory keeps others out.
        assert_eq!(peer_allowed(None, 1000), Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn links_files_and_shared_directories_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("rich-console-refused-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let private = dir.join("private");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        // A symbolic link at the socket's path, even to a place of the
        // user's, is refused and left alone.
        let target = private.join("elsewhere");
        let link = private.join("console.sock");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(bind(&link).unwrap_err().contains("symbolic link"));
        assert!(std::fs::symlink_metadata(&link).is_ok());
        assert!(!target.exists());
        std::fs::remove_file(&link).unwrap();
        // A file that is not a socket is never removed.
        std::fs::write(&link, "notes").unwrap();
        assert!(bind(&link).unwrap_err().contains("not a socket"));
        assert_eq!(std::fs::read_to_string(&link).unwrap(), "notes");
        std::fs::remove_file(&link).unwrap();
        // A directory others can enter or write is refused, not changed.
        let shared = dir.join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            bind(&shared.join("console.sock"))
                .unwrap_err()
                .contains("chmod 700")
        );
        let mode = std::fs::metadata(&shared).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
        // A directory that is a link is refused too.
        let linked = dir.join("linked");
        std::os::unix::fs::symlink(&private, &linked).unwrap();
        assert!(
            bind(&linked.join("console.sock"))
                .unwrap_err()
                .contains("symbolic link")
        );
        // In a private directory, it binds 0600.
        let listener = bind(&private.join("console.sock")).unwrap();
        let mode = std::fs::metadata(private.join("console.sock"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
