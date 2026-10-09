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
/// character. `searched`: how many of the first bytes were already
/// searched for a paste's end without finding it, so a long paste that
/// arrives over many reads is searched once, not again on every read.
fn cut(bytes: &[u8], searched: usize) -> Cut {
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
        // A lone escape may be the start of a sequence split across reads
        // (Alt+x as `ESC`, then `x`): kept until the rest comes or the
        // control-sequence wait runs out.
        None => Cut::Short,
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
                        // The end may have begun in the last bytes searched.
                        let from = searched
                            .saturating_sub(sequence.len() + PASTE_END.len() - 1)
                            .min(text.len());
                        match text[from..]
                            .windows(PASTE_END.len())
                            .position(|w| w == PASTE_END)
                        {
                            Some(at) => Cut::Paste(
                                String::from_utf8_lossy(&text[..from + at]).into_owned(),
                                sequence.len() + from + at + PASTE_END.len(),
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
    /// How much of `pending` was searched for a paste's end: see [`cut`].
    searched: usize,
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
            searched: 0,
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

    /// Read what the terminal has. Nothing, once the terminal says it has
    /// input, means it hung up: the end of input, an error, rather than a
    /// wait that returns at once forever.
    fn fill(&mut self) -> io::Result<()> {
        let mut buffer = [0u8; 1024];
        match self.tty.read(&mut buffer) {
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the terminal hung up",
            )),
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
            let cut = match cut(&self.pending, self.searched) {
                // Nothing came after it: the Escape key.
                Cut::Short if last && self.pending == [0x1b] => Cut::Event(1),
                cut => cut,
            };
            let used = match cut {
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
                Cut::Short if !last => {
                    self.searched = self.pending.len();
                    return;
                }
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
            self.searched = 0;
        }
    }

    /// Bytes read from the terminal by someone else (keys that came with an
    /// answer the session asked for), as if read here.
    pub(super) fn unread(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        self.drain(false);
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
            // termion's own reader would after an ESC alone, and longer
            // after more of a sequence (which no one types), so a busy
            // machine's late rest is not typed out.
            let escape = Duration::from_millis(termion::raw::CONTROL_SEQUENCE_TIMEOUT);
            loop {
                let timeout = if self.pending.len() > 1 {
                    escape * 5
                } else {
                    escape
                };
                if self.pending.is_empty() || !self.wait(timeout)?.0 {
                    break;
                }
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
    use crate::event::{Key, KeyCode, Modifiers};

    #[test]
    fn input_is_cut_into_events() {
        assert_eq!(cut(b"ab", 0), Cut::Event(1));
        assert_eq!(cut("é!".as_bytes(), 0), Cut::Event(2));
        assert_eq!(cut(&"é".as_bytes()[..1], 0), Cut::Short);
        // Esc alone at the end of a read is the key.
        assert_eq!(cut(b"\x1b", 0), Cut::Short);
        assert_eq!(cut(b"\x1bx!", 0), Cut::Event(2));
        assert_eq!(cut(b"\x1b[", 0), Cut::Short);
        assert_eq!(cut(b"\x1b[1;5", 0), Cut::Short);
        assert_eq!(cut(b"\x1b[1;5Cx", 0), Cut::Event(6));
        assert_eq!(cut(b"\x1bOPx", 0), Cut::Event(3));
        assert_eq!(cut(b"\x1b[<0;10;5Mx", 0), Cut::Event(10));
        assert_eq!(cut(b"\x1b[M !!x", 0), Cut::Event(6));
        assert_eq!(cut(b"\x1b[\x01", 0), Cut::Skip(2));
        assert_eq!(
            cut(b"\x1b[200~a\x1b[b\x1b[201~x", 0),
            Cut::Paste("a\x1b[b".into(), 16)
        );
        assert_eq!(cut(b"\x1b[200~ab", 0), Cut::Short);
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
            searched: 0,
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
        // Alt+x split across reads: the escape waits for its `x`.
        reader.pending = b"\x1b".to_vec();
        reader.drain(false);
        assert_eq!(reader.pending, b"\x1b");
        assert!(reader.events.is_empty());
        reader.pending.push(b'x');
        reader.drain(false);
        assert_eq!(
            reader.events.drain(..).collect::<Vec<_>>(),
            [Event::Key(Key::with(
                KeyCode::Char('x'),
                Modifiers {
                    alt: true,
                    ..Modifiers::default()
                }
            ))]
        );
        // A lone escape with nothing after it is the Escape key.
        reader.pending = b"\x1b".to_vec();
        reader.drain(true);
        assert_eq!(
            reader.events.drain(..).collect::<Vec<_>>(),
            [Event::Key(Key::new(KeyCode::Escape))]
        );
    }

    #[test]
    fn answers_to_the_start_up_query_are_no_keys() {
        // A DECRQM report, kitty flags and the device attributes, late or
        // among keys: each cut out whole, and read as nothing.
        assert_eq!(cut(b"\x1b[?2026;2$yx", 0), Cut::Event(11));
        let (resized, notify) = UnixStream::pair().unwrap();
        let mut reader = Reader {
            tty: tempfile_tty(),
            pending: b"a\x1b[?2026;2$yb\x1b[?0u\x1b[?62;22cc\x1b[?2026;0$y".to_vec(),
            searched: 0,
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
                Event::Key(Key::char('b')),
                Event::Key(Key::char('c')),
            ]
        );
    }

    #[test]
    fn a_long_paste_is_searched_once() {
        // 8 MiB arriving 1 KiB a read: each read searches only what it
        // added for the end, so the whole takes linear time.
        let mut reader = reader(PASTE_START);
        let start = std::time::Instant::now();
        for _ in 0..8 * 1024 {
            reader.pending.extend_from_slice(&[b'x'; 1024]);
            reader.drain(false);
            assert!(start.elapsed() < Duration::from_secs(5), "too slow");
        }
        // The end, cut across two reads.
        reader.pending.extend_from_slice(&PASTE_END[..3]);
        reader.drain(false);
        reader.pending.extend_from_slice(&PASTE_END[3..]);
        reader.pending.push(b'a');
        reader.drain(false);
        let events: Vec<Event> = reader.events.drain(..).collect();
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], Event::Paste(text) if text.len() == 8 << 20 && !text.contains('\x1b'))
        );
        assert_eq!(events[1], Event::Key(Key::char('a')));
        assert!(reader.pending.is_empty() && reader.searched == 0);
    }

    #[test]
    fn a_terminal_that_hangs_up_ends_the_input() {
        // `/dev/null` reads as a terminal that hung up: always ready, never
        // anything. Even with a sequence cut short pending, the reader
        // ends rather than wait forever.
        for pending in [&b""[..], b"\x1b[1;"] {
            let mut reader = reader(pending);
            let (sent, done) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = sent.send(reader.next(Duration::from_millis(10)).map_err(|e| e.kind()));
            });
            assert_eq!(
                done.recv_timeout(Duration::from_secs(5)),
                Ok(Err(io::ErrorKind::UnexpectedEof)),
                "{pending:?}"
            );
        }
    }

    /// A reader on no terminal, with `pending` read.
    fn reader(pending: &[u8]) -> Reader {
        let (resized, notify) = UnixStream::pair().unwrap();
        Reader {
            tty: tempfile_tty(),
            pending: pending.to_vec(),
            searched: 0,
            events: VecDeque::new(),
            held: HeldButton::default(),
            resized,
            resize_signal: signal_hook::low_level::pipe::register(
                signal_hook::consts::SIGWINCH,
                notify,
            )
            .unwrap(),
        }
    }

    /// Any file stands in for the terminal where nothing reads it.
    fn tempfile_tty() -> File {
        File::open("/dev/null").unwrap()
    }
}
