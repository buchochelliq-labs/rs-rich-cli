---
name: Port a module
about: Track porting one upstream `rich` module to Rust
title: "port: <module>"
labels: ["type:port", "area:core"]
---

## Module

Upstream: `rich/<module>.py` (+ any siblings: ______)
Target: `crates/rich/src/<file>.rs`
Pinned upstream ref: see `UPSTREAM.toml`

## Scope

- [ ] Port public types/functions faithfully (cite upstream in doc comments)
- [ ] Reuse existing core types instead of re-deriving
- [ ] Add extension-point trait to `protocol.rs` if upstream exposes one
- [ ] Golden parity cases added to `scripts/capture_golden.py` + `crates/rich/tests/`
- [ ] Unit tests for behavior awkward to golden-capture
- [ ] `docs/PORTING.md` status updated
- [ ] Any unavoidable deviation recorded in `docs/DIVERGENCES.md`

## Notes

Follow the `port-module` skill. Anything that is *not* faithful upstream behavior
belongs in `rich-ext`, not here.
