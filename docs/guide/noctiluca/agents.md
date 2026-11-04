# Scripts and coding agents

A running Noctiluca can be driven from another terminal: by a shell
script, or by a coding agent that wants to look at the page it is
building. The agent reads the page as the projection's text, which costs
far fewer tokens than a screenshot and works over plain SSH, and acts on
it through the same guarded actions a person uses.

On Linux, macOS, the BSDs and (since 0.0.25) Windows, where the socket is
a named pipe; the commands and their output are the same.

## Turning it on

The control socket is off unless you ask for it:

```bash
noctiluca --control http://localhost:3000        # scripts may read and act
noctiluca --read-only http://localhost:3000      # scripts may only read
```

or in [the config](config.md#agent):

```toml
[agent]
control = true
```

Each session then listens on a socket of its own,
`$XDG_RUNTIME_DIR/noctiluca/noctiluca-PID.sock` (on Windows
`%TEMP%\noctiluca-SID\noctiluca-PID.sock`, a file naming the session's
named pipe), and every action a script takes shows in the status bar as it
happens (`agent: click The "Send"
button`).

## `noctiluca ls`

```console
$ noctiluca ls
   48213  http://localhost:3000/  My app
   48377  https://docs.rs/serde (read-only)  serde - Rust
```

The sessions listening: their pids, addresses and titles. `--json` prints
the same as a JSON array.

## `noctiluca action`

```text
noctiluca action [--session PID] [--json] [--no-wait] VERB [ARGS…]
```

With one session running, `--session` can be left out.

| Verb | What it does | Changes anything |
|---|---|---|
| `info` | The session: pid, address, title, loading, whether the page is read (`ready`), the view, read-only | no |
| `snapshot [--reader]` | The page as text, every usable control marked `[ID]` where it is, and (with `--json`) the controls listed with their roles, names, values and links | no |
| `find TEXT…` | The rows of the projection that hold TEXT | no |
| `open URL` | Open an address, a keyword or a search, as the address bar would | yes |
| `click ID\|NAME` | Click a link, a button or a box by its id from a snapshot, or by its name; a field gets the focus | yes |
| `type [--into ID\|NAME] TEXT…` | Replace a field's value (with `--into`), or type where the page's focus is | yes |
| `key KEYS` | Press keys in the page: `enter`, `tab`, `ctrl+a`, several with spaces | yes |
| `scroll up\|down\|top\|bottom` | Scroll the page by a screen, or to an end | yes |
| `back`, `forward` | Go through the history | yes |

After `open`, `back` and `forward` the command waits (up to 30 seconds)
until the new page is read, and after `click`, `type`, `key` and `scroll`
for a moment, so the next `snapshot` sees what it did. `--no-wait` returns
at once.

A short session, the way an agent uses it (the page is the crate's form
fixture):

```console
$ noctiluca action snapshot
# Form fixture
# file:///noctiluca/fixtures/form.html

 Sign up

      [0]Name [Ada_]               [1]Secret [••••••••]
                                                            [2][ ] Send news
   Size
    [3]( ) Small       [4](•) Large

       [5]Mirror ‹First mirror ▾›        [6]Volume ◀ 50 ▶[7][Send] [Unavailable]

 [9][Schedule remote update]
$ noctiluca action type --into 0 Ada Lovelace
ok
$ noctiluca action click Send
ok
$ noctiluca action find "Send news"
    3  [2][ ] Send news
```

A control's id holds for the page as it was read: after the page changes,
take a new snapshot. A disabled control has no id in the text and cannot
be clicked. An action that the page refuses (the control went, or changed
meaning) says so in the browser's status bar, as it would for a person.

## Sending an element to an agent: `Ctrl+G`

`Ctrl+G` (`agent.send`) shows the link hints; the hint you type sends that
element to your agent instead of following it: its role, name, link and
value, and the page's address and title:

```text
[Noctiluca] link "Pricing" -> https://example.com/pricing
on https://example.com/ (Example)
```

Where it goes is `[agent] send` in [the config](config.md#agent):

- `clipboard` (the default): the clipboard, for you to paste into the
  agent;
- `pane`: typed into the pane beside this one, through the terminal's API
  (tmux, WezTerm, kitty with remote control, or a terminal with
  terminal-browser's socket), which suits `noctiluca --split right` beside
  an agent. No Enter is pressed: you read it and send it;
- `pane:ID`: typed into that pane (tmux's `%3`, WezTerm's `12`).

When the pane cannot be reached, it is copied to the clipboard instead,
and a toast says why.

## What is protected

- **Only you reach a session.** The socket is in a directory of its own,
  yours, with mode 700, made before the socket is; a directory there that
  is someone else's, or open to others, is refused and nothing listens.
  On Linux, macOS and the BSDs each connection's user is checked too, and
  `noctiluca action` checks that the session is yours. There is no network
  listener.
- **On Windows** the session listens on a named pipe whose security
  descriptor admits only your account (you own it; its access list grants
  you alone, with nothing inherited), that takes no connection from
  another machine, and whose name holds 128 random bits. The name is in the
  file at the socket's path, in a directory whose access list grants you
  alone. `noctiluca action` follows only such a file of yours naming such
  a pipe, and checks the pipe is yours before it sends anything; the
  session checks the caller's account where Windows lets it.
- **Paths are not followed.** A symbolic link where the socket or its
  directory should be is refused, by the session and by the commands.
- **`--read-only` is the session's rule.** The session refuses every verb
  that changes anything before it reaches the browser, whatever the client
  asks.
- **Messages are small and short-lived.** A request is at most 64 kB and a
  reply 16 MB; each read and write waits at most 5 seconds; a few
  connections are answered at once.
- **The socket goes when Noctiluca does**: on quitting, on SIGTERM, SIGHUP
  and SIGQUIT, and on a crash. One left by a killed session is replaced
  by the next session that takes its name.

What it does not protect against:

- **Pages talk to your agent.** A snapshot is the page's own text, cleaned
  of control characters but not of meaning. A page can write instructions
  meant for an agent; an agent should treat it as data, not orders.
- **A kept profile is signed in.** With `--profile` an agent acts on your
  signed-in sites; Noctiluca says so when it starts. Use `--read-only` to
  let it read only.
- **No record outlives the session.** Actions show in the status bar as
  they happen; nothing is written to a file.

See [the threat model](../../design/threat-model.md#4-noctiluca-and-chrome)
for the rows and gaps.
