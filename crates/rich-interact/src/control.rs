//! A control socket (#816): a running program answers requests from
//! scripts and agents of the same user, one JSON line in and one JSON line
//! out per connection. Behind the off-by-default `control` feature, and
//! Unix only for now (a named pipe on Windows is not written yet).
//!
//! The program names its [`Verb`]s, says which of them change anything
//! ([`Verb::mutates`]), binds a [`ControlServer`] and [`serve`]s it with a
//! handler; a client [`call`]s it. The protocol:
//!
//! - **A request** is `{"verb": "open", "args": {…}}` and a newline.
//! - **A reply** is `{"ok": true, "result": …}` or
//!   `{"ok": false, "error": "…"}` and a newline. Then the connection
//!   closes.
//!
//! **What is protected, and how:**
//!
//! - **Only this user reaches the socket.** It lives in a directory of its
//!   own that is this user's, with mode 0700, made before the socket is
//!   bound ([`private_dir`], [`runtime_dir`]); an existing directory that
//!   is anyone else's, or open to others, or a symbolic link, is refused.
//!   Where the platform tells (Linux's `SO_PEERCRED`, the BSDs' and
//!   macOS's `getpeereid`), each connection's peer must be this user too,
//!   and a client checks that the server is.
//! - **The path is never followed.** A symbolic link where the socket or
//!   its directory should be is refused, by the server and the client.
//! - **Read-only sessions** ([`ControlServer::read_only`]) refuse every
//!   verb that mutates, before the handler sees it. A verb the program did
//!   not name is refused too.
//! - **Bounded:** a request is at most [`MAX_REQUEST`] bytes and a reply
//!   [`MAX_REPLY`]; reads and writes time out ([`IO_TIMEOUT`]); at most
//!   [`MAX_CONNECTIONS`] are answered at once, and others are closed.
//! - **No network listener:** a Unix socket, nothing else.
//!
//! ```no_run
//! # #[cfg(unix)] {
//! use rich_interact::control::{self, ControlServer, Request, Verb};
//! use serde_json::json;
//!
//! const VERBS: &[Verb] = &[Verb::read("info", "who is here"), Verb::write("open", "open a URL")];
//! let dir = control::runtime_dir("my-app")?;
//! let server = ControlServer::bind(dir.join("my-app-1.sock"), VERBS)?.read_only(true);
//! std::thread::spawn(move || server.serve(|request: &Request| match request.verb.as_str() {
//!     "info" => Ok(json!({"name": "my-app"})),
//!     _ => Err("not here".into()),
//! }));
//! let reply = control::call(&dir.join("my-app-1.sock"), &Request::new("info", json!({})), control::IO_TIMEOUT)?;
//! assert_eq!(reply.unwrap()["name"], "my-app");
//! # }
//! # Ok::<(), std::io::Error>(())
//! ```

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

/// The most bytes a request may have, its newline included.
pub const MAX_REQUEST: u64 = 64 * 1024;
/// The most bytes a reply may have, its newline included.
pub const MAX_REPLY: u64 = 16 << 20;
/// How long a read or a write on a connection may wait.
pub const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// The most connections answered at once; more are closed unanswered.
pub const MAX_CONNECTIONS: usize = 8;

/// A verb a program answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Verb {
    /// Its name in requests: `open`, `snapshot`.
    pub name: &'static str,
    /// Whether it changes anything (a read-only session refuses it).
    pub mutates: bool,
    /// What it does, for people.
    pub about: &'static str,
}

impl Verb {
    /// A verb that only reads.
    pub const fn read(name: &'static str, about: &'static str) -> Verb {
        Verb {
            name,
            mutates: false,
            about,
        }
    }

    /// A verb that changes something.
    pub const fn write(name: &'static str, about: &'static str) -> Verb {
        Verb {
            name,
            mutates: true,
            about,
        }
    }
}

/// A request: a verb and its arguments (an object, or null).
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub verb: String,
    pub args: Value,
}

impl Request {
    pub fn new(verb: impl Into<String>, args: Value) -> Request {
        Request {
            verb: verb.into(),
            args,
        }
    }

    /// The request as its line, newline included.
    pub fn to_line(&self) -> String {
        let mut line = json!({"verb": self.verb, "args": self.args}).to_string();
        line.push('\n');
        line
    }

    /// A request from its line. Fails for anything but an object with a
    /// string `verb` (and `args`, when given, an object or null).
    pub fn parse(line: &str) -> io::Result<Request> {
        let value: Value = serde_json::from_str(line.trim()).map_err(|e| invalid(e.to_string()))?;
        let verb = value
            .get("verb")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("a request needs a string \"verb\""))?;
        let args = value.get("args").cloned().unwrap_or(Value::Null);
        if !(args.is_object() || args.is_null()) {
            return Err(invalid("\"args\" must be an object"));
        }
        Ok(Request::new(verb, args))
    }
}

/// What a handler answers: a result, or why not.
pub type Reply = Result<Value, String>;

/// A reply as its line, newline included.
pub fn reply_line(reply: &Reply) -> String {
    let mut line = match reply {
        Ok(result) => json!({"ok": true, "result": result}),
        Err(error) => json!({"ok": false, "error": error}),
    }
    .to_string();
    line.push('\n');
    line
}

/// A reply from its line.
pub fn parse_reply(line: &str) -> io::Result<Reply> {
    let value: Value = serde_json::from_str(line.trim()).map_err(|e| invalid(e.to_string()))?;
    match value.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(Ok(value.get("result").cloned().unwrap_or(Value::Null))),
        Some(false) => Ok(Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("refused")
            .to_string())),
        None => Err(invalid("a reply needs \"ok\"")),
    }
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

fn refused(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, why.into())
}

/// This process's effective user.
#[allow(unsafe_code)] // geteuid has no preconditions and cannot fail.
pub fn current_uid() -> u32 {
    // SAFETY: as above.
    unsafe { libc::geteuid() }
}

/// Make the directory `path` (its parent must exist) this user's, with
/// mode 0700, or check that it already is: a real directory (not a
/// symbolic link), owned by this user, with no permission for anyone
/// else. Made with its mode at once, so it is never open to others.
pub fn private_dir(path: &Path) -> io::Result<()> {
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => check_private_dir(path),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => check_private_dir(path),
        Err(e) => Err(e),
    }
}

/// Whether `path` is a private directory, as [`private_dir`] makes one.
pub fn check_private_dir(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    let shown = path.display();
    if meta.file_type().is_symlink() {
        return Err(refused(format!("{shown} is a symbolic link")));
    }
    if !meta.is_dir() {
        return Err(refused(format!("{shown} is not a directory")));
    }
    if meta.uid() != current_uid() {
        return Err(refused(format!("{shown} is another user's")));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(refused(format!(
            "{shown} is open to other users (mode {:o}; it must be 700)",
            meta.mode() & 0o777
        )));
    }
    Ok(())
}

/// The private directory for `app`'s sockets, made if need be:
/// `$XDG_RUNTIME_DIR/APP` when that variable names an absolute directory
/// of this user's that no one else can write to, else `TMPDIR/APP-UID`.
/// `app` is letters, digits, `-` and `_`.
pub fn runtime_dir(app: &str) -> io::Result<PathBuf> {
    runtime_dir_in(app, std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
}

/// [`runtime_dir`] with the runtime directory given, not read from the
/// environment.
pub fn runtime_dir_in(app: &str, runtime: Option<PathBuf>) -> io::Result<PathBuf> {
    if app.is_empty()
        || !app
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(invalid(format!("{app:?} is not an app name")));
    }
    let usable = |dir: &Path| {
        dir.is_absolute()
            && std::fs::symlink_metadata(dir)
                .is_ok_and(|m| m.is_dir() && m.uid() == current_uid() && m.mode() & 0o022 == 0)
    };
    let dir = match runtime.filter(|d| usable(d)) {
        Some(base) => base.join(app),
        None => std::env::temp_dir().join(format!("{app}-{}", current_uid())),
    };
    private_dir(&dir)?;
    Ok(dir)
}

/// Whether `path` is a socket this user may talk to: not a symbolic link,
/// a socket, this user's, in a private directory.
pub fn check_socket(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    let shown = path.display();
    if meta.file_type().is_symlink() {
        return Err(refused(format!("{shown} is a symbolic link")));
    }
    if !meta.file_type().is_socket() {
        return Err(refused(format!("{shown} is not a socket")));
    }
    if meta.uid() != current_uid() {
        return Err(refused(format!("{shown} is another user's socket")));
    }
    check_private_dir(path.parent().unwrap_or(Path::new("/")))
}

/// The user at the other end of `stream`, where the platform tells.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(unsafe_code)] // getsockopt into a ucred of the size given.
pub fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the fd is open for `stream`'s life; `cred` and `len` are
    // valid for writes of the size passed.
    let done = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if done != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(cred.uid)
}

/// The user at the other end of `stream`, where the platform tells.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
#[allow(unsafe_code)] // getpeereid into two locals.
pub fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: the fd is open for `stream`'s life; both pointers are to
    // locals.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

/// The user at the other end of `stream`: this platform does not tell.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
pub fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this platform does not tell a socket's peer",
    ))
}

/// Whether a peer may talk: its user is `me`; where the platform does not
/// tell, the private directory is the only guard and it may.
pub fn peer_allowed(peer: io::Result<u32>, me: u32) -> io::Result<()> {
    match peer {
        Ok(uid) if uid == me => Ok(()),
        Ok(uid) => Err(refused(format!("the peer is another user ({uid})"))),
        Err(e) if e.kind() == io::ErrorKind::Unsupported => Ok(()),
        Err(e) => Err(e),
    }
}

/// One line of at most `limit` bytes, newline included, as text.
fn read_line(stream: impl Read, limit: u64) -> io::Result<String> {
    let mut bytes = Vec::new();
    BufReader::new(stream.take(limit + 1)).read_until(b'\n', &mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid(format!("longer than {limit} bytes")));
    }
    String::from_utf8(bytes).map_err(|_| invalid("not UTF-8"))
}

/// A bound control socket, not yet answering: [`serve`](Self::serve) it.
/// The socket file goes when it is dropped (or [`Closer::close`]d).
#[derive(Debug)]
pub struct ControlServer {
    listener: UnixListener,
    path: PathBuf,
    verbs: Vec<Verb>,
    read_only: bool,
    timeout: Duration,
    closed: Arc<AtomicBool>,
}

impl ControlServer {
    /// Bind a socket at `path`, answering `verbs`. Its directory must be
    /// private ([`private_dir`]). A file already at `path` is refused,
    /// unless it is a socket of this user's that no one answers (one left
    /// by a program that died), which is replaced; a symbolic link is
    /// always refused.
    pub fn bind(path: impl AsRef<Path>, verbs: &[Verb]) -> io::Result<ControlServer> {
        let path = path.as_ref().to_path_buf();
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| invalid("a socket path needs a directory"))?;
        check_private_dir(dir)?;
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(refused(format!("{} is a symbolic link", path.display())));
            }
            Ok(meta)
                if meta.file_type().is_socket()
                    && meta.uid() == current_uid()
                    && UnixStream::connect(&path).is_err() =>
            {
                std::fs::remove_file(&path)?;
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{} is in use", path.display()),
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(&path)?;
        Ok(ControlServer {
            listener,
            path,
            verbs: verbs.to_vec(),
            read_only: false,
            timeout: IO_TIMEOUT,
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Refuse every verb that mutates.
    #[must_use]
    pub fn read_only(mut self, on: bool) -> ControlServer {
        self.read_only = on;
        self
    }

    /// How long a read or write on a connection may wait (default
    /// [`IO_TIMEOUT`]).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> ControlServer {
        self.timeout = timeout;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    /// What stops [`serve`](Self::serve) from another thread.
    pub fn closer(&self) -> Closer {
        Closer {
            path: self.path.clone(),
            closed: self.closed.clone(),
        }
    }

    /// Answer connections until [closed](Closer::close), each on a thread
    /// of its own: checked ([`peer_allowed`]), read, refused if its verb is
    /// unknown or mutates in a read-only session, else given to
    /// `handler`, whose reply is written back.
    pub fn serve(self, handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) {
        let handler = Arc::new(handler);
        let active = Arc::new(AtomicUsize::new(0));
        let verbs: Arc<[Verb]> = self.verbs.clone().into();
        for stream in self.listener.incoming() {
            if self.closed.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                active.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let (handler, active, verbs) = (handler.clone(), active.clone(), verbs.clone());
            let (read_only, timeout) = (self.read_only, self.timeout);
            std::thread::spawn(move || {
                let _ = answer(stream, &verbs, read_only, timeout, &*handler);
                active.fetch_sub(1, Ordering::SeqCst);
            });
        }
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.closer().remove();
    }
}

/// One connection: its peer checked, its request read and answered.
fn answer(
    mut stream: UnixStream,
    verbs: &[Verb],
    read_only: bool,
    timeout: Duration,
    handler: &(dyn Fn(&Request) -> Reply + Send + Sync),
) -> io::Result<()> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    peer_allowed(peer_uid(&stream), current_uid())?;
    let reply = match read_line(&stream, MAX_REQUEST).and_then(|line| Request::parse(&line)) {
        Err(e) => Err(format!("bad request: {e}")),
        Ok(request) => match verbs.iter().find(|v| v.name == request.verb) {
            None => Err(format!("unknown verb {:?}", request.verb)),
            Some(verb) if verb.mutates && read_only => Err(format!(
                "read-only session: {:?} would change the page",
                verb.name
            )),
            Some(_) => handler(&request),
        },
    };
    let mut line = reply_line(&reply);
    if line.len() as u64 > MAX_REPLY {
        line = reply_line(&Err(format!("the reply is longer than {MAX_REPLY} bytes")));
    }
    stream.write_all(line.as_bytes())?;
    stream.flush()
}

/// Stops a [`ControlServer`]'s [`serve`](ControlServer::serve) and removes
/// its socket file.
#[derive(Clone, Debug)]
pub struct Closer {
    path: PathBuf,
    closed: Arc<AtomicBool>,
}

impl Closer {
    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            // Wake the accept, which then sees it is closed.
            let _ = UnixStream::connect(&self.path);
        }
        self.remove();
    }

    /// Remove the socket file, if it is still a socket.
    fn remove(&self) {
        if std::fs::symlink_metadata(&self.path).is_ok_and(|m| m.file_type().is_socket()) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Send `request` to the socket at `path` and read its reply, each read
/// and write waiting at most `timeout`. The socket is checked first
/// ([`check_socket`]), and so is the server's user where the platform
/// tells.
pub fn call(path: &Path, request: &Request, timeout: Duration) -> io::Result<Reply> {
    check_socket(path)?;
    let mut stream = UnixStream::connect(path)?;
    peer_allowed(peer_uid(&stream), current_uid())?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let line = request.to_line();
    if line.len() as u64 > MAX_REQUEST {
        return Err(invalid(format!(
            "the request is longer than {MAX_REQUEST} bytes"
        )));
    }
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    parse_reply(&read_line(&stream, MAX_REPLY)?)
}

/// The sockets in `dir` whose names start with `prefix` and end in
/// `.sock`, this user's, sorted; symbolic links are left out. A socket no
/// one answers is listed too: [`call`] says so.
pub fn sessions(dir: &Path, prefix: &str) -> io::Result<Vec<PathBuf>> {
    check_private_dir(dir)?;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(prefix) || !name.ends_with(".sock") {
            continue;
        }
        let path = entry.path();
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|m| m.file_type().is_socket() && m.uid() == current_uid())
        {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_replies_round_trip() {
        let request = Request::new("click", json!({"target": "12"}));
        assert_eq!(Request::parse(&request.to_line()).unwrap(), request);
        assert!(Request::parse("{\"verb\": 3}").is_err());
        assert!(Request::parse("{\"verb\": \"x\", \"args\": [1]}").is_err());
        assert_eq!(
            parse_reply(&reply_line(&Ok(json!(1)))).unwrap(),
            Ok(json!(1))
        );
        assert_eq!(
            parse_reply(&reply_line(&Err("no".into()))).unwrap(),
            Err("no".into())
        );
    }

    #[test]
    fn a_peer_must_be_this_user_where_the_platform_tells() {
        assert!(peer_allowed(Ok(1000), 1000).is_ok());
        let other = peer_allowed(Ok(1001), 1000).unwrap_err();
        assert_eq!(other.kind(), io::ErrorKind::PermissionDenied);
        let untold = io::Error::new(io::ErrorKind::Unsupported, "no");
        assert!(peer_allowed(Err(untold), 1000).is_ok());
        assert!(peer_allowed(Err(io::Error::other("broken")), 1000).is_err());
    }

    #[test]
    fn a_line_is_bounded() {
        assert_eq!(read_line(&b"abc\nrest"[..], 10).unwrap(), "abc\n");
        assert!(read_line(&[b'x'; 20][..], 10).is_err());
    }
}
