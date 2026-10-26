# Contributing

Thanks for helping build the Rust `rich` port! The golden rule of this project is
in **[AGENTS.md](AGENTS.md)** — please read it first. The short version:

- **`crates/rich` is a faithful mirror of upstream.** Match upstream behavior
  exactly.
- **Our own renderers and library features go in `crates/rich-ext`** (or
  `crates/rich-art`), never in core. Documented `rs-rich-cli` binary-boundary
  conveniences may live in `crates/rich-cli` when they only compose public APIs.
- The dependency arrow is one-way: `rich-cli → rich-ext → rich` and
  `rich-cli → rich-art → rich`, with `rich-ext → rich-plugin-api → rich` and
  `rich-ext → rich-macros` behind ext's optional `macros` feature, and
  `rich-cli → rich-mermaid → rich-plugin-api`, and `rich-lumis → rich-plugin-api`, and `rich-cli → rich-record → rich-ext`, and `rich-interact → rich-ext`, and `rich-micro → rich-ext`, and `rich-mermaid → rich-diagram → rich`, and `rich-cli → rich-data → rich-ext`, and `rich-ratatui → rich` (with `rich-interact` behind its `interact` feature), and `rich-intuituive → rich-interact → rich-ext`, and `rich-web → rich-intuituive`, and `rich-embed → rich-intuituive`, `rich-record`.
  Core depends on none of them.
- Each crate (`rs-rich`, `rs-rich-plugin-api`, `rs-rich-macros`, `rs-rich-ext`,
  `rs-rich-art`, `rs-rich-mermaid`, `rs-rich-lumis`, `rs-rich-record`, `rs-rich-interact`, `rs-rich-micro`, `rs-rich-diagram`, `rs-rich-data`, `rs-rich-ratatui`, `rs-rich-intuituive`, `rs-rich-web`, `rs-rich-embed`, `rs-rich-cli`) has its **own independent SemVer**, bumped
  whenever that crate ships. Which upstream release we track lives in
  `UPSTREAM.toml`, not in a version number — see AGENTS.md → Versioning.

## Common workflows

Both are driven by skills (and documented for humans too):

- **Port a module** → [`.claude/skills/port-module/SKILL.md`](.claude/skills/port-module/SKILL.md)
- **Sync a new upstream release** → [`.claude/skills/sync-upstream/SKILL.md`](.claude/skills/sync-upstream/SKILL.md)
- **Cut a release** → [`.claude/skills/release/SKILL.md`](.claude/skills/release/SKILL.md)

Pick up a `type:port` issue, follow the skill, and open a PR.

## Branching

Read **[docs/BRANCHING.md](docs/BRANCHING.md)**. In one line: cut from fresh
`origin/main`, target `main`, and after merging *verify* the work reached `main`
rather than trusting the badge.

That last part is not paranoia — this repo has lost work this way three times.
CI enforces the base branch, merged branches are deleted automatically, and a
local hook refuses to push to a branch whose PR already merged. Enable it once
per clone:

```bash
git config core.hooksPath .githooks
```

## Parity is the point

New faithful-port behavior needs a **golden test** captured from real `rich`:

```bash
pip install "rich==$(grep -m1 -A2 '\[rich\]' UPSTREAM.toml | grep version | cut -d'"' -f2)"
python scripts/capture_golden.py
```

Then assert byte-equality in `crates/rich/tests/`. Only genuinely-upstream output
goes in golden fixtures — our conveniences get ordinary unit tests.

## API conventions

**Builders are `#[must_use]`.** A builder method takes `self` and returns
`Self` (`Node::text("x").bold()`), so a call whose result is dropped does
nothing: the value was moved in and is gone. Every crate except `crates/rich`
(the mirror keeps upstream's shape) turns on
`clippy::return_self_not_must_use` in its `lib.rs`, so such a method must say
so:

- A type that is only useful once used (a renderable, a widget, a component,
  an `*Options` struct, `Node`, `Graph`) carries `#[must_use = "…"]` on the
  type itself, and every function that returns one warns when it is dropped.
- A setter on any other type carries `#[must_use]` on the method.
- A setter on a `Copy` handle that makes its change through shared state
  (`Transport::tick`, `Timer::max_catch_up`) is `#[must_use]` too, though
  the change is made either way: one rule for every builder. A caller
  that only wants the change writes `let _ = transport.tick(d);`.

## Before every PR

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same three. PRs that change a module should also update its row in
[docs/PORTING.md](docs/PORTING.md) and tick the relevant roadmap issue.

### When you change a public API

Every published crate's public API is checked in as a snapshot,
`api/<crate>.txt`, made by `cargo public-api`. CI's `public API and SemVer`
job fails when a crate's API differs from its snapshot, with the diff. If the
change is meant, update the snapshot and commit it, so the review sees the API
change:

```bash
python scripts/public_api.py --install         # prints the commands for the pinned tools
python scripts/public_api.py --bless rs-rich   # or no crate: every one
python scripts/public_api.py --check
```

The same job runs `cargo-semver-checks` against each crate's last release on
crates.io (`python scripts/semver_check.py`). Cargo treats every 0.0.x bump as
breaking, so a 0.0.x crate is checked as if its bump were a minor one: each
breaking change must be listed in `api/semver-breaks.toml` under the version
that ships it, with the reason, and a migration note in the CHANGELOG. Both
tools and the nightly behind the snapshots are pinned in `api/tools.toml`.

The Python package's type stubs are checked against the built wheel with
mypy's `stubtest`, and a typed program (`crates/rich-py/tests/typing_usage.py`)
under `mypy --strict`: `python scripts/check_python_stubs.py`, with `rs_rich`
installed. A new class, function or parameter in the bindings needs its stub
in `_native.pyi`.

## Deviations

If you must deviate from upstream in core, it goes behind an off-by-default Cargo
feature and is recorded in [docs/DIVERGENCES.md](docs/DIVERGENCES.md). If you're
adding something upstream doesn't have, it belongs in `rich-ext`, not core.
