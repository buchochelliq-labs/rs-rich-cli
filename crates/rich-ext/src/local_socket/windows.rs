//! The local socket on Windows: a named pipe whose name an address file
//! holds, in a directory only this user can use.
//!
//! How the pipe is kept to this user (see the module's docs too):
//!
//! - **Its name is unguessable:** `\\.\pipe\rich-local-` and 128 bits from
//!   `BCryptGenRandom`, written to the address file, which the directory's
//!   DACL lets this user alone read. Named pipes share one namespace on a
//!   machine, so a name is not a guard on its own: the DACL is.
//! - **Its security descriptor** is `O:<sid>D:P(A;;GA;;;<sid>)`: this
//!   user owns it, the DACL is protected (nothing inherited) and grants
//!   this user's SID alone. Anyone else's `CreateFile` on it is refused
//!   with `ERROR_ACCESS_DENIED`, and so is making another instance of it.
//! - **`FILE_FLAG_FIRST_PIPE_INSTANCE`:** if a pipe of the name exists
//!   already (someone guessed it and made it first), the bind fails, so
//!   the listener never shares a name with another's pipe.
//! - **`PIPE_REJECT_REMOTE_CLIENTS`:** no connection from another machine.
//! - **A client** connects with `SECURITY_SQOS_PRESENT |
//!   SECURITY_IDENTIFICATION`, so a server can learn who it is but never
//!   act as it, and checks the pipe's owner is this user before it writes
//!   anything. It follows only an address file of this user's naming a
//!   pipe of the form above: never another machine's pipe (which would
//!   send the user's credentials there) or another program's.
//! - **A server** checks the client process's user where Windows lets it
//!   read that process's token (not always across elevation); where it
//!   cannot, the DACL has kept everyone else out already.

use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE,
    GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SE_FILE_OBJECT, SE_KERNEL_OBJECT, SE_OBJECT_TYPE,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Security::{
    ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetLengthSid,
    GetTokenInformation, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSID,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAG_OVERLAPPED, FILE_READ_ATTRIBUTES, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, READ_CONTROL,
    ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, INFINITE, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
};

use super::refused;

/// What every pipe's name starts with.
const PIPE_PREFIX: &str = r"\\.\pipe\rich-local-";
/// An address file's first line.
const HEADER: &str = "rich-local-socket 1";
/// The pipe's buffers, each way.
const BUFFER: u32 = 64 * 1024;
/// How long a client waits for a busy pipe's next instance.
const BUSY_WAIT: Duration = Duration::from_secs(5);
/// The access-allowed ACE types (plain, compound, object, callback,
/// callback object); every other type grants nothing.
const ALLOWED_ACES: [u8; 5] = [0, 4, 5, 9, 11];

/// This user: the SID (4-byte aligned, as Windows wants it) and its text.
struct Me {
    sid: Vec<u32>,
    text: String,
}

impl Me {
    fn psid(&self) -> PSID {
        self.sid.as_ptr() as PSID
    }
}

fn me() -> io::Result<&'static Me> {
    static ME: OnceLock<Me> = OnceLock::new();
    if let Some(me) = ME.get() {
        return Ok(me);
    }
    let me = read_me()?;
    Ok(ME.get_or_init(|| me))
}

#[allow(unsafe_code)] // the process's own token, queried.
fn read_me() -> io::Result<Me> {
    let mut token = std::ptr::null_mut();
    // SAFETY: the pseudo handle needs no closing; `token` is writable.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a handle just opened, owned from here.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let sid = token_user(token.as_raw_handle())?;
    let text = sid_text(sid.as_ptr() as PSID)?;
    Ok(Me { sid, text })
}

/// The user SID of `token` (opened with `TOKEN_QUERY`), copied.
#[allow(unsafe_code)] // GetTokenInformation into a buffer of the size it asks.
fn token_user(token: HANDLE) -> io::Result<Vec<u32>> {
    let mut len = 0u32;
    // SAFETY: a size query: no buffer, `len` writable. It fails with
    // ERROR_INSUFFICIENT_BUFFER and sets `len`.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len) };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `buffer` holds `len` bytes, aligned for TOKEN_USER.
    let done =
        unsafe { GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), len, &mut len) };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: filled as a TOKEN_USER, whose SID points into `buffer`.
    let sid = unsafe { (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid };
    // SAFETY: a valid SID, `n` bytes long (a multiple of 4).
    let n = unsafe { GetLengthSid(sid) } as usize;
    // SAFETY: the SID's `n` bytes, 4-byte aligned inside `buffer`.
    let words = unsafe { std::slice::from_raw_parts(sid as *const u32, n.div_ceil(4)) };
    Ok(words.to_vec())
}

#[allow(unsafe_code)] // ConvertSidToStringSidW's string, read and freed.
fn sid_text(sid: PSID) -> io::Result<String> {
    let mut text = std::ptr::null_mut();
    // SAFETY: a valid SID; `text` is writable.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a NUL-terminated string the call allocated, freed once.
    let out = unsafe {
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let out = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        out
    };
    Ok(out)
}

pub(super) fn current_user() -> String {
    me().map_or_else(|_| "unknown".into(), |me| me.text.clone())
}

fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain([0]).collect()
}

/// A security descriptor from SDDL, freed when dropped.
struct Descriptor(*mut core::ffi::c_void);

impl Descriptor {
    #[allow(unsafe_code)] // the conversion's descriptor, kept until dropped.
    fn new(sddl: &str) -> io::Result<Descriptor> {
        let sddl = wide(sddl);
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: NUL-terminated UTF-16; `descriptor` is writable.
        let done = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Descriptor(descriptor))
    }

    /// The descriptor of an object this user alone may use, as its owner.
    fn private(rights: &str) -> io::Result<Descriptor> {
        let me = &me()?.text;
        Descriptor::new(&format!("O:{me}D:P({rights};;;{me})"))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}

impl Drop for Descriptor {
    #[allow(unsafe_code)] // freed once, as allocated.
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptor…W.
        unsafe { LocalFree(self.0) };
    }
}

/// An object's owner and DACL, read from its handle.
struct Security {
    owner: PSID,
    dacl: *mut ACL,
    descriptor: *mut core::ffi::c_void,
}

impl Security {
    #[allow(unsafe_code)] // GetSecurityInfo's descriptor, kept until dropped.
    fn of(
        handle: HANDLE,
        kind: SE_OBJECT_TYPE,
        info: OBJECT_SECURITY_INFORMATION,
    ) -> io::Result<Security> {
        let mut out = Security {
            owner: std::ptr::null_mut(),
            dacl: std::ptr::null_mut(),
            descriptor: std::ptr::null_mut(),
        };
        // SAFETY: an open handle with READ_CONTROL; the pointers are
        // writable, and point into `descriptor`, freed on drop.
        let code = unsafe {
            GetSecurityInfo(
                handle,
                kind,
                info,
                &mut out.owner,
                std::ptr::null_mut(),
                &mut out.dacl,
                std::ptr::null_mut(),
                &mut out.descriptor,
            )
        };
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        Ok(out)
    }

    #[allow(unsafe_code)] // two valid SIDs compared.
    fn owned_by_me(&self) -> io::Result<bool> {
        let me = me()?;
        // SAFETY: both SIDs are valid while `self` and `me` live.
        Ok(!self.owner.is_null() && unsafe { EqualSid(self.owner, me.psid()) } != 0)
    }

    /// Whether every grant in the DACL is to this user. No DACL at all
    /// grants everyone everything.
    #[allow(unsafe_code)] // the ACL's ACEs, read in place.
    fn admits_only_me(&self) -> io::Result<bool> {
        let me = me()?;
        if self.dacl.is_null() {
            return Ok(false);
        }
        // SAFETY: a valid ACL while `self` lives.
        let count = unsafe { (*self.dacl).AceCount };
        for index in 0..u32::from(count) {
            let mut ace = std::ptr::null_mut();
            // SAFETY: an index below the ACE count; `ace` is writable.
            if unsafe { GetAce(self.dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every ACE starts with its header.
            let kind = unsafe { (*(ace as *const ACE_HEADER)).AceType };
            if !ALLOWED_ACES.contains(&kind) {
                continue;
            }
            // A plain allow ACE: header, mask, then the SID. The others
            // (object, callback) are never in what this module makes.
            if kind != 0 {
                return Ok(false);
            }
            // SAFETY: an ACCESS_ALLOWED_ACE: the SID follows the 4-byte
            // header and the 4-byte mask.
            let sid = unsafe { (ace as *mut u8).add(8) } as PSID;
            // SAFETY: both SIDs are valid here.
            if unsafe { EqualSid(sid, me.psid()) } == 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl Drop for Security {
    #[allow(unsafe_code)] // freed once, as allocated.
    fn drop(&mut self) {
        // SAFETY: allocated by GetSecurityInfo (or null).
        unsafe { LocalFree(self.descriptor) };
    }
}

/// `path` opened for its attributes and security, never through a link.
fn open_no_follow(path: &Path, access: u32) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .access_mode(access)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

pub(super) fn is_link(meta: &std::fs::Metadata) -> bool {
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[allow(unsafe_code)] // CreateDirectoryW with a descriptor that outlives it.
pub(super) fn private_dir(path: &Path) -> io::Result<()> {
    let descriptor = Descriptor::private("A;OICI;FA")?;
    let attributes = descriptor.attributes();
    let name = wide(path);
    // SAFETY: a NUL-terminated path; the attributes and their descriptor
    // live through the call.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
            return Err(error);
        }
    }
    check_private_dir(path)
}

pub(super) fn check_private_dir(path: &Path) -> io::Result<()> {
    let dir = open_no_follow(path, READ_CONTROL | FILE_READ_ATTRIBUTES)?;
    let meta = dir.metadata()?;
    let shown = path.display();
    if is_link(&meta) {
        return Err(refused(format!("{shown} is a symbolic link")));
    }
    if !meta.is_dir() {
        return Err(refused(format!("{shown} is not a directory")));
    }
    let security = Security::of(
        dir.as_raw_handle(),
        SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
    )?;
    if !security.owned_by_me()? {
        return Err(refused(format!("{shown} is another user's")));
    }
    if !security.admits_only_me()? {
        return Err(refused(format!(
            "{shown} is open to other users (its access list grants someone else; it must \
             grant you alone)"
        )));
    }
    Ok(())
}

pub(super) fn is_own_dir(dir: &Path) -> bool {
    dir.is_absolute() && check_private_dir(dir).is_ok()
}

/// The pipe an address file names: the file must be this user's, not a
/// link, and name a pipe of this module's form.
fn read_address(path: &Path) -> io::Result<String> {
    let mut file = open_no_follow(path, GENERIC_READ)?;
    let meta = file.metadata()?;
    let shown = path.display();
    if is_link(&meta) {
        return Err(refused(format!("{shown} is a symbolic link")));
    }
    let not_one = || refused(format!("{shown} is not a socket"));
    if !meta.is_file() || meta.len() > 256 {
        return Err(not_one());
    }
    let security = Security::of(
        file.as_raw_handle(),
        SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION,
    )?;
    if !security.owned_by_me()? {
        return Err(refused(format!("{shown} is another user's socket")));
    }
    let mut text = String::new();
    (&mut file)
        .take(256)
        .read_to_string(&mut text)
        .map_err(|_| not_one())?;
    parse_address(&text).ok_or_else(not_one)
}

/// The pipe's name in an address file's text, when it is one.
fn parse_address(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return None;
    }
    let name = lines.next()?;
    let token = name.strip_prefix(PIPE_PREFIX)?;
    (token.len() == 32
        && token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
    .then(|| name.to_string())
}

pub(super) fn check_endpoint(path: &Path) -> io::Result<()> {
    read_address(path).map(drop)
}

/// 128 bits from the system's random source, in hex.
#[allow(unsafe_code)] // BCryptGenRandom into a local buffer.
fn random_token() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    // SAFETY: the buffer is writable for its length; no algorithm handle
    // with the system-preferred flag.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(io::Error::other(format!(
            "no random bytes (NTSTATUS {status:#x})"
        )));
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// One instance of the pipe `name` (wide, NUL-terminated), this user's
/// alone; the first must be the first of its name.
#[allow(unsafe_code)] // CreateNamedPipeW with a descriptor that outlives it.
fn create_instance(name: &[u16], first: bool) -> io::Result<OwnedHandle> {
    let descriptor = Descriptor::private("A;;GA")?;
    let attributes = descriptor.attributes();
    let mut open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: a NUL-terminated name; the attributes live through the call.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER,
            BUFFER,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a handle just made, owned from here.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// The address file at `path`, made new (never over anything), this
/// user's alone, naming `pipe`.
#[allow(unsafe_code)] // CreateFileW with a descriptor that outlives it.
fn write_address(path: &Path, pipe: &str) -> io::Result<()> {
    let descriptor = Descriptor::private("A;;FA")?;
    let attributes = descriptor.attributes();
    let name = wide(path);
    // SAFETY: a NUL-terminated path; the attributes live through the call.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a file handle just opened, owned from here.
    let mut file = std::fs::File::from(unsafe { OwnedHandle::from_raw_handle(handle) });
    file.write_all(format!("{HEADER}\n{pipe}\n").as_bytes())
}

/// An event for one overlapped operation.
#[allow(unsafe_code)] // a manual-reset event, owned.
fn event() -> io::Result<OwnedHandle> {
    // SAFETY: no attributes, no name.
    let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a handle just made, owned from here.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Run one overlapped operation on `handle` (`start` begins it with the
/// OVERLAPPED it is given) and wait for it, at most `timeout`. On a
/// timeout the operation is cancelled and waited for, so the OVERLAPPED
/// on this stack outlives it, and the error is `TimedOut`.
#[allow(unsafe_code)] // the OVERLAPPED stays put until the operation ends.
fn overlapped(
    handle: HANDLE,
    timeout: Option<Duration>,
    start: impl FnOnce(*mut OVERLAPPED) -> i32,
) -> io::Result<u32> {
    let event = event()?;
    // SAFETY: an all-zero OVERLAPPED is a valid one.
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    ov.hEvent = event.as_raw_handle();
    if start(&mut ov) == 0 {
        // SAFETY: no preconditions.
        let code = unsafe { GetLastError() };
        if code != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let wait = timeout.map_or(INFINITE, |d| {
            d.as_millis().clamp(1, u128::from(INFINITE - 1)) as u32
        });
        // SAFETY: the event lives until this function returns.
        if unsafe { WaitForSingleObject(ov.hEvent, wait) } != WAIT_OBJECT_0 {
            let mut done = 0;
            // SAFETY: `ov` is the pending operation's; waiting for it to end
            // (cancelled or not) before `ov` goes.
            let ended = unsafe {
                CancelIoEx(handle, &ov);
                GetOverlappedResult(handle, &ov, &mut done, 1)
            };
            if ended != 0 {
                return Ok(done);
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_OPERATION_ABORTED as i32) {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "timed out"));
            }
            return Err(error);
        }
    }
    let mut done = 0;
    // SAFETY: the operation has ended; `ov` is its.
    if unsafe { GetOverlappedResult(handle, &ov, &mut done, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(done)
}

#[derive(Debug)]
pub(super) struct Listener {
    /// The pipe's name, wide and NUL-terminated.
    name: Vec<u16>,
    /// The instance the next client connects to.
    pending: Mutex<OwnedHandle>,
}

impl Listener {
    pub(super) fn bind(path: &Path) -> io::Result<Listener> {
        let pipe = format!("{PIPE_PREFIX}{}", random_token()?);
        let name = wide(&pipe);
        let first = create_instance(&name, true)?;
        write_address(path, &pipe)?;
        Ok(Listener {
            name,
            pending: Mutex::new(first),
        })
    }

    /// Wait for a client on the pending instance, and make the next one
    /// before handing it out, so a client never finds no instance free.
    #[allow(unsafe_code)] // ConnectNamedPipe, overlapped.
    pub(super) fn accept(&self) -> io::Result<Stream> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let handle = pending.as_raw_handle();
        // SAFETY: the instance is open while `pending` is held.
        let connected = overlapped(handle, None, |ov| unsafe { ConnectNamedPipe(handle, ov) });
        let next = create_instance(&self.name, false)?;
        let instance = std::mem::replace(&mut *pending, next);
        match connected {
            Ok(_) => {}
            // A client that connected before the wait began.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) => {}
            Err(e) => return Err(e),
        }
        Ok(Stream::new(instance, true))
    }
}

#[derive(Debug)]
pub(super) struct Stream {
    handle: OwnedHandle,
    /// The listener's end (else the client's).
    server: bool,
    /// The timeouts in milliseconds, `u64::MAX` for none.
    read_timeout: AtomicU64,
    write_timeout: AtomicU64,
}

const NO_TIMEOUT: u64 = u64::MAX;

impl Stream {
    fn new(handle: OwnedHandle, server: bool) -> Stream {
        Stream {
            handle,
            server,
            read_timeout: AtomicU64::new(NO_TIMEOUT),
            write_timeout: AtomicU64::new(NO_TIMEOUT),
        }
    }

    /// Open the pipe the address file at `path` names, waiting a while
    /// for an instance when all are busy.
    #[allow(unsafe_code)] // CreateFileW on the pipe.
    pub(super) fn connect(path: &Path) -> io::Result<Stream> {
        let name = wide(read_address(path)?);
        let deadline = Instant::now() + BUSY_WAIT;
        loop {
            // SAFETY: a NUL-terminated name; no attributes or template.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    std::ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                // SAFETY: a handle just opened, owned from here.
                return Ok(Stream::new(
                    unsafe { OwnedHandle::from_raw_handle(handle) },
                    false,
                ));
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) || Instant::now() >= deadline {
                return Err(error);
            }
            // SAFETY: a NUL-terminated name.
            unsafe { WaitNamedPipeW(name.as_ptr(), 500) };
        }
    }

    pub(super) fn peer_allowed(&self) -> io::Result<()> {
        if self.server {
            return match self.client_user() {
                Some(false) => Err(refused("the peer is another user")),
                _ => Ok(()),
            };
        }
        let security = Security::of(
            self.handle.as_raw_handle(),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
        )?;
        if security.owned_by_me()? {
            Ok(())
        } else {
            Err(refused("the pipe is another user's"))
        }
    }

    /// Whether the client process is this user's: `None` where Windows
    /// does not let this process read its token.
    #[allow(unsafe_code)] // the client process's token, queried.
    fn client_user(&self) -> Option<bool> {
        let mut pid = 0;
        // SAFETY: an open server instance; `pid` is writable.
        if unsafe { GetNamedPipeClientProcessId(self.handle.as_raw_handle(), &mut pid) } == 0 {
            return None;
        }
        // SAFETY: no handle is inherited.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        // SAFETY: a handle just opened, owned from here.
        let process = unsafe { OwnedHandle::from_raw_handle(process) };
        let mut token = std::ptr::null_mut();
        // SAFETY: an open process handle; `token` is writable.
        if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }
        // SAFETY: a handle just opened, owned from here.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let sid = token_user(token.as_raw_handle()).ok()?;
        let me = me().ok()?;
        // SAFETY: two valid SIDs.
        Some(unsafe { EqualSid(sid.as_ptr() as PSID, me.psid()) } != 0)
    }

    fn timeout(cell: &AtomicU64) -> Option<Duration> {
        match cell.load(Ordering::Relaxed) {
            NO_TIMEOUT => None,
            ms => Some(Duration::from_millis(ms)),
        }
    }

    fn set_timeout(cell: &AtomicU64, timeout: Option<Duration>) -> io::Result<()> {
        let ms = match timeout {
            None => NO_TIMEOUT,
            Some(d) if d.is_zero() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "cannot set a 0 duration timeout",
                ));
            }
            Some(d) => (d.as_millis() as u64).clamp(1, NO_TIMEOUT - 1),
        };
        cell.store(ms, Ordering::Relaxed);
        Ok(())
    }

    pub(super) fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        Stream::set_timeout(&self.read_timeout, timeout)
    }

    pub(super) fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        Stream::set_timeout(&self.write_timeout, timeout)
    }

    #[allow(unsafe_code)] // ReadFile into `buf`, which outlives the operation.
    pub(super) fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let handle = self.handle.as_raw_handle();
        let len = buf.len().min(u32::MAX as usize) as u32;
        let ptr = buf.as_mut_ptr();
        let timeout = Stream::timeout(&self.read_timeout);
        // SAFETY: `buf` is writable for `len` bytes until the operation
        // ends, which `overlapped` waits for.
        match overlapped(handle, timeout, |ov| unsafe {
            ReadFile(handle, ptr, len, std::ptr::null_mut(), ov)
        }) {
            Ok(n) => Ok(n as usize),
            // The other end closed: the end of the stream.
            Err(e)
                if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32)
                    || e.raw_os_error() == Some(ERROR_PIPE_NOT_CONNECTED as i32) =>
            {
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }

    #[allow(unsafe_code)] // WriteFile from `buf`, which outlives the operation.
    pub(super) fn write(&self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let handle = self.handle.as_raw_handle();
        let len = buf.len().min(u32::MAX as usize) as u32;
        let ptr = buf.as_ptr();
        let timeout = Stream::timeout(&self.write_timeout);
        // SAFETY: `buf` is readable for `len` bytes until the operation
        // ends, which `overlapped` waits for.
        overlapped(handle, timeout, |ov| unsafe {
            WriteFile(handle, ptr, len, std::ptr::null_mut(), ov)
        })
        .map(|n| n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_socket::{LocalListener, LocalStream, private_dir};

    fn private(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("rich-pipe-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dir = base.join("p");
        private_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn the_pipe_and_its_directory_admit_this_user_alone() {
        let dir = private("dacl");
        let listener = LocalListener::bind(dir.join("app.sock")).unwrap();
        let client = LocalStream::connect(dir.join("app.sock")).unwrap();
        let pipe = Security::of(
            client.0.handle.as_raw_handle(),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        )
        .unwrap();
        assert!(pipe.owned_by_me().unwrap());
        assert!(pipe.admits_only_me().unwrap());
        let address = open_no_follow(&dir.join("app.sock"), READ_CONTROL).unwrap();
        let file = Security::of(
            address.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
        )
        .unwrap();
        assert!(file.owned_by_me().unwrap());
        assert!(file.admits_only_me().unwrap());
        drop((client, listener));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    /// Another user, as CI can show one: the anonymous logon's token,
    /// which a thread can take on without any privilege. Its `CreateFile`
    /// on the pipe is refused by the pipe's DACL, and its read of the
    /// address file by the directory's.
    #[test]
    #[allow(unsafe_code)] // impersonation on a thread of the test's own.
    fn another_users_access_is_refused() {
        use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
        use windows_sys::Win32::Security::{ImpersonateAnonymousToken, RevertToSelf};
        use windows_sys::Win32::System::Threading::GetCurrentThread;
        let dir = private("anonymous");
        let path = dir.join("app.sock");
        let listener = LocalListener::bind(&path).unwrap();
        let pipe = read_address(&path).unwrap();
        let (pipe_error, file_error) = std::thread::spawn(move || {
            // SAFETY: this thread's own token, reverted before it ends.
            assert_ne!(
                unsafe { ImpersonateAnonymousToken(GetCurrentThread()) },
                0,
                "impersonating the anonymous logon: {}",
                io::Error::last_os_error()
            );
            let name = wide(&pipe);
            // SAFETY: a NUL-terminated name.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };
            let pipe_error = (handle == INVALID_HANDLE_VALUE)
                .then(io::Error::last_os_error)
                .and_then(|e| e.raw_os_error());
            let file_error = read_address(&path).err().map(|e| e.kind());
            // SAFETY: as above.
            unsafe { RevertToSelf() };
            (pipe_error, file_error)
        })
        .join()
        .unwrap();
        assert_eq!(pipe_error, Some(ERROR_ACCESS_DENIED as i32));
        assert_eq!(file_error, Some(io::ErrorKind::PermissionDenied));
        // This user still connects.
        assert!(LocalStream::connect(dir.join("app.sock")).is_ok());
        drop(listener);
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_pipe_name_taken_first_is_never_shared() {
        let name = wide(format!("{PIPE_PREFIX}{}", random_token().unwrap()));
        let squatter = create_instance(&name, true).unwrap();
        assert!(create_instance(&name, true).is_err());
        drop(squatter);
        assert!(create_instance(&name, true).is_ok());
    }

    #[test]
    fn an_address_file_names_only_a_local_pipe_of_this_form() {
        let token = "0123456789abcdef0123456789abcdef";
        let good = format!("{HEADER}\n{PIPE_PREFIX}{token}\n");
        assert_eq!(
            parse_address(&good).as_deref(),
            Some(format!("{PIPE_PREFIX}{token}").as_str())
        );
        for bad in [
            format!("{HEADER}\n\\\\server\\pipe\\rich-local-{token}\n"),
            format!("{HEADER}\n\\\\.\\pipe\\other-{token}\n"),
            format!("{HEADER}\n{PIPE_PREFIX}{token}0\n"),
            format!("{HEADER}\n{PIPE_PREFIX}../{}\n", &token[3..]),
            format!("other\n{PIPE_PREFIX}{token}\n"),
        ] {
            assert_eq!(parse_address(&bad), None, "{bad}");
        }
        let dir = private("address");
        let path = dir.join("app.sock");
        std::fs::write(
            &path,
            format!("{HEADER}\n\\\\server\\pipe\\rich-local-{token}\n"),
        )
        .unwrap();
        assert!(LocalStream::connect(&path).is_err());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
