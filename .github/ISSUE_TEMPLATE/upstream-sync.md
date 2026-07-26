---
name: Upstream sync
about: Track syncing the port to a new upstream `rich` / `rich-cli` release
title: "sync: rich <version>"
labels: ["upstream-sync", "type:infra"]
---

## Target

- Project: `rich` / `rich-cli` (delete one)
- New version: ______
- New tag/sha: ______
- Currently pinned in `UPSTREAM.toml`: ______

## Checklist (see the `sync-upstream` skill)

- [ ] Diff upstream `<pinned_sha>..<new_tag>`
- [ ] Map changed modules via `docs/PORTING.md`
- [ ] Port the diffs into the mirror crate (no `rich-ext` behavior in core)
- [ ] Bump mirror crate `version` to the exact upstream version
- [ ] Update `UPSTREAM.toml` (version, tag, sha)
- [ ] `pip install rich==<version>` + `python scripts/capture_golden.py`; investigate every changed fixture
- [ ] `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
- [ ] Confirm `rich-ext` still builds/tests unchanged
- [ ] `CHANGELOG.md` entry + `docs/PORTING.md` status updates
