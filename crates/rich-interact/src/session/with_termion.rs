//! The termion backend (#677): raw mode, the terminal's size, and its keys
//! and mouse read with termion, on Unix.
//!
//! termion reads input with a blocking iterator; a session waits with a
//! timeout. So the reader here waits on the terminal itself, reads what is
//! there, and cuts it into one event's bytes each for termion's parser
//! ([`parse_event`](termion::event::parse_event)), which wants an event's
//! bytes complete. Resizes come from SIGWINCH through a pipe, and pastes
//! (whose markers termion does not know) are cut out before the parser.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::time::Duration;

use termion::raw::{IntoRawMode, RawTerminal};

use crate::event::{from_termion, Event, HeldButton};

/// termion's raw mode on the terminal, kept where the panic hook and the
/// signal thread reach it.
static RAW: Mutex<Option<RawTerminal<File>>> = Mutex::new(None);

/// Turn raw mode on (the first time, termion records the mode to go back
/// to) or back off.
pub(super) fn raw_mode(on: bool, wait: bool) -> io::Result<()> {
    let mut raw = super::lock(&RAW, wait)?;
    match (raw.as_ref(), on) {
        (Some(terminal), true) => terminal.activate_raw_mode(),
        (Some(terminal), false) => terminal.suspend_raw_mode(),
        (None, true) => {
            *raw = Some(termion::get_tty()?.into_raw_mode()?);
            Ok(())
        }
        (None, false) => Ok(()),
    }
}

/// The session is over: drop termion's raw terminal, which puts the mode it
/// recorded back once more.
pub(super) fn release() {
    let terminal = super::lock(&RAW, true).ok().and_then(|mut raw| raw.take());
    drop(terminal);
}

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// The first event's bytes in `bytes`.
#[derive(Debug, PartialEq, Eq)]
enum Cut {
    /// The first `n` bytes are one event, for termion's parser.
    Event(usize),
    /// Pasted text, and the bytes it took with its markers.
    Paste(String, usize),
    /// The first `n` bytes are not an event termion could read.
    Skip(usize),
    /// An event's bytes, cut short: the rest may still come.
    Short,
}

/// How long a UTF-8 character that starts with `lead` is.
fn utf8_len(lead: u8) -> usize {
    match lead {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

/// Cut the first event's bytes from `bytes` (not empty), as termion's own
/// reader does: Esc alone at the end of what was read is the Esc key, an
/// escape sequence runs to its final byte, and anything else is one
/// character.
fn cut(bytes: &[u8]) -> Cut {
    let whole = |n: usize| {
        if bytes.len() >= n {
            Cut::Event(n)
        } else {
            Cut::Short
        }
    };
    if bytes[0] != 0x1b {
        return whole(utf8_len(bytes[0]));
    }
    match bytes.get(1) {
        None => Cut::Event(1),
        // SS3: F1 to F4.
        Some(b'O') => whole(3),
        Some(b'[') => match bytes.get(2) {
            None => Cut::Short,
            // X10 mouse: three bytes after it, whatever they are.
            Some(b'M') => whole(6),
            // The Linux console's F1 to F5.
            Some(b'[') => whole(4),
            Some(_) => {
                let mut end = 2;
                while end < bytes.len() && (0x20..=0x3f).contains(&bytes[end]) {
                    end += 1;
                }
                match bytes.get(end) {
                    None => Cut::Short,
                    Some(0x40..=0x7e) => {
                        let sequence = &bytes[..=end];
                        if sequence != PASTE_START {
                            return Cut::Event(end + 1);
                        }
                        let text = &bytes[sequence.len()..];
                        match text.windows(PASTE_END.len()).position(|w| w == PASTE_END) {
                            Some(at) => Cut::Paste(
                                String::from_utf8_lossy(&text[..at]).into_owned(),
                                sequence.len() + at + PASTE_END.len(),
                            ),
                            None => Cut::Short,
                        }
                    }
                    // Not a sequence: leave what follows to be read.
                    Some(_) => Cut::Skip(end),
                }
            }
        },
        // Alt with a character.
        Some(&next) => whole(1 + utf8_len(next)),
    }
}

/// termion's event for one event's bytes; `None` for what it cannot read.
fn parse(bytes: &[u8]) -> Option<termion::event::Event> {
    if bytes == b"\x1b" {
        return Some(termion::event::Event::Key(termion::event::Key::Esc));
    }
    // termion's parser unwraps what it expects, so bytes it does not (a
    // number too large) panic: caught, and read as nothing.
    super::catch_panic(|| {
        let mut rest = bytes[1..].iter().map(|&b| Ok(b));
        termion::event::parse_event(bytes[0], &mut rest).ok()
    })
    .ok()
    .flatten()
}

/// Reads the terminal's events with termion.
pub(super) struct Reader {
    tty: File,
    /// Read but not yet an event.
    pending: Vec<u8>,
    events: VecDeque<Event>,
    held: HeldButton,
    /// Written to on SIGWINCH.
    resized: UnixStream,
    resize_signal: signal_hook::SigId,
}

impl Reader {
    pub(super) fn open() -> io::Result<Reader> {
        let tty = termion::get_tty()?;
        let (resized, notify) = UnixStream::pair()?;
        resized.set_nonblocking(true)?;
        notify.set_nonblocking(true)?;
        let resize_signal =
            signal_hook::low_level::pipe::register(signal_hook::consts::SIGWINCH, notify)?;
        Ok(Reader {
            tty,
            pending: Vec::new(),
            events: VecDeque::new(),
            held: HeldButton::default(),
            resized,
            resize_signal,
        })
    }

    /// The terminal's size, as termion measures it.
    pub(super) fn size(&self) -> io::Result<(u16, u16)> {
        termion::terminal_size_fd(&self.tty)
    }

    /// Wait up to `wait` for input or a resize: (input, resized).
    fn wait(&self, wait: Duration) -> io::Result<(bool, bool)> {
        use filedescriptor::{poll, pollfd, POLLIN};
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
            Ok(_) => Ok((ready[0].revents != 0, ready[1].revents != 0)),
            // A signal (SIGWINCH, SIGCONT): look again next time.
            Err(filedescriptor::Error::Poll(e)) if e.kind() == io::ErrorKind::Interrupted => {
                Ok((false, false))
            }
            Err(e) => Err(io::Error::other(e.to_string())),
        }
    }

    /// Read what the terminal has.
    fn fill(&mut self) -> io::Result<()> {
        let mut buffer = [0u8; 1024];
        match self.tty.read(&mut buffer) {
            Ok(read) => {
                self.pending.extend_from_slice(&buffer[..read]);
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Turn the pending bytes into events. `last`: no more is coming, so
    /// an event cut short is read as far as it goes.
    fn drain(&mut self, last: bool) {
        while !self.pending.is_empty() {
            let used = match cut(&self.pending) {
                Cut::Event(n) => {
                    if let Some(event) = parse(&self.pending[..n]) {
                        if let Some(event) = from_termion(event, &mut self.held) {
                            self.events.push_back(event);
                        }
                    }
                    n
                }
                Cut::Paste(text, n) => {
                    self.events.push_back(Event::Paste(text));
                    n
                }
                Cut::Skip(n) => n,
                Cut::Short if !last => return,
                // A paste whose end never came is still what was pasted;
                // anything else cut short is dropped.
                Cut::Short => {
                    if self.pending.starts_with(PASTE_START) {
                        let text = &self.pending[PASTE_START.len()..];
                        let text = String::from_utf8_lossy(text).into_owned();
                        self.events.push_back(Event::Paste(text));
                    }
                    self.pending.len()
                }
            };
            self.pending.drain(..used);
        }
    }

    /// The next event, waiting up to `wait` for one.
    pub(super) fn next(&mut self, wait: Duration) -> io::Result<Option<Event>> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let (input, resized) = self.wait(wait)?;
        if resized {
            let mut drained = [0u8; 64];
            while matches!(self.resized.read(&mut drained), Ok(n) if n > 0) {}
            let (columns, rows) = self.size()?;
            return Ok(Some(Event::Resize { columns, rows }));
        }
        if input {
            self.fill()?;
            self.drain(false);
            // An escape sequence cut short: wait for its rest as long as
            // termion's own reader would.
            let timeout = Duration::from_millis(termion::raw::CONTROL_SEQUENCE_TIMEOUT);
            while !self.pending.is_empty() && self.wait(timeout)?.0 {
                self.fill()?;
                self.drain(false);
            }
            self.drain(true);
        }
        Ok(self.events.pop_front())
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.resize_signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Key, KeyCode};

    #[test]
    fn input_is_cut_into_events() {
        assert_eq!(cut(b"ab"), Cut::Event(1));
        assert_eq!(cut("é!".as_bytes()), Cut::Event(2));
        assert_eq!(cut(&"é".as_bytes()[..1]), Cut::Short);
        // Esc alone at the end of a read is the key.
        assert_eq!(cut(b"\x1b"), Cut::Event(1));
        assert_eq!(cut(b"\x1bx!"), Cut::Event(2));
        assert_eq!(cut(b"\x1b["), Cut::Short);
        assert_eq!(cut(b"\x1b[1;5"), Cut::Short);
        assert_eq!(cut(b"\x1b[1;5Cx"), Cut::Event(6));
        assert_eq!(cut(b"\x1bOPx"), Cut::Event(3));
        assert_eq!(cut(b"\x1b[<0;10;5Mx"), Cut::Event(10));
        assert_eq!(cut(b"\x1b[M !!x"), Cut::Event(6));
        assert_eq!(cut(b"\x1b[\x01"), Cut::Skip(2));
        assert_eq!(
            cut(b"\x1b[200~a\x1b[b\x1b[201~x"),
            Cut::Paste("a\x1b[b".into(), 16)
        );
        assert_eq!(cut(b"\x1b[200~ab"), Cut::Short);
    }

    #[test]
    fn what_termion_cannot_read_is_nothing() {
        assert_eq!(
            parse(b"\x1b"),
            Some(termion::event::Event::Key(termion::event::Key::Esc))
        );
        // termion parses `~` sequences' numbers as bytes, and unwraps.
        assert_eq!(parse(b"\x1b[300~"), None);
        assert!(matches!(
            parse(b"\x1b[57399u"),
            Some(termion::event::Event::Unsupported(_)) | None
        ));
    }

    #[test]
    fn pending_bytes_drain_into_events() {
        let (resized, notify) = UnixStream::pair().unwrap();
        let mut reader = Reader {
            tty: tempfile_tty(),
            pending: b"a\x1b[Ab\x1b[200~hi\x1b[201~\x1b".to_vec(),
            events: VecDeque::new(),
            held: HeldButton::default(),
            resized,
            resize_signal: signal_hook::low_level::pipe::register(
                signal_hook::consts::SIGWINCH,
                notify,
            )
            .unwrap(),
        };
        reader.drain(true);
        let events: Vec<Event> = reader.events.drain(..).collect();
        assert_eq!(
            events,
            [
                Event::Key(Key::char('a')),
                Event::Key(Key::new(KeyCode::Up)),
                Event::Key(Key::char('b')),
                Event::Paste("hi".into()),
                Event::Key(Key::new(KeyCode::Escape)),
            ]
        );
        // Cut short, and more may come: kept.
        reader.pending = b"\x1b[1;".to_vec();
        reader.drain(false);
        assert_eq!(reader.pending, b"\x1b[1;");
        reader.drain(true);
        assert!(reader.pending.is_empty() && reader.events.is_empty());
    }

    /// Any file stands in for the terminal where nothing reads it.
    fn tempfile_tty() -> File {
        File::open("/dev/null").unwrap()
    }
}
