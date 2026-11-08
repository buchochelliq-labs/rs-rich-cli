# Noctiluca media quality: handoff

Updated: 2026-11-08.

## Working state

- Branch: `codex/noctiluca-media-quality` in an isolated local clone.
- Source baseline: `7fcdbf7263f81d89c2975a47ee4761b4a0e1c7cc`.
- Specification: `docs/superpowers/specs/2026-11-08-noctiluca-media-quality-design.md`.
- Plan: `docs/superpowers/plans/2026-11-08-noctiluca-media-quality.md`.
- User prioritises video/chat, image bleed and judder; adaptive rearrangement is permitted; reusable components must ship through the framework's public widgets.
- Official Rust 1.90 toolchain prepared under `/workspace/work/toolchain`; locked baseline Noctiluca build passed, binary reports 0.1.0. Logs: `/workspace/work/noctiluca-baseline-build.log`.
- Focused baseline: 90 library + 8 image + 10 picture tests passed (108 total).
- Chromium could not start inside the nested test sandbox; outside it, sandbox-enabled Chrome successfully accessed local HTTP fixtures. Browser policy blocked file URLs; no sandbox override was used.
- Workspace checks require Rust 1.91 because `rich-lumis` explicitly declares the higher floor; the initial 1.90 workspace failure is a toolchain mismatch. Explicit 1.91 checks preserve the default 1.90 toolchain.
- Formatting passed. Rust 1.91 whole-workspace clippy failed on existing `clippy::nonminimal_bool` diagnostics in `crates/rich/src/markdown_url.rs:197` and `crates/rich/src/pretty.rs:258` (three diagnostics). No unrelated source fixes were applied. Whole-workspace tests exhausted disk space while linking, before running.
- Documentation milestone `6f91ea8` and partial Task 1 corpus/baseline milestone `81648e2` are committed. Whole-workspace checks remain incomplete or failing as described above.
- Independent specification review: no important unanswered questions; component API audit incorporated (public struct compatibility, physical/CSS metrics distinction, full cache invalidation and browser playback authority).
- Task 1 baseline added fixture controls and browser-free tests; the first review requested corrections to intrinsic sizing and keyed chat edits. Product fixes, new widgets and native-terminal acceptance are not yet implemented or verified.

## Next

Task 2 is in progress: finish terminal metrics and capability work and its review before Task 3 lifecycle work, including deterministic completion after invalidation. Task 4 must measure layout calls, encoding, latency, queue and memory costs across the workloads; Task 6 must record native-terminal and transport evidence. Preserve generic widgets and browser action guards. Do not select a production fix solely from the initial baseline.
