//! What a terminal can draw beyond text (#573): the graphics protocol, and
//! the size of one cell in pixels.
//!
//! Core's `TargetCapabilities` stays as upstream has it (it knows only
//! Sixel), so these facts travel beside it in a [`GraphicsEnvironment`] built
//! from a capability [`Report`]. The cell size comes from the terminal's
//! window size in pixels (`TIOCGWINSZ`'s pixel fields, divided by its rows
//! and columns); where the terminal leaves those at zero,
//! [`GraphicsEnvironment::system`] asks with a `CSI 16 t` query, and only
//! when both stdin and stdout are a terminal, so a pipe is never written a
//! query and never waits for a reply.
//!
//! The query shares the terminal with the user, so it stays out of the way:
//! it is skipped unless the process is in the terminal's foreground process
//! group (a background job that touched the terminal's modes would be
//! stopped with `SIGTTOU`), and skipped when input is already waiting
//! (typeahead belongs to whatever reads next). A device-attributes request
//! (`CSI c`) follows it, so the reader stops as soon as the terminal has
//! answered both. A byte read during the exchange that is not part of a
//! reply is dropped, never pushed back on the terminal's input queue:
//! `TIOCSTI` would replay whatever arrived (another program's terminal
//! reply included) to the shell as typed input. Only a key pressed in the
//! milliseconds the terminal takes to answer can be lost that way.

use std::time::Duration;

use crate::capabilities::{
    Capabilities, ColorDepth, Environment, Field, Graphics, Origin, Report, SystemEnvironment,
};

/// The size of one terminal cell in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CellPixels {
    pub width: u16,
    pub height: u16,
}

impl CellPixels {
    /// A cell size, or `None` when either side is zero.
    pub fn new(width: u16, height: u16) -> Option<CellPixels> {
        (width > 0 && height > 0).then_some(CellPixels { width, height })
    }

    /// The cell size a window of `pixels` across `cells` implies.
    pub fn from_window(pixels: (u16, u16), cells: (u16, u16)) -> Option<CellPixels> {
        if cells.0 == 0 || cells.1 == 0 {
            return None;
        }
        CellPixels::new(pixels.0 / cells.0, pixels.1 / cells.1)
    }

    /// Parse `WIDTHxHEIGHT`, as `8x16`.
    pub fn parse(value: &str) -> Option<CellPixels> {
        let (w, h) = value.trim().split_once(['x', 'X'])?;
        CellPixels::new(w.trim().parse().ok()?, h.trim().parse().ok()?)
    }
}

impl std::fmt::Display for CellPixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// The cell size in a `CSI 16 t` reply (`ESC [ 6 ; height ; width t`), if
/// `bytes` holds one.
pub fn parse_cell_pixels_reply(bytes: &[u8]) -> Option<CellPixels> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find("\x1b[6;")?;
    let body = &text[start + 4..];
    let end = body.find('t')?;
    let (height, width) = body[..end].split_once(';')?;
    CellPixels::new(width.parse().ok()?, height.parse().ok()?)
}

/// Ask the controlling terminal for its cell size with `CSI 16 t`, waiting
/// at most `timeout` for the reply. The terminal is put in raw mode for the
/// exchange and restored after. `None` without a controlling terminal, on
/// Windows, when the process is not in the terminal's foreground process
/// group, when input is already waiting to be read, or when the terminal
/// does not answer in time.
///
/// Only call it when the program is interactive: it writes to and reads
/// from `/dev/tty`.
pub fn query_cell_pixels(timeout: Duration) -> Option<CellPixels> {
    #[cfg(unix)]
    {
        query_unix(timeout)
    }
    #[cfg(not(unix))]
    {
        let _ = timeout;
        None
    }
}

#[cfg(unix)]
fn query_unix(timeout: Duration) -> Option<CellPixels> {
    use rustix::fs::{open, Mode, OFlags};
    use rustix::termios::{tcgetattr, tcsetattr, OptionalActions};

    let tty = open(
        "/dev/tty",
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .ok()?;
    // A background job may not change the terminal's modes or read from it:
    // the kernel would stop it (SIGTTOU, SIGTTIN) until `fg`. Ask only from
    // the foreground, with both signals blocked so a job sent to the
    // background in between is not stopped (its read fails with EIO).
    if !in_foreground(&tty) {
        return None;
    }
    let _blocked = JobSignals::block()?;
    if !in_foreground(&tty) {
        return None;
    }
    let saved = tcgetattr(&tty).ok()?;
    let mut raw = saved.clone();
    raw.make_raw();
    tcsetattr(&tty, OptionalActions::Now, &raw).ok()?;
    // In raw mode a partly typed line is readable too: anything waiting is
    // the user's, for whatever reads next, so there is no query.
    let cell = if input_waiting(&tty) {
        None
    } else {
        exchange(&tty, timeout).0
    };
    let _ = tcsetattr(&tty, OptionalActions::Now, &saved);
    cell
}

/// Whether this process is in `tty`'s foreground process group.
#[cfg(unix)]
fn in_foreground(tty: &rustix::fd::OwnedFd) -> bool {
    rustix::termios::tcgetpgrp(tty).is_ok_and(|group| group == rustix::process::getpgrp())
}

/// Whether `tty` has input waiting, without waiting.
#[cfg(unix)]
fn input_waiting(tty: &rustix::fd::OwnedFd) -> bool {
    // An error counts as waiting: when in doubt, do not read.
    readable(tty, Duration::ZERO) != Some(false)
}

/// Whether `tty` has input to read within `timeout`; `None` on an error.
/// This is `select`, not `poll`: macOS's `poll` does not support terminal
/// devices (it answers `POLLNVAL` for `/dev/tty`).
#[cfg(unix)]
#[allow(unsafe_code)]
fn readable(tty: &rustix::fd::OwnedFd, timeout: Duration) -> Option<bool> {
    use rustix::fd::AsRawFd;
    let fd = tty.as_raw_fd();
    if !(0..libc::FD_SETSIZE as i32).contains(&fd) {
        return None;
    }
    let mut wait = libc::timeval {
        tv_sec: timeout.as_secs().try_into().unwrap_or(libc::time_t::MAX),
        tv_usec: timeout.subsec_micros() as _,
    };
    // SAFETY: `set` is initialised by FD_ZERO before use, `fd` is open and
    // below FD_SETSIZE, and select only reads and writes `set` and `wait`.
    let ready = unsafe {
        let mut set: libc::fd_set = std::mem::zeroed();
        libc::FD_ZERO(&mut set);
        libc::FD_SET(fd, &mut set);
        libc::select(
            fd + 1,
            &mut set,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut wait,
        )
    };
    match ready {
        0 => Some(false),
        n if n > 0 => Some(true),
        _ => None,
    }
}

/// `SIGTTOU` and `SIGTTIN` blocked on this thread until dropped.
#[cfg(unix)]
struct JobSignals(libc::sigset_t);

// rustix has no safe wrapper for the signal mask.
#[cfg(unix)]
#[allow(unsafe_code)]
impl JobSignals {
    fn block() -> Option<JobSignals> {
        // SAFETY: the sets are initialised by sigemptyset before use, and
        // pthread_sigmask only reads `set` and writes `old`.
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGTTOU);
            libc::sigaddset(&mut set, libc::SIGTTIN);
            let mut old: libc::sigset_t = std::mem::zeroed();
            (libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old) == 0).then_some(JobSignals(old))
        }
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
impl Drop for JobSignals {
    fn drop(&mut self) {
        // SAFETY: restores the mask `block` saved.
        unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.0, std::ptr::null_mut());
        }
    }
}

/// What one escape sequence read during the exchange turned out to be.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
enum Reply {
    /// Not finished yet.
    Partial,
    /// The `CSI 16 t` reply, when it names a cell size.
    CellSize(Option<CellPixels>),
    /// The device-attributes reply (`ESC [ ? … c`): the terminal has
    /// answered everything asked.
    Attributes,
    /// Not a reply to either question: the user's input.
    Other,
}

/// Classify `sequence`, which starts with ESC.
#[cfg_attr(not(unix), allow(dead_code))]
fn classify(sequence: &[u8]) -> Reply {
    let Some(rest) = sequence.strip_prefix(b"\x1b") else {
        return Reply::Other;
    };
    let Some((&first, rest)) = rest.split_first() else {
        return Reply::Partial;
    };
    if first != b'[' {
        return Reply::Other;
    }
    let (attributes, body) = match rest.split_first() {
        None => return Reply::Partial,
        Some((b'?', body)) => (true, body),
        Some(_) => (false, rest),
    };
    let Some((&last, params)) = body.split_last() else {
        return Reply::Partial;
    };
    let in_params = |b: &u8| b.is_ascii_digit() || *b == b';';
    if !params.iter().all(in_params) || sequence.len() > 32 {
        return Reply::Other;
    }
    match (attributes, last) {
        (_, b) if in_params(&b) => Reply::Partial,
        (true, b'c') => Reply::Attributes,
        (false, b't') => Reply::CellSize(parse_cell_pixels_reply(sequence)),
        _ => Reply::Other,
    }
}

/// Write the query and a device-attributes request, and read the replies
/// a byte at a time until the second arrives or `timeout` passes. Returns
/// the cell size and the bytes read that were not part of a reply.
#[cfg(unix)]
fn exchange(tty: &rustix::fd::OwnedFd, timeout: Duration) -> (Option<CellPixels>, Vec<u8>) {
    use std::time::Instant;

    if rustix::io::write(tty, b"\x1b[16t\x1b[c").is_err() {
        return (None, Vec::new());
    }
    let deadline = Instant::now() + timeout;
    let mut cell = None;
    let mut sequence: Vec<u8> = Vec::new();
    let mut stray: Vec<u8> = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if readable(tty, left) != Some(true) {
            break;
        }
        let mut byte = [0u8; 1];
        if !matches!(rustix::io::read(tty, &mut byte), Ok(1)) {
            break;
        }
        if sequence.is_empty() && byte[0] != 0x1b {
            stray.push(byte[0]);
            continue;
        }
        sequence.push(byte[0]);
        match classify(&sequence) {
            Reply::Partial => {}
            Reply::CellSize(found) => {
                cell = cell.or(found);
                sequence.clear();
            }
            Reply::Attributes => {
                sequence.clear();
                break;
            }
            Reply::Other => stray.append(&mut sequence),
        }
    }
    stray.append(&mut sequence);
    (cell, stray)
}

/// The window size stdout's terminal reports, as `(columns, rows)` and
/// `(x pixels, y pixels)`, when it is a terminal that says.
pub(crate) fn stdout_window() -> Option<((u16, u16), (u16, u16))> {
    #[cfg(unix)]
    {
        let size = rustix::termios::tcgetwinsize(std::io::stdout()).ok()?;
        Some(((size.ws_col, size.ws_row), (size.ws_xpixel, size.ws_ypixel)))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// What the terminal can draw beyond text, with where each answer came
/// from: the ext-side environment micro-asset renderers read (core's
/// `TargetCapabilities` is left as upstream has it).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct GraphicsEnvironment {
    /// The graphics protocol found (or overridden).
    pub graphics: Field<Graphics>,
    /// Whether Sixel was found, separately (WezTerm speaks both).
    pub sixel: Field<bool>,
    /// The size of one cell in pixels, when the terminal said.
    pub cell_pixels: Field<Option<CellPixels>>,
    /// stdout is a terminal.
    pub interactive: bool,
    /// Colour depth, for block renderings.
    pub color: ColorDepth,
    /// Whether emoji and block characters can be shown.
    pub unicode: bool,
    /// Whether animation may run (off with reduced motion or
    /// `RICH_ANIMATION=0`).
    pub animation: Field<bool>,
}

impl GraphicsEnvironment {
    /// From a capability report, reading the cell size from `env`: the
    /// `RICH_CELL_PIXELS=WxH` override, then the terminal's window size in
    /// pixels. Never queries the terminal.
    pub fn from_report(report: &Report, env: &dyn Environment) -> GraphicsEnvironment {
        let cell_pixels = match env.var("RICH_CELL_PIXELS").filter(|v| !v.is_empty()) {
            Some(value) => match CellPixels::parse(&value) {
                Some(cell) => Field::new(
                    Some(cell),
                    Origin::Environment("RICH_CELL_PIXELS".into()),
                    format!("RICH_CELL_PIXELS={cell}"),
                ),
                None => Field::new(None, Origin::Default, "RICH_CELL_PIXELS is not WxH"),
            },
            None => match env.cell_pixels().and_then(|(w, h)| CellPixels::new(w, h)) {
                Some(cell) if report.interactive.value => Field::new(
                    Some(cell),
                    Origin::Inferred,
                    "terminal window size in pixels",
                ),
                _ => Field::new(None, Origin::Default, "the terminal did not report pixels"),
            },
        };
        GraphicsEnvironment {
            graphics: report.graphics.clone(),
            sixel: report.sixel.clone(),
            cell_pixels,
            interactive: report.interactive.value,
            color: report.color.value,
            unicode: report.unicode.value,
            animation: report.animation.clone(),
        }
    }

    /// Detect from `env`, never querying the terminal.
    pub fn detect(env: &dyn Environment) -> GraphicsEnvironment {
        GraphicsEnvironment::from_report(&Capabilities::detect(env), env)
    }

    /// Detect from the process. When the cell size is still unknown and both
    /// stdin and stdout are a terminal, ask the terminal (`CSI 16 t`,
    /// waiting at most 100 ms) — from the foreground only, and only when no
    /// input is waiting ([`query_cell_pixels`]).
    pub fn system() -> GraphicsEnvironment {
        use std::io::IsTerminal;
        let mut environment = GraphicsEnvironment::detect(&SystemEnvironment);
        if environment.cell_pixels.value.is_none()
            && environment.interactive
            && std::io::stdin().is_terminal()
            && environment.graphics.value != Graphics::None
        {
            if let Some(cell) = query_cell_pixels(Duration::from_millis(100)) {
                environment.cell_pixels =
                    Field::new(Some(cell), Origin::Inferred, "CSI 16 t reply");
            }
        }
        environment
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::MapEnvironment;

    #[test]
    fn parses_cell_sizes() {
        assert_eq!(CellPixels::parse("8x16"), CellPixels::new(8, 16));
        assert_eq!(CellPixels::parse(" 10 X 20 "), CellPixels::new(10, 20));
        assert_eq!(CellPixels::parse("0x16"), None);
        assert_eq!(CellPixels::parse("8"), None);
        assert_eq!(
            CellPixels::from_window((800, 600), (100, 30)),
            CellPixels::new(8, 20)
        );
        assert_eq!(CellPixels::from_window((0, 0), (100, 30)), None);
    }

    #[test]
    fn parses_the_query_reply() {
        assert_eq!(
            parse_cell_pixels_reply(b"junk\x1b[6;18;9t"),
            CellPixels::new(9, 18)
        );
        assert_eq!(parse_cell_pixels_reply(b"\x1b[6;18"), None);
        assert_eq!(parse_cell_pixels_reply(b"\x1b[4;600;800t"), None);
    }

    #[test]
    fn classifies_what_the_query_reads() {
        assert_eq!(classify(b"\x1b"), Reply::Partial);
        assert_eq!(classify(b"\x1b["), Reply::Partial);
        assert_eq!(classify(b"\x1b[6;18"), Reply::Partial);
        assert_eq!(classify(b"\x1b[?62;2"), Reply::Partial);
        assert_eq!(
            classify(b"\x1b[6;18;9t"),
            Reply::CellSize(CellPixels::new(9, 18))
        );
        assert_eq!(classify(b"\x1b[4;600;800t"), Reply::CellSize(None));
        assert_eq!(classify(b"\x1b[?62;22c"), Reply::Attributes);
        // Keys the user pressed: not replies, so dropped.
        assert_eq!(classify(b"\x1b[A"), Reply::Other);
        assert_eq!(classify(b"\x1bOP"), Reply::Other);
        assert_eq!(classify(b"\x1b\x1b"), Reply::Other);
        assert_eq!(classify(b"\x1b[1;5D"), Reply::Other);
    }

    #[test]
    fn cell_size_comes_from_the_window_on_a_terminal_only() {
        let tty = MapEnvironment::tty()
            .var("TERM", "xterm-kitty")
            .cell_pixels(9, 18);
        let environment = GraphicsEnvironment::detect(&tty);
        assert_eq!(environment.graphics.value, Graphics::Kitty);
        assert_eq!(environment.cell_pixels.value, CellPixels::new(9, 18));
        let pipe = MapEnvironment::new().cell_pixels(9, 18);
        assert_eq!(GraphicsEnvironment::detect(&pipe).cell_pixels.value, None);
        let forced = MapEnvironment::new().var("RICH_CELL_PIXELS", "10x20");
        let environment = GraphicsEnvironment::detect(&forced);
        assert_eq!(environment.cell_pixels.value, CellPixels::new(10, 20));
        assert_eq!(
            environment.cell_pixels.origin,
            Origin::Environment("RICH_CELL_PIXELS".into())
        );
    }
}
