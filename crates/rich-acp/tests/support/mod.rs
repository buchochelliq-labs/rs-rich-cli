//! A fake agent that replays a transcript, and a client that records what
//! it is told.
//!
//! A transcript (`tests/fixtures/*.jsonl`) is one step per line:
//!
//! - `{"agent": {...}}`: the agent writes this message;
//! - `{"client": {...}}`: the client must send a message that matches this
//!   one (every field given must be equal; fields left out are not
//!   checked);
//! - `{"raw": "..."}`: the agent writes this text as a line, as is (for
//!   messages that are not JSON);
//! - `{"quiet": ms}`: the client must send nothing for this long;
//! - `{"note": "..."}`: ignored.
//!
//! `$WORKSPACE` in a transcript is replaced by the test's workspace.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rich_acp::*;
use serde_json::Value;

const STEP_TIMEOUT: Duration = Duration::from_secs(10);

pub fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A fresh directory for one test.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rich-acp-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

pub struct FakeAgent {
    replay: JoinHandle<Result<(), String>>,
}

impl FakeAgent {
    /// Waits for the transcript to finish; panics with what went wrong.
    pub fn finish(self) {
        match self.replay.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => panic!("the transcript did not replay: {error}"),
            Err(_) => panic!("the fake agent panicked"),
        }
    }
}

/// Starts a fake agent replaying `transcript` and a connection to it.
pub fn start(
    transcript: &str,
    workspace: Option<&Path>,
    client: impl Client,
) -> (Connection, FakeAgent) {
    let text = match workspace {
        Some(dir) => {
            let escaped = serde_json::to_string(&dir.display().to_string()).unwrap();
            transcript.replace("$WORKSPACE", &escaped[1..escaped.len() - 1])
        }
        None => transcript.to_owned(),
    };
    let steps: Vec<Value> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("bad step {line}: {e}")))
        .collect();
    let (from_agent, to_client) = std::io::pipe().unwrap();
    let (from_client, to_agent) = std::io::pipe().unwrap();
    let (lines, received) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(from_client);
        let mut line = String::new();
        while matches!(reader.read_line(&mut line), Ok(n) if n > 0) {
            if lines.send(line.clone()).is_err() {
                break;
            }
            line.clear();
        }
    });
    let replay = thread::spawn(move || replay(steps, to_client, received));
    let connection = Connection::from_streams(from_agent, to_agent, client).unwrap();
    (connection, FakeAgent { replay })
}

fn replay(
    steps: Vec<Value>,
    mut out: impl Write,
    received: Receiver<String>,
) -> Result<(), String> {
    for (n, step) in steps.into_iter().enumerate() {
        let n = n + 1;
        if let Some(message) = step.get("agent") {
            writeln!(out, "{message}").map_err(|e| format!("step {n}: {e}"))?;
            out.flush().map_err(|e| format!("step {n}: {e}"))?;
        } else if let Some(raw) = step.get("raw").and_then(Value::as_str) {
            writeln!(out, "{raw}").map_err(|e| format!("step {n}: {e}"))?;
        } else if let Some(expected) = step.get("client") {
            let line = match received.recv_timeout(STEP_TIMEOUT) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!("step {n}: nothing came, expected {expected}"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("step {n}: the client closed, expected {expected}"));
                }
            };
            let actual: Value = serde_json::from_str(&line)
                .map_err(|e| format!("step {n}: not JSON ({e}): {line}"))?;
            if !matches(expected, &actual) {
                return Err(format!(
                    "step {n}:\n expected {expected}\n      got {actual}"
                ));
            }
        } else if let Some(ms) = step.get("quiet").and_then(Value::as_u64) {
            if let Ok(line) = received.recv_timeout(Duration::from_millis(ms)) {
                return Err(format!("step {n}: expected nothing, got {}", line.trim()));
            }
        } else if step.get("note").is_none() {
            return Err(format!("step {n}: unknown step {step}"));
        }
    }
    Ok(())
}

/// Whether every field of `expected` is in `actual` with the same value
/// (recursively for objects; arrays and scalars exactly).
pub fn matches(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => expected
            .iter()
            .all(|(key, value)| actual.get(key).is_some_and(|actual| matches(value, actual))),
        (Value::Array(expected), Value::Array(actual)) => {
            expected.len() == actual.len()
                && expected.iter().zip(actual).all(|(e, a)| matches(e, a))
        }
        _ => expected == actual,
    }
}

/// How the recorder answers a permission request.
pub enum Answer {
    /// Choose the option with this id.
    Choose(&'static str),
    /// Block until released, then choose this option (for cancellation).
    BlockThen(&'static str),
}

/// A client that records everything and answers from a script.
pub struct Recorder {
    pub updates: Mutex<Vec<SessionNotification>>,
    pub permissions: Mutex<Vec<RequestPermissionRequest>>,
    pub errors: Mutex<Vec<String>>,
    pub logs: Mutex<Vec<String>>,
    pub workspace: Option<Workspace>,
    pub terminals: Option<Terminals>,
    answer: Answer,
    asked: Mutex<Option<Sender<()>>>,
    release: Mutex<Option<Receiver<()>>>,
    release_tx: Mutex<Option<Sender<()>>>,
}

impl Recorder {
    pub fn new(answer: Answer) -> Recorder {
        let (release_tx, release) = mpsc::channel();
        Recorder {
            updates: Mutex::default(),
            permissions: Mutex::default(),
            errors: Mutex::default(),
            logs: Mutex::default(),
            workspace: None,
            terminals: None,
            answer,
            asked: Mutex::new(None),
            release: Mutex::new(Some(release)),
            release_tx: Mutex::new(Some(release_tx)),
        }
    }

    pub fn in_workspace(mut self, dir: &Path) -> Recorder {
        let workspace = Workspace::new(dir).unwrap();
        self.terminals = Some(Terminals::new(workspace.clone()));
        self.workspace = Some(workspace);
        self
    }

    /// A channel told when a permission request arrives.
    pub fn on_ask(&self) -> Receiver<()> {
        let (tx, rx) = mpsc::channel();
        *self.asked.lock().unwrap() = Some(tx);
        rx
    }

    /// Lets a blocked permission handler answer.
    pub fn release(&self) {
        if let Some(tx) = self.release_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    pub fn updates(&self) -> Vec<SessionUpdate> {
        self.updates
            .lock()
            .unwrap()
            .iter()
            .map(|n| n.update.clone())
            .collect()
    }

    /// The agent's message text, chunks joined.
    pub fn message_text(&self) -> String {
        self.updates()
            .iter()
            .filter_map(|update| match update {
                SessionUpdate::AgentMessageChunk(chunk) => {
                    chunk.content.as_text().map(str::to_owned)
                }
                _ => None,
            })
            .collect()
    }

    fn terminals(&self) -> Result<&Terminals, RpcError> {
        self.terminals
            .as_ref()
            .ok_or_else(|| RpcError::internal("no terminals"))
    }

    fn workspace(&self) -> Result<&Workspace, RpcError> {
        self.workspace
            .as_ref()
            .ok_or_else(|| RpcError::internal("no workspace"))
    }
}

impl Client for Recorder {
    fn session_update(&self, notification: SessionNotification) {
        self.updates.lock().unwrap().push(notification);
    }

    fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, RpcError> {
        self.permissions.lock().unwrap().push(request);
        if let Some(tx) = self.asked.lock().unwrap().as_ref() {
            let _ = tx.send(());
        }
        let option = match self.answer {
            Answer::Choose(option) => option,
            Answer::BlockThen(option) => {
                let release = self.release.lock().unwrap().take();
                if let Some(release) = release {
                    let _ = release.recv();
                }
                option
            }
        };
        Ok(RequestPermissionResponse::selected(option))
    }

    fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> Result<ReadTextFileResponse, RpcError> {
        fs::read_text_file(self.workspace()?, &request)
    }

    fn write_text_file(
        &self,
        request: WriteTextFileRequest,
    ) -> Result<WriteTextFileResponse, RpcError> {
        fs::write_text_file(self.workspace()?, &request)
    }

    fn create_terminal(
        &self,
        request: CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, RpcError> {
        self.terminals()?.create(&request)
    }

    fn terminal_output(
        &self,
        request: TerminalRequest,
    ) -> Result<TerminalOutputResponse, RpcError> {
        self.terminals()?.output(&request)
    }

    fn wait_for_terminal_exit(
        &self,
        request: TerminalRequest,
    ) -> Result<TerminalExitStatus, RpcError> {
        self.terminals()?.wait_for_exit(&request)
    }

    fn kill_terminal(&self, request: TerminalRequest) -> Result<(), RpcError> {
        self.terminals()?.kill(&request)
    }

    fn release_terminal(&self, request: TerminalRequest) -> Result<(), RpcError> {
        self.terminals()?.release(&request)
    }

    fn agent_log(&self, line: &str) {
        self.logs.lock().unwrap().push(line.to_owned());
    }

    fn protocol_error(&self, error: &Error) {
        self.errors.lock().unwrap().push(error.to_string());
    }
}

pub fn shared(recorder: Recorder) -> Arc<Recorder> {
    Arc::new(recorder)
}

pub fn client_info() -> Implementation {
    Implementation::new("rich-acp-tests", "0.0.0")
}
