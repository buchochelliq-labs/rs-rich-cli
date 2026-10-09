# rs-rich-embed

Other programs and web pages inside an
[intuiTUIve](https://crates.io/crates/rs-rich-intuituive) app. Part of
[rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli), an addition
rather than a port.

- **`terminal(cmd)`** runs any program in a pane: a shell, `htop`, an
  editor, a terminal browser. Keys, the mouse, pastes and resizes go to it;
  its screen is followed by rs-rich-record's VT emulator (vt100 with rich's
  character widths) and drawn in the pane, with scrollback; its exit is a
  signal and a callback.
- **`web_view(url)`** shows a web page in a pane, with an address bar, and
  the address, title, loading state, back and forward as signals.

```rust,no_run
use intuituive::prelude::*;
use rich_embed::{terminal, web_view_with, ProgramEngine};

fn main() -> std::io::Result<()> {
    App::new(|| {
        let page = web_view_with(ProgramEngine::new("w3m"), "https://example.com");
        row([
            terminal("bash").on_exit(|_, cx| cx.quit()).node().panel("shell"),
            page.node().panel("web"),
        ])
    })
    .run()
}
```

Each sits on a trait an app can implement itself:

- **`PtyHost`**: `LocalPty` (a PTY on Unix, ConPTY on Windows, through
  portable-pty) and `ReplayHost` (bytes played back, for tests); an SSH
  session is a third.
- **`WebEngine`**, whose frames are cells or pixels (drawn as coloured half
  blocks):
  - `ProgramEngine` (default): a terminal browser you install (Carbonyl,
    Browsh, Chawan, w3m) run in a pane;
  - `ChromeEngine` (feature `chrome`): a headless Chrome or Chromium you
    install, over the DevTools protocol, sandbox on, temporary private
    profile, downloads off;
  - `BrowshEngine` (feature `browsh`): Browsh's HTTP mode, a page as text,
    its server bound to `127.0.0.1`.

No browser ships with this crate, and none is downloaded.

See [the guide](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/guide/intuituive/embed.md).
MIT licensed.
