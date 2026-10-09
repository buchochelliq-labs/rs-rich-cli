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
//! On Unix the session reads the terminal itself and has a termwiz
//! [`InputParser`] read the bytes, rather than termwiz's `poll_input`, which
//! reads a sequence a read cut short (`ESC [ 1 ; 5`, then `C`) as typed
//! characters: here, as with the termion backend, the parser waits up to
//! 100 ms for the rest. Resizes come from SIGWINCH through a pipe. The
//! terminal itself lives behind a lock, which the signal thread takes to
//! give the terminal back for a suspend; reading never holds it.

use std::io;
use std::sync::Mutex;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use rich::ColorSystem;
use termwiz::caps::{Capabilities, ColorLevel, ProbeHints};
use termwiz::input::InputEvent;
#[cfg(unix)]
use termwiz::input::InputParser;
use termwiz::terminal::{SystemTerminal, Terminal};

use super::Output;

/// termwiz's terminal, kept where the panic hook and the signal thread
/// reach it.
static TERMINAL: Mutex<Option<SystemTerminal>> = Mutex::new(None);

/// What termwiz's probe found, and the terminal's input.
pub(super) struct Probed {
    pub(super) mouse: bool,
    pub(super) paste: bool,
    pub(super) color: Option<ColorSystem>,
    /// Boxed: termwiz's parser is large, and this sits in the session.
    #[cfg(unix)]
    reader: Box<Reader>,
}

/// How long the parser waits for the rest of a sequence a read cut short,
/// as termion's reader does.
#[cfg(unix)]
const SEQUENCE_WAIT: Duration = Duration::from_millis(100);

/// Reads the terminal, and parses what it read with termwiz's parser.
#[cfg(unix)]
struct Reader {
    tty: std::fs::File,
    parser: InputParser,
    /// A CSI sequence a read cut short, not yet given to the parser: it
    /// reads one it knows no key for (a mouse report) as typed characters
    /// at once, even told more may follow.
    held: Vec<u8>,
    events: std::collections::VecDeque<InputEvent>,
    /// When the last read was, while a sequence may be cut short (held, or
    /// in the parser): once [`SEQUENCE_WAIT`] passes with no more, it is
    /// read as it stands.
    last_read: Option<Instant>,
    /// Written to on SIGWINCH.
    resized: std::os::unix::net::UnixStream,
    resize_signal: signal_hook::SigId,
}

#[cfg(unix)]
impl Reader {
    fn open(tty: std::fs::File) -> io::Result<Reader> {
        let (resized, notify) = std::os::unix::net::UnixStream::pair()?;
        resized.set_nonblocking(true)?;
        notify.set_nonblocking(true)?;
        let resize_signal =
            signal_hook::low_level::pipe::register(signal_hook::consts::SIGWINCH, notify)?;
        Ok(Reader {
            tty,
            parser: InputParser::new(),
            held: Vec::new(),
            events: Default::default(),
            last_read: None,
            resized,
            resize_signal,
        })
    }

    /// Parse `bytes`, more of which may follow: a CSI sequence they end
    /// in the middle of is held until its rest comes.
    fn parse(&mut self, bytes: &[u8]) {
        self.held.extend_from_slice(bytes);
        let cut = match self.held.iter().rposition(|&b| b == 0x1b) {
            Some(at)
                if self.held.get(at + 1) == Some(&b'[')
                    && self.held[at + 2..]
                        .iter()
                        .all(|b| (0x20..=0x3f).contains(b)) =>
            {
                at
            }
            _ => self.held.len(),
        };
        let events = &mut self.events;
        self.parser
            .parse(&self.held[..cut], |event| events.push_back(event), true);
        self.held.drain(..cut);
        self.last_read = Some(Instant::now());
    }

    /// The next event within `wait` (or a little sooner).
    fn next(&mut self, wait: Duration) -> io::Result<Option<InputEvent>> {
        use filedescriptor::{poll, pollfd, POLLIN};
        use std::io::Read;
        use std::os::unix::io::AsRawFd;
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let wait = match self.last_read {
            Some(at) => wait.min((at + SEQUENCE_WAIT).saturating_duration_since(Instant::now())),
            None => wait,
        };
        let mut ready = [
            pollfd {
                fd: self.tty.as_raw_fd(),
                events: POLLIN,
                revents: 0,
            },
            pollfd {
                fd: self.resized.as_raw_fd(),
                events: POLLIN,
                revents: 0,
            },
        ];
        match poll(&mut ready, Some(wait)) {
            Ok(_) => {}
            // A signal (SIGWINCH, SIGCONT): look again next time.
            Err(filedescriptor::Error::Poll(e)) if e.kind() == io::ErrorKind::Interrupted => {
                return Ok(None)
            }
            Err(e) => return Err(other(e)),
        }
        if ready[1].revents != 0 {
            let mut drained = [0u8; 64];
            while matches!(self.resized.read(&mut drained), Ok(n) if n > 0) {}
            let (cols, rows) = size()?;
            return Ok(Some(InputEvent::Resized {
                cols: cols.into(),
                rows: rows.into(),
            }));
        }
        if ready[0].revents != 0 {
            let mut buffer = [0u8; 1024];
            match self.tty.read(&mut buffer) {
                // Nothing from a terminal that said it had input: it hung up.
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the terminal hung up",
                    ))
                }
                Ok(read) => self.parse(&buffer[..read]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        } else if self
            .last_read
            .is_some_and(|at| at.elapsed() >= SEQUENCE_WAIT)
        {
            // No more came: what is held is read as it stands.
            let events = &mut self.events;
            let held = std::mem::take(&mut self.held);
            self.parser
                .parse(&held, |event| events.push_back(event), false);
            self.last_read = None;
        }
        Ok(self.events.pop_front())
    }
}

#[cfg(unix)]
impl Drop for Reader {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.resize_signal);
    }
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
        reader: Box::new(Reader::open(tty)?),
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
pub(super) fn poll(probed: &mut Probed, wait: Duration) -> io::Result<Option<InputEvent>> {
    #[cfg(unix)]
    {
        probed.reader.next(wait)
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
