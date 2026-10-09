//! One app session: an intuiTUIve [`Driver`] fed by a WebSocket, on the
//! connection's own thread.
//!
//! The loop is the one [`App::run_on`](intuituive::App::run_on) runs, with a
//! socket in place of a terminal: bring the app up to date, send what it
//! drew, then wait for a message no longer than the app can wait (its next
//! timer, animation frame or toast). A read that times out keeps any part
//! of a frame it got, so waiting this way loses nothing.
//!
//! What is sent depends on the page: terminal output for xterm.js, or the
//! [DOM renderer's](crate::dom) messages.

use std::io::{self, ErrorKind};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use intuituive::interact::Event;
use intuituive::{App, Driver};
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::CloseFrame;
use tungstenite::{Message, WebSocket};

use crate::dom::Dom;
use crate::input::decode;
use crate::Renderer;

/// Sent before the first frame: the alternate screen (no scrollback, so a
/// resize never leaves the view scrolled), report the mouse (clicks, drags,
/// the wheel, as SGR), bracket pastes, hide the cursor.
const SETUP: &str = "\x1b[?1049h\x1b[H\x1b[2J\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b[?25l";
/// Sent after the last frame: the input modes off again. The page stays on
/// the alternate screen, so the last frame stays in view under the page's
/// "session ended" note.
const TEARDOWN: &str = "\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1006l\x1b[?2004l";

/// The largest terminal a browser may ask for, each way.
pub(crate) const MAX_CELLS: u16 = 1000;

/// How long a session that is ending waits for the page to answer its
/// close, reading (and dropping) what the page still sends, so the last
/// output is not cut off by a reset.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// What a message from the page asks for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PageMessage {
    /// Input: these events, in order.
    Input(Vec<Event>),
    /// The terminal is now this size.
    Resize(u16, u16),
    /// The page's clipboard took (`true`) or refused the copy it was sent
    /// this many copies ago, counting from 1.
    Copied(u64, bool),
    /// The page has drawn this many more bytes of a program's output.
    Shown(u64),
    /// Not a message the page sends.
    Unknown,
}

/// Read one message of the page's protocol: `d` and input bytes, `r` and
/// `columns,rows`, `c` and a copy's answer, or `a` and a count of bytes
/// drawn.
pub(crate) fn parse_message(text: &str) -> PageMessage {
    if let Some(input) = text.strip_prefix('d') {
        return PageMessage::Input(decode(input));
    }
    if let Some(size) = text.strip_prefix('r') {
        if let Some((columns, rows)) = parse_size(size) {
            return PageMessage::Resize(columns, rows);
        }
    }
    // `c` and the copy's number, `:1` when the clipboard took it, `:0`
    // when it did not.
    if let Some((number, ok)) = text.strip_prefix('c').and_then(|t| t.split_once(':')) {
        if let (Ok(number), "0" | "1") = (number.parse(), ok) {
            return PageMessage::Copied(number, ok == "1");
        }
    }
    if let Some(Ok(bytes)) = text.strip_prefix('a').map(str::parse) {
        return PageMessage::Shown(bytes);
    }
    PageMessage::Unknown
}

/// `columns,rows`, each from 1 to [`MAX_CELLS`].
pub(crate) fn parse_size(text: &str) -> Option<(u16, u16)> {
    let (columns, rows) = text.split_once(',')?;
    let columns: u16 = columns.trim().parse().ok()?;
    let rows: u16 = rows.trim().parse().ok()?;
    let ok = |n: u16| (1..=MAX_CELLS).contains(&n);
    (ok(columns) && ok(rows)).then_some((columns, rows))
}

/// Run `app` at `columns` x `rows` over `socket`, drawn by `renderer`,
/// until it quits, the page goes, or `stopping` is set.
pub(crate) fn run(
    socket: &mut WebSocket<TcpStream>,
    app: App,
    columns: u16,
    rows: u16,
    stopping: &AtomicBool,
    renderer: Renderer,
) -> io::Result<()> {
    let start = Instant::now();
    let mut driver = app.driver(columns, rows);
    // Copies go to the page (as OSC 52 for xterm.js), which puts them on
    // the clipboard; it says whether that worked, and only then does a
    // toast say so.
    driver.set_clipboard(true);
    let mut copies = Copies::default();
    let mut dom = (renderer == Renderer::Dom).then(Dom::new);
    match &dom {
        Some(dom) => send(socket, &dom.hello())?,
        None => send(socket, SETUP)?,
    }
    let result = (|| -> io::Result<()> {
        loop {
            if stopping.load(Ordering::SeqCst) {
                return Ok(());
            }
            driver.update(start.elapsed());
            tell(&mut driver, socket, &mut copies, dom.as_mut())?;
            if driver.is_done() {
                return Ok(());
            }
            if let Some(out) = driver.render() {
                match &mut dom {
                    Some(dom) => {
                        for message in dom.update(&driver) {
                            send(socket, &message)?;
                        }
                    }
                    None if !out.is_empty() => send(socket, &out)?,
                    None => {}
                }
            }
            tell(&mut driver, socket, &mut copies, dom.as_mut())?;
            if driver.is_done() {
                return Ok(());
            }
            // At least a millisecond: a zero read timeout means "forever".
            let wait = driver
                .timeout(start.elapsed())
                .max(Duration::from_millis(1));
            socket.get_mut().set_read_timeout(Some(wait))?;
            let message = match socket.read() {
                Ok(message) => message,
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    continue;
                }
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return Ok(());
                }
                Err(tungstenite::Error::Io(e)) => return Err(e),
                Err(e) => return Err(io::Error::other(e)),
            };
            let text = match &message {
                Message::Text(text) => text.as_str(),
                Message::Binary(bytes) => std::str::from_utf8(bytes).unwrap_or(""),
                // The page went: tungstenite answers the close.
                Message::Close(_) => return Ok(()),
                _ => continue,
            };
            match parse_message(text) {
                PageMessage::Input(events) => {
                    for event in events {
                        driver.event(event);
                    }
                }
                PageMessage::Resize(columns, rows) => driver.event(Event::Resize { columns, rows }),
                PageMessage::Copied(number, ok) => copies.answered(&mut driver, number, ok),
                PageMessage::Shown(_) | PageMessage::Unknown => {}
            }
            tell(&mut driver, socket, &mut copies, dom.as_mut())?;
        }
    })();
    // Leave the page's terminal as it was, then say goodbye. Errors here
    // only mean the page has gone already.
    if socket.can_write() && dom.is_none() {
        let mut last = driver.finish();
        last.push_str(TEARDOWN);
        let _ = send(socket, &last);
    }
    close(socket, None);
    result
}

/// Send what the app copied and, to a DOM page, what it announced.
fn tell(
    driver: &mut Driver,
    socket: &mut WebSocket<TcpStream>,
    copies: &mut Copies,
    dom: Option<&mut Dom>,
) -> io::Result<()> {
    let dom = dom.is_some();
    copies.send(driver, socket, dom)?;
    let said = driver.take_announcements();
    if dom {
        for announcement in said {
            send(socket, &crate::dom::say(&announcement))?;
        }
    }
    Ok(())
}

/// Send `text` as one message.
pub(crate) fn send(socket: &mut WebSocket<TcpStream>, text: &str) -> io::Result<()> {
    deliver(socket, Message::text(text))
}

/// Send `message`, as an [`io::Error`] when it fails.
pub(crate) fn deliver(socket: &mut WebSocket<TcpStream>, message: Message) -> io::Result<()> {
    socket.send(message).map_err(|e| match e {
        tungstenite::Error::Io(e) => e,
        e => io::Error::other(e),
    })
}

/// End the session: a close (with `reason`, which the page shows, cut to
/// the 123 bytes a close frame carries), then whatever the page still
/// sends is read until it answers, for at most [`CLOSE_WAIT`]. When the
/// page closed first, this sends tungstenite's queued answer.
pub(crate) fn close(socket: &mut WebSocket<TcpStream>, reason: Option<&str>) {
    if socket.can_write() {
        let frame = reason.map(|reason| {
            let mut end = reason.len().min(123);
            while !reason.is_char_boundary(end) {
                end -= 1;
            }
            CloseFrame {
                code: CloseCode::Normal,
                reason: reason[..end].to_string().into(),
            }
        });
        let _ = socket.close(frame);
    }
    let _ = socket.flush();
    let deadline = Instant::now() + CLOSE_WAIT;
    let _ = socket
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(50)));
    while Instant::now() < deadline {
        match socket.read() {
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
}

/// Copies sent to the page (OSC 52, or the DOM renderer's `copy`) that it
/// has not answered yet, by number, from 1.
#[derive(Default)]
struct Copies {
    sent: u64,
    waiting: std::collections::VecDeque<(u64, String)>,
}

/// At most this many copies wait for the page's answer; older ones are
/// forgotten (a page that never answers).
const COPIES_WAITING: usize = 16;

impl Copies {
    /// Send what the app copied to the page.
    fn send(
        &mut self,
        driver: &mut Driver,
        socket: &mut WebSocket<TcpStream>,
        dom: bool,
    ) -> io::Result<()> {
        for text in driver.take_copies() {
            self.sent += 1;
            let message = if dom {
                crate::dom::copy(self.sent, &text)
            } else {
                let encoded = data_encoding::BASE64.encode(text.as_bytes());
                format!("\x1b]52;c;{encoded}\x07")
            };
            send(socket, &message)?;
            self.waiting.push_back((self.sent, text));
            if self.waiting.len() > COPIES_WAITING {
                self.waiting.pop_front();
            }
        }
        Ok(())
    }

    /// The page answered copy `number`: tell the app if it is on the
    /// clipboard.
    fn answered(&mut self, driver: &mut Driver, number: u64, ok: bool) {
        if let Some(at) = self.waiting.iter().position(|(n, _)| *n == number) {
            let (_, text) = self.waiting.remove(at).expect("found");
            if ok {
                driver.copied(&text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use intuituive::interact::Key;

    #[test]
    fn a_copy_is_copied_only_once_the_page_says_so() {
        use intuituive::prelude::*;
        let mut driver = App::new(|| label("x")).driver(30, 4);
        let toast = |driver: &mut Driver| {
            driver.update(std::time::Duration::ZERO);
            let _ = driver.render();
            driver.screen().plain().join("\n").contains("Copied")
        };
        let mut copies = Copies::default();
        copies.waiting.push_back((1, "abc".into()));
        copies.waiting.push_back((2, "de".into()));
        // Refused: no toast; an answer to a copy not sent: nothing.
        copies.answered(&mut driver, 1, false);
        copies.answered(&mut driver, 9, true);
        assert!(!toast(&mut driver));
        copies.answered(&mut driver, 2, true);
        assert!(toast(&mut driver));
        assert!(copies.waiting.is_empty());
    }

    #[test]
    fn messages_parse() {
        assert_eq!(
            parse_message("dq\r"),
            PageMessage::Input(vec![
                Event::Key(Key::char('q')),
                Event::Key(Key::parse("enter").unwrap())
            ])
        );
        assert_eq!(parse_message("r120,40"), PageMessage::Resize(120, 40));
        assert_eq!(parse_message("r0,40"), PageMessage::Unknown);
        assert_eq!(parse_message("r5000,40"), PageMessage::Unknown);
        assert_eq!(parse_message("rx,y"), PageMessage::Unknown);
        assert_eq!(parse_message("zzz"), PageMessage::Unknown);
        assert_eq!(parse_message(""), PageMessage::Unknown);
        assert_eq!(parse_message("d"), PageMessage::Input(vec![]));
        assert_eq!(parse_message("c3:1"), PageMessage::Copied(3, true));
        assert_eq!(parse_message("c3:0"), PageMessage::Copied(3, false));
        assert_eq!(parse_message("c3:2"), PageMessage::Unknown);
        assert_eq!(parse_message("cx:1"), PageMessage::Unknown);
        assert_eq!(parse_message("a4096"), PageMessage::Shown(4096));
        assert_eq!(parse_message("a-1"), PageMessage::Unknown);
        assert_eq!(parse_message("a"), PageMessage::Unknown);
    }
}
