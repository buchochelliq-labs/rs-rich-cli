//! The little HTTP the server speaks: reading a request's head, the checks
//! a WebSocket upgrade must pass, and plain responses for the page and its
//! assets. Every response closes its connection.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr};

/// The longest request head read; longer is refused.
const MAX_HEAD: usize = 16 * 1024;

/// A request's method, path, query and headers (names in lower case).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Head {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// Parse a complete head (up to and including the blank line).
    pub fn parse(bytes: &[u8]) -> Option<Head> {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(bytes) {
            Ok(httparse::Status::Complete(_)) => {}
            _ => return None,
        }
        let target = request.path?;
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let query = query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                (name.to_string(), value.to_string())
            })
            .collect();
        let headers = request
            .headers
            .iter()
            .map(|h| {
                (
                    h.name.to_ascii_lowercase(),
                    String::from_utf8_lossy(h.value).trim().to_string(),
                )
            })
            .collect();
        Some(Head {
            method: request.method?.to_string(),
            path: path.to_string(),
            query,
            headers,
        })
    }

    /// The first header called `name` (lower case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The first query parameter called `name`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Read a request head from `stream`: up to the blank line, at most
/// [`MAX_HEAD`] bytes. Nothing after the head is read (a browser sends
/// nothing more before the server answers an upgrade or a `GET`).
pub(crate) fn read_head(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    // One byte at a time, so nothing past the head is consumed. Heads are
    // small, and the stream is buffered by the kernel.
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_HEAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request head too long",
            ));
        }
        match stream.read(&mut byte)? {
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            _ => head.push(byte[0]),
        }
    }
    Ok(head)
}

/// Compare two tokens in time that does not depend on where they differ.
pub(crate) fn same_token(given: &str, expected: &str) -> bool {
    let (a, b) = (given.as_bytes(), expected.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The `Host` values the server answers to, from the address it is bound
/// to: for a loopback address, that address and `localhost`, with its port;
/// for another address, that address; for an unspecified address (all
/// interfaces), `None`: any host, as long as the origin matches it.
pub(crate) fn allowed_hosts(local: SocketAddr) -> Option<Vec<String>> {
    let port = local.port();
    let ip = local.ip();
    if ip.is_unspecified() {
        return None;
    }
    let mut names = vec![match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    }];
    if ip.is_loopback() {
        names.push("localhost".into());
    }
    // Browsers leave HTTP's default port out of `Host` and the origin.
    let mut hosts: Vec<String> = names.iter().map(|name| format!("{name}:{port}")).collect();
    if port == 80 {
        hosts.extend(names);
    }
    Some(hosts)
}

/// Whether a WebSocket upgrade may come from `origin`, sent to `host`.
///
/// Browsers send the page's origin with every WebSocket handshake, and any
/// web page can open a WebSocket to a localhost port: the origin is what
/// tells this server's own page from another site's. It must be this
/// server's own address (`http://` and an allowed host, which must also be
/// the `Host` the request was sent to, against DNS rebinding), or one of
/// `extra`, the origins a proxy in front of the server serves the page
/// from. A request with no origin (not from a browser) is refused too.
pub(crate) fn origin_allowed(
    origin: Option<&str>,
    host: Option<&str>,
    hosts: Option<&[String]>,
    extra: &[String],
) -> bool {
    let Some(origin) = origin else {
        return false;
    };
    let origin = origin.trim_end_matches('/');
    if extra
        .iter()
        .any(|o| o.trim_end_matches('/').eq_ignore_ascii_case(origin))
    {
        return true;
    }
    let Some(host) = host else {
        return false;
    };
    let host_allowed = match hosts {
        Some(hosts) => hosts.iter().any(|h| h.eq_ignore_ascii_case(host)),
        None => true,
    };
    host_allowed
        && origin
            .strip_prefix("http://")
            .is_some_and(|o| o.eq_ignore_ascii_case(host))
}

/// Whether `head` asks to upgrade to a WebSocket (RFC 6455, version 13),
/// with the key to answer.
pub(crate) fn websocket_key(head: &Head) -> Option<&str> {
    let upgrade = head.header("upgrade")?;
    let connection = head.header("connection")?;
    let version = head.header("sec-websocket-version")?;
    if head.method != "GET"
        || !upgrade.eq_ignore_ascii_case("websocket")
        || !connection
            .split(',')
            .any(|t| t.trim().eq_ignore_ascii_case("upgrade"))
        || version != "13"
    {
        return None;
    }
    head.header("sec-websocket-key").filter(|k| !k.is_empty())
}

/// A plain response: `status`, a type, the body, and any `extra` headers.
pub(crate) fn respond(
    stream: &mut impl Write,
    status: &str,
    content_type: &str,
    extra: &[(&str, String)],
    body: &[u8],
) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Connection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n",
        body.len()
    );
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// A plain-text error response.
pub(crate) fn refuse(stream: &mut impl Write, status: &str, why: &str) -> io::Result<()> {
    respond(
        stream,
        status,
        "text/plain; charset=utf-8",
        &[],
        format!("{why}\n").as_bytes(),
    )
}

/// Escape `text` for HTML.
pub(crate) fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(text: &str) -> Head {
        Head::parse(text.as_bytes()).expect("a complete head")
    }

    #[test]
    fn parses_a_head() {
        let h = head("GET /ws?token=abc&cols=80&rows=24 HTTP/1.1\r\nHost: 127.0.0.1:9\r\nOrigin: http://127.0.0.1:9\r\n\r\n");
        assert_eq!(h.method, "GET");
        assert_eq!(h.path, "/ws");
        assert_eq!(h.param("token"), Some("abc"));
        assert_eq!(h.param("cols"), Some("80"));
        assert_eq!(h.param("nope"), None);
        assert_eq!(h.header("host"), Some("127.0.0.1:9"));
        assert!(Head::parse(b"GET / HTTP/1.1\r\nHost: x\r\n").is_none());
    }

    #[test]
    fn reads_only_the_head() {
        let mut input: &[u8] = b"GET / HTTP/1.1\r\nHost: x\r\n\r\nafter";
        let got = read_head(&mut input).unwrap();
        assert!(got.ends_with(b"\r\n\r\n"));
        assert_eq!(input, b"after");
        let mut short: &[u8] = b"GET / HTTP/1.1\r\n";
        assert!(read_head(&mut short).is_err());
        let long = vec![b'a'; MAX_HEAD + 10];
        assert!(read_head(&mut long.as_slice()).is_err());
    }

    #[test]
    fn tokens_compare_exactly() {
        assert!(same_token("abc123", "abc123"));
        assert!(!same_token("abc124", "abc123"));
        assert!(!same_token("abc12", "abc123"));
        assert!(!same_token("", "abc123"));
    }

    #[test]
    fn hosts_follow_the_bound_address() {
        let local: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        assert_eq!(
            allowed_hosts(local),
            Some(vec!["127.0.0.1:8080".into(), "localhost:8080".into()])
        );
        let v6: SocketAddr = "[::1]:8080".parse().unwrap();
        assert_eq!(
            allowed_hosts(v6),
            Some(vec!["[::1]:8080".into(), "localhost:8080".into()])
        );
        // On port 80 browsers send no port: both forms are this server.
        let lan: SocketAddr = "192.168.1.5:80".parse().unwrap();
        assert_eq!(
            allowed_hosts(lan),
            Some(vec!["192.168.1.5:80".into(), "192.168.1.5".into()])
        );
        let local80: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let hosts = allowed_hosts(local80).unwrap();
        assert!(origin_allowed(
            Some("http://127.0.0.1"),
            Some("127.0.0.1"),
            Some(hosts.as_slice()),
            &[]
        ));
        assert!(origin_allowed(
            Some("http://localhost"),
            Some("localhost"),
            Some(hosts.as_slice()),
            &[]
        ));
        let all: SocketAddr = "0.0.0.0:80".parse().unwrap();
        assert_eq!(allowed_hosts(all), None);
    }

    #[test]
    fn origins_are_checked() {
        let hosts = allowed_hosts("127.0.0.1:8080".parse().unwrap()).unwrap();
        let hosts = Some(hosts.as_slice());
        let ok =
            |origin: Option<&str>, host: Option<&str>| origin_allowed(origin, host, hosts, &[]);
        // The server's own page.
        assert!(ok(Some("http://127.0.0.1:8080"), Some("127.0.0.1:8080")));
        assert!(ok(Some("http://localhost:8080"), Some("localhost:8080")));
        assert!(ok(Some("http://LOCALHOST:8080/"), Some("localhost:8080")));
        // Another site, another port, no origin, an opaque origin.
        assert!(!ok(Some("http://evil.example"), Some("127.0.0.1:8080")));
        assert!(!ok(Some("http://127.0.0.1:9999"), Some("127.0.0.1:8080")));
        assert!(!ok(None, Some("127.0.0.1:8080")));
        assert!(!ok(Some("null"), Some("127.0.0.1:8080")));
        // Origin and host agree, but the host is not this server: a
        // rebound name.
        assert!(!ok(
            Some("http://evil.example:8080"),
            Some("evil.example:8080")
        ));
        // The page served from the same host over HTTPS needs `extra`.
        assert!(!ok(Some("https://127.0.0.1:8080"), Some("127.0.0.1:8080")));
        assert!(!ok(Some("http://127.0.0.1:8080"), None));
        // A proxy's origin, allowed by name.
        let extra = ["https://term.example.com".to_string()];
        assert!(origin_allowed(
            Some("https://term.example.com"),
            Some("anything"),
            hosts,
            &extra
        ));
        // Bound to every interface: any host, but the origin must match it.
        assert!(origin_allowed(
            Some("http://10.0.0.2:80"),
            Some("10.0.0.2:80"),
            None,
            &[]
        ));
        assert!(!origin_allowed(
            Some("http://evil.example"),
            Some("10.0.0.2:80"),
            None,
            &[]
        ));
    }

    #[test]
    fn upgrades_are_recognised() {
        let upgrade = "GET /ws HTTP/1.1\r\nHost: h\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
        assert_eq!(
            websocket_key(&head(upgrade)),
            Some("dGhlIHNhbXBsZSBub25jZQ==")
        );
        let old = upgrade.replace("Version: 13", "Version: 8");
        assert_eq!(websocket_key(&head(&old)), None);
        let plain = "GET /ws HTTP/1.1\r\nHost: h\r\n\r\n";
        assert_eq!(websocket_key(&head(plain)), None);
    }

    #[test]
    fn responses_close_and_escape() {
        let mut out = Vec::new();
        respond(
            &mut out,
            "200 OK",
            "text/plain",
            &[("X-A", "b".into())],
            b"hi",
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        assert!(text.contains("X-A: b\r\n"));
        assert!(text.ends_with("\r\n\r\nhi"));
        assert_eq!(escape_html("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }
}
