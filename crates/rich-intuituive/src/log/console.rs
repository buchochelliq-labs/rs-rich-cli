//! A log console in a second terminal (the `tracing` plugin): Textual's
//! `textual console`. An app calls [`install_console`]; when
//! `INTUITUIVE_CONSOLE` is set it connects to the socket `rich console`
//! listens on and sends its `tracing` events there as JSON lines (the
//! records `rich log` reads), so a full-screen app is debugged without
//! writing over its own screen.
//!
//! - The socket is a Unix domain socket, never a TCP port: `rich console`
//!   makes it in a directory only the user can use, readable and writable
//!   by the user alone (mode 0600), and drops another user's connection.
//! - The app connects only to a socket of its own user: a symbolic link,
//!   something that is not a socket, or another user's socket or listener
//!   (by the peer's credentials, where the platform has them) is refused.
//! - With the variable unset (or `0`), nothing is opened.
//! - The inspector's trace (keys, focus, signal writes) is sent too, at
//!   `TRACE` with the target `intuituive::trace`, while no inspector shows
//!   it; `rich console --trace` shows it.
//! - Unix only for now: on Windows, where std has no local socket,
//!   [`install_console`] does nothing (a named pipe is a follow-up).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rich_ext::adapters::{EventLayer, EventSink};
use rich_ext::event::{StructuredEvent, Value};
use rich_ext::log_stream::Record;

/// The variable that turns the console on: `1` for the
/// [default socket](default_console_socket), else the socket's path.
pub const CONSOLE_VAR: &str = "INTUITUIVE_CONSOLE";

/// The target the inspector's trace is sent with.
pub const TRACE_TARGET: &str = "intuituive::trace";

/// Events that may wait for the socket before more are dropped.
#[cfg(unix)]
const BACKLOG: usize = 4_096;

/// Whether an app's trace is sent to a console.
static TRACING: AtomicBool = AtomicBool::new(false);

/// Whether the inspector's trace goes to a console.
pub(crate) fn console_tracing() -> bool {
    TRACING.load(Ordering::Relaxed)
}

/// Where `rich console` listens unless told otherwise:
/// `console/console.sock` in intuiTUIve's [state directory](crate::state_dir)
/// (`$XDG_STATE_HOME/intuituive`, else `~/.local/state/intuituive`;
/// `~/Library/Application Support/intuituive` on macOS), in a directory of
/// its own that only the user can use.
pub fn default_console_socket() -> Option<PathBuf> {
    crate::state::state_file("intuituive", "console").map(|dir| dir.join("console.sock"))
}

/// The socket [`CONSOLE_VAR`] names: `None` when it is unset, empty or
/// `0`; the [default](default_console_socket) for `1`; else the path it
/// holds.
pub fn console_socket() -> Option<PathBuf> {
    let value = std::env::var_os(CONSOLE_VAR)?;
    match value.to_str() {
        Some("" | "0") => None,
        Some("1") => default_console_socket(),
        _ => Some(PathBuf::from(value)),
    }
}

/// Connect to the console listening at `path` and send it the app's
/// `tracing` events: a layer for your own subscriber. Events are written
/// by a thread of their own; those that find a full backlog are dropped,
/// so the thread that logs never waits for the console. An error where
/// nothing listens there, where the path is not a socket of this user's
/// (a symbolic link is refused) or its listener is another user's, and
/// off Unix.
pub fn console_layer(path: &Path) -> io::Result<EventLayer> {
    let sink = ConsoleSink::connect(path)?;
    TRACING.store(true, Ordering::Relaxed);
    Ok(EventLayer::new(std::sync::Arc::new(sink)))
}

/// Send the app's `tracing` events to `rich console` when
/// [`CONSOLE_VAR`] is set: a subscriber of the [console
/// layer](console_layer) alone becomes the global default. `Ok(false)`
/// (and nothing opened) when the variable is unset, and off Unix; an error
/// when nothing listens on the socket or a global subscriber is set
/// already (then add [`console_layer`] to yours).
///
/// ```no_run
/// use intuituive::prelude::*;
///
/// intuituive::log::install_console().ok();
/// App::new(|| label("hello").on_key("l", |cx| cx.log("pressed l"))).run().unwrap();
/// ```
pub fn install_console() -> io::Result<bool> {
    if !cfg!(unix) {
        return Ok(false);
    }
    let Some(path) = console_socket() else {
        return Ok(false);
    };
    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry().with(console_layer(&path)?);
    tracing::subscriber::set_global_default(subscriber).map_err(io::Error::other)?;
    Ok(true)
}

/// Where events go: a bounded queue a writer thread empties into the
/// socket.
struct ConsoleSink {
    queue: std::sync::mpsc::SyncSender<String>,
}

impl ConsoleSink {
    #[cfg(unix)]
    fn connect(path: &Path) -> io::Result<ConsoleSink> {
        use std::io::Write;
        let mut stream = connect_own(path)?;
        let (queue, lines) = std::sync::mpsc::sync_channel::<String>(BACKLOG);
        std::thread::Builder::new()
            .name("intuituive-console".into())
            .spawn(move || {
                for line in lines {
                    // The console went away: stop sending.
                    if stream.write_all(line.as_bytes()).is_err() {
                        TRACING.store(false, Ordering::Relaxed);
                        break;
                    }
                }
            })?;
        Ok(ConsoleSink { queue })
    }

    #[cfg(not(unix))]
    fn connect(_path: &Path) -> io::Result<ConsoleSink> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the log console needs a Unix domain socket",
        ))
    }
}

/// This process's effective user.
#[cfg(unix)]
#[allow(unsafe_code)] // geteuid has no preconditions and cannot fail.
fn current_uid() -> u32 {
    // SAFETY: as above.
    unsafe { libc::geteuid() }
}

/// The user at the other end of `stream`, where the platform says.
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

/// Connect to `path` only when it is a socket of this user's (not a
/// symbolic link) and the listener is this user's, where the platform
/// says: a console another user set up never receives the app's events.
#[cfg(unix)]
fn connect_own(path: &Path) -> io::Result<std::os::unix::net::UnixStream> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let refused = |why: String| io::Error::new(io::ErrorKind::PermissionDenied, why);
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(refused(format!("{} is a symbolic link", path.display())));
    }
    if !meta.file_type().is_socket() || meta.uid() != current_uid() {
        return Err(refused(format!(
            "{} is not a socket of this user's",
            path.display()
        )));
    }
    let stream = std::os::unix::net::UnixStream::connect(path)?;
    match peer_uid(&stream) {
        Some(uid) if uid != current_uid() => Err(refused(format!(
            "the console at {} is another user's",
            path.display()
        ))),
        _ => Ok(stream),
    }
}

impl EventSink for ConsoleSink {
    fn emit(&self, event: StructuredEvent) -> io::Result<()> {
        let mut line = json_line(&Record::from_event(&event));
        line.push('\n');
        self.queue
            .try_send(line)
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "the console's backlog is full"))
    }
}

/// A record as a JSON object on one line, its fields in order.
pub(crate) fn json_line(record: &Record) -> String {
    let mut out = String::from("{");
    for (i, (key, value)) in record.fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_string(&mut out, key);
        out.push(':');
        json_value(&mut out, value);
    }
    out.push('}');
    out
}

fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Integer(n) => out.push_str(&n.to_string()),
        Value::Unsigned(n) => out.push_str(&n.to_string()),
        Value::Integer128(n) => out.push_str(&n.to_string()),
        Value::Unsigned128(n) => out.push_str(&n.to_string()),
        Value::Float(f) if f.is_finite() => out.push_str(&format!("{f:?}")),
        Value::Float(_) => out.push_str("null"),
        Value::String(text) => json_string(out, text),
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_value(out, item);
            }
            out.push(']');
        }
        Value::Map(entries) => {
            out.push('{');
            for (i, (key, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_string(out, key);
                out.push(':');
                json_value(out, item);
            }
            out.push('}');
        }
    }
}

/// The inspector's trace, as `tracing` events for the console.
pub(crate) fn send_trace(entries: Vec<(crate::inspect::TraceKind, String)>) {
    for (kind, text) in entries {
        tracing::trace!(target: "intuituive::trace", kind = ?kind, "{text}");
    }
}

impl crate::app::Ctx {
    /// Log `message` as a `tracing` event at `DEBUG`, with the target
    /// `app`: what Textual's `self.log(..)` does, shown by `rich console`
    /// (see [`install_console`]) or a [`Log`](crate::Log) fed by
    /// [`install_tracing`](crate::log::install_tracing). The `tracing`
    /// plugin.
    pub fn log(&mut self, message: impl std::fmt::Display) {
        tracing::debug!(target: "app", "{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn only_a_socket_of_this_users_is_connected_to() {
        let dir = std::env::temp_dir().join(format!("intuituive-console-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("console.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        // The user's own socket and listener.
        let stream = connect_own(&path).unwrap();
        let (server, _) = listener.accept().unwrap();
        assert_eq!(peer_uid(&stream).unwrap_or(current_uid()), current_uid());
        assert_eq!(peer_uid(&server).unwrap_or(current_uid()), current_uid());
        // A symbolic link to it is refused.
        let link = dir.join("link.sock");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let error = connect_own(&link).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        // So is a file that is not a socket.
        let file = dir.join("file");
        std::fs::write(&file, "").unwrap();
        assert!(
            connect_own(&file)
                .unwrap_err()
                .to_string()
                .contains("not a socket")
        );
        drop((stream, server, listener));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_socket_has_a_directory_of_its_own() {
        assert!(
            default_console_socket()
                .is_none_or(|path| path.ends_with("intuituive/console/console.sock"))
        );
    }

    #[test]
    fn records_are_json_lines() {
        let record = Record {
            fields: vec![
                ("level".into(), Value::String("INFO".into())),
                ("message".into(), Value::String("say \"hi\"\n".into())),
                ("n".into(), Value::Integer(-3)),
                ("x".into(), Value::Float(1.5)),
                ("ok".into(), Value::Bool(true)),
                (
                    "list".into(),
                    Value::List(vec![Value::Null, Value::Unsigned(2)]),
                ),
            ],
            format: "json".into(),
            continuation: Vec::new(),
        };
        assert_eq!(
            json_line(&record),
            r#"{"level":"INFO","message":"say \"hi\"\n","n":-3,"x":1.5,"ok":true,"list":[null,2]}"#
        );
    }
}
