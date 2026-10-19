# A playground in the docs

Status: proposal, for discussion. Nothing is built.

The idea: a terminal on the docs site where a reader types `rich` commands
and sees them run, in their browser, with nothing to install. A sandboxed
shell around the CLI, so pipes, files and a few familiar commands work too:

```text
$ echo '# Hello' | rich - --markdown
$ rich data.csv --csv --title Sales
$ rich deps Cargo.toml --graph
```

The suggestion is to start from [Bashkit](https://github.com/everruns/bashkit)
(`bashkit` on crates.io, MIT), a virtual bash interpreter written in Rust.

## What Bashkit is

As of 0.19.0 (read from its source, not yet built here):

- **A bash interpreter in-process.** It doesn't fork or exec: about 160
  commands (`ls`, `cat`, `grep`, `sed`, `awk`, `jq`, `sort`, …) are
  reimplemented in Rust. Pipes, redirections, variables, functions and loops
  work.
- **A virtual file system.** It's in memory by default, with overlay and
  mountable backends, so files exist only in the sandbox.
- **Closed by default.** There's no network unless it's turned on, no host
  file system unless something is mounted, and no host timezone. It has a
  written threat model (`docs/threat-model.md`) organised by category:
  DoS, sandbox escape, information disclosure and so on.
- **Resource limits.** It caps commands (10,000 by default), loop iterations,
  function depth, output size (1 MiB each for stdout and stderr), input size
  and parse effort, and has a wall-clock timeout.
- **Custom commands.** There's a `Builtin` trait, and a `ClapBuiltin` that
  takes a `#[derive(clap::Parser)]` struct. Our CLI is clap-shaped already.
- **A browser build.** `@everruns/bashkit-wasm` on npm is single-threaded
  and needs no `SharedArrayBuffer` or `COOP`/`COEP` headers. A static site
  can therefore host it, including GitHub Pages, where the docs live.
- **Async, on tokio.** Output can stream through a callback, but it's a
  batch model: a command line goes in, and stdout, stderr and an exit code
  come out.

## What I think of it

It's a good start, for most of what the CLI does. There is one hard limit,
which shapes the plan.

**Where it fits.** Most `rich` commands are filters: input in, styled
output out. Markdown, syntax, JSON, CSV and tables, `diff`, `inspect`,
`banner`, `mermaid`, `dot`, `deps`, `schema` and `chart` all work that
way. A `rich` builtin that renders to an ANSI string, run under Bashkit and
drawn by xterm.js, shows them exactly as a terminal would. Bashkit adds the
parts a demo needs that we'd otherwise build ourselves: pipes, files to
work on, `echo`/`cat`/`jq` to feed it, limits, and a security model someone
has already thought through.

**Where it doesn't fit: anything interactive.** Bashkit has no terminal. A
builtin gets its stdin as a string and returns its output when it finishes.
There's no raw key input, no window size changing under it, no alternate
screen and no frames. Without those, these can't work:

- `rich choose`/`filter`/`input`/`confirm`, `pager`, `explore` and `log
  --follow`;
- `--gif` playback, `watch` and `rich record`;
- every intuiTUIve app.

These are the most impressive things we have, so a playground without
them undersells the project. They need a different engine; see phase 3.

**Smaller things to check in a spike:**

- **Does `rich` build for `wasm32-unknown-unknown`?** The renderers
  probably do. The rest must stay out of the browser build: PTY and
  `crossterm` (`record`, `interact`), `notify` (`watch`), `ureq` (`fetch`),
  Oniguruma (`onig`; the default `fancy-regex` path is fine), and anything
  that spawns a process (the `mmdc` backend). That's a feature set:
  roughly `rich-cli --no-default-features --features art,mermaid` plus a
  new `wasm` cfg. Image decoding is pure Rust and should build.
- **Size.** The syntect syntax and theme dumps and the world map are
  several MB before compression. A first load of 5–10 MB of wasm is too
  much for a docs page. The playground should load only when someone
  clicks into it, and probably ship a smaller syntax set.
- **Two runtimes.** Bashkit-wasm is a JS package with its own wasm. Our
  `rich` would be a second wasm module, or we build Bashkit ourselves as a
  Rust dependency with `rich` registered as a builtin, giving one module.
  The second is simpler at run time and keeps one toolchain. It means
  Bashkit's Rust API, tokio in single-threaded wasm, and its `wasm_js`
  feature, all of which it supports.
- **Width and colour.** A builtin needs to know the terminal's width and
  that it's truecolor. Pass `COLUMNS`/`TERM`/`COLORTERM` as environment
  variables that xterm.js keeps current; the CLI already reads them.
- **Fidelity.** Bashkit's bash is a reimplementation, so its edge cases
  will differ from GNU bash. For a demo that's acceptable; we should say
  so on the page.
- **Maintenance.** Bashkit is young and versions fast (0.18 to 0.19 within
  weeks). Pin it, and keep the playground off the release gates so an
  upstream change can't block a release.

## The alternatives

- **A real shell on a server** (`rich serve` already runs a program on a
  PTY per browser tab, with memory and CPU limits per session). It gives
  full fidelity, interactive commands included. But it means a public
  service running arbitrary commands: a container or microVM per visitor,
  egress control, abuse handling, and a bill. That's too much to own for a
  docs page. It suits a self-hosted demo, not the public site.
- **A wasm terminal of our own, no shell:** an input line that runs
  `rich ARGS` against a few sample files. It's smaller and simpler, but has
  no pipes or files, and we'd write and secure the input handling
  ourselves. Bashkit gives more for less.
- **WASI with a POSIX shell** (wasmer.sh or WebContainers style). It's
  heavier, often needs `SharedArrayBuffer` (and so special headers GitHub
  Pages can't send), and brings no more than Bashkit does for this.

## The proposal

Phased, each phase useful on its own:

1. **Spike.** Build `rich` (the renderers, no TTY features) for
   `wasm32-unknown-unknown` as a Bashkit `ClapBuiltin`. Measure the wasm
   size and the start-up time. Find which crates refuse wasm. The output is
   a short report and a go/no-go.
2. **The playground page.** A docs page, plus "try it" buttons on the CLI
   pages that open it with that page's command filled in. It has xterm.js
   (already vendored by rs-rich-web), a line editor with history, and a
   virtual file system seeded with the files the docs use (README,
   `data.csv`, `Cargo.toml`, a GIF). The network is off and the limits are
   at Bashkit's defaults. Everything is static, loaded on first use, and
   there's no server.
3. **Interactive commands, without Bashkit.** intuiTUIve already separates
   the app from its terminal: there's a caller-owned loop, a headless
   driver, and rs-rich-web's DOM renderer. A browser backend would feed
   xterm.js's key and resize events to the driver and draw its frames.
   With that, the pickers, the pager, `explore` and the intuiTUIve demos
   (the Yazi and scope-tui clones included) run in the page too, launched
   from the playground as full-screen programs. This is the larger piece,
   but it's the same work as "intuiTUIve in the browser", which rs-rich-web
   has started for the server side.
4. **Docs as tests.** The same wasm build can check the docs' examples:
   every command on a page runs in the playground engine in CI, so a
   documented command that breaks fails the docs build. The tapes already
   do this for recordings.

## Risks

- **Size and load time** decide whether people use it. Measure them in the
  spike before committing.
- **Upstream churn.** Bashkit is a young dependency on a fast-moving
  project. Keep it pinned and isolated from the libraries and the CLI's
  release.
- **Expectations.** A sandboxed bash that isn't GNU bash, and no
  interactive commands until phase 3, can surprise people. Say both
  plainly on the page.
- **Security.** It's client-side only, so the visitor's tab is the
  sandbox, and nothing runs on our side. With the network off and no host
  mounts, the remaining risk is a page that hangs a tab. Bashkit's limits
  and timeout cover that.

## Open questions

- Should a session be shareable as a link (the command line and the
  virtual files in the URL), as Rust's playground does?
- Should a session be recordable as a tape, so a reader's experiment
  becomes a recording?
- Should the playground link from every CLI example in the docs, or from a
  few chosen ones?
