---
name: port-module
description: Port a single Python `rich` module to Rust in this repo, with parity tests. Use when the user says "port <module>", "implement rich's table/panel/progress/…", or picks up a roadmap porting issue.
---

# Port one `rich` module to Rust

Goal: faithfully port a single upstream module into `crates/rich`, proven with
golden parity tests. Read [AGENTS.md](../../../AGENTS.md) first.

## 1. Locate

- Find the module row in [docs/PORTING.md](../../../docs/PORTING.md): it names the
  target `crates/rich/src/<file>.rs` and any sibling `.py` files that fold in.
- Read the upstream source **at the pinned ref** in
  [`UPSTREAM.toml`](../../../UPSTREAM.toml) (not `master`), e.g.:
  ```bash
  gh api repos/Textualize/rich/contents/rich/<module>.py?ref=<git_tag> \
    --jq '.content' | base64 -d
  ```

## 2. Port faithfully

- Mirror upstream's structure, public names, and semantics. Rust-idiomatic where
  it doesn't change behavior (e.g. `Option<bool>` for tri-state flags, `Result`
  for what upstream raises).
- Start the file with `//! Port of upstream rich/<module>.py` and cite the
  functions you're porting in doc comments.
- Reuse existing core types (`Color`, `Style`, `Segment`, `Text`, `Console`)
  rather than re-deriving them.
- Keep it in **core** only if it's faithful upstream behavior. Anything we're
  adding on top goes in `crates/rich-ext` — see AGENTS.md.
- If the module is an extension seam upstream (a protocol/ABC), add the trait to
  `crates/rich/src/protocol.rs`.

## 3. Prove parity

1. Add representative cases to `scripts/capture_golden.py` (only genuinely-upstream
   behavior — not our conveniences).
2. Capture from the real library at the pinned version:
   ```bash
   pip install "rich==<version-from-UPSTREAM.toml>"
   python scripts/capture_golden.py
   ```
3. Assert byte-equality in `crates/rich/tests/` (extend `golden.rs` or add a
   sibling). Also add focused `#[cfg(test)]` unit tests in the module for behavior
   that's awkward to golden-capture.

## 4. Record

- Update the module's **status** and **parity** columns in `docs/PORTING.md`
  (🟡 partial / 🟢 complete; ✅ when golden-tested).
- Note any unavoidable deviation in `docs/DIVERGENCES.md` (should be rare).
- Tick the item in the module's roadmap issue.
- **Showcase it in the demo.** If the new module is a user-visible renderable or
  helper, add it to `run_demo` in `crates/rich-cli/src/main.rs` so
  `cargo run -p rich-cli` always demonstrates every capability ported so far.
  Verify with `COLUMNS=64 cargo run -p rich-cli`.

## 5. Gate

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

## Done when

The module compiles, its behavior matches upstream in golden + unit tests, the
docs/PORTING.md status is updated, and the full gate passes.
