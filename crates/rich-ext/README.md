# rich-ext

Extensions and the internal plugin registry for the [`rich`](../rich) Rust port.

## Why this crate exists

[`rich`](../rich) is a **faithful mirror** of Python `rich`: it ships upstream's
behaviour and nothing else, so that absorbing a new upstream release is a
diff-and-port rather than a merge conflict.

Everything we add on top lives here instead. This crate carries an
**independent SemVer** (starting at `0.1.0`) and never mirrors an upstream
version — unlike `rich` and `rich-cli`, whose numbers track their upstreams.

The rule, in one line: *never edit the core to add a feature.*

## What's here

- The **extension registry** — the seam that installs extra highlighters (and
  in future, boxes, themes and renderables) into a `Console` by explicit
  registration rather than compile-time magic.
- `ConsoleExt`, which adds `install_extensions()` to a `Console`.
- Example extensions, including a custom highlighter that proves the plugin
  boundary works without touching the core.

```rust
use rich::Console;
use rich_ext::ConsoleExt;

let mut console = Console::builder().build();
console.install_extensions();
```

## Status

The registry surface is currently **internal** — `rich-ext` is its only
registrant. Promoting it to a stable public API, and evaluating dynamic
third-party plugin loading, is tracked as its own roadmap issue.

## Licence

MIT.
