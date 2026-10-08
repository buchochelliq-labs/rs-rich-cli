//! The termwiz backend (#678): raw mode, the alternate screen, the
//! terminal's size and its events through termwiz's terminal, on Unix and
//! Windows.
//!
//! termwiz probes the terminal (terminfo, `COLORTERM`, `NO_COLOR`) for its
//! capabilities: the session reads the colours and whether the mouse and
//! bracketed paste are to be used from that probe, and turns the mouse and
//! paste on itself, as with every backend, so termwiz is told to leave
//! them alone.
//!
//! The terminal lives behind a lock, which the signal thread takes to give
//! the terminal back for a suspend. So on Unix a read waits for input on
//! the terminal without holding it, and only takes it to have termwiz
//! read what arrived: held through termwiz's own wait, the lock would be
//! taken again the moment it was let go, and the suspend kept waiting.

use std::io;
use std::sync::Mutex;
use std::time::Duration;

use rich::ColorSystem;
use termwiz::caps::{Capabilities, ColorLevel, ProbeHints};
use termwiz::input::InputEvent;
use termwiz::terminal::{SystemTerminal, Terminal};

use super::Output;

/// termwiz's terminal, kept where the panic hook and the signal thread
/// reach it.
static TERMINAL: Mutex<Option<SystemTerminal>> = Mutex::new(None);

/// What termwiz's probe found, and the terminal to wait on.
pub(super) struct Probed {
    pub(super) mouse: bool,
    pub(super) paste: bool,
    pub(super) color: Option<ColorSystem>,
    /// The terminal, for waiting until it has input.
    #[cfg(unix)]
    tty: std::fs::File,
}

fn other(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

/// Probe the terminal and open it: keys from the terminal itself (on Unix,
/// `/dev/tty`, so they come when standard input is a pipe), modes written
/// to `output`.
pub(super) fn open(output: Output) -> io::Result<Probed> {
    let probed = Capabilities::new_from_env().map_err(other)?;
    let caps = Capabilities::new_with_hints(
        ProbeHints::new_from_env()
            .mouse_reporting(Some(false))
            .bracketed_paste(Some(false)),
    )
    .map_err(other)?;
    #[cfg(unix)]
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")?;
    #[cfg(unix)]
    let terminal = match output {
        Output::Stdout => SystemTerminal::new_with(caps, &tty, &io::stdout()),
        Output::Stderr => SystemTerminal::new_with(caps, &tty, &io::stderr()),
    };
    #[cfg(windows)]
    let terminal = {
        let _ = output;
        SystemTerminal::new(caps)
    };
    *super::lock(&TERMINAL, true)? = Some(terminal.map_err(other)?);
    Ok(Probed {
        mouse: probed.mouse_reporting(),
        paste: probed.bracketed_paste(),
        color: match probed.color_level() {
            ColorLevel::MonoChrome => None,
            ColorLevel::Sixteen => Some(ColorSystem::Standard),
            ColorLevel::TwoFiftySix => Some(ColorSystem::EightBit),
            ColorLevel::TrueColor => Some(ColorSystem::Truecolor),
        },
        #[cfg(unix)]
        tty,
    })
}

/// Run `f` on the terminal, if it is open.
fn with<R>(
    wait: bool,
    f: impl FnOnce(&mut SystemTerminal) -> termwiz::Result<R>,
) -> io::Result<Option<R>> {
    let mut terminal = super::lock(&TERMINAL, wait)?;
    match terminal.as_mut() {
        Some(terminal) => f(terminal).map(Some).map_err(other),
        None => Ok(None),
    }
}

pub(super) fn raw_mode(on: bool, wait: bool) -> io::Result<()> {
    with(wait, |terminal| {
        if on {
            terminal.set_raw_mode()?;
        } else {
            terminal.set_cooked_mode()?;
        }
        terminal.flush()
    })
    .map(drop)
}

pub(super) fn alternate_screen(on: bool, wait: bool) -> io::Result<()> {
    with(wait, |terminal| {
        if on {
            terminal.enter_alternate_screen()?;
        } else {
            terminal.exit_alternate_screen()?;
        }
        terminal.flush()
    })
    .map(drop)
}

/// The terminal's size, as termwiz measures it.
pub(super) fn size() -> io::Result<(u16, u16)> {
    let size = with(true, |terminal| terminal.get_screen_size())?
        .ok_or_else(|| other("the termwiz terminal is closed"))?;
    let clamp = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    Ok((clamp(size.cols), clamp(size.rows)))
}

/// termwiz's next event within `wait` (or a little sooner).
pub(super) fn poll(probed: &Probed, wait: Duration) -> io::Result<Option<InputEvent>> {
    #[cfg(unix)]
    {
        // What termwiz has already: events parsed from an earlier read, a
        // resize.
        let now = || with(true, |terminal| terminal.poll_input(Some(Duration::ZERO)));
        if let Some(event) = now()?.flatten() {
            return Ok(Some(event));
        }
        // A resize arrives through termwiz's own pipe, which this wait
        // does not watch: short waits, so it is seen soon after.
        let mut ready = [filedescriptor::pollfd {
            fd: std::os::unix::io::AsRawFd::as_raw_fd(&probed.tty),
            events: filedescriptor::POLLIN,
            revents: 0,
        }];
        match filedescriptor::poll(&mut ready, Some(wait.min(Duration::from_millis(50)))) {
            Ok(_) => {}
            Err(filedescriptor::Error::Poll(e)) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(other(e)),
        }
        Ok(now()?.flatten())
    }
    #[cfg(not(unix))]
    {
        let _ = probed;
        Ok(with(true, |terminal| terminal.poll_input(Some(wait)))?.flatten())
    }
}

/// The session is over: drop termwiz's terminal, which puts back the mode
/// it found.
pub(super) fn release() {
    let terminal = super::lock(&TERMINAL, true)
        .ok()
        .and_then(|mut terminal| terminal.take());
    drop(terminal);
}
