//! Cornacai (kor-NAH-kai: Portuguese *cornaca*, the elephant's rider and
//! guide, plus "AI"): a terminal client for coding agents that speak the
//! [Agent Client Protocol](https://agentclientprotocol.com) (ACP). You ride
//! and steer; the agent does the heavy lifting.
//!
//! It is intuiTUIve's showcase app for conversations, built on
//! [rs-rich-acp](rich_acp) (the protocol) and intuiTUIve (the screen), and
//! written clean-room from the public ACP specification. The design is
//! [`docs/design/cornacai.md`][design]; the user guide is
//! [Cornacai][guide] on the docs site.
//!
//! [design]: https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/cornacai.md
//! [guide]: https://buchochelliq-labs.github.io/rs-rich-cli/guide/cornacai/
//!
//! # The pieces
//!
//! - [`app`]: the app ([`app::app`] from [`app::Options`]): sessions as
//!   tabs, each a conversation that streams in as Markdown, follows its end
//!   and folds tool calls into cards; the prompt editor with `@` file
//!   mentions; permission prompts; each write reviewed as a diff, hunk by
//!   hunk; terminal commands asked about; a shell pane.
//! - [`session`]: a session's conversation as state, and how a
//!   `session/update` changes it.
//! - [`bridge`]: rs-rich-acp's `Client`, handing everything to the app's
//!   loop.
//! - [`mention`]: `@` mentions sent as `resource_link`s.
//! - [`config`]: the configuration file and the agents Cornacai knows.
//! - [`clean`]: agent text is untrusted: control sequences neutralised,
//!   hidden characters revealed.
//!
//! # A conversation with a fake agent
//!
//! rs-rich-acp's [`replay`](rich_acp::replay) plays a recorded transcript
//! as the agent, so the app runs with no agent installed:
//!
//! ```
//! use std::time::Duration;
//!
//! use cornacai::app::{Launch, Options, app, wait_for};
//! use rich_acp::replay::Transcript;
//!
//! let transcript = Transcript::parse(r#"
//! {"client": {"method": "initialize"}}
//! {"agent": {"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1, "agentInfo": {"name": "echo", "version": "1.0"}}}}
//! {"client": {"method": "session/new"}}
//! {"agent": {"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "s1"}}}
//! "#).unwrap();
//! let launch = Launch::Replay { transcript, timeout: Some(Duration::from_secs(10)) };
//! let (app, handle) = app(Options::new(launch, std::env::temp_dir()));
//! let mut driver = app.driver(80, 12);
//! wait_for(&mut driver, "echo 1.0", Duration::from_secs(10));
//! handle.finish_replay().unwrap().unwrap();
//! ```

pub mod app;
pub mod bridge;
pub mod clean;
pub mod config;
pub mod mention;
pub mod session;
