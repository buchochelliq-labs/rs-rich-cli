# Plugin & extension design

This port keeps its core a faithful mirror of upstream `rich`. Everything we add
lives on the *outside* of that core, through a small set of **extension points**.
This document describes how that works today (internal-facing) and the intended
path to opening it up to third-party plugins.

## Why

If our own features were woven into `crates/rich`, every upstream sync would be a
manual three-way merge. Instead:

- `crates/rich` defines **extension-point traits** and calls them, but ships only
  upstream's built-in implementations. It has **no knowledge** of `rich-ext`.
- `crates/rich-ext` provides extra implementations and **registers** them onto a
  `Console`. Syncing upstream never touches `rich-ext`.

## Extension points (today)

Defined in [`crates/rich/src/protocol.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich/src/protocol.rs):

| trait          | upstream analogue          | purpose |
|----------------|----------------------------|---------|
| `Renderable`   | `__rich_console__` protocol | make a custom type printable by `Console` |
| `Highlighter`  | `Highlighter` ABC          | add style spans to `Text` (numbers, URLs, syntax, …) |

More seams (custom `Box` sets, spinners, themes) are added here as the
corresponding modules are ported — always as a trait the core calls, never as an
`if cfg!(feature = "ours")` branch inside core logic.

## Registration (explicit, not magic)

We deliberately use **explicit registration** rather than compile-time
auto-discovery (`inventory`/`linkme`): it is easier to debug, reason about, and
test, and it keeps the install order deterministic.

```rust
use rich::{Console, ColorSystem};
use rich_ext::{ExtensionRegistry, ConsoleExt};

// Option A: the convenience trait
let mut console = Console::new();
console.install_extensions();

// Option B: a registry you compose yourself
let mut console = Console::new();
let mut registry = ExtensionRegistry::new();
registry.register_highlighter(|| Box::new(rich_ext::NumberHighlighter::new()));
registry.install(&mut console);
```

The registry ([`crates/rich-ext/src/registry.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-ext/src/registry.rs))
holds *factories* so one registry can be installed onto many consoles.

## Roadmap: from internal to public

1. **Now — internal.** `rich-ext` is the only registrant. The registry API is
   usable but not yet a stability promise.
2. **Next — stable public API.** Promote `register_*` + the extension traits to a
   documented, semver-stable surface so downstream crates can register their own
   highlighters/renderables against a released `rich`.
3. **Later — third-party plugin loading.** Evaluate compile-time aggregation
   (`inventory`/`linkme`) for "just add the dependency" registration, and/or a
   dynamic/WASM boundary for runtime plugins. Tracked as its own roadmap issue;
   not built until the trait surface has settled.

Whatever we add, the invariant holds: **the core never learns about a specific
extension.**
