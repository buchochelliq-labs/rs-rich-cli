//! What the tests share: raw requests, a WebSocket client opened as the
//! page opens one, and reads that wait for what a session sends.

// Each test file uses its own share of these.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

pub const TOKEN: &str = "test-token-0123456789";

/// Send a raw request; the status line and the whole response.
pub fn request(addr: SocketAddr, text: &str) -> (String, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(text.as_bytes()).unwrap();
    let mut response = Vec::new();
    // A 101 keeps the connection open: read what there is.
    let mut buf = [0u8; 65536];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                response.extend_from_slice(&buf[..n]);
                if response.starts_with(b"HTTP/1.1 101")
                    && response.windows(4).any(|w| w == b"\r\n\r\n")
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let response = String::from_utf8_lossy(&response).to_string();
    let status = response.lines().next().unwrap_or("").to_string();
    (status, response)
}

pub fn get(addr: SocketAddr, target: &str) -> (String, String) {
    request(
        addr,
        &format!("GET {target} HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
    )
}

/// A raw upgrade request with `token`, from `origin`, to `host`; the
/// status line.
pub fn upgrade(addr: SocketAddr, token: &str, origin: Option<&str>, host: &str) -> String {
    let origin = origin.map_or(String::new(), |o| format!("Origin: {o}\r\n"));
    let text = format!(
        "GET /ws?token={token}&cols=40&rows=5 HTTP/1.1\r\nHost: {host}\r\n{origin}\
         Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    request(addr, &text).0
}

/// A WebSocket client, as the page opens one, with `extra` added to the
/// query (`"&renderer=dom"`).
pub fn connect_with(
    addr: SocketAddr,
    columns: u16,
    rows: u16,
    extra: &str,
) -> WebSocket<TcpStream> {
    let url = format!("ws://{addr}/ws?token={TOKEN}&cols={columns}&rows={rows}{extra}");
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Origin", format!("http://{addr}").parse().unwrap());
    let stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let (socket, response) = tungstenite::client(request, stream).unwrap();
    assert_eq!(response.status(), 101);
    socket
}

pub fn connect(addr: SocketAddr, columns: u16, rows: u16) -> WebSocket<TcpStream> {
    connect_with(addr, columns, rows, "")
}

fn timed_out(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(e) if matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ))
}

/// Read the text the server sends until it contains `want`, within 5 s;
/// all of it.
pub fn read_until(socket: &mut WebSocket<TcpStream>, want: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = String::new();
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(text)) => {
                seen.push_str(text.as_str());
                if seen.contains(want) {
                    return seen;
                }
            }
            Ok(_) => {}
            Err(e) if timed_out(&e) => {}
            Err(e) => panic!("reading, waiting for {want:?}: {e}; seen {seen:?}"),
        }
    }
    panic!("no {want:?} within 5 s; seen {seen:?}");
}

/// Wait until the server sends its close, within 5 s: the text before it.
pub fn read_close(socket: &mut WebSocket<TcpStream>) -> String {
    read_close_reason(socket).0
}

/// Wait until the server sends its close, within 5 s: the text before it,
/// and the close's reason.
pub fn read_close_reason(socket: &mut WebSocket<TcpStream>) -> (String, String) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = String::new();
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(text)) => seen.push_str(text.as_str()),
            Ok(Message::Close(frame)) => {
                let reason = frame.map_or(String::new(), |f| f.reason.to_string());
                return (seen, reason);
            }
            Ok(_) => {}
            Err(e) if timed_out(&e) => {}
            Err(_) => return (seen, String::new()),
        }
    }
    panic!("no close within 5 s; seen {seen:?}");
}

/// A program's output, as the page reads it: binary messages, each
/// answered with `a` and its length when `ack`. Reads for at most `within`,
/// until `done` says the output so far is enough, or the close; the output
/// and the close's reason (`None` when it did not come).
pub fn read_program(
    socket: &mut WebSocket<TcpStream>,
    ack: bool,
    within: Duration,
    mut done: impl FnMut(&[u8]) -> bool,
) -> (Vec<u8>, Option<String>) {
    let deadline = Instant::now() + within;
    let mut out = Vec::new();
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Binary(bytes)) => {
                out.extend_from_slice(&bytes);
                if ack {
                    socket
                        .send(Message::text(format!("a{}", bytes.len())))
                        .unwrap();
                }
                if done(&out) {
                    return (out, None);
                }
            }
            Ok(Message::Close(frame)) => {
                let reason = frame.map_or(String::new(), |f| f.reason.to_string());
                return (out, Some(reason));
            }
            Ok(_) => {}
            Err(e) if timed_out(&e) => {}
            Err(_) => return (out, None),
        }
    }
    (out, None)
}

pub fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "{what} within 5 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}
