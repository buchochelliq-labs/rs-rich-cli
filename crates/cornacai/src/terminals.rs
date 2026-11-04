//! The agent's terminals (`terminal/*`): on pipes, with rs-rich-acp's
//! [`Terminals`], or, with the `pty` setting, each command on a PTY of its
//! own (rs-rich-embed's [`LocalPty`]), so programs that look for a
//! terminal colour their output and draw their progress as they would for
//! the user. A command whose PTY cannot be opened runs on pipes instead.
//!
//! What a command on a PTY writes is read as a terminal would show it, as
//! plain text: escape sequences (colour, cursor movement, titles) are
//! dropped, a carriage return starts its line again (a progress bar keeps
//! its last state), a backspace takes back a character. The agent reads
//! that text, kept to the request's `outputByteLimit` as on pipes.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use rich_acp::{
    CreateTerminalRequest, CreateTerminalResponse, DEFAULT_OUTPUT_LIMIT, RpcError,
    TerminalExitStatus, TerminalId, TerminalOutputResponse, TerminalRequest, Terminals, Workspace,
};
use rich_embed::{LocalPty, PtyHost};

/// The size of a command's PTY: columns, rows.
pub const PTY_SIZE: (u16, u16) = (120, 40);

/// How often a PTY's output and exit are looked at.
const PUMP_TURN: Duration = Duration::from_millis(15);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A started host for a command, or why there is none.
type Opener = fn(&CreateTerminalRequest, &Path) -> io::Result<Box<dyn PtyHost + Send>>;

/// The command on a local PTY, started at [`PTY_SIZE`].
fn open_local(request: &CreateTerminalRequest, cwd: &Path) -> io::Result<Box<dyn PtyHost + Send>> {
    let mut command = rich_embed::Command::new(&request.command)
        .args(&request.args)
        .cwd(cwd);
    for var in &request.env {
        command = command.env(&var.name, &var.value);
    }
    let mut host = LocalPty::new(command);
    host.start(PTY_SIZE.0, PTY_SIZE.1)?;
    Ok(Box::new(host))
}

/// The agent's terminals: pipes, or PTYs falling back to pipes.
pub struct AgentTerminals {
    workspace: Workspace,
    pipes: Terminals,
    pty: bool,
    opener: Opener,
    next: AtomicU64,
    ptys: Mutex<HashMap<TerminalId, Arc<PtyTerminal>>>,
}

impl AgentTerminals {
    /// Terminals inside `workspace`, on PTYs when `pty` is on.
    pub fn new(workspace: Workspace, pty: bool) -> AgentTerminals {
        AgentTerminals::with_opener(workspace, pty, open_local)
    }

    fn with_opener(workspace: Workspace, pty: bool, opener: Opener) -> AgentTerminals {
        AgentTerminals {
            pipes: Terminals::new(workspace.clone()),
            workspace,
            pty,
            opener,
            next: AtomicU64::new(1),
            ptys: Mutex::new(HashMap::new()),
        }
    }

    /// Whether commands go on PTYs.
    pub fn uses_pty(&self) -> bool {
        self.pty
    }

    /// `terminal/create`: starts the command, on a PTY when the setting is
    /// on and one opens, else on pipes. Its `cwd` must be inside the
    /// workspace.
    pub fn create(
        &self,
        request: &CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, RpcError> {
        if !self.pty {
            return self.pipes.create(request);
        }
        let cwd = match &request.cwd {
            Some(cwd) => self.workspace.check(cwd)?,
            None => self.workspace.cwd().to_path_buf(),
        };
        let host = match (self.opener)(request, &cwd) {
            Ok(host) => host,
            // No PTY here (none left, or none on this system): pipes.
            Err(_) => return self.pipes.create(request),
        };
        let limit = request
            .output_byte_limit
            .map_or(DEFAULT_OUTPUT_LIMIT, |limit| {
                usize::try_from(limit).unwrap_or(usize::MAX)
            });
        let terminal = Arc::new(PtyTerminal {
            host: Mutex::new(host),
            state: Mutex::new(PtyState {
                plain: Plain::new(limit),
                exit: None,
            }),
            changed: Condvar::new(),
        });
        let pumped = Arc::clone(&terminal);
        thread::Builder::new()
            .name("cornacai-pty".into())
            .spawn(move || pump(&pumped))
            .map_err(|e| RpcError::internal(format!("cannot watch the command: {e}")))?;
        let id = TerminalId(format!("pty_{}", self.next.fetch_add(1, Ordering::SeqCst)));
        lock(&self.ptys).insert(id.clone(), terminal);
        Ok(CreateTerminalResponse { terminal_id: id })
    }

    fn pty(&self, id: &TerminalId) -> Option<Arc<PtyTerminal>> {
        lock(&self.ptys).get(id).cloned()
    }

    /// `terminal/output`.
    pub fn output(&self, request: &TerminalRequest) -> Result<TerminalOutputResponse, RpcError> {
        let Some(terminal) = self.pty(&request.terminal_id) else {
            return self.pipes.output(request);
        };
        let state = lock(&terminal.state);
        Ok(TerminalOutputResponse {
            output: state.plain.text.clone(),
            truncated: state.plain.truncated,
            exit_status: state.exit.clone(),
        })
    }

    /// `terminal/wait_for_exit`.
    pub fn wait_for_exit(&self, request: &TerminalRequest) -> Result<TerminalExitStatus, RpcError> {
        let Some(terminal) = self.pty(&request.terminal_id) else {
            return self.pipes.wait_for_exit(request);
        };
        let mut state = lock(&terminal.state);
        loop {
            if let Some(exit) = &state.exit {
                return Ok(exit.clone());
            }
            state = terminal
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// `terminal/kill`: ends the command; the terminal stays readable.
    pub fn kill(&self, request: &TerminalRequest) -> Result<(), RpcError> {
        let Some(terminal) = self.pty(&request.terminal_id) else {
            return self.pipes.kill(request);
        };
        let _ = lock(&terminal.host).kill();
        Ok(())
    }

    /// `terminal/release`: ends the command if it still runs and forgets
    /// the terminal.
    pub fn release(&self, request: &TerminalRequest) -> Result<(), RpcError> {
        let Some(terminal) = lock(&self.ptys).remove(&request.terminal_id) else {
            return self.pipes.release(request);
        };
        let _ = lock(&terminal.host).kill();
        Ok(())
    }
}

impl Drop for AgentTerminals {
    fn drop(&mut self) {
        for terminal in lock(&self.ptys).values() {
            let _ = lock(&terminal.host).kill();
        }
    }
}

struct PtyState {
    plain: Plain,
    exit: Option<TerminalExitStatus>,
}

struct PtyTerminal {
    host: Mutex<Box<dyn PtyHost + Send>>,
    state: Mutex<PtyState>,
    changed: Condvar,
}

/// Reads the command's output as it comes, until it has exited.
fn pump(terminal: &PtyTerminal) {
    loop {
        let (bytes, exit) = {
            let mut host = lock(&terminal.host);
            let bytes = host.read();
            // Reported only once the output before it has been read.
            let exit = host.exit_status();
            (bytes, exit)
        };
        if !bytes.is_empty() || exit.is_some() {
            let mut state = lock(&terminal.state);
            state.plain.push(&bytes);
            if let Some(status) = &exit {
                state.exit = Some(match status.signal() {
                    Some(signal) => TerminalExitStatus {
                        exit_code: None,
                        signal: Some(signal.to_owned()),
                    },
                    None => TerminalExitStatus {
                        exit_code: Some(status.code()),
                        signal: None,
                    },
                });
            }
            terminal.changed.notify_all();
        }
        if exit.is_some() {
            return;
        }
        thread::sleep(PUMP_TURN);
    }
}

/// Where [`Plain`] is in an escape sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Escape {
    None,
    /// After `ESC`.
    Start,
    /// After `ESC` and an intermediate byte.
    Intermediate,
    /// A control sequence, after `ESC [`.
    Csi,
    /// A string (OSC, DCS, APC, PM, SOS), to BEL or `ESC \`.
    InString,
    /// `ESC` inside a string.
    StringEnd,
}

/// A terminal's output read as plain text, kept to a byte limit.
#[derive(Clone, Debug)]
pub struct Plain {
    text: String,
    truncated: bool,
    limit: usize,
    /// Where the current line starts in `text`.
    line_start: usize,
    /// A carriage return seen: the next character starts the line again.
    returned: bool,
    escape: Escape,
    /// Bytes of a character not yet whole.
    partial: Vec<u8>,
}

impl Plain {
    /// Empty, keeping at most `limit` bytes (the oldest dropped, on a
    /// character boundary).
    pub fn new(limit: usize) -> Plain {
        Plain {
            text: String::new(),
            truncated: false,
            limit,
            line_start: 0,
            returned: false,
            escape: Escape::None,
            partial: Vec::new(),
        }
    }

    /// The text so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether older text was dropped for the limit.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Reads `bytes`, as they came from the terminal.
    pub fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte(byte);
        }
        if self.text.len() > self.limit {
            let mut cut = self.text.len() - self.limit;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
            self.line_start = self.line_start.saturating_sub(cut);
            self.truncated = true;
        }
    }

    fn byte(&mut self, byte: u8) {
        match self.escape {
            Escape::None => {}
            Escape::Start => {
                self.escape = match byte {
                    b'[' => Escape::Csi,
                    b']' | b'P' | b'_' | b'^' | b'X' => Escape::InString,
                    0x20..=0x2f => Escape::Intermediate,
                    _ => Escape::None,
                };
                return;
            }
            Escape::Intermediate => {
                if !(0x20..=0x2f).contains(&byte) {
                    self.escape = Escape::None;
                }
                return;
            }
            Escape::Csi => {
                if (0x40..=0x7e).contains(&byte) {
                    self.escape = Escape::None;
                }
                return;
            }
            Escape::InString => {
                match byte {
                    0x07 => self.escape = Escape::None,
                    0x1b => self.escape = Escape::StringEnd,
                    _ => {}
                }
                return;
            }
            Escape::StringEnd => {
                self.escape = if byte == b'\\' {
                    Escape::None
                } else {
                    Escape::InString
                };
                return;
            }
        }
        match byte {
            0x1b => {
                self.partial.clear();
                self.escape = Escape::Start;
            }
            b'\r' => self.returned = true,
            b'\n' => {
                self.returned = false;
                self.text.push('\n');
                self.line_start = self.text.len();
            }
            0x08 => {
                if self.text.len() > self.line_start {
                    self.text.pop();
                }
            }
            b'\t' => self.char('\t'),
            0x00..=0x1f | 0x7f => {}
            0x20..=0x7e => self.char(byte as char),
            _ => {
                self.partial.push(byte);
                match std::str::from_utf8(&self.partial) {
                    Ok(text) => {
                        let text = text.to_owned();
                        self.partial.clear();
                        for c in text.chars() {
                            self.char(c);
                        }
                    }
                    Err(e) if e.error_len().is_some() => {
                        self.partial.clear();
                        self.char(char::REPLACEMENT_CHARACTER);
                    }
                    Err(_) => {}
                }
            }
        }
    }

    fn char(&mut self, c: char) {
        if self.returned {
            self.returned = false;
            self.text.truncate(self.line_start);
        }
        self.text.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(bytes: &[u8]) -> String {
        let mut plain = Plain::new(usize::MAX);
        plain.push(bytes);
        plain.text().to_owned()
    }

    #[test]
    fn terminal_output_reads_as_the_terminal_shows_it() {
        // Colour and a title gone; CRLF a newline.
        assert_eq!(
            plain(b"\x1b]0;title\x07\x1b[1;32mok\x1b[0m\r\nnext\r\n"),
            "ok\nnext\n"
        );
        // A progress bar keeps its last state.
        assert_eq!(plain(b"10%\r50%\r100%\ndone"), "100%\ndone");
        // A backspace takes back a character, never past the line.
        assert_eq!(plain(b"ab\x08c\n\x08x"), "ac\nx");
        // UTF-8 split anywhere, and a bad byte replaced.
        let mut split = Plain::new(usize::MAX);
        split.push(&"é€".as_bytes()[..3]);
        split.push(&"é€".as_bytes()[3..]);
        assert_eq!(split.text(), "é€");
        assert_eq!(plain(b"a\xffb"), "a\u{fffd}b");
        // A string ended by ESC \, and a two-byte escape.
        assert_eq!(plain(b"\x1bP1;2\x1b\\x\x1b=y\x1b(Bz"), "xyz");
    }

    #[test]
    fn the_limit_drops_the_oldest_on_a_character_boundary() {
        let mut plain = Plain::new(4);
        plain.push("aé€".as_bytes());
        assert!(plain.truncated());
        assert_eq!(plain.text(), "€");
        // A carriage return after the cut starts the kept line again.
        let mut plain = Plain::new(6);
        plain.push(b"0123456789\rab");
        assert_eq!(plain.text(), "ab");
    }

    fn workspace(name: &str) -> Workspace {
        let dir =
            std::env::temp_dir().join(format!("cornacai-terms-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Workspace::new(dir.canonicalize().unwrap()).unwrap()
    }

    fn echo(text: &str) -> CreateTerminalRequest {
        CreateTerminalRequest {
            session_id: "s".into(),
            command: "echo".into(),
            args: vec![text.into()],
            ..CreateTerminalRequest::default()
        }
    }

    fn run(terminals: &AgentTerminals, request: &CreateTerminalRequest) -> (TerminalId, String) {
        let id = terminals.create(request).unwrap().terminal_id;
        let asked = TerminalRequest {
            session_id: "s".into(),
            terminal_id: id.clone(),
        };
        let exit = terminals.wait_for_exit(&asked).unwrap();
        assert_eq!(exit.exit_code, Some(0));
        let out = terminals.output(&asked).unwrap();
        terminals.release(&asked).unwrap();
        (id, out.output)
    }

    #[cfg(unix)]
    #[test]
    fn a_command_runs_on_a_pty_when_asked() {
        let terminals = AgentTerminals::new(workspace("pty"), true);
        let mut request = echo("x");
        request.command = "sh".into();
        request.args = vec![
            "-c".into(),
            "[ -t 1 ] && printf 'on a tty\\033[31m!\\033[0m\\n' || echo 'on pipes'".into(),
        ];
        let (id, out) = run(&terminals, &request);
        assert!(id.0.starts_with("pty_"), "{id}");
        assert_eq!(out.trim_end(), "on a tty!");
    }

    #[test]
    fn without_the_setting_or_a_pty_commands_run_on_pipes() {
        let terminals = AgentTerminals::new(workspace("pipes"), false);
        let (id, out) = run(&terminals, &echo("piped"));
        assert!(id.0.starts_with("term_"), "{id}");
        assert_eq!(out.trim_end(), "piped");
        fn no_pty(_: &CreateTerminalRequest, _: &Path) -> io::Result<Box<dyn PtyHost + Send>> {
            Err(io::Error::other("no PTY here"))
        }
        let terminals = AgentTerminals::with_opener(workspace("fallback"), true, no_pty);
        let (id, out) = run(&terminals, &echo("fell back"));
        assert!(id.0.starts_with("term_"), "{id}");
        assert_eq!(out.trim_end(), "fell back");
    }

    #[test]
    fn a_pty_terminal_stays_inside_the_workspace() {
        let terminals = AgentTerminals::new(workspace("outside"), true);
        let mut request = echo("x");
        request.cwd = Some(std::env::temp_dir().canonicalize().unwrap());
        assert!(terminals.create(&request).is_err());
    }
}
