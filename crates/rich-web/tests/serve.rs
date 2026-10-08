//! The server over real sockets on 127.0.0.1: the page and its token, the
//! WebSocket's token, origin and session checks, and an app driven end to
//! end through a WebSocket client, as the page drives it.

mod common;

use common::{connect, get, read_close, read_until, request, upgrade, wait_for, TOKEN};
use rich_web::intuituive::prelude::*;
use rich_web::{Handle, Server};
use tungstenite::Message;

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
