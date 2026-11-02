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

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use rich_acp::replay::{Transcript, connect};
use rich_acp::*;

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

pub struct FakeAgent(rich_acp::replay::FakeAgent);

impl FakeAgent {
    /// Waits for the transcript to finish; panics with what went wrong.
    pub fn finish(self) {
        if let Err(error) = self.0.finish() {
            panic!("the transcript did not replay: {error}");
        }
    }
}

/// Starts a fake agent replaying `transcript` and a connection to it.
pub fn start(
    transcript: &str,
    workspace: Option<&Path>,
    client: impl Client,
) -> (Connection, FakeAgent) {
    let mut steps = Transcript::parse(transcript).unwrap_or_else(|e| panic!("bad transcript: {e}"));
    if let Some(dir) = workspace {
        steps = steps.var("$WORKSPACE", &dir.display().to_string());
    }
    let (connection, agent) = connect(steps, client).unwrap();
    (connection, FakeAgent(agent))
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
