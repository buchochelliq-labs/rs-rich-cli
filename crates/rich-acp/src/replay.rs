//! A fake agent that replays a recorded transcript: for testing a client,
//! or showing an app, with no agent installed and no network.
//!
//! A transcript is JSON Lines, one step per line:
//!
//! - `{"agent": {...}}`: the agent writes this message;
//! - `{"client": {...}}`: the client must send a message that
//!   [`matches`] this one (every field given must be equal; fields left
//!   out are not checked);
//! - `{"raw": "..."}`: the agent writes this text as a line, as is (for
//!   messages that are not JSON);
//! - `{"quiet": ms}`: the client must send nothing for this long;
//! - `{"note": "..."}`: ignored.
//!
//! [`Transcript::var`] replaces a placeholder (`$WORKSPACE`, say) in every
//! string of the transcript, so a recording can name the test's own
//! directory.
//!
//! ```
//! use rich_acp::replay::{Transcript, connect};
//! use rich_acp::*;
//!
//! struct Quiet;
//! impl Client for Quiet {
//!     fn request_permission(
//!         &self,
//!         _: RequestPermissionRequest,
//!     ) -> Result<RequestPermissionResponse, RpcError> {
//!         Ok(RequestPermissionResponse::cancelled())
//!     }
//! }
//!
//! let transcript = Transcript::parse(
//!     r#"{"client": {"method": "initialize"}}
//! {"agent": {"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1}}}"#,
//! )
//! .unwrap();
//! let (agent, replay) = connect(transcript, Quiet).unwrap();
//! agent
//!     .initialize(InitializeRequest::new(
//!         ClientCapabilities::none(),
//!         Implementation::new("doc", "0.0.0"),
//!     ))
//!     .unwrap();
//! replay.finish().unwrap();
//! ```

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::Value;

use crate::client::Client;
use crate::connection::Connection;
use crate::error::Error;

/// How long a `client` step waits for the client's message by default.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(10);

/// A recorded conversation, parsed: the steps a fake agent replays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    steps: Vec<Value>,
}

impl Transcript {
    /// Parses JSON Lines, one step per line; blank lines are skipped.
    pub fn parse(text: &str) -> Result<Transcript, Error> {
        let steps = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<Vec<Value>, _>>()?;
        Ok(Transcript { steps })
    }

    /// Reads and parses the file at `path`.
    pub fn read(path: impl AsRef<std::path::Path>) -> Result<Transcript, Error> {
        Transcript::parse(&std::fs::read_to_string(path)?)
    }

    /// Replaces `name` with `value` in every string of every step.
    #[must_use]
    pub fn var(mut self, name: &str, value: &str) -> Transcript {
        fn walk(value: &mut Value, name: &str, with: &str) {
            match value {
                Value::String(text) if text.contains(name) => *text = text.replace(name, with),
                Value::Array(items) => items.iter_mut().for_each(|v| walk(v, name, with)),
                Value::Object(map) => map.values_mut().for_each(|v| walk(v, name, with)),
                _ => {}
            }
        }
        for step in &mut self.steps {
            walk(step, name, value);
        }
        self
    }

    /// The steps, as parsed.
    pub fn steps(&self) -> &[Value] {
        &self.steps
    }

    /// How many steps there are.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Why a replay stopped short: the step (1-based) and what went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayError {
    /// The step that failed, counted from 1 (0: the fake agent panicked).
    pub step: usize,
    /// What happened.
    pub message: String,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "step {}: {}", self.step, self.message)
    }
}

impl std::error::Error for ReplayError {}

/// A fake agent replaying a transcript on a thread of its own.
pub struct FakeAgent {
    replay: JoinHandle<Result<(), ReplayError>>,
}

impl FakeAgent {
    /// Waits for the transcript to end, and says whether every step
    /// replayed.
    pub fn finish(self) -> Result<(), ReplayError> {
        self.replay.join().unwrap_or_else(|_| {
            Err(ReplayError {
                step: 0,
                message: "the fake agent panicked".into(),
            })
        })
    }

    /// Whether the transcript has ended (or failed).
    pub fn is_finished(&self) -> bool {
        self.replay.is_finished()
    }
}

/// Starts a fake agent replaying `transcript`, and a connection from
/// `client` to it over in-process pipes. A `client` step waits
/// [`STEP_TIMEOUT`] for its message.
pub fn connect(
    transcript: Transcript,
    client: impl Client,
) -> Result<(Connection, FakeAgent), Error> {
    connect_with(transcript, client, Some(STEP_TIMEOUT))
}

/// [`connect`], with each `client` step waiting `timeout` for its message,
/// or for ever with `None` (a person typing the prompts the transcript
/// expects).
pub fn connect_with(
    transcript: Transcript,
    client: impl Client,
    timeout: Option<Duration>,
) -> Result<(Connection, FakeAgent), Error> {
    let (from_agent, to_client) = std::io::pipe()?;
    let (from_client, to_agent) = std::io::pipe()?;
    let (lines, received) = mpsc::channel();
    thread::Builder::new()
        .name("acp-replay-input".into())
        .spawn(move || {
            let mut reader = BufReader::new(from_client);
            let mut line = String::new();
            while matches!(reader.read_line(&mut line), Ok(n) if n > 0) {
                if lines.send(line.clone()).is_err() {
                    break;
                }
                line.clear();
            }
        })?;
    let steps = transcript.steps;
    let replay = thread::Builder::new()
        .name("acp-replay".into())
        .spawn(move || replay(steps, to_client, received, timeout))?;
    let connection = Connection::from_streams(from_agent, to_agent, client)?;
    Ok((connection, FakeAgent { replay }))
}

fn replay(
    steps: Vec<Value>,
    mut out: impl Write,
    received: Receiver<String>,
    timeout: Option<Duration>,
) -> Result<(), ReplayError> {
    for (n, step) in steps.into_iter().enumerate() {
        let n = n + 1;
        let fail = |message: String| ReplayError { step: n, message };
        if let Some(message) = step.get("agent") {
            writeln!(out, "{message}").map_err(|e| fail(e.to_string()))?;
            out.flush().map_err(|e| fail(e.to_string()))?;
        } else if let Some(raw) = step.get("raw").and_then(Value::as_str) {
            writeln!(out, "{raw}").map_err(|e| fail(e.to_string()))?;
            out.flush().map_err(|e| fail(e.to_string()))?;
        } else if let Some(expected) = step.get("client") {
            let line = match timeout {
                Some(timeout) => received.recv_timeout(timeout),
                None => received.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            let line = match line {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(fail(format!("nothing came, expected {expected}")));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(fail(format!("the client closed, expected {expected}")));
                }
            };
            let actual: Value =
                serde_json::from_str(&line).map_err(|e| fail(format!("not JSON ({e}): {line}")))?;
            if !matches(expected, &actual) {
                return Err(fail(format!("\n expected {expected}\n      got {actual}")));
            }
        } else if let Some(ms) = step.get("quiet").and_then(Value::as_u64) {
            if let Ok(line) = received.recv_timeout(Duration::from_millis(ms)) {
                return Err(fail(format!("expected nothing, got {}", line.trim())));
            }
        } else if step.get("note").is_none() {
            return Err(fail(format!("unknown step {step}")));
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
