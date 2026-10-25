# Using it as a library

The `noctiluca` crate is the binary and the library it is built from. The
library is young: its API follows the browser, and it changes between
releases. Its reference is [on docs.rs](https://docs.rs/noctiluca).

```toml
[dependencies]
noctiluca = "0.0.1"
```

## The pieces

| Module | What it holds |
|---|---|
| `engine` | The `Engine` trait: what Noctiluca needs from a browser |
| `cdp` | `ChromeBackend`, the engine over Chrome (through rs-rich-embed), and `find_chrome` |
| `tab` | `Tab`: an engine on a thread of its own, keeping the page model current and running guarded actions |
| `model` | `Page`, the page model: blocks of text, controls and media, rebuilt from each snapshot |
| `layout` | The page as lines of cells: `reader` and `projection` |
| `app` | The browser as an intuiTUIve app: `app(engine, Options)` |
| `actions` | The action registry: every action, its keys in both presets, and `keymap` |
| `config`, `bookmarks`, `url` | The config file, the bookmarks file, and the address bar's rules |
| `dump` | `read_page` (open a page and wait until it is still) and `dump` (`--dump`'s output) |
| `clean` | `clean`: page text made safe for a terminal |
| `testing` | `FakeEngine`: an engine over saved snapshots, for tests with no browser |

## The engine

An `Engine` lays pages out at a size in cells, opens addresses, steps
through history, and gives back what Noctiluca reads:

- `snapshot`: the page as laid out, with its accessibility tree (a
  `PageSnapshot`, two JSON values);
- `capture`: the pixels of a region, as PNG or JPEG;
- `act`: a `PageAction` done to the page as real input (a click on a node,
  text typed, a field's value replaced, an option chosen, a media
  operation);
- `playback`: a video's or audio's state;
- `events`: what happened since the last call (the page changed, a
  navigation, a load, a download).

`new_tab` and `close_tab` refuse by default: the Chrome backend drives one
page for now. The trait is there so the browser behind Noctiluca can
change.

`ChromeBackend::start(ChromeOptions { … })` starts Chrome: the binary (else
[the one found](getting-started.md#the-chrome-it-needs)), sound, a kept
profile, `no_js`, and the size in cells. Downloads are denied unless
`quarantine` names a folder.

## The tab and the page model

`Tab::spawn(engine, update)` runs the engine on a thread of its own.
Send it a `Command` (`Open`, `History`, `Act`, `Activate`, `Media`,
`Resize`, `Refresh`), and it calls `update` with each `Update`: a new
`Page`, the address and loading state, an action's result, a download.

When the page changes, the tab waits a moment (100 ms) and reads it once,
so a page that changes every frame costs one reading per moment, not one
per frame. Before an action runs, the tab reads the page again and checks
the control is the same one; if not, the action is refused with a
`Refusal` and nothing is sent.

A `Page` has its `url`, `title`, `blocks` (headings, paragraphs, list
items, code, tables, media and controls, in reading order), `controls`
(role, accessible name, value, state, options, link) and `media`. Every
string in it is cleaned already. `page.text()` is its text, and
`page.find(role, name)` its controls with that role and name.

## Reading a page

`dump::read_page` opens an address and waits until the page has loaded
and stayed still for a moment, as `--dump` does. With
[`FakeEngine`](#testing-with-no-browser) it needs no browser; this is a
doc test of the crate (the `?` needs a function that returns
`std::io::Result`):

```rust
use std::path::Path;
use std::time::Duration;

use noctiluca::testing::FakeEngine;

let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
let engine = FakeEngine::from_dir(&snapshots)?;
let page = noctiluca::dump::read_page(
    Box::new(engine),
    "https://fixture.example/second.html",
    Duration::from_secs(10),
)?;
assert_eq!(page.title, "Second page");
assert!(page.text().contains("You followed the link."));
assert_eq!(page.find("link", "Back to the start").len(), 1);
```

With Chrome, as a normal user:

```rust
use std::time::Duration;

use noctiluca::cdp::{ChromeBackend, ChromeOptions};

let engine = ChromeBackend::start(ChromeOptions {
    columns: 100,
    rows: 40,
    ..ChromeOptions::default()
})?;
let page = noctiluca::dump::read_page(
    Box::new(engine),
    "https://example.com",
    Duration::from_secs(60),
)?;
println!("{}", page.text());
```

`noctiluca::dump::dump(&page, noctiluca::args::Dump::Json)` prints it as `--dump json`
does.

## The address bar's rules

`Config` parses the config file, and `destination` is the address bar:

```rust
use noctiluca::config::{Config, Destination};

let config = Config::parse(
    r#"
[shortcuts]
wiki = "https://en.wikipedia.org/w/index.php?search={}"
"#,
)
.unwrap();
assert!(config.warnings.is_empty());
assert_eq!(
    config.destination("wiki sea sparkle", "about:blank", &[]),
    Destination::Keyword("https://en.wikipedia.org/w/index.php?search=sea+sparkle".into())
);
```

## The browser as an app

`app::app(engine, Options { config, bookmarks, url, view, images })`
returns the whole browser as an intuiTUIve `App`, over any engine. Call
`.run()` on it, or drive it headless in a test with intuiTUIve's
`Driver` (`app.driver(columns, rows)`), as the crate's own app tests do.

## Testing with no browser

`testing::FakeEngine` is an engine over saved snapshots. `from_dir`
reads every `NAME.json` (`{"dom": …, "ax": …}`) of a folder, and an
address opens the snapshot named by its last path segment
(`https://any.host/docs.html?q=x` opens `docs.json`). It records every
action and every address opened in its shared `state`, follows a clicked
link to the snapshot its address names, and `on_act` lets a test change
the page when an action runs. Its pictures are a flat blue box.

The crate's `capture` example saves Chrome's snapshots of local pages for
it:

```sh
cargo run -p noctiluca --example capture -- OUT_DIR page.html …
```

`FakeEngine` is always in the library. The `fake-engine` Cargo feature is
for the binary only: built with it, `noctiluca` replays the snapshots in
the folder `NOCTILUCA_FAKE_ENGINE` names instead of starting Chrome. The
end-to-end test and the docs recordings run the binary so. A build
without the feature ignores the variable.
