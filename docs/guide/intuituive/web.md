# Serving an app to a browser

`rs-rich-web` (`rich_web`, new in 0.0.18) serves an intuiTUIve app to a web
browser. Each browser tab that opens it gets its own copy of the app, on its
own thread. The page draws the terminal with [xterm.js](https://xtermjs.org).
The app's frames reach it over a WebSocket, and its keys, mouse and resizes
come back as the same events a terminal gives. The app does not change: the
function that builds it for `App::run` builds it for the browser too.

It serves [any terminal program](#any-terminal-program) the same way, one
pseudo-terminal per tab (`rich serve -- htop` from the command line), and
can draw an app with [its own DOM](#the-dom-renderer) instead of xterm.js,
carrying the app's accessibility tree to the browser's screen reader.

```toml
[dependencies]
rs-rich-web = "0.0.1"
```

The framework is re-exported as `rich_web::intuituive`, so this one
dependency is enough.

```rust
use rich_web::intuituive::prelude::*;

fn counter() -> App {
    App::new(|| {
        let count = signal(0);
        column([
            text!("[b]Count:[/] {count}").panel("Counter"),
            label("[dim]+ adds one"),
        ])
        .on_key("+", move |_| count.update(|c| *c += 1))
    })
}

fn main() -> std::io::Result<()> {
    rich_web::serve("127.0.0.1:8080", counter)
}
```

```text
$ cargo run
Serving intuiTUIve at http://127.0.0.1:8080/?token=3f9c0e…
```

Open the printed address. It carries the token the server checks, so open
it exactly as printed. The crate's example serves a clock, a text box, a
counter and a list:

```bash
cargo run -p rs-rich-web --example serve
```

## Options

`serve` is `Server::bind` and `Server::run` with the defaults. `Server`
takes options first:

```rust
use rich_web::Server;

Server::bind("127.0.0.1:0", counter)? // port 0: any free port
    .title("Counter")                 // the page's title
    .max_sessions(4)                  // tabs at once (8 by default)
    .run()?;                          // prints the address, serves until the process ends
```

- **`spawn()`** serves on a background thread and returns a `Handle`
  (`url()`, `local_addr()`, `sessions()`, `stop()`). Stopping it, or
  dropping it, ends every session, and returns once every program a
  session ran has ended. Tests use it to serve on `127.0.0.1:0`.
- **`run_until(f)`** is `run` that serves until `f` returns (a wait for a
  signal, say), then stops as `stop()` does.
- **`token(t)`** fixes the token instead of drawing a random one, for an
  address that stays the same between runs. Keep it as long and as secret.
- **`allow_origin(o)`** accepts the page from another origin. That origin is
  where a reverse proxy serves it (see [below](#beyond-this-computer)).
- **`renderer(Renderer::Dom)`** draws the app with the
  [DOM renderer](#the-dom-renderer) instead of xterm.js.
- **`max_buffered(bytes)`** bounds the output a
  [program's](#any-terminal-program) session holds for a slow page.

## How a session runs

The server runs one intuiTUIve [`Driver`](index.md#owning-the-loop) per
connection. Each turn of the loop updates the app, sends what changed, and
then waits for the page's next message, but no longer than the app can
wait: until its next timer, animation frame or toast. Timers and background
tasks keep drawing while nobody types.

- **The page** is served by the crate: `index.html`, a small `app.js`, and
  xterm.js 6.0.0 with its fit add-on. They are vendored in the crate,
  pinned by hash and embedded in the binary, so nothing is fetched from a
  CDN. The page fills the window and follows its size.
- **Keys, the mouse and pastes** arrive as the bytes an xterm sends:
  control characters, `ESC [` sequences, SGR mouse reports and bracketed
  pastes. `rich_web::input::decode` turns them into rs-rich-interact
  events, the same ones crossterm gives for those bytes. Ctrl+I is Tab, an
  upper-case letter has no Shift, and so on, so key bindings work the
  same in a terminal and in a browser.
- **Copying**: text the app copies (a mouse selection, `Ctx::copy`) goes to
  the page as OSC 52, and the page puts it on the browser's clipboard.
  The page answers each copy, and the "Copied" toast shows only when the
  browser took it: a browser that refuses (no permission, or a plain-HTTP
  page served to another machine, which has no clipboard API) shows none.
- **Quitting**: when the app quits (a handler calls `cx.quit()`, or Ctrl+C
  reaches it unbound), the session ends and the page says so. Reloading
  starts a new session.

## Any terminal program

`serve_command` runs a program instead of an app: one copy per tab, each on
a pseudo-terminal of its own (a PTY on Unix, ConPTY on Windows), through
[rs-rich-embed](embed.md)'s `LocalPty`, and streams it to the same
xterm.js page.

```rust
fn main() -> std::io::Result<()> {
    rich_web::serve_command("127.0.0.1:8080", ["htop", "-d", "10"])
}
```

A string is a program name, an array or a vector is the program and its
arguments, and a `rich_web::Command` also sets the environment and the
working directory. The program sees `TERM=xterm-256color`.

- **Input** goes to the program as the page sent it: keys, pastes (bracketed
  when the program asked for that) and mouse reports when it turned the
  mouse on. Resizing the window resizes the pseudo-terminal, so the program
  gets `SIGWINCH` and redraws.
- **Output** goes to the page as binary messages, and the page keeps a
  scrollback of 5,000 lines, as a terminal does.
- **The exit** ends the session: the page shows how the program ended
  ("The program exited with code 0." or "ended by signal …"), and a
  program that cannot start says why. Closing the tab, or stopping the
  server, ends the program: it is hung up on (`SIGHUP`, as when a
  terminal closes), and one still running a second later (it ignores
  `SIGHUP`) is killed with `SIGKILL`, with everything in its process
  group.
- **A slow page holds the program back.** The page says when it has drawn
  each message. With half of `max_buffered` (1 MiB by default) sent and not
  yet drawn, the server stops reading the program's output, and with the
  other half waiting unread, the program's writes wait, as they do on a
  slow terminal. `yes` or `cat` of a large file in a tab therefore costs a
  bounded amount of memory, and nothing is dropped.

`Server::bind_command(addr, command)` takes the same options as an app's
server (`title` is the program's name by default). `Server::bind_host(addr,
|| Box::new(host))` runs a program on any other `PtyHost`: an SSH session,
a container's exec stream, or rs-rich-embed's `ReplayHost` in a test.

### From the command line: `rich serve`

The `rich` command has it behind its `serve` feature, which is off by
default (a default build has no network server):

```bash
cargo install rs-rich-cli --features serve
rich serve -- htop
```

```text
Serving htop at http://127.0.0.1:8080/?token=9b1e4c…
```

| Option | Default | |
|---|---|---|
| `--bind ADDR` | `127.0.0.1` | The address to listen on. Anything else lets other machines reach it, and prints a warning. |
| `--port N` | `8080` | The port; `0` picks a free one. |
| `--max-sessions N` | `8` | Programs (tabs) at once; one more is refused until one ends. |
| `--allow-origin ORIGIN` | | Also accept the page from ORIGIN, where a reverse proxy serves it. Repeatable. |

Everything after `--` (or after the first word that is not an option) is
the program and its arguments. Ctrl+C stops the server and every program
it started (on Unix, so do `SIGTERM` and `SIGHUP`): `rich` exits once each
program has ended, so none outlives it.

Serving a shell gives a shell to anyone who has the address with its
token. Keep it on `127.0.0.1` unless a proxy with authentication is in
front (see [Security](#security)).

## The DOM renderer

`Renderer::Dom` draws an app with the page's own DOM instead of xterm.js:
the screen as a grid of styled spans, and over it the app's
[accessibility tree](index.md#accessibility) as elements carrying ARIA roles,
names and states, so a browser's screen reader reads the app as it reads a
web page.

```rust
use rich_web::{Renderer, Server};

Server::bind("127.0.0.1:8080", counter)?
    .renderer(Renderer::Dom)
    .run()?;
```

xterm.js stays the default. A page's address can choose either way:
`&renderer=dom` after the token asks for the DOM renderer from a server
whose default is xterm.js, and `&renderer=xterm` the other way. A program
has no tree, so a program's page is always xterm.js.

What the page carries:

- **The grid:** each line as runs of text in one style; styles become CSS
  (in xterm.js's default palette, so an app looks the same drawn either
  way), wide characters take two cells, and the focused text box's caret is
  drawn where the app put the cursor. The grid is hidden from assistive
  technology (`aria-hidden`): the tree describes it.
- **The tree:** each node of `Driver::accessibility()` as an element with
  `AccessNode::aria_attributes()`: its role, its name as `aria-label`, and
  its states (`aria-expanded`, `aria-checked`, `aria-selected`,
  `aria-busy`, `aria-disabled`, `aria-posinset`, `aria-setsize`). A widget
  of items (a list, a table, a tree, tabs, a menu) gets a child element for
  its selected item (a `listitem`, `row`, `treeitem`, `tab`, `menuitem` or
  `gridcell`), which holds the item's states and text. Text and a status
  read as `AccessNode::describe()` writes them, a text box reads its value.
  Elements are nested as the tree nests, and laid over the cells they
  describe, so a screen reader's highlight is where the node is drawn. The
  whole tree is one `role="application"` region named by the page's title,
  so the screen reader passes keys through to the app.
- **The focus:** the focused node's element (its selected item, for a
  widget of items) has the browser's focus, so a screen reader says what
  has it as it moves.
- **Announcements** (toasts, a dialog opening, live nodes changing,
  `Ctx::announce`) go to an `aria-live` region: assertive when urgent,
  polite otherwise. Status and log elements are kept quiet themselves, so
  nothing is said twice.
- **Input** is the xterm.js page's: keys are turned into the bytes an
  xterm sends for them (Ctrl+Shift+C and Ctrl+Shift+V stay the browser's
  copy and paste), clicks, drags, the wheel and pointer movement into SGR
  mouse reports on the cell under the pointer, and pastes into bracketed
  pastes. Copies go to the clipboard as with xterm.js.

The page's script, `dom.js`, is the crate's own (no third-party code), a
few hundred lines, and runs under the same content security policy.

### The protocol

Version 1, described in full in the `rich_web::dom` module. The page sends
what the xterm.js page sends (`d` and input bytes, `r` and
`columns,rows`, `c` and a copy's answer). The server sends JSON objects,
each named by `t`:

| Message | When | Carries |
|---|---|---|
| `hello` | first | `protocol`: 1. A page that speaks another version stops. |
| `frame` | after a frame that changed something | `cols`, `rows`, new `styles` as `[number, css]`, the changed `lines` as `[y, runs]` (each run `[text, style]`, or `[text, style, 2]` for a wide character), `cursor` |
| `tree` | when the accessibility tree changed | `focus` and `nodes`: `id`, `depth`, `rect`, `attrs`, `text`, and `item` for a widget of items |
| `say` | an announcement | `text`, `urgent` |
| `copy` | the app copied text | `n`, `text`; the page answers `c` |

`rich_web::dom::Dom` builds these from a `Driver`, so they can be checked
without a browser, as the crate's tests do.

## Security

Anyone who opens a session can do whatever the app (or the program) lets
them do. The server is therefore closed by default:

- **It listens where you tell it.** Use a loopback address (`127.0.0.1`,
  `[::1]`, `localhost`) to keep it on this computer. Any other address
  prints a warning at start.
- **A random token** (128 bits from the operating system's random source)
  is part of the printed address. The page and the WebSocket both refuse
  a request without it.
- **The `Origin` header is checked.** Browsers let any web page open a
  WebSocket to a localhost port, so the upgrade must come from this
  server's own page. That means `http://` plus the address the server is
  bound to (or `localhost` and its port), which must also be the `Host`
  the request was sent to. This guards against DNS rebinding. It can also
  come from an origin allowed with `allow_origin`. A request with no
  origin is refused.
- **Sessions are capped.** One more than `max_sessions` is refused with
  `503` until a session ends.
- **A program's output is bounded** by `max_buffered`: a page that stops
  reading holds the program back rather than growing the server's memory.
- **The page is locked down:** a content security policy allows only its
  own scripts and styles and a WebSocket to the server, and other sites
  cannot frame it.

The same checks apply to apps, programs and both renderers, and the tests
cover each of them.

### Beyond this computer

There is no other authentication and no TLS. To reach an app from another
machine, keep the server on a loopback address and put a reverse proxy in
front of it. The proxy must provide both TLS and authentication (nginx,
Caddy or Traefik with basic auth or single sign-on, for example). Then
allow the address the proxy serves it at:

```rust
Server::bind("127.0.0.1:8080", counter)?
    .allow_origin("https://term.example.com")
    .run()?;
```

The proxy must pass WebSocket upgrades through to `/ws`. The token is
still required.

## Status

All three phases of serving to a browser (0.0.18 plan, workstream 7) are
here: intuiTUIve apps, any terminal program, and the DOM renderer. Every
session runs on a thread of the server's process; one process per session,
as textual-serve isolates them, is planned for 0.0.19.
