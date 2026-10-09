//! [`BrowshEngine`]: pages as text from Browsh's HTTP server mode, for
//! read-only views.
//!
//! `browsh --http-server-mode` renders a page in its headless Firefox and
//! answers `GET /<url>` with the page as text (plain text with the
//! `X-Browsh-Raw-Mode: PLAIN` header). The engine asks over a plain socket
//! on its own thread, shows the text, and scrolls it with the keys and the
//! wheel; links and forms are not followed (that is Browsh's own terminal
//! mode, which [`ProgramEngine`](crate::ProgramEngine) runs).
//!
//! Browsh is a program the user installs; nothing is downloaded. The
//! engine starts it with a configuration of its own, in a temporary
//! directory removed when the engine goes, that binds its server to
//! `127.0.0.1` (Browsh's own default is every interface: anyone who could
//! reach the machine could have it fetch pages); the user's own Browsh
//! configuration is not read. To use that, start Browsh yourself and
//! [`connect`](BrowshEngine::connect).

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use rich::Segment;
use rich_intuituive::interact::{KeyCode, MouseKind};

use crate::host::Notify;
use crate::web::{PageState, WebEngine, WebFrame, WebInput};

/// Where Browsh's HTTP server listens unless told otherwise.
pub const DEFAULT_SERVER: &str = "127.0.0.1:4333";
/// How long a page may take, including Browsh starting its browser.
const FETCH_TIMEOUT: Duration = Duration::from_secs(90);
/// The variable Browsh finds its configuration directory by, and where in
/// it its own directory is.
const CONFIG_HOME: (&str, &str) = if cfg!(windows) {
    ("APPDATA", "browsh")
} else if cfg!(target_os = "macos") {
    ("HOME", "Library/Application Support/browsh")
} else {
    ("XDG_CONFIG_HOME", "browsh")
};
/// The engine's configuration for Browsh: its HTTP server on this
/// computer only. Browsh reads its sample configuration first, so the rest
/// keeps Browsh's defaults.
const CONFIG: &str = "# Written by rs-rich-embed's BrowshEngine.\n\
                      [http-server]\nport = 4333\nbind = \"127.0.0.1\"\n";

#[derive(Default)]
struct Shared {
    /// The newest page's lines, until shown.
    page: Option<Result<Vec<String>, String>>,
    /// Which fetch is current: older ones' answers are dropped.
    generation: u64,
    notify: Option<Notify>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Pages as text from Browsh's HTTP server mode.
pub struct BrowshEngine {
    server: String,
    /// The program to start the server with; `None`: one is running.
    program: Option<String>,
    child: Option<Child>,
    /// The temporary configuration directory the server was started with.
    config: Option<PathBuf>,
    shared: Arc<Mutex<Shared>>,
    lines: Vec<String>,
    scroll: usize,
    size: (u16, u16),
    history: Vec<String>,
    at: usize,
    loading: bool,
    error: Option<String>,
    /// The view needs a new frame.
    dirty: bool,
}

impl Default for BrowshEngine {
    fn default() -> Self {
        BrowshEngine::new()
    }
}

impl BrowshEngine {
    /// Start `browsh --http-server-mode` (from `PATH`) with the first
    /// page, and ask it at [`DEFAULT_SERVER`]. It is started with the
    /// engine's own configuration, which binds it to `127.0.0.1` only, and
    /// ended, with the Firefox it started, when the engine goes.
    pub fn new() -> BrowshEngine {
        BrowshEngine {
            server: DEFAULT_SERVER.to_string(),
            program: Some("browsh".to_string()),
            child: None,
            config: None,
            shared: Arc::default(),
            lines: Vec::new(),
            scroll: 0,
            size: (80, 24),
            history: Vec::new(),
            at: 0,
            loading: false,
            error: None,
            dirty: false,
        }
    }

    /// Ask a Browsh server already running at `address` (`host:port`);
    /// none is started.
    pub fn connect(address: impl Into<String>) -> BrowshEngine {
        let mut engine = BrowshEngine::new();
        engine.server = address.into();
        engine.program = None;
        engine
    }

    /// Start the server with `program` instead of `browsh` from `PATH`.
    pub fn program(mut self, program: impl Into<String>) -> BrowshEngine {
        self.program = Some(program.into());
        self
    }

    fn start_server(&mut self) -> io::Result<()> {
        let Some(program) = &self.program else {
            return Ok(());
        };
        if self.child.is_some() {
            return Ok(());
        }
        // Its address is Browsh's own setting (`http-server` in its
        // config): the engine's own config says 127.0.0.1:4333. For
        // another, start it yourself and `connect`.
        let config = temporary_config()?;
        let mut command = Command::new(program);
        command
            .arg("--http-server-mode")
            .env(CONFIG_HOME.0, &config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // A group of its own, with the Firefox it starts, to end together.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let child = command.spawn().map_err(|error| {
            let _ = std::fs::remove_dir_all(&config);
            io::Error::new(
                error.kind(),
                format!("could not start {program} (install Browsh, or connect to a running server): {error}"),
            )
        })?;
        self.child = Some(child);
        self.config = Some(config);
        Ok(())
    }

    /// Fetch the history's current page on a thread of its own.
    fn fetch(&mut self) -> io::Result<()> {
        let Some(url) = self.history.get(self.at).cloned() else {
            return Ok(());
        };
        if let Err(error) = self.start_server() {
            self.error = Some(error.to_string());
            self.dirty = true;
            return Err(error);
        }
        self.loading = true;
        self.error = None;
        let generation = {
            let mut shared = lock(&self.shared);
            shared.generation += 1;
            shared.generation
        };
        let shared = Arc::clone(&self.shared);
        let server = self.server.clone();
        thread::spawn(move || {
            let result = fetch_page(&server, &url);
            let notify = {
                let mut shared = lock(&shared);
                if shared.generation != generation {
                    return;
                }
                shared.page = Some(result.map(|text| page_lines(&text)));
                shared.notify.clone()
            };
            if let Some(notify) = notify {
                notify();
            }
        });
        Ok(())
    }

    fn scroll_by(&mut self, rows: isize) {
        let most = self.lines.len().saturating_sub(self.size.1 as usize);
        let to = (self.scroll as isize + rows).clamp(0, most as isize) as usize;
        if to != self.scroll {
            self.scroll = to;
            self.dirty = true;
        }
    }
}

impl Drop for BrowshEngine {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            kill_group(&child);
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(config) = &self.config {
            let _ = std::fs::remove_dir_all(config);
        }
    }
}

/// Kill the process group Browsh leads (it and the Firefox it started),
/// with SIGKILL.
#[cfg(unix)]
#[allow(unsafe_code)]
fn kill_group(child: &Child) {
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        // SAFETY: `killpg` takes plain integers. Browsh has not been
        // reaped, so its id still names the group it leads.
        unsafe {
            libc::killpg(group, libc::SIGKILL);
        }
    }
}

/// Elsewhere only Browsh itself is ended.
#[cfg(not(unix))]
fn kill_group(_child: &Child) {}

/// A new directory holding the engine's configuration for Browsh, to set
/// as its configuration home.
fn temporary_config() -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    loop {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "rich-embed-browsh-{}-{n}-{nanos}",
            std::process::id()
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => {
                let written = write_config(&dir);
                if written.is_err() {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                return written.map(|()| dir);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn write_config(home: &Path) -> io::Result<()> {
    let dir = home.join(CONFIG_HOME.1);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("config.toml"), CONFIG)
}

/// The request for `url`: the page as plain text. Spaces and control
/// characters in the address are percent-encoded, so that it stays one
/// request line.
pub(crate) fn request(server: &str, url: &str) -> String {
    let mut target = String::with_capacity(url.len());
    for c in url.chars() {
        if c == ' ' || c.is_control() {
            let mut bytes = [0u8; 4];
            for byte in c.encode_utf8(&mut bytes).bytes() {
                target.push_str(&format!("%{byte:02X}"));
            }
        } else {
            target.push(c);
        }
    }
    format!("GET /{target} HTTP/1.0\r\nHost: {server}\r\nX-Browsh-Raw-Mode: PLAIN\r\n\r\n")
}

/// Ask the server for `url`, retrying while it starts.
fn fetch_page(server: &str, url: &str) -> Result<String, String> {
    let start = Instant::now();
    let mut stream = loop {
        match TcpStream::connect(server) {
            Ok(stream) => break stream,
            Err(error) if start.elapsed() > FETCH_TIMEOUT => {
                return Err(format!("Browsh is not answering at {server}: {error}"));
            }
            Err(_) => thread::sleep(Duration::from_millis(250)),
        }
    };
    let _ = stream.set_read_timeout(Some(FETCH_TIMEOUT));
    stream
        .write_all(request(server, url).as_bytes())
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    parse_response(&response)
}

/// The body of an HTTP response, or why there is none.
pub(crate) fn parse_response(response: &[u8]) -> Result<String, String> {
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("an incomplete answer from Browsh")?;
    let head = String::from_utf8_lossy(&response[..split]);
    let body = &response[split + 4..];
    let mut lines = head.lines();
    let status = lines.next().unwrap_or_default();
    let code: u16 = status
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("not an HTTP answer: {status}"))?;
    let chunked = lines.any(|line| {
        let line = line.to_ascii_lowercase();
        line.starts_with("transfer-encoding:") && line.contains("chunked")
    });
    let body = if chunked {
        dechunk(body)
    } else {
        body.to_vec()
    };
    let text = String::from_utf8_lossy(&body).into_owned();
    if (200..300).contains(&code) {
        Ok(text)
    } else {
        Err(format!("Browsh answered {code}: {}", text.trim()))
    }
}

/// A chunked body, joined.
fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(end) = body.windows(2).position(|w| w == b"\r\n") {
        let size = String::from_utf8_lossy(&body[..end]);
        let size = size.split(';').next().unwrap_or("0").trim();
        let Ok(size) = usize::from_str_radix(size, 16) else {
            break;
        };
        body = &body[end + 2..];
        if size == 0 || body.len() < size {
            out.extend_from_slice(&body[..size.min(body.len())]);
            break;
        }
        out.extend_from_slice(&body[..size]);
        body = body.get(size + 2..).unwrap_or_default();
    }
    out
}

/// A page's text as lines, with tabs and controls made harmless.
fn page_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            line.chars()
                .map(|c| if c == '\t' { ' ' } else { c })
                .filter(|c| !c.is_control())
                .collect()
        })
        .collect()
}

impl WebEngine for BrowshEngine {
    fn open(&mut self, url: &str) -> io::Result<()> {
        if !self.history.is_empty() {
            self.history.truncate(self.at + 1);
        }
        self.history.push(url.to_string());
        self.at = self.history.len() - 1;
        self.fetch()
    }

    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        self.size = (columns, rows);
        self.scroll_by(0);
        self.dirty = true;
        Ok(())
    }

    fn input(&mut self, input: WebInput) -> io::Result<()> {
        let page = self.size.1.max(1) as isize;
        match input {
            WebInput::Key(key) => match key.code {
                KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
                KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_by(page),
                KeyCode::PageUp => self.scroll_by(-page),
                KeyCode::Home | KeyCode::Char('g') => self.scroll_by(isize::MIN / 2),
                KeyCode::End | KeyCode::Char('G') => self.scroll_by(isize::MAX / 2),
                _ => {}
            },
            WebInput::Mouse(mouse) => match mouse.kind {
                MouseKind::ScrollDown => self.scroll_by(3),
                MouseKind::ScrollUp => self.scroll_by(-3),
                _ => {}
            },
            WebInput::Paste(_) => {}
        }
        Ok(())
    }

    fn back(&mut self) -> io::Result<()> {
        if self.at == 0 {
            return Ok(());
        }
        self.at -= 1;
        self.fetch()
    }

    fn forward(&mut self) -> io::Result<()> {
        if self.at + 1 >= self.history.len() {
            return Ok(());
        }
        self.at += 1;
        self.fetch()
    }

    fn reload(&mut self) -> io::Result<()> {
        self.fetch()
    }

    fn poll(&mut self) -> Option<WebFrame> {
        if let Some(page) = lock(&self.shared).page.take() {
            self.loading = false;
            match page {
                Ok(lines) => {
                    self.lines = lines;
                    self.scroll = 0;
                }
                Err(error) => self.error = Some(error),
            }
            self.dirty = true;
        }
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        let lines = self
            .lines
            .iter()
            .skip(self.scroll)
            .take(self.size.1 as usize)
            .map(|line| vec![Segment::new(line.clone(), None)])
            .collect();
        Some(WebFrame::Cells {
            lines,
            cursor: None,
        })
    }

    fn state(&self) -> PageState {
        PageState {
            url: self.history.get(self.at).cloned().unwrap_or_default(),
            title: String::new(),
            loading: self.loading,
            can_go_back: self.at > 0,
            can_go_forward: self.at + 1 < self.history.len(),
            error: self.error.clone(),
        }
    }

    fn set_notify(&mut self, notify: Notify) {
        lock(&self.shared).notify = Some(notify);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich_intuituive::interact::Key;

    #[test]
    fn a_page_is_asked_for_as_plain_text() {
        assert_eq!(
            request("127.0.0.1:4333", "https://example.com"),
            "GET /https://example.com HTTP/1.0\r\nHost: 127.0.0.1:4333\r\n\
             X-Browsh-Raw-Mode: PLAIN\r\n\r\n"
        );
    }

    #[test]
    fn an_address_stays_one_request_line() {
        assert_eq!(
            request("127.0.0.1:4333", "https://a.example/x y\r\nX-Evil: 1\u{85}"),
            "GET /https://a.example/x%20y%0D%0AX-Evil:%201%C2%85 HTTP/1.0\r\n\
             Host: 127.0.0.1:4333\r\nX-Browsh-Raw-Mode: PLAIN\r\n\r\n"
        );
    }

    #[test]
    fn answers_are_read_plain_or_chunked() {
        let plain = b"HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nHello\nworld\n";
        assert_eq!(parse_response(plain).unwrap(), "Hello\nworld\n");
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nHello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap(), "Hello world");
        let failed = b"HTTP/1.0 500 Internal Server Error\r\n\r\nno browser\n";
        assert_eq!(
            parse_response(failed).unwrap_err(),
            "Browsh answered 500: no browser"
        );
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn the_page_scrolls_with_the_keys() {
        let mut engine = BrowshEngine::connect("127.0.0.1:9");
        engine.resize(20, 2).unwrap();
        lock(&engine.shared).page = Some(Ok(page_lines("one\ntwo\tx\nthree\nfour\x07")));
        let text = |frame: Option<WebFrame>| match frame {
            Some(WebFrame::Cells { lines, .. }) => lines
                .iter()
                .map(|line| line[0].text.clone())
                .collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        assert_eq!(text(engine.poll()), ["one", "two x"]);
        assert!(engine.poll().is_none());
        engine
            .input(WebInput::Key(Key::parse("pagedown").unwrap()))
            .unwrap();
        assert_eq!(text(engine.poll()), ["three", "four"]);
        engine.input(WebInput::Key(Key::char('g'))).unwrap();
        assert_eq!(text(engine.poll()), ["one", "two x"]);
    }
}
