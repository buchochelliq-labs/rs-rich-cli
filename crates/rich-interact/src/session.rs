//! The terminal session: raw mode, the alternate screen, mouse and paste
//! modes, the kitty keyboard protocol, and their restoration on every way
//! out (#489).
//!
//! Everything a [`Session`] turns on is recorded in a process-wide flag
//! set, and undone by whichever comes first: [`Session::leave`], dropping
//! the session (an early return, `?`), or a panic, through a hook installed
//! on first use that restores the terminal before the panic message prints
//! (a panic caught by [`catch_panic`], which the session survives, leaves
//! it alone).
//! Ctrl+C arrives as a key in raw mode, so it ends the event loop the
//! ordinary way. On Unix, SIGTERM, SIGHUP and SIGQUIT restore the terminal
//! too, from a thread that then takes the signal's default action. Ctrl+Z
//! (a key in raw mode) and SIGTSTP suspend: the terminal is given back, the
//! process stops as the shell expects, and on `fg` the modes come back and
//! the view repaints. [`Session::handoff`] gives the terminal to another program
//! (`$EDITOR`, a pager) and takes it back.
//!
//! A session drives the terminal with one library, its
//! [`BackendKind`]: crossterm by default, or termion (#677) or termwiz
//! (#678) behind their features. The library reads keys, the mouse and
//! resizes, measures the terminal, and turns raw mode and the alternate
//! screen on and off; what is turned on, and how it is given back on every
//! way out above, is the same whichever it is.
//!
//! The terminal's modes are process-wide, so only one session exists at a
//! time: starting a second while the first is alive (a component that
//! calls [`run`](crate::run) from inside another) fails with
//! [`io::ErrorKind::ResourceBusy`] and changes nothing.

use std::cell::Cell;
use std::fmt;
use std::io::{self, Write};
use std::panic::UnwindSafe;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

use crate::event::{from_crossterm, from_crossterm_kitty, Event};

#[cfg(all(unix, feature = "termion"))]
mod with_termion;
#[cfg(feature = "termwiz")]
mod with_termwiz;

const RAW: u8 = 1;
const ALTERNATE: u8 = 2;
const MOUSE: u8 = 4;
const PASTE: u8 = 8;
const KITTY: u8 = 16;

/// The kitty keyboard protocol's flags a session pushes: disambiguate the
/// keys a legacy terminal sends alike (1) and report releases and repeats
/// (2). Not every key as an escape code (8): typed text would then arrive
/// as a base key and modifiers rather than the character the layout makes,
/// so plain text keys, Enter, Tab and Backspace report no release. The
/// terminal keeps a stack of flags for each screen, so they are pushed
/// after entering the alternate screen and popped before leaving it.
/// termwiz reads the first flag's keys but not the second's releases, so
/// a termwiz session pushes the first only.
const PUSH_KITTY: &str = "\x1b[>3u";
#[cfg(feature = "termwiz")]
const PUSH_KITTY_KEYS: &str = "\x1b[>1u";
const POP_KITTY: &str = "\x1b[<1u";

/// Whether the terminal answered the kitty keyboard protocol's query: 0 not
/// asked yet, 1 no, 2 yes. Asked once a process, since asking can wait for
/// a terminal that does not answer.
static KITTY_ANSWER: AtomicU8 = AtomicU8::new(0);

/// What is currently turned on, for the panic hook.
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static HOOK: Once = Once::new();
/// Whether a [`Session`] is alive. Held from `start` until drop.
static OWNED: AtomicBool = AtomicBool::new(false);
/// Whether the live session paints to standard error, for the panic hook.
static ON_STDERR: AtomicBool = AtomicBool::new(false);
/// The library the live session drives the terminal with
/// ([`BackendKind::index`]), for the panic hook and the signal thread.
static LIBRARY: AtomicU8 = AtomicU8::new(0);

/// The library a [`Session`] drives the terminal with
/// ([`SessionOptions::backend`]).
///
/// Each reads keys, the mouse and resizes its own way and turns raw mode
/// and the alternate screen on and off itself; the modes a session turns
/// on, and their restoration on every way out, are the same for all of
/// them. termion and termwiz are behind the crate's features of the same
/// names, off by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BackendKind {
    /// crossterm, on every platform: the default. With a terminal that has
    /// the kitty keyboard protocol, every key is
    /// [`exact`](crate::Key::exact) and releases arrive.
    #[default]
    Crossterm,
    /// termion (feature `termion`), on Unix. Keys arrive as a legacy
    /// terminal sends them, whatever the terminal: termion does not read
    /// the kitty keyboard protocol. The mouse comes without modifiers, and
    /// without movement while no button is held.
    #[cfg(all(unix, feature = "termion"))]
    Termion,
    /// termwiz (feature `termwiz`), on Unix and Windows. With a terminal
    /// that has the kitty keyboard protocol every key is exact, but releases
    /// do not arrive: termwiz does not read them. Its probe of the terminal
    /// (terminfo, `COLORTERM`) decides the colours an
    /// [`EventLoop`](crate::EventLoop) paints with.
    #[cfg(feature = "termwiz")]
    Termwiz,
}

impl BackendKind {
    /// Every backend this build has, crossterm first.
    pub const ALL: &'static [BackendKind] = &[
        BackendKind::Crossterm,
        #[cfg(all(unix, feature = "termion"))]
        BackendKind::Termion,
        #[cfg(feature = "termwiz")]
        BackendKind::Termwiz,
    ];

    /// The library's name: `crossterm`, `termion` or `termwiz`.
    pub fn name(self) -> &'static str {
        match self {
            BackendKind::Crossterm => "crossterm",
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion => "termion",
            #[cfg(feature = "termwiz")]
            BackendKind::Termwiz => "termwiz",
        }
    }

    /// The backend named `name` (any case), if this build has it.
    pub fn from_name(name: &str) -> Option<BackendKind> {
        BackendKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.name().eq_ignore_ascii_case(name))
    }

    fn index(self) -> u8 {
        match self {
            BackendKind::Crossterm => 0,
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion => 1,
            #[cfg(feature = "termwiz")]
            BackendKind::Termwiz => 2,
        }
    }

    /// The library the live session uses.
    fn live() -> BackendKind {
        let index = LIBRARY.load(Ordering::SeqCst);
        BackendKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.index() == index)
            .unwrap_or_default()
    }

    /// Turn raw mode on or off. `wait`: wait for the library's lock (off
    /// in the panic hook, whose thread may hold it).
    fn raw_mode(self, on: bool, wait: bool) -> io::Result<()> {
        let _ = wait;
        match self {
            BackendKind::Crossterm if on => crossterm::terminal::enable_raw_mode(),
            BackendKind::Crossterm => crossterm::terminal::disable_raw_mode(),
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion => with_termion::raw_mode(on, wait),
            #[cfg(feature = "termwiz")]
            BackendKind::Termwiz => with_termwiz::raw_mode(on, wait),
        }
    }

    /// Whether the library enters and leaves the alternate screen with a
    /// call of its own, keeping track of it (termwiz), rather than having
    /// the session write it with the other modes.
    fn owns_alternate_screen(self) -> bool {
        #[cfg(feature = "termwiz")]
        if self == BackendKind::Termwiz {
            return true;
        }
        false
    }

    /// Enter or leave the alternate screen through a library that
    /// [owns it](Self::owns_alternate_screen); `None` for one that has the
    /// session write it, as [`alternate_sequence`](Self::alternate_sequence)
    /// says.
    fn alternate_screen(self, on: bool, wait: bool) -> Option<io::Result<()>> {
        let _ = (on, wait);
        match self {
            #[cfg(feature = "termwiz")]
            BackendKind::Termwiz => Some(with_termwiz::alternate_screen(on, wait)),
            _ => None,
        }
    }

    /// The sequence that enters (`on`) or leaves the alternate screen, for
    /// a library without a call of its own: termion's
    /// `ToAlternateScreen` and `ToMainScreen`, and crossterm's same bytes.
    fn alternate_sequence(self, on: bool) -> String {
        match self {
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion if on => termion::screen::ToAlternateScreen.to_string(),
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion => termion::screen::ToMainScreen.to_string(),
            _ if on => "\x1b[?1049h".into(),
            _ => "\x1b[?1049l".into(),
        }
    }

    /// The kitty keyboard flags this library reads keys with.
    fn kitty_push(self) -> &'static str {
        match self {
            #[cfg(feature = "termwiz")]
            BackendKind::Termwiz => PUSH_KITTY_KEYS,
            _ => PUSH_KITTY,
        }
    }

    /// Give back what the library holds once the session is over.
    fn release(self) {
        match self {
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion => with_termion::release(),
            #[cfg(feature = "termwiz")]
            BackendKind::Termwiz => with_termwiz::release(),
            _ => {}
        }
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Where a session paints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Output {
    /// Standard output.
    #[default]
    Stdout,
    /// Standard error, which leaves standard output to the answer a script
    /// captures: `choice=$(rich choose a b c)`. Keys still come from the
    /// terminal, which crossterm opens itself when standard input is a pipe.
    Stderr,
}

impl Output {
    /// Whether the stream is a terminal.
    pub fn is_terminal(self) -> bool {
        use std::io::IsTerminal;
        match self {
            Output::Stdout => io::stdout().is_terminal(),
            Output::Stderr => io::stderr().is_terminal(),
        }
    }

    fn write(self, text: &str) -> io::Result<()> {
        match self {
            Output::Stdout => {
                let mut out = io::stdout().lock();
                out.write_all(text.as_bytes())?;
                out.flush()
            }
            Output::Stderr => {
                let mut out = io::stderr().lock();
                out.write_all(text.as_bytes())?;
                out.flush()
            }
        }
    }

    /// The live session's stream.
    fn live() -> Output {
        if ON_STDERR.load(Ordering::SeqCst) {
            Output::Stderr
        } else {
            Output::Stdout
        }
    }
}

/// Whether keys can be read from a terminal: standard input, or the
/// process's controlling terminal when standard input is a pipe.
pub(crate) fn keys_available() -> bool {
    use std::io::IsTerminal;
    if io::stdin().is_terminal() {
        return true;
    }
    #[cfg(unix)]
    let terminal = "/dev/tty";
    #[cfg(windows)]
    let terminal = "CONIN$";
    #[cfg(any(unix, windows))]
    return std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(terminal)
        .is_ok();
    #[cfg(not(any(unix, windows)))]
    false
}

/// Undo whatever `ACTIVE` records. Safe to call twice. `wait`: as for
/// [`BackendKind::raw_mode`].
fn restore(wait: bool) -> io::Result<()> {
    let active = ACTIVE.swap(0, Ordering::SeqCst);
    if active == 0 {
        return Ok(());
    }
    let library = BackendKind::live();
    let out = undo(active, library);
    let written = Output::live().write(&out);
    if active & ALTERNATE != 0 {
        if let Some(left) = library.alternate_screen(false, wait) {
            left?;
        }
    }
    if active & RAW != 0 {
        library.raw_mode(false, wait)?;
    }
    written
}

/// The sequences that turn off what `active` records: all of it, but for
/// the alternate screen of a library that leaves it itself.
fn undo(active: u8, library: BackendKind) -> String {
    let mut out = String::new();
    if active & KITTY != 0 {
        out.push_str(POP_KITTY);
    }
    if active & MOUSE != 0 {
        out.push_str("\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l");
    }
    if active & PASTE != 0 {
        out.push_str("\x1b[?2004l");
    }
    out.push_str("\x1b[?25h");
    if active & ALTERNATE != 0 && !library.owns_alternate_screen() {
        out.push_str(&library.alternate_sequence(false));
    }
    out
}

/// Raw mode is on for a line read without echo
/// ([`StdLineIo::read_secret`](crate::policy::StdLineIo)), outside any
/// session: the signal thread turns it off too.
static RAW_LINE: AtomicBool = AtomicBool::new(false);
#[cfg(unix)]
static SIGNALS: Once = Once::new();

/// Record that a line is being read in raw mode (`on`), or no longer is.
pub(crate) fn raw_line(on: bool) {
    if on {
        watch_signals();
    }
    RAW_LINE.store(on, Ordering::SeqCst);
}

/// SIGTERM, SIGHUP and SIGQUIT end the process without unwinding, so
/// neither `Drop` nor the panic hook would give the terminal back. From
/// the first session on, a thread waits for them: it restores whatever is
/// on, then takes the signal's default action (the process ends as it
/// would have). The handlers stay installed once the session ends, since
/// removing them would leave the signals ignored; with nothing on they
/// only take the default action.
fn watch_signals() {
    #[cfg(unix)]
    SIGNALS.call_once(|| {
        use signal_hook::consts::{SIGHUP, SIGQUIT, SIGTERM, SIGTSTP};
        let Ok(mut signals) =
            signal_hook::iterator::Signals::new([SIGTERM, SIGHUP, SIGQUIT, SIGTSTP])
        else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("rich-interact-signals".into())
            .spawn(move || {
                for signal in signals.forever() {
                    if signal == SIGTSTP {
                        suspend();
                        RESUMED.store(true, Ordering::SeqCst);
                    } else {
                        restore_for_signal();
                        let _ = signal_hook::low_level::emulate_default_handler(signal);
                    }
                }
            });
    });
}

/// [`restore`] from the signal thread. The sequences go straight to the
/// terminal rather than through the standard stream's lock, which the
/// interrupted thread may hold.
#[cfg(unix)]
fn restore_for_signal() {
    let active = ACTIVE.swap(0, Ordering::SeqCst);
    let library = BackendKind::live();
    if active != 0 {
        write_direct(&undo(active, library));
    }
    if active & ALTERNATE != 0 {
        let _ = library.alternate_screen(false, true);
    }
    if active & RAW != 0 {
        let _ = library.raw_mode(false, true);
    }
    if RAW_LINE.swap(false, Ordering::SeqCst) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Set when the process came back from a suspend it did not start itself
/// (SIGTSTP from outside): [`Session::read`] then asks for a repaint.
static RESUMED: AtomicBool = AtomicBool::new(false);

/// Give the terminal back, stop the process as a shell's job control
/// expects (Ctrl+Z, `kill -TSTP`), and when it continues (`fg`, SIGCONT)
/// turn back on what was on. SIGSTOP cannot be caught, so it still stops
/// with the modes on.
/// Held for the whole of a suspend, from giving the terminal back until its
/// modes are on again, and by [`Session::leave`]. A resume from an outside
/// SIGTSTP runs on the signal thread while the event loop runs on, so
/// without it a session that ended during the stop (a key queued, a timer
/// due) would have its modes turned back on after it restored them.
static SUSPENDING: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(unix)]
fn suspend() {
    let _suspending = SUSPENDING.lock().unwrap_or_else(|e| e.into_inner());
    let active = ACTIVE.load(Ordering::SeqCst);
    let raw_line = RAW_LINE.load(Ordering::SeqCst);
    let library = BackendKind::live();
    restore_for_signal();
    // Stops here until SIGCONT.
    let _ = signal_hook::low_level::emulate_default_handler(signal_hook::consts::SIGTSTP);
    if active & RAW != 0 {
        let _ = library.raw_mode(true, true);
    }
    if raw_line {
        let _ = crossterm::terminal::enable_raw_mode();
    }
    RAW_LINE.store(raw_line, Ordering::SeqCst);
    let mut out = String::new();
    if active & ALTERNATE != 0 {
        if library.alternate_screen(true, true).is_none() {
            out.push_str(&library.alternate_sequence(true));
        }
        out.push_str("\x1b[H");
    }
    if active & KITTY != 0 {
        out.push_str(library.kitty_push());
    }
    if active & MOUSE != 0 {
        out.push_str("\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h");
    }
    if active & PASTE != 0 {
        out.push_str("\x1b[?2004h");
    }
    if active != 0 {
        out.push_str("\x1b[?25l");
    }
    ACTIVE.fetch_or(active, Ordering::SeqCst);
    write_direct(&out);
}

/// `text` straight to the terminal, else to the session's stream.
#[cfg(unix)]
fn write_direct(text: &str) {
    if text.is_empty() {
        return;
    }
    let direct = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .and_then(|mut tty| tty.write_all(text.as_bytes()));
    if direct.is_err() {
        let _ = Output::live().write(text);
    }
}

/// A library's lock, for a backend that keeps its terminal where the panic
/// hook and the signal thread reach it. With `wait` off it gives up at once
/// rather than wait for a holder, which may be the panicking thread itself.
#[cfg(any(all(unix, feature = "termion"), feature = "termwiz"))]
fn lock<T>(mutex: &std::sync::Mutex<T>, wait: bool) -> io::Result<std::sync::MutexGuard<'_, T>> {
    use std::sync::TryLockError;
    if wait {
        return Ok(mutex.lock().unwrap_or_else(|e| e.into_inner()));
    }
    match mutex.try_lock() {
        Ok(guard) => Ok(guard),
        Err(TryLockError::Poisoned(e)) => Ok(e.into_inner()),
        Err(TryLockError::WouldBlock) => Err(io::ErrorKind::WouldBlock.into()),
    }
}

/// Ask the terminal `query` through `/dev/tty`, and read until `done` says
/// the answer is complete, for two seconds at most (a terminal that does
/// not answer). `None` when it does not answer in time. Keys typed while it
/// waits are lost.
#[cfg(all(unix, any(feature = "termion", feature = "termwiz")))]
fn ask_terminal(query: &str, done: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    tty.write_all(query.as_bytes()).ok()?;
    tty.flush().ok()?;
    let end = Instant::now() + Duration::from_secs(2);
    let mut answer = Vec::new();
    while !done(&answer) {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        let mut ready = [filedescriptor::pollfd {
            fd: tty.as_raw_fd(),
            events: filedescriptor::POLLIN,
            revents: 0,
        }];
        match filedescriptor::poll(&mut ready, Some(left)) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(filedescriptor::Error::Poll(e)) if e.kind() == io::ErrorKind::Interrupted => {
                continue
            }
            Err(_) => return None,
        }
        let mut buffer = [0u8; 256];
        match tty.read(&mut buffer) {
            Ok(0) | Err(_) => return None,
            Ok(read) => answer.extend_from_slice(&buffer[..read]),
        }
    }
    Some(answer)
}

/// The final byte of each complete `ESC [ ? <digits and ;> <final>` in
/// `bytes`, in order: the shape of the device attributes answer (`c`) and
/// of the kitty flags one (`u`).
#[cfg(all(unix, feature = "termwiz"))]
fn private_answers(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    let mut at = 0;
    std::iter::from_fn(move || {
        while at + 3 <= bytes.len() {
            if bytes[at..].starts_with(b"\x1b[?") {
                let mut end = at + 3;
                while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b';') {
                    end += 1;
                }
                if end < bytes.len() {
                    at = end + 1;
                    return Some(bytes[end]);
                }
                return None;
            }
            at += 1;
        }
        None
    })
}

/// Whether the terminal has the kitty keyboard protocol, asked directly:
/// its flags, then the device attributes, which every terminal answers, so
/// a terminal without the protocol answers only the second.
#[cfg(all(unix, feature = "termwiz"))]
fn ask_kitty() -> bool {
    let answer = ask_terminal("\x1b[?u\x1b[c", |bytes| {
        private_answers(bytes).any(|last| last == b'c')
    });
    answer.is_some_and(|bytes| private_answers(&bytes).any(|last| last == b'u'))
}

/// The cursor's row (0-based), asked directly (`CSI 6 n`).
#[cfg(all(unix, any(feature = "termion", feature = "termwiz")))]
fn ask_cursor_row() -> Option<u16> {
    fn report(bytes: &[u8]) -> Option<u16> {
        let start = bytes.windows(2).position(|w| w == b"\x1b[")? + 2;
        let end = start + bytes[start..].iter().position(|&b| b == b'R')?;
        let text = std::str::from_utf8(&bytes[start..end]).ok()?;
        let (row, _) = text.split_once(';')?;
        row.parse::<u16>().ok().map(|row| row.saturating_sub(1))
    }
    report(&ask_terminal("\x1b[6n", |bytes| report(bytes).is_some())?)
}

thread_local! {
    /// How many [`catch_panic`] calls are running on this thread.
    static CATCHING: Cell<usize> = const { Cell::new(0) };
}

/// [`std::panic::catch_unwind`] for code that runs inside a live session
/// and carries on after a panic (a plugin component's
/// [`PluginView`](crate::plugin::PluginView)): the session's panic hook
/// neither gives the terminal back nor prints the panic message over the
/// view for a panic caught here, since the session goes on. A panic nothing
/// catches still restores the terminal before its message prints.
pub fn catch_panic<R>(f: impl FnOnce() -> R + UnwindSafe) -> std::thread::Result<R> {
    CATCHING.with(|depth| depth.set(depth.get() + 1));
    let result = std::panic::catch_unwind(f);
    CATCHING.with(|depth| depth.set(depth.get() - 1));
    result
}

fn install_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Caught on this thread while a session is live: the session
            // runs on, and the catcher reports the panic itself.
            if CATCHING.with(Cell::get) > 0 && ACTIVE.load(Ordering::SeqCst) != 0 {
                return;
            }
            let _ = restore(false);
            previous(info);
        }));
    });
}

/// What a session turns on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionOptions {
    /// Take over the whole screen, and give it back as it was on exit.
    /// Without it, components paint inline, below the cursor.
    pub alternate_screen: bool,
    /// Report mouse clicks and the wheel.
    pub mouse: bool,
    /// Deliver pasted text as one [`Event::Paste`].
    pub bracketed_paste: bool,
    /// Where to paint.
    pub output: Output,
    /// Read keys as a legacy terminal sends them, even from a terminal with
    /// the kitty keyboard protocol. Without it, a terminal that answers the
    /// protocol's query has it turned on: Ctrl+I and Tab are told apart
    /// (see [`Key::matches`](crate::Key::matches)), and key releases arrive
    /// as [`Event::KeyUp`] (with crossterm; see [`BackendKind`]).
    pub legacy_keys: bool,
    /// The library that drives the terminal: crossterm unless this says
    /// otherwise.
    pub backend: BackendKind,
}

/// Where the event loop reads events and writes paints: the terminal, or
/// the [headless](crate::headless) driver.
pub trait Backend {
    /// (columns, rows).
    fn size(&self) -> (u16, u16);
    /// The next event, or `None` when `timeout` passes first. `None` for
    /// `timeout` waits as long as it takes.
    fn read(&mut self, timeout: Option<Duration>) -> io::Result<Option<Event>>;
    /// Write and flush.
    fn write(&mut self, text: &str) -> io::Result<()>;
    /// Time since the backend started (virtual in the headless driver).
    fn elapsed(&self) -> Duration;
    /// Run `command` with the terminal as it was before the session, then
    /// take the terminal back. Returns the exit code.
    fn handoff(&mut self, command: &mut Command) -> io::Result<Option<i32>>;
    /// Whether the region is the alternate screen (painted from the top).
    fn alternate_screen(&self) -> bool;
    /// The terminal row an inline region started on, for placing mouse
    /// events; 0 when unknown. The alternate screen always starts at 0.
    fn origin(&self) -> u16 {
        0
    }
    /// The plain text of a view just painted: the headless driver records
    /// it.
    fn painted(&mut self, text: &str) {
        let _ = text;
    }
    /// Whether Ctrl+Z suspends (a real terminal on Unix), rather than
    /// reaching the component as a key.
    fn can_suspend(&self) -> bool {
        false
    }
    /// Give the terminal back, stop until the shell continues the process,
    /// then take the terminal back. Only called when
    /// [`can_suspend`](Backend::can_suspend).
    fn suspend(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// Whether the process was suspended from outside (SIGTSTP) and has
    /// continued since the last call: the screen is then the shell's.
    fn take_resumed(&mut self) -> bool {
        false
    }
    /// Whether [`copy`](Backend::copy) reaches a clipboard (#488): `Ok`
    /// when it does, or why not. The default: it does not.
    fn clipboard(&self) -> Result<(), String> {
        Err("this backend has no clipboard".into())
    }
    /// Put `text` on the clipboard. Only called when
    /// [`clipboard`](Backend::clipboard) is `Ok`.
    fn copy(&mut self, text: &str) -> io::Result<()> {
        let _ = text;
        Ok(())
    }
}

/// How a session reads the terminal: the library's reader, opened on the
/// first [`Session::enter`] and kept across hand-offs and suspends.
enum Input {
    Crossterm,
    #[cfg(all(unix, feature = "termion"))]
    Termion(with_termion::Reader),
    #[cfg(feature = "termwiz")]
    Termwiz {
        held: crate::event::HeldButton,
        probed: with_termwiz::Probed,
    },
}

impl Input {
    fn open(kind: BackendKind, output: Output) -> io::Result<Input> {
        let _ = output;
        Ok(match kind {
            BackendKind::Crossterm => Input::Crossterm,
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion => Input::Termion(with_termion::Reader::open()?),
            #[cfg(feature = "termwiz")]
            BackendKind::Termwiz => Input::Termwiz {
                held: Default::default(),
                probed: with_termwiz::open(output)?,
            },
        })
    }
}

/// The real terminal: keys from it, paints to standard output or standard
/// error ([`SessionOptions::output`]), through the library
/// [`SessionOptions::backend`] names.
pub struct Session {
    options: SessionOptions,
    start: Instant,
    active: bool,
    /// The row the cursor was on when the session started (inline, with
    /// the mouse on).
    origin: u16,
    /// Whether the terminal takes OSC 52 copies, detected at the start.
    clipboard: rich_ext::clipboard::Clipboard,
    /// Whether keys are read with the kitty keyboard protocol.
    kitty: bool,
    /// `None` until the first `enter`.
    input: Option<Input>,
}

/// Whether the terminal has the kitty keyboard protocol, asked once a
/// process with `ask` (with raw mode on): a terminal that answers neither
/// the protocol's query nor the device attributes one after it keeps the
/// session waiting (two seconds).
fn kitty_answered(ask: impl FnOnce() -> bool) -> bool {
    match KITTY_ANSWER.load(Ordering::SeqCst) {
        0 => {
            let yes = query_reaches_terminal() && ask();
            KITTY_ANSWER.store(if yes { 2 } else { 1 }, Ordering::SeqCst);
            yes
        }
        answer => answer == 2,
    }
}

/// Whether crossterm's keyboard query would reach the terminal. It means
/// to write to `/dev/tty`, but opens it read-only, so the query always goes
/// to standard output: in `answer=$(rich write)` it would land in the
/// answer, and the terminal's reply at the shell prompt afterwards. The
/// other backends ask the same way, so a terminal is asked or not
/// whichever drives it.
fn query_reaches_terminal() -> bool {
    use std::io::IsTerminal;
    io::stdout().is_terminal()
}

impl Session {
    /// Turn on raw mode and the options. Fails, changing nothing, when the
    /// terminal cannot do raw mode or another session is alive.
    pub fn start(options: SessionOptions) -> io::Result<Session> {
        if OWNED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::ResourceBusy,
                "a terminal session is already running",
            ));
        }
        install_hook();
        watch_signals();
        // From here on, dropping the session releases ownership, including
        // when `enter` fails part way.
        let mut session = Session {
            options,
            start: Instant::now(),
            active: false,
            origin: 0,
            clipboard: rich_ext::clipboard::Clipboard::detect(
                &crate::clipboard::SessionEnvironment,
            ),
            kitty: false,
            input: None,
        };
        session.enter()?;
        Ok(session)
    }

    fn enter(&mut self) -> io::Result<()> {
        let library = self.options.backend;
        ON_STDERR.store(self.options.output == Output::Stderr, Ordering::SeqCst);
        LIBRARY.store(library.index(), Ordering::SeqCst);
        if self.input.is_none() {
            self.input = Some(Input::open(library, self.options.output)?);
        }
        library.raw_mode(true, true)?;
        ACTIVE.fetch_or(RAW, Ordering::SeqCst);
        // Active as soon as anything is on, so a failure below still
        // restores on drop.
        self.active = true;
        let mut out = String::new();
        if self.options.alternate_screen {
            ACTIVE.fetch_or(ALTERNATE, Ordering::SeqCst);
            match library.alternate_screen(true, true) {
                Some(entered) => entered?,
                None => out.push_str(&library.alternate_sequence(true)),
            }
            out.push_str("\x1b[H");
        }
        self.kitty = !self.options.legacy_keys && self.kitty_answered();
        if self.kitty {
            out.push_str(library.kitty_push());
            ACTIVE.fetch_or(KITTY, Ordering::SeqCst);
        }
        let (mouse, paste) = self.capable();
        if self.options.mouse && mouse {
            out.push_str("\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h");
            ACTIVE.fetch_or(MOUSE, Ordering::SeqCst);
        }
        if self.options.bracketed_paste && paste {
            out.push_str("\x1b[?2004h");
            ACTIVE.fetch_or(PASTE, Ordering::SeqCst);
        }
        out.push_str("\x1b[?25l");
        self.options.output.write(&out)?;
        // Where an inline region starts, so clicks land on the right rows.
        // Asking writes a query to standard output, so only when that is
        // the terminal being painted.
        if self.options.mouse
            && !self.options.alternate_screen
            && self.options.output == Output::Stdout
        {
            self.origin = self.cursor_row().unwrap_or(0);
        }
        Ok(())
    }

    /// Whether the library reads the kitty keyboard protocol's keys, and
    /// the terminal has it.
    fn kitty_answered(&self) -> bool {
        match self.options.backend {
            BackendKind::Crossterm => kitty_answered(|| {
                crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
            }),
            // termion's reader does not know the protocol's keys.
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion => false,
            #[cfg(all(unix, feature = "termwiz"))]
            BackendKind::Termwiz => kitty_answered(ask_kitty),
            #[cfg(all(not(unix), feature = "termwiz"))]
            BackendKind::Termwiz => false,
        }
    }

    /// Whether the library's probe of the terminal allows the mouse and
    /// bracketed paste.
    fn capable(&self) -> (bool, bool) {
        match &self.input {
            #[cfg(feature = "termwiz")]
            Some(Input::Termwiz { probed, .. }) => (probed.mouse, probed.paste),
            _ => (true, true),
        }
    }

    /// The cursor's row, asked of the terminal.
    fn cursor_row(&self) -> Option<u16> {
        match self.options.backend {
            BackendKind::Crossterm => crossterm::cursor::position().ok().map(|(_, row)| row),
            #[cfg(all(unix, any(feature = "termion", feature = "termwiz")))]
            #[allow(unreachable_patterns)]
            _ => ask_cursor_row(),
            #[cfg(all(not(unix), feature = "termwiz"))]
            _ => None,
        }
    }

    /// The terminal's size, as the library measures it.
    fn measure(&self) -> io::Result<(u16, u16)> {
        match &self.input {
            #[cfg(all(unix, feature = "termion"))]
            Some(Input::Termion(reader)) => reader.size(),
            #[cfg(feature = "termwiz")]
            Some(Input::Termwiz { .. }) => with_termwiz::size(),
            _ => crossterm::terminal::size(),
        }
    }

    /// The next event within `wait`, or `None`: the wait passed, or the
    /// library read something components do not see (focus changes, a
    /// sequence it does not know).
    fn next_event(&mut self, wait: Duration) -> io::Result<Option<Event>> {
        let kitty = self.kitty;
        match &mut self.input {
            #[cfg(all(unix, feature = "termion"))]
            Some(Input::Termion(reader)) => reader.next(wait),
            #[cfg(feature = "termwiz")]
            Some(Input::Termwiz { held, probed }) => Ok(with_termwiz::poll(probed, wait)?
                .and_then(|event| {
                    if kitty {
                        crate::event::from_termwiz_kitty(event, held)
                    } else {
                        crate::event::from_termwiz(event, held)
                    }
                })),
            _ => {
                if !crossterm::event::poll(wait)? {
                    return Ok(None);
                }
                let event = crossterm::event::read()?;
                Ok(if kitty {
                    from_crossterm_kitty(event)
                } else {
                    from_crossterm(event)
                })
            }
        }
    }

    /// Restore the terminal. Also done on drop and on panic.
    pub fn leave(&mut self) -> io::Result<()> {
        if !std::mem::take(&mut self.active) {
            return Ok(());
        }
        // After any suspend under way has turned the modes back on, so
        // this restores them rather than being undone by it.
        let _suspending = SUSPENDING.lock().unwrap_or_else(|e| e.into_inner());
        restore(true)
    }

    pub fn options(&self) -> SessionOptions {
        self.options
    }

    /// Whether keys are read with the kitty keyboard protocol: the terminal
    /// has it, the backend reads it, and [`SessionOptions::legacy_keys`] is
    /// off. Every key is then [`exact`](crate::Key::exact), and with
    /// crossterm releases arrive.
    pub fn kitty_keys(&self) -> bool {
        self.kitty
    }

    /// The colours the backend's own probe of the terminal found, when it
    /// has one (termwiz's: terminfo and `COLORTERM`); `None` to leave it to
    /// rich's detection. `Some(None)` is no colour.
    pub fn color_system(&self) -> Option<Option<rich::ColorSystem>> {
        match &self.input {
            #[cfg(feature = "termwiz")]
            Some(Input::Termwiz { probed, .. }) => Some(probed.color),
            _ => None,
        }
    }

    /// A resize to the current size, which repaints the whole view: after a
    /// suspend, what is on the screen is the shell's.
    fn repaint(&mut self) -> Event {
        if self.options.mouse
            && !self.options.alternate_screen
            && self.options.output == Output::Stdout
        {
            self.origin = self.cursor_row().unwrap_or(0);
        }
        let (columns, rows) = self.measure().unwrap_or((80, 24));
        Event::Resize { columns, rows }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.leave();
        if self.input.take().is_some() {
            self.options.backend.release();
        }
        OWNED.store(false, Ordering::SeqCst);
    }
}

impl Backend for Session {
    fn size(&self) -> (u16, u16) {
        self.measure().unwrap_or((80, 24))
    }

    fn read(&mut self, timeout: Option<Duration>) -> io::Result<Option<Event>> {
        let end = timeout.map(|timeout| Instant::now() + timeout);
        loop {
            // Back from a SIGTSTP sent from outside: the screen is the
            // shell's, so repaint all of it.
            if RESUMED.load(Ordering::SeqCst) {
                return Ok(Some(self.repaint()));
            }
            // Short waits, so a resume is noticed without a key.
            let wait = match end {
                Some(end) => end.saturating_duration_since(Instant::now()),
                None => Duration::from_secs(3600),
            }
            .min(Duration::from_millis(250));
            // Events the library reports that components do not see (focus)
            // are skipped, not returned as a timeout.
            if let Some(event) = self.next_event(wait)? {
                return Ok(Some(event));
            }
            if end.is_some_and(|end| Instant::now() >= end) {
                return Ok(None);
            }
        }
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        self.options.output.write(text)
    }

    fn clipboard(&self) -> Result<(), String> {
        let field = self.clipboard.field();
        if field.value {
            Ok(())
        } else {
            Err(field.reason.clone())
        }
    }

    fn copy(&mut self, text: &str) -> io::Result<()> {
        let sequence = rich_ext::clipboard::osc52(text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        self.options.output.write(&sequence)
    }

    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    fn handoff(&mut self, command: &mut Command) -> io::Result<Option<i32>> {
        self.leave()?;
        let status = command.status();
        self.enter()?;
        Ok(status?.code())
    }

    fn alternate_screen(&self) -> bool {
        self.options.alternate_screen
    }

    fn origin(&self) -> u16 {
        self.origin
    }

    fn can_suspend(&self) -> bool {
        cfg!(unix)
    }

    fn suspend(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            suspend();
            self.repaint();
        }
        Ok(())
    }

    fn take_resumed(&mut self) -> bool {
        RESUMED.swap(false, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backends_are_named() {
        assert_eq!(BackendKind::default(), BackendKind::Crossterm);
        assert_eq!(BackendKind::ALL[0], BackendKind::Crossterm);
        for kind in BackendKind::ALL {
            assert_eq!(BackendKind::from_name(kind.name()), Some(*kind));
            assert_eq!(
                BackendKind::from_name(&kind.name().to_uppercase()),
                Some(*kind)
            );
            assert_eq!(kind.to_string(), kind.name());
        }
        assert_eq!(BackendKind::from_name("curses"), None);
        assert_eq!(
            BackendKind::ALL.len(),
            1 + usize::from(cfg!(all(unix, feature = "termion")))
                + usize::from(cfg!(feature = "termwiz"))
        );
    }

    #[test]
    fn every_backend_leaves_the_same_modes() {
        let all = RAW | ALTERNATE | MOUSE | PASTE | KITTY;
        let expected =
            "\x1b[<1u\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?2004l\x1b[?25h";
        for kind in BackendKind::ALL {
            let out = undo(all, *kind);
            assert!(out.starts_with(expected), "{kind}: {out:?}");
            // The alternate screen last: in the sequences, or by the
            // library's own call.
            let rest = &out[expected.len()..];
            assert!(rest == "\x1b[?1049l" || rest.is_empty(), "{kind}: {rest:?}");
            assert_eq!(rest.is_empty(), kind.owns_alternate_screen());
        }
    }

    #[cfg(all(unix, feature = "termwiz"))]
    #[test]
    fn private_answers_are_found() {
        let answers: Vec<u8> = private_answers(b"x\x1b[?0u\x1b[?62;22c").collect();
        assert_eq!(answers, b"uc");
        assert_eq!(private_answers(b"\x1b[?62").count(), 0);
    }
}
