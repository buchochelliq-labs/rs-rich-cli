# rs-rich-web

Serve [intuiTUIve](https://crates.io/crates/rs-rich-intuituive) terminal apps
to a web browser. It is part of
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

## Security

- It listens only where it is told. Use a loopback address to keep it on
  this computer; any other address prints a warning.
- A random 128-bit token is part of the printed URL. The page and the
  WebSocket refuse requests without it.
- The WebSocket's `Origin` must be the server's own page, or an origin
  allowed with `allow_origin`. Without this check, any website could open
  a WebSocket to a localhost port.
- Sessions are capped (8 by default).

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
