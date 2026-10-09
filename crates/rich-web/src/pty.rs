//! One program's session: a [`PtyHost`] fed by a WebSocket, on the
//! connection's own thread.
//!
//! The page's input goes to the program as it came (xterm.js already
//! speaks the program's language), resizes resize the PTY, and the
//! program's output goes to the page as binary messages. The page counts
//! what it has drawn (`a` and a number of bytes): with more than a window's
//! worth sent and not yet drawn, nothing more is read from the host, and a
//! [`LocalPty`](rich_embed::LocalPty) with backpressure then stops reading
//! the program, whose writes wait. The program's exit ends the session
//! with a close whose reason the page shows.

use std::io::{self, ErrorKind};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rich_embed::PtyHost;
use tungstenite::{Message, WebSocket};

use crate::session::{close, deliver, parse_message, PageMessage};

/// How long one turn of the loop waits for the page: the longest a
/// program's output waits to be sent.
const TURN: Duration = Duration::from_millis(10);
/// The most output sent in one message.
const CHUNK: usize = 32 * 1024;

/// `max_buffered` split into the window of output sent and not yet drawn,
/// and the output a host may hold unread.
pub(crate) fn split(max_buffered: usize) -> (usize, usize) {
    let window = (max_buffered / 2).max(1);
    (window, max_buffered.saturating_sub(window).max(1))
}

/// Run the program on `host` at `columns` x `rows` over `socket` until it
/// exits, the page goes, or `stopping` is set; then end it.
pub(crate) fn run(
    socket: &mut WebSocket<TcpStream>,
    mut host: Box<dyn PtyHost>,
    columns: u16,
    rows: u16,
    stopping: &AtomicBool,
    window: usize,
) -> io::Result<()> {
    let result = match host.start(columns, rows) {
        Ok(()) => serve(socket, &mut *host, stopping, window),
        Err(error) => Ok(Some(format!("The program could not start: {error}."))),
    };
    let _ = host.kill();
    drop(host);
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
            let _ = host.write(input.as_bytes());
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
