# `no_std`: not now

**Status:** decided for 0.0.21: **not now**. No code changes. This note
records what was measured, so the question doesn't have to be measured again
from scratch. It answers row 23 of the ratatui comparison's gap list
(`no_std`/`alloc` layer: low value, large; "not now; would be a new crate,
never `rich` by default"). It's linked from
[intuiTUIve against ratatui](intuituive-vs-ratatui.md#112-portability).

## The question

Should any rs-rich crate build with `#![no_std]` plus `alloc`, so that styles,
segments, text and cells can be used on targets without an operating system,
such as microcontrollers driving a small display?

## What ratatui does

ratatui's `no_std` concept page (read from
[the website's source](https://github.com/ratatui/ratatui-website), written
for ratatui 0.30.2) describes this:

- `ratatui = { default-features = false }` builds with `no_std` plus `alloc`.
  `portable-atomic` covers targets without atomics.
- Only widgets, layout, text and the buffer work there. None of the built-in
  backends do: crossterm, termion and termwiz need `std`. The app brings a
  `Backend`, for example `mousefood` for `embedded-graphics` displays. The
  page's example is an ESP32 (`riscv32imc-unknown-none-elf`).
- Widget authors depend on `ratatui-core` without default features, write
  `#![no_std]` with `extern crate alloc`, use `core` and `alloc` types, and
  keep a `std` feature for conveniences. Data is passed in, not read from
  files.
- The check is `cargo check --no-default-features` on a `no_std` target.

This works for ratatui because its core never owned the terminal: a
`Buffer` of cells is the product, and output is the backend's job.

## What we measured

On branch `021-nostd` (cut from `e264aa5`, the 0.0.21 versions), Rust 1.99.
No crate in the workspace is `no_std` today.

### The first blockers on a bare-metal target

`cargo check --target x86_64-unknown-none` (installed with rustup for this
note) stops in the dependencies, before any of our code is compiled:

| Crate (features) | Stops at | Why |
|---|---|---|
| `rs-rich` (default) | `memchr` | built with its `std` feature, which `fancy-regex`'s and `serde_json`'s default `std` features turn on |
| `rs-rich` `--no-default-features` | `memchr`, `regex-syntax` | the same, and `regex-syntax`'s `std`, also from `fancy-regex`'s defaults |
| `rs-rich-ext` `--no-default-features` | `memchr`, `bit-vec` | the same; `bit-vec`'s `std` comes from `fancy-regex`'s defaults |
| `rs-rich-intuituive` (no features) | `serde_core`, among hundreds of errors | `serde`'s `std` feature, then everything below |

These first blockers are cheap: each of those crates has a `no_std` mode,
and only our feature choices turn `std` on. The ones behind them are not.

### Dependencies

From `cargo tree -e normal`:

| Dependency | Pulled by | `no_std`? |
|---|---|---|
| `unicode-width`, `hashbrown`, `indexmap`, `memchr`, `bit-vec` | rich, rich-ext | yes |
| `fancy-regex` 0.19 (and `regex-automata`, `regex-syntax`) | rich, rich-ext | yes, with `default-features = false` (it has an `alloc`-only mode) |
| `serde_json` | rich (`Console::print_json`, `JSON`) | yes, with its `alloc` feature instead of `std` |
| `terminal_size` | rich (`Console`'s size) | **no**: it asks the OS |
| `anstyle-query` | rich (colour-system detection) | **no**: it reads the environment |
| `syntect` | rich's default `syntax` feature | **no**: files, `std::sync`, its own dumps |
| `pulldown-cmark` | rich's default `markdown` feature | **no** |
| `rs-rich-plugin-api`'s `inventory` | rich-ext | `no_std`, but it registers through platform static constructors |
| `crossterm`, `signal-hook`, `mio`, `filedescriptor`, `parking_lot` | rich-interact, so every intuiTUIve app | **no**: terminals, signals, file descriptors |
| `kasuari` (the constraint solver) | intuiTUIve | yes, with `default-features = false` |

### Our own code

Counts of `std::` paths in each crate's `src/` (tests included, so they are
upper bounds), and what a `#![no_std]` attempt says.

**`rs-rich`**, 60 files. Adding `#![no_std]` and `extern crate alloc` to
`lib.rs` (a throwaway experiment, not committed) gives 1,276 errors. 1,066
are the missing `std` prelude (`Vec`, `String`, `format!`, …), which
`use alloc::…` fixes mechanically. 209 name `std::` paths, led by
`console.rs` (48), `progress.rs` (46), `prompt.rs` (13), `protocol.rs` (12),
`json.rs` (12) and `theme.rs` (11). The ones with no `core` or `alloc`
equivalent:

| API | Where | Count |
|---|---|---|
| `std::io` (`Write`, `stdout`, `stdin`, `Result`) | `console.rs`, `pager.rs`, `progress.rs`, `prompt.rs`, `live.rs` | 58 |
| `std::sync` beyond `Arc` and atomics (`Mutex`, `OnceLock`, `LazyLock`, `mpsc`) | 19 files, including `markup.rs` (compiled regexes), `cells.rs` and `theme.rs` | about 35 |
| `std::env` (`var`, `var_os`, `temp_dir`) | `console.rs` (terminal detection, `NO_COLOR`, `COLUMNS`), `cells.rs`, `theme.rs`, `pager.rs`, `progress.rs` | 16 |
| `std::thread` (`current`, `spawn`, `ThreadId`) | `console.rs`, `progress.rs`, `live.rs`, `tree.rs` | 12 |
| `std::time::Instant` | `console.rs` (`monotonic`, the default `get_time`), `progress.rs`, `live.rs` | 3 |
| `std::fs`, `std::process`, `std::panic::catch_unwind` | `progress.rs`, `theme.rs`, `pager.rs`, `console.rs`, `live.rs` | 15 |
| `HashMap` and `HashSet` with the default (random) hasher | `console.rs`, `json.rs`, `theme.rs`, `syntax_cache.rs` | 9 |

The modules an `alloc` layer would want are close to clean on their own:
`segment.rs` uses only `std::mem`, `style.rs` only `mem` and `hash`, and
`color.rs` and `control.rs` nothing. But they are not separable along module
lines:

- `segment.rs` and `text.rs` import `console` (for `ConsoleOptions` and the
  render protocol), and `style.rs` imports `text` and `theme`. Upstream's
  modules are shaped the same way, and the mirror keeps that shape.
- `cells.rs` reads `UNICODE_VERSION` from the environment once, into a
  `OnceLock`, as upstream does.
- `markup.rs` keeps its compiled regexes in `LazyLock`s.

**`rs-rich-ext`** `--no-default-features`: 56 of its modules are always
compiled. Of those, `std::io` is used in 19, `std::time` in 15, `std::env`
in 11, `std::fs` in 8, `std::process` in 8, `HashMap` in 8 and
`std::thread` in 3. Those are the crate's purpose (files, git, processes,
notifications, the clipboard, logging), not incidental. Its non-optional
dependencies add `rs-rich-plugin-api` and `unicode-width`.

**`rs-rich-intuituive`** with no features: 59 of its 67 modules are
compiled. `Instant` or another `std::time` type is used in 19 of them (23
calls to `Instant::now()`; 22 files use only `Duration`, which `core` has),
threads in 5 (`app`, `jobs`, `reactive`, `state`, `task`), the environment
in 5, processes in 5, files in 4, `HashMap` in 15, and `Rc`/`RefCell` in 32
(fine: `alloc` has both). Its dependency on `rs-rich-interact` brings in
crossterm and the rest of the terminal stack above, and `rs-rich-interact`
turns on `rich`'s `syntax` feature, so syntect too.

### The nearer goal: wasm in a browser

The docs playground spike (`docs/design/docs-playground.md` on
`claude/next-priority-items-3c5p8e-wip-play`, with the spike in
`playground/spike/`) asked the practical version of this question: can
rs-rich run where there is no terminal and no OS, in a browser?

- `wasm32-unknown-unknown` has a `std` that compiles. **`rs-rich`, both by
  default and `--no-default-features`, builds for it unchanged**, and so do
  `rs-rich-ext` (all its feature sets that were tried), `rs-rich-diagram`,
  `rs-rich-data`, `rs-rich-mermaid`, `rs-rich-art` and `rs-rich-micro`.
- What fails to *build* is the host layer: crossterm in `rs-rich-interact`
  (the spike swapped in a 20-line stub on wasm, so intuiTUIve builds too),
  PTYs in `rs-rich-record` and `rs-rich-web`, `cap-std`/`notify`/`ctrlc` in
  the CLI, and tree-sitter's C build in `rs-rich-lumis`.
- What builds but **panics at run time** is the same list as above:
  `Instant::now()` and `SystemTime::now()` ("time not implemented on this
  platform"), and `std::thread::spawn`. `console::monotonic()` would panic,
  so the playground passes its own clock through `ConsoleBuilder::get_time`
  (upstream's `get_time` seam) and core needs no change. `Live`'s and
  `Progress`'s refresh threads and intuiTUIve's background tasks can't be
  used there. stdout, `std::env` and `std::fs` return nothing or `Err`;
  they don't panic.
- With those avoided, it works: markup, tables and `export_html` render in
  the page, and the intuiTUIve to-do app runs on `App::driver` in xterm.js.
  The lean core module is about 444 KB gzipped.

So the portability users can actually reach today is wasm, and it costs
nothing in core: the seams it needs (`get_time`, `Console::capture`,
`App::driver`) are already there.

## Options

### A. A new `alloc`-only crate (`rs-rich-core` or similar)

A new crate, `#![no_std]` with `alloc`, holding the plain data: `Color`,
`ColorTriplet`, `Style`, `Segment`, `Control`, cell widths, perhaps `Text`
spans and markup parsing on `fancy-regex` without `std`. `rich` would
re-export its types, so nothing changes for users of `rich`.

- It is the only shape that fits AGENTS.md: `rich` keeps its upstream
  modules and behaviour, and the new crate is ours.
- But `rich` would depend on it, and the types would move out of the mirror.
  Every upstream sync that touches `style.py`, `segment.py`, `color.py`,
  `cells.py`, `text.py` or `markup.py` would land in two crates. Those are
  the most-synced modules there are. That's a structural divergence, and it
  would need an accepted plan and a DIVERGENCES entry.
- The cut is not clean (above): `Segment` and `Text` reach `Console`
  through the render protocol, `Style` reaches `Theme`, and `cells` reads
  the environment. Each needs a seam, and the seam is a change to ported
  code.
- What it would buy is a `Style` and `Segment` type on a microcontroller.
  To draw anything there, an app would also want `Text`'s wrapping, `Table`,
  `Panel` and a backend, and those live in `Console`'s render pipeline,
  which is I/O-shaped by design. ratatui got this cheaply because its core
  was a buffer from the start; ours is a `Console`.

### B. A `no_std` feature on `rich`

`rich` would get `#![cfg_attr(not(feature = "std"), no_std)]`, with `std`
on by default and `Console`, `Live`, `Progress`, `Prompt` and the pager
behind it.

- A default build would still behave like upstream, so the letter of
  AGENTS.md's third rule would hold. But making the feature work means
  cfg-gating most of `console.rs` and every renderable's use of it, and
  replacing `std::sync`, `HashMap`'s hasher and `env` reads across 19+
  files. That is an edit to almost every ported module for something
  upstream doesn't have, which is what the mirror rule exists to prevent:
  every sync would have to re-apply it.
- Turning `std` *off* by default (ratatui's shape) would change what
  `rs-rich` with default features is. **Ruled out.**

### C. Do nothing now

Keep the crates as they are. Treat wasm as the portability target, and
keep the door to A open by not making it harder.

## Costs

| | A: new crate | B: feature on `rich` | C: nothing |
|---|---|---|---|
| Size | L: a crate, moved types, seams in `segment`/`text`/`style`/`cells`, a DIVERGENCES entry | L: cfg gates across most of `crates/rich` | none |
| Sync cost | every sync of the six busiest modules lands in two crates | every sync re-applies the gates | none |
| Who gains | embedded-display users, who haven't asked | the same | — |
| Risk | the mirror's module map stops matching upstream's | parity drift hidden behind a feature | none |

## Decision

**Not now (C).** Nobody has asked for rs-rich on a microcontroller. The
cost lands on the thing this repo protects most, upstream syncs. And the
target users can actually reach, a browser, already works without any of
it.

### What would reopen it

Any one of these:

- a user, or a crate in the workspace, needs rs-rich's types on a target
  with no `std` at all (`*-none-*`, an RTOS), not just without a terminal;
- upstream `rich` splits its data types away from `Console`, so a split
  here would follow upstream's shape instead of fighting it;
- ratatui interop needs `rs-rich-ratatui`'s conversions in a `no_std` app.
  Today they take `rich::Style` and `Segment`, so this would be option A's
  first user.

Reopening means a plan for option A, with a DIVERGENCES entry. Option B
stays ruled out.

### What we do meanwhile

- **Keep new generic code free of needless `std` where it costs nothing.**
  In `rich-ext`, `rich-interact` and intuiTUIve's core: take time as a value
  (`now: Instant` or a clock passed in), not `Instant::now()` deep inside a
  widget; take data as arguments, not paths; prefer `core::`/`alloc::`
  paths when both work; keep threads, files and the environment at the
  edges (the app, a plugin, a feature). This is ratatui's advice for widget
  authors, and it's also what makes code testable and wasm-safe. It is not
  a rule to rewrite existing code, and it never applies to `crates/rich`,
  which follows upstream.
- **Use the wasm targets as the practical portability check.** That
  `rs-rich` and `rs-rich-ext` build for `wasm32-unknown-unknown`, and that
  `rich-interact` and intuiTUIve do with the playground's crossterm gate, is
  the property worth keeping. A `cargo check --target wasm32-unknown-unknown`
  job for those crates in CI would catch a regression. That's a proposal for
  the playground plan, not part of this note.
- **Cheap dependency hygiene** when someone is in the manifest anyway: turn
  off default features we don't use (`rs-rich-ratatui` on `ratatui-core`,
  gap 22 of the comparison). Don't do it in `rich` for `no_std`'s sake.
