# Cornacai: a terminal client for coding agents

Cornacai (kor-NAH-kai: Portuguese *cornaca*, the elephant's rider and
guide, plus "AI") is a terminal front end for coding agents that speak the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP): Claude Code
through its ACP adapter, Gemini CLI, Codex and the others. You ride and
steer; the agent does the heavy lifting. It is a crate of its own,
[`cornacai`](https://crates.io/crates/cornacai), built on
[intuiTUIve](intuituive/index.md), rs-rich-acp (the protocol) and
rs-rich-embed (the shell pane).

Here it runs against a fake agent that replays a recorded conversation: a
prompt with an `@` file mention, a plan and an answer streaming in, the
agent's edit reviewed as a diff and accepted, a permission prompt for a
command, and the finished turn.

<div class="tape-player" data-cast="../../media/tapes/cornacai/cornacai.cast" data-poster="npt:0:3">
  <img src="../media/tapes/cornacai/cornacai.gif" alt="Cornacai: a prompt with a file mention, a streamed answer with a plan, an edit reviewed as a diff, a permission prompt and the finished turn">
</div>

| An `@` mention | An edit to review | A permission prompt | Done |
|---|---|---|---|
| ![The prompt completing @src/main.rs](../media/tapes/cornacai/mention.png) | ![The agent's edit as a diff, one hunk to accept or reject](../media/tapes/cornacai/review.png) | ![The agent asking to run cargo test, with its three options](../media/tapes/cornacai/permission.png) | ![The finished turn: the answer, the tool cards and the plan](../media/tapes/cornacai/done.png) |

[Tape](../tapes/cornacai.tape) · [Cast](../media/tapes/cornacai/cornacai.cast) · [GIF](../media/tapes/cornacai/cornacai.gif)

## Install and start

```sh
cargo install cornacai
cornacai                      # the configured agent, or one found on PATH
cornacai --agent "gemini --experimental-acp"
cornacai -- claude-code-acp   # the agent's command after --
cornacai --agents             # the agents Cornacai knows, and which are installed
cornacai --sessions           # the sessions kept to reopen
cornacai --resume ID          # reopen one (session/load)
cornacai --pty                # the agent's commands on a terminal
```

The first time, with no agent configured, Cornacai looks on `PATH` for the
agents it knows, offers the ones it finds, and saves the choice in
`cornacai/config.toml` in your config directory (`$XDG_CONFIG_HOME`, else
`~/.config`). An agent given with `--agent` is saved the same way when none
is configured yet. Cornacai never installs an agent: `--agents` says what to
install, and you do it.

### Agents from the marketplace

The [plugin marketplace](../marketplace.md) lists agents too, as the `agent`
kind: each listing gives the agent's launch command, the environment
variables it reads, and how to install its program.

```sh
rich plugin search --kind agent
rich plugin install echo-agent    # records its command; installs nothing
```

Installing an agent listing downloads nothing and runs nothing: `rich`
records the listing's verified command in `agents.json` in its plugin
directory, and prints the install hint for you to follow. `cornacai
--agents` and the first-run choice then offer it after the built-in ones,
once its program is on `PATH`. `rich plugin remove ID` forgets it.

The workspace is the current directory (or `--cwd DIR`): the sessions'
working directory, and the root the agent's file reads and writes through
Cornacai are kept inside.

## The screen

- **The tabs,** at the top: one per session, titled by the agent when it
  names the session, with `…` while a turn runs; then the agent's name and
  version as it reports them.
- **The conversation.** Your prompts, the agent's answers as Markdown,
  streaming in, and its thoughts folded under **Thinking**. The view
  follows the end while the agent writes; scroll up and it stays where you
  are, and End (or scrolling back down) follows again.
- **Tool calls** are folded cards: the kind and the agent's title, a
  status at the right (pending, a spinner while it runs, `✓` or
  `✗ failed`), and a one-line summary while folded: the file, or the
  output's last line. Enter or a click unfolds one: its command, its
  output, its diffs and its terminals' output.
- **The plan,** when the agent has one, above the prompt: `✓` done, `▸`
  under way, `○` to do. Each plan update replaces it whole.
- **The prompt,** which grows to six rows, then scrolls; its title says
  whether the agent is working and the session's mode.

## Keys

| Key | What it does |
|---|---|
| Enter | Send the prompt (takes the completion while its list is open) |
| Alt+Enter, Shift+Enter | A new line in the prompt |
| Up, Down on the prompt's first or last line | Earlier prompts, this session's |
| `@` | Complete a file or directory of the workspace |
| Esc | Cancel the running turn |
| Ctrl+N | A new session |
| Ctrl+O | Reopen an earlier session of this agent in this workspace |
| Alt+N, Alt+P, Alt+1 … Alt+9 | The next, the previous, or the numbered session |
| Ctrl+T | Open or close the shell pane (and move into or out of it) |
| F2 | The agent's log (what it wrote to its standard error) |
| Ctrl+Q | Quit |

## `@` file mentions

Type `@` and part of a path: the prompt offers the workspace's files that
match, fuzzily, best first (`.git`, `target` and `node_modules` are not
walked). Enter takes one. When the prompt is sent, each `@path` that names
a file or directory inside the workspace goes with it as a
`resource_link` (the file's URI, its name and its size), which every ACP
agent accepts; the agent reads the file itself. The conversation shows
which files went with the prompt.

## What the agent asks

The agent runs with whatever its own process may do; ACP does not sandbox
it. Cornacai guards what the agent asks the *client* to do, and shows you
the rest.

- **Permission prompts.** Every `session/request_permission` opens a
  prompt: what the agent wants to do (from the tool's kind), its title
  marked as the agent's own words, the files and the command in full, and
  one button per option the agent offers, numbered 1 to 9. Esc rejects
  once. "Always" choices are the agent's to remember; Cornacai keeps no
  allow-list of its own.
- **Edits.** Every `fs/write_text_file` opens the file's text and the
  agent's as a diff. `n` and `p` move between hunks, `a` and `r` accept or
  reject the one under the cursor, `A` and `R` all of them; the last
  decision writes what you accepted. `y` writes as decided so far
  (undecided hunks keep the old lines), `x` or Esc refuses the write. A
  write refused whole is answered with an error, so the agent knows; a
  partial one is written and noted in the conversation (the agent is told
  the write succeeded). Files that control what runs (`.git/`, editor and
  agent settings, MCP configuration) are flagged.
- **Commands.** Every `terminal/create` asks first, with the command line
  and its directory. An allowed command runs in the workspace, on pipes by
  default, and its output, as the agent reads it, shows in its card. With
  `--pty` (or `pty = true` under `[terminal]` in the config) each command
  runs on a terminal of its own (120 x 40), so programs that look for one
  colour their output and draw their progress; what they write is read as
  the terminal shows it, as plain text (colour and cursor movement dropped,
  a progress bar at its last state). A command whose terminal cannot be
  opened runs on pipes instead.
- **Reads** inside the workspace are answered without asking. A path
  outside it is refused, whatever the agent says.

`--no-read`, `--no-write` and `--no-terminal` (or `[capabilities]` in the
config) stop offering a capability at all: the agent is then refused before
Cornacai sees the request.

Everything the agent says is untrusted: control characters in its text and
in command output are neutralised, so it cannot move the cursor or retitle
the terminal, and characters that hide or reorder text (bidirectional
controls, zero-width characters, tags) are shown as `<U+202E>` in titles,
paths, commands and options.

## The shell pane

Ctrl+T opens a shell beside the conversation: your `$SHELL` (or `--shell
CMD`, or `shell` in the config) on a real terminal, which lives as long as
Cornacai does, so `cd` and environment changes stick. It is yours, not the
agent's: nothing typed there or printed by it reaches the agent. While it
has the focus every key goes to the shell except Ctrl+T (back to the
prompt), Ctrl+Q, Alt+N and Alt+P.

## Sessions

Ctrl+N opens another session on the same agent connection; each has its
own conversation, plan, prompt and history, and its tab. A permission
prompt or an edit from a session in the background still opens at once,
naming its session.

### Reopening a session

Cornacai keeps the sessions it opens in `cornacai/sessions.jsonl` in your
state directory (`$XDG_STATE_HOME`, else `~/.local/state`): each one's id,
the agent's command, the workspace, its title (the agent's, else your
first prompt) and when it was last used; the 200 most recent. The
conversation itself is the agent's to keep.

When the agent offers `session/load` (its `loadSession` capability),
Ctrl+O lists the sessions of this agent in this workspace, newest first,
and the one you pick opens in a tab of its own (or its tab, if it is open
already): the agent replays the conversation into it, and you go on from
there. `cornacai --resume ID` starts with one, and `cornacai --sessions`
lists them all. An agent without `session/load` says so, and `--resume`
opens a new session instead.

## Signing in

Some agents need you to sign in before they open a session or answer.
When one answers "authentication required", Cornacai shows the sign-in
methods the agent offered when it started, one button each; the one you
pick is sent to the agent (`authenticate`), which does the signing in its
own way (a browser page, an API key it reads), and what failed is sent
again: the session opens, or your prompt goes through. Esc signs in with
none. Methods that need the agent run in a terminal are not offered;
sign in with the agent's own command for those, then start Cornacai
again.

## Configuration

```toml
# cornacai/config.toml
agent = ["claude-code-acp"]   # or a string, split on spaces
shell = ["bash", "-l"]

[capabilities]
read = true       # answer reads inside the workspace
write = true      # review each write as a diff
terminal = true   # ask about each command

[terminal]
pty = true        # the agent's commands on a terminal (default: pipes)
```

## As a library

The app is a library too: `cornacai::app::app(Options)` returns the
intuiTUIve `App` and a handle on its connection. With `Launch::Replay`,
rs-rich-acp's fake agent plays a recorded transcript, which is how the
crate's tests drive it on intuiTUIve's `Driver` (no agent, no network) and
how the tape above runs (the binary's test-only `fake-agent` feature reads
the transcript named by `CORNACAI_FAKE_AGENT`).

```rust
use std::time::Duration;
use cornacai::app::{Launch, Options, app, wait_for};
use rich_acp::replay::Transcript;

let transcript = Transcript::read("conversation.jsonl")?;
let launch = Launch::Replay { transcript, timeout: Some(Duration::from_secs(10)) };
let (app, handle) = app(Options::new(launch, "/path/to/workspace"));
let mut driver = app.driver(100, 30);
wait_for(&mut driver, "Prompt", Duration::from_secs(10));
```

## What is not there yet

- An agent terminal shown live as a terminal in its card: on a PTY its
  output is read as plain text.
- Sending a selection from the shell pane to the agent.
- Sign-in methods that run the agent in a terminal.

The design, its security model and its open questions are in
[the design note](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/cornacai.md).
Cornacai is written clean-room from the public ACP specification; it is
inspired by Toad's public feature list, and none of Toad's code is used.
