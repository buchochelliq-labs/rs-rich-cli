//! WebSocket frames from a browser, read as rs-rich-web's server reads
//! them: tungstenite on the server side, with the server's message and
//! frame limits, and each text or binary message's input (`d…`) given to
//! the input decoder.
//!
//! The server's own message parser (`parse_message`) and request-head
//! reader are crate-private, so they are reached only through the decoder.
#![no_main]

use std::io::{self, Cursor, Read, Write};

use libfuzzer_sys::fuzz_target;
use rich_fuzz::MAX_PARSE_INPUT;
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::{Message, WebSocket};

/// rs-rich-web's `MAX_MESSAGE`: the largest message and frame it accepts.
const MAX_MESSAGE: usize = 1 << 20;

/// The fuzzed bytes to read; writes (pongs, close replies) are dropped.
struct Stream(Cursor<Vec<u8>>);

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fuzz_target!(|data: &[u8]| {
    let bytes = data[..data.len().min(MAX_PARSE_INPUT)].to_vec();
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let mut socket =
        WebSocket::from_raw_socket(Stream(Cursor::new(bytes)), Role::Server, Some(config));
    // Each read takes at least one byte or fails, so this ends.
    while let Ok(message) = socket.read() {
        let text = match &message {
            Message::Text(text) => text.as_str(),
            Message::Binary(bytes) => std::str::from_utf8(bytes).unwrap_or(""),
            Message::Close(_) => return,
            _ => continue,
        };
        if let Some(input) = text.strip_prefix('d') {
            let _ = rich_web::input::decode(input);
        }
    }
});
