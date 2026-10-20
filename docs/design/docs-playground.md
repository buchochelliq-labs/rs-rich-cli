# A playground in the docs

Status: proposal, with a spike that works (`playground/spike/`). Nothing is
on the site yet. The decisions for the maintainer are at the end.

The idea: readers try rs-rich on the docs site, in their browser, with
nothing to install. There are three tiers:

1. **Core renderables.** A reader edits console markup, a table's rows or a
   Markdown snippet in the page and sees the result straight away, drawn as
   a terminal draws it.
2. **The `rich` CLI's filters.** A small terminal where `rich` commands run
   against a few sample files (`rich data.csv --csv`,
   `echo '# Hi' | rich - --markdown`).
3. **intuiTUIve apps.** Full-screen apps and the interactive commands
   (`choose`, `pager`, `explore`), driven with the keyboard and the mouse.

## What runs where

Everything runs client-side. The site is a static GitHub Pages site
(`mkdocs build`, deployed by `.github/workflows/docs.yml`), so there's no
server to run anything on, and there won't be one.

```text
docs page (MkDocs) ─▶ playground.js (loaded with the page, a few KB)
                          │  on the reader's first click, not before:
                          ▼
              rich_playground_bg.wasm + glue ─▶ ANSI text ─▶ xterm.js
                (one module per docs version)   (or HTML from Console::export_html)
                          ▲
                          └── keys, mouse and resizes from xterm.js (tier 3)
```

- The **wasm module** is rs-rich compiled for `wasm32-unknown-unknown`,
  with a thin `wasm-bindgen` API: a string or bytes go in, and ANSI text
  comes out.
- **xterm.js** draws the ANSI text. rs-rich-web already vendors it
  (`crates/rich-web/assets/xterm/`: `@xterm/xterm` 6.0.0, MIT, with the
  licence file beside it and its hashes pinned by `build.rs`). The
  playground copies those files, so there's one pinned copy in the repo.
- The **page** is plain MkDocs. A small script finds the playground blocks
  and wires them up.

## Measured: what builds for wasm

These were checked with `cargo check --target wasm32-unknown-unknown` (and
`wasm32-wasip1` for the CLI) on this branch, at the 0.0.21 versions, with
Rust 1.99.

| Crate (features) | wasm32-unknown-unknown | What fails, and what a wasm path needs |
|---|---|---|
| `rs-rich` (default: syntect, pulldown-cmark) | **builds** | Nothing fails to compile. The problems only show at run time (see below). |
| `rs-rich` `--no-default-features` | **builds** | Same as above. |
| `rs-rich-ext` `--no-default-features`, default, and the CLI's set (`data,yaml,toml,xml,jsonpath,serde,testing`) | **builds** | The `cfg(unix)`/`cfg(windows)` dependencies (rustix, libc, windows-sys) drop out by themselves. |
| `rs-rich-interact` | **fails**: `crossterm` (9 errors in crossterm itself: `sys::enable_raw_mode`, `size`, `position`, …) | crossterm has no wasm backend. **On this branch:** crossterm is a `cfg(not(target_arch = "wasm32"))` dependency, and a private stub stands in on wasm (about 20 lines in 4 files; commit `04aed23`). With that it builds, and native builds are unchanged (clippy and the lib tests pass). |
| `rs-rich-intuituive` (no features) | **fails**, but only through rs-rich-interact | It builds once rs-rich-interact does. One fix at run time (below). |
| `rs-rich-diagram` (`plugin,graphviz`), `rs-rich-data` (`er`), `rs-rich-mermaid`, `rs-rich-art` (also `gif,sixel`), `rs-rich-micro` | **builds** | Nothing. So DOT, Mermaid, ER diagrams, data profiles, FIGlet, images and GIF decoding can all go in. |
| `rs-rich-lumis` | **fails** | tree-sitter 0.26's C build refuses this target ("upgrade tree-sitter to 0.27"). Leave it out; syntect covers highlighting. |
| `rs-rich-web` | **fails** | portable-pty and `filedescriptor`, through rs-rich-embed. Its `input.rs` (xterm.js bytes to `Event`s) has no such dependency, and the spike includes it as it is. |
| `rs-rich-record` | **fails** | `filedescriptor` and `serial2` (PTY). Not needed: the browser *is* the terminal. |
| `rs-rich-cli` `--no-default-features` | **fails** | `cap-std` → rustix → `errno`. Its non-optional `notify`, `ctrlc`, `cap-std`, `cap-fs-ext` and `tempfile` are all host features. |
| `rs-rich-cli` `--no-default-features`, **wasm32-wasip1** | **fails**: `fs-set-times` (through `cap-std`) needs nightly on WASI | When an experiment made `cap-std`, `cap-fs-ext`, `notify` and `ctrlc` non-WASI dependencies, everything else built (`tempfile` included). What was left was 21 errors, all uses of those four crates, in `watch.rs`, `batch_output.rs`, `batch.rs`, `demo.rs` and `logs.rs`. |

The failures come from **terminals** (crossterm, PTYs), the **file system
layer** (cap-std), **file watching** (notify), **signals** (ctrlc) and one
**C build** (tree-sitter). None of the renderers fail. `getrandom` is never
reached on these paths, and nothing that does compile uses `std::process`
in a way that breaks the build.

### What compiles but panics at run time

`wasm32-unknown-unknown` has a std that compiles everything, but some
calls fail when they run:

- **`std::time::Instant::now()` and `SystemTime::now()` panic** ("time not
  implemented on this platform"). The spike hit this at once.
  intuiTUIve's `ThreadClock::new()` (`task.rs`) took `Instant::now()` when
  every app was created. On this branch that's now a zero clock on wasm,
  since there are no threads there to keep to it. In core,
  `console::monotonic()` (the default clock behind `get_time`) would panic
  too. The playground passes its own clock instead
  (`ConsoleBuilder::get_time`, upstream's `get_time` seam), so core needs
  no change. The remaining calls are behind the inspector (`app.rs`,
  `node.rs`), file state (`state.rs`) and `Progress`/`Live`. The
  playground must not call those, or must cfg-gate them.
- **`std::thread::spawn` panics.** This affects `Live` and `Progress`'s
  auto-refresh threads, intuiTUIve's background tasks (`task`, `jobs`,
  `reactive.rs`'s spawned work) and `rich-interact`'s input and file
  components. In the playground, these render statically (`Progress`
  without `Live`), or they don't appear.
- **stdout, stdin, `std::env` and `std::fs`** return nothing or `Err`. They
  don't panic. A console is used through `Console::capture` and
  `export_html`, never through stdout.
- **Panics abort.** The spike sets a panic hook that writes to
  `console.error`. With `panic = "abort"`, a panic leaves the module
  unusable until the page reloads, so the playground should reload the
  module after a panic.

## The spike

`playground/spike/` is a `cdylib` outside the workspace. It has its own
empty `[workspace]` and depends on the crates by path. `build.sh` builds it
with `wasm-bindgen` 0.2.129 (`--target web`) and copies rs-rich-web's
xterm.js beside the page. The page is `web/index.html` and `web/main.js`.
Serve `web/` with any static server, for example
`python -m http.server -d playground/spike/web`.

**It works.** It was checked in headless Chromium (Playwright): the page
loaded, the text came back from the terminals, and a screenshot was taken.

- **Tier 1:** console markup from a textarea is rendered to ANSI by
  `Console::capture(|c| c.print_str(..))` on every keystroke and drawn by
  xterm.js. A table built from editable CSV is shown twice, as ANSI and
  through `Console::export_html` in an iframe. Truecolor, emoji, the repr
  highlighter, links and box drawing all come through.
- **Tier 3:** the intuiTUIve tutorial's to-do app runs unchanged
  (`crates/rich-intuituive/examples/todo.rs`, included by path). It runs
  on `App::driver(cols, rows)`, the same push loop rs-rich-web's server
  runs. xterm.js's `onData` bytes go through rs-rich-web's `input::decode`
  (also included by path) into `Driver::event`. `Driver::render`'s bytes
  go back to `term.write`. Timers come from `Driver::timeout` and a
  `setTimeout`, and the time from `performance.now()`. In the scripted
  check, the reader typed "Ship the playground", pressed Enter, then Tab,
  then Space, and the app added the to-do, moved focus to the list and
  ticked the first item, as it does in a terminal. Mouse reporting is
  turned on and the decoder handles SGR reports, but the check didn't
  click.
- **Start-up:** "Ready" (fetch, compile, instantiate) came 28–63 ms after
  the module started loading, from a local server with a warm cache. Over a
  network, the download dominates (see sizes).

### Sizes

These are release builds (`opt-level = "z"`, LTO, one codegen unit,
`panic = "abort"`, stripped), after `wasm-bindgen`, without `wasm-opt`
(binaryen was not installed; it usually takes another 10–20%). Gzip is
`gzip -9`.

| Module | Raw | Gzipped |
|---|---:|---:|
| Core without syntect (`--no-default-features`): markup, Table, Panel, export_html | 1.34 MB | 444 KB |
| Core with syntect and pulldown-cmark (`Syntax`, `Markdown`) | 2.19 MB | 981 KB |
| The above plus intuiTUIve and the to-do app (`--features app`) | 2.67 MB | 1.18 MB |
| xterm.js 6.0.0 (`xterm.js`, once per site version) | 489 KB | 120 KB |
| wasm-bindgen glue JS | 15 KB | ~4 KB |

Where the bytes go, from `twiggy top` on an unstripped build:

- In the lean module, **`.rodata` is 616 KB**: emoji names, Unicode
  tables, box and spinner tables, and colour names. The regex engines
  (`fancy-regex` over `regex-automata`, used by the highlighters) are the
  largest code.
- In the app build, **`.rodata` is 1.1 MB**, because syntect's packed
  syntax and theme sets join it. pulldown-cmark and intuiTUIve's
  `App::frame`, `Node::draw` and `Driver::event` are the next largest. Any
  intuiTUIve build pulls syntect today: rs-rich-interact turns on
  `rich/syntax`, and rs-rich-intuituive takes rs-rich-ext with its
  defaults.

## The three tiers

### Tier 1: core renderables, from the page

The reader edits **console markup**, **Markdown**, **CSV** or **JSON** in a
textarea, or picks options (box style, justify, width) from a few
controls. The output updates as they type. This isn't Rust code. Compiling
Rust in the browser isn't possible at this size, and a Rust-to-wasm
compile server is the server we're avoiding. The editable things are the
inputs the library takes as text. The Rust code that produced the block is
shown beside it, unchanged, from the guide's compiled examples.

- **Crates:** `rs-rich` (`syntax`, `markdown`), plus `rs-rich-ext` for its
  renderables (charts, trees, diffs, data views).
- **cfg-gates:** none in core. The console comes from
  `Console::builder().force_terminal(true).color_system(Truecolor).width(cols).get_time(..)`.
- **Output:** ANSI to xterm.js. Without xterm.js, HTML from
  `Console::export_html` (tested in the spike), which saves xterm.js's
  120 KB on pages that only show static output.

### Tier 2: the `rich` CLI's filters

Most `rich` commands are filters: input in, styled output out. The renderers
behind them all build (table above). The CLI binary doesn't. There are two
ways to get it running, and the choice is the main open decision.

**A. WASI.** Build `rs-rich-cli` for `wasm32-wasip1` and run it under a
small JS WASI shim with an in-memory file system (for example
`@bjorn3/browser_wasi_shim`, MIT/Apache, single-threaded, no special
headers). The CLI's code stays as it is: `std::fs` reads the virtual
files, and stdout is captured and written to xterm.js. The cost is
cfg-gating four host-only dependencies (`cap-std`, `cap-fs-ext`, `notify`,
`ctrlc`) and their 21 uses, measured above. Pipes and `echo` come from a
tiny line runner of our own, or from Bashkit (below), with our module as a
custom command.

**B. Bashkit, with `rich` as a builtin.** This is the earlier proposal,
kept below. It needs the CLI as a function, `run(args, stdin, env) ->
(stdout, stderr, code)`, on `wasm32-unknown-unknown`. Today `rich_cli::run`
writes to the process's stdout and reads real files, so that's an I/O seam
through the whole CLI. It would also make one module of Bashkit and us.

The recommendation is **A first**. It is mostly mechanical work, the CLI
stays one code path, and the virtual file system *is* the "sample files"
feature. Bashkit can wrap the WASI module later, if pipes and `jq` turn out
to matter. The CLI module will be larger than the spike's, because it adds
clap, the data parsers, diagram, mermaid and art. Measure it in phase 2
before promising a budget; the estimate is 3.5–5 MB raw and 1.5–2 MB
gzipped.

### Tier 3: intuiTUIve apps, with keyboard and mouse

The spike proves this tier. `Driver` is already the browser's loop.
rs-rich-web's server drives it the same way, from a WebSocket, so the
browser version moves that loop into the page:

- **Input:** xterm.js `onData` → rs-rich-web's `input::decode` →
  `Driver::event`. Resizes from xterm.js's fit addon → `Event::Resize`.
- **Output:** `Driver::render()` → `term.write`. rs-rich-web's DOM
  renderer (`dom.rs`, with ARIA from `Driver::accessibility`) could serve
  the accessible view the same way.
- **Time:** `performance.now()` into `Driver::update`, and a `setTimeout`
  for `Driver::timeout`.
- **Clipboard:** `Driver::take_copies` → `navigator.clipboard`, as
  rs-rich-web's page does with OSC 52.

What it needs in the crates:

1. **rs-rich-interact:** crossterm behind `cfg(not(target_arch = "wasm32"))`.
   That's done on this branch (`04aed23`), marked experimental. The public
   `from_crossterm*` functions are left out on wasm.
2. **rs-rich-intuituive:** no wall clock on wasm (done, same commit). Then
   an audit of the other `Instant::now()` and `thread::spawn` call sites
   that an app can reach: the inspector, `task`/`jobs`, `watch`, and
   `files`. Each one gets a wasm path, or is documented as unavailable in
   the browser.
3. **rs-rich-web:** move `input.rs` (and the DOM renderer's message
   building) out of the server's dependency tree. For example, a
   `default-features = false` build of rs-rich-web without tungstenite,
   rs-rich-embed and getrandom, so the playground depends on it instead of
   including a file by path.
4. **rs-rich-interact's own `EventLoop`** (`choose`, `filter`, `pager`,
   `explore`) is a *pull* loop: `run()` blocks in `Backend::read`. A browser
   can't block. It needs a push API beside `run`: a `step(event)` or
   `pump(now)` that does one turn of the loop and returns. That's additive,
   and `run_inner`'s body is already one turn. Another option is JSPI
   (JavaScript Promise Integration), which lets a wasm call wait for a
   promise. Today it's Chromium-only, so it's not a base to build on.

## How it plugs into MkDocs

**Blocks in pages.** No MkDocs plugin and no Python formatter are needed.
superfences and attr_list are already on, so a fenced block carries a
class and attributes:

````markdown
```text { .rich-playground data-kind="markup" data-width="60" }
[bold magenta]Hello[/], [u]world[/]! :rocket:
```
````

Without JavaScript, this is an ordinary code block showing the input. With
JavaScript, `docs/assets/playground.js` (listed in `extra_javascript`, as
`tapes.js` is) finds `.rich-playground` blocks on each page change.
Material's instant navigation fires `document$`, as `tapes.js` already
handles. The script adds a **Try it** button. On the first click it loads
the module and xterm.js, turns the block into an editor, and shows the
output below it. Kinds are `markup`, `markdown`, `table` (CSV), `json`,
`syntax`, `cli` (a command line, tier 2) and `app` (a named demo, tier 3).

**A playground page** (`docs/playground.md`) has all three tiers at full
size, and the CLI pages' "try it" links open it with the command filled in.

**Where the module lives.** It goes in `docs/assets/playground/` at the
site root of each version: `rich_playground_bg.wasm`, its glue JS, and the
xterm.js copy. Pages refer to it relatively, so `/0.0.21/` loads 0.0.21's
module and `/dev/` loads main's.

**The per-version copy.** The versioned build (`build_versioned_docs.py`)
builds each release's docs from history, so each release needs its
module. Rust isn't built per version there, and builds take minutes each.
The recommendation:

- **Releases:** the release workflow builds the module once and attaches
  it to the GitHub release (`playground-<version>.tar.gz`).
  `build_versioned_docs.py` downloads it into that version's
  `assets/playground/`. A release without one (every release before this
  ships) gets pages without the **Try it** button, because the script
  checks a `playground.json` stamp first.
- **dev:** `docs.yml` builds the module from main (Rust and `wasm-bindgen`
  are cached, a few minutes), before `build_versioned_docs.py`.
- **Not committed to git.** Two to three MB of binary per change would
  bloat the repository. The screenshots and tapes are committed, but they
  change rarely and are reviewed as images.
- **The Pages budget:** about 3 MB raw per version (the module and
  xterm.js). The script already drops the oldest releases to stay under
  1 GB, so the playground costs a handful of versions at most.

## Without JavaScript

Every playground block is a code block first, so the input is always
shown. For tier 1, the docs can also commit the rendered output as SVG, as
the guide's screenshots already are, and show it under the block. The
**Try it** button only appears when the script runs and `WebAssembly`
exists. A failed load (an old browser, blocked wasm) leaves the static
block as it was, with a one-line note.

## Security

It all runs in the reader's tab. There's no server, no network access from
the module (it has no imports beyond `console.error` and wasm-bindgen's
glue), and no host file system. What a reader types reaches only their own
tab. The remaining risk is a hung tab: a huge input, or an app in a loop.
The page caps input sizes, and runs tier 2 and tier 3 in a Web Worker
(xterm.js stays on the main thread), so a runaway module can be terminated
and reloaded. A shared link (an open question below) would put someone
else's input in the reader's page. It's rendered as text, never as HTML,
except through `export_html`, whose output goes in a sandboxed `srcdoc`
iframe, as the spike does.

## Phased plan

1. **Tier 1 on the site.** Add a `playground/` crate (outside the
   workspace's release cohort, `publish = false`) with the spike's API
   grown to markup, Markdown, Syntax, JSON, Table and Tree. Add
   `docs/assets/playground.js` and the `.rich-playground` blocks on about
   ten guide pages. Build the module in `docs.yml` for dev, and attach it
   to releases. Run `wasm-opt`, and measure again. **About 1 MB gzipped**
   with syntect, **about 450 KB** without. Offer both: the lean module
   first, and the syntect one only when a block needs it. No crate changes.
2. **Tier 2: the CLI on WASI.** cfg-gate the CLI's four host-only
   dependencies, add the shim and a sample file system, and make
   `docs/playground.md` a terminal. Measure first; **an estimated 1.5–2 MB
   gzipped**, loaded only on the playground page and on "try it".
3. **Tier 3: apps.** Land the rs-rich-interact/rs-rich-intuituive gates
   properly (with a wasm `cargo check` in CI), the run-time audit, the
   input decoder split out of rs-rich-web, and a `step` API on
   `EventLoop`. Then the pickers, the pager, `explore` and the intuiTUIve
   demos run in the page. **About 1.2 MB gzipped** for an app. That's the
   spike's figure, and syntect is most of the difference from tier 1.
4. **Docs as tests.** The same module renders every playground block in
   CI (Node runs wasm without a browser), so a block whose input no longer
   renders fails the docs build, as the tapes already do for recordings.

## Bashkit, as evaluated before

[Bashkit](https://github.com/everruns/bashkit) (`bashkit` on crates.io,
MIT) is a virtual bash interpreter in Rust. As of 0.19.0:

- About 160 commands are reimplemented in-process.
- It has pipes, redirections, functions and loops.
- It has an in-memory file system, and no network or host file system
  unless something turns them on.
- It has a written threat model.
- It has resource limits.
- It has a `ClapBuiltin` for clap-shaped custom commands.
- It has a single-threaded browser build (`@everruns/bashkit-wasm`) that
  needs no `SharedArrayBuffer`, so GitHub Pages can host it.

It's batch-shaped: a command line goes in, and output and an exit code
come out. There's no raw key input, no resize and no frames, so it can
never run tier 3.

It still fits tier 2's *shell*, if we want pipes, `jq` and `grep`. It is
young and moves fast (0.18 to 0.19 within weeks), so pin it and keep it
off the release gates. As noted under tier 2, it needs the CLI as an
in-process function, which the WASI route doesn't.

## Alternatives considered

- **A real shell on a server.** `rich serve` already runs a program on a
  PTY per browser tab, with limits. That's full fidelity, but it means a
  public service running arbitrary commands, with sandboxes, egress
  control, abuse handling and a bill. It suits a self-hosted demo, not the
  docs site.
- **WASI with a full POSIX shell** (wasmer.sh or WebContainers style). It's
  heavier, and often needs `SharedArrayBuffer`, which needs COOP/COEP
  headers that GitHub Pages can't send.
- **A JS reimplementation** of the renderers. That's a second
  implementation to keep in step. The point of the playground is that it's
  the real library.

## Risks

- **Size and load time** decide whether people use it. The numbers above
  are measured, and phase 1 adds `wasm-opt` and lazy loading. The module
  never loads with the page, only on a click.
- **Run-time panics on wasm.** These are the `Instant` and `thread` calls
  above. A wasm `cargo check` in CI stops compile breaks, but not these.
  Phase 4's render-every-block check runs the code, so it catches them.
- **The crate gates drift.** A new crossterm or `Instant` use in
  rs-rich-interact or rs-rich-intuituive breaks the wasm build silently,
  unless CI checks it. Add `cargo check --target wasm32-unknown-unknown -p
  rs-rich-intuituive` to CI in phase 3.
- **Core stays a mirror.** Nothing here changes `crates/rich`'s behaviour.
  The playground uses its public API, and `get_time` for the clock.
  `cargo clippy --target wasm32-unknown-unknown` does flag one core lint
  (`pager.rs`'s `drop(stdin)`, because `ChildStdin` has no `Drop` there).
  CI doesn't lint that target, so leave it alone.

## Decisions for the maintainer

1. **Tier 2's engine:** the CLI on WASI with our own line runner
   (recommended), or Bashkit with an in-process CLI. Or WASI first and
   Bashkit around it later.
2. **The experimental gates** in rs-rich-interact and rs-rich-intuituive
   (`04aed23`): keep them for phase 3 (with a CI wasm check), or drop the
   commit and redo it then. They're small and native-neutral.
3. **Where each version's module comes from:** release assets plus a dev
   build in `docs.yml` (recommended), or committed under `docs/assets/`.
4. **The syntect question:** ship two modules (lean and full), or one full
   module (about 1 MB gzipped) for simplicity. Separately, should
   rs-rich-interact stop turning on `rich/syntax` unconditionally, so an
   app build can drop syntect?
5. **Editable Rust:** confirm that tier 1 edits inputs (markup, Markdown,
   CSV, options), not Rust source.
6. **Sharing:** should a playground state be shareable as a link (the
   input in the URL), as Rust's playground does?
7. **Recording:** should a session be recordable as a tape, so a reader's
   experiment becomes a recording?
8. **Reach:** "Try it" on every CLI example in the docs, or on a few
   chosen ones?
