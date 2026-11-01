//! [`Terminals`]: the commands an agent runs through `terminal/*`, on
//! pipes (no PTY), inside a [`Workspace`].
//!
//! Each command's standard output and error are read into one buffer, which
//! keeps at most the request's `outputByteLimit` bytes, dropping the oldest
//! on a character boundary as the protocol asks. A client that wants a PTY
//! (colour, interactive programs) answers `terminal/*` itself, with
//! rs-rich-embed's `PtyHost` for instance.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use crate::error::RpcError;
use crate::schema::{
    CreateTerminalRequest, CreateTerminalResponse, TerminalExitStatus, TerminalId,
    TerminalOutputResponse, TerminalRequest,
};
use crate::workspace::Workspace;

/// The output kept when a request sets no limit: 16 MiB.
pub const DEFAULT_OUTPUT_LIMIT: usize = 16 << 20;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
struct State {
    output: Vec<u8>,
    truncated: bool,
    open_streams: usize,
    exit: Option<TerminalExitStatus>,
}

struct Terminal {
    child: Mutex<Child>,
    state: Mutex<State>,
    changed: Condvar,
    limit: usize,
}

impl State {
    fn push(&mut self, bytes: &[u8], limit: usize) {
        self.output.extend_from_slice(bytes);
        if self.output.len() > limit {
            let mut cut = self.output.len() - limit;
            // Never start inside a UTF-8 sequence.
            while cut < self.output.len() && (self.output[cut] & 0b1100_0000) == 0b1000_0000 {
                cut += 1;
            }
            self.output.drain(..cut);
            self.truncated = true;
        }
    }
}

impl Terminal {
    fn append(&self, bytes: &[u8]) {
        lock(&self.state).push(bytes, self.limit);
        self.changed.notify_all();
    }
}

/// A table of the agent's terminals.
pub struct Terminals {
    workspace: Workspace,
    next: AtomicU64,
    terminals: Mutex<HashMap<TerminalId, Arc<Terminal>>>,
}

impl Terminals {
    /// Terminals whose commands run inside `workspace`.
    pub fn new(workspace: Workspace) -> Terminals {
        Terminals {
            workspace,
            next: AtomicU64::new(1),
            terminals: Mutex::new(HashMap::new()),
        }
    }

    /// `terminal/create`: starts the command and returns at once. Its
    /// `cwd` must be inside the workspace (the workspace's own when
    /// absent). The command's policy (which commands may run at all) is the
    /// client's, before it calls this.
    pub fn create(
        &self,
        request: &CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, RpcError> {
        let cwd = match &request.cwd {
            Some(cwd) => self.workspace.check(cwd)?,
            None => self.workspace.cwd().to_path_buf(),
        };
        let mut command = Command::new(&request.command);
        command
            .args(&request.args)
            .envs(request.env.iter().map(|var| (&var.name, &var.value)))
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            RpcError::internal(format!("cannot run {}: {error}", request.command))
        })?;
        let streams: Vec<Box<dyn Read + Send>> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .collect();
        let limit = request
            .output_byte_limit
            .map_or(DEFAULT_OUTPUT_LIMIT, |limit| {
                usize::try_from(limit).unwrap_or(usize::MAX)
            });
        let terminal = Arc::new(Terminal {
            child: Mutex::new(child),
            state: Mutex::new(State {
                open_streams: streams.len(),
                ..State::default()
            }),
            changed: Condvar::new(),
            limit,
        });
        for mut stream in streams {
            let terminal = Arc::clone(&terminal);
            thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                while let Ok(n) = stream.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    terminal.append(&buffer[..n]);
                }
                lock(&terminal.state).open_streams -= 1;
                terminal.changed.notify_all();
            });
        }
        let watched = Arc::clone(&terminal);
        thread::spawn(move || watch(&watched));
        let id = TerminalId(format!("term_{}", self.next.fetch_add(1, Ordering::SeqCst)));
        lock(&self.terminals).insert(id.clone(), terminal);
        Ok(CreateTerminalResponse { terminal_id: id })
    }

    fn get(&self, id: &TerminalId) -> Result<Arc<Terminal>, RpcError> {
        lock(&self.terminals)
            .get(id)
            .cloned()
            .ok_or_else(|| RpcError::not_found(format!("no terminal {id}")))
    }

    /// `terminal/output`: the output so far, and the exit status once the
    /// command has exited.
    pub fn output(&self, request: &TerminalRequest) -> Result<TerminalOutputResponse, RpcError> {
        let terminal = self.get(&request.terminal_id)?;
        let state = lock(&terminal.state);
        Ok(TerminalOutputResponse {
            output: String::from_utf8_lossy(&state.output).into_owned(),
            truncated: state.truncated,
            exit_status: state.exit.clone(),
        })
    }

    /// `terminal/wait_for_exit`: blocks until the command exits.
    pub fn wait_for_exit(&self, request: &TerminalRequest) -> Result<TerminalExitStatus, RpcError> {
        let terminal = self.get(&request.terminal_id)?;
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

    /// `terminal/kill`: stops the command; the terminal stays readable.
    pub fn kill(&self, request: &TerminalRequest) -> Result<(), RpcError> {
        let terminal = self.get(&request.terminal_id)?;
        let _ = lock(&terminal.child).kill();
        Ok(())
    }

    /// `terminal/release`: stops the command if it still runs and forgets
    /// the terminal.
    pub fn release(&self, request: &TerminalRequest) -> Result<(), RpcError> {
        let terminal = lock(&self.terminals)
            .remove(&request.terminal_id)
            .ok_or_else(|| RpcError::not_found(format!("no terminal {}", request.terminal_id)))?;
        let _ = lock(&terminal.child).kill();
        Ok(())
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        for terminal in lock(&self.terminals).values() {
            let _ = lock(&terminal.child).kill();
        }
    }
}

/// Waits for the command to exit (polling, so `kill` can take the child's
/// lock meanwhile), lets its output drain for a moment, then records how it
/// ended.
fn watch(terminal: &Terminal) {
    let status = loop {
        match lock(&terminal.child).try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        thread::sleep(Duration::from_millis(15));
    };
    let mut state = lock(&terminal.state);
    if state.open_streams > 0 {
        // A background grandchild may hold the pipes open: do not wait on
        // it for long.
        state = terminal
            .changed
            .wait_timeout_while(state, Duration::from_millis(500), |state| {
                state.open_streams > 0
            })
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    state.exit = Some(status.map(exit_status).unwrap_or_default());
    terminal.changed.notify_all();
}

fn exit_status(status: ExitStatus) -> TerminalExitStatus {
    TerminalExitStatus {
        exit_code: status.code().map(|code| code as u32),
        signal: signal_name(status),
    }
}

#[cfg(unix)]
fn signal_name(status: ExitStatus) -> Option<String> {
    use std::os::unix::process::ExitStatusExt;
    let signal = status.signal()?;
    let name = match signal {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        6 => "SIGABRT",
        9 => "SIGKILL",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => return Some(signal.to_string()),
    };
    Some(name.to_owned())
}

#[cfg(not(unix))]
fn signal_name(_status: ExitStatus) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_keeps_utf8_boundaries() {
        let mut state = State::default();
        state.push("aé€".as_bytes(), 4); // 1 + 2 + 3 bytes
        assert!(state.truncated);
        assert_eq!(std::str::from_utf8(&state.output).unwrap(), "€");
        let mut state = State::default();
        state.push(b"abc", 4);
        assert!(!state.truncated);
    }
}
