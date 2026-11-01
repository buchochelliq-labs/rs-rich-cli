# Cornacai: a terminal client for coding agents

Status: proposal, with phase 2 (`rs-rich-acp`) on branch `024-ws1`.
Written 1 November 2026. Tracking issue:
[#794](https://github.com/buchochelliq-labs/rs-rich-cli/issues/794).

**Cornacai** (kor-NAH-kai: Portuguese *cornaca*, the elephant's rider and
guide, plus "AI") is a terminal front end for coding agents that speak the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP): Claude Code
through its ACP adapter, Gemini CLI, Codex and the others. You ride and
steer; the agent does the heavy lifting. It is intuiTUIve's showcase app for
conversations, as Noctiluca is for the web.

## Clean room

Cornacai is inspired by Toad's public feature list and screenshots. Toad is
AGPL-3.0: **its source is never read**, and nothing is copied, translated or
ported from it (code, markup, styles or tests). Everything here comes from:

- the public ACP specification and its JSON schema (Apache-2.0), read at the
  `agentclientprotocol/agent-client-protocol` repository, schema 1.15.0
  (commit `212f0cac`, 31 October 2026);
- this workspace's own crates.

A contributor who has read Toad's source does not work on Cornacai.

## Scope, by phase

| Phase | What | Release |
|---|---|---|
| 1 | This note. | 0.0.24 |
| 2 | `rs-rich-acp`: the protocol client, with transcript-replay tests and an example that drives a real agent. | 0.0.24 |
| 3 | The first app: one session, a conversation view with streaming Markdown, the prompt editor with `@` file mentions, permission prompts and a diff view of the agent's edits. | 0.0.24 wave 3, once #797, #798, #800, #804 and #799 have merged |
| 4 | The shell pane, concurrent sessions and resume, the marketplace's `agent` kind, the guide and tapes. | 0.0.24 or 0.0.25 |

Out of scope for now: being an ACP *agent* (only the client side is built),
the remote (HTTP/WebSocket) transport still in RFD, elicitation, terminal
authentication, session compaction and notices (none of them is advertised,
so a conforming agent does not send them), and MCP server management beyond
passing the user's configured servers through on `session/new`.

## Protocol version

ACP version **1** (the protocol's single integer major version). The v2
schema in the specification repository is still marked unstable; it is
tracked, not implemented. `initialize` offers 1; an agent that answers with
another version is refused with `Error::UnsupportedVersion`, and the app
tells the user, as the spec asks.

## The crates

```
cornacai ──▶ rs-rich-acp                      (the protocol: no terminal code)
    ├──▶ rs-rich-intuituive ──▶ rs-rich-interact ──▶ rs-rich-ext ──▶ rs-rich
    ├──▶ rs-rich-embed (PtyHost: the shell pane, terminal/* on a PTY)
    └──▶ rs-rich-marketplace (phase 4: finding and installing agents)
```

### `rs-rich-acp` (`crates/rich-acp`, lib `rich_acp`)

A generic, reusable ACP client library: any Rust program can drive an agent
with it. It depends on nothing of ours (serde and serde_json only) and has
**no terminal code**: it never draws, prompts or reads keys.

- **Transport:** JSON-RPC 2.0 over stdio, one message per line.
  `Connection::spawn` starts the agent as a child process (stdin and stdout
  piped; stderr read line by line and handed to the client as logs, so it
  never lands on the app's screen). `Connection::from_streams` runs over any
  reader and writer: in-process pipes in the tests, a socket later.
- **Threads, no async runtime.** One reader thread parses lines and routes
  them: responses to the waiting caller, `session/update` notifications to
  the client in arrival order, and each agent-to-client request to a thread
  of its own, so a permission prompt waiting on the user or a
  `terminal/wait_for_exit` does not stall the stream. This follows
  rs-rich-embed's hosts and rs-rich-web's server, which also avoid a
  runtime.
- **The app implements `Client`:** `session_update`, `request_permission`
  (required), and the optional `read_text_file`, `write_text_file` and
  `terminal/*` handlers, which answer "method not found" by default.
- **Typed messages:** serde structs for the methods a first client needs
  (`initialize`, `authenticate`, `session/new`, `session/load`,
  `session/set_mode`, `session/prompt`, `session/cancel`, `session/update`
  and every agent-to-client method); anything else goes through
  `Connection::request` and `Client::ext_method` as raw JSON. Unknown
  fields are ignored (`_meta` among them, for now); unknown `sessionUpdate`,
  content and tool-call-content variants arrive as `Unknown { kind, value }`
  with the raw JSON; unknown strings in open enums (tool kinds, statuses,
  stop reasons) are kept as `Unknown(String)`.
- **Helpers the app may use, not must:** `Workspace` (the root set and the
  path checks below), `fs::read_text_file` and `fs::write_text_file` on it,
  and `Terminals`, a process table for the `terminal/*` methods with the
  spec's output byte limit and truncation on a character boundary.

### `cornacai` (`crates/cornacai`, phase 3)

The app, binary `cornacai`, MIT, in this workspace and released with the
cohort like Noctiluca, so it is built and tested against intuiTUIve as it
changes. It adds nothing to intuiTUIve's core: what it needs that other apps
could use is proposed as generic core pieces (the parity items below), and
anything specific to agents stays in the app.

## How the app maps onto intuiTUIve

| Cornacai needs | Built from | Item |
|---|---|---|
| The transcript: messages appended as they stream, the view staying at the bottom until the reader scrolls up | `scroll(..).follow_end()` | #798 |
| Agent answers as Markdown, appended chunk by chunk | the streaming Markdown node: `markdown(signal)`, `MarkdownStream::append`, which parses again only the last open block | #797 |
| Thoughts and tool calls as folded entries whose title and status change live (a tool call goes pending, in progress, completed or failed) | `collapsible`'s live title, status slot and summary | #804 |
| The prompt: Enter submits, Shift+Enter or Alt+Enter adds a line, history, grows to a few rows, `@` opens a fuzzy file picker | `text_area` with `complete_at`, `history`, `grow` and the submit key; the `@` file completer from the `files` plugin (`files::miller` and `rich filter`'s fuzzy matcher) | #800 |
| A diff of each edit, hunk by hunk, with accept and reject | the interactive diff node on rs-rich-ext's `DiffView`, fed by ACP's `diff` tool-call content (`path`, `oldText`, `newText`) | #799 |
| Permission prompts | intuiTUIve dialogs, one button per `PermissionOption` | exists |
| The plan | a list of the entries with their status and priority, replaced whole on each `plan` update, as the spec says | exists |
| Tool output | `Log` with renderables | #802 |
| Themes | the theme system and `--theme` | #801 |

Session updates arrive on rs-rich-acp's reader thread. The app turns each
into a message for its own loop (intuiTUIve's `Notify`-style wake), and the
loop applies it to signals: a chunk appends to the open message's
`MarkdownStream`, a `tool_call` adds a collapsible entry, a
`tool_call_update` patches it (`ToolCall::apply`), a `plan` replaces the
plan. Nothing in the UI touches the connection's threads.

### rs-rich-embed's `PtyHost`

- **The shell pane (phase 4).** A persistent shell beside the conversation,
  on `LocalPty` behind rs-rich-embed's terminal pane, the same host
  Noctiluca and `rich serve` use: `cd` and environment changes stick. It is
  the user's shell, **not the agent's**: nothing typed there or printed by
  it reaches the agent unless the user sends it (a "send selection to
  agent" action).
- **`terminal/*` for the agent.** Phase 3 answers `terminal/create` with
  rs-rich-acp's `Terminals` (pipes, no PTY), embedding a terminal's output
  in its tool call as text. Phase 4 can run them on a `LocalPty` instead and
  show each live in its tool call through rs-rich-embed's pane, read only.
  The output is shown with terminal control sequences neutralised (the
  spec's rule) either way.

## Security model

ACP's trust model is that the agent runs with whatever its process is
allowed; the protocol does not sandbox it. Cornacai can only guard what the
agent asks the *client* to do, and be honest about the rest.

1. **Capabilities are opt-in.** `ClientCapabilities::default()` offers
   nothing: no file reads, no writes, no terminals. The app turns each on
   from the user's settings (for the first run: reads on, writes and
   terminals asking). rs-rich-acp refuses, with "method not found"
   (`-32601`), any `fs/*` or `terminal/*` request that the client did not
   advertise in `initialize`, before the app's code sees it, so a
   capability turned off cannot be reached by a misbehaving agent.
2. **Sandboxed to the workspace.** The session's root set is its `cwd`
   plus any directories the user added. `Workspace::check` requires an
   absolute path, resolves symbolic links at the time of use (for a write,
   the nearest existing ancestor), compares by component, and fails closed:
   a path outside the roots is refused with "invalid params". The same
   check applies to `terminal/create`'s `cwd`, and a missing `cwd` means
   the session's.
3. **Permission prompts, no auto-approve by default.** Every
   `session/request_permission` is shown to the user. The prompt describes
   the operation from the tool call's `kind`, `locations` and content, shows
   commands and paths in full, and marks the title and option names as the
   agent's own words. "Always" choices are the agent's to remember, per the
   spec; Cornacai keeps no allow-list of its own in phase 3. An auto-approve
   setting, if one is added, is off by default, scoped to a kind and the
   workspace, and never matches on the agent's title or option name alone.
4. **Writes that control execution ask.** Even with writes on,
   `fs/write_text_file` to version-control hooks, editor or agent settings
   and MCP configuration (`.git/`, `.vscode/`, `.claude/`, `.mcp.json` and
   the like) asks the user first.
5. **Cancellation answers what is pending.** `Connection::cancel` sends
   `session/cancel` and itself answers every unanswered permission request
   of that session with the `cancelled` outcome, as the spec requires; the
   app's late answer is dropped.
6. **Agent text is untrusted.** Markdown is rendered without raw HTML;
   links open only on user action, with the full target shown;
   bidirectional, zero-width and tag characters in commands, paths, diffs
   and permission prompts are revealed; control sequences in agent text and
   terminal output are neutralised; images are drawn only from inline
   `data`, never fetched from an agent-supplied URL without the user.
7. **The agent is identified by its launch command,** never by its
   self-reported `agentInfo`.

## The marketplace's `agent` kind (phase 4)

A fifth listing kind next to `crate`, `wasm-plugin`, `micro-pack` and
`theme` ([the listing format](marketplace.md)): an agent's name, its ACP
launch command and arguments, the program it needs on `PATH` (and how to
install it), the ACP version, and its homepage. Signed like every listing;
`rich plugin search --kind agent` finds them, and Cornacai's first run offers
the ones whose program is installed. Installing an agent never runs an
installer by itself: the listing says what to install, the user does it.

## First run

1. `cornacai` with no configuration looks for known agents on `PATH` (and,
   in phase 4, in the marketplace's `agent` listings) and offers a list; the
   choice is saved in `~/.config/cornacai/config.toml` with its command.
2. It starts the agent, initialises, and shows the agent's name and
   version. If the agent advertises authentication methods and
   `session/new` answers "authentication required", it lists them; an
   agent-handled method is called with `authenticate`.
3. The workspace is the current directory. Cornacai shows the root set and
   the capabilities it will offer (reads, writes, terminals) before the
   first prompt, with the permission defaults above.
4. One session; the conversation is kept in memory. Resume (`session/load`,
   when the agent advertises `loadSession`) and a session store are phase 4.

## Open questions

- **The session store:** JSONL per session (simple, greppable, append-only)
  or SQLite (search across sessions). JSONL first is the leaning.
- **Concurrent sessions:** several sessions on one agent connection
  (protocol-wise free) or one process per session (isolation). The API
  allows both.
- **`terminal/*` on a PTY or pipes:** pipes are simpler and enough for
  build and test output; a PTY gives colour and interactive programs. Pipes
  in phase 3, a PTY behind a setting in phase 4.
- **v2:** when the v2 schema goes stable, whether rs-rich-acp speaks both
  (negotiating on `initialize`) or moves to v2 in a breaking release.
- **Which agents are tested in CI:** none can run there without
  credentials. The transcripts are the CI tests; a real agent is run by hand
  before a release and noted in the pull request.
- **Name and site:** cornac.ai or cornacai.com, the maintainer's call.
