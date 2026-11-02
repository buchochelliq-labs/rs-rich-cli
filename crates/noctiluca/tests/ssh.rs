//! `--ssh HOST` (#817) against a stand-in for ssh: this test binary, run
//! as `ssh` by a two-line script, serves SOCKS5 on the `-D` address as
//! `ssh -D` does, and connects what it is asked for on "the far side". A
//! connection to the far side's `localhost:1` reaches the test's dev
//! server, which listens elsewhere, so only a request that went through
//! the tunnel can reach it. With `RICH_EMBED_CHROME` set, Chrome itself
//! opens `http://localhost:1/` through it; without, that test skips.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use noctiluca::ssh::{SshOptions, Tunnel, TunnelState, socks_greets};

/// The far side's port that leads to the dev server.
const FAR_PORT: u16 = 1;

/// The stand-in ssh, when the script runs this binary as it: SOCKS5 on
/// `-D`'s address; with `-o FakeExitAfter=1`, it exits after one
/// connection (a dropped tunnel).
#[test]
fn fake_ssh() {
    let Ok(args) = std::env::var("NOCTILUCA_FAKE_SSH_ARGS") else {
        return;
    };
    let dev: u16 = std::env::var("NOCTILUCA_FAKE_SSH_DEV")
        .unwrap()
        .parse()
        .unwrap();
    let words: Vec<&str> = args.split(' ').collect();
    let bind = words
        .windows(2)
        .find(|w| w[0] == "-D")
        .map(|w| w[1])
        .expect("-D ADDRESS:PORT");
    let exit_after_one = words.contains(&"FakeExitAfter=1");
    let listener = TcpListener::bind(bind).expect("the forward's port");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let served = std::thread::spawn(move || serve(stream, dev));
        if exit_after_one {
            let _ = served.join();
            std::process::exit(0);
        }
    }
}

/// One SOCKS5 connection, as `ssh -D` serves it: no authentication, then
/// CONNECT to a name or an IPv4 address. The far side's `localhost:1` is
/// the dev server.
fn serve(mut client: TcpStream, dev: u16) {
    let mut head = [0u8; 2];
    if client.read_exact(&mut head).is_err() || head[0] != 5 {
        return;
    }
    let mut methods = vec![0u8; head[1] as usize];
    let _ = client.read_exact(&mut methods);
    let _ = client.write_all(&[5, 0]);
    let mut request = [0u8; 4];
    if client.read_exact(&mut request).is_err() {
        return;
    }
    let host = match request[3] {
        1 => {
            let mut ip = [0u8; 4];
            let _ = client.read_exact(&mut ip);
            Ipv4Addr::from(ip).to_string()
        }
        3 => {
            let mut len = [0u8; 1];
            let _ = client.read_exact(&mut len);
            let mut name = vec![0u8; len[0] as usize];
            let _ = client.read_exact(&mut name);
            String::from_utf8_lossy(&name).into_owned()
        }
        _ => return,
    };
    let mut port = [0u8; 2];
    let _ = client.read_exact(&mut port);
    let port = u16::from_be_bytes(port);
    let target = match (host.as_str(), port) {
        ("localhost" | "127.0.0.1", FAR_PORT) => format!("127.0.0.1:{dev}"),
        _ => {
            // Host unreachable.
            let _ = client.write_all(&[5, 4, 0, 1, 0, 0, 0, 0, 0, 0]);
            return;
        }
    };
    let Ok(upstream) = TcpStream::connect(target) else {
        let _ = client.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]);
        return;
    };
    let _ = client.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]);
    let (mut a, mut b) = (client.try_clone().unwrap(), upstream.try_clone().unwrap());
    let (mut c, mut d) = (upstream, client);
    let up = std::thread::spawn(move || std::io::copy(&mut a, &mut b));
    let _ = std::io::copy(&mut c, &mut d);
    let _ = d.shutdown(std::net::Shutdown::Both);
    let _ = up.join();
}

/// A dev server on a port of its own: every request gets one small page.
fn dev_server() -> u16 {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || {
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let body = "<html><head><title>Far side</title></head>\
                            <body><p>Served on the far side.</p></body></html>";
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    port
}

/// A script that runs this binary as the stand-in ssh, with its
/// arguments in the environment.
fn fake_ssh_program(dev: u16) -> PathBuf {
    // One per dev server: tests run side by side.
    let dir = std::env::temp_dir().join(format!("noctiluca-fake-ssh-{}-{dev}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("ssh");
    let exe = std::env::current_exe().unwrap();
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nNOCTILUCA_FAKE_SSH_ARGS=\"$*\" NOCTILUCA_FAKE_SSH_DEV={dev} \
             exec '{}' --exact fake_ssh --nocapture --test-threads=1 >/dev/null\n",
            exe.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    script
}

/// Through the SOCKS5 port: CONNECT to `host`:`port` by name, then a GET;
/// what came back.
fn fetch(socks: u16, host: &str, port: u16) -> std::io::Result<String> {
    let mut stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, socks))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&[5, 1, 0])?;
    let mut answer = [0u8; 2];
    stream.read_exact(&mut answer)?;
    let mut request = vec![5, 1, 0, 3, host.len() as u8];
    request.extend_from_slice(host.as_bytes());
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request)?;
    let mut reply = [0u8; 10];
    stream.read_exact(&mut reply)?;
    if reply[1] != 0 {
        return Err(std::io::Error::other(format!(
            "SOCKS refused: {}",
            reply[1]
        )));
    }
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    let mut out = String::new();
    stream.read_to_string(&mut out)?;
    Ok(out)
}

fn unix() -> bool {
    if cfg!(unix) {
        return true;
    }
    eprintln!("skipped: the stand-in ssh is a shell script");
    false
}

#[test]
fn the_far_side_s_localhost_is_reached_through_the_tunnel() {
    if !unix() {
        return;
    }
    let dev = dev_server();
    let mut options = SshOptions::new("me@devbox");
    options.program = fake_ssh_program(dev);
    let tunnel = Tunnel::open(options, Duration::from_secs(20)).unwrap();
    let port = tunnel.port();
    let page = fetch(port, "localhost", FAR_PORT).unwrap();
    assert!(page.contains("Served on the far side."), "{page}");
    // Names are the far side's to resolve: another one is refused there.
    assert!(fetch(port, "elsewhere.invalid", 80).is_err());
    // Dropping the tunnel stops ssh: its port answers no more.
    drop(tunnel);
    let end = Instant::now() + Duration::from_secs(5);
    while socks_greets(port, Duration::from_millis(200)) {
        assert!(Instant::now() < end, "the tunnel outlived its Tunnel");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_dropped_tunnel_says_so_and_comes_back_on_its_port() {
    if !unix() {
        return;
    }
    let dev = dev_server();
    let mut options = SshOptions::new("me@devbox");
    options.program = fake_ssh_program(dev);
    // The stand-in exits after one connection: the readiness check's.
    options.args = vec!["-o".into(), "FakeExitAfter=1".into()];
    let mut tunnel = Tunnel::open(options, Duration::from_secs(20)).unwrap();
    let port = tunnel.port();
    let end = Instant::now() + Duration::from_secs(10);
    let why = loop {
        if let TunnelState::Down(why) = tunnel.check() {
            break why;
        }
        assert!(Instant::now() < end, "the tunnel never dropped");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(why, "ssh to me@devbox exited");
    tunnel.reconnect().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if tunnel.check() == TunnelState::Up {
            break;
        }
        assert!(Instant::now() < end, "the tunnel never came back");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(tunnel.port(), port, "Chrome's proxy switch names the port");
}

/// Chrome itself, through the tunnel: `http://localhost:1/` is the far
/// side's, so the page arrives only if Chrome sent `localhost` through
/// the proxy (`<-loopback>`) and resolved nothing itself.
#[test]
fn chrome_opens_the_far_side_s_localhost() {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME").map(PathBuf::from) else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    if !unix() {
        return;
    }
    use noctiluca::cdp::{ChromeBackend, ChromeOptions};
    use noctiluca::tab::{Command, Tab, Update};
    let dev = dev_server();
    let mut options = SshOptions::new("me@devbox");
    options.program = fake_ssh_program(dev);
    let tunnel = Tunnel::open(options, Duration::from_secs(20)).unwrap();
    let backend = ChromeBackend::start(ChromeOptions {
        binary: Some(binary),
        switches: tunnel.chrome_switches(),
        columns: 100,
        rows: 40,
        ..ChromeOptions::default()
    })
    .unwrap();
    let (send, updates) = std::sync::mpsc::channel();
    let tab = Tab::spawn(Box::new(backend), move |u| {
        let _ = send.send(u);
    });
    tab.send(Command::Open(format!("http://localhost:{FAR_PORT}/")));
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Update::Page(page)) = updates.recv_timeout(Duration::from_millis(200))
            && page.text().contains("Served on the far side.")
        {
            break;
        }
        assert!(Instant::now() < end, "the far side's page never came");
    }
}

/// The app's status bar says where the network goes, and when the tunnel
/// drops; it reconnects by itself.
#[test]
fn the_status_bar_says_where_the_network_goes_and_when_it_drops() {
    if !unix() {
        return;
    }
    use noctiluca::app::{Options, app};
    use noctiluca::ssh::TunnelLink;
    use noctiluca::testing::FakeEngine;
    let screen = |d: &rich_intuituive::Driver| d.screen().plain().join("\n");
    let wait = |d: &mut rich_intuituive::Driver, what: &str| {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            d.advance(Duration::from_millis(100));
            d.settle();
            if screen(d).contains(what) {
                return;
            }
            assert!(Instant::now() < end, "no {what:?}:\n{}", screen(d));
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    for drops in [false, true] {
        let dev = dev_server();
        let mut ssh = SshOptions::new("me@devbox");
        ssh.program = fake_ssh_program(dev);
        if drops {
            ssh.args = vec!["-o".into(), "FakeExitAfter=1".into()];
        }
        let tunnel = TunnelLink::new(Tunnel::open(ssh, Duration::from_secs(20)).unwrap());
        let snapshots = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        let engine = FakeEngine::from_dir(&snapshots).unwrap();
        let options = Options {
            config: Default::default(),
            bookmarks: Default::default(),
            url: Some("reader.html".into()),
            view: None,
            images: None,
            tunnel: Some(tunnel),
        };
        let mut driver = app(Box::new(engine), options).driver(100, 20);
        if drops {
            wait(&mut driver, "ssh me@devbox dropped");
        } else {
            wait(&mut driver, "via ssh me@devbox");
        }
    }
}
