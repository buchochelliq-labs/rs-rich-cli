//! The local socket on Unix: a Unix domain socket in a 0700 directory.

use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

use super::refused;

/// This process's effective user.
#[allow(unsafe_code)] // geteuid has no preconditions and cannot fail.
pub(super) fn current_uid() -> u32 {
    // SAFETY: as above.
    unsafe { libc::geteuid() }
}

pub(super) fn current_user() -> String {
    current_uid().to_string()
}

pub(super) fn is_link(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

pub(super) fn private_dir(path: &Path) -> io::Result<()> {
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => check_private_dir(path),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => check_private_dir(path),
        Err(e) => Err(e),
    }
}

pub(super) fn check_private_dir(path: &Path) -> io::Result<()> {
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

pub(super) fn is_own_dir(dir: &Path) -> bool {
    dir.is_absolute()
        && std::fs::symlink_metadata(dir)
            .is_ok_and(|m| m.is_dir() && m.uid() == current_uid() && m.mode() & 0o022 == 0)
}

pub(super) fn check_endpoint(path: &Path) -> io::Result<()> {
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
    Ok(())
}

/// The user at the other end of `stream`, where the platform tells.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(unsafe_code)] // getsockopt into a ucred of the size given.
pub(super) fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
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
pub(super) fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (libc::uid_t::MAX, libc::gid_t::MAX);
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
pub(super) fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this platform does not tell a socket's peer",
    ))
}

#[derive(Debug)]
pub(super) struct Listener(UnixListener);

impl Listener {
    /// Bind, then make the socket 0600 and check it is. The directory is
    /// private already, so no one else reaches the socket before its mode
    /// is set. (The umask is not changed: it is the whole process's, and
    /// other threads create files meanwhile.)
    pub(super) fn bind(path: &Path) -> io::Result<Listener> {
        let listener = UnixListener::bind(path)?;
        let shown = path.display();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        let mode = std::fs::symlink_metadata(path)?.permissions().mode() & 0o777;
        if mode != 0o600 {
            drop(listener);
            let _ = std::fs::remove_file(path);
            return Err(refused(format!(
                "{shown} was created with mode {mode:o}, not 600: refused"
            )));
        }
        Ok(Listener(listener))
    }

    pub(super) fn accept(&self) -> io::Result<Stream> {
        self.0.accept().map(|(stream, _)| Stream(stream))
    }
}

#[derive(Debug)]
pub(super) struct Stream(UnixStream);

impl Stream {
    pub(super) fn connect(path: &Path) -> io::Result<Stream> {
        UnixStream::connect(path).map(Stream)
    }

    pub(super) fn peer_allowed(&self) -> io::Result<()> {
        match peer_uid(&self.0) {
            Ok(uid) if uid == current_uid() => Ok(()),
            Ok(uid) => Err(refused(format!("the peer is another user ({uid})"))),
            Err(e) if e.kind() == io::ErrorKind::Unsupported => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub(super) fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_read_timeout(timeout)
    }

    pub(super) fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_write_timeout(timeout)
    }

    pub(super) fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.0).read(buf)
    }

    pub(super) fn write(&self, buf: &[u8]) -> io::Result<usize> {
        (&self.0).write(buf)
    }

    pub(super) fn as_unix(&self) -> &UnixStream {
        &self.0
    }
}
