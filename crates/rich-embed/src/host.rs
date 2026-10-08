//! What runs behind a terminal pane: [`PtyHost`], and its two
//! implementations here, [`LocalPty`] (a program on this machine) and
//! [`ReplayHost`] (bytes played back, for tests and demos).
//!
//! A host starts the program at a size, carries bytes both ways and the
//! size when it changes, and says when the program has exited. Output is
//! read from the host's own thread into a buffer, and the host calls the
//! [`Notify`] it was given whenever there is something new: the pane then
//! reads it on the app's thread, between frames. An SSH session, a
//! container's exec stream or a remote shell is another implementation of
//! the same trait, passed to [`terminal_with`](crate::terminal_with).

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

/// Called by a host, from any thread, when output arrives or the program
/// exits. It only schedules work: the pane reads the host on the app's
/// thread.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// How a program ended.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExitStatus {
    code: u32,
    signal: Option<String>,
}

impl ExitStatus {
    /// It exited with `code`.
    pub fn with_code(code: u32) -> ExitStatus {
        ExitStatus { code, signal: None }
    }

    /// It was ended by `signal` (its name, such as `"Terminated"`).
    pub fn with_signal(signal: impl Into<String>) -> ExitStatus {
        ExitStatus {
            code: 1,
            signal: Some(signal.into()),
        }
    }

    /// The exit code (1 when a signal ended it).
    pub fn code(&self) -> u32 {
        self.code
    }

    /// The signal that ended it, if one did.
    pub fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }

    /// Whether it exited with code 0.
    pub fn success(&self) -> bool {
        self.code == 0 && self.signal.is_none()
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.signal {
            Some(signal) => write!(f, "ended by signal {signal}"),
            None => write!(f, "exited with code {}", self.code),
        }
    }
}

impl From<portable_pty::ExitStatus> for ExitStatus {
    fn from(status: portable_pty::ExitStatus) -> ExitStatus {
        match status.signal() {
            Some(signal) => ExitStatus::with_signal(signal),
            None => ExitStatus::with_code(status.exit_code()),
        }
    }
}

/// Starts a program and carries its bytes and its size: what a terminal
/// pane talks to. Every method is called on the app's thread.
pub trait PtyHost {
    /// Start the program on a screen `columns` x `rows` cells. Called once,
    /// when the pane is first laid out.
    fn start(&mut self, columns: u16, rows: u16) -> io::Result<()>;

    /// Send `bytes` to the program, as if typed.
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// The pane is now `columns` x `rows` cells.
    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()>;

    /// The output that arrived since the last call; empty when there is
    /// none. Never blocks.
    fn read(&mut self) -> Vec<u8>;

    /// Call `notify` whenever output arrives or the program exits. Given
    /// once, before or after [`start`](Self::start); a host with something
    /// already waiting calls it at once.
    fn set_notify(&mut self, notify: Notify);

    /// How the program ended, once it has, and once the output it wrote
    /// before that has been [read](Self::read).
    fn exit_status(&mut self) -> Option<ExitStatus>;

    /// End the program. Also called when the pane goes away.
    fn kill(&mut self) -> io::Result<()>;
}

/// A program and its arguments, environment and directory, for
/// [`LocalPty`]. A string is a program name; an array or a vector is the
/// program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    cwd: Option<PathBuf>,
}

impl Command {
    pub fn new(program: impl Into<OsString>) -> Command {
        Command {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Command {
        self.args.push(arg.into());
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: Into<OsString>>(mut self, args: I) -> Command {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set `key` in the program's environment (it inherits the app's,
    /// with `TERM=xterm-256color`).
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Command {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Run in `dir` (default: the app's current directory).
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Command {
        self.cwd = Some(dir.into());
        self
    }

    /// The program's name.
    pub fn program(&self) -> &OsString {
        &self.program
    }

    fn builder(&self) -> CommandBuilder {
        let mut builder = CommandBuilder::new(&self.program);
        builder.args(&self.args);
        builder.env("TERM", "xterm-256color");
        for (key, value) in &self.env {
            builder.env(key, value);
        }
        match &self.cwd {
            Some(dir) => builder.cwd(dir),
            // portable-pty would start in the home directory.
            None => {
                if let Ok(dir) = std::env::current_dir() {
                    builder.cwd(dir);
                }
            }
        }
        builder
    }
}

impl From<&str> for Command {
    fn from(program: &str) -> Command {
        Command::new(program)
    }
}

impl From<String> for Command {
    fn from(program: String) -> Command {
        Command::new(program)
    }
}

impl<S: Into<OsString>, const N: usize> From<[S; N]> for Command {
    fn from(parts: [S; N]) -> Command {
        Command::from_parts(parts)
    }
}

impl<S: Into<OsString>> From<Vec<S>> for Command {
    fn from(parts: Vec<S>) -> Command {
        Command::from_parts(parts)
    }
}

impl Command {
    fn from_parts<S: Into<OsString>>(parts: impl IntoIterator<Item = S>) -> Command {
        let mut parts = parts.into_iter().map(Into::into);
        let program = parts.next().unwrap_or_default();
        Command::new(program).args(parts)
    }
}

/// Output kept for the pane at most, in bytes: past it the oldest goes,
/// so a program that floods a pane the app is not reading costs a bounded
/// amount of memory.
const MAX_BUFFERED: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct Output {
    buffer: VecDeque<u8>,
    /// The reader reached the end of the output.
    eof: bool,
    /// The reader is waiting for room ([`LocalPty::backpressure`]).
    paused: bool,
    /// The host is gone: a paused reader stops.
    closed: bool,
    status: Option<ExitStatus>,
    notify: Option<Notify>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Notify outside the lock.
fn notify(shared: &Mutex<Output>) {
    let notify = lock(shared).notify.clone();
    if let Some(notify) = notify {
        notify();
    }
}

/// A program on this machine, on a PTY (ConPTY on Windows), through
/// portable-pty as rs-rich-record's tapes run theirs.
pub struct LocalPty {
    command: Command,
    shared: Arc<Mutex<Output>>,
    master: Option<Box<dyn MasterPty + Send>>,
    /// To the writer thread: a program that stops reading its input fills
    /// the PTY, and a write would then block the app.
    writer: Option<mpsc::Sender<Vec<u8>>>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    /// Hold at most this much unread output, then stop reading.
    backpressure: Option<usize>,
}

impl LocalPty {
    pub fn new(command: impl Into<Command>) -> LocalPty {
        LocalPty {
            command: command.into(),
            shared: Arc::default(),
            master: None,
            writer: None,
            killer: None,
            backpressure: None,
        }
    }

    /// Hold at most about `bytes` of output that has not been
    /// [read](PtyHost::read) (one read of the PTY more, at most 64 KiB);
    /// past it, stop reading the program's output until some is read, so
    /// its writes wait, as they do on a slow terminal. Without this, output
    /// is never waited for: past 16 MiB unread, the oldest is dropped. For
    /// a reader that may fall behind and must not lose anything, such as a
    /// browser at the end of a network (rs-rich-web).
    pub fn backpressure(mut self, bytes: usize) -> LocalPty {
        self.backpressure = Some(bytes.max(1));
        self
    }

    fn size(columns: u16, rows: u16) -> PtySize {
        PtySize {
            rows: rows.max(1),
            cols: columns.max(1),
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

impl PtyHost for LocalPty {
    fn start(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        if self.master.is_some() {
            return Ok(());
        }
        let pty = native_pty_system()
            .openpty(LocalPty::size(columns, rows))
            .map_err(io::Error::other)?;
        let mut child = pty
            .slave
            .spawn_command(self.command.builder())
            .map_err(io::Error::other)?;
        // The child holds the only other end: its exit ends the output.
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().map_err(io::Error::other)?;
        let mut writer = pty.master.take_writer().map_err(io::Error::other)?;
        let (input, keys) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            for bytes in keys {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        self.writer = Some(input);
        self.killer = Some(child.clone_killer());
        self.master = Some(pty.master);

        let shared = Arc::clone(&self.shared);
        let limit = self.backpressure;
        thread::spawn(move || {
            let mut chunk = [0u8; 65536];
            loop {
                if let Some(limit) = limit {
                    if !wait_for_room(&shared, limit) {
                        break;
                    }
                }
                let read = match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                {
                    let mut output = lock(&shared);
                    output.buffer.extend(&chunk[..read]);
                    let over = output.buffer.len().saturating_sub(MAX_BUFFERED);
                    output.buffer.drain(..over);
                }
                notify(&shared);
            }
            lock(&shared).eof = true;
            notify(&shared);
        });

        let shared = Arc::clone(&self.shared);
        thread::spawn(move || {
            let status = match child.wait() {
                Ok(status) => ExitStatus::from(status),
                Err(error) => ExitStatus::with_signal(error.to_string()),
            };
            // What it wrote before exiting is still on its way through the
            // PTY: give the reader a moment to reach the end of it, so the
            // exit is reported after the last output. A reader paused for
            // room is not reading: its moment waits for it.
            let mut waited = 0;
            while waited < 50 {
                let (eof, paused) = {
                    let output = lock(&shared);
                    (output.eof, output.paused)
                };
                if eof {
                    break;
                }
                if !paused {
                    waited += 1;
                }
                thread::sleep(Duration::from_millis(10));
            }
            lock(&shared).status = Some(status);
            notify(&shared);
        });
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match &self.writer {
            Some(writer) => writer
                .send(bytes.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the program has gone")),
            None => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "the program has not started",
            )),
        }
    }

    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        match &self.master {
            Some(master) => master
                .resize(LocalPty::size(columns, rows))
                .map_err(io::Error::other),
            None => Ok(()),
        }
    }

    fn read(&mut self) -> Vec<u8> {
        lock(&self.shared).buffer.drain(..).collect()
    }

    fn set_notify(&mut self, notify: Notify) {
        let waiting = {
            let mut output = lock(&self.shared);
            output.notify = Some(notify.clone());
            !output.buffer.is_empty() || output.status.is_some()
        };
        if waiting {
            notify();
        }
    }

    fn exit_status(&mut self) -> Option<ExitStatus> {
        let output = lock(&self.shared);
        if output.buffer.is_empty() {
            output.status.clone()
        } else {
            None
        }
    }

    fn kill(&mut self) -> io::Result<()> {
        if lock(&self.shared).status.is_some() {
            return Ok(());
        }
        match &mut self.killer {
            Some(killer) => killer.kill(),
            None => Ok(()),
        }
    }
}

impl Drop for LocalPty {
    fn drop(&mut self) {
        lock(&self.shared).closed = true;
        let _ = self.kill();
    }
}

/// Wait until the unread output is under `limit`; `false` once the host
/// has gone.
fn wait_for_room(shared: &Mutex<Output>, limit: usize) -> bool {
    loop {
        {
            let mut output = lock(shared);
            if output.closed {
                return false;
            }
            output.paused = output.buffer.len() >= limit;
            if !output.paused {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// What a [`ReplayHost`] was asked to do, and what it is to play: shared
/// with its [`ReplayHandle`]s.
#[derive(Default)]
struct Replay {
    /// Output to play once started.
    script: Vec<u8>,
    /// How it ends once the script has played, if it does.
    script_exit: Option<ExitStatus>,
    /// Output waiting to be read.
    pending: Vec<u8>,
    status: Option<ExitStatus>,
    started: Option<(u16, u16)>,
    written: Vec<u8>,
    sizes: Vec<(u16, u16)>,
    killed: bool,
    notify: Option<Notify>,
}

/// A host that plays bytes back instead of running anything: for tests,
/// demos and recordings. What it plays is given up front
/// ([`output`](Self::output), [`exit`](Self::exit)) or later through a
/// [`ReplayHandle`], which also sees what the pane sent it.
///
/// ```
/// use rich_embed::{ExitStatus, ReplayHost};
///
/// let host = ReplayHost::new().output("hello\r\n").exit(ExitStatus::with_code(0));
/// let handle = host.handle();
/// assert!(handle.written().is_empty());
/// ```
#[derive(Default)]
pub struct ReplayHost {
    state: Arc<Mutex<Replay>>,
}

impl ReplayHost {
    pub fn new() -> ReplayHost {
        ReplayHost::default()
    }

    /// Play `bytes` as the program's output once it starts.
    pub fn output(self, bytes: impl AsRef<[u8]>) -> ReplayHost {
        lock(&self.state).script.extend_from_slice(bytes.as_ref());
        self
    }

    /// Exit with `status` once the output has played.
    pub fn exit(self, status: ExitStatus) -> ReplayHost {
        lock(&self.state).script_exit = Some(status);
        self
    }

    /// A handle to feed more output, end the program, and see what was
    /// sent to it. It may go to another thread.
    pub fn handle(&self) -> ReplayHandle {
        ReplayHandle {
            state: Arc::clone(&self.state),
        }
    }
}

/// Feeds a [`ReplayHost`] and reads back what it was sent.
#[derive(Clone)]
pub struct ReplayHandle {
    state: Arc<Mutex<Replay>>,
}

impl ReplayHandle {
    fn notify(&self) {
        let notify = lock(&self.state).notify.clone();
        if let Some(notify) = notify {
            notify();
        }
    }

    /// Output from the program, now (it waits for the start if the host has
    /// not started yet).
    pub fn feed(&self, bytes: impl AsRef<[u8]>) {
        {
            let mut state = lock(&self.state);
            if state.started.is_some() {
                state.pending.extend_from_slice(bytes.as_ref());
            } else {
                state.script.extend_from_slice(bytes.as_ref());
            }
        }
        self.notify();
    }

    /// The program exits with `status`.
    pub fn exit(&self, status: ExitStatus) {
        {
            let mut state = lock(&self.state);
            if state.started.is_some() {
                state.status = Some(status);
            } else {
                state.script_exit = Some(status);
            }
        }
        self.notify();
    }

    /// Every byte the pane sent: keys, the mouse, pastes.
    pub fn written(&self) -> Vec<u8> {
        lock(&self.state).written.clone()
    }

    /// The bytes sent since the last call.
    pub fn take_written(&self) -> Vec<u8> {
        std::mem::take(&mut lock(&self.state).written)
    }

    /// The size it started at, once it has.
    pub fn started(&self) -> Option<(u16, u16)> {
        lock(&self.state).started
    }

    /// Every resize after the start, as columns and rows.
    pub fn sizes(&self) -> Vec<(u16, u16)> {
        lock(&self.state).sizes.clone()
    }

    /// Whether it was killed.
    pub fn killed(&self) -> bool {
        lock(&self.state).killed
    }
}

impl PtyHost for ReplayHost {
    fn start(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        {
            let mut state = lock(&self.state);
            if state.started.is_some() {
                return Ok(());
            }
            state.started = Some((columns, rows));
            let script = std::mem::take(&mut state.script);
            state.pending.extend_from_slice(&script);
            state.status = state.script_exit.take();
        }
        self.handle().notify();
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        lock(&self.state).written.extend_from_slice(bytes);
        Ok(())
    }

    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        lock(&self.state).sizes.push((columns, rows));
        Ok(())
    }

    fn read(&mut self) -> Vec<u8> {
        std::mem::take(&mut lock(&self.state).pending)
    }

    fn set_notify(&mut self, notify: Notify) {
        let waiting = {
            let mut state = lock(&self.state);
            state.notify = Some(notify.clone());
            !state.pending.is_empty() || state.status.is_some()
        };
        if waiting {
            notify();
        }
    }

    fn exit_status(&mut self) -> Option<ExitStatus> {
        let state = lock(&self.state);
        if state.pending.is_empty() {
            state.status.clone()
        } else {
            None
        }
    }

    fn kill(&mut self) -> io::Result<()> {
        let mut state = lock(&self.state);
        state.killed = true;
        if state.status.is_none() && state.started.is_some() {
            state.status = Some(ExitStatus::with_signal("Killed"));
        }
        Ok(())
    }
}
