//! [`Client`]: what the app implements to answer the agent.

use std::sync::Arc;

use serde_json::Value;

use crate::error::{Error, RpcError};
use crate::schema::{
    CreateTerminalRequest, CreateTerminalResponse, ReadTextFileRequest, ReadTextFileResponse,
    RequestPermissionRequest, RequestPermissionResponse, SessionNotification, TerminalExitStatus,
    TerminalOutputResponse, TerminalRequest, WriteTextFileRequest, WriteTextFileResponse,
};

/// The client side of a connection: the app's answers to the agent.
///
/// [`session_update`](Client::session_update) is called on the
/// connection's reader thread, in the order the notifications arrive, so it
/// should hand the update to the app's own loop and return. Every request
/// from the agent runs on a thread of its own, so a handler may block (on
/// the user, for a permission prompt, or on a command, for
/// `terminal/wait_for_exit`) without holding up the stream.
///
/// Only [`request_permission`](Client::request_permission) must be
/// written. The file and terminal handlers answer "method not found" until
/// they are, and the connection refuses them anyway unless the matching
/// capability was offered in `initialize`
/// ([`ClientCapabilities`](crate::ClientCapabilities)).
pub trait Client: Send + Sync + 'static {
    /// A `session/update` notification: a message chunk, a tool call or its
    /// update, a plan, ….
    fn session_update(&self, notification: SessionNotification) {
        let _ = notification;
    }

    /// `session/request_permission`: ask the user. If the app cancels the
    /// turn with [`Connection::cancel`](crate::Connection::cancel) while
    /// this is waiting, the connection answers `cancelled` itself and this
    /// handler's answer is dropped.
    fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, RpcError>;

    /// `fs/read_text_file` (needs `fs.readTextFile`). See
    /// [`fs::read_text_file`](crate::fs::read_text_file).
    fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> Result<ReadTextFileResponse, RpcError> {
        let _ = request;
        Err(RpcError::method_not_found("fs/read_text_file"))
    }

    /// `fs/write_text_file` (needs `fs.writeTextFile`). See
    /// [`fs::write_text_file`](crate::fs::write_text_file).
    fn write_text_file(
        &self,
        request: WriteTextFileRequest,
    ) -> Result<WriteTextFileResponse, RpcError> {
        let _ = request;
        Err(RpcError::method_not_found("fs/write_text_file"))
    }

    /// `terminal/create` (needs `terminal`). See
    /// [`Terminals`](crate::Terminals).
    fn create_terminal(
        &self,
        request: CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, RpcError> {
        let _ = request;
        Err(RpcError::method_not_found("terminal/create"))
    }

    /// `terminal/output`.
    fn terminal_output(
        &self,
        request: TerminalRequest,
    ) -> Result<TerminalOutputResponse, RpcError> {
        let _ = request;
        Err(RpcError::method_not_found("terminal/output"))
    }

    /// `terminal/wait_for_exit`.
    fn wait_for_terminal_exit(
        &self,
        request: TerminalRequest,
    ) -> Result<TerminalExitStatus, RpcError> {
        let _ = request;
        Err(RpcError::method_not_found("terminal/wait_for_exit"))
    }

    /// `terminal/kill`: stop the command, keep the terminal.
    fn kill_terminal(&self, request: TerminalRequest) -> Result<(), RpcError> {
        let _ = request;
        Err(RpcError::method_not_found("terminal/kill"))
    }

    /// `terminal/release`: stop the command if it runs, forget the
    /// terminal.
    fn release_terminal(&self, request: TerminalRequest) -> Result<(), RpcError> {
        let _ = request;
        Err(RpcError::method_not_found("terminal/release"))
    }

    /// Any other request: extensions (`_`-prefixed) and methods this crate
    /// does not know.
    fn ext_method(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let _ = params;
        Err(RpcError::method_not_found(method))
    }

    /// Any other notification.
    fn ext_notification(&self, method: &str, params: Value) {
        let _ = (method, params);
    }

    /// A line the agent wrote to its standard error (a spawned agent only).
    fn agent_log(&self, line: &str) {
        let _ = line;
    }

    /// A message from the agent that could not be read: not JSON, not
    /// JSON-RPC, or a notification whose parameters do not fit. The
    /// connection skips it and carries on.
    fn protocol_error(&self, error: &Error) {
        let _ = error;
    }
}

impl<C: Client> Client for Arc<C> {
    fn session_update(&self, notification: SessionNotification) {
        (**self).session_update(notification)
    }
    fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, RpcError> {
        (**self).request_permission(request)
    }
    fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> Result<ReadTextFileResponse, RpcError> {
        (**self).read_text_file(request)
    }
    fn write_text_file(
        &self,
        request: WriteTextFileRequest,
    ) -> Result<WriteTextFileResponse, RpcError> {
        (**self).write_text_file(request)
    }
    fn create_terminal(
        &self,
        request: CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, RpcError> {
        (**self).create_terminal(request)
    }
    fn terminal_output(
        &self,
        request: TerminalRequest,
    ) -> Result<TerminalOutputResponse, RpcError> {
        (**self).terminal_output(request)
    }
    fn wait_for_terminal_exit(
        &self,
        request: TerminalRequest,
    ) -> Result<TerminalExitStatus, RpcError> {
        (**self).wait_for_terminal_exit(request)
    }
    fn kill_terminal(&self, request: TerminalRequest) -> Result<(), RpcError> {
        (**self).kill_terminal(request)
    }
    fn release_terminal(&self, request: TerminalRequest) -> Result<(), RpcError> {
        (**self).release_terminal(request)
    }
    fn ext_method(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        (**self).ext_method(method, params)
    }
    fn ext_notification(&self, method: &str, params: Value) {
        (**self).ext_notification(method, params)
    }
    fn agent_log(&self, line: &str) {
        (**self).agent_log(line)
    }
    fn protocol_error(&self, error: &Error) {
        (**self).protocol_error(error)
    }
}
