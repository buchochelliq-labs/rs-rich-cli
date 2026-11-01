//! Errors: [`RpcError`], a JSON-RPC error object either side sends, and
//! [`Error`], what a call on a [`Connection`](crate::Connection) can fail
//! with.

use std::fmt;
use std::io;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A JSON-RPC 2.0 error object: what the agent answered a request with, or
/// what a [`Client`](crate::Client) handler answers the agent with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// The code. ACP's are the associated constants; an unknown one is a
    /// generic failure.
    pub code: i64,
    /// A short description.
    pub message: String,
    /// More, shaped by the code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// The message is not valid JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// Not a valid request, or a request before `initialize`.
    pub const INVALID_REQUEST: i64 = -32600;
    /// The receiver does not implement the method, or did not advertise it.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// Malformed or unsupported parameters.
    pub const INVALID_PARAMS: i64 = -32602;
    /// The receiver failed unexpectedly.
    pub const INTERNAL_ERROR: i64 = -32603;
    /// The request was cancelled.
    pub const REQUEST_CANCELLED: i64 = -32800;
    /// Authenticate first.
    pub const AUTH_REQUIRED: i64 = -32000;
    /// A session, file or terminal named does not exist.
    pub const RESOURCE_NOT_FOUND: i64 = -32002;

    /// An error with a code and a message.
    pub fn new(code: i64, message: impl Into<String>) -> RpcError {
        RpcError {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// The same error with `data`.
    pub fn with_data(mut self, data: Value) -> RpcError {
        self.data = Some(data);
        self
    }

    /// `-32601` for `method`.
    pub fn method_not_found(method: &str) -> RpcError {
        RpcError::new(Self::METHOD_NOT_FOUND, "Method not found")
            .with_data(Value::String(method.to_owned()))
    }

    /// `-32602` with a reason.
    pub fn invalid_params(reason: impl Into<String>) -> RpcError {
        RpcError::new(Self::INVALID_PARAMS, reason)
    }

    /// `-32603` with a reason.
    pub fn internal(reason: impl Into<String>) -> RpcError {
        RpcError::new(Self::INTERNAL_ERROR, reason)
    }

    /// `-32002` with a reason.
    pub fn not_found(reason: impl Into<String>) -> RpcError {
        RpcError::new(Self::RESOURCE_NOT_FOUND, reason)
    }

    /// `-32800`.
    pub fn cancelled() -> RpcError {
        RpcError::new(Self::REQUEST_CANCELLED, "Request cancelled")
    }

    /// Whether the agent asks the client to authenticate first.
    pub fn is_auth_required(&self) -> bool {
        self.code == Self::AUTH_REQUIRED
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

impl From<io::Error> for RpcError {
    fn from(error: io::Error) -> RpcError {
        match error.kind() {
            io::ErrorKind::NotFound => RpcError::not_found(error.to_string()),
            io::ErrorKind::PermissionDenied
            | io::ErrorKind::InvalidInput
            | io::ErrorKind::InvalidData => RpcError::invalid_params(error.to_string()),
            _ => RpcError::internal(error.to_string()),
        }
    }
}

/// What a call on a [`Connection`](crate::Connection) can fail with.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Starting the agent, or reading or writing its streams, failed.
    Io(io::Error),
    /// A message could not be written or a result could not be read as its
    /// type.
    Json(serde_json::Error),
    /// The agent answered with an error.
    Rpc(RpcError),
    /// The agent chose a protocol version this crate does not speak.
    UnsupportedVersion {
        /// What the client offered.
        offered: u16,
        /// What the agent answered.
        answered: u16,
    },
    /// The connection is closed: the agent exited or closed its output.
    Closed,
    /// No answer came in time.
    Timeout,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "agent I/O failed: {error}"),
            Error::Json(error) => write!(f, "bad ACP message: {error}"),
            Error::Rpc(error) => write!(f, "the agent answered: {error}"),
            Error::UnsupportedVersion { offered, answered } => write!(
                f,
                "the agent speaks ACP version {answered}; this client speaks version {offered}"
            ),
            Error::Closed => f.write_str("the agent closed the connection"),
            Error::Timeout => f.write_str("the agent did not answer in time"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(error) => Some(error),
            Error::Json(error) => Some(error),
            Error::Rpc(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Error {
        Error::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Error {
        Error::Json(error)
    }
}

impl From<RpcError> for Error {
    fn from(error: RpcError) -> Error {
        Error::Rpc(error)
    }
}
