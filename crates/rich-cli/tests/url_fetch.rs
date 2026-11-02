//! `rich URL` against local servers: a redirect from `https://` to
//! `http://` is refused before it is requested, and a fetched body's
//! terminal controls are shown, not sent, unless `--no-sanitize` (both
//! DIVERGENCES §50). Not upstream: rich-cli follows the redirect and
//! passes the controls through.
//!
//! The HTTPS server presents rs-rich-marketplace's test certificate; the
//! child process trusts its CA through `SSL_CERT_FILE`, which the platform
//! verifier reads on Linux and the BSDs (Windows and macOS ask the OS).
#![cfg(feature = "fetch")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// Read a request head; `None` if the client went away first.
fn read_head(stream: &mut impl Read) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let mut byte = [0u8];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        head.push(byte[0]);
        if head.len() > 16_384 {
            return None;
        }
    }
    Some(head)
}

/// A plain-HTTP server answering `body` (as `content_type`) to every
/// request; returns its port and the number of requests it has had.
fn serve_http(body: &'static [u8], content_type: &'static str) -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            if read_head(&mut stream).is_none() {
                continue;
            }
            counted.fetch_add(1, Ordering::SeqCst);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    (port, hits)
}

fn tls_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rich-marketplace/tests/fixtures/tls")
        .join(name)
}

/// An HTTPS server whose every answer is a `302` to `location`.
#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn serve_https_redirect(location: String) -> u16 {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let certs = vec![CertificateDer::from_pem_file(tls_fixture("server.pem")).unwrap()];
    let key = PrivateKeyDer::from_pem_file(tls_fixture("server.key")).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(tcp) = stream else { return };
            let _ = redirect(tcp, Arc::clone(&config), &location);
        }
    });
    port
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn redirect(
    mut tcp: TcpStream,
    config: Arc<rustls::ServerConfig>,
    location: &str,
) -> std::io::Result<()> {
    tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut connection = rustls::ServerConnection::new(config).map_err(std::io::Error::other)?;
    let mut tls = rustls::Stream::new(&mut connection, &mut tcp);
    if read_head(&mut tls).is_none() {
        return Ok(());
    }
    let answer = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\
         Connection: close\r\n\r\n"
    );
    tls.write_all(answer.as_bytes())?;
    tls.conn.send_close_notify();
    tls.flush()
}

fn rich(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rich"));
    command
        .args(["--no-config", "--no-color"])
        .args(args)
        .env("COLUMNS", "80")
        .env_remove("FORCE_COLOR")
        .env_remove("SSL_CERT_DIR")
        .env("SSL_CERT_FILE", tls_fixture("ca.pem"))
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost");
    for proxy in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        command.env_remove(proxy);
    }
    command
}

fn run(args: &[&str]) -> Output {
    rich(args).output().unwrap()
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "android")
))]
#[test]
fn a_redirect_from_https_to_http_is_refused_before_it_is_requested() {
    let (plain, hits) = serve_http(b"over plain http\n", "text/plain");
    let secure = serve_https_redirect(format!("http://127.0.0.1:{plain}/page.txt"));
    let out = run(&[&format!("https://127.0.0.1:{secure}/page.txt")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr.contains("refused its redirect from https://")
            && stderr.contains(&format!("http://127.0.0.1:{plain}/page.txt")),
        "{stderr}"
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("over plain http"));
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the http:// hop was requested"
    );

    // The same address, asked for as http://, is still fetched.
    let out = run(&[&format!("http://127.0.0.1:{plain}/page.txt")]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("over plain http"));
}

/// A body that sets the window title, writes the clipboard, hides a link
/// and clears the screen.
const HOSTILE: &[u8] = b"# Notes\n\nbefore \x1b]0;owned\x07 \x1b]52;c;aGk=\x07 \
\x1b]8;;https://example.invalid/\x1b\\link\x1b]8;;\x1b\\ \x1b[2J after\n";

fn controls(bytes: &[u8]) -> bool {
    bytes.windows(2).any(|w| w == b"\x1b]" || w == b"\x1b[")
}

#[test]
fn a_urls_terminal_controls_are_shown_not_sent_unless_asked() {
    let (port, _) = serve_http(HOSTILE, "text/plain");
    let url = format!("http://127.0.0.1:{port}/notes");
    // Text, Markdown and --print: every mode sanitizes a fetched body.
    for args in [
        vec![url.as_str()],
        vec![url.as_str(), "--markdown"],
        vec![url.as_str(), "--syntax", "--lexer", "text"],
    ] {
        let out = run(&args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !controls(&out.stdout),
            "{args:?}: {:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        let shown = String::from_utf8_lossy(&out.stdout);
        assert!(shown.contains("after"), "{args:?}: {shown}");
    }
    // `--no-sanitize` lets them through, as upstream does.
    let out = run(&[&url, "--no-sanitize"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        controls(&out.stdout),
        "{:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}
