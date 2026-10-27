//! `HttpsTransport` trusts the system's certificate store, and follows a
//! redirect whose headers arrive a piece at a time.
//!
//! A local HTTPS server presents a certificate from a test CA
//! (tests/fixtures/tls) that no bundled root knows: a stand-in for a
//! company's TLS-inspecting proxy. `SSL_CERT_FILE` puts that CA in the
//! "system" store, which the platform verifier reads on Linux and the BSDs
//! (rustls-native-certs); Windows and macOS ask the OS, which no variable
//! changes, so the test runs where the variable is honoured.
//!
//! One test function: it sets process-wide variables.

#![cfg(all(
    feature = "http",
    unix,
    not(target_os = "macos"),
    not(target_os = "ios"),
    not(target_os = "android")
))]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rich_marketplace::tls::Trust;
use rich_marketplace::{FetchError, HttpsTransport, Transport};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

const BODY: &str = "[theme]\nname = \"fixture\"\n";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tls")
        .join(name)
}

/// Serve `connections` HTTPS connections on a free port: `/redirect` is a
/// chunked `302` to `/file`, sent in many small TLS records with the blank
/// line that ends its headers on its own (as a TLS-inspecting proxy was
/// seen to); `/file` is [`BODY`].
fn serve(connections: usize) -> u16 {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let certs = vec![CertificateDer::from_pem_file(fixture("server.pem")).unwrap()];
    let key = PrivateKeyDer::from_pem_file(fixture("server.key")).unwrap();
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
        for _ in 0..connections {
            let Ok((tcp, _)) = listener.accept() else {
                return;
            };
            let config = Arc::clone(&config);
            thread::spawn(move || {
                // A refused handshake (the bundled-roots client) just ends.
                let _ = answer(tcp, config, port);
            });
        }
    });
    port
}

fn answer(mut tcp: TcpStream, config: Arc<rustls::ServerConfig>, port: u16) -> std::io::Result<()> {
    tcp.set_nodelay(true)?;
    tcp.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut connection = rustls::ServerConnection::new(config).map_err(std::io::Error::other)?;
    let mut tls = rustls::Stream::new(&mut connection, &mut tcp);
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = tls.read(&mut buffer)?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buffer[..n]);
    }
    let request = String::from_utf8_lossy(&request);
    let path = request.split_whitespace().nth(1).unwrap_or("");
    let pieces: Vec<String> = if path == "/redirect" {
        vec![
            "HTTP/1.1 302 Found\r\n".into(),
            "Connection: close\r\n".into(),
            "Transfer-Encoding: chunked\r\n".into(),
            "Content-Type: text/html".into(),
            "\r\n".into(),
            "Location".into(),
            ": ".into(),
            format!("https://127.0.0.1:{port}/file"),
            "\r\n".into(),
            // The end of the headers, in a record of its own.
            "\r\n".into(),
            "0\r\n".into(),
            "\r\n".into(),
        ]
    } else {
        vec![format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{BODY}",
            BODY.len()
        )]
    };
    for piece in pieces {
        tls.write_all(piece.as_bytes())?;
        tls.flush()?;
        thread::sleep(Duration::from_millis(30));
    }
    tls.conn.send_close_notify();
    tls.flush()
}

#[test]
fn the_system_store_is_trusted_and_redirects_survive_fragmented_headers() {
    // The test CA is the whole "system" store; no proxy in the way.
    std::env::set_var("SSL_CERT_FILE", fixture("ca.pem"));
    std::env::remove_var("SSL_CERT_DIR");
    for proxy in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
        std::env::remove_var(proxy);
    }
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");

    // The bundled roots alone refuse the server, as they refuse a company
    // proxy's certificates.
    let port = serve(1);
    let bundled: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(Trust::Bundled.tls_config())
        .build()
        .into();
    let refused = bundled
        .get(&format!("https://127.0.0.1:{port}/file"))
        .call()
        .unwrap_err();
    assert!(
        refused.to_string().contains("UnknownIssuer"),
        "bundled roots: {refused}"
    );

    // The transport trusts the system's store, and follows the redirect
    // itself. Connections: the redirect, then the file.
    let port = serve(2);
    let body = HttpsTransport::new()
        .get(&format!("https://127.0.0.1:{port}/redirect"), 1024)
        .unwrap();
    assert_eq!(String::from_utf8(body).unwrap(), BODY);

    // The size bound still holds after a redirect.
    let port = serve(2);
    let error = HttpsTransport::new()
        .get(&format!("https://127.0.0.1:{port}/redirect"), 4)
        .unwrap_err();
    assert!(
        matches!(error, FetchError::TooLarge { limit: 4, .. }),
        "{error}"
    );
}
