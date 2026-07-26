# Contributing

Thanks for helping build the Rust `rich` port! The golden rule of this project is
in **[AGENTS.md](AGENTS.md)** — please read it first. The short version:

- **`crates/rich` and `crates/rich-cli` are faithful mirrors of upstream.** Match
  upstream behavior exactly; their versions track upstream and only move on a sync.
- **Our own features go in `crates/rich-ext`**, which versions independently.
- The dependency arrow is one-way: `rich-cli → rich-ext → rich`.

## Common workflows

Both are driven by skills (and documented for humans too):

- **Port a module** → [`.claude/skills/port-module/SKILL.md`](.claude/skills/port-module/SKILL.md)
- **Sync a new upstream release** → [`.claude/skills/sync-upstream/SKILL.md`](.claude/skills/sync-upstream/SKILL.md)

Pick up a `type:port` issue, follow the skill, and open a PR.

## Parity is the point

New faithful-port behavior needs a **golden test** captured from real `rich`:

```bash
pip install "rich==$(grep -m1 -A2 '\[rich\]' UPSTREAM.toml | grep version | cut -d'"' -f2)"
python scripts/capture_golden.py
```

Then assert byte-equality in `crates/rich/tests/`. Only genuinely-upstream output
goes in golden fixtures — our conveniences get ordinary unit tests.

## Before every PR

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same three. PRs that change a module should also update its row in
[docs/PORTING.md](docs/PORTING.md) and tick the relevant roadmap issue.

## Deviations

If you must deviate from upstream in core, it goes behind an off-by-default Cargo
feature and is recorded in [docs/DIVERGENCES.md](docs/DIVERGENCES.md). If you're
adding something upstream doesn't have, it belongs in `rich-ext`, not core.
