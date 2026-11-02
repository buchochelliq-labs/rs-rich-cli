//! The terminal's own API (#820): open a split beside this program, send
//! text to another pane, move the focus. Behind the `host` feature.
//!
//! A [`TerminalHost`] is one of:
//!
//! - **[`SocketHost`]:** terminal-browser's terminal protocol, which a
//!   terminal implements on a Unix socket named by
//!   `TERMINAL_BROWSER_TERMINAL_SOCKET`. One JSON request per connection,
//!   ending in a newline, and one JSON reply; `hello` first, which lists
//!   the methods the terminal has. A method it does not list is never
//!   sent: the call fails with [`HostError::Unsupported`].
//! - **Adapters, best effort, for terminals that do not speak it:**
//!   [`Tmux`] (`split-window`, `send-keys`, `select-pane`), [`WezTerm`]
//!   (`wezterm cli`), [`Kitty`] (`kitten @`, which needs the user's
//!   `allow_remote_control`) and [`Zellij`] (`zellij action`, splits
//!   only).
//!
//! [`detect`] picks one from the environment. Every command runs with its
//! arguments as a list, never through a shell, except tmux's pane command,
//! which tmux itself hands to a shell: it is quoted word by word
//! ([`shell_quote`]).
//!
//! **What it can do to the terminal:** a split runs a command, and
//! [`send_text`](TerminalHost::send_text) types into another pane, which
//! may be a shell. A program calls them only for what the user asked (a
//! `--split` switch, a "send to agent" key), never for what a page or a
//! file says. The socket must be a socket and this user's
//! ([`SocketHost::connect`]).

use std::fmt;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

/// The variable a terminal speaking the socket protocol sets to its
/// socket's path.
pub const SOCKET_ENV: &str = "TERMINAL_BROWSER_TERMINAL_SOCKET";

/// How long a socket call may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// Where a split opens, beside the current pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Right,
    Left,
    Up,
    Down,
}

impl Direction {
    /// `right`, `left`, `up` or `down` (`above` and `below` too).
    pub fn parse(text: &str) -> Option<Direction> {
        Some(match text.to_ascii_lowercase().as_str() {
            "right" => Direction::Right,
            "left" => Direction::Left,
            "up" | "above" | "top" => Direction::Up,
            "down" | "below" | "bottom" => Direction::Down,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Direction::Right => "right",
            Direction::Left => "left",
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }
}

/// A pane of the terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    /// The terminal's id for it: tmux's `%3`, WezTerm's `12`.
    pub id: String,
    pub title: String,
    /// Whether it has the focus.
    pub focused: bool,
}

impl Pane {
    pub fn new(id: impl Into<String>) -> Pane {
        Pane {
            id: id.into(),
            title: String::new(),
            focused: false,
        }
    }
}

/// A split to open: a new pane beside the current one, running `command`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    pub direction: Direction,
    /// The program and its arguments; empty: the terminal's shell.
    pub command: Vec<String>,
    /// The new pane's share, in percent; `None`: the terminal's default.
    pub size: Option<u8>,
}

impl Split {
    pub fn new(direction: Direction, command: Vec<String>) -> Split {
        Split {
            direction,
            command,
            size: None,
        }
    }
}

/// What a host can be asked, by the socket protocol's names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    GetCurrentPane,
    ListPanes,
    Neighbor,
    Split,
    SendText,
    PasteKey,
    FocusPane,
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::GetCurrentPane,
        Method::ListPanes,
        Method::Neighbor,
        Method::Split,
        Method::SendText,
        Method::PasteKey,
        Method::FocusPane,
    ];

    /// The protocol's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Method::GetCurrentPane => "getCurrentPane",
            Method::ListPanes => "listPanes",
            Method::Neighbor => "neighbor",
            Method::Split => "split",
            Method::SendText => "sendText",
            Method::PasteKey => "pasteKey",
            Method::FocusPane => "focusPane",
        }
    }
}

/// Why a call failed.
#[derive(Debug)]
pub enum HostError {
    /// The terminal does not have this method (its `hello` did not list
    /// it, or the adapter cannot do it): nothing was sent.
    Unsupported(&'static str),
    /// The terminal answered with an error.
    Refused(String),
    /// The socket or the command failed.
    Io(io::Error),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Unsupported(method) => write!(f, "the terminal cannot {method}"),
            HostError::Refused(why) => write!(f, "the terminal refused: {why}"),
            HostError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for HostError {}

impl From<io::Error> for HostError {
    fn from(error: io::Error) -> HostError {
        HostError::Io(error)
    }
}

pub type Result<T> = std::result::Result<T, HostError>;

/// A terminal, or a multiplexer, that can be asked to split, type and
/// focus. A method [`supports`](Self::supports) says no to fails with
/// [`HostError::Unsupported`] without doing anything.
pub trait TerminalHost {
    /// Its name: `tmux`, `wezterm`, or the socket's terminal's own.
    fn name(&self) -> &str;
    fn supports(&self, method: Method) -> bool;
    /// The pane this program runs in.
    fn current_pane(&self) -> Result<Pane>;
    fn list_panes(&self) -> Result<Vec<Pane>>;
    /// The pane beside the current one in `direction`, if any.
    fn neighbor(&self, direction: Direction) -> Result<Option<Pane>>;
    /// Open `split`; the new pane.
    fn split(&self, split: &Split) -> Result<Pane>;
    /// Type `text` into `pane`, as if pasted there (no Enter added).
    fn send_text(&self, pane: &Pane, text: &str) -> Result<()>;
    /// Move the focus to `pane`.
    fn focus_pane(&self, pane: &Pane) -> Result<()>;
}

/// The host this program runs under, from the environment: the socket
/// protocol's terminal first, then tmux, zellij, WezTerm and kitty, as
/// their own variables say; `None` under none of them.
pub fn detect() -> Option<Box<dyn TerminalHost>> {
    detect_in(&|name| std::env::var(name).ok())
}

/// [`detect`] over the variables `var` gives.
pub fn detect_in(var: &dyn Fn(&str) -> Option<String>) -> Option<Box<dyn TerminalHost>> {
    let set = |name: &str| var(name).filter(|v| !v.is_empty());
    #[cfg(unix)]
    if let Some(path) = set(SOCKET_ENV)
        && let Ok(host) = SocketHost::connect(&path)
    {
        return Some(Box::new(host));
    }
    if set("TMUX").is_some() {
        return Some(Box::new(Tmux::new(set("TMUX_PANE"))));
    }
    if set("ZELLIJ").is_some() {
        return Some(Box::new(Zellij::new()));
    }
    if let Some(pane) = set("WEZTERM_PANE") {
        return Some(Box::new(WezTerm::new(Some(pane))));
    }
    if let Some(window) = set("KITTY_WINDOW_ID") {
        return Some(Box::new(Kitty::new(Some(window))));
    }
    None
}

/// `words` as one POSIX shell command line, each word single-quoted so
/// the shell runs exactly them: no expansion, no other command.
///
/// ```
/// use rich_interact::host::shell_quote;
///
/// let line = shell_quote(&["noctiluca".into(), "it's; rm -rf ~".into()]);
/// assert_eq!(line, r#"'noctiluca' 'it'\''s; rm -rf ~'"#);
/// ```
pub fn shell_quote(words: &[String]) -> String {
    words
        .iter()
        .map(|w| format!("'{}'", w.replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Run `program` with `args`; what it printed, trimmed, or an error with
/// what it said.
fn run(program: &Path, args: &[String]) -> Result<String> {
    let out = Command::new(program).args(args).output().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            HostError::Io(io::Error::new(
                error.kind(),
                format!("{} not found", program.display()),
            ))
        } else {
            HostError::Io(error)
        }
    })?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let first = said.lines().next().unwrap_or("").trim();
        return Err(HostError::Refused(if first.is_empty() {
            format!("{} exited with {}", program.display(), out.status)
        } else {
            first.to_string()
        }));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

// --- The socket protocol -------------------------------------------------

/// A terminal that speaks terminal-browser's socket protocol.
#[cfg(unix)]
#[derive(Clone, Debug)]
pub struct SocketHost {
    path: PathBuf,
    name: String,
    methods: Vec<String>,
}

#[cfg(unix)]
impl SocketHost {
    /// Connect to the socket at `path` and say `hello`: the methods the
    /// terminal lists are the ones this host sends. Refuses a path that is
    /// not a socket, or is another user's.
    pub fn connect(path: impl AsRef<Path>) -> Result<SocketHost> {
        let path = path.as_ref().to_path_buf();
        check_socket(&path)?;
        let mut host = SocketHost {
            path,
            name: "terminal".into(),
            methods: Vec::new(),
        };
        let hello = host.request("hello", json!({}))?;
        let methods = hello
            .get("methods")
            .or_else(|| hello.pointer("/capabilities/methods"))
            .and_then(Value::as_array)
            .ok_or_else(|| HostError::Refused("hello listed no methods".into()))?;
        host.methods = methods
            .iter()
            .filter_map(|m| m.as_str().map(str::to_string))
            .collect();
        if let Some(name) = hello
            .get("name")
            .or_else(|| hello.get("terminal"))
            .and_then(Value::as_str)
        {
            host.name = name.to_string();
        }
        Ok(host)
    }

    /// The methods the terminal's `hello` listed.
    pub fn methods(&self) -> &[String] {
        &self.methods
    }

    /// One request on a connection of its own; the reply's result.
    fn request(&self, method: &str, params: Value) -> Result<Value> {
        use std::os::unix::net::UnixStream;
        let mut stream = UnixStream::connect(&self.path)?;
        stream.set_read_timeout(Some(CALL_TIMEOUT))?;
        stream.set_write_timeout(Some(CALL_TIMEOUT))?;
        let mut line = json!({"id": 1, "method": method, "params": params}).to_string();
        line.push('\n');
        stream.write_all(line.as_bytes())?;
        stream.flush()?;
        let mut reply = String::new();
        io::BufReader::new(stream.take_limited()).read_line(&mut reply)?;
        parse_reply(&reply)
    }

    /// `method`, if the terminal has it.
    fn call(&self, method: Method, params: Value) -> Result<Value> {
        if !self.supports(method) {
            return Err(HostError::Unsupported(method.name()));
        }
        self.request(method.name(), params)
    }
}

/// A reply's result: `{"result": …}`, or the reply itself when it has no
/// `result`; an `error` (a string, or an object with a `message`), or
/// `"ok": false`, is a refusal.
pub fn parse_reply(line: &str) -> Result<Value> {
    let reply: Value = serde_json::from_str(line.trim())
        .map_err(|e| HostError::Refused(format!("not a JSON reply: {e}")))?;
    let error = reply.get("error").filter(|e| !e.is_null());
    if let Some(error) = error {
        let why = error
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| error.to_string());
        return Err(HostError::Refused(why));
    }
    if reply.get("ok") == Some(&Value::Bool(false)) {
        return Err(HostError::Refused("the terminal said no".into()));
    }
    Ok(reply.get("result").cloned().unwrap_or(reply))
}

/// A pane from a reply: an object with an `id` (or `paneId`), a string or
/// a number, and `title` and `focused` when given.
fn pane_of(value: &Value) -> Option<Pane> {
    let value = value.get("pane").unwrap_or(value);
    let id = value.get("id").or_else(|| value.get("paneId"))?;
    let id = match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    Some(Pane {
        id,
        title: value
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        focused: value
            .get("focused")
            .or_else(|| value.get("active"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

#[cfg(unix)]
trait TakeLimited: Sized {
    /// At most a megabyte of reply.
    fn take_limited(self) -> io::Take<Self>;
}

#[cfg(unix)]
impl TakeLimited for std::os::unix::net::UnixStream {
    fn take_limited(self) -> io::Take<Self> {
        io::Read::take(self, 1 << 20)
    }
}

/// The path is a socket and this user's.
#[cfg(unix)]
fn check_socket(path: &Path) -> Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let meta = std::fs::metadata(path)?;
    if !meta.file_type().is_socket() {
        return Err(HostError::Refused(format!(
            "{} is not a socket",
            path.display()
        )));
    }
    if Some(meta.uid()) != current_uid() {
        return Err(HostError::Refused(format!(
            "{} is another user's socket",
            path.display()
        )));
    }
    Ok(())
}

/// This process's user.
#[cfg(unix)]
#[allow(unsafe_code)] // geteuid has no preconditions and cannot fail.
fn current_uid() -> Option<u32> {
    // SAFETY: as above.
    Some(unsafe { libc::geteuid() })
}

#[cfg(unix)]
impl TerminalHost for SocketHost {
    fn name(&self) -> &str {
        &self.name
    }

    fn supports(&self, method: Method) -> bool {
        self.methods.iter().any(|m| m == method.name())
    }

    fn current_pane(&self) -> Result<Pane> {
        let reply = self.call(Method::GetCurrentPane, json!({}))?;
        pane_of(&reply).ok_or_else(|| HostError::Refused("no pane in the reply".into()))
    }

    fn list_panes(&self) -> Result<Vec<Pane>> {
        let reply = self.call(Method::ListPanes, json!({}))?;
        let panes = reply
            .get("panes")
            .unwrap_or(&reply)
            .as_array()
            .ok_or_else(|| HostError::Refused("no panes in the reply".into()))?;
        Ok(panes.iter().filter_map(pane_of).collect())
    }

    fn neighbor(&self, direction: Direction) -> Result<Option<Pane>> {
        let reply = self.call(Method::Neighbor, json!({"direction": direction.name()}))?;
        Ok(pane_of(&reply))
    }

    fn split(&self, split: &Split) -> Result<Pane> {
        let mut params = json!({"direction": split.direction.name()});
        if !split.command.is_empty() {
            params["command"] = json!(split.command);
        }
        if let Some(size) = split.size {
            params["size"] = json!(size);
        }
        let reply = self.call(Method::Split, params)?;
        pane_of(&reply).ok_or_else(|| HostError::Refused("no pane in the reply".into()))
    }

    fn send_text(&self, pane: &Pane, text: &str) -> Result<()> {
        self.call(Method::SendText, json!({"paneId": pane.id, "text": text}))
            .map(drop)
    }

    fn focus_pane(&self, pane: &Pane) -> Result<()> {
        self.call(Method::FocusPane, json!({"paneId": pane.id}))
            .map(drop)
    }
}

// --- tmux -------------------------------------------------------------------

/// tmux, through its command line.
#[derive(Clone, Debug)]
pub struct Tmux {
    program: PathBuf,
    /// The server's socket (`-S`), for a server other than `$TMUX`'s.
    socket: Option<PathBuf>,
    /// `$TMUX_PANE`: the pane this program runs in.
    pane: Option<String>,
}

impl Tmux {
    pub fn new(pane: Option<String>) -> Tmux {
        Tmux {
            program: PathBuf::from("tmux"),
            socket: None,
            pane,
        }
    }

    /// The server whose socket is `path` (tmux's `-S`).
    #[must_use]
    pub fn socket(mut self, path: impl Into<PathBuf>) -> Tmux {
        self.socket = Some(path.into());
        self
    }

    fn args(&self, rest: &[String]) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(socket) = &self.socket {
            args.push("-S".into());
            args.push(socket.display().to_string());
        }
        args.extend(rest.iter().cloned());
        args
    }

    /// The pane commands are about: `$TMUX_PANE`, else tmux's current one.
    fn target(&self) -> Vec<String> {
        match &self.pane {
            Some(pane) => vec!["-t".into(), pane.clone()],
            None => Vec::new(),
        }
    }

    /// tmux's arguments for `split` (without the server's).
    pub fn split_args(&self, split: &Split) -> Vec<String> {
        let mut args = strings(&["split-window", "-P", "-F", "#{pane_id}"]);
        args.push(match split.direction {
            Direction::Right | Direction::Left => "-h".into(),
            Direction::Up | Direction::Down => "-v".into(),
        });
        if matches!(split.direction, Direction::Left | Direction::Up) {
            args.push("-b".into());
        }
        if let Some(size) = split.size {
            args.push("-l".into());
            args.push(format!("{size}%"));
        }
        args.extend(self.target());
        if !split.command.is_empty() {
            // tmux runs the pane's command through a shell.
            args.push("--".into());
            args.push(shell_quote(&split.command));
        }
        args
    }

    fn tmux(&self, rest: &[String]) -> Result<String> {
        run(&self.program, &self.args(rest))
    }

    fn pane_at(&self, target: &str) -> Result<Pane> {
        let mut args = strings(&["display-message", "-p"]);
        args.extend(["-t".into(), target.into()]);
        args.push("#{pane_id}|#{pane_active}|#{pane_title}".into());
        let out = self.tmux(&args)?;
        tmux_pane(&out).ok_or_else(|| HostError::Refused(format!("no pane {target}")))
    }
}

/// The pane beside `id` in `direction`, among `places` (each pane's id
/// and its left, top, width and height in cells, a one-cell border
/// between two): the one along that edge that shares the most of it.
pub fn neighbor_of(
    id: &str,
    places: &[(String, [i64; 4])],
    direction: Direction,
) -> Option<String> {
    let (_, [x, y, w, h]) = places.iter().find(|(p, _)| p == id)?;
    let overlap = |a: i64, al: i64, b: i64, bl: i64| (a + al).min(b + bl) - a.max(b);
    places
        .iter()
        .filter(|(p, _)| p != id)
        .filter_map(|(p, [px, py, pw, ph])| {
            let (touches, shared) = match direction {
                Direction::Right => (*px == x + w + 1, overlap(*y, *h, *py, *ph)),
                Direction::Left => (px + pw + 1 == *x, overlap(*y, *h, *py, *ph)),
                Direction::Down => (*py == y + h + 1, overlap(*x, *w, *px, *pw)),
                Direction::Up => (py + ph + 1 == *y, overlap(*x, *w, *px, *pw)),
            };
            (touches && shared > 0).then_some((shared, p))
        })
        .max_by_key(|(shared, _)| *shared)
        .map(|(_, p)| p.clone())
}

/// A pane from tmux's `#{pane_id}|#{pane_active}|#{pane_title}` (`|`, as
/// `display-message` shows a tab as `_`; the title, last, may hold `|`).
fn tmux_pane(line: &str) -> Option<Pane> {
    let mut parts = line.splitn(3, '|');
    let id = parts.next()?.trim();
    if !id.starts_with('%') {
        return None;
    }
    Some(Pane {
        id: id.to_string(),
        focused: parts.next() == Some("1"),
        title: parts.next().unwrap_or("").to_string(),
    })
}

impl TerminalHost for Tmux {
    fn name(&self) -> &str {
        "tmux"
    }

    fn supports(&self, method: Method) -> bool {
        method != Method::PasteKey
    }

    fn current_pane(&self) -> Result<Pane> {
        match &self.pane {
            Some(pane) => self.pane_at(pane),
            None => {
                let mut args = strings(&["display-message", "-p"]);
                args.push("#{pane_id}|#{pane_active}|#{pane_title}".into());
                let out = self.tmux(&args)?;
                tmux_pane(&out).ok_or_else(|| HostError::Refused("no current pane".into()))
            }
        }
    }

    fn list_panes(&self) -> Result<Vec<Pane>> {
        let mut args = strings(&["list-panes"]);
        args.extend(self.target());
        args.extend(strings(&["-F", "#{pane_id}|#{pane_active}|#{pane_title}"]));
        Ok(self.tmux(&args)?.lines().filter_map(tmux_pane).collect())
    }

    fn neighbor(&self, direction: Direction) -> Result<Option<Pane>> {
        // From the panes' places: tmux's own `{right-of}` is relative to
        // the window's active pane, which need not be this program's.
        let current = self.current_pane()?;
        let mut args = strings(&["list-panes"]);
        args.extend(self.target());
        args.extend(strings(&[
            "-F",
            "#{pane_id}|#{pane_left}|#{pane_top}|#{pane_width}|#{pane_height}",
        ]));
        let places: Vec<(String, [i64; 4])> = self
            .tmux(&args)?
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('|');
                let id = parts.next()?.to_string();
                let mut at = [0i64; 4];
                for slot in &mut at {
                    *slot = parts.next()?.parse().ok()?;
                }
                Some((id, at))
            })
            .collect();
        Ok(neighbor_of(&current.id, &places, direction).map(Pane::new))
    }

    fn split(&self, split: &Split) -> Result<Pane> {
        let id = self.tmux(&self.split_args(split))?;
        Ok(Pane::new(id.lines().next().unwrap_or_default()))
    }

    fn send_text(&self, pane: &Pane, text: &str) -> Result<()> {
        let mut args = strings(&["send-keys", "-l", "-t"]);
        args.push(pane.id.clone());
        args.push("--".into());
        args.push(text.into());
        self.tmux(&args).map(drop)
    }

    fn focus_pane(&self, pane: &Pane) -> Result<()> {
        let mut args = strings(&["select-pane", "-t"]);
        args.push(pane.id.clone());
        self.tmux(&args).map(drop)
    }
}

// --- WezTerm ----------------------------------------------------------------

/// WezTerm, through `wezterm cli`.
#[derive(Clone, Debug)]
pub struct WezTerm {
    program: PathBuf,
    /// `$WEZTERM_PANE`: the pane this program runs in.
    pane: Option<String>,
}

impl WezTerm {
    pub fn new(pane: Option<String>) -> WezTerm {
        WezTerm {
            program: PathBuf::from("wezterm"),
            pane,
        }
    }

    /// `wezterm`'s arguments for `split`.
    pub fn split_args(&self, split: &Split) -> Vec<String> {
        let mut args = strings(&["cli", "split-pane"]);
        args.push(
            match split.direction {
                Direction::Right => "--right",
                Direction::Left => "--left",
                Direction::Up => "--top",
                Direction::Down => "--bottom",
            }
            .into(),
        );
        if let Some(size) = split.size {
            args.push("--percent".into());
            args.push(size.to_string());
        }
        if let Some(pane) = &self.pane {
            args.push("--pane-id".into());
            args.push(pane.clone());
        }
        if !split.command.is_empty() {
            args.push("--".into());
            args.extend(split.command.iter().cloned());
        }
        args
    }
}

impl TerminalHost for WezTerm {
    fn name(&self) -> &str {
        "wezterm"
    }

    fn supports(&self, method: Method) -> bool {
        matches!(
            method,
            Method::GetCurrentPane
                | Method::ListPanes
                | Method::Split
                | Method::SendText
                | Method::FocusPane
        )
    }

    fn current_pane(&self) -> Result<Pane> {
        self.pane
            .clone()
            .map(|id| Pane {
                focused: true,
                ..Pane::new(id)
            })
            .ok_or(HostError::Unsupported(Method::GetCurrentPane.name()))
    }

    fn list_panes(&self) -> Result<Vec<Pane>> {
        let out = run(
            &self.program,
            &strings(&["cli", "list", "--format", "json"]),
        )?;
        let list: Value = serde_json::from_str(&out)
            .map_err(|e| HostError::Refused(format!("wezterm's list: {e}")))?;
        Ok(list
            .as_array()
            .map(|panes| {
                panes
                    .iter()
                    .filter_map(|p| {
                        Some(Pane {
                            id: p.get("pane_id")?.as_u64()?.to_string(),
                            title: p
                                .get("title")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .into(),
                            focused: p.get("is_active").and_then(Value::as_bool) == Some(true),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn neighbor(&self, _: Direction) -> Result<Option<Pane>> {
        Err(HostError::Unsupported(Method::Neighbor.name()))
    }

    fn split(&self, split: &Split) -> Result<Pane> {
        Ok(Pane::new(run(&self.program, &self.split_args(split))?))
    }

    fn send_text(&self, pane: &Pane, text: &str) -> Result<()> {
        let mut args = strings(&["cli", "send-text", "--pane-id"]);
        args.push(pane.id.clone());
        args.push("--".into());
        args.push(text.into());
        run(&self.program, &args).map(drop)
    }

    fn focus_pane(&self, pane: &Pane) -> Result<()> {
        let mut args = strings(&["cli", "activate-pane", "--pane-id"]);
        args.push(pane.id.clone());
        run(&self.program, &args).map(drop)
    }
}

// --- kitty ------------------------------------------------------------------

/// kitty, through its remote control (`kitten @`), which works only where
/// the user turned it on (`allow_remote_control`).
#[derive(Clone, Debug)]
pub struct Kitty {
    program: PathBuf,
    /// `$KITTY_WINDOW_ID`: the window this program runs in.
    window: Option<String>,
}

impl Kitty {
    pub fn new(window: Option<String>) -> Kitty {
        Kitty {
            program: PathBuf::from("kitten"),
            window,
        }
    }

    /// `kitten`'s arguments for `split`; `None` for a direction kitty's
    /// splits do not take (left, up).
    pub fn split_args(&self, split: &Split) -> Option<Vec<String>> {
        let location = match split.direction {
            Direction::Right => "vsplit",
            Direction::Down => "hsplit",
            Direction::Left | Direction::Up => return None,
        };
        let mut args = strings(&["@", "launch", "--type=window", "--cwd=current"]);
        args.push(format!("--location={location}"));
        if let Some(size) = split.size {
            args.push(format!("--bias={size}"));
        }
        if let Some(window) = &self.window {
            args.push(format!("--match=id:{window}"));
        }
        if !split.command.is_empty() {
            args.push("--".into());
            args.extend(split.command.iter().cloned());
        }
        Some(args)
    }
}

impl TerminalHost for Kitty {
    fn name(&self) -> &str {
        "kitty"
    }

    fn supports(&self, method: Method) -> bool {
        matches!(
            method,
            Method::GetCurrentPane | Method::Split | Method::SendText | Method::FocusPane
        )
    }

    fn current_pane(&self) -> Result<Pane> {
        self.window
            .clone()
            .map(|id| Pane {
                focused: true,
                ..Pane::new(id)
            })
            .ok_or(HostError::Unsupported(Method::GetCurrentPane.name()))
    }

    fn list_panes(&self) -> Result<Vec<Pane>> {
        Err(HostError::Unsupported(Method::ListPanes.name()))
    }

    fn neighbor(&self, _: Direction) -> Result<Option<Pane>> {
        Err(HostError::Unsupported(Method::Neighbor.name()))
    }

    fn split(&self, split: &Split) -> Result<Pane> {
        let args = self
            .split_args(split)
            .ok_or(HostError::Unsupported("split left or up"))?;
        Ok(Pane::new(run(&self.program, &args)?))
    }

    fn send_text(&self, pane: &Pane, text: &str) -> Result<()> {
        let mut args = strings(&["@", "send-text"]);
        args.push(format!("--match=id:{}", pane.id));
        args.push("--".into());
        args.push(text.into());
        run(&self.program, &args).map(drop)
    }

    fn focus_pane(&self, pane: &Pane) -> Result<()> {
        let mut args = strings(&["@", "focus-window"]);
        args.push(format!("--match=id:{}", pane.id));
        run(&self.program, &args).map(drop)
    }
}

// --- zellij -----------------------------------------------------------------

/// zellij, through `zellij action`: splits only (its actions name no
/// pane, so text and the focus cannot be aimed).
#[derive(Clone, Debug)]
pub struct Zellij {
    program: PathBuf,
}

impl Zellij {
    pub fn new() -> Zellij {
        Zellij {
            program: PathBuf::from("zellij"),
        }
    }

    /// `zellij`'s arguments for `split`; `None` for left or up.
    pub fn split_args(&self, split: &Split) -> Option<Vec<String>> {
        let direction = match split.direction {
            Direction::Right => "right",
            Direction::Down => "down",
            Direction::Left | Direction::Up => return None,
        };
        let mut args = strings(&["action", "new-pane", "--direction", direction]);
        if !split.command.is_empty() {
            args.push("--".into());
            args.extend(split.command.iter().cloned());
        }
        Some(args)
    }
}

impl Default for Zellij {
    fn default() -> Zellij {
        Zellij::new()
    }
}

impl TerminalHost for Zellij {
    fn name(&self) -> &str {
        "zellij"
    }

    fn supports(&self, method: Method) -> bool {
        method == Method::Split
    }

    fn current_pane(&self) -> Result<Pane> {
        Err(HostError::Unsupported(Method::GetCurrentPane.name()))
    }

    fn list_panes(&self) -> Result<Vec<Pane>> {
        Err(HostError::Unsupported(Method::ListPanes.name()))
    }

    fn neighbor(&self, _: Direction) -> Result<Option<Pane>> {
        Err(HostError::Unsupported(Method::Neighbor.name()))
    }

    fn split(&self, split: &Split) -> Result<Pane> {
        let args = self
            .split_args(split)
            .ok_or(HostError::Unsupported("split left or up"))?;
        run(&self.program, &args)?;
        // zellij prints no id.
        Ok(Pane::new(""))
    }

    fn send_text(&self, _: &Pane, _: &str) -> Result<()> {
        Err(HostError::Unsupported(Method::SendText.name()))
    }

    fn focus_pane(&self, _: &Pane) -> Result<()> {
        Err(HostError::Unsupported(Method::FocusPane.name()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(direction: Direction) -> Split {
        Split {
            direction,
            command: strings(&["noctiluca", "https://example.com/?a=1&b='2'"]),
            size: Some(40),
        }
    }

    #[test]
    fn directions_parse_by_name() {
        assert_eq!(Direction::parse("RIGHT"), Some(Direction::Right));
        assert_eq!(Direction::parse("below"), Some(Direction::Down));
        assert_eq!(Direction::parse("sideways"), None);
        for d in [
            Direction::Right,
            Direction::Left,
            Direction::Up,
            Direction::Down,
        ] {
            assert_eq!(Direction::parse(d.name()), Some(d));
        }
    }

    #[test]
    fn a_quoted_command_runs_as_its_words() {
        let line = shell_quote(&strings(&["a b", "$(id)", "it's", ""]));
        assert_eq!(line, r"'a b' '$(id)' 'it'\''s' ''");
        let out = Command::new("sh")
            .args(["-c", &format!("printf '%s|' {line}")])
            .output();
        if let Ok(out) = out {
            assert_eq!(String::from_utf8_lossy(&out.stdout), "a b|$(id)|it's||");
        }
    }

    #[test]
    fn replies_and_refusals_are_read() {
        assert_eq!(
            parse_reply(r#"{"id":1,"result":{"methods":["split"]}}"#).unwrap(),
            json!({"methods": ["split"]})
        );
        // A reply with no `result` is its own result.
        assert_eq!(
            parse_reply(r#"{"methods":["split"]}"#).unwrap()["methods"][0],
            "split"
        );
        let refused = |line: &str| match parse_reply(line) {
            Err(HostError::Refused(why)) => why,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            refused(r#"{"id":1,"error":{"message":"no such pane"}}"#),
            "no such pane"
        );
        assert_eq!(refused(r#"{"error":"busy"}"#), "busy");
        assert!(refused(r#"{"ok":false}"#).contains("said no"));
        assert!(refused("not json").contains("not a JSON reply"));
        assert!(parse_reply(r#"{"id":1,"result":null,"error":null}"#).is_ok());
    }

    #[test]
    fn panes_are_read_from_ids_strings_or_numbers() {
        assert_eq!(
            pane_of(&json!({"id": 3, "title": "agent", "focused": true})),
            Some(Pane {
                id: "3".into(),
                title: "agent".into(),
                focused: true
            })
        );
        assert_eq!(
            pane_of(&json!({"pane": {"paneId": "p1"}})).unwrap().id,
            "p1"
        );
        assert_eq!(pane_of(&json!({"title": "no id"})), None);
        assert_eq!(
            tmux_pane("%3|1|vim"),
            Some(Pane {
                id: "%3".into(),
                focused: true,
                title: "vim".into()
            })
        );
        assert_eq!(tmux_pane("no pane"), None);
    }

    #[test]
    fn the_neighbor_is_the_pane_along_that_edge() {
        // %0 on the left half, %1 top right, %2 bottom right.
        let places = vec![
            ("%0".to_string(), [0, 0, 40, 24]),
            ("%1".to_string(), [41, 0, 39, 12]),
            ("%2".to_string(), [41, 13, 39, 11]),
        ];
        assert_eq!(
            neighbor_of("%0", &places, Direction::Right).as_deref(),
            Some("%1")
        );
        assert_eq!(
            neighbor_of("%2", &places, Direction::Left).as_deref(),
            Some("%0")
        );
        assert_eq!(
            neighbor_of("%1", &places, Direction::Down).as_deref(),
            Some("%2")
        );
        assert_eq!(
            neighbor_of("%2", &places, Direction::Up).as_deref(),
            Some("%1")
        );
        assert_eq!(neighbor_of("%0", &places, Direction::Left), None);
        assert_eq!(neighbor_of("%9", &places, Direction::Left), None);
    }

    #[test]
    fn tmux_splits_with_its_flags_and_one_quoted_command() {
        let tmux = Tmux::new(Some("%1".into()));
        let args = tmux.split_args(&split(Direction::Left));
        assert_eq!(
            args,
            [
                "split-window",
                "-P",
                "-F",
                "#{pane_id}",
                "-h",
                "-b",
                "-l",
                "40%",
                "-t",
                "%1",
                "--",
                r"'noctiluca' 'https://example.com/?a=1&b='\''2'\'''"
            ]
        );
        let down = tmux.split_args(&Split::new(Direction::Down, Vec::new()));
        assert_eq!(
            down,
            ["split-window", "-P", "-F", "#{pane_id}", "-v", "-t", "%1"]
        );
        // A server of its own.
        let args = Tmux::new(None)
            .socket("/tmp/s")
            .args(&strings(&["list-panes"]));
        assert_eq!(args, ["-S", "/tmp/s", "list-panes"]);
    }

    #[test]
    fn the_adapters_pass_the_command_as_words() {
        let wez = WezTerm::new(Some("7".into())).split_args(&split(Direction::Up));
        assert_eq!(
            wez,
            [
                "cli",
                "split-pane",
                "--top",
                "--percent",
                "40",
                "--pane-id",
                "7",
                "--",
                "noctiluca",
                "https://example.com/?a=1&b='2'"
            ]
        );
        let kitty = Kitty::new(Some("2".into()));
        assert_eq!(
            kitty.split_args(&split(Direction::Right)).unwrap(),
            [
                "@",
                "launch",
                "--type=window",
                "--cwd=current",
                "--location=vsplit",
                "--bias=40",
                "--match=id:2",
                "--",
                "noctiluca",
                "https://example.com/?a=1&b='2'"
            ]
        );
        assert!(kitty.split_args(&split(Direction::Left)).is_none());
        let zellij = Zellij::new().split_args(&split(Direction::Down)).unwrap();
        assert_eq!(
            zellij,
            [
                "action",
                "new-pane",
                "--direction",
                "down",
                "--",
                "noctiluca",
                "https://example.com/?a=1&b='2'"
            ]
        );
        // What an adapter cannot do is said, and nothing runs.
        assert!(matches!(
            Zellij::new().send_text(&Pane::new("1"), "x"),
            Err(HostError::Unsupported("sendText"))
        ));
        assert!(matches!(
            Kitty::new(None).split(&split(Direction::Up)),
            Err(HostError::Unsupported(_))
        ));
    }

    #[test]
    fn the_environment_picks_the_host() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        let name = |pairs: &'static [(&'static str, &'static str)]| {
            detect_in(&env(pairs)).map(|h| h.name().to_string())
        };
        assert_eq!(name(&[]), None);
        assert_eq!(
            name(&[("TMUX", "/tmp/tmux-1/default,1,0"), ("TMUX_PANE", "%0")]).as_deref(),
            Some("tmux")
        );
        assert_eq!(name(&[("ZELLIJ", "0")]).as_deref(), Some("zellij"));
        assert_eq!(name(&[("WEZTERM_PANE", "3")]).as_deref(), Some("wezterm"));
        assert_eq!(name(&[("KITTY_WINDOW_ID", "1")]).as_deref(), Some("kitty"));
        // tmux inside WezTerm: the innermost, tmux, splits.
        assert_eq!(
            name(&[("WEZTERM_PANE", "3"), ("TMUX", "x"), ("TMUX_PANE", "%1")]).as_deref(),
            Some("tmux")
        );
        // A socket that is not there is passed over.
        assert_eq!(
            name(&[(SOCKET_ENV, "/nonexistent/terminal.sock"), ("ZELLIJ", "0")]).as_deref(),
            Some("zellij")
        );
        // An empty variable is no host.
        assert_eq!(name(&[("TMUX", "")]), None);
    }
}
