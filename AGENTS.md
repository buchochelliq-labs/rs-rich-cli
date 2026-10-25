# AGENTS.md — how to maintain this port

This repo is a **Rust port of the Python [`rich`](https://github.com/Textualize/rich)
library** (plus the [`rich-cli`](https://github.com/Textualize/rich-cli) tool). Its
entire value proposition is that it **tracks upstream faithfully** while letting us
add our own features **without** making upstream syncs painful. Every rule below
exists to protect that.

This file is the contract for both human and AI contributors. Read it before
touching anything under `crates/`.

---

## The one rule everything follows

> **The core library (`crates/rich`) is a faithful mirror. New renderers and
> library-facing features go in `crates/rich-ext`. `crates/rich-cli` mirrors
> upstream commands, but may own documented binary-boundary conveniences when
> they only compose public `rich` / `rich-ext` APIs and do not change core
> behavior.**

The dependency graph is one-directional and must stay that way:

```
rich-cli ──▶ rich-ext ──▶ rich
                 │          ▲   (core: no deps on ext/cli/macros, no knowledge of them)
                 ├──▶ rich-plugin-api ──▶ rich   (the plugin contract; plugins depend on it, not on ext)
                 └──▶ rich-macros   (proc-macros; optional, behind ext's `macros` feature)
rich-cli ──▶ rich-mermaid ──▶ rich-plugin-api      (a plugin crate; optional rich-art for `mmdc`)
                  └──▶ rich-diagram ──▶ rich     (graph model, layered layout, `Diagram` renderable,
                            └ ─ ▶ rich-plugin-api   the DOT parser; the plugin API only behind its
                                                 `plugin` feature: the `dot` fence and source renderer)
rich-cli ──▶ rich-diagram                          (`rich dot`, ```dot fences, `rich deps --graph`)
             rich-lumis   ──▶ rich-plugin-api      (a plugin crate: the lumis/tree-sitter highlighter)
             rich-record  ──▶ rich-ext ──▶ rich    (tapes: `rich record`; PTY, VT emulator, renderers)
rich-cli ──▶ rich-interact ──▶ rich-ext ──▶ rich   (interactive components: session, event loop, viewport;
                                                 `rich choose`/`filter`/`input`/`pager`/`explore`/…)
rich-cli ──▶ rich-micro ──▶ rich-ext ──▶ rich      (micro assets: model, `.richmicro` packages, layered
                 ├──▶ rich-plugin-api   registry, `:micro:name:` markup, terminal graphics;
                 └──▶ rich-art          `rich micro`, behind the CLI's `art` feature)
rich-cli ──▶ rich-art ──▶ rich                     (FIGlet, images, GIFs, image diffs; the CLI's `art` feature)
rich-cli ──▶ rich-data ──▶ rich-ext ──▶ rich       (tabular data: row sources; CSV/TSV, the reader `--csv`
                                                 uses; JSONL, serde and Arrow (`arrow` feature) adapters;
                                                 type inference, column statistics, profiles, data-quality
                                                 reports and SQL result sets)
                 └──▶ rich-diagram           (optional, the `er` feature: ER diagrams of a schema)
             rich-ratatui ──▶ rich, ratatui-core   (ratatui interop: `RichWidget`, style and buffer conversions;
                  ├──▶ rich-ext (no defaults)   its colour names re-exported from `rich_ext::palette`;
                  └ ─ ▶ rich-interact          `RatatuiComponent` behind its `interact` feature. Nothing in
                                                 the workspace depends on it, or on ratatui)
             rich-intuituive ──▶ rich-interact ──▶ rich-ext ──▶ rich   (intuiTUIve, the TUI framework:
                  └ ─ ▶ rich-art, cpal, notify,  signals, retained tree, cell screen; ratatui only as a
                        hound, rich-micro        dev-dependency, for its benchmark gate. Its off-by-default
                                                 `art` (image nodes, big text, sprite sheets), `audio`
                                                 (with `pulse`), `wav`, `watch`, `viz` and `micro`
                                                 (micro assets as sprites) features)
             rich-embed ──▶ rich-intuituive, rich-record   (a terminal pane and a web view: `PtyHost`
                  └ ─ ▶ tungstenite, image       (portable-pty), `WebEngine`; Chrome over DevTools behind
                                                 its `chrome` feature, Browsh behind `browsh`)
             noctiluca ──▶ rich-intuituive, rich-embed   (Noctiluca, the terminal web browser: its engine
                  └──▶ rich                       trait over embed's ChromeEngine, the page model, reader
                                                 mode and the projection, web widgets over stock ones.
                                                 A leaf: nothing depends on it, and it adds nothing to
                                                 intuiTUIve core)
rich-cli ─ ─ ▶ rich-web ──▶ rich-intuituive, rich-embed   (serving to a browser: a WebSocket session per
                                                 tab, an app or a program on a PTY (rs-rich-embed's
                                                 `PtyHost`), a vendored xterm.js page or a DOM renderer
                                                 with ARIA; tungstenite, no async runtime. The CLI's
                                                 `rich serve`, behind its off-by-default `serve` feature.
                                                 rs-rich-embed and the network sit behind rs-rich-web's
                                                 default `server` feature: without it, only its input
                                                 decoder and DOM messages, for the docs playground's wasm)
             rich-marketplace ──▶ rich-plugin-api, rich   (the plugin marketplace client: the minisign-signed
                                                 index and its key chain, listings, version resolution
                                                 against the host's plugin API and ABI, verified HTTPS
                                                 installs and the lockfile; no terminal code. ureq
                                                 behind its default `http` feature)
rich-py  ──▶ rich, rich-ext, rich-art, rich-mermaid, rich-diagram, rich-micro, rich-interact, rich-cli,
             rich-intuituive, rich-web, rich-embed
                           (Python bindings; outside the workspace, released to PyPI only)
```

If a change would make `crates/rich` diverge from upstream `rich`, it is almost
certainly in the wrong place — see [Adding our own features](#adding-our-own-features).
If a CLI-only feature changes the `rich` binary rather than the library, record
it in the release plan and `docs/PORTING.md`, keep renderers in `rich-ext` where
possible, and keep the boundary free of core back-dependencies.

---

## Versioning

**Every crate versions independently, by its own SemVer. Started at `0.0.1`.**

| crate          | version source    | when it bumps                          |
|----------------|-------------------|----------------------------------------|
| `rs-rich`      | independent SemVer | whenever we ship anything              |
| `rs-rich-cli`  | independent SemVer | whenever we ship anything              |
| `rs-rich-ext`  | independent SemVer | whenever we ship anything              |
| `rs-rich-plugin-api` | independent SemVer | whenever we ship anything        |
| `rs-rich-mermaid` | independent SemVer | whenever we ship anything           |
| `rs-rich-lumis` | independent SemVer | whenever we ship anything             |
| `rs-rich-record` | independent SemVer | whenever we ship anything            |
| `rs-rich-interact` | independent SemVer | whenever we ship anything          |
| `rs-rich-micro` | independent SemVer | whenever we ship anything             |
| `rs-rich-diagram` | independent SemVer | whenever we ship anything           |
| `rs-rich-data` | independent SemVer | whenever we ship anything              |
| `rs-rich-ratatui` | independent SemVer | whenever we ship anything           |
| `rs-rich-intuituive` | independent SemVer | whenever we ship anything        |
| `rs-rich-web`  | independent SemVer | whenever we ship anything              |
| `rs-rich-embed` | independent SemVer | whenever we ship anything             |
| `rs-rich-marketplace` | independent SemVer | whenever we ship anything       |
| `noctiluca`    | independent SemVer | whenever we ship anything              |
| `rs-rich-macros` | independent SemVer | whenever we ship anything            |
| `rs-rich-art`  | independent SemVer | whenever we ship anything              |
| `rs-rich` (PyPI, `crates/rich-py`) | independent SemVer, `python-v…` tags | whenever we ship anything |

### Why not mirror the upstream version?

The port originally set `crates/rich`'s version to the upstream `rich` version it
mirrored, so the number told you which feature set you got. That policy does not
survive contact with crates.io, and it was dropped before the first release:

- Publishing as `15.0.0` claims a stable API with fourteen majors behind it. This
  is a young Rust API that still takes breaking changes regularly.
- The first breaking change would force `16.0.0` — and then upstream `rich` 16
  lands and the number can no longer mirror anything. The policy breaks on its
  first real use.

**Which upstream release we track now lives in [`UPSTREAM.toml`](UPSTREAM.toml)
only**, and is restated in the README. That file is still the source of truth for
a sync, and it is still updated as part of one — it just no longer drives a crate
version.

- **Never** bump `rs-rich` to ship one of *our* features. Features still go in
  `rich-ext` unless they are documented `rich-cli` binary-boundary conveniences.
  The mirror/ext boundary is unchanged; only the numbering is.

---

## Adding our own features

1. **Default location: `crates/rich-ext`.** New renderables, highlighters, boxes
   and themes go here. Register them onto a `Console` through the extension
   registry (see [docs/PLUGINS.md](docs/PLUGINS.md)); do not reach into core
   internals. CLI conveniences that are only command routing, exit/report
   contracts, streaming orchestration, or release automation may live in
   `crates/rich-cli` when their accepted release plan says so.
2. **If a feature genuinely needs a core hook**, add it as a new *extension-point
   trait* in `crates/rich/src/protocol.rs` (the sanctioned seam), keeping core's
   own behavior unchanged. Extensions implement the trait from `rich-ext`.
3. **If — and only if — a feature is impossible without changing core behavior**,
   gate it behind a Cargo `feature` flag that is **off by default**, and record it
   in [docs/DIVERGENCES.md](docs/DIVERGENCES.md). A default build of `crates/rich`
   must always behave like upstream.

If you find yourself editing a faithful-port library module to add non-upstream
behavior, stop and move it to `rich-ext`.

**Performance work in the mirror is allowed, behaviour change is not.** The
maintainer decided this on 2026-10-25. `crates/rich` may be made faster where
its output stays byte-identical. That is proven by:

- the golden fixtures;
- the parity tables;
- the differential fuzzer (`scripts/diff_rich.py`).

A parity break still fails CI. Keep the upstream structure so syncs stay easy:

- put fast paths and caches beside the ported code, not in place of it;
- record each one in [docs/PORTING.md](docs/PORTING.md)'s "Performance
  notes", so a sync knows to carry it over;
- keep a change only when the benchmarks
  (`crates/rich-intuituive/tests/paint_bench.rs` and
  `tests/versus_ratatui.rs`) show a win.

It is not a divergence, since behaviour is identical, so nothing goes in
DIVERGENCES.

One public API change was made for speed: `Segment.text` and `Color.name`
became `CompactString`s in 0.0.23, by the maintainer's explicit decision
on 2026-10-25 (output unchanged). Any other change to rich's public types
needs the same explicit decision.

**Two drawing paths.** rich's path (render to `Segment`s, then write them
into a screen) is the faithful reference. rs-rich-ext's fast path
(`surface::Draw`, `TextRun`, `Screen::write_runs`) writes cells straight
into a `Screen`, and is what rs-rich-ext's widgets and intuiTUIve draw
with by default. Both must give the same cells: a fast path comes with a
test that compares it with rich's path.

**intuiTUIve's core components are generic.** `crates/rich-intuituive`'s
always-compiled code holds only components other components can reuse: no
domain types, names or parameters (files and paths, audio, one app, one
data source). A domain component is a plugin (an off-by-default feature
module such as `files`, `pcm`/`audio`, `watch`, `art`, or another crate),
built on a generic core piece where one exists (`files::miller` on
`miller_of`, `files::tasks` on `jobs`). A non-generic proposal is not
accepted into core; reshape it as a generic core piece plus a plugin. See
[the demo gaps' intro](docs/design/intuituive-demo-gaps.md).

---

## Porting a module (parity workflow)

Use the [`port-module`](.claude/skills/port-module/SKILL.md) skill. In short:

1. Find the module in [docs/PORTING.md](docs/PORTING.md); note its target Rust file
   and mapping. Read the upstream source at the pinned ref.
2. Port it into `crates/rich`, matching upstream structure and naming so future
   diffs line up. Keep `//! Port of upstream rich/<module>.py` headers.
3. **Prove parity with golden tests.** Add cases to `scripts/capture_golden.py`,
   run it against the pinned `rich` version to capture real output, and assert
   byte-equality in `crates/rich/tests/`. Only genuinely-upstream behavior goes in
   golden fixtures; our conveniences are tested separately.
4. Update the module's status in [docs/PORTING.md](docs/PORTING.md).

### Parity / golden tests

- Golden fixtures are captured from the **real Python `rich`** at the version in
  `UPSTREAM.toml`:
  ```bash
  pip install "rich==$(grep -A2 '\[rich\]' UPSTREAM.toml | grep version | cut -d'"' -f2)"
  python scripts/capture_golden.py
  ```
- The Rust test (`crates/rich/tests/golden.rs`) asserts our output matches those
  bytes. A parity break must fail CI, never be silently accepted.

#### Never install `rich-cli` into the same environment as `rich`

`UPSTREAM.toml` pins **both** `rich` 15.0.0 and `rich-cli` 1.8.1, and those two
**cannot coexist**: `rich-cli` 1.8.1 requires `rich<13`, so installing it
silently *downgrades* `rich` to 12.6.0 — the exact library this port measures
itself against.

Nothing about that failure looks wrong. `pip` prints no error, the goldens still
run, and every subsequent parity check quietly compares against the wrong
upstream. It is the most dangerous state this repo can be in, because a
comparison that is measuring the wrong thing is indistinguishable from one that
passes.

Keep the CLI oracle in its own virtualenv, and check the library version before
trusting any parity result:

```bash
python -m venv .venv-richcli
.venv-richcli/Scripts/python -m pip install "rich-cli==1.8.1"   # rich 12.6.0 lives here
python -c "from importlib.metadata import version; print(version('rich'))"  # must be 15.0.0
```

Use the venv **only** for questions about *CLI semantics* (what a flag does,
whether it errors, what the default is). Do not use it as a rendering oracle:
its `rich` is 12.6.0, not the version `crates/rich` mirrors.

Two more environment traps that silently produce wrong comparisons on Windows:
`NO_COLOR` is set on some machines and strips colour from the captured side
only, and Python's text stdout translates `\n` to `\r\n`, which makes every
measured line one cell wider than it is. Clear the first; strip the second.

---

## Syncing a new upstream release

Use the [`sync-upstream`](.claude/skills/sync-upstream/SKILL.md) skill:

1. Diff upstream between the ref in `UPSTREAM.toml` and the new tag.
2. Map each changed `.py` to its Rust file via `docs/PORTING.md` and port the diff.
3. Bump the mirror crate's independent SemVer as appropriate, and update
   `UPSTREAM.toml`; never copy the Python version into the crate manifest.
4. Re-capture golden fixtures against the new version; make CI green.
5. Add a `CHANGELOG.md` entry noting the upstream version absorbed.

`rich-ext` code should keep working across syncs because it only touches core
through public APIs / extension traits. If a sync forces an `rich-ext` change,
that's a signal the coupling was too tight — prefer widening a core trait.

---

## Long-running and delegated work: save state as you go

Sessions and agents can end without warning (a time or usage limit, a
restarted container). Work that exists only in a worktree is lost with it.

- **Commit at every milestone**, not only at the end. Unfinished work is
  committed too, marked `WIP` in the message with a line saying what is
  done, partial and left.
- **When a session may run out**, every running agent commits its state at
  once, and the lead pushes each in-flight branch to a backup branch named
  `<working branch>-wip-<name>` (never onto the working branch itself), so
  nothing lives only on the machine.
- **Keep a handoff note** for a multi-step effort
  ([docs/design/intuituive-status.md](docs/design/intuituive-status.md) is
  the demo loop's): what is merged, what is in flight on which branch, and
  what is next, updated whenever that changes. A new session starts there.
- **Delegated agents** are told the same in their brief: commit at
  milestones, mark WIP, and list what is left.

## Before you commit

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

All three must pass. CI runs the same. Keep comments matching the surrounding
density and cite the upstream module in ported files.
