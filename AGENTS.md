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

> **The core (`crates/rich`, `crates/rich-cli`) is a faithful mirror. Our own
> features go in `crates/rich-ext`. Never mix the two.**

The dependency graph is one-directional and must stay that way:

```
rich-cli ──▶ rich-ext ──▶ rich
                          (core: no deps on ext/cli, no knowledge of them)
```

If a change would make `crates/rich` diverge from upstream `rich`, it is almost
certainly in the wrong place — see [Adding our own features](#adding-our-own-features).

---

## Versioning

**Every crate versions independently, by its own SemVer. Started at `0.0.1`.**

| crate          | version source    | when it bumps                          |
|----------------|-------------------|----------------------------------------|
| `rs-rich`      | independent SemVer | whenever we ship anything              |
| `rs-rich-cli`  | independent SemVer | whenever we ship anything              |
| `rs-rich-ext`  | independent SemVer | whenever we ship anything              |
| `rs-rich-art`  | independent SemVer | whenever we ship anything              |

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

- **Never** bump a mirror crate to ship one of *our* features. Features still go
  in `rich-ext`. The mirror/ext boundary is unchanged; only the numbering is.

---

## Adding our own features

1. **Default location: `crates/rich-ext`.** New renderables, highlighters, boxes,
   themes, CLI conveniences — all go here. Register them onto a `Console` through
   the extension registry (see [docs/PLUGINS.md](docs/PLUGINS.md)); do not reach
   into core internals.
2. **If a feature genuinely needs a core hook**, add it as a new *extension-point
   trait* in `crates/rich/src/protocol.rs` (the sanctioned seam), keeping core's
   own behavior unchanged. Extensions implement the trait from `rich-ext`.
3. **If — and only if — a feature is impossible without changing core behavior**,
   gate it behind a Cargo `feature` flag that is **off by default**, and record it
   in [docs/DIVERGENCES.md](docs/DIVERGENCES.md). A default build of `crates/rich`
   must always behave like upstream.

If you find yourself editing a faithful-port module to add non-upstream behavior,
stop and move it to `rich-ext`.

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
3. Bump the mirror crate's `version` to match, update `UPSTREAM.toml`.
4. Re-capture golden fixtures against the new version; make CI green.
5. Add a `CHANGELOG.md` entry noting the upstream version absorbed.

`rich-ext` code should keep working across syncs because it only touches core
through public APIs / extension traits. If a sync forces an `rich-ext` change,
that's a signal the coupling was too tight — prefer widening a core trait.

---

## Before you commit

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

All three must pass. CI runs the same. Keep comments matching the surrounding
density and cite the upstream module in ported files.
