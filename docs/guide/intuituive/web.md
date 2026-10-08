# Serving an app to a browser

`rs-rich-web` (`rich_web`, new in 0.0.18) serves an intuiTUIve app to a web
browser. Each browser tab that opens it gets its own copy of the app, on its
own thread. The page draws the terminal with [xterm.js](https://xtermjs.org).
The app's frames reach it over a WebSocket, and its keys, mouse and resizes
come back as the same events a terminal gives. The app does not change: the
function that builds it for `App::run` builds it for the browser too.

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
  dropping it, ends every session. Tests use it to serve on
  `127.0.0.1:0`.
- **`token(t)`** fixes the token instead of drawing a random one, for an
  address that stays the same between runs. Keep it as long and as secret.
- **`allow_origin(o)`** accepts the page from another origin. That origin is
  where a reverse proxy serves it (see [below](#beyond-this-computer)).

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

## Security

Anyone who opens a session can do whatever the app lets them do. The
server is therefore closed by default:

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
- **The page is locked down:** a content security policy allows only its
  own scripts and styles and a WebSocket to the server, and other sites
  cannot frame it.

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

This is phase 1 of serving to a browser (0.0.18 plan, workstream 7):
intuiTUIve apps. Still to come:

- **`rich serve -- program`**, which runs any terminal program in a
  pseudo-terminal per connection and streams it to the same page;
- **a DOM renderer** that carries widget roles to the browser's screen
  reader.
