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
//! Browsh is a program the user installs; nothing is downloaded.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
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
    /// page, and ask it at [`DEFAULT_SERVER`].
    pub fn new() -> BrowshEngine {
        BrowshEngine {
            server: DEFAULT_SERVER.to_string(),
            program: Some("browsh".to_string()),
            child: None,
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
        // Its port is Browsh's own setting (`http-server.port` in its
        // config, 4333 by default); for another, start it yourself and
        // `connect`.
        let child = Command::new(program)
            .arg("--http-server-mode")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("could not start {program} (install Browsh, or connect to a running server): {error}"),
                )
            })?;
        self.child = Some(child);
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
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The request for `url`: the page as plain text.
pub(crate) fn request(server: &str, url: &str) -> String {
    format!("GET /{url} HTTP/1.0\r\nHost: {server}\r\nX-Browsh-Raw-Mode: PLAIN\r\n\r\n")
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
