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
/// Windows, or when the terminal does not answer in time.
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
    use rustix::event::{poll, PollFd, PollFlags, Timespec};
    use rustix::fs::{open, Mode, OFlags};
    use rustix::termios::{tcgetattr, tcsetattr, OptionalActions};
    use std::time::Instant;

    let tty = open(
        "/dev/tty",
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .ok()?;
    let saved = tcgetattr(&tty).ok()?;
    let mut raw = saved.clone();
    raw.make_raw();
    tcsetattr(&tty, OptionalActions::Now, &raw).ok()?;
    let result = (|| {
        rustix::io::write(&tty, b"\x1b[16t").ok()?;
        let deadline = Instant::now() + timeout;
        let mut reply = Vec::new();
        let mut buffer = [0u8; 64];
        while reply.len() < 256 {
            let left = deadline.checked_duration_since(Instant::now())?;
            let wait = Timespec {
                tv_sec: left.as_secs() as _,
                tv_nsec: left.subsec_nanos() as _,
            };
            let mut fds = [PollFd::new(&tty, PollFlags::IN)];
            if poll(&mut fds, Some(&wait)).ok()? == 0 {
                return None;
            }
            let read = rustix::io::read(&tty, &mut buffer).ok()?;
            if read == 0 {
                return None;
            }
            reply.extend_from_slice(&buffer[..read]);
            if let Some(cell) = parse_cell_pixels_reply(&reply) {
                return Some(cell);
            }
        }
        None
    })();
    let _ = tcsetattr(&tty, OptionalActions::Now, &saved);
    result
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
    /// waiting at most 100 ms).
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
