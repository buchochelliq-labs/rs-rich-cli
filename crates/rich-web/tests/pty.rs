//! A program per session, over real sockets on 127.0.0.1: input, resizes
//! and the exit end to end on a real PTY (Unix, through `sh`), the same
//! security checks as an app's, the cap on output held for a slow page, and
//! a test host anywhere.

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::{connect, get, read_close_reason, read_program, upgrade, wait_for, TOKEN};
use rich_embed::ReplayHost;
use rich_web::{ExitStatus, Server};
use tungstenite::Message;

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_host_carries_input_resizes_and_the_exit() {
    let host = ReplayHost::new().output("hello from the host\r\n");
    let handle = host.handle();
    let slot = std::sync::Mutex::new(Some(host));
    let server = Server::bind_host("127.0.0.1:0", move || {
        Box::new(slot.lock().unwrap().take().unwrap_or_default())
    })
    .unwrap()
    .token(TOKEN)
    .spawn()
    .unwrap();
    let mut socket = connect(server.local_addr(), 50, 7);
    let (out, _) = read_program(&mut socket, true, Duration::from_secs(5), |out| {
        text(out).contains("hello from the host")
    });
    assert_eq!(text(&out), "hello from the host\r\n");
    assert_eq!(handle.started(), Some((50, 7)));

    // Input goes to the program as it came: keys, a paste, a mouse report.
    socket.send(Message::text("dls\r")).unwrap();
    socket
        .send(Message::text("d\x1b[200~pasted\x1b[201~\x1b[<0;3;4M"))
        .unwrap();
    socket.send(Message::text("r80,24")).unwrap();
    wait_for("the input and the resize", || {
        handle.sizes() == [(80, 24)] && handle.written().ends_with(b"\x1b[<0;3;4M")
    });
    assert_eq!(
        text(&handle.written()),
        "ls\r\x1b[200~pasted\x1b[201~\x1b[<0;3;4M"
    );

    // More output, then the exit: the close says how it ended.
    handle.feed("bye\r\n");
    handle.exit(ExitStatus::with_code(4));
    let (out, reason) = read_program(&mut socket, true, Duration::from_secs(5), |_| false);
    assert_eq!(text(&out), "bye\r\n");
    assert_eq!(reason.as_deref(), Some("The program exited with code 4."));
    wait_for("the session to end", || server.sessions() == 0);
}

#[test]
fn the_page_closing_ends_the_program() {
    let host = ReplayHost::new().output("x");
    let handle = host.handle();
    let slot = std::sync::Mutex::new(Some(host));
    let server = Server::bind_host("127.0.0.1:0", move || {
        Box::new(slot.lock().unwrap().take().unwrap_or_default())
    })
    .unwrap()
    .token(TOKEN)
    .spawn()
    .unwrap();
    let mut socket = connect(server.local_addr(), 40, 5);
    read_program(&mut socket, true, Duration::from_secs(5), |out| {
        !out.is_empty()
    });
    socket.close(None).unwrap();
    let _ = read_close_reason(&mut socket);
    wait_for("the session to end", || server.sessions() == 0);
    assert!(handle.killed());
}

#[test]
fn stopping_the_server_ends_a_program() {
    let server = Server::bind_host("127.0.0.1:0", || Box::new(ReplayHost::new().output("x")))
        .unwrap()
        .token(TOKEN)
        .spawn()
        .unwrap();
    let mut socket = connect(server.local_addr(), 40, 5);
    read_program(&mut socket, true, Duration::from_secs(5), |out| {
        !out.is_empty()
    });
    server.stop();
    let (_, reason) = read_program(&mut socket, true, Duration::from_secs(5), |_| false);
    assert_eq!(reason.as_deref(), Some("The server stopped."));
}

#[test]
fn a_program_is_checked_like_an_app() {
    let server = Server::bind_command("127.0.0.1:0", ["cat"])
        .unwrap()
        .token(TOKEN)
        .max_sessions(1)
        .spawn()
        .unwrap();
    let addr = server.local_addr();
    let own = format!("http://{addr}");
    let host = addr.to_string();

    // The page needs the token, and is the xterm.js page whatever it asks
    // for: a program has no tree for the DOM renderer.
    let (status, _) = get(addr, "/");
    assert!(status.contains("403"), "{status}");
    for target in [
        format!("/?token={TOKEN}"),
        format!("/?token={TOKEN}&renderer=dom"),
    ] {
        let (status, page) = get(addr, &target);
        assert!(status.contains("200"), "{status}");
        assert!(page.contains("<title>cat</title>"), "titled by the program");
        assert!(page.contains("data-mode=\"program\""));
        assert!(page.contains("<script src=\"xterm.js\"></script>"));
    }

    // The WebSocket: token and origin, refused before any program starts.
    assert!(upgrade(addr, "wrong", Some(&own), &host).contains("403"));
    assert!(upgrade(addr, TOKEN, None, &host).contains("403"));
    assert!(upgrade(addr, TOKEN, Some("http://evil.example"), &host).contains("403"));
    let rebound = format!("evil.example:{}", addr.port());
    assert!(upgrade(addr, TOKEN, Some(&format!("http://{rebound}")), &rebound).contains("403"));
    assert_eq!(server.sessions(), 0);
    server.stop();

    // The cap, with a program that runs until its page goes (a test host,
    // so this runs where `cat` does not).
    let server = Server::bind_host("127.0.0.1:0", || Box::new(ReplayHost::new()))
        .unwrap()
        .token(TOKEN)
        .max_sessions(1)
        .spawn()
        .unwrap();
    let addr = server.local_addr();
    let own = format!("http://{addr}");
    let mut first = connect(addr, 40, 5);
    wait_for("the session to start", || server.sessions() == 1);
    assert!(upgrade(addr, TOKEN, Some(&own), &addr.to_string()).contains("503"));
    // Its slot is free again once it ends.
    first.close(None).unwrap();
    let _ = read_close_reason(&mut first);
    wait_for("the session to end", || server.sessions() == 0);
    let _second = connect(addr, 40, 5);
    wait_for("another session", || server.sessions() == 1);
    server.stop();
}

#[cfg(unix)]
#[test]
fn a_program_runs_end_to_end_on_a_pty() {
    let script = "printf 'ready\\n'; read line; echo \"got:$line\"; stty size; exit 3";
    let server = Server::bind_command("127.0.0.1:0", ["sh", "-c", script])
        .unwrap()
        .token(TOKEN)
        .spawn()
        .unwrap();
    let mut socket = connect(server.local_addr(), 50, 7);
    let (out, _) = read_program(&mut socket, true, Duration::from_secs(10), |out| {
        text(out).contains("ready")
    });
    assert!(text(&out).contains("ready"), "{:?}", text(&out));

    // A resize, then a line typed: the program sees both.
    socket.send(Message::text("r60,9")).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    socket.send(Message::text("dhello\r")).unwrap();
    let (out, reason) = read_program(&mut socket, true, Duration::from_secs(10), |_| false);
    let out = text(&out);
    assert!(out.contains("got:hello"), "{out:?}");
    assert!(out.contains("9 60"), "the new size: {out:?}");
    assert_eq!(reason.as_deref(), Some("The program exited with code 3."));
    wait_for("the session to end", || server.sessions() == 0);
}

#[cfg(unix)]
#[test]
fn a_program_that_cannot_start_says_so() {
    let server = Server::bind_command("127.0.0.1:0", ["/nonexistent/rich-web-test-program"])
        .unwrap()
        .token(TOKEN)
        .spawn()
        .unwrap();
    let mut socket = connect(server.local_addr(), 40, 5);
    let (_, reason) = read_program(&mut socket, true, Duration::from_secs(10), |_| false);
    let reason = reason.expect("a close");
    assert!(
        reason.starts_with("The program could not start")
            || reason.starts_with("The program exited with code"),
        "{reason}"
    );
    assert!(reason.len() <= 123);
}

#[cfg(unix)]
#[test]
fn output_waits_for_a_slow_page() {
    let script = "head -c 400000 /dev/zero | tr '\\0' x; echo; echo END";
    let server = Server::bind_command("127.0.0.1:0", ["sh", "-c", script])
        .unwrap()
        .token(TOKEN)
        .max_buffered(8192)
        .spawn()
        .unwrap();
    let mut socket = connect(server.local_addr(), 80, 24);
    // A page that draws nothing: what is sent stops at the window (and one
    // read of the PTY), and the program waits rather than finishing.
    let (held, reason) = read_program(&mut socket, false, Duration::from_millis(800), |_| false);
    assert_eq!(reason, None, "the program waits");
    assert!(
        !held.is_empty() && held.len() <= 8192 + 64 * 1024,
        "{} bytes sent before the page drew any",
        held.len()
    );
    // The page catches up: everything arrives, nothing dropped, then the
    // exit.
    socket
        .send(Message::text(format!("a{}", held.len())))
        .unwrap();
    let (rest, reason) = read_program(&mut socket, true, Duration::from_secs(20), |_| false);
    let all = text(&[held, rest].concat());
    assert_eq!(all.matches('x').count(), 400_000);
    assert!(all.contains("END"));
    assert_eq!(reason.as_deref(), Some("The program exited with code 0."));
}

#[test]
fn a_request_still_arriving_at_stop_is_not_served() {
    let made = Arc::new(AtomicUsize::new(0));
    let counter = made.clone();
    let server = Server::bind_host("127.0.0.1:0", move || {
        counter.fetch_add(1, Ordering::SeqCst);
        Box::new(ReplayHost::new())
    })
    .unwrap()
    .token(TOKEN)
    .spawn()
    .unwrap();
    let addr = server.local_addr();
    let text = format!(
        "GET /ws?token={TOKEN}&cols=40&rows=5 HTTP/1.1\r\nHost: {addr}\r\n\
         Origin: http://{addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(&text.as_bytes()[..20]).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    server.stop();
    // The rest arrives after the stop: nothing is served, nothing started.
    let _ = stream.write_all(&text.as_bytes()[20..]);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    assert!(response.is_empty(), "{}", text_of(&response));
    assert_eq!(made.load(Ordering::SeqCst), 0);
}

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The `pid:N` a program printed.
#[cfg(unix)]
fn read_pid(socket: &mut tungstenite::WebSocket<TcpStream>) -> u32 {
    let pid = |out: &[u8]| -> Option<u32> {
        text(out)
            .split("pid:")
            .nth(1)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    };
    let (out, _) = read_program(socket, true, Duration::from_secs(10), |out| {
        pid(out).is_some()
    });
    pid(&out).unwrap_or_else(|| panic!("no pid in {:?}", text(&out)))
}

#[cfg(unix)]
#[test]
fn a_program_that_ignores_hangups_does_not_outlive_its_session() {
    use common::running;

    let script = "trap '' HUP; echo pid:$$; exec sleep 60";
    let server = Server::bind_command("127.0.0.1:0", ["sh", "-c", script])
        .unwrap()
        .token(TOKEN)
        .spawn()
        .unwrap();
    // The page goes: the program is killed once it has had a moment.
    let mut socket = connect(server.local_addr(), 40, 5);
    let pid = read_pid(&mut socket);
    assert!(running(pid));
    socket.close(None).unwrap();
    let _ = read_close_reason(&mut socket);
    wait_for("the session to end", || server.sessions() == 0);
    assert!(!running(pid), "the program outlived its session");

    // The server stops: it returns once the program has ended.
    let mut socket = connect(server.local_addr(), 40, 5);
    let pid = read_pid(&mut socket);
    server.stop();
    assert!(!running(pid), "the program outlived the server");
}

#[cfg(unix)]
#[test]
fn a_paste_flood_waits_in_the_network_not_in_the_server() {
    // A program that reads none of its input, in raw mode: the PTY holds
    // a little, then its writes wait.
    let script = "stty raw -echo; echo ready; exec sleep 60";
    let server = Server::bind_command("127.0.0.1:0", ["sh", "-c", script])
        .unwrap()
        .token(TOKEN)
        .spawn()
        .unwrap();
    let mut socket = connect(server.local_addr(), 40, 5);
    read_program(&mut socket, true, Duration::from_secs(10), |out| {
        text(out).contains("ready")
    });
    socket
        .get_mut()
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    // 64 pastes of 1 MiB: the server holds 1 MiB for the program and one
    // paste more, then stops reading, so the rest stays in the network's
    // buffers (a few MiB) and the page's sends wait.
    let paste = format!("d{}", "x".repeat((1 << 20) - 16));
    let mut sent = 0;
    while sent < 64 && socket.send(Message::text(paste.clone())).is_ok() {
        sent += 1;
    }
    assert!(
        sent < 32,
        "{sent} MiB of pastes taken by a program that reads none"
    );
    // The session still ends when the server stops.
    server.stop();
}

#[test]
fn a_program_cannot_write_the_clipboard() {
    // The page puts OSC 52 copies on the clipboard for an app only: a
    // program's output is whatever it shows (a file, a fetched page).
    let server = Server::bind_host("127.0.0.1:0", || Box::new(ReplayHost::new()))
        .unwrap()
        .token(TOKEN)
        .spawn()
        .unwrap();
    let addr = server.local_addr();
    let (status, page) = get(addr, &format!("/?token={TOKEN}"));
    assert!(status.contains("200"), "{status}");
    assert!(page.contains("data-mode=\"program\""));
    let (status, script) = get(addr, "/app.js");
    assert!(status.contains("200"), "{status}");
    assert_eq!(script.matches("registerOscHandler").count(), 1);
    let guard = script
        .find("if (mode === \"app\") {")
        .expect("the mode guard");
    let handler = script.find("registerOscHandler(52").unwrap();
    assert!(guard < handler, "the handler is registered for every mode");
    server.stop();
}
