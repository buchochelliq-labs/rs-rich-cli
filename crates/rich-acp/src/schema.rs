//! The ACP version 1 messages a client sends and receives, as serde types.
//!
//! Written from the public specification (agentclientprotocol.com, schema
//! 1.15.0). Field names follow the wire's `camelCase`; discriminators its
//! `snake_case`. Reading is tolerant, as the spec's evolution rules need:
//!
//! - unknown fields are ignored;
//! - an unknown variant of a tagged union ([`SessionUpdate`],
//!   [`ContentBlock`], [`ToolCallContent`]) arrives as `Unknown`, with its
//!   tag and the raw JSON;
//! - an unknown string in an open enum ([`ToolKind`], [`StopReason`], …)
//!   arrives as `Unknown(String)`, kept as sent;
//! - an omitted optional field and `null` both read as `None`.

use std::fmt;
use std::path::PathBuf;

use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// The protocol version this crate speaks: ACP's major version 1.
pub const PROTOCOL_VERSION: u16 = 1;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// The identifier as a string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(id.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// A session, as the agent named it in `session/new`.
    SessionId
);
id_type!(
    /// A tool call, unique within its session.
    ToolCallId
);
id_type!(
    /// A terminal, as the client named it in `terminal/create`.
    TerminalId
);
id_type!(
    /// One of a permission request's options.
    PermissionOptionId
);
id_type!(
    /// An authentication method the agent advertised.
    AuthMethodId
);
id_type!(
    /// A session mode the agent advertised.
    SessionModeId
);

macro_rules! open_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $wire:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vm])* $variant,)*
            /// A value this version does not know, kept as sent.
            Unknown(String),
        }

        impl $name {
            /// The value as it is written on the wire.
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $wire,)*
                    Self::Unknown(value) => value,
                }
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                match value {
                    $($wire => Self::$variant,)*
                    other => Self::Unknown(other.to_owned()),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Ok(Self::from(value.as_str()))
            }
        }
    };
}

/// Writes a variant's struct with its tag first.
fn tagged<T: Serialize, S: Serializer>(
    tag: &str,
    kind: &str,
    inner: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = Map::new();
    map.insert(tag.to_owned(), Value::String(kind.to_owned()));
    match serde_json::to_value(inner).map_err(S::Error::custom)? {
        Value::Object(fields) => {
            for (key, value) in fields {
                if key != tag {
                    map.insert(key, value);
                }
            }
        }
        _ => return Err(S::Error::custom("a tagged variant must be an object")),
    }
    Value::Object(map).serialize(serializer)
}

macro_rules! tagged_enum {
    ($(#[$m:meta])* $name:ident, tag = $tag:literal {
        $($(#[$vm:meta])* $variant:ident($ty:ty) = $wire:literal),* $(,)?
    }) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $($(#[$vm])* $variant($ty),)*
            /// A variant this version does not know: its tag and the
            /// message as sent.
            Unknown {
                /// The tag's value.
                kind: String,
                /// The whole object, tag included.
                value: Value,
            },
        }

        impl $name {
            /// The tag's value on the wire.
            pub fn kind(&self) -> &str {
                match self {
                    $(Self::$variant(_) => $wire,)*
                    Self::Unknown { kind, .. } => kind,
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self {
                    $(Self::$variant(inner) => tagged($tag, $wire, inner, serializer),)*
                    Self::Unknown { value, .. } => value.serialize(serializer),
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = Value::deserialize(deserializer)?;
                let kind = value
                    .get($tag)
                    .and_then(Value::as_str)
                    .ok_or_else(|| D::Error::custom(concat!("missing `", $tag, "`")))?
                    .to_owned();
                match kind.as_str() {
                    $($wire => serde_json::from_value(value)
                        .map(Self::$variant)
                        .map_err(D::Error::custom),)*
                    _ => Ok(Self::Unknown { kind, value }),
                }
            }
        }
    };
}

// ---------------------------------------------------------------------------
// initialize, authenticate

/// A program's name and version, as `clientInfo` and `agentInfo` carry
/// them. Self-reported: never a basis for a security decision.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Implementation {
    /// The programmatic name.
    pub name: String,
    /// A name for people, when it differs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The version.
    pub version: String,
}

impl Implementation {
    /// A name and version, with no title.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Implementation {
        Implementation {
            name: name.into(),
            title: None,
            version: version.into(),
        }
    }

    /// The title if there is one, else the name.
    pub fn display_name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// The file-system methods a client offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemCapabilities {
    /// `fs/read_text_file` is available.
    #[serde(default)]
    pub read_text_file: bool,
    /// `fs/write_text_file` is available.
    #[serde(default)]
    pub write_text_file: bool,
}

/// What a client offers the agent. The default offers nothing: every
/// capability is opt-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    /// File reads and writes.
    #[serde(default)]
    pub fs: FileSystemCapabilities,
    /// All the `terminal/*` methods.
    #[serde(default)]
    pub terminal: bool,
}

impl ClientCapabilities {
    /// Nothing offered (the same as `default()`).
    pub fn none() -> ClientCapabilities {
        ClientCapabilities::default()
    }

    /// Offer `fs/read_text_file`.
    pub fn read_files(mut self, on: bool) -> ClientCapabilities {
        self.fs.read_text_file = on;
        self
    }

    /// Offer `fs/write_text_file`.
    pub fn write_files(mut self, on: bool) -> ClientCapabilities {
        self.fs.write_text_file = on;
        self
    }

    /// Offer the `terminal/*` methods.
    pub fn terminals(mut self, on: bool) -> ClientCapabilities {
        self.terminal = on;
        self
    }

    /// Whether the agent may call `method` under these capabilities. Methods
    /// that are not gated by a capability (`session/request_permission`,
    /// extensions) are allowed.
    pub fn allows(&self, method: &str) -> bool {
        match method {
            "fs/read_text_file" => self.fs.read_text_file,
            "fs/write_text_file" => self.fs.write_text_file,
            _ if method.starts_with("terminal/") => self.terminal,
            _ => true,
        }
    }
}

/// `initialize`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    /// The newest version the client speaks.
    pub protocol_version: u16,
    /// What the client offers.
    #[serde(default)]
    pub client_capabilities: ClientCapabilities,
    /// The client's name and version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_info: Option<Implementation>,
}

impl InitializeRequest {
    /// Version 1, the given capabilities, and this client's name.
    pub fn new(capabilities: ClientCapabilities, client: Implementation) -> InitializeRequest {
        InitializeRequest {
            protocol_version: PROTOCOL_VERSION,
            client_capabilities: capabilities,
            client_info: Some(client),
        }
    }
}

/// The content types an agent accepts in a prompt beyond text and resource
/// links.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCapabilities {
    /// `image` blocks.
    #[serde(default)]
    pub image: bool,
    /// `audio` blocks.
    #[serde(default)]
    pub audio: bool,
    /// `resource` (embedded) blocks.
    #[serde(default)]
    pub embedded_context: bool,
}

/// The MCP transports an agent connects to beyond stdio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct McpCapabilities {
    /// HTTP.
    #[serde(default)]
    pub http: bool,
    /// SSE (deprecated by MCP).
    #[serde(default)]
    pub sse: bool,
}

/// What an agent supports.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    /// `session/load` is available.
    #[serde(default)]
    pub load_session: bool,
    /// Prompt content beyond the baseline.
    #[serde(default)]
    pub prompt_capabilities: PromptCapabilities,
    /// MCP transports beyond stdio.
    #[serde(default)]
    pub mcp_capabilities: McpCapabilities,
    /// Further session methods (`resume`, `close`, `list`, …), as sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_capabilities: Option<Value>,
}

/// An authentication method an agent offers.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethod {
    /// Its id, for `authenticate`.
    pub id: AuthMethodId,
    /// A name for people.
    pub name: String,
    /// More about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `agent` (the default when absent: the agent handles it through
    /// `authenticate`) or `terminal`.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// `initialize`'s result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    /// The version the agent chose.
    pub protocol_version: u16,
    /// What the agent supports.
    #[serde(default)]
    pub agent_capabilities: AgentCapabilities,
    /// How to authenticate, if the agent needs it.
    #[serde(default, deserialize_with = "null_as_default")]
    pub auth_methods: Vec<AuthMethod>,
    /// The agent's name and version (self-reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_info: Option<Implementation>,
}

/// `authenticate`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateRequest {
    /// One of the methods `initialize` advertised.
    pub method_id: AuthMethodId,
}

// ---------------------------------------------------------------------------
// sessions

/// An environment variable, for MCP servers and terminals.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EnvVariable {
    /// Its name.
    pub name: String,
    /// Its value.
    pub value: String,
}

impl EnvVariable {
    /// A name and a value.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> EnvVariable {
        EnvVariable {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// An HTTP header, for MCP servers over HTTP or SSE.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HttpHeader {
    /// Its name.
    pub name: String,
    /// Its value.
    pub value: String,
}

/// An MCP server for the agent to connect to. `kind` is `None` for stdio
/// (the default transport, which every agent supports), else `"http"` or
/// `"sse"` with a `url`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    /// `http` or `sse`; `None` for stdio.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// A name for people.
    pub name: String,
    /// The server's executable (stdio): an absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<PathBuf>,
    /// Its arguments (stdio).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Its environment (stdio).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<EnvVariable>>,
    /// Its URL (HTTP, SSE).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Headers to send (HTTP, SSE).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<HttpHeader>>,
}

impl McpServer {
    /// A server the agent runs over stdio.
    pub fn stdio(
        name: impl Into<String>,
        command: impl Into<PathBuf>,
        args: Vec<String>,
    ) -> McpServer {
        McpServer {
            name: name.into(),
            command: Some(command.into()),
            args: Some(args),
            env: Some(Vec::new()),
            ..McpServer::default()
        }
    }

    /// A server over HTTP (needs the agent's `mcpCapabilities.http`).
    pub fn http(name: impl Into<String>, url: impl Into<String>) -> McpServer {
        McpServer {
            kind: Some("http".to_owned()),
            name: name.into(),
            url: Some(url.into()),
            headers: Some(Vec::new()),
            ..McpServer::default()
        }
    }
}

/// `session/new`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionRequest {
    /// The session's working directory: absolute, and the first root of its
    /// root set.
    pub cwd: PathBuf,
    /// MCP servers for the agent to connect to.
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

impl NewSessionRequest {
    /// A session in `cwd` with no MCP servers.
    pub fn new(cwd: impl Into<PathBuf>) -> NewSessionRequest {
        NewSessionRequest {
            cwd: cwd.into(),
            mcp_servers: Vec::new(),
        }
    }
}

/// A mode a session can be in.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMode {
    /// Its id.
    pub id: SessionModeId,
    /// A name for people.
    pub name: String,
    /// More about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The modes a session offers and the one it is in.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModeState {
    /// The current mode.
    pub current_mode_id: SessionModeId,
    /// Every mode.
    #[serde(default)]
    pub available_modes: Vec<SessionMode>,
}

/// `session/new`'s result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionResponse {
    /// The new session.
    pub session_id: SessionId,
    /// Its modes, if the agent has any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<SessionModeState>,
    /// Its configuration options, as sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_options: Option<Value>,
}

/// `session/load`'s parameters (needs the agent's `loadSession`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionRequest {
    /// The session to load.
    pub session_id: SessionId,
    /// Its working directory.
    pub cwd: PathBuf,
    /// MCP servers for the agent to connect to.
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

/// `session/load`'s result, sent after the history has been replayed as
/// `session/update` notifications.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionResponse {
    /// The session's modes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<SessionModeState>,
}

/// `session/set_mode`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModeRequest {
    /// The session.
    pub session_id: SessionId,
    /// One of its available modes.
    pub mode_id: SessionModeId,
}

// ---------------------------------------------------------------------------
// content

/// Text, which clients render as Markdown.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextContent {
    /// The text.
    pub text: String,
    /// Display hints, as sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

/// An image, base64-encoded.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    /// Base64 bytes.
    pub data: String,
    /// Its MIME type.
    pub mime_type: String,
    /// Where it came from. Not to be fetched without the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Display hints, as sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

/// Audio, base64-encoded.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioContent {
    /// Base64 bytes.
    pub data: String,
    /// Its MIME type.
    pub mime_type: String,
    /// Display hints, as sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

/// A reference to a resource the agent can read.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLink {
    /// Its URI.
    pub uri: String,
    /// A name for people.
    pub name: String,
    /// Its MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// A title for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// More about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Its size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// A text resource's contents.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextResourceContents {
    /// Its URI.
    pub uri: String,
    /// The text.
    pub text: String,
    /// Its MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// A binary resource's contents, base64-encoded.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobResourceContents {
    /// Its URI.
    pub uri: String,
    /// Base64 bytes.
    pub blob: String,
    /// Its MIME type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// An embedded resource's contents: text or bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResourceContents {
    /// Text.
    Text(TextResourceContents),
    /// Bytes.
    Blob(BlobResourceContents),
}

/// A resource embedded in a message (needs the agent's `embeddedContext`
/// in a prompt).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedResource {
    /// The contents.
    pub resource: ResourceContents,
    /// Display hints, as sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

tagged_enum! {
    /// A piece of displayable content: in prompts, message chunks and tool
    /// call results.
    ContentBlock, tag = "type" {
        /// Text (Markdown).
        Text(TextContent) = "text",
        /// An image.
        Image(ImageContent) = "image",
        /// Audio.
        Audio(AudioContent) = "audio",
        /// A link to a resource.
        ResourceLink(ResourceLink) = "resource_link",
        /// An embedded resource.
        Resource(EmbeddedResource) = "resource",
    }
}

impl ContentBlock {
    /// A text block.
    pub fn text(text: impl Into<String>) -> ContentBlock {
        ContentBlock::Text(TextContent {
            text: text.into(),
            annotations: None,
        })
    }

    /// The text, for a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text(text) => Some(&text.text),
            _ => None,
        }
    }
}

impl From<&str> for ContentBlock {
    fn from(text: &str) -> ContentBlock {
        ContentBlock::text(text)
    }
}

impl From<String> for ContentBlock {
    fn from(text: String) -> ContentBlock {
        ContentBlock::text(text)
    }
}

// ---------------------------------------------------------------------------
// prompt turns

/// `session/prompt`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    /// The session.
    pub session_id: SessionId,
    /// The user's message.
    pub prompt: Vec<ContentBlock>,
}

impl PromptRequest {
    /// A prompt of one text block.
    pub fn text(session_id: impl Into<SessionId>, text: impl Into<String>) -> PromptRequest {
        PromptRequest {
            session_id: session_id.into(),
            prompt: vec![ContentBlock::text(text)],
        }
    }
}

open_enum! {
    /// Why a prompt turn ended.
    StopReason {
        /// The model finished without asking for more tools.
        EndTurn = "end_turn",
        /// The token limit was reached.
        MaxTokens = "max_tokens",
        /// The turn's request limit was reached.
        MaxTurnRequests = "max_turn_requests",
        /// The agent refused.
        Refusal = "refusal",
        /// The client cancelled the turn.
        Cancelled = "cancelled",
    }
}

/// `session/prompt`'s result: the turn is over.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    /// Why it ended.
    pub stop_reason: StopReason,
}

/// `session/cancel`'s parameters (a notification).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelNotification {
    /// The session whose turn to cancel.
    pub session_id: SessionId,
}

/// `session/update`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNotification {
    /// The session.
    pub session_id: SessionId,
    /// What changed.
    pub update: SessionUpdate,
}

/// A chunk of a message (the user's in a replay, the agent's, or its
/// thoughts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentChunk {
    /// The chunk.
    pub content: ContentBlock,
    /// Chunks with the same id belong to one message; a new id starts
    /// another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

open_enum! {
    /// What sort of tool a call is, for its icon and presentation.
    ToolKind {
        /// Reading files or data.
        Read = "read",
        /// Changing files or content.
        Edit = "edit",
        /// Removing files or data.
        Delete = "delete",
        /// Moving or renaming files.
        Move = "move",
        /// Searching.
        Search = "search",
        /// Running commands or code.
        Execute = "execute",
        /// Internal reasoning or planning.
        Think = "think",
        /// Fetching external data.
        Fetch = "fetch",
        /// Switching the session's mode.
        SwitchMode = "switch_mode",
        /// Anything else (the default).
        Other = "other",
    }
}

// The enum comes from `open_enum!`, which cannot carry `#[default]`.
#[allow(clippy::derivable_impls)]
impl Default for ToolKind {
    fn default() -> ToolKind {
        ToolKind::Other
    }
}

open_enum! {
    /// Where a tool call is in its life.
    ToolCallStatus {
        /// Not started: its input is streaming or it awaits approval (the
        /// default).
        Pending = "pending",
        /// Running.
        InProgress = "in_progress",
        /// Done.
        Completed = "completed",
        /// Failed.
        Failed = "failed",
    }
}

#[allow(clippy::derivable_impls)]
impl Default for ToolCallStatus {
    fn default() -> ToolCallStatus {
        ToolCallStatus::Pending
    }
}

/// A file change, for a diff view.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    /// The file: an absolute path.
    pub path: PathBuf,
    /// Its text before; `None` for a new file.
    #[serde(default)]
    pub old_text: Option<String>,
    /// Its text after.
    pub new_text: String,
}

/// A terminal whose output belongs in a tool call.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalContent {
    /// The terminal, from `terminal/create`.
    pub terminal_id: TerminalId,
}

/// Content wrapped for a tool call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WrappedContent {
    /// The content.
    pub content: ContentBlock,
}

tagged_enum! {
    /// What a tool call produced.
    ToolCallContent, tag = "type" {
        /// Ordinary content.
        Content(WrappedContent) = "content",
        /// A file change.
        Diff(Diff) = "diff",
        /// A terminal's live output.
        Terminal(TerminalContent) = "terminal",
    }
}

/// A file a tool call is working on, for following the agent. Display only:
/// it grants nothing.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallLocation {
    /// The file: an absolute path.
    pub path: PathBuf,
    /// A 1-based line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// A new tool call.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// Its id.
    pub tool_call_id: ToolCallId,
    /// What it is doing, for people (the agent's words).
    pub title: String,
    /// The tool's programmatic name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its kind.
    #[serde(default)]
    pub kind: ToolKind,
    /// Its status.
    #[serde(default)]
    pub status: ToolCallStatus,
    /// What it produced.
    #[serde(default, deserialize_with = "null_as_default")]
    pub content: Vec<ToolCallContent>,
    /// The files it touches.
    #[serde(default, deserialize_with = "null_as_default")]
    pub locations: Vec<ToolCallLocation>,
    /// The tool's raw input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<Value>,
    /// The tool's raw output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<Value>,
}

impl ToolCall {
    /// Applies an update: each field it carries replaces this call's.
    pub fn apply(&mut self, update: &ToolCallUpdate) {
        if let Some(title) = &update.title {
            self.title.clone_from(title);
        }
        if let Some(name) = &update.name {
            self.name = Some(name.clone());
        }
        if let Some(kind) = &update.kind {
            self.kind = kind.clone();
        }
        if let Some(status) = &update.status {
            self.status = status.clone();
        }
        if let Some(content) = &update.content {
            self.content.clone_from(content);
        }
        if let Some(locations) = &update.locations {
            self.locations.clone_from(locations);
        }
        if let Some(raw) = &update.raw_input {
            self.raw_input = Some(raw.clone());
        }
        if let Some(raw) = &update.raw_output {
            self.raw_output = Some(raw.clone());
        }
    }
}

/// A change to a tool call: only the fields that changed. Also what a
/// permission request describes.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallUpdate {
    /// The call.
    pub tool_call_id: ToolCallId,
    /// A new title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// A new name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A new kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ToolKind>,
    /// A new status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ToolCallStatus>,
    /// Replaces the content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ToolCallContent>>,
    /// Replaces the locations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<ToolCallLocation>>,
    /// A new raw input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_input: Option<Value>,
    /// A new raw output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_output: Option<Value>,
}

open_enum! {
    /// How much a plan entry matters.
    PlanEntryPriority {
        /// High.
        High = "high",
        /// Medium.
        Medium = "medium",
        /// Low.
        Low = "low",
    }
}

open_enum! {
    /// Where a plan entry stands.
    PlanEntryStatus {
        /// Not started.
        Pending = "pending",
        /// Under way.
        InProgress = "in_progress",
        /// Done.
        Completed = "completed",
    }
}

/// One step of a plan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanEntry {
    /// What it does.
    pub content: String,
    /// How much it matters.
    pub priority: PlanEntryPriority,
    /// Where it stands.
    pub status: PlanEntryStatus,
}

/// The agent's plan. Each one replaces the last whole.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Plan {
    /// Every entry.
    #[serde(default)]
    pub entries: Vec<PlanEntry>,
}

/// A slash command the agent offers.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AvailableCommand {
    /// Its name, without the slash.
    pub name: String,
    /// What it does.
    #[serde(default)]
    pub description: String,
    /// Its input, as sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
}

/// The slash commands are ready or have changed.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableCommandsUpdate {
    /// Every command.
    #[serde(default)]
    pub available_commands: Vec<AvailableCommand>,
}

/// The session's mode changed.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentModeUpdate {
    /// The new mode.
    pub current_mode_id: SessionModeId,
}

/// A cost, in a currency or another unit (credits).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Cost {
    /// How much.
    pub amount: f64,
    /// Of what: `USD`, `credits`, ….
    pub currency: String,
}

/// The session's context use and cost.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct UsageUpdate {
    /// Tokens in the context.
    pub used: u64,
    /// The context's size in tokens.
    pub size: u64,
    /// The cost so far.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

tagged_enum! {
    /// What a `session/update` reports.
    SessionUpdate, tag = "sessionUpdate" {
        /// A chunk of the user's message (when a session is replayed).
        UserMessageChunk(ContentChunk) = "user_message_chunk",
        /// A chunk of the agent's answer.
        AgentMessageChunk(ContentChunk) = "agent_message_chunk",
        /// A chunk of the agent's reasoning.
        AgentThoughtChunk(ContentChunk) = "agent_thought_chunk",
        /// A new tool call.
        ToolCall(ToolCall) = "tool_call",
        /// A change to a tool call.
        ToolCallUpdate(ToolCallUpdate) = "tool_call_update",
        /// The plan, whole.
        Plan(Plan) = "plan",
        /// The slash commands.
        AvailableCommandsUpdate(AvailableCommandsUpdate) = "available_commands_update",
        /// The mode changed.
        CurrentModeUpdate(CurrentModeUpdate) = "current_mode_update",
        /// Context use and cost.
        UsageUpdate(UsageUpdate) = "usage_update",
    }
}

// ---------------------------------------------------------------------------
// permission

open_enum! {
    /// What a permission option does: a hint for its presentation. The
    /// option's name says its scope.
    PermissionOptionKind {
        /// Allow this once.
        AllowOnce = "allow_once",
        /// Allow, and the agent remembers.
        AllowAlways = "allow_always",
        /// Reject this once.
        RejectOnce = "reject_once",
        /// Reject, and the agent remembers.
        RejectAlways = "reject_always",
    }
}

impl PermissionOptionKind {
    /// Whether choosing it allows the operation.
    pub fn allows(&self) -> bool {
        matches!(
            self,
            PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
        )
    }
}

/// One choice in a permission request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    /// Its id, for the answer.
    pub option_id: PermissionOptionId,
    /// Its label (the agent's words).
    pub name: String,
    /// What it does.
    pub kind: PermissionOptionKind,
}

/// `session/request_permission`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionRequest {
    /// The session.
    pub session_id: SessionId,
    /// The operation, described as a tool call.
    pub tool_call: ToolCallUpdate,
    /// The choices.
    pub options: Vec<PermissionOption>,
}

impl RequestPermissionRequest {
    /// The first option of `kind`, if the agent offered one.
    pub fn option(&self, kind: &PermissionOptionKind) -> Option<&PermissionOption> {
        self.options.iter().find(|option| &option.kind == kind)
    }
}

/// The user's decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RequestPermissionOutcome {
    /// The turn was cancelled.
    Cancelled,
    /// The user chose an option.
    Selected {
        /// The option.
        #[serde(rename = "optionId")]
        option_id: PermissionOptionId,
    },
}

/// `session/request_permission`'s result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestPermissionResponse {
    /// The decision.
    pub outcome: RequestPermissionOutcome,
}

impl RequestPermissionResponse {
    /// The user chose `option`.
    pub fn selected(option: impl Into<PermissionOptionId>) -> RequestPermissionResponse {
        RequestPermissionResponse {
            outcome: RequestPermissionOutcome::Selected {
                option_id: option.into(),
            },
        }
    }

    /// The turn was cancelled.
    pub fn cancelled() -> RequestPermissionResponse {
        RequestPermissionResponse {
            outcome: RequestPermissionOutcome::Cancelled,
        }
    }
}

// ---------------------------------------------------------------------------
// fs

/// `fs/read_text_file`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextFileRequest {
    /// The session (its root set bounds the path).
    pub session_id: SessionId,
    /// The file: an absolute path.
    pub path: PathBuf,
    /// The 1-based line to start at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// At most this many lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// `fs/read_text_file`'s result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadTextFileResponse {
    /// The text read.
    pub content: String,
}

/// `fs/write_text_file`'s parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteTextFileRequest {
    /// The session (its root set bounds the path).
    pub session_id: SessionId,
    /// The file: an absolute path, created if missing.
    pub path: PathBuf,
    /// The text to write.
    pub content: String,
}

/// `fs/write_text_file`'s result (empty).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct WriteTextFileResponse {}

// ---------------------------------------------------------------------------
// terminal

/// `terminal/create`'s parameters.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalRequest {
    /// The session.
    pub session_id: SessionId,
    /// The program.
    pub command: String,
    /// Its arguments.
    #[serde(default, deserialize_with = "null_as_default")]
    pub args: Vec<String>,
    /// Its extra environment.
    #[serde(default, deserialize_with = "null_as_default")]
    pub env: Vec<EnvVariable>,
    /// Its working directory: absolute; the session's when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Keep at most this many bytes of output, dropping the oldest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_byte_limit: Option<u64>,
}

/// `terminal/create`'s result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalResponse {
    /// The new terminal.
    pub terminal_id: TerminalId,
}

/// The parameters of `terminal/output`, `terminal/wait_for_exit`,
/// `terminal/kill` and `terminal/release`: which terminal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalRequest {
    /// The session.
    pub session_id: SessionId,
    /// The terminal.
    pub terminal_id: TerminalId,
}

/// How a terminal's command ended.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitStatus {
    /// Its exit code (none when a signal ended it).
    #[serde(default)]
    pub exit_code: Option<u32>,
    /// The signal that ended it.
    #[serde(default)]
    pub signal: Option<String>,
}

/// `terminal/output`'s result.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputResponse {
    /// The output kept so far.
    pub output: String,
    /// Whether older output was dropped for the byte limit.
    pub truncated: bool,
    /// Set once the command has exited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_status: Option<TerminalExitStatus>,
}

/// `terminal/wait_for_exit`'s result.
pub type WaitForTerminalExitResponse = TerminalExitStatus;

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_update_is_kept() {
        let raw = json!({"sessionUpdate": "brand_new", "x": 1});
        let update: SessionUpdate = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(update.kind(), "brand_new");
        assert_eq!(serde_json::to_value(&update).unwrap(), raw);
    }

    #[test]
    fn known_update_round_trips_with_tag_first() {
        let raw = json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hi"}, "extra": true});
        let update: SessionUpdate = serde_json::from_value(raw).unwrap();
        let SessionUpdate::AgentMessageChunk(chunk) = &update else {
            panic!("{update:?}");
        };
        assert_eq!(chunk.content.as_text(), Some("hi"));
        let text = serde_json::to_string(&update).unwrap();
        assert!(
            text.starts_with(r#"{"sessionUpdate":"agent_message_chunk""#),
            "{text}"
        );
    }

    #[test]
    fn open_enums_keep_unknown_strings() {
        let kind: ToolKind = serde_json::from_value(json!("teleport")).unwrap();
        assert_eq!(kind, ToolKind::Unknown("teleport".into()));
        assert_eq!(serde_json::to_value(&kind).unwrap(), json!("teleport"));
        let stop: StopReason = serde_json::from_value(json!("end_turn")).unwrap();
        assert_eq!(stop, StopReason::EndTurn);
    }

    #[test]
    fn tool_call_defaults_and_updates() {
        let mut call: ToolCall =
            serde_json::from_value(json!({"toolCallId": "c1", "title": "Read", "content": null}))
                .unwrap();
        assert_eq!(call.kind, ToolKind::Other);
        assert_eq!(call.status, ToolCallStatus::Pending);
        let update: ToolCallUpdate = serde_json::from_value(
            json!({"toolCallId": "c1", "status": "completed", "title": null}),
        )
        .unwrap();
        call.apply(&update);
        assert_eq!(call.status, ToolCallStatus::Completed);
        assert_eq!(call.title, "Read");
    }

    #[test]
    fn capabilities_are_opt_in() {
        let none = ClientCapabilities::default();
        assert_eq!(
            serde_json::to_value(none).unwrap(),
            json!({"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false})
        );
        assert!(!none.allows("fs/read_text_file"));
        assert!(!none.allows("terminal/create"));
        assert!(none.allows("session/request_permission"));
        assert!(
            ClientCapabilities::none()
                .terminals(true)
                .allows("terminal/output")
        );
    }

    #[test]
    fn permission_outcomes() {
        assert_eq!(
            serde_json::to_value(RequestPermissionResponse::selected("allow")).unwrap(),
            json!({"outcome": {"outcome": "selected", "optionId": "allow"}})
        );
        assert_eq!(
            serde_json::to_value(RequestPermissionResponse::cancelled()).unwrap(),
            json!({"outcome": {"outcome": "cancelled"}})
        );
    }
}
