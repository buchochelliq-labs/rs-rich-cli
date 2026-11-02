//! A client for the [Agent Client Protocol](https://agentclientprotocol.com)
//! (ACP), version 1: drive a coding agent (Claude Code through its ACP
//! adapter, Gemini CLI, Codex, …) over JSON-RPC 2.0 on its standard input
//! and output.
//!
//! This crate is an rs-rich addition, not a port: Python `rich` has no
//! agents. It is Cornacai's protocol layer
//! ([the design note](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/cornacai.md)),
//! written clean-room from the public specification, and it has no terminal
//! code: the app draws, prompts and decides.
//!
//! - [`Connection`] starts the agent ([`Connection::spawn`]) or runs over
//!   any streams ([`Connection::from_streams`]), and makes the client's
//!   calls: [`initialize`](Connection::initialize),
//!   [`new_session`](Connection::new_session),
//!   [`prompt`](Connection::prompt), [`cancel`](Connection::cancel), ….
//! - The app implements [`Client`] to answer the agent: streamed
//!   [`SessionUpdate`]s, permission requests, and the file and terminal
//!   methods.
//! - Capabilities are opt-in: [`ClientCapabilities::default`] offers
//!   nothing, and the connection refuses a file or terminal request the
//!   client did not offer before the client sees it.
//! - [`Workspace`], [`fs`] and [`Terminals`] answer those requests inside
//!   the session's root set, if the app wants them.
//!
//! Threads, no async runtime: one reader thread per connection, and a
//! thread per request from the agent, so a handler may block on the user.
//!
//! ```no_run
//! use std::process::Command;
//! use rich_acp::*;
//!
//! struct App;
//!
//! impl Client for App {
//!     fn session_update(&self, notification: SessionNotification) {
//!         if let SessionUpdate::AgentMessageChunk(chunk) = notification.update {
//!             print!("{}", chunk.content.as_text().unwrap_or(""));
//!         }
//!     }
//!
//!     fn request_permission(
//!         &self,
//!         request: RequestPermissionRequest,
//!     ) -> Result<RequestPermissionResponse, RpcError> {
//!         // A real client asks the user. This one refuses.
//!         Ok(match request.option(&PermissionOptionKind::RejectOnce) {
//!             Some(option) => RequestPermissionResponse::selected(option.option_id.clone()),
//!             None => RequestPermissionResponse::cancelled(),
//!         })
//!     }
//! }
//!
//! # fn main() -> Result<(), rich_acp::Error> {
//! let agent = Connection::spawn(Command::new("my-agent"), App)?;
//! agent.initialize(InitializeRequest::new(
//!     ClientCapabilities::none(),
//!     Implementation::new("example", "0.1.0"),
//! ))?;
//! let session = agent.new_session(NewSessionRequest::new("/home/me/project"))?;
//! let done = agent.prompt(PromptRequest::text(session.session_id, "Hello!"))?;
//! println!("\n({})", done.stop_reason);
//! # Ok(())
//! # }
//! ```

mod client;
mod connection;
mod error;
pub mod fs;
mod schema;
mod terminals;
mod workspace;

pub use client::Client;
pub use connection::{Connection, Pending};
pub use error::{Error, RpcError};
pub use schema::*;
pub use terminals::{DEFAULT_OUTPUT_LIMIT, Terminals};
pub use workspace::{Workspace, controls_execution};
