//! A local socket private to one user: a Unix domain socket on Unix, a
//! named pipe on Windows, behind one API (the `local-socket` feature).
//! What `rich console`'s log socket and rs-rich-interact's control socket
//! listen on, so the same code paths run on both.
//!
//! An endpoint is named by a path, in a [private directory](private_dir):
//!
//! - **On Unix** the path is the socket itself. Its directory is the
//!   user's, mode 0700; the socket is made 0600; a peer's user is checked
//!   where the platform tells (`SO_PEERCRED` on Linux and Android,
//!   `getpeereid` on macOS and the BSDs).
//! - **On Windows** the path is a small *address file* naming the pipe:
//!   `\\.\pipe\rich-local-` and 32 hex digits from the system's random
//!   source (`BCryptGenRandom`), so the name is unguessable, and only the
//!   user can read the file that holds it. The directory and the pipe have
//!   protected DACLs that admit this user's SID alone (no inherited
//!   grants: not Administrators, not SYSTEM), with the user as owner. The
//!   pipe is made with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a pipe of that
//!   name made first by anyone else makes the bind fail instead of
//!   sharing it, and `PIPE_REJECT_REMOTE_CLIENTS`, so no other machine
//!   reaches it. A client checks that the pipe's owner is this user before
//!   it sends anything, and connects at the `SECURITY_IDENTIFICATION`
//!   level, so a server can never act as it; a server checks the client
//!   process's user where Windows lets it read that process's token. An
//!   address file names only a local pipe of that form: one that names a
//!   pipe on another machine, or any other, is refused.
//! - **On both,** a symbolic link (on Windows, any reparse point) where
//!   the endpoint or its directory should be is refused, and so is
//!   another user's endpoint. Binding over an endpoint of this user's that
//!   no one answers (left by a program that died) replaces it; binding
//!   over a live one fails with [`io::ErrorKind::AddrInUse`]; anything
//!   else at the path is refused and left alone.
//!
//! ```no_run
//! # #[cfg(any(unix, windows))] {
//! use std::io::{BufRead, BufReader, Write};
//! use rich_ext::local_socket::{self, LocalListener, LocalStream};
//!
//! let dir = std::env::temp_dir().join("my-app-sockets");
//! local_socket::private_dir(&dir)?;
//! let listener = LocalListener::bind(dir.join("my-app.sock"))?;
//! std::thread::spawn(move || {
//!     for stream in listener.incoming().flatten() {
//!         if stream.peer_allowed().is_ok() {
//!             let line = BufReader::new(&stream).lines().next();
//!             println!("{line:?}");
//!         }
//!     }
//! });
//! let mut client = LocalStream::connect(dir.join("my-app.sock"))?;
//! client.write_all(b"hello\n")?;
//! # }
//! # Ok::<(), std::io::Error>(())
//! ```

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
#[path = "local_socket/unix.rs"]
mod sys;
#[cfg(windows)]
#[path = "local_socket/windows.rs"]
mod sys;

pub(crate) fn refused(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, why.into())
}

/// Make the directory `path` (its parent must exist) this user's alone,
/// or check that it already is: a real directory (not a symbolic link or
/// other reparse point), owned by this user, that no one else can use.
/// Made private at once, never open to others in between: mode 0700 on
/// Unix; on Windows a protected DACL that grants this user's SID alone
/// (`O:<sid>D:P(A;OICI;FA;;;<sid>)`), which what is made in it inherits.
pub fn private_dir(path: &Path) -> io::Result<()> {
    sys::private_dir(path)
}

/// Whether `path` is a private directory, as [`private_dir`] makes one: on
/// Unix this user's with no permission for anyone else; on Windows this
/// user's, with a DACL whose every grant is to this user.
pub fn check_private_dir(path: &Path) -> io::Result<()> {
    sys::check_private_dir(path)
}

/// Whether `path` is an endpoint of this user's: not a symbolic link (or
/// reparse point); on Unix a socket, on Windows an address file naming a
/// pipe of the form this module makes; owned by this user.
pub fn check_endpoint(path: &Path) -> io::Result<()> {
    sys::check_endpoint(path)
}

/// Whether `path` is an endpoint of this user's ([`check_endpoint`]).
pub fn is_endpoint(path: &Path) -> bool {
    check_endpoint(path).is_ok()
}

/// Remove the endpoint at `path`, if it is one of this user's: what a
/// server does on its way out. Anything else there is left alone.
pub fn remove_endpoint(path: &Path) {
    if is_endpoint(path) {
        let _ = std::fs::remove_file(path);
    }
}

/// This user, as text for names: the effective uid on Unix, the SID
/// (`S-1-5-21-…`) on Windows.
pub fn current_user() -> String {
    sys::current_user()
}

/// This process's effective user (Unix).
#[cfg(unix)]
pub fn current_uid() -> u32 {
    sys::current_uid()
}

/// The user at the other end of a Unix socket, where the platform tells
/// (`SO_PEERCRED` on Linux and Android, `getpeereid` on macOS and the
/// BSDs); [`Unsupported`](io::ErrorKind::Unsupported) elsewhere.
#[cfg(unix)]
pub fn peer_uid(stream: &std::os::unix::net::UnixStream) -> io::Result<u32> {
    sys::peer_uid(stream)
}

/// Whether `dir` is a real directory (not a link) of this user's that no
/// one else can write to: a place to make a [private
/// directory](private_dir) in, as `XDG_RUNTIME_DIR` should be.
pub fn is_own_dir(dir: &Path) -> bool {
    sys::is_own_dir(dir)
}

/// A bound local socket: [`accept`](Self::accept) its connections.
#[derive(Debug)]
pub struct LocalListener {
    inner: sys::Listener,
    path: PathBuf,
}

impl LocalListener {
    /// Listen at `path`, whose directory must be [private](private_dir).
    /// A symbolic link at `path` is refused; an endpoint of this user's
    /// that no one answers is replaced; a live one is
    /// [`AddrInUse`](io::ErrorKind::AddrInUse); anything else is refused
    /// ([`PermissionDenied`](io::ErrorKind::PermissionDenied)) and left
    /// alone. The endpoint stays when the listener is dropped (a later
    /// bind replaces it); [`remove_endpoint`] removes it.
    pub fn bind(path: impl AsRef<Path>) -> io::Result<LocalListener> {
        let path = path.as_ref().to_path_buf();
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        check_private_dir(dir)?;
        let shown = path.display();
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if sys::is_link(&meta) => {
                return Err(refused(format!("{shown} is a symbolic link")));
            }
            Ok(_) => {
                if check_endpoint(&path).is_err() {
                    return Err(refused(format!(
                        "{shown} is not a socket of this user's: refused (remove it, or use \
                         another path)"
                    )));
                }
                if sys::Stream::connect(&path).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        format!("{shown} is in use"),
                    ));
                }
                std::fs::remove_file(&path)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let inner = sys::Listener::bind(&path)?;
        Ok(LocalListener { inner, path })
    }

    /// The next connection, waiting for one. Check its peer with
    /// [`LocalStream::peer_allowed`].
    pub fn accept(&self) -> io::Result<LocalStream> {
        self.inner.accept().map(LocalStream)
    }

    /// The connections, as they come: [`accept`](Self::accept) forever.
    pub fn incoming(&self) -> Incoming<'_> {
        Incoming { listener: self }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// [`LocalListener::incoming`]'s connections.
#[derive(Debug)]
pub struct Incoming<'a> {
    listener: &'a LocalListener,
}

impl Iterator for Incoming<'_> {
    type Item = io::Result<LocalStream>;

    fn next(&mut self) -> Option<io::Result<LocalStream>> {
        Some(self.listener.accept())
    }
}

/// A connection, from either end: [`Read`] and [`Write`] (also through
/// `&LocalStream`), with timeouts.
#[derive(Debug)]
pub struct LocalStream(sys::Stream);

impl LocalStream {
    /// Connect to the endpoint at `path`: it must be this user's
    /// ([`check_endpoint`]), and so must its listener, where the platform
    /// tells (on Windows always: the pipe's owner).
    pub fn connect(path: impl AsRef<Path>) -> io::Result<LocalStream> {
        let path = path.as_ref();
        check_endpoint(path)?;
        let stream = LocalStream(sys::Stream::connect(path)?);
        stream.peer_allowed()?;
        Ok(stream)
    }

    /// Whether the other end is this user: an error
    /// ([`PermissionDenied`](io::ErrorKind::PermissionDenied)) when it is
    /// another user's. Where the platform cannot tell, it is let in: the
    /// private directory (and on Windows the pipe's DACL) keeps others
    /// out.
    pub fn peer_allowed(&self) -> io::Result<()> {
        self.0.peer_allowed()
    }

    /// How long a read may wait (`None`: for ever).
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_read_timeout(timeout)
    }

    /// How long a write may wait (`None`: for ever).
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_write_timeout(timeout)
    }

    /// The Unix socket underneath.
    #[cfg(unix)]
    pub fn as_unix(&self) -> &std::os::unix::net::UnixStream {
        self.0.as_unix()
    }
}

impl Read for LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}

impl Read for &LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for LocalStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for &LocalStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    /// A fresh private directory for one test.
    fn private(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("rich-local-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dir = base.join("p");
        private_dir(&dir).unwrap();
        check_private_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn a_client_connects_and_lines_go_both_ways() {
        let dir = private("lines");
        let path = dir.join("app.sock");
        let listener = LocalListener::bind(&path).unwrap();
        assert!(is_endpoint(&path));
        let server = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            stream.peer_allowed().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            (&stream).write_all(line.to_uppercase().as_bytes()).unwrap();
            listener
        });
        let mut client = LocalStream::connect(&path).unwrap();
        client.write_all(b"hello\n").unwrap();
        let mut reply = String::new();
        BufReader::new(&client).read_line(&mut reply).unwrap();
        assert_eq!(reply, "HELLO\n");
        let listener = server.join().unwrap();
        // Two listeners cannot share a path.
        assert_eq!(
            LocalListener::bind(&path).unwrap_err().kind(),
            io::ErrorKind::AddrInUse
        );
        drop(client);
        drop(listener);
        // Its endpoint, left behind, is replaced.
        let again = LocalListener::bind(&path).unwrap();
        drop(again);
        remove_endpoint(&path);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn the_other_end_closing_is_the_end_of_the_stream() {
        let dir = private("eof");
        let path = dir.join("app.sock");
        let listener = LocalListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            (&stream).write_all(b"bye\n").unwrap();
        });
        let client = LocalStream::connect(&path).unwrap();
        server.join().unwrap();
        let mut all = String::new();
        (&client).read_to_string(&mut all).unwrap();
        assert_eq!(all, "bye\n");
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_read_times_out() {
        let dir = private("timeout");
        let path = dir.join("app.sock");
        let listener = LocalListener::bind(&path).unwrap();
        let held = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(800));
            drop(stream);
        });
        let client = LocalStream::connect(&path).unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let started = std::time::Instant::now();
        let error = (&client).read(&mut [0; 8]).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ),
            "{error:?}"
        );
        assert!(started.elapsed() < Duration::from_millis(700));
        held.join().unwrap();
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn what_is_not_an_endpoint_is_refused_and_left_alone() {
        let dir = private("refused");
        let path = dir.join("app.sock");
        std::fs::write(&path, "notes").unwrap();
        assert!(!is_endpoint(&path));
        let error = LocalListener::bind(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("not a socket"), "{error}");
        assert_eq!(
            LocalStream::connect(&path).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        remove_endpoint(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "notes");
        // Nothing listens where nothing is.
        assert!(LocalStream::connect(dir.join("none.sock")).is_err());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_listener_needs_a_private_directory() {
        let base = std::env::temp_dir().join(format!("rich-local-{}-open", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // A directory made the ordinary way is open to others (on Unix by
        // its mode, on Windows by the grants it inherits).
        let open = base.join("open");
        std::fs::create_dir(&open).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(check_private_dir(&open).is_err());
        assert!(LocalListener::bind(open.join("app.sock")).is_err());
        assert!(is_own_dir(&base) || cfg!(windows));
        assert!(!current_user().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
