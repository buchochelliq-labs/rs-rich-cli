//! Serve intuiTUIve apps to a web browser.
//!
//! [`serve`] listens on an address and runs one [`App`] per browser tab,
//! each on its own thread. The page is [xterm.js](https://xtermjs.org),
//! vendored in this crate and served by it; the app's frames go to it as
//! terminal output over a WebSocket, and its keys, mouse and resizes come
//! back as the same [`Event`](intuituive::interact::Event)s a terminal
//! gives. No async runtime, and nothing fetched from a CDN.
//!
//! The framework is re-exported as [`rich_web::intuituive`](intuituive), so
//! an app served this way needs no other dependency.
//!
//! ```no_run
//! use rich_web::intuituive::prelude::*;
//!
//! fn counter() -> App {
//!     App::new(|| {
//!         let count = signal(0);
//!         text!("[b]Count:[/] {count} (+ adds one)")
//!             .on_key("+", move |_| count.update(|c| *c += 1))
//!     })
//! }
//!
//! fn main() -> std::io::Result<()> {
//!     // Prints http://127.0.0.1:8080/?token=… to open in a browser.
//!     rich_web::serve("127.0.0.1:8080", counter)
//! }
//! ```
//!
//! [`Server`] is the same with options: a session cap, a fixed token, the
//! page's title, origins a proxy serves the page from, and
//! [`spawn`](Server::spawn) to run in the background.
//!
//! # Security
//!
//! Anyone who can open a session can do whatever the app lets them, so the
//! server is closed by default:
//!
//! - **It listens where it is told.** Give it a loopback address
//!   (`127.0.0.1`, `[::1]`, `localhost`) to keep it on this computer, as
//!   the examples do. Any other address prints a warning.
//! - **A random token** (128 bits from the operating system) is part of the
//!   URL printed at start; the page and the WebSocket both refuse a request
//!   without it.
//! - **The `Origin` header is checked**: browsers let any web page open a
//!   WebSocket to a localhost port, so the upgrade must come from this
//!   server's own page (its address, which must also be the `Host` asked
//!   for, against DNS rebinding) or from an origin allowed with
//!   [`Server::allow_origin`].
//! - **Sessions are capped** ([`DEFAULT_MAX_SESSIONS`] unless set with
//!   [`Server::max_sessions`]); one more is refused with `503`.
//!
//! There is no other authentication and no TLS. To reach the server from
//! another machine, put it behind a reverse proxy that has both, and allow
//! the proxy's origin.

// The framework, under the name its own crate gives it (the workspace's
// dependency key makes it `rich_intuituive`), and re-exported, so an app needs
// only this crate.
pub extern crate rich_intuituive as intuituive;

use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use intuituive::App;
use tungstenite::protocol::{Role, WebSocketConfig};
use tungstenite::WebSocket;

mod http;
pub mod input;
mod session;

use http::Head;

/// The version of xterm.js the page uses, vendored in this crate.
pub const XTERM_VERSION: &str = "6.0.0";

/// The sessions a server runs at once unless told otherwise.
pub const DEFAULT_MAX_SESSIONS: usize = 8;

/// Connections still sending their request at once; more are closed.
const MAX_PENDING: usize = 64;
/// How long a connection may take to send its request.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a write to a browser may take before the session ends.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// The largest message a page may send (a big paste).
const MAX_MESSAGE: usize = 1 << 20;

const PAGE: &str = include_str!("../assets/index.html");
const PAGE_JS: &str = include_str!("../assets/app.js");
const XTERM_JS: &str = include_str!("../assets/xterm/xterm.js");
const XTERM_CSS: &str = include_str!("../assets/xterm/xterm.css");
const FIT_JS: &str = include_str!("../assets/xterm/addon-fit.js");

/// Serve `app` at `addr`: print the URL to open (with its token), then run
/// one app, made by `app`, per browser tab that opens it, until the process
/// ends. [`Server::bind`] and [`Server::run`] with the defaults.
///
/// `app` is called on each session's own thread, so the [`App`] itself
/// never crosses threads.
pub fn serve<A, F>(addr: A, app: F) -> io::Result<()>
where
    A: ToSocketAddrs,
    F: Fn() -> App + Send + Sync + 'static,
{
    Server::bind(addr, app)?.run()
}

/// A server that is bound and ready to [`run`](Self::run) or
/// [`spawn`](Self::spawn), with its options.
pub struct Server {
    listener: TcpListener,
    shared: Shared,
}

/// What every connection's thread reads.
struct Shared {
    app: Box<dyn Fn() -> App + Send + Sync>,
    token: String,
    max_sessions: usize,
    origins: Vec<String>,
    title: String,
    /// The `Host` values the server answers to (`None`: any).
    hosts: Option<Vec<String>>,
    /// Sessions running.
    sessions: AtomicUsize,
    /// Connections still sending their request.
    pending: AtomicUsize,
    stopping: AtomicBool,
}

impl Server {
    /// Listen on `addr` (`"127.0.0.1:0"` picks a free port) with a fresh
    /// random token, to serve the apps `app` makes.
    pub fn bind<A, F>(addr: A, app: F) -> io::Result<Server>
    where
        A: ToSocketAddrs,
        F: Fn() -> App + Send + Sync + 'static,
    {
        let listener = TcpListener::bind(addr)?;
        let hosts = http::allowed_hosts(listener.local_addr()?);
        Ok(Server {
            listener,
            shared: Shared {
                app: Box::new(app),
                token: random_token()?,
                max_sessions: DEFAULT_MAX_SESSIONS,
                origins: Vec::new(),
                title: "intuiTUIve".to_string(),
                hosts,
                sessions: AtomicUsize::new(0),
                pending: AtomicUsize::new(0),
                stopping: AtomicBool::new(false),
            },
        })
    }

    /// Run at most `n` sessions at once (8 by default); a browser that
    /// opens one more is refused until one ends.
    pub fn max_sessions(mut self, n: usize) -> Server {
        self.shared.max_sessions = n;
        self
    }

    /// Use `token` instead of a random one, for an address that stays the
    /// same between runs. Keep it as secret, and as long: anyone with it
    /// can use the app.
    ///
    /// # Panics
    /// When `token` is empty or has characters other than ASCII letters,
    /// digits, `-`, `_`, `.` and `~` (it goes in a URL as it is).
    pub fn token(mut self, token: impl Into<String>) -> Server {
        let token = token.into();
        assert!(
            !token.is_empty()
                && token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.~".contains(&b)),
            "a token is ASCII letters, digits, '-', '_', '.' and '~'"
        );
        self.shared.token = token;
        self
    }

    /// Also accept WebSocket upgrades from pages served at `origin` (say
    /// `"https://term.example.com"`): the address a reverse proxy serves
    /// this server's page from. The token is still required.
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Server {
        self.shared.origins.push(origin.into());
        self
    }

    /// The page's title (`intuiTUIve` by default).
    pub fn title(mut self, title: impl Into<String>) -> Server {
        self.shared.title = title.into();
        self
    }

    /// The address the server listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("a bound listener has an address")
    }

    /// The token browsers must give.
    pub fn access_token(&self) -> &str {
        &self.shared.token
    }

    /// The address to open in a browser, with the token.
    pub fn url(&self) -> String {
        url_for(self.local_addr(), &self.shared.token)
    }

    /// Print the URL (and a warning when the address is not loopback),
    /// then serve until the process ends.
    pub fn run(self) -> io::Result<()> {
        let local = self.local_addr();
        let mut out = io::stdout().lock();
        writeln!(out, "Serving {} at {}", self.shared.title, self.url())?;
        out.flush()?;
        drop(out);
        if !local.ip().is_loopback() {
            eprintln!(
                "warning: listening on {local}, beyond this computer. Anyone who can reach it \
                 with the token can use the app: there is no other authentication and no TLS. \
                 Put it behind a reverse proxy that has both."
            );
        }
        accept(self.listener, Arc::new(self.shared));
        Ok(())
    }

    /// Serve on a thread of its own, printing nothing, until the
    /// [`Handle`] is stopped or dropped.
    pub fn spawn(self) -> io::Result<Handle> {
        let local = self.local_addr();
        let url = self.url();
        let shared = Arc::new(self.shared);
        let listener = self.listener;
        let thread = {
            let shared = shared.clone();
            thread::Builder::new()
                .name("rich-web accept".into())
                .spawn(move || accept(listener, shared))?
        };
        Ok(Handle {
            local,
            url,
            shared,
            thread: Some(thread),
        })
    }
}

/// A server running in the background, from [`Server::spawn`]. Stopping it
/// (or dropping it) stops accepting connections and ends every session.
pub struct Handle {
    local: SocketAddr,
    url: String,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Handle {
    /// The address the server listens on.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// The address to open in a browser, with the token.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The sessions running now.
    pub fn sessions(&self) -> usize {
        self.shared.sessions.load(Ordering::SeqCst)
    }

    /// Stop: no more connections, and every session ends (within one turn
    /// of its loop, at most 50 ms).
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            // Wake the accept loop, which then sees `stopping`.
            let _ = TcpStream::connect_timeout(&reachable(self.local), Duration::from_secs(1));
            let _ = thread.join();
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The address to connect to for `local`: loopback when it is every
/// interface.
fn reachable(local: SocketAddr) -> SocketAddr {
    let ip = match local.ip() {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, local.port())
}

fn url_for(local: SocketAddr, token: &str) -> String {
    format!("http://{}/?token={token}", reachable(local))
}

/// 128 bits from the operating system's random source, as hex.
fn random_token() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| io::Error::other(format!("no random token: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn accept(listener: TcpListener, shared: Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else {
            // Out of file descriptors, say: let some go before the next.
            thread::sleep(Duration::from_millis(10));
            continue;
        };
        if shared.pending.fetch_add(1, Ordering::SeqCst) >= MAX_PENDING {
            shared.pending.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let pending = Pending(shared.clone());
        let spawned = thread::Builder::new()
            .name("rich-web connection".into())
            .spawn(move || connection(stream, pending));
        if spawned.is_err() {
            // The closure, and the `Pending` in it, were dropped.
            continue;
        }
    }
}

/// A connection still sending its request: counted until dropped.
struct Pending(Arc<Shared>);

impl Drop for Pending {
    fn drop(&mut self) {
        self.0.pending.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A running session: counted until dropped.
struct Slot(Arc<Shared>);

impl Slot {
    fn acquire(shared: &Arc<Shared>) -> Option<Slot> {
        let mut n = shared.sessions.load(Ordering::SeqCst);
        loop {
            if n >= shared.max_sessions {
                return None;
            }
            match shared
                .sessions
                .compare_exchange(n, n + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return Some(Slot(shared.clone())),
                Err(now) => n = now,
            }
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.sessions.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Serve one connection: the page, an asset, or a session.
fn connection(mut stream: TcpStream, pending: Pending) {
    let _ = stream.set_nodelay(true);
    if stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err()
    {
        return;
    }
    let Ok(bytes) = http::read_head(&mut stream) else {
        return;
    };
    let Some(head) = Head::parse(&bytes) else {
        let _ = http::refuse(&mut stream, "400 Bad Request", "Bad request");
        return;
    };
    let shared = pending.0.clone();
    if head.method != "GET" {
        let _ = http::refuse(&mut stream, "405 Method Not Allowed", "Only GET");
        return;
    }
    let js = "text/javascript; charset=utf-8";
    let asset = match head.path.as_str() {
        "/ws" => {
            websocket(stream, &head, &shared, pending);
            return;
        }
        "/" | "/index.html" => {
            page(&mut stream, &head, &shared);
            return;
        }
        "/app.js" => (js, PAGE_JS),
        "/xterm.js" => (js, XTERM_JS),
        "/addon-fit.js" => (js, FIT_JS),
        "/xterm.css" => ("text/css; charset=utf-8", XTERM_CSS),
        _ => {
            let _ = http::refuse(&mut stream, "404 Not Found", "Not found");
            return;
        }
    };
    let _ = http::respond(&mut stream, "200 OK", asset.0, &[], asset.1.as_bytes());
}

/// The page, for a request with the token.
fn page(stream: &mut TcpStream, head: &Head, shared: &Shared) {
    if !head
        .param("token")
        .is_some_and(|t| http::same_token(t, &shared.token))
    {
        let _ = http::refuse(
            stream,
            "403 Forbidden",
            "Open the address the server printed when it started, with its token.",
        );
        return;
    }
    // The page loads only its own scripts and styles, opens a WebSocket
    // only to this server, and is never framed by another site.
    let mut connect = "'self'".to_string();
    if let Some(host) = head.header("host").filter(|h| {
        h.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
    }) {
        connect.push_str(&format!(" ws://{host} wss://{host}"));
    }
    let policy = format!(
        "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; font-src 'self' data:; connect-src {connect}; \
         frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
    );
    let body = PAGE.replace("{{title}}", &http::escape_html(&shared.title));
    let _ = http::respond(
        stream,
        "200 OK",
        "text/html; charset=utf-8",
        &[
            ("Content-Security-Policy", policy),
            ("X-Frame-Options", "DENY".to_string()),
        ],
        body.as_bytes(),
    );
}

/// Why an upgrade is refused, as a status and a message; `None` when it
/// may go ahead.
fn check_upgrade(head: &Head, shared: &Shared) -> Option<(&'static str, &'static str)> {
    if http::websocket_key(head).is_none() {
        return Some(("400 Bad Request", "Expected a WebSocket upgrade."));
    }
    if !head
        .param("token")
        .is_some_and(|t| http::same_token(t, &shared.token))
    {
        return Some(("403 Forbidden", "Wrong or missing token."));
    }
    if !http::origin_allowed(
        head.header("origin"),
        head.header("host"),
        shared.hosts.as_deref(),
        &shared.origins,
    ) {
        return Some(("403 Forbidden", "This origin may not open a session."));
    }
    None
}

/// Upgrade to a WebSocket and run a session on it, or refuse.
fn websocket(mut stream: TcpStream, head: &Head, shared: &Arc<Shared>, pending: Pending) {
    if let Some((status, why)) = check_upgrade(head, shared) {
        let _ = http::refuse(&mut stream, status, why);
        return;
    }
    let Some(_slot) = Slot::acquire(shared) else {
        let _ = http::refuse(
            &mut stream,
            "503 Service Unavailable",
            "Too many sessions: try again when one has ended.",
        );
        return;
    };
    let key = http::websocket_key(head).expect("checked");
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let answer = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    if stream.write_all(answer.as_bytes()).is_err() || stream.flush().is_err() {
        return;
    }
    drop(pending);
    let size = |name, default| {
        head.param(name)
            .and_then(|n| n.parse::<u16>().ok())
            .filter(|n| (1..=session::MAX_CELLS).contains(n))
            .unwrap_or(default)
    };
    let (columns, rows) = (size("cols", 80), size("rows", 24));
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let mut socket = WebSocket::from_raw_socket(stream, Role::Server, Some(config));
    let app = (shared.app)();
    let _ = session::run(&mut socket, app, columns, rows, &shared.stopping);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_hex() {
        let a = random_token().unwrap();
        let b = random_token().unwrap();
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn urls_point_somewhere_reachable() {
        let token = "t0k";
        assert_eq!(
            url_for("127.0.0.1:8080".parse().unwrap(), token),
            "http://127.0.0.1:8080/?token=t0k"
        );
        assert_eq!(
            url_for("0.0.0.0:80".parse().unwrap(), token),
            "http://127.0.0.1:80/?token=t0k"
        );
        assert_eq!(
            url_for("[::]:80".parse().unwrap(), token),
            "http://[::1]:80/?token=t0k"
        );
    }

    #[test]
    #[should_panic(expected = "a token is")]
    fn tokens_must_be_url_safe() {
        let _ = Server::bind("127.0.0.1:0", || App::new(|| intuituive::label("x")))
            .unwrap()
            .token("has space");
    }
}
