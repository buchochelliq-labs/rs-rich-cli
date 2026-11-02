//! [`Connection`]: JSON-RPC 2.0 to an agent, one message per line.
//!
//! One reader thread parses what the agent writes and routes it: a response
//! to the [`Pending`] call waiting on it, a `session/update` to
//! [`Client::session_update`] in arrival order, and each request from the
//! agent to a thread of its own, whose answer is written back when the
//! handler returns. Writes from any thread are serialised by a lock; a
//! message is one line because serde_json escapes every newline.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::marker::PhantomData;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::client::Client;
use crate::error::{Error, RpcError};
use crate::schema::{
    AgentCapabilities, AuthMethodId, AuthenticateRequest, CancelNotification, ClientCapabilities,
    InitializeRequest, InitializeResponse, LoadSessionRequest, LoadSessionResponse,
    NewSessionRequest, NewSessionResponse, PROTOCOL_VERSION, PromptRequest, PromptResponse,
    RequestPermissionResponse, SessionId, SessionNotification, SetSessionModeRequest,
};

type Reply = Result<Value, RpcError>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A permission request the client has not answered yet.
struct OpenPermission {
    id: Value,
    session_id: SessionId,
}

struct Shared {
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    pending: Mutex<HashMap<u64, Sender<Reply>>>,
    next_id: AtomicU64,
    closed: AtomicBool,
    offered: Mutex<ClientCapabilities>,
    agent: Mutex<Option<AgentCapabilities>>,
    permissions: Mutex<HashMap<String, OpenPermission>>,
    client: Arc<dyn Client>,
}

impl Shared {
    fn write(&self, message: &Value) -> Result<(), Error> {
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        let mut writer = lock(&self.writer);
        let Some(writer) = writer.as_mut() else {
            return Err(Error::Closed);
        };
        writer.write_all(&line)?;
        writer.flush()?;
        Ok(())
    }

    fn respond(&self, id: Value, reply: Reply) {
        let message = match reply {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        };
        // The agent may be gone; there is no one left to tell.
        let _ = self.write(&message);
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        // Dropping the senders wakes every waiting call with `Closed`.
        lock(&self.pending).clear();
    }
}

/// A connection to an agent: the client end of ACP.
///
/// Calls block until the agent answers; share the connection behind an
/// [`Arc`] to call [`cancel`](Connection::cancel) from another thread while
/// a [`prompt`](Connection::prompt) waits, or use
/// [`start_prompt`](Connection::start_prompt). Dropping it closes the
/// agent's input and, for a spawned agent, waits a moment for it to exit
/// before killing it.
pub struct Connection {
    shared: Arc<Shared>,
    child: Mutex<Option<Child>>,
}

impl Connection {
    /// Starts `command` as the agent, with its standard input and output
    /// piped to this connection. Its standard error is read line by line
    /// into [`Client::agent_log`], so it never reaches the terminal.
    pub fn spawn(mut command: Command, client: impl Client) -> Result<Connection, Error> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let missing = || Error::Io(io::Error::other("the agent's pipes were not opened"));
        let stdin: ChildStdin = child.stdin.take().ok_or_else(missing)?;
        let stdout = child.stdout.take().ok_or_else(missing)?;
        let stderr = child.stderr.take().ok_or_else(missing)?;
        let client: Arc<dyn Client> = Arc::new(client);
        let logs = Arc::clone(&client);
        thread::Builder::new()
            .name("acp-agent-stderr".into())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = Vec::new();
                while matches!(reader.read_until(b'\n', &mut line), Ok(n) if n > 0) {
                    let text = String::from_utf8_lossy(&line);
                    logs.agent_log(text.trim_end_matches(['\r', '\n']));
                    line.clear();
                }
            })?;
        let connection = Connection::start(stdout, stdin, client)?;
        *lock(&connection.child) = Some(child);
        Ok(connection)
    }

    /// A connection over any pair of streams: what the agent writes is
    /// read from `reader`, what the client sends goes to `writer`.
    pub fn from_streams(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        client: impl Client,
    ) -> Result<Connection, Error> {
        Connection::start(reader, writer, Arc::new(client))
    }

    fn start(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        client: Arc<dyn Client>,
    ) -> Result<Connection, Error> {
        let shared = Arc::new(Shared {
            writer: Mutex::new(Some(Box::new(writer))),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            offered: Mutex::new(ClientCapabilities::none()),
            agent: Mutex::new(None),
            permissions: Mutex::new(HashMap::new()),
            client,
        });
        let reading = Arc::clone(&shared);
        thread::Builder::new()
            .name("acp-reader".into())
            .spawn(move || read_loop(reader, reading))?;
        Ok(Connection {
            shared,
            child: Mutex::new(None),
        })
    }

    /// `initialize`: offers `request`'s protocol version and capabilities.
    /// The capabilities offered here are the only ones the agent may use:
    /// a file or terminal request they do not cover is refused with
    /// "method not found" before the client sees it. An agent that answers
    /// with a version this crate does not speak is refused with
    /// [`Error::UnsupportedVersion`].
    pub fn initialize(&self, request: InitializeRequest) -> Result<InitializeResponse, Error> {
        *lock(&self.shared.offered) = request.client_capabilities;
        let offered = request.protocol_version;
        let response: InitializeResponse = self.request("initialize", &request)?.wait()?;
        if response.protocol_version != PROTOCOL_VERSION {
            return Err(Error::UnsupportedVersion {
                offered,
                answered: response.protocol_version,
            });
        }
        *lock(&self.shared.agent) = Some(response.agent_capabilities.clone());
        Ok(response)
    }

    /// `authenticate` with one of the methods `initialize` advertised.
    pub fn authenticate(&self, method_id: impl Into<AuthMethodId>) -> Result<(), Error> {
        let request = AuthenticateRequest {
            method_id: method_id.into(),
        };
        self.request::<Value>("authenticate", &request)?
            .wait()
            .map(drop)
    }

    /// `session/new`.
    pub fn new_session(&self, request: NewSessionRequest) -> Result<NewSessionResponse, Error> {
        self.request("session/new", &request)?.wait()
    }

    /// `session/load` (only when the agent advertised `loadSession`): the
    /// history arrives as `session/update` notifications before this
    /// returns.
    pub fn load_session(&self, request: LoadSessionRequest) -> Result<LoadSessionResponse, Error> {
        self.request("session/load", &request)?.wait()
    }

    /// `session/set_mode`.
    pub fn set_session_mode(&self, request: SetSessionModeRequest) -> Result<(), Error> {
        self.request::<Value>("session/set_mode", &request)?
            .wait()
            .map(drop)
    }

    /// `session/prompt`: sends the user's message and blocks until the turn
    /// ends. Updates arrive at [`Client::session_update`] meanwhile.
    pub fn prompt(&self, request: PromptRequest) -> Result<PromptResponse, Error> {
        self.start_prompt(request)?.wait()
    }

    /// `session/prompt` without waiting: the turn's end is the
    /// [`Pending`]'s answer.
    pub fn start_prompt(&self, request: PromptRequest) -> Result<Pending<PromptResponse>, Error> {
        self.request("session/prompt", &request)
    }

    /// `session/cancel`: asks the agent to stop the session's turn, which
    /// then ends with [`StopReason::Cancelled`](crate::StopReason). Every
    /// permission request of that session still unanswered is answered
    /// `cancelled` here, as the protocol requires; the client's own answer
    /// to it, when it comes, is dropped.
    pub fn cancel(&self, session_id: &SessionId) -> Result<(), Error> {
        let notification = CancelNotification {
            session_id: session_id.clone(),
        };
        self.notify("session/cancel", &notification)?;
        let open: Vec<OpenPermission> = {
            let mut permissions = lock(&self.shared.permissions);
            let keys: Vec<String> = permissions
                .iter()
                .filter(|(_, open)| &open.session_id == session_id)
                .map(|(key, _)| key.clone())
                .collect();
            keys.iter()
                .filter_map(|key| permissions.remove(key))
                .collect()
        };
        for permission in open {
            let answer = serde_json::to_value(RequestPermissionResponse::cancelled())?;
            self.shared.respond(permission.id, Ok(answer));
        }
        Ok(())
    }

    /// Sends a request and returns at once; [`Pending::wait`] reads the
    /// answer as `R`. For methods this crate has no call for (extensions
    /// start with `_`).
    pub fn request<R: DeserializeOwned>(
        &self,
        method: &str,
        params: &impl Serialize,
    ) -> Result<Pending<R>, Error> {
        if self.is_closed() {
            return Err(Error::Closed);
        }
        let id = self.shared.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        lock(&self.shared.pending).insert(id, sender);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": serde_json::to_value(params)?,
        });
        if let Err(error) = self.shared.write(&message) {
            lock(&self.shared.pending).remove(&id);
            return Err(error);
        }
        Ok(Pending {
            id,
            receiver,
            _answer: PhantomData,
        })
    }

    /// Sends a notification.
    pub fn notify(&self, method: &str, params: &impl Serialize) -> Result<(), Error> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": serde_json::to_value(params)?,
        });
        self.shared.write(&message)
    }

    /// The capabilities offered in `initialize` (none before it).
    pub fn client_capabilities(&self) -> ClientCapabilities {
        *lock(&self.shared.offered)
    }

    /// The agent's capabilities, once `initialize` has succeeded.
    pub fn agent_capabilities(&self) -> Option<AgentCapabilities> {
        lock(&self.shared.agent).clone()
    }

    /// Whether the agent has closed its output (or exited).
    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::SeqCst)
    }

    /// Closes the agent's input and waits up to `grace` for a spawned agent
    /// to exit, killing it after. Returns its exit status (`None` for a
    /// connection over streams).
    pub fn shutdown(&self, grace: Duration) -> Result<Option<ExitStatus>, Error> {
        lock(&self.shared.writer).take();
        let Some(mut child) = lock(&self.child).take() else {
            return Ok(None);
        };
        let deadline = Instant::now() + grace;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Ok(Some(child.wait()?));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.shutdown(Duration::from_secs(2));
    }
}

/// A request sent, its answer still to come.
#[must_use = "a request's answer is read with `wait`"]
pub struct Pending<R> {
    id: u64,
    receiver: Receiver<Reply>,
    _answer: PhantomData<fn() -> R>,
}

impl<R: DeserializeOwned> Pending<R> {
    /// The request's JSON-RPC id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Blocks until the agent answers, or the connection closes.
    pub fn wait(self) -> Result<R, Error> {
        match self.receiver.recv() {
            Ok(reply) => decode(reply),
            Err(_) => Err(Error::Closed),
        }
    }

    /// Blocks until the agent answers or `timeout` passes
    /// ([`Error::Timeout`], with the request still pending: wait again).
    pub fn wait_timeout(&self, timeout: Duration) -> Result<R, Error> {
        match self.receiver.recv_timeout(timeout) {
            Ok(reply) => decode(reply),
            Err(RecvTimeoutError::Timeout) => Err(Error::Timeout),
            Err(RecvTimeoutError::Disconnected) => Err(Error::Closed),
        }
    }
}

fn decode<R: DeserializeOwned>(reply: Reply) -> Result<R, Error> {
    Ok(serde_json::from_value(reply?)?)
}

fn read_loop(reader: impl Read, shared: Arc<Shared>) {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(text) {
            Ok(message) => route(&shared, message),
            Err(error) => shared.client.protocol_error(&Error::Json(error)),
        }
    }
    shared.close();
}

fn route(shared: &Arc<Shared>, mut message: Value) {
    let Some(object) = message.as_object_mut() else {
        shared
            .client
            .protocol_error(&Error::Json(serde::de::Error::custom(
                "a message must be an object",
            )));
        return;
    };
    let id = object.remove("id");
    let method = object
        .remove("method")
        .and_then(|m| m.as_str().map(str::to_owned));
    let params = object.remove("params").unwrap_or(Value::Null);
    match (method, id) {
        (Some(method), Some(id)) => incoming_request(shared, id, method, params),
        (Some(method), None) => incoming_notification(shared, &method, params),
        (None, Some(id)) => {
            let reply = match (object.remove("result"), object.remove("error")) {
                (_, Some(error)) => Err(serde_json::from_value(error)
                    .unwrap_or_else(|_| RpcError::internal("the agent sent a malformed error"))),
                (Some(result), None) => Ok(result),
                (None, None) => Ok(Value::Null),
            };
            let sender = id.as_u64().and_then(|id| lock(&shared.pending).remove(&id));
            match sender {
                Some(sender) => {
                    let _ = sender.send(reply);
                }
                None => shared
                    .client
                    .protocol_error(&Error::Json(serde::de::Error::custom(format!(
                        "a response to an unknown request: {id}"
                    )))),
            }
        }
        (None, None) => shared
            .client
            .protocol_error(&Error::Json(serde::de::Error::custom(
                "neither a request nor a response",
            ))),
    }
}

fn incoming_notification(shared: &Shared, method: &str, params: Value) {
    match method {
        "session/update" => match serde_json::from_value::<SessionNotification>(params) {
            Ok(notification) => shared.client.session_update(notification),
            Err(error) => shared.client.protocol_error(&Error::Json(error)),
        },
        // The agent withdrawing a request of its own: every handler here
        // answers in the end, which the protocol allows.
        "$/cancel_request" => {}
        _ => shared.client.ext_notification(method, params),
    }
}

const PERMISSION: &str = "session/request_permission";

fn incoming_request(shared: &Arc<Shared>, id: Value, method: String, params: Value) {
    if !lock(&shared.offered).allows(&method) {
        shared.respond(id, Err(RpcError::method_not_found(&method)));
        return;
    }
    let key = id.to_string();
    if method == PERMISSION {
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .map(SessionId::from)
            .unwrap_or_default();
        lock(&shared.permissions).insert(
            key.clone(),
            OpenPermission {
                id: id.clone(),
                session_id,
            },
        );
    }
    let handling = Arc::clone(shared);
    let answer_id = id.clone();
    let spawned = thread::Builder::new()
        .name("acp-request".into())
        .spawn(move || {
            let reply = dispatch(&*handling.client, &method, params);
            // A cancelled permission request has been answered already.
            if method == PERMISSION && lock(&handling.permissions).remove(&key).is_none() {
                return;
            }
            handling.respond(answer_id, reply);
        });
    if spawned.is_err() {
        shared.respond(
            id,
            Err(RpcError::internal("the client could not start a handler")),
        );
    }
}

fn call<P: DeserializeOwned, R: Serialize>(
    params: Value,
    handler: impl FnOnce(P) -> Result<R, RpcError>,
) -> Reply {
    let params = serde_json::from_value(params)
        .map_err(|error| RpcError::invalid_params(error.to_string()))?;
    let result = handler(params)?;
    serde_json::to_value(result).map_err(|error| RpcError::internal(error.to_string()))
}

fn empty(result: Result<(), RpcError>) -> Result<Value, RpcError> {
    result.map(|()| json!({}))
}

fn dispatch(client: &dyn Client, method: &str, params: Value) -> Reply {
    match method {
        PERMISSION => call(params, |p| client.request_permission(p)),
        "fs/read_text_file" => call(params, |p| client.read_text_file(p)),
        "fs/write_text_file" => call(params, |p| client.write_text_file(p)),
        "terminal/create" => call(params, |p| client.create_terminal(p)),
        "terminal/output" => call(params, |p| client.terminal_output(p)),
        "terminal/wait_for_exit" => call(params, |p| client.wait_for_terminal_exit(p)),
        "terminal/kill" => call(params, |p| empty(client.kill_terminal(p))),
        "terminal/release" => call(params, |p| empty(client.release_terminal(p))),
        _ => client.ext_method(method, params),
    }
}
