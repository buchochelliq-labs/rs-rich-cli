# rs-rich-acp

A client for the [Agent Client Protocol](https://agentclientprotocol.com)
(ACP), version 1: it drives a coding agent (Claude Code through its ACP
adapter, Gemini CLI, Codex and others) over JSON-RPC 2.0 on the agent's
standard input and output. It is the protocol layer of Cornacai, the
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli) terminal client
for coding agents
([the design note](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/cornacai.md)),
and is usable on its own. It is an rs-rich addition, not a port: Python
`rich` has no agents. It has no terminal code; the app draws, prompts and
decides.

- **`Connection`** starts the agent (`Connection::spawn`, with its
  standard error read into the client's log, never onto the screen) or runs
  over any reader and writer (`Connection::from_streams`), and makes the
  client's calls: `initialize`, `authenticate`, `new_session`,
  `load_session`, `set_session_mode`, `prompt` (or `start_prompt`, which
  returns a `Pending`), `cancel`, and `request` / `notify` for anything
  else.
- **`Client`** is what the app implements: `session_update` (message and
  thought chunks, tool calls and their updates, plans, commands, modes,
  usage), `request_permission`, and the optional `fs/*` and `terminal/*`
  handlers.
- **Typed messages,** tolerant as the spec's evolution rules need: unknown
  fields are ignored, unknown variants arrive as `Unknown` with the raw
  JSON, unknown enum strings are kept.
- **Opt-in capabilities.** `ClientCapabilities::default()` offers nothing.
  A file or terminal request the client did not offer in `initialize` is
  answered "method not found" before the app's code sees it.
- **Cancellation done right.** `Connection::cancel` sends `session/cancel`
  and answers every open permission request of the session `cancelled`
  itself; the app's late answer is dropped.
- **Helpers for the answers:** `Workspace` (the session's root set: paths
  absolute, resolved at use, compared by component, failing closed),
  `fs::read_text_file` / `fs::write_text_file` inside it,
  `controls_execution` (files to ask about before a write: VCS hooks,
  editor and agent settings, MCP configuration), and `Terminals`, which
  runs `terminal/*` commands on pipes with the output byte limit.
- **A fake agent for tests:** `replay` plays a recorded transcript (JSON
  Lines: what the agent writes, and what the client must send) over
  in-process pipes, so a client is tested, or an app shown, with no agent
  installed.

Threads, no async runtime: a reader thread per connection, and a thread per
request from the agent, so a handler may block on the user.

```rust,no_run
use std::process::Command;
use rich_acp::*;

struct App;

impl Client for App {
    fn session_update(&self, notification: SessionNotification) {
        if let SessionUpdate::AgentMessageChunk(chunk) = notification.update {
            print!("{}", chunk.content.as_text().unwrap_or(""));
        }
    }

    fn request_permission(&self, _: RequestPermissionRequest) -> Result<RequestPermissionResponse, RpcError> {
        Ok(RequestPermissionResponse::cancelled()) // a real client asks the user
    }
}

let agent = Connection::spawn(Command::new("my-agent"), App)?;
agent.initialize(InitializeRequest::new(ClientCapabilities::none(), Implementation::new("me", "0.1.0")))?;
let session = agent.new_session(NewSessionRequest::new("/home/me/project"))?;
let done = agent.prompt(PromptRequest::text(session.session_id, "Hello!"))?;
println!("\n({})", done.stop_reason);
# Ok::<(), rich_acp::Error>(())
```

`examples/agent.rs` drives a real agent when `RICH_ACP_AGENT` names one
(and skips otherwise), and can record the conversation as a transcript
(`RICH_ACP_RECORD`). The tests replay transcripts under `tests/fixtures/`
through a fake agent on in-process pipes: no network, no agent installed.

Clean room: written from the public ACP specification (Apache-2.0); no code
from any other ACP client was read or used.

MIT licensed.
