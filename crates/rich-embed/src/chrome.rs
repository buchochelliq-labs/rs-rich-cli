//! [`ChromeEngine`]: headless Chrome or Chromium, which the user installs,
//! driven over the DevTools protocol.
//!
//! The protocol subset is spoken directly, on one thread of the engine's
//! own, so no async runtime comes with it:
//!
//! - on Linux and macOS, over a pipe: the browser is started with
//!   `--remote-debugging-pipe` and reads DevTools messages from descriptor 3
//!   and writes them to descriptor 4, each a JSON text ended by a NUL byte.
//!   No port is opened. The descriptors are passed with `command-fds`;
//! - on Windows, over a WebSocket (`tungstenite`) on a random port of
//!   `127.0.0.1` (`--remote-debugging-port=0`), whose address the browser
//!   prints on its standard error. See [`ChromeEngine`]'s security note;
//! - the browser is started headless with a temporary profile directory
//!   (only this user's, removed when the engine goes), its sandbox on (no
//!   switch that turns any of it off, ever), and downloads denied
//!   (`Browser.setDownloadBehavior`);
//! - a page target is created and attached in flat mode;
//! - frames come from `Page.startScreencast` (JPEG, each acknowledged),
//!   at the page's size in cells times [`ChromeEngine::cell_pixels`];
//! - keys, the mouse and pastes go in as `Input.dispatchKeyEvent`,
//!   `Input.dispatchMouseEvent` and `Input.insertText`;
//! - the address, loading state and history come from `Page` events and
//!   `Page.getNavigationHistory`.
//!
//! The browser is found at an explicit path, else by the usual names on
//! `PATH` ([`BINARY_NAMES`]). Nothing is downloaded.

use std::io::{BufRead, BufReader, ErrorKind};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use std::{fmt, io};

use rich_intuituive::interact::{Button, Key, KeyCode, ModifierKey, Modifiers, Mouse, MouseKind};
use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

use crate::host::Notify;
use crate::pixels::Pixels;
use crate::web::{PageState, WebEngine, WebFrame, WebInput};

/// Names the browser is looked for under on `PATH`, in order.
pub const BINARY_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
    "headless_shell",
    "microsoft-edge",
];

/// Where browsers install themselves outside `PATH`.
const KNOWN_PATHS: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

/// How the engine reaches the browser's DevTools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wire {
    /// `--remote-debugging-pipe`: descriptors 3 and 4, no port.
    #[cfg(unix)]
    Pipe,
    /// `--remote-debugging-port=0`: a WebSocket on a port of `127.0.0.1`.
    /// Windows's; on Unix only the tests choose it, to keep it exercised.
    #[cfg_attr(all(unix, not(test)), allow(dead_code))]
    Port,
}

impl Wire {
    /// The pipe on Linux and macOS; the port on Windows, where the browser
    /// takes the pipe only as two inheritable handles, which nothing here
    /// can pass without `unsafe`.
    #[cfg(unix)]
    const DEFAULT: Wire = Wire::Pipe;
    #[cfg(not(unix))]
    const DEFAULT: Wire = Wire::Port;

    /// The browser's switch for it.
    fn switch(self) -> &'static str {
        match self {
            #[cfg(unix)]
            Wire::Pipe => "--remote-debugging-pipe",
            Wire::Port => "--remote-debugging-port=0",
        }
    }
}

/// How long the browser has to say where its DevTools endpoint is.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a read of the socket waits before the thread looks at what the
/// app asked for.
const POLL: Duration = Duration::from_millis(15);

/// What the app asked the browser to do, for the engine's thread.
#[derive(Clone, Debug, PartialEq)]
enum Request {
    Open(String),
    Resize(u16, u16),
    Input(WebInput),
    Back,
    Forward,
    Reload,
}

#[derive(Default)]
struct Shared {
    frame: Option<Pixels>,
    state: PageState,
    notify: Option<Notify>,
    /// The browser process, so the engine can end it from any thread.
    child: Option<Child>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

fn notify(shared: &Mutex<Shared>) {
    let notify = lock(shared).notify.clone();
    if let Some(notify) = notify {
        notify();
    }
}

/// A headless Chrome or Chromium behind a web view. Frames are pixels,
/// drawn as half blocks.
///
/// # Security
///
/// On Linux and macOS the engine drives the browser over DevTools on a
/// pipe only the two processes hold: no port is opened, and nothing else on
/// the machine can reach it.
///
/// On Windows it uses a random TCP port of `127.0.0.1`. While the view is
/// open, that port can be reached by other programs and other users on this
/// machine, and DevTools has no authentication of its own: whoever finds it
/// can drive the browser as this user (open local files, run script in
/// pages). On Windows, use this engine on a machine you do not share.
///
/// Everywhere, the profile directory is readable by this user only.
pub struct ChromeEngine {
    binary: Option<PathBuf>,
    args: Vec<String>,
    cell: (u32, u32),
    shared: Arc<Mutex<Shared>>,
    commands: Option<Sender<Request>>,
    profile: Option<PathBuf>,
    wire: Wire,
}

impl fmt::Debug for ChromeEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChromeEngine")
            .field("binary", &self.binary)
            .field("cell", &self.cell)
            .finish()
    }
}

impl Default for ChromeEngine {
    fn default() -> Self {
        ChromeEngine::new()
    }
}

impl ChromeEngine {
    /// The browser found by [`find`](Self::find) when it starts.
    pub fn new() -> ChromeEngine {
        ChromeEngine {
            binary: None,
            args: Vec::new(),
            cell: (10, 20),
            shared: Arc::default(),
            commands: None,
            profile: None,
            wire: Wire::DEFAULT,
        }
    }

    /// Run the browser at `path`.
    #[must_use]
    pub fn binary(mut self, path: impl Into<PathBuf>) -> ChromeEngine {
        self.binary = Some(path.into());
        self
    }

    /// More command-line switches for the browser. The sandbox stays on
    /// whatever they say: `--no-sandbox`, and every other switch that turns
    /// part of it off, is dropped, however it is spelled. So is every
    /// `--remote-debugging-…` switch: the engine says where DevTools goes.
    #[must_use]
    pub fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, args: I) -> ChromeEngine {
        self.args.extend(
            args.into_iter()
                .map(Into::into)
                .filter(|arg| !turns_sandbox_off(arg) && !moves_devtools(arg)),
        );
        self
    }

    /// The CSS pixels one cell of the pane stands for (default 10 x 20):
    /// the page is laid out at the pane's size times this.
    #[must_use]
    pub fn cell_pixels(mut self, width: u32, height: u32) -> ChromeEngine {
        self.cell = (width.max(1), height.max(1));
        self
    }

    /// The first browser on `PATH` by [`BINARY_NAMES`], or in a usual
    /// install location.
    pub fn find() -> Option<PathBuf> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        for name in BINARY_NAMES {
            for dir in std::env::split_paths(&path) {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
                let exe = candidate.with_extension("exe");
                if cfg!(windows) && exe.is_file() {
                    return Some(exe);
                }
            }
        }
        KNOWN_PATHS
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
    }

    fn fail(&self, message: impl Into<String>) {
        lock(&self.shared).state.error = Some(message.into());
        notify(&self.shared);
    }

    /// Start the browser and the engine's thread.
    fn start(&mut self) -> io::Result<()> {
        let Some(binary) = self.binary.clone().or_else(ChromeEngine::find) else {
            let message = format!(
                "no Chrome or Chromium found: install one, or name it with \
                 ChromeEngine::binary (looked for {})",
                BINARY_NAMES.join(", ")
            );
            self.fail(message.clone());
            return Err(io::Error::new(ErrorKind::NotFound, message));
        };
        let profile = temporary_profile()?;
        // The window's first size; the page is sized to the pane once
        // attached.
        let mut command = Command::new(&binary);
        command
            .args(launch_args(
                &profile, &self.args, 80, 24, self.cell, self.wire,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let spawned = connect(&mut command, self.wire).and_then(|connect| {
            let child = command.spawn()?;
            Ok((child, connect))
        });
        // Our copies of the browser's ends of the pipe close with the
        // command, so its exit reads as the end of the pipe.
        drop(command);
        // The profile is the engine's to remove only once Chrome runs in
        // it; a failed start removes it here, so retries leave none behind.
        let (mut child, connect) = match spawned {
            Ok(spawned) => spawned,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&profile);
                return Err(error);
            }
        };
        self.profile = Some(profile.clone());
        let stderr = child.stderr.take().expect("piped stderr");
        lock(&self.shared).child = Some(child);
        let (sender, receiver) = mpsc::channel();
        self.commands = Some(sender);
        let shared = Arc::clone(&self.shared);
        let cell = self.cell;
        thread::spawn(move || {
            if let Err(error) = run(connect, stderr, receiver, &shared, cell) {
                let mut state = lock(&shared);
                state.state.error = Some(format!("Chrome: {error}"));
                state.state.loading = false;
                drop(state);
                notify(&shared);
            }
        });
        Ok(())
    }

    fn send(&mut self, command: Request) -> io::Result<()> {
        if self.commands.is_none() {
            self.start()?;
        }
        if let Some(commands) = &self.commands {
            commands
                .send(command)
                .map_err(|_| io::Error::new(ErrorKind::BrokenPipe, "the browser has gone"))?;
        }
        Ok(())
    }
}

impl Drop for ChromeEngine {
    fn drop(&mut self) {
        // Closing the channel ends the thread's loop; the browser goes now.
        self.commands = None;
        let child = lock(&self.shared).child.take();
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(profile) = &self.profile {
            let _ = std::fs::remove_dir_all(profile);
        }
    }
}

impl WebEngine for ChromeEngine {
    fn open(&mut self, url: &str) -> io::Result<()> {
        {
            let mut shared = lock(&self.shared);
            shared.state.loading = true;
            shared.state.error = None;
        }
        self.send(Request::Open(url.to_string()))
    }

    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        self.send(Request::Resize(columns, rows))
    }

    fn input(&mut self, input: WebInput) -> io::Result<()> {
        self.send(Request::Input(input))
    }

    fn back(&mut self) -> io::Result<()> {
        self.send(Request::Back)
    }

    fn forward(&mut self) -> io::Result<()> {
        self.send(Request::Forward)
    }

    fn reload(&mut self) -> io::Result<()> {
        self.send(Request::Reload)
    }

    fn poll(&mut self) -> Option<WebFrame> {
        lock(&self.shared).frame.take().map(WebFrame::Pixels)
    }

    fn state(&self) -> PageState {
        lock(&self.shared).state.clone()
    }

    fn set_notify(&mut self, notify: Notify) {
        lock(&self.shared).notify = Some(notify);
    }
}

/// The engine's ends of the browser's DevTools connection, before it runs.
enum Connect {
    /// Read the address from the browser's standard error, then connect.
    Port,
    /// Write to the browser's descriptor 3, read from its descriptor 4.
    #[cfg(unix)]
    Pipe(io::PipeWriter, io::PipeReader),
}

/// Set `command` up for `wire`: for the pipe, two pipes whose far ends the
/// browser gets as descriptors 3 (it reads) and 4 (it writes).
#[cfg_attr(not(unix), allow(unused_variables))]
fn connect(command: &mut Command, wire: Wire) -> io::Result<Connect> {
    match wire {
        #[cfg(unix)]
        Wire::Pipe => {
            use command_fds::{CommandFdExt, FdMapping};

            let (browser_reads, we_write) = io::pipe()?;
            let (we_read, browser_writes) = io::pipe()?;
            command
                .fd_mappings(vec![
                    FdMapping {
                        parent_fd: browser_reads.into(),
                        child_fd: 3,
                    },
                    FdMapping {
                        parent_fd: browser_writes.into(),
                        child_fd: 4,
                    },
                ])
                .map_err(|_| io::Error::other("descriptors 3 and 4 collide"))?;
            Ok(Connect::Pipe(we_write, we_read))
        }
        Wire::Port => Ok(Connect::Port),
    }
}

/// A new, empty directory for the browser's profile.
fn temporary_profile() -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    loop {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "rich-embed-chrome-{}-{n}-{nanos}",
            std::process::id()
        ));
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut builder = std::fs::DirBuilder::new();
        // This user's only: it holds the browser's cookies and its cache.
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Whether `arg` is a switch that turns any part of the browser's sandbox
/// off (`--no-sandbox`, `--disable-setuid-sandbox`, `--single-process`, …),
/// in any spelling the browser reads as one: one dash or two (or a slash
/// on Windows), any case, with or without a value.
pub(crate) fn turns_sandbox_off(arg: &str) -> bool {
    let name = arg.trim_start_matches(|c| c == '-' || (cfg!(windows) && c == '/'));
    if name.len() == arg.len() {
        // Not a switch: an address.
        return false;
    }
    let name = name
        .split('=')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    ((name.starts_with("no-") || name.starts_with("disable-")) && name.contains("sandbox"))
        || matches!(name.as_str(), "single-process" | "no-zygote")
}

/// Whether `arg` opens DevTools somewhere of its own
/// (`--remote-debugging-port`, `--remote-debugging-pipe`, …): the engine
/// chooses where DevTools goes, and a port would undo the pipe.
pub(crate) fn moves_devtools(arg: &str) -> bool {
    let name = arg.trim_start_matches(|c| c == '-' || (cfg!(windows) && c == '/'));
    name.len() != arg.len() && name.to_ascii_lowercase().starts_with("remote-debugging-")
}

/// The browser's command line: headless, with DevTools on `wire`, in its
/// own profile, without first-run screens, extensions or background
/// traffic. Never a switch that turns the sandbox off, nor one that moves
/// DevTools elsewhere.
pub(crate) fn launch_args(
    profile: &Path,
    extra: &[String],
    columns: u16,
    rows: u16,
    cell: (u32, u32),
    wire: Wire,
) -> Vec<String> {
    let (width, height) = viewport(columns, rows, cell);
    let mut args = vec![
        "--headless=new".to_string(),
        wire.switch().to_string(),
        format!("--user-data-dir={}", profile.display()),
        format!("--window-size={width},{height}"),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-extensions".to_string(),
        "--disable-background-networking".to_string(),
        "--disable-sync".to_string(),
        "--mute-audio".to_string(),
        "--hide-scrollbars".to_string(),
    ];
    args.extend(
        extra
            .iter()
            .filter(|arg| !turns_sandbox_off(arg) && !moves_devtools(arg))
            .cloned(),
    );
    args.push("about:blank".to_string());
    args
}

/// The page's size in CSS pixels for a pane of `columns` x `rows` cells.
fn viewport(columns: u16, rows: u16, (cw, ch): (u32, u32)) -> (u32, u32) {
    (columns.max(1) as u32 * cw, rows.max(1) as u32 * ch)
}

/// The DevTools address in a line of the browser's standard error.
pub(crate) fn devtools_url(line: &str) -> Option<String> {
    let at = line.find("ws://")?;
    let url = line[at..].trim();
    url.contains("/devtools/browser/").then(|| url.to_string())
}

/// A DevTools command: `{"id", "method", "params"}`, and the session it is
/// for (none: the browser itself).
pub(crate) fn command(id: u64, method: &str, params: Value, session: Option<&str>) -> String {
    let mut message = json!({"id": id, "method": method, "params": params});
    if let Some(session) = session {
        message["sessionId"] = Value::String(session.to_string());
    }
    message.to_string()
}

/// The DevTools modifier bits: Alt 1, Ctrl 2, Meta 4, Shift 8.
fn modifier_bits(modifiers: Modifiers) -> u8 {
    modifiers.alt as u8 + 2 * modifiers.ctrl as u8 + 8 * modifiers.shift as u8
}

/// A key's DOM name, its Windows virtual key code, and the text it types.
fn key_parts(key: Key) -> (String, u32, Option<String>) {
    let named = |name: &str, code: u32| (name.to_string(), code, None);
    match key.code {
        KeyCode::Char(c) => {
            let upper = c.to_ascii_uppercase();
            let code = if upper.is_ascii_alphanumeric() {
                upper as u32
            } else if c == ' ' {
                32
            } else {
                0
            };
            let text = (!key.modifiers.ctrl && !key.modifiers.alt).then(|| c.to_string());
            (c.to_string(), code, text)
        }
        KeyCode::Enter => ("Enter".into(), 13, Some("\r".into())),
        KeyCode::Tab => named("Tab", 9),
        KeyCode::BackTab => named("Tab", 9),
        KeyCode::Backspace => named("Backspace", 8),
        KeyCode::Delete => named("Delete", 46),
        KeyCode::Insert => named("Insert", 45),
        KeyCode::Escape => named("Escape", 27),
        KeyCode::Up => named("ArrowUp", 38),
        KeyCode::Down => named("ArrowDown", 40),
        KeyCode::Left => named("ArrowLeft", 37),
        KeyCode::Right => named("ArrowRight", 39),
        KeyCode::Home => named("Home", 36),
        KeyCode::End => named("End", 35),
        KeyCode::PageUp => named("PageUp", 33),
        KeyCode::PageDown => named("PageDown", 34),
        KeyCode::Menu => named("ContextMenu", 93),
        KeyCode::Modifier(which) => match which {
            ModifierKey::Shift => named("Shift", 16),
            ModifierKey::Ctrl => named("Control", 17),
            ModifierKey::Alt => named("Alt", 18),
            ModifierKey::Super => named("Meta", 91),
        },
        KeyCode::F(n) => (format!("F{n}"), 111 + n as u32, None),
    }
}

/// The `Input.dispatchKeyEvent` parameters for a key press: down (with its
/// text, if it types one) and up.
pub(crate) fn key_events(key: Key) -> Vec<Value> {
    let (name, code, text) = key_parts(key);
    let mut modifiers = modifier_bits(key.modifiers);
    if key.code == KeyCode::BackTab {
        modifiers |= 8;
    }
    let mut down = json!({
        "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
        "key": name,
        "windowsVirtualKeyCode": code,
        "modifiers": modifiers,
    });
    if let Some(text) = text {
        down["text"] = Value::String(text.clone());
        down["unmodifiedText"] = Value::String(text);
    }
    let up = json!({
        "type": "keyUp",
        "key": name,
        "windowsVirtualKeyCode": code,
        "modifiers": modifiers,
    });
    vec![down, up]
}

/// The `Input.dispatchMouseEvent` parameters for `mouse` at a cell of the
/// pane, at the middle of that cell in CSS pixels.
pub(crate) fn mouse_event(mouse: Mouse, (cw, ch): (u32, u32)) -> Value {
    let x = mouse.column as f64 * cw as f64 + cw as f64 / 2.0;
    let y = mouse.row as f64 * ch as f64 + ch as f64 / 2.0;
    let button = |b: Button| match b {
        Button::Left => "left",
        Button::Middle => "middle",
        Button::Right => "right",
    };
    let (kind, button, clicks) = match mouse.kind {
        MouseKind::Down(b) => ("mousePressed", button(b), 1),
        MouseKind::Up(b) => ("mouseReleased", button(b), 1),
        MouseKind::Drag(b) => ("mouseMoved", button(b), 0),
        MouseKind::Moved => ("mouseMoved", "none", 0),
        MouseKind::ScrollUp | MouseKind::ScrollDown => ("mouseWheel", "none", 0),
    };
    let mut event = json!({
        "type": kind,
        "x": x,
        "y": y,
        "button": button,
        "clickCount": clicks,
        "modifiers": modifier_bits(mouse.modifiers),
    });
    if kind == "mouseWheel" {
        let rows = if mouse.kind == MouseKind::ScrollUp {
            -3.0
        } else {
            3.0
        };
        event["deltaX"] = json!(0);
        event["deltaY"] = json!(rows * ch as f64);
    }
    event
}

/// Standard base64, as DevTools sends screencast frames.
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' => continue,
            _ => return None,
        };
        buffer = buffer << 6 | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A screencast frame (base64 JPEG or PNG) as RGB pixels.
fn decode_frame(data: &str) -> Option<Pixels> {
    let bytes = base64_decode(data)?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgb8();
    Pixels::new(image.width(), image.height(), image.into_raw())
}

/// Wait for the browser to print where DevTools listens, then keep reading
/// its standard error so it never blocks on a full pipe.
fn devtools_address(stderr: ChildStderr) -> io::Result<String> {
    let (found, wait) = mpsc::channel();
    thread::spawn(move || {
        let mut found = Some(found);
        let mut last = String::new();
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if let Some(url) = devtools_url(&line) {
                if let Some(found) = found.take() {
                    let _ = found.send(Ok(url));
                }
            } else if found.is_some() && !line.trim().is_empty() {
                last = line;
            }
        }
        if let Some(found) = found {
            let _ = found.send(Err(last));
        }
    });
    match wait.recv_timeout(START_TIMEOUT) {
        Ok(Ok(url)) => Ok(url),
        Ok(Err(last)) => Err(io::Error::other(if last.is_empty() {
            "the browser exited before DevTools started".to_string()
        } else {
            format!("the browser exited before DevTools started: {last}")
        })),
        Err(_) => Err(io::Error::new(
            ErrorKind::TimedOut,
            "the browser did not start DevTools in time",
        )),
    }
}

/// Keep reading the browser's standard error so it never blocks on a full
/// pipe; its last line, once it ends, says why the browser went.
#[cfg(unix)]
fn drain(stderr: ChildStderr) -> Receiver<String> {
    let (last_line, wait) = mpsc::channel();
    thread::spawn(move || {
        let mut last = String::new();
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if !line.trim().is_empty() {
                last = line;
            }
        }
        let _ = last_line.send(last);
    });
    wait
}

/// The DevTools connection's messages, whichever way they travel.
enum Transport {
    /// A WebSocket on a port of `127.0.0.1`, read with a timeout.
    WebSocket(Box<WebSocket<TcpStream>>),
    /// NUL-ended JSON on a pipe each way, read on a thread of its own so
    /// the engine's thread can wait with a timeout.
    #[cfg(unix)]
    Pipe {
        write: io::PipeWriter,
        read: Receiver<Vec<u8>>,
        /// The browser's last line on standard error, once it ends.
        stderr: Receiver<String>,
    },
}

impl Transport {
    /// Over the WebSocket the browser announced on its standard error.
    fn websocket(stderr: ChildStderr) -> io::Result<Transport> {
        let url = devtools_address(stderr)?;
        let address = url
            .trim_start_matches("ws://")
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let stream = TcpStream::connect(&address)?;
        let (socket, _) = tungstenite::client::client(url.as_str(), stream)
            .map_err(|error| io::Error::other(error.to_string()))?;
        socket.get_ref().set_read_timeout(Some(POLL))?;
        Ok(Transport::WebSocket(Box::new(socket)))
    }

    /// Over the pipe to the browser's descriptors 3 and 4.
    #[cfg(unix)]
    fn pipe(write: io::PipeWriter, read: io::PipeReader, stderr: ChildStderr) -> Transport {
        let (messages, receive) = mpsc::channel();
        thread::spawn(move || {
            let mut read = BufReader::new(read);
            loop {
                let mut message = Vec::new();
                match read.read_until(0, &mut message) {
                    // The pipe's end: the browser went.
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if message.last() == Some(&0) {
                            message.pop();
                        }
                        if messages.send(message).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Transport::Pipe {
            write,
            read: receive,
            stderr: drain(stderr),
        }
    }

    fn send(&mut self, text: &str) -> io::Result<()> {
        match self {
            Transport::WebSocket(socket) => socket
                .send(Message::text(text))
                .map_err(|error| io::Error::other(error.to_string())),
            #[cfg(unix)]
            Transport::Pipe { write, .. } => {
                // JSON escapes NUL inside strings, so it only ever ends one.
                let mut bytes = Vec::with_capacity(text.len() + 1);
                bytes.extend_from_slice(text.as_bytes());
                bytes.push(0);
                io::Write::write_all(write, &bytes)?;
                io::Write::flush(write)
            }
        }
    }

    /// The next message, or none if none came within [`POLL`].
    fn read(&mut self) -> io::Result<Option<String>> {
        match self {
            Transport::WebSocket(socket) => match socket.read() {
                Ok(Message::Text(text)) => Ok(Some(text.as_str().to_string())),
                Ok(Message::Close(_)) => Err(io::Error::other("the browser closed")),
                Ok(_) => Ok(None),
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    Ok(None)
                }
                Err(error) => Err(io::Error::other(error.to_string())),
            },
            #[cfg(unix)]
            Transport::Pipe { read, stderr, .. } => match read.recv_timeout(POLL) {
                Ok(message) => Ok(Some(String::from_utf8_lossy(&message).into_owned())),
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let last = stderr
                        .recv_timeout(Duration::from_secs(1))
                        .unwrap_or_default();
                    Err(io::Error::other(if last.is_empty() {
                        "the browser closed".to_string()
                    } else {
                        format!("the browser closed: {last}")
                    }))
                }
            },
        }
    }
}

/// What a pending command's answer is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    CreateTarget,
    Attach,
    History,
    Other,
}

/// The DevTools connection, on the engine's thread.
struct Session {
    transport: Transport,
    next: u64,
    pending: Vec<(u64, Pending)>,
    session: Option<String>,
    target: Option<String>,
    /// History entries' ids, and the current one's index.
    history: Vec<i64>,
    current: usize,
    /// The page's area in cells, for the viewport and the screencast.
    size: (u16, u16),
    cell: (u32, u32),
    /// What the app asked for before the page was attached.
    waiting: Vec<Request>,
}

impl Session {
    fn send(&mut self, method: &str, params: Value, kind: Pending, page: bool) -> io::Result<()> {
        self.next += 1;
        let session = if page { self.session.clone() } else { None };
        let text = command(self.next, method, params, session.as_deref());
        self.pending.push((self.next, kind));
        self.transport.send(&text)
    }

    fn page(&mut self, method: &str, params: Value) -> io::Result<()> {
        self.send(method, params, Pending::Other, true)
    }

    fn screencast(&mut self) -> io::Result<()> {
        let (width, height) = viewport(self.size.0, self.size.1, self.cell);
        self.page(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": false}),
        )?;
        self.page("Page.stopScreencast", json!({}))?;
        self.page(
            "Page.startScreencast",
            json!({"format": "jpeg", "quality": 70, "maxWidth": width, "maxHeight": height,
                   "everyNthFrame": 1}),
        )
    }

    fn history(&mut self) -> io::Result<()> {
        self.send(
            "Page.getNavigationHistory",
            json!({}),
            Pending::History,
            true,
        )
    }

    fn apply(&mut self, command: Request) -> io::Result<()> {
        if self.session.is_none() {
            self.waiting.push(command);
            return Ok(());
        }
        match command {
            Request::Open(url) => self.page("Page.navigate", json!({"url": url})),
            Request::Resize(columns, rows) => {
                self.size = (columns, rows);
                self.screencast()
            }
            Request::Input(WebInput::Key(key)) => {
                for event in key_events(key) {
                    self.page("Input.dispatchKeyEvent", event)?;
                }
                Ok(())
            }
            Request::Input(WebInput::Mouse(mouse)) => {
                let event = mouse_event(mouse, self.cell);
                self.page("Input.dispatchMouseEvent", event)
            }
            Request::Input(WebInput::Paste(text)) => {
                self.page("Input.insertText", json!({"text": text}))
            }
            Request::Back if self.current > 0 => {
                let entry = self.history[self.current - 1];
                self.page("Page.navigateToHistoryEntry", json!({"entryId": entry}))
            }
            Request::Forward if self.current + 1 < self.history.len() => {
                let entry = self.history[self.current + 1];
                self.page("Page.navigateToHistoryEntry", json!({"entryId": entry}))
            }
            Request::Back | Request::Forward => Ok(()),
            Request::Reload => self.page("Page.reload", json!({})),
        }
    }

    /// Handle one message from the browser.
    fn message(&mut self, text: &str, shared: &Mutex<Shared>) -> io::Result<()> {
        let Ok(message) = serde_json::from_str::<Value>(text) else {
            return Ok(());
        };
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            let kind = self
                .pending
                .iter()
                .position(|(n, _)| *n == id)
                .map(|at| self.pending.remove(at).1)
                .unwrap_or(Pending::Other);
            if let Some(error) = message.get("error") {
                if kind != Pending::Other {
                    return Err(io::Error::other(error.to_string()));
                }
                return Ok(());
            }
            let result = &message["result"];
            match kind {
                Pending::CreateTarget => {
                    let target = result["targetId"].as_str().unwrap_or_default().to_string();
                    self.target = Some(target.clone());
                    self.send(
                        "Target.attachToTarget",
                        json!({"targetId": target, "flatten": true}),
                        Pending::Attach,
                        false,
                    )?;
                }
                Pending::Attach => {
                    self.session = result["sessionId"].as_str().map(str::to_string);
                    self.page("Page.enable", json!({}))?;
                    self.screencast()?;
                    for command in std::mem::take(&mut self.waiting) {
                        self.apply(command)?;
                    }
                }
                Pending::History => {
                    let entries = result["entries"].as_array().cloned().unwrap_or_default();
                    self.history = entries
                        .iter()
                        .filter_map(|entry| entry["id"].as_i64())
                        .collect();
                    self.current = result["currentIndex"].as_u64().unwrap_or(0) as usize;
                    let entry = entries.get(self.current);
                    {
                        let mut shared = lock(shared);
                        if let Some(entry) = entry {
                            if let Some(url) = entry["url"].as_str() {
                                shared.state.url = url.to_string();
                            }
                            shared.state.title =
                                entry["title"].as_str().unwrap_or_default().to_string();
                        }
                        shared.state.can_go_back = self.current > 0;
                        shared.state.can_go_forward = self.current + 1 < self.history.len();
                    }
                    notify(shared);
                }
                Pending::Other => {}
            }
            return Ok(());
        }
        let params = &message["params"];
        match message["method"].as_str().unwrap_or_default() {
            "Page.screencastFrame" => {
                if let Some(ack) = params["sessionId"].as_i64() {
                    self.page("Page.screencastFrameAck", json!({"sessionId": ack}))?;
                }
                if let Some(pixels) = params["data"].as_str().and_then(decode_frame) {
                    lock(shared).frame = Some(pixels);
                    notify(shared);
                }
            }
            "Page.frameStartedLoading" | "Page.frameStoppedLoading" => {
                let main = params["frameId"].as_str() == self.target.as_deref();
                if main {
                    let started = message["method"] == "Page.frameStartedLoading";
                    lock(shared).state.loading = started;
                    notify(shared);
                    if !started {
                        self.history()?;
                    }
                }
            }
            "Page.frameNavigated" if params["frame"].get("parentId").is_none() => {
                if let Some(url) = params["frame"]["url"].as_str() {
                    lock(shared).state.url = url.to_string();
                    notify(shared);
                }
                self.history()?;
            }
            "Page.navigatedWithinDocument" => {
                if let Some(url) = params["url"].as_str() {
                    lock(shared).state.url = url.to_string();
                    notify(shared);
                }
                self.history()?;
            }
            "Inspector.detached" | "Target.detachedFromTarget" => {
                return Err(io::Error::other("the page was closed"));
            }
            _ => {}
        }
        Ok(())
    }
}

/// The engine's thread: connect, then pass messages both ways until the
/// engine goes.
fn run(
    connect: Connect,
    stderr: ChildStderr,
    commands: Receiver<Request>,
    shared: &Mutex<Shared>,
    cell: (u32, u32),
) -> io::Result<()> {
    let transport = match connect {
        Connect::Port => Transport::websocket(stderr)?,
        #[cfg(unix)]
        Connect::Pipe(write, read) => Transport::pipe(write, read, stderr),
    };
    let mut session = Session {
        transport,
        next: 0,
        pending: Vec::new(),
        session: None,
        target: None,
        history: Vec::new(),
        current: 0,
        size: (80, 24),
        cell,
        waiting: Vec::new(),
    };
    session.send(
        "Browser.setDownloadBehavior",
        json!({"behavior": "deny"}),
        Pending::Other,
        false,
    )?;
    session.send(
        "Target.createTarget",
        json!({"url": "about:blank"}),
        Pending::CreateTarget,
        false,
    )?;
    let started = Instant::now();
    loop {
        loop {
            match commands.try_recv() {
                Ok(command) => session.apply(command)?,
                Err(mpsc::TryRecvError::Empty) => break,
                // The engine went: close the browser.
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = session.send("Browser.close", json!({}), Pending::Other, false);
                    return Ok(());
                }
            }
        }
        if let Some(text) = session.transport.read()? {
            session.message(&text, shared)?;
        }
        if session.session.is_none() && started.elapsed() > START_TIMEOUT {
            return Err(io::Error::new(
                ErrorKind::TimedOut,
                "the browser did not open a page in time",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Held by the tests that start a Chrome that is not there: each makes
    /// and removes a profile, which would move another's count of them.
    static STARTS: Mutex<()> = Mutex::new(());

    #[test]
    fn a_chrome_that_cannot_start_leaves_no_profile_behind() {
        let _starts = STARTS.lock().unwrap_or_else(|e| e.into_inner());
        let ours = format!("rich-embed-chrome-{}-", std::process::id());
        let profiles = || {
            std::fs::read_dir(std::env::temp_dir())
                .map(|dir| {
                    dir.flatten()
                        .filter(|e| e.file_name().to_string_lossy().starts_with(&ours))
                        .count()
                })
                .unwrap_or(0)
        };
        let before = profiles();
        let mut engine = ChromeEngine::new().binary("/nonexistent/rich-embed-chrome");
        for _ in 0..3 {
            assert!(engine.start().is_err());
        }
        assert!(engine.profile.is_none());
        assert_eq!(profiles(), before);
    }

    #[test]
    fn commands_are_devtools_json() {
        let text = command(7, "Page.navigate", json!({"url": "https://a"}), Some("S1"));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value,
            json!({"id": 7, "method": "Page.navigate", "params": {"url": "https://a"},
                   "sessionId": "S1"})
        );
        let browser: Value =
            serde_json::from_str(&command(1, "Browser.close", json!({}), None)).unwrap();
        assert!(browser.get("sessionId").is_none());
    }

    #[test]
    fn keys_become_key_events() {
        let events = key_events(Key::char('a'));
        assert_eq!(
            events[0],
            json!({"type": "keyDown", "key": "a", "windowsVirtualKeyCode": 65, "modifiers": 0,
                   "text": "a", "unmodifiedText": "a"})
        );
        assert_eq!(events[1]["type"], "keyUp");
        let enter = key_events(Key::parse("enter").unwrap());
        assert_eq!(enter[0]["text"], "\r");
        let up = key_events(Key::parse("ctrl+up").unwrap());
        assert_eq!(
            up[0],
            json!({"type": "rawKeyDown", "key": "ArrowUp", "windowsVirtualKeyCode": 38,
                   "modifiers": 2})
        );
        // Ctrl with a letter types nothing; Shift+Tab is Tab with Shift.
        assert!(key_events(Key::ctrl('l'))[0].get("text").is_none());
        assert_eq!(
            key_events(Key::parse("shift+tab").unwrap())[0]["modifiers"],
            8
        );
        assert_eq!(
            key_events(Key::parse("f5").unwrap())[0]["windowsVirtualKeyCode"],
            116
        );
    }

    #[test]
    fn the_mouse_lands_in_the_middle_of_its_cell() {
        let press = mouse_event(Mouse::new(MouseKind::Down(Button::Left), 2, 1), (10, 20));
        assert_eq!(
            press,
            json!({"type": "mousePressed", "x": 25.0, "y": 30.0, "button": "left",
                   "clickCount": 1, "modifiers": 0})
        );
        let wheel = mouse_event(Mouse::new(MouseKind::ScrollDown, 0, 0), (10, 20));
        assert_eq!(wheel["type"], "mouseWheel");
        assert_eq!(wheel["deltaY"], 60.0);
        let moved = mouse_event(Mouse::new(MouseKind::Moved, 0, 0), (10, 20));
        assert_eq!(moved["button"], "none");
    }

    #[test]
    fn the_browser_is_launched_sandboxed_in_its_own_profile() {
        let extra = vec!["--no-sandbox".to_string(), "--lang=en".to_string()];
        let args = launch_args(
            Path::new("/tmp/profile"),
            &extra,
            80,
            24,
            (10, 20),
            Wire::DEFAULT,
        );
        assert!(args.contains(&"--headless=new".to_string()));
        assert!(args.contains(&"--user-data-dir=/tmp/profile".to_string()));
        assert!(args.contains(&"--window-size=800,480".to_string()));
        assert!(args.contains(&"--lang=en".to_string()));
        assert!(!args.iter().any(|arg| arg.contains("no-sandbox")));
        assert_eq!(args.last().unwrap(), "about:blank");
        let engine = ChromeEngine::new().args(["--no-sandbox"]);
        assert!(engine.args.is_empty());
    }

    #[test]
    fn devtools_runs_over_a_pipe_on_unix_and_a_port_elsewhere() {
        let args = launch_args(Path::new("/p"), &[], 80, 24, (10, 20), Wire::DEFAULT);
        let pipe = args.iter().any(|arg| arg == "--remote-debugging-pipe");
        let port = args.iter().any(|arg| arg == "--remote-debugging-port=0");
        assert_eq!((pipe, port), (cfg!(unix), !cfg!(unix)), "{args:?}");
        assert_eq!(ChromeEngine::new().wire, Wire::DEFAULT);
    }

    #[test]
    fn a_devtools_switch_of_the_callers_is_dropped() {
        let extra: Vec<String> = [
            "--remote-debugging-port=9222",
            "-Remote-Debugging-Address=0.0.0.0",
            "--remote-debugging-pipe",
            "--lang=en",
            "https://remote-debugging-port.example",
        ]
        .map(String::from)
        .to_vec();
        let args = launch_args(Path::new("/p"), &extra, 80, 24, (10, 20), Wire::DEFAULT);
        let devtools: Vec<_> = args
            .iter()
            .filter(|arg| arg.to_ascii_lowercase().starts_with("--remote-debugging"))
            .collect();
        assert_eq!(devtools, [Wire::DEFAULT.switch()]);
        let engine = ChromeEngine::new().args(extra);
        assert_eq!(
            engine.args,
            ["--lang=en", "https://remote-debugging-port.example"]
        );
    }

    /// A page's first frame over `wire`, from the browser `RICH_EMBED_CHROME`
    /// names (skipped without one). Both ways are tried on Linux and macOS,
    /// so the port Windows uses is still exercised where a browser is.
    fn a_frame_arrives_over(wire: Wire) {
        let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
            eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
            return;
        };
        let mut engine = ChromeEngine::new().binary(binary);
        engine.wire = wire;
        engine.resize(8, 4).unwrap();
        engine
            .open("data:text/html,<body style='background:%2300ff00'>")
            .unwrap();
        // A green frame at the pane's size: 8 x 4 cells of 10 x 20 pixels.
        let green = |frame: &Option<WebFrame>| match frame {
            Some(WebFrame::Pixels(p)) => {
                let at = (p.width as usize * p.height as usize / 2 + p.width as usize / 2) * 3;
                (p.width, p.height) == (80, 80)
                    && p.rgb
                        .get(at..at + 3)
                        .is_some_and(|c| c[0] < 60 && c[1] > 200 && c[2] < 60)
            }
            _ => false,
        };
        let start = Instant::now();
        let mut frame = None;
        while !green(&frame) && start.elapsed() < Duration::from_secs(60) {
            frame = engine.poll().or(frame);
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            green(&frame),
            "no green frame over {wire:?}: {:?}",
            engine.state().error
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_frame_arrives_over_the_pipe() {
        a_frame_arrives_over(Wire::Pipe);
    }

    #[test]
    fn a_frame_arrives_over_the_port() {
        a_frame_arrives_over(Wire::Port);
    }

    #[test]
    fn every_spelling_of_a_sandbox_switch_is_dropped() {
        for arg in [
            "--no-sandbox",
            "-no-sandbox",
            "---no-sandbox",
            "--No-Sandbox",
            "--no-sandbox=1",
            "--disable-setuid-sandbox",
            "--disable-namespace-sandbox",
            "-disable-seccomp-filter-sandbox",
            "--disable-gpu-sandbox",
            "--no-zygote-sandbox",
            "--no-sandbox-and-elevated",
            "--single-process",
            "--no-zygote",
        ] {
            assert!(turns_sandbox_off(arg), "{arg}");
        }
        assert_eq!(turns_sandbox_off("/no-sandbox"), cfg!(windows));
        for arg in [
            "--lang=en",
            "--enable-sandbox",
            "https://no-sandbox.example",
            "--",
        ] {
            assert!(!turns_sandbox_off(arg), "{arg}");
        }
        let extra: Vec<String> = ["-no-sandbox", "--Disable-Setuid-Sandbox", "--lang=en"]
            .map(String::from)
            .to_vec();
        let args = launch_args(
            Path::new("/tmp/profile"),
            &extra,
            80,
            24,
            (10, 20),
            Wire::DEFAULT,
        );
        assert!(!args
            .iter()
            .any(|arg| arg.to_ascii_lowercase().contains("sandbox")));
        let engine = ChromeEngine::new().args(extra);
        assert_eq!(engine.args, ["--lang=en"]);
    }

    #[cfg(unix)]
    #[test]
    fn the_profile_is_this_users_only() {
        use std::os::unix::fs::PermissionsExt;

        let _starts = STARTS.lock().unwrap_or_else(|e| e.into_inner());
        let profile = temporary_profile().unwrap();
        let mode = std::fs::metadata(&profile).unwrap().permissions().mode();
        std::fs::remove_dir(&profile).unwrap();
        assert_eq!(mode & 0o777, 0o700, "{mode:o}");
    }

    #[test]
    fn the_devtools_address_is_read_from_standard_error() {
        assert_eq!(
            devtools_url("DevTools listening on ws://127.0.0.1:41235/devtools/browser/ab-cd"),
            Some("ws://127.0.0.1:41235/devtools/browser/ab-cd".to_string())
        );
        assert_eq!(devtools_url("[1:2:ERROR] something else"), None);
    }

    #[test]
    fn frames_decode_from_base64() {
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(base64_decode("not base64!"), None);
        // A 1 x 1 PNG, red.
        let mut png = Vec::new();
        image::RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let encoded = encode(&png);
        let pixels = decode_frame(&encoded).unwrap();
        assert_eq!(
            (pixels.width, pixels.height, pixels.rgb),
            (1, 1, vec![255, 0, 0])
        );
    }

    /// Base64 for the test above.
    fn encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().fold(0u32, |n, &b| n << 8 | b as u32) << (8 * (3 - chunk.len()));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
        out
    }

    #[test]
    fn a_missing_browser_is_an_error_not_a_hang() {
        let _starts = STARTS.lock().unwrap_or_else(|e| e.into_inner());
        let mut engine = ChromeEngine::new().binary("/nonexistent/chrome-for-a-test");
        assert!(engine.resize(80, 24).is_err());
    }
}
