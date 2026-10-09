//! The terminal session: raw mode, the alternate screen, mouse and paste
//! modes, the kitty keyboard protocol, synchronized output, and their
//! restoration on every way out (#489).
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
//! Where the terminal has synchronized output (DEC private mode 2026), each
//! frame is written as one synchronized update: between `CSI ? 2026 h` and
//! `CSI ? 2026 l`, so the terminal shows it whole, without tearing, and a
//! screen reader that follows the cursor does not see it jump across each
//! changed region. The terminal is asked in the start-up query that asks
//! for the kitty keyboard protocol, so detecting it costs no extra wait;
//! `RICH_SYNC_OUTPUT` and [`SessionOptions::synchronized_output`] override
//! what it answers.
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
/// Frames are written as synchronized updates: the restore ends one a write
/// cut short left open.
const SYNC: u8 = 32;

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

/// What the terminal answered the start-up query ([`Answers::encode`]): 0
/// not asked yet. Asked once a process, since asking can wait for a
/// terminal that does not answer.
static ANSWERS: AtomicU8 = AtomicU8::new(0);

/// Begins a synchronized update (DEC private mode 2026): the terminal holds
/// the screen until [`END_SYNCHRONIZED_UPDATE`].
pub const BEGIN_SYNCHRONIZED_UPDATE: &str = "\x1b[?2026h";
/// Ends a synchronized update: the terminal shows what was written since
/// [`BEGIN_SYNCHRONIZED_UPDATE`] in one go.
pub const END_SYNCHRONIZED_UPDATE: &str = "\x1b[?2026l";
/// Asks whether the terminal knows mode 2026 (DECRQM). Sent first, before
/// the kitty query and the device attributes: a reply arriving after the
/// session stopped waiting then runs into theirs, which crossterm's parser
/// reads as one answer of its own, never as keys.
#[cfg(unix)]
const ASK_SYNC: &str = "\x1b[?2026$p";
/// The environment variable that turns synchronized output off (`0`) or on
/// (`1`) whatever the terminal answers.
pub const SYNC_OUTPUT_ENV: &str = "RICH_SYNC_OUTPUT";

/// `frame` as one synchronized update; nothing for an empty frame.
pub fn synchronized_update(frame: &str) -> String {
    if frame.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(
        BEGIN_SYNCHRONIZED_UPDATE.len() + frame.len() + END_SYNCHRONIZED_UPDATE.len(),
    );
    out.push_str(BEGIN_SYNCHRONIZED_UPDATE);
    out.push_str(frame);
    out.push_str(END_SYNCHRONIZED_UPDATE);
    out
}

/// What `RICH_SYNC_OUTPUT`'s `value` forces: `0` off, `1` on; anything else,
/// or unset, leaves it to the terminal's answer.
fn sync_override(value: Option<&str>) -> Option<bool> {
    match value.map(str::trim) {
        Some("0") => Some(false),
        Some("1") => Some(true),
        _ => None,
    }
}

/// What the environment forces synchronized output to, if anything.
fn sync_from_env() -> Option<bool> {
    sync_override(std::env::var(SYNC_OUTPUT_ENV).ok().as_deref())
}

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

    /// What to write once the library's raw mode is off, for what turning
    /// it off leaves on: termwiz's raw mode sets xterm's modifyOtherKeys to
    /// level 2 and its cooked mode to 1, and only dropping its terminal
    /// sets it back to 0, as a shell or a program handed the terminal
    /// expects.
    fn after_raw_mode(self) -> &'static str {
        match self {
            #[cfg(all(unix, feature = "termwiz"))]
            BackendKind::Termwiz => "\x1b[>4;0m",
            _ => "",
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
    // Every step is tried, so a failed one leaves no other on; the first
    // error is the one returned.
    let written = Output::live().write(&out);
    let left = if active & ALTERNATE != 0 {
        library.alternate_screen(false, wait).unwrap_or(Ok(()))
    } else {
        Ok(())
    };
    let raw = if active & RAW != 0 {
        let off = library.raw_mode(false, wait);
        let after = match library.after_raw_mode() {
            "" => Ok(()),
            after => Output::live().write(after),
        };
        off.and(after)
    } else {
        Ok(())
    };
    written.and(left).and(raw)
}

/// The sequences that turn off what `active` records: all of it, but for
/// the alternate screen of a library that leaves it itself. A synchronized
/// update first, so a frame cut short (a panic, a signal mid-write) is
/// shown and the rest are not held.
fn undo(active: u8, library: BackendKind) -> String {
    let mut out = String::new();
    if active & SYNC != 0 {
        out.push_str(END_SYNCHRONIZED_UPDATE);
    }
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
        write_direct(library.after_raw_mode());
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
/// not answer). `None` when it does not answer in time. What it returns
/// holds anything else read meanwhile, such as keys typed.
#[cfg(unix)]
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

/// One complete `ESC [ ? <parameters> <intermediates> <final>` in what the
/// terminal answered: the shape of the device attributes answer (`c`), the
/// kitty flags one (`u`) and DECRQM's (`$ y`).
#[cfg(any(unix, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PrivateAnswer<'a> {
    parameters: &'a [u8],
    intermediates: &'a [u8],
    last: u8,
}

/// Each complete private answer in `bytes`, in order. Anything else (keys
/// typed meanwhile, an answer cut short) is passed over.
#[cfg(any(unix, test))]
fn private_answers(bytes: &[u8]) -> impl Iterator<Item = PrivateAnswer<'_>> + '_ {
    let mut at = 0;
    std::iter::from_fn(move || {
        while at + 3 <= bytes.len() {
            if !bytes[at..].starts_with(b"\x1b[?") {
                at += 1;
                continue;
            }
            let start = at + 3;
            let mut end = start;
            while end < bytes.len() && (0x30..=0x3f).contains(&bytes[end]) {
                end += 1;
            }
            let middle = end;
            while end < bytes.len() && (0x20..=0x2f).contains(&bytes[end]) {
                end += 1;
            }
            match bytes.get(end) {
                None => return None,
                Some(&last @ 0x40..=0x7e) => {
                    at = end + 1;
                    return Some(PrivateAnswer {
                        parameters: &bytes[start..middle],
                        intermediates: &bytes[middle..end],
                        last,
                    });
                }
                // Not an answer: look again from the byte that broke it.
                Some(_) => at = end,
            }
        }
        None
    })
}

/// What the terminal answered the start-up query.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Answers {
    /// It has the kitty keyboard protocol.
    kitty: bool,
    /// It knows synchronized output (mode 2026).
    sync: bool,
}

impl Answers {
    /// For [`ANSWERS`]: never 0.
    fn encode(self) -> u8 {
        1 | (u8::from(self.kitty) << 1) | (u8::from(self.sync) << 2)
    }

    fn decode(bits: u8) -> Answers {
        Answers {
            kitty: bits & 2 != 0,
            sync: bits & 4 != 0,
        }
    }

    /// The answers in `bytes`, once the device attributes, which every
    /// terminal answers last, have come; `None` before. A DECRQM report of
    /// mode 2026 set, reset or permanently set (1, 2, 3) means the terminal
    /// knows it; not recognised (0) or permanently reset (4), or no report,
    /// means it does not.
    #[cfg(any(unix, test))]
    fn read(bytes: &[u8]) -> Option<Answers> {
        let mut answers = Answers::default();
        for answer in private_answers(bytes) {
            match (answer.last, answer.intermediates) {
                (b'c', b"") => return Some(answers),
                (b'u', b"") => answers.kitty = true,
                (b'y', b"$") => {
                    if let Some(state) = answer.parameters.strip_prefix(b"2026;") {
                        answers.sync = matches!(state, b"1" | b"2" | b"3");
                    }
                }
                _ => {}
            }
        }
        None
    }
}

/// The start-up query, asked directly: synchronized output, the kitty
/// keyboard protocol's flags, then the device attributes, which every
/// terminal answers, so a terminal without the others answers only the
/// last. One round trip for both questions.
#[cfg(unix)]
fn ask_start() -> Answers {
    let query = format!("{ASK_SYNC}\x1b[?u\x1b[c");
    ask_terminal(&query, |bytes| Answers::read(bytes).is_some())
        .and_then(|bytes| Answers::read(&bytes))
        .unwrap_or_default()
}

/// The first complete cursor position report (`ESC [ <row> ; <column> R`)
/// in `bytes`: its row (0-based), and where it is. Anything else (a key
/// typed meanwhile, a mouse report, an answer cut short) is passed over.
#[cfg(any(all(unix, any(feature = "termion", feature = "termwiz")), test))]
fn cursor_report(bytes: &[u8]) -> Option<(u16, std::ops::Range<usize>)> {
    let digits = |from: usize| {
        from + bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut at = 0;
    while let Some(found) = bytes[at..].windows(2).position(|w| w == b"\x1b[") {
        let start = at + found;
        let row_end = digits(start + 2);
        if row_end > start + 2 && bytes.get(row_end) == Some(&b';') {
            let end = digits(row_end + 1);
            if end > row_end + 1 && bytes.get(end) == Some(&b'R') {
                let row = std::str::from_utf8(&bytes[start + 2..row_end]).map(str::parse::<u16>);
                if let Ok(Ok(row)) = row {
                    return Some((row.saturating_sub(1), start..end + 1));
                }
            }
        }
        at = start + 1;
    }
    None
}

/// The cursor's row (0-based), asked directly (`CSI 6 n`), and the rest of
/// what was read with the answer (keys typed meanwhile), for the reader.
#[cfg(all(unix, any(feature = "termion", feature = "termwiz")))]
fn ask_cursor_row() -> (Option<u16>, Vec<u8>) {
    let Some(bytes) = ask_terminal("\x1b[6n", |bytes| cursor_report(bytes).is_some()) else {
        return (None, Vec::new());
    };
    match cursor_report(&bytes) {
        Some((row, at)) => {
            let mut rest = bytes[..at.start].to_vec();
            rest.extend_from_slice(&bytes[at.end..]);
            (Some(row), rest)
        }
        None => (None, bytes),
    }
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
    /// Write each frame as one synchronized update (DEC private mode 2026):
    /// `Some(true)` always, `Some(false)` never. `None`, the default, leaves
    /// it to `RICH_SYNC_OUTPUT` (`0` off, `1` on) and then to the terminal,
    /// which is asked in the kitty keyboard protocol's start-up query. A
    /// session that asks nothing ([`legacy_keys`](Self::legacy_keys), the
    /// termion backend, standard output not a terminal) leaves it off
    /// unless this or the environment turns it on.
    pub synchronized_output: Option<bool>,
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
    /// Whether [`write_frame`](Backend::write_frame) writes each frame as
    /// one synchronized update. The default: no.
    fn synchronized_output(&self) -> bool {
        false
    }
    /// Write a frame the painter made: nothing at all when it is empty,
    /// and with [`synchronized_output`](Backend::synchronized_output)
    /// between `CSI ? 2026 h` and `CSI ? 2026 l`, in the one write.
    fn write_frame(&mut self, frame: &str) -> io::Result<()> {
        if frame.is_empty() {
            Ok(())
        } else if self.synchronized_output() {
            self.write(&synchronized_update(frame))
        } else {
            self.write(frame)
        }
    }
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
    /// Whether frames are written as synchronized updates.
    sync: bool,
    /// `None` until the first `enter`.
    input: Option<Input>,
}

/// What the terminal answered the start-up query, asked once a process
/// with `ask` (with raw mode on): a terminal that answers none of it, not
/// even the device attributes query at its end, keeps the session waiting
/// (two seconds).
fn answered(ask: impl FnOnce() -> Answers) -> Answers {
    match ANSWERS.load(Ordering::SeqCst) {
        0 => {
            let answers = if query_reaches_terminal() {
                ask()
            } else {
                Answers::default()
            };
            ANSWERS.store(answers.encode(), Ordering::SeqCst);
            answers
        }
        bits => Answers::decode(bits),
    }
}

/// Whether the start-up query is asked. crossterm's own keyboard query
/// meant to write to `/dev/tty`, but opened it read-only, so it went to
/// standard output: in `answer=$(rich write)` it would have landed in the
/// answer, and the terminal's reply at the shell prompt afterwards. The
/// session asks directly now, but still only with standard output a
/// terminal, so a captured answer starts as it did; every backend asks the
/// same way, so a terminal is asked or not whichever drives it.
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
            sync: false,
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
        // The start-up query asks for the kitty keyboard protocol and
        // synchronized output in one round trip. With nothing to read the
        // protocol's keys (legacy keys, termion), it is not asked at all:
        // synchronized output then waits on being forced.
        let answers = if self.options.legacy_keys {
            Answers::default()
        } else {
            self.answered()
        };
        self.kitty = answers.kitty;
        if self.kitty {
            out.push_str(library.kitty_push());
            ACTIVE.fetch_or(KITTY, Ordering::SeqCst);
        }
        self.sync = self
            .options
            .synchronized_output
            .or_else(sync_from_env)
            .unwrap_or(answers.sync);
        if self.sync {
            ACTIVE.fetch_or(SYNC, Ordering::SeqCst);
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

    /// What the terminal answered the start-up query, for a library that
    /// reads the kitty keyboard protocol's keys; nothing asked for one that
    /// does not.
    fn answered(&self) -> Answers {
        match self.options.backend {
            #[cfg(unix)]
            BackendKind::Crossterm => answered(ask_start),
            // Windows: crossterm's own answer, which reads no reply.
            #[cfg(not(unix))]
            BackendKind::Crossterm => answered(|| Answers {
                kitty: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
                sync: false,
            }),
            // termion's reader does not know the protocol's keys.
            #[cfg(all(unix, feature = "termion"))]
            BackendKind::Termion => Answers::default(),
            #[cfg(all(unix, feature = "termwiz"))]
            BackendKind::Termwiz => answered(ask_start),
            #[cfg(all(not(unix), feature = "termwiz"))]
            BackendKind::Termwiz => Answers::default(),
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

    /// The cursor's row, asked of the terminal. Keys that came with the
    /// answer go to the reader.
    fn cursor_row(&mut self) -> Option<u16> {
        match self.options.backend {
            BackendKind::Crossterm => crossterm::cursor::position().ok().map(|(_, row)| row),
            #[cfg(all(unix, any(feature = "termion", feature = "termwiz")))]
            #[allow(unreachable_patterns)]
            _ => {
                let (row, rest) = ask_cursor_row();
                match &mut self.input {
                    #[cfg(feature = "termion")]
                    Some(Input::Termion(reader)) => reader.unread(&rest),
                    #[cfg(feature = "termwiz")]
                    Some(Input::Termwiz { probed, .. }) => with_termwiz::unread(probed, &rest),
                    _ => {}
                }
                row
            }
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

    /// Whether frames are written as synchronized updates: forced by
    /// [`SessionOptions::synchronized_output`] or `RICH_SYNC_OUTPUT`, or
    /// the terminal answered that it knows mode 2026.
    pub fn synchronized_output(&self) -> bool {
        self.sync
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

    fn synchronized_output(&self) -> bool {
        self.sync
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

    #[test]
    fn private_answers_are_found() {
        let lasts = |bytes: &[u8]| -> Vec<u8> { private_answers(bytes).map(|a| a.last).collect() };
        assert_eq!(lasts(b"x\x1b[?0u\x1b[?62;22c"), b"uc");
        assert_eq!(lasts(b"\x1b[?62"), b"");
        let decrqm: Vec<_> = private_answers(b"\x1b[?2026;2$y").collect();
        assert_eq!(
            decrqm,
            [PrivateAnswer {
                parameters: b"2026;2",
                intermediates: b"$",
                last: b'y',
            }]
        );
        // An answer broken off by another sequence is passed over, and the
        // next one still read.
        assert_eq!(lasts(b"\x1b[?2026\x1b[?62c"), b"c");
    }

    #[test]
    fn the_start_up_answers_are_read() {
        let read = |bytes: &[u8]| Answers::read(bytes);
        let both = Answers {
            kitty: true,
            sync: true,
        };
        let sync = Answers {
            kitty: false,
            sync: true,
        };
        // Supported: set or reset (recognised), or permanently set.
        assert_eq!(read(b"\x1b[?2026;2$y\x1b[?0u\x1b[?62;22c"), Some(both));
        assert_eq!(read(b"\x1b[?2026;1$y\x1b[?62c"), Some(sync));
        assert_eq!(read(b"\x1b[?2026;3$y\x1b[?62c"), Some(sync));
        // Not recognised, or permanently reset: not supported.
        assert_eq!(read(b"\x1b[?2026;0$y\x1b[?62c"), Some(Answers::default()));
        assert_eq!(read(b"\x1b[?2026;4$y\x1b[?62c"), Some(Answers::default()));
        // No report at all (a terminal that does not know DECRQM).
        assert_eq!(
            read(b"\x1b[?1u\x1b[?62c"),
            Some(Answers {
                kitty: true,
                sync: false,
            })
        );
        // Another mode's report says nothing about 2026.
        assert_eq!(read(b"\x1b[?2004;1$y\x1b[?62c"), Some(Answers::default()));
        // Until the device attributes come, the answer is not complete:
        // the session waits, then goes on with neither.
        assert_eq!(read(b""), None);
        assert_eq!(read(b"\x1b[?2026;2$y\x1b[?0u"), None);
        assert_eq!(read(b"\x1b[?2026;2$y\x1b[?6"), None);
        // Keys typed while it waits, between and inside the answers' runs,
        // are passed over.
        assert_eq!(
            read(b"j\x1b[A\x1b[?2026;2$yk\x1b[?0u\r\x1b[?62;22c"),
            Some(both)
        );
        for answers in [Answers::default(), sync, both] {
            assert_ne!(answers.encode(), 0);
            assert_eq!(Answers::decode(answers.encode()), answers);
        }
    }

    #[test]
    fn a_cursor_report_is_found_among_other_input() {
        assert_eq!(cursor_report(b"\x1b[12;1R"), Some((11, 0..7)));
        // After an arrow key, a mouse report or a sequence cut short, and
        // with a key after it.
        assert_eq!(cursor_report(b"\x1b[A\x1b[12;1R"), Some((11, 3..10)));
        assert_eq!(
            cursor_report(b"\x1b[<0;2;13M\x1b[3;40Rx"),
            Some((2, 10..17))
        );
        assert_eq!(cursor_report(b"\x1b[1;\x1b[5;1R"), Some((4, 4..10)));
        // Not complete, or not a report.
        for bytes in [
            &b"\x1b[12;"[..],
            b"\x1b[12R",
            b"\x1b[;1R",
            b"\x1b[1;5C",
            b"12;1R",
        ] {
            assert_eq!(cursor_report(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn the_environment_forces_synchronized_output() {
        assert_eq!(sync_override(Some("0")), Some(false));
        assert_eq!(sync_override(Some("1")), Some(true));
        assert_eq!(sync_override(Some(" 1\n")), Some(true));
        assert_eq!(sync_override(None), None);
        assert_eq!(sync_override(Some("")), None);
        assert_eq!(sync_override(Some("auto")), None);
    }

    #[test]
    fn a_frame_is_one_synchronized_update_and_an_empty_one_nothing() {
        assert_eq!(
            synchronized_update("\x1b[2;1Hhi\x1b[?25l"),
            "\x1b[?2026h\x1b[2;1Hhi\x1b[?25l\x1b[?2026l"
        );
        assert_eq!(synchronized_update(""), "");
    }

    #[test]
    fn the_restore_ends_a_synchronized_update_first() {
        for kind in BackendKind::ALL {
            let out = undo(RAW | SYNC | MOUSE, *kind);
            assert!(out.starts_with("\x1b[?2026l\x1b[?1006l"), "{kind}: {out:?}");
            assert_eq!(out.matches("2026").count(), 1);
            // Off, nothing about it.
            assert!(!undo(RAW | MOUSE, *kind).contains("2026"));
        }
    }
}
