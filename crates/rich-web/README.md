# rs-rich-web

Serve [intuiTUIve](https://crates.io/crates/rs-rich-intuituive) terminal apps,
or any terminal program, to a web browser. It is part of
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli), an addition
rather than a port.

Each browser tab gets its own copy of the app, on its own thread. The page
draws the terminal with [xterm.js](https://xtermjs.org), which is vendored in
this crate and served by it. The app's frames go out over a WebSocket, and
the page's keys, mouse and resizes come back as the events a terminal would
give. There is no async runtime and nothing is fetched from a CDN.

```rust,no_run
use rich_web::intuituive::prelude::*;

fn counter() -> App {
    App::new(|| {
        let count = signal(0);
        text!("[b]Count:[/] {count} (+ adds one)")
            .on_key("+", move |_| count.update(|c| *c += 1))
    })
}

fn main() -> std::io::Result<()> {
    // Prints http://127.0.0.1:8080/?token=… to open in a browser.
    rich_web::serve("127.0.0.1:8080", counter)
}
```

`Server` adds options: a session cap, a fixed token, the page's title,
origins a proxy serves the page from, and `spawn` to run in the background.

```bash
cargo run -p rs-rich-web --example serve
```

## Any terminal program

`serve_command` runs a program on a pseudo-terminal per tab instead (a PTY
on Unix, ConPTY on Windows, through
[rs-rich-embed](https://crates.io/crates/rs-rich-embed)'s `LocalPty`) and
streams it to the same page. Keys, pastes and resizes go to the program;
its exit ends the session and the page shows its status. `rich serve --
PROGRAM` in [rs-rich-cli](https://crates.io/crates/rs-rich-cli) (behind its
`serve` feature) does this from the command line.

```rust,no_run
fn main() -> std::io::Result<()> {
    rich_web::serve_command("127.0.0.1:8080", ["htop"])
}
```

A page that falls behind holds the program back instead of the server
buffering without end: at most `max_buffered` bytes (1 MiB by default) wait
for the page, then the program's writes wait. `Server::bind_host` takes any
other `PtyHost`.

## A DOM renderer

`Renderer::Dom`, or `&renderer=dom` in the page's address, draws an app as
a grid of styled spans instead of xterm.js, and carries its accessibility
tree as ARIA: roles, names and states, the focused node focused in the
page, and announcements in a live region, so a browser's screen reader
reads the app. The page's script is small and the crate's own; the wire
protocol is versioned and described in the `dom` module.

## Security

- It listens only where it is told. Use a loopback address to keep it on
  this computer; any other address prints a warning.
- A random 128-bit token is part of the printed URL. The page and the
  WebSocket refuse requests without it.
- The WebSocket's `Origin` must be the server's own page, or an origin
  allowed with `allow_origin`. Without this check, any website could open
  a WebSocket to a localhost port.
- Sessions are capped (8 by default), and a connection has 10 seconds to
  send its whole request.
- A program's session holds a bounded amount of output for its page, and
  of input for the program.
- Stopping the server ends every program, killing one that ignores the
  hang-up a second later; a program's output cannot write the browser's
  clipboard.

There is no other authentication and no TLS. To expose an app beyond this
computer, put it behind a reverse proxy that provides both.

## xterm.js

`@xterm/xterm` 6.0.0 and `@xterm/addon-fit` 0.11.0 (MIT), copied unchanged
from the npm registry and checked against pinned SHA-256 hashes at build time.
Versions, sources and licences are listed in
[`assets/xterm/README.md`](assets/xterm/README.md).

## Licence

MIT, as for rs-rich. The vendored xterm.js files keep their own MIT licences
(`assets/xterm/LICENSE-*`).
