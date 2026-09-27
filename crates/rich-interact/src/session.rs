//! The terminal session: raw mode, the alternate screen, mouse and paste
//! modes, and their restoration on every way out (#489).
//!
//! Everything a [`Session`] turns on is recorded in a process-wide flag
//! set, and undone by whichever comes first: [`Session::leave`], dropping
//! the session (an early return, `?`), or a panic, through a hook installed
//! on first use that restores the terminal before the panic message prints.
//! Ctrl+C arrives as a key in raw mode, so it ends the event loop the
//! ordinary way. [`Session::handoff`] gives the terminal to another program
//! (`$EDITOR`, a pager) and takes it back.
//!
//! The terminal's modes are process-wide, so only one session exists at a
//! time: starting a second while the first is alive (a component that
//! calls [`run`](crate::run) from inside another) fails with
//! [`io::ErrorKind::ResourceBusy`] and changes nothing.

use std::io::{self, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

use crate::event::{from_crossterm, Event};

const RAW: u8 = 1;
const ALTERNATE: u8 = 2;
const MOUSE: u8 = 4;
const PASTE: u8 = 8;

/// What is currently turned on, for the panic hook.
static ACTIVE: AtomicU8 = AtomicU8::new(0);
static HOOK: Once = Once::new();
/// Whether a [`Session`] is alive. Held from `start` until drop.
static OWNED: AtomicBool = AtomicBool::new(false);

/// Undo whatever `ACTIVE` records. Safe to call twice.
fn restore() -> io::Result<()> {
    let active = ACTIVE.swap(0, Ordering::SeqCst);
    if active == 0 {
        return Ok(());
    }
    let mut out = String::new();
    if active & MOUSE != 0 {
        out.push_str("\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l");
    }
    if active & PASTE != 0 {
        out.push_str("\x1b[?2004l");
    }
    out.push_str("\x1b[?25h");
    if active & ALTERNATE != 0 {
        out.push_str("\x1b[?1049l");
    }
    let mut stdout = io::stdout();
    let written = stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush());
    if active & RAW != 0 {
        crossterm::terminal::disable_raw_mode()?;
    }
    written
}

fn install_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = restore();
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
    /// The plain text of a view just painted: the headless driver records
    /// it.
    fn painted(&mut self, text: &str) {
        let _ = text;
    }
}

/// The real terminal, on stdin and stdout.
pub struct Session {
    options: SessionOptions,
    start: Instant,
    active: bool,
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
        // From here on, dropping the session releases ownership, including
        // when `enter` fails part way.
        let mut session = Session {
            options,
            start: Instant::now(),
            active: false,
        };
        session.enter()?;
        Ok(session)
    }

    fn enter(&mut self) -> io::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        ACTIVE.fetch_or(RAW, Ordering::SeqCst);
        // Active as soon as anything is on, so a failure below still
        // restores on drop.
        self.active = true;
        let mut out = String::new();
        if self.options.alternate_screen {
            out.push_str("\x1b[?1049h\x1b[H");
            ACTIVE.fetch_or(ALTERNATE, Ordering::SeqCst);
        }
        if self.options.mouse {
            out.push_str("\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h");
            ACTIVE.fetch_or(MOUSE, Ordering::SeqCst);
        }
        if self.options.bracketed_paste {
            out.push_str("\x1b[?2004h");
            ACTIVE.fetch_or(PASTE, Ordering::SeqCst);
        }
        out.push_str("\x1b[?25l");
        let mut stdout = io::stdout();
        stdout.write_all(out.as_bytes())?;
        stdout.flush()
    }

    /// Restore the terminal. Also done on drop and on panic.
    pub fn leave(&mut self) -> io::Result<()> {
        if !std::mem::take(&mut self.active) {
            return Ok(());
        }
        restore()
    }

    pub fn options(&self) -> SessionOptions {
        self.options
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.leave();
        OWNED.store(false, Ordering::SeqCst);
    }
}

impl Backend for Session {
    fn size(&self) -> (u16, u16) {
        crossterm::terminal::size().unwrap_or((80, 24))
    }

    fn read(&mut self, timeout: Option<Duration>) -> io::Result<Option<Event>> {
        let end = timeout.map(|timeout| Instant::now() + timeout);
        loop {
            let wait = match end {
                Some(end) => end.saturating_duration_since(Instant::now()),
                None => Duration::from_secs(3600),
            };
            if !crossterm::event::poll(wait)? {
                if end.is_some() {
                    return Ok(None);
                }
                continue;
            }
            // Events crossterm reports that components do not see (key
            // releases, focus) are skipped, not returned as a timeout.
            if let Some(event) = from_crossterm(crossterm::event::read()?) {
                return Ok(Some(event));
            }
        }
    }

    fn write(&mut self, text: &str) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        stdout.write_all(text.as_bytes())?;
        stdout.flush()
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
}
