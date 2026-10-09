//! One program's session: a [`PtyHost`] fed by a WebSocket, on the
//! connection's own thread.
//!
//! The page's input goes to the program as it came (xterm.js already
//! speaks the program's language), resizes resize the PTY, and the
//! program's output goes to the page as binary messages. The page counts
//! what it has drawn (`a` and a number of bytes): with more than a window's
//! worth sent and not yet drawn, nothing more is read from the host, and a
//! [`LocalPty`](rich_embed::LocalPty) with backpressure then stops reading
//! the program, whose writes wait. Input is held back the same way: while
//! the host refuses it (a [`LocalPty`](rich_embed::LocalPty) holds at most
//! 1 MiB that its program has not read), nothing more is read from the
//! page, whose sends then wait in the network. The program's exit ends
//! the session with a close whose reason the page shows; when the session
//! ends first, the program is ended, and the session waits until it has.

use std::io::{self, ErrorKind};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rich_embed::PtyHost;
use tungstenite::{Message, WebSocket};

use crate::session::{close, deliver, parse_message, PageMessage};

/// How long one turn of the loop waits for the page: the longest a
/// program's output waits to be sent.
const TURN: Duration = Duration::from_millis(10);
/// The most output sent in one message.
const CHUNK: usize = 32 * 1024;
/// How long a session waits for its program to end once it has ended it
/// (a [`LocalPty`](rich_embed::LocalPty) kills one still running after a
/// second).
const END_WAIT: Duration = Duration::from_secs(3);

/// `max_buffered` split into the window of output sent and not yet drawn,
/// and the output a host may hold unread.
pub(crate) fn split(max_buffered: usize) -> (usize, usize) {
    let window = (max_buffered / 2).max(1);
    (window, max_buffered.saturating_sub(window).max(1))
}

/// Counts a program as running until dropped.
struct Running<'a>(&'a AtomicUsize);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Run the program on `host` at `columns` x `rows` over `socket` until it
/// exits, the page goes, or `stopping` is set; then end it, and wait until
/// it has ended. `running` counts it meanwhile.
pub(crate) fn run(
    socket: &mut WebSocket<TcpStream>,
    mut host: Box<dyn PtyHost>,
    columns: u16,
    rows: u16,
    stopping: &AtomicBool,
    running: &AtomicUsize,
    window: usize,
) -> io::Result<()> {
    // Counted before `stopping` is read, as `Handle::stop` sets `stopping`
    // before it waits for the count: a program either never starts or is
    // waited for.
    running.fetch_add(1, Ordering::SeqCst);
    let counted = Running(running);
    let result = if stopping.load(Ordering::SeqCst) {
        Ok(Some("The server stopped.".into()))
    } else {
        match host.start(columns, rows) {
            Ok(()) => {
                let result = serve(socket, &mut *host, stopping, window);
                end(&mut *host);
                result
            }
            Err(error) => Ok(Some(format!("The program could not start: {error}."))),
        }
    };
    let _ = host.kill();
    drop(host);
    drop(counted);
    match result {
        Ok(reason) => {
            close(socket, reason.as_deref());
            Ok(())
        }
        Err(error) => {
            close(socket, None);
            Err(error)
        }
    }
}

/// End the program, and wait (at most [`END_WAIT`]) until it has ended.
/// What it writes meanwhile is dropped.
fn end(host: &mut dyn PtyHost) {
    let _ = host.kill();
    let deadline = Instant::now() + END_WAIT;
    while host.exit_status().is_none() && Instant::now() < deadline {
        host.read();
        thread::sleep(Duration::from_millis(10));
    }
}

/// The loop; the reason to give the page when the session ends, `None`
/// when the page went first.
fn serve(
    socket: &mut WebSocket<TcpStream>,
    host: &mut dyn PtyHost,
    stopping: &AtomicBool,
    window: usize,
) -> io::Result<Option<String>> {
    // Bytes sent that the page has not said it drew.
    let mut unshown: u64 = 0;
    // Input the host refused, its program's input being full: until the
    // host takes it, the page is not read.
    let mut waiting: Option<Vec<u8>> = None;
    let window = window as u64;
    socket.get_mut().set_read_timeout(Some(TURN))?;
    loop {
        if stopping.load(Ordering::SeqCst) {
            return Ok(Some("The server stopped.".into()));
        }
        if unshown < window {
            let out = host.read();
            for chunk in out.chunks(CHUNK) {
                deliver(socket, Message::binary(chunk.to_vec()))?;
                unshown += chunk.len() as u64;
            }
        }
        // Only once its output has all been read.
        if let Some(status) = host.exit_status() {
            return Ok(Some(format!("The program {status}.")));
        }
        if let Some(input) = waiting.take() {
            if refused(host.write(&input)) {
                waiting = Some(input);
                if page_gone(socket) {
                    return Ok(None);
                }
                continue;
            }
        }
        let message = match socket.read() {
            Ok(message) => message,
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(None);
            }
            Err(tungstenite::Error::Io(e)) => return Err(e),
            Err(e) => return Err(io::Error::other(e)),
        };
        let text = match &message {
            Message::Text(text) => text.as_str(),
            Message::Close(_) => return Ok(None),
            _ => continue,
        };
        // Input as it came: the program reads what a terminal sends. A
        // program that has gone reports its exit on the next turn.
        if let Some(input) = text.strip_prefix('d') {
            if refused(host.write(input.as_bytes())) {
                waiting = Some(input.as_bytes().to_vec());
            }
            continue;
        }
        match parse_message(text) {
            PageMessage::Resize(columns, rows) => {
                let _ = host.resize(columns, rows);
            }
            PageMessage::Shown(bytes) => unshown = unshown.saturating_sub(bytes),
            _ => {}
        }
    }
}

/// Whether a write was refused for now, to be tried again.
fn refused(result: io::Result<()>) -> bool {
    matches!(result, Err(error) if error.kind() == ErrorKind::WouldBlock)
}

/// Whether the page has closed the connection, looking for at most one turn
/// without reading what it sent.
fn page_gone(socket: &mut WebSocket<TcpStream>) -> bool {
    match socket.get_ref().peek(&mut [0u8; 1]) {
        Ok(0) => true,
        Ok(_) => {
            // More is waiting: wait a turn.
            thread::sleep(TURN);
            false
        }
        Err(error) => !matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_is_split_in_two() {
        assert_eq!(split(1 << 20), (1 << 19, 1 << 19));
        assert_eq!(split(5), (2, 3));
        assert_eq!(split(0), (1, 1));
    }
}
