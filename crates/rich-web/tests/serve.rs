//! The server over real sockets on 127.0.0.1: the page and its token, the
//! WebSocket's token, origin and session checks, and an app driven end to
//! end through a WebSocket client, as the page drives it.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use rich_web::intuituive::prelude::*;
use rich_web::{Handle, Server};
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message, WebSocket};

const TOKEN: &str = "test-token-0123456789";

/// An app whose one line is replaced whole by each key, so the cells a key
/// changes spell the new word.
fn app() -> App {
    App::new(|| {
        let word = signal("waiting".to_string());
        text!("{word}")
            .on_key("+", move |_| word.set("PRESSED".into()))
            .on_key("up", move |_| word.set("NORTH".into()))
            .on_key("ctrl+v", move |_| word.set("VIVID".into()))
            .on_click(move |_| word.set("CLICKED".into()))
    })
}

fn start(sessions: usize) -> Handle {
    Server::bind("127.0.0.1:0", app)
        .unwrap()
        .token(TOKEN)
        .max_sessions(sessions)
        .title("Test <app>")
        .spawn()
        .unwrap()
}

/// Send a raw request; the status line and the whole response.
fn request(addr: SocketAddr, text: &str) -> (String, String) {
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

fn get(addr: SocketAddr, target: &str) -> (String, String) {
    request(
        addr,
        &format!("GET {target} HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
    )
}

/// A raw upgrade request with `token`, from `origin`, to `host`.
fn upgrade(addr: SocketAddr, token: &str, origin: Option<&str>, host: &str) -> String {
    let origin = origin.map_or(String::new(), |o| format!("Origin: {o}\r\n"));
    let text = format!(
        "GET /ws?token={token}&cols=40&rows=5 HTTP/1.1\r\nHost: {host}\r\n{origin}\
         Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    request(addr, &text).0
}

/// A WebSocket client, as the page opens one.
fn connect(addr: SocketAddr, columns: u16, rows: u16) -> WebSocket<TcpStream> {
    let url = format!("ws://{addr}/ws?token={TOKEN}&cols={columns}&rows={rows}");
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

/// Read what the server sends until it contains `want`, within 5 s; all
/// of it.
fn read_until(socket: &mut WebSocket<TcpStream>, want: &str) -> String {
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
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => panic!("reading, waiting for {want:?}: {e}; seen {seen:?}"),
        }
    }
    panic!("no {want:?} within 5 s; seen {seen:?}");
}

/// Wait until the server sends its close, within 5 s.
fn read_close(socket: &mut WebSocket<TcpStream>) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = String::new();
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(text)) => seen.push_str(text.as_str()),
            Ok(Message::Close(_)) => return seen,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return seen,
        }
    }
    panic!("no close within 5 s; seen {seen:?}");
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "{what} within 5 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_page_needs_the_token() {
    let server = start(8);
    let addr = server.local_addr();
    assert!(server.url().ends_with(&format!("/?token={TOKEN}")));

    let (status, _) = get(addr, "/");
    assert!(status.contains("403"), "{status}");
    let (status, _) = get(addr, "/?token=wrong");
    assert!(status.contains("403"), "{status}");

    let (status, page) = get(addr, &format!("/?token={TOKEN}"));
    assert!(status.contains("200"), "{status}");
    assert!(page.contains("<script src=\"xterm.js\"></script>"));
    // The title is escaped into the page.
    assert!(page.contains("<title>Test &lt;app&gt;</title>"));
    assert!(page.contains("Content-Security-Policy: default-src 'none'"));
    assert!(page.contains("frame-ancestors 'none'"));
    assert!(page.contains("X-Frame-Options: DENY"));

    // The page's assets, served by the crate.
    for (path, needle) in [
        ("/xterm.js", "Terminal"),
        ("/addon-fit.js", "FitAddon"),
        ("/xterm.css", ".xterm"),
        ("/app.js", "registerOscHandler"),
    ] {
        let (status, body) = get(addr, path);
        assert!(status.contains("200"), "{path}: {status}");
        assert!(body.contains(needle), "{path}");
    }
    let (status, _) = get(addr, "/../etc/passwd");
    assert!(status.contains("404"), "{status}");
    let (status, _) = request(addr, "POST / HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(status.contains("405"), "{status}");
    server.stop();
}

#[test]
fn the_websocket_checks_token_and_origin() {
    let server = start(8);
    let addr = server.local_addr();
    let port = addr.port();
    let own = format!("http://{addr}");
    let host = addr.to_string();

    // The server's own page, by address and by name.
    assert!(upgrade(addr, TOKEN, Some(&own), &host).contains("101"));
    let localhost = format!("localhost:{port}");
    assert!(upgrade(
        addr,
        TOKEN,
        Some(&format!("http://{localhost}")),
        &localhost
    )
    .contains("101"));

    // A wrong or missing token.
    assert!(upgrade(addr, "wrong", Some(&own), &host).contains("403"));
    assert!(upgrade(addr, "", Some(&own), &host).contains("403"));
    // No origin, another site, another port, a rebound name.
    assert!(upgrade(addr, TOKEN, None, &host).contains("403"));
    assert!(upgrade(addr, TOKEN, Some("http://evil.example"), &host).contains("403"));
    assert!(upgrade(addr, TOKEN, Some("http://127.0.0.1:1"), &host).contains("403"));
    let rebound = format!("evil.example:{port}");
    assert!(upgrade(addr, TOKEN, Some(&format!("http://{rebound}")), &rebound).contains("403"));
    // Not an upgrade at all.
    let (status, _) = get(addr, &format!("/ws?token={TOKEN}"));
    assert!(status.contains("400"), "{status}");
    server.stop();
}

#[test]
fn an_allowed_origin_may_connect() {
    let server = Server::bind("127.0.0.1:0", app)
        .unwrap()
        .token(TOKEN)
        .allow_origin("https://term.example.com")
        .spawn()
        .unwrap();
    let addr = server.local_addr();
    assert!(upgrade(
        addr,
        TOKEN,
        Some("https://term.example.com"),
        "term.example.com"
    )
    .contains("101"));
    assert!(upgrade(addr, TOKEN, Some("https://other.example.com"), "x").contains("403"));
}

#[test]
fn sessions_are_capped() {
    let server = start(1);
    let addr = server.local_addr();
    let own = format!("http://{addr}");
    let host = addr.to_string();

    let mut first = connect(addr, 40, 5);
    read_until(&mut first, "waiting");
    assert_eq!(server.sessions(), 1);
    assert!(upgrade(addr, TOKEN, Some(&own), &host).contains("503"));

    // The first session ends: its slot is free again.
    first.close(None).unwrap();
    let _ = read_close(&mut first);
    wait_for("the session to end", || server.sessions() == 0);
    let mut second = connect(addr, 40, 5);
    read_until(&mut second, "waiting");
    server.stop();
}

#[test]
fn an_app_runs_end_to_end() {
    let server = start(8);
    let mut socket = connect(server.local_addr(), 40, 5);

    // The first frame, after the modes the page needs.
    let first = read_until(&mut socket, "waiting");
    assert!(first.contains("\x1b[?1049h"), "the alternate screen");
    assert!(first.contains("\x1b[?1006h"), "SGR mouse reports on");
    assert!(first.contains("\x1b[?2004h"), "bracketed paste on");

    // Keys, as xterm.js sends them; each changes the line.
    socket.send(Message::text("d+")).unwrap();
    read_until(&mut socket, "PRESSED");
    socket.send(Message::text("d\x1b[A")).unwrap();
    read_until(&mut socket, "NORTH");
    socket.send(Message::text("d\x16")).unwrap();
    read_until(&mut socket, "VIVID");
    // A click on the line, as an SGR report (press and release).
    socket
        .send(Message::text("d\x1b[<0;2;1M\x1b[<0;2;1m"))
        .unwrap();
    read_until(&mut socket, "CLICKED");

    // A new size draws everything again.
    socket.send(Message::text("r60,10")).unwrap();
    read_until(&mut socket, "CLICKED");

    // Ctrl+C quits the app: the server restores the modes and closes.
    socket.send(Message::text("d\x03")).unwrap();
    let last = read_close(&mut socket);
    assert!(
        last.contains("\x1b[?1006l"),
        "modes off at the end: {last:?}"
    );
    wait_for("the session to end", || server.sessions() == 0);
    server.stop();
}

#[test]
fn stopping_the_server_ends_its_sessions() {
    let server = start(8);
    let mut socket = connect(server.local_addr(), 40, 5);
    read_until(&mut socket, "waiting");
    server.stop();
    read_close(&mut socket);
}
