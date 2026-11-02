//! The little HTTP the server speaks: reading a request's head, the checks
//! a WebSocket upgrade must pass, and plain responses for the page and its
//! assets. Every response closes its connection.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Instant;

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

/// A stream whose reads all end by one deadline, however slowly the bytes
/// come: each read waits only for what is left of it.
pub(crate) struct Deadline<'a> {
    stream: &'a TcpStream,
    until: Instant,
}

impl<'a> Deadline<'a> {
    pub fn new(stream: &'a TcpStream, until: Instant) -> Deadline<'a> {
        Deadline { stream, until }
    }
}

impl Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "too slow"));
        }
        self.stream.set_read_timeout(Some(left))?;
        let mut stream = self.stream;
        stream.read(buf)
    }
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
/// for another address, that address. For an unspecified address (all
/// interfaces), still a list, never any host: the loopback addresses,
/// `localhost` and this machine's own name ([`machine_names`]), to which
/// each connection adds the address it arrived on ([`connection_hosts`])
/// and [`Server::allow_host`](crate::Server::allow_host) adds more. A name
/// someone else's DNS points here (DNS rebinding) is none of these.
pub(crate) fn allowed_hosts(local: SocketAddr) -> Vec<String> {
    let ip = local.ip();
    let names = if ip.is_unspecified() {
        let mut names = vec![
            "127.0.0.1".to_string(),
            "[::1]".to_string(),
            "localhost".to_string(),
        ];
        names.extend(machine_names());
        names
    } else {
        let mut names = vec![ip_host(ip)];
        if ip.is_loopback() {
            names.push("localhost".into());
        }
        names
    };
    with_port(&names, local.port())
}

/// `names` with `:port`, and bare too on port 80: browsers leave HTTP's
/// default port out of `Host` and the origin.
fn with_port(names: &[String], port: u16) -> Vec<String> {
    let mut hosts: Vec<String> = names.iter().map(|name| format!("{name}:{port}")).collect();
    if port == 80 {
        hosts.extend(names.iter().cloned());
    }
    hosts
}

/// An address as a `Host` names it: IPv6 in brackets, an IPv4-mapped IPv6
/// address (a v4 client of a socket bound to `::`) as the IPv4 one.
fn ip_host(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => format!("[{v6}]"),
        },
    }
}

/// The `Host` values for the address a connection arrived on (its local
/// address): what a browser sends when it was given that address.
pub(crate) fn connection_hosts(local: SocketAddr) -> Vec<String> {
    with_port(&[ip_host(local.ip())], local.port())
}

/// The `Host` values for a name given to
/// [`Server::allow_host`](crate::Server::allow_host): as given when it has
/// a port (`name:8080`, `[::1]:8080`), else with the server's.
pub(crate) fn host_entries(given: &str, port: u16) -> Vec<String> {
    let given = given.trim().to_ascii_lowercase();
    let has_port = match given.rsplit_once(':') {
        Some((head, tail)) => {
            !tail.is_empty()
                && tail.bytes().all(|b| b.is_ascii_digit())
                && (!head.contains(':') || head.ends_with(']'))
        }
        None => false,
    };
    if has_port {
        vec![given]
    } else {
        with_port(&[given], port)
    }
}

/// This machine's own name, as a browser on the network might be given it:
/// its host name and, when that has no dot, `NAME.local` (mDNS). Names
/// with anything but letters, digits, `-` and `.` are left out.
pub(crate) fn machine_names() -> Vec<String> {
    #[cfg(unix)]
    let name = rustix::system::uname()
        .nodename()
        .to_string_lossy()
        .into_owned();
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").unwrap_or_default();
    #[cfg(not(any(unix, windows)))]
    let name = String::new();
    let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty()
        || name == "localhost"
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return Vec::new();
    }
    let mut names = vec![name.clone()];
    if !name.contains('.') {
        names.push(format!("{name}.local"));
    }
    names
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
    hosts: &[String],
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
    hosts.iter().any(|h| h.eq_ignore_ascii_case(host))
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
        let h = head(
            "GET /ws?token=abc&cols=80&rows=24 HTTP/1.1\r\nHost: 127.0.0.1:9\r\nOrigin: http://127.0.0.1:9\r\n\r\n",
        );
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
    fn a_head_has_one_deadline_however_slowly_it_comes() {
        use std::net::TcpListener;
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            // A byte every 100 ms: each read is quick, the whole is not.
            for byte in b"GET / HTTP/1.1\r\nHost: x\r\n\r\n" {
                if stream.write_all(&[*byte]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let (stream, _) = listener.accept().unwrap();
        let start = Instant::now();
        let until = start + Duration::from_millis(500);
        let error = read_head(&mut Deadline::new(&stream, until)).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ),
            "{error}"
        );
        let took = start.elapsed();
        assert!(
            took >= Duration::from_millis(450) && took < Duration::from_secs(2),
            "{took:?}"
        );
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
            vec!["127.0.0.1:8080".to_string(), "localhost:8080".into()]
        );
        let v6: SocketAddr = "[::1]:8080".parse().unwrap();
        assert_eq!(
            allowed_hosts(v6),
            vec!["[::1]:8080".to_string(), "localhost:8080".into()]
        );
        // On port 80 browsers send no port: both forms are this server.
        let lan: SocketAddr = "192.168.1.5:80".parse().unwrap();
        assert_eq!(
            allowed_hosts(lan),
            vec!["192.168.1.5:80".to_string(), "192.168.1.5".into()]
        );
        let local80: SocketAddr = "127.0.0.1:80".parse().unwrap();
        let hosts = allowed_hosts(local80);
        assert!(origin_allowed(
            Some("http://127.0.0.1"),
            Some("127.0.0.1"),
            &hosts,
            &[]
        ));
        assert!(origin_allowed(
            Some("http://localhost"),
            Some("localhost"),
            &hosts,
            &[]
        ));
    }

    #[test]
    fn every_interface_still_has_a_host_list() {
        let all: SocketAddr = "0.0.0.0:8080".parse().unwrap();
        let hosts = allowed_hosts(all);
        for host in ["127.0.0.1:8080", "[::1]:8080", "localhost:8080"] {
            assert!(hosts.iter().any(|h| h == host), "{host}: {hosts:?}");
        }
        for name in machine_names() {
            assert!(hosts.contains(&format!("{name}:8080")), "{hosts:?}");
        }
        // A rebound name is not on it, though its origin matches its host.
        assert!(!origin_allowed(
            Some("http://evil.example:8080"),
            Some("evil.example:8080"),
            &hosts,
            &[]
        ));
        // The address a connection arrived on is added for it, an IPv4
        // client of `::` included.
        let mut with_lan = hosts.clone();
        with_lan.extend(connection_hosts("10.0.0.2:8080".parse().unwrap()));
        assert!(origin_allowed(
            Some("http://10.0.0.2:8080"),
            Some("10.0.0.2:8080"),
            &with_lan,
            &[]
        ));
        assert!(!origin_allowed(
            Some("http://evil.example"),
            Some("10.0.0.2:8080"),
            &with_lan,
            &[]
        ));
        assert_eq!(
            connection_hosts("[::ffff:10.0.0.2]:8080".parse().unwrap()),
            ["10.0.0.2:8080"]
        );
        assert_eq!(
            connection_hosts("[fd00::2]:80".parse().unwrap()),
            ["[fd00::2]:80", "[fd00::2]"]
        );
    }

    #[test]
    fn allowed_hosts_take_the_port_unless_they_name_one() {
        assert_eq!(host_entries("Box.LAN", 8080), ["box.lan:8080"]);
        assert_eq!(host_entries("box.lan:9000", 8080), ["box.lan:9000"]);
        assert_eq!(host_entries("[fd00::2]", 8080), ["[fd00::2]:8080"]);
        assert_eq!(host_entries("[fd00::2]:9000", 8080), ["[fd00::2]:9000"]);
        assert_eq!(host_entries("box", 80), ["box:80", "box"]);
    }

    #[test]
    fn origins_are_checked() {
        let hosts = allowed_hosts("127.0.0.1:8080".parse().unwrap());
        let hosts = hosts.as_slice();
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
