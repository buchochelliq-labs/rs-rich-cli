# Screen Metadata Transport Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove redundant temporary Frame construction for ordinary Screen metadata while preserving exact existing cells.

**Architecture:** Keep the existing direct cell writer and narrow its rejection condition to the metadata that Frame actually interprets. Preserve the old Frame writer as fallback and parity oracle. All behavior lives in shared rich-ext and is available to every public Screen consumer.

**Tech Stack:** Rust 1.91 workspace tests, Rust 1.90 release diagnostics; rich-ext surface/frame; optional shared art/intui consumers.

**Spec:** `docs/superpowers/specs/2026-11-08-screen-metadata-transport-design.md` (parent design remains binding).

## Global Constraints

- No edits to faithful `crates/rich`, dependency/feature changes or browser-specific metadata parsing in generic Screen.
- No new required public fields or changed public signatures/defaults.
- Preserve complete ordinary metadata, links, visual styles, Unicode widths, continuation/combining cells, controls, clipping and clearing exactly as Frame does.
- Recognised region tags and embedded newlines retain the existing Frame fallback.
- No native/performance acceptance runs concurrently with builds, browsers or other CPU jobs. Preserve original gates and all historical evidence; diagnostics are not acceptance.
- One source/Cargo implementer at a time; commit milestones; do not spawn helpers/reviewers. Root owns review dispatch and integration; no push/merge/publish.

## Review Focus

- A recognised region key mixed with ordinary metadata must strip only what Frame strips; malformed keys preserve the Frame result (Task 1 parity).
- Metadata on Unicode combining fragments and wide glyphs must survive edge splitting and zero-width joins (Task 1 parity).
- Suppressed control Segments, visible controls and embedded newlines must retain separate existing meanings (Task 1 fallback/parity).
- Rewriting a previously wide/multiline region with shorter tagged text must clear stale cells and avoid stale metadata (Task 1 mutation parity).
- `write_runs` styles belong to their Screen; compaction/blits must preserve full metadata without a persistent StyleId cache (Task 1 public consumer and mutation parity).

---

### Task 1: Direct ordinary-metadata Screen writes

**Files:**
- Modify/test: `crates/rich-ext/src/surface/screen.rs` (`write_simple_lines`, `write_runs`, existing parity tests).
- Reference only: `crates/rich-ext/src/frame.rs`, `crates/rich-ext/src/frame/regions.rs`, `crates/rich/src/protocol.rs`.
- Create external diagnostic/consumer artifacts under `/workspace/work/noctiluca-continuation-screen/` (not production APIs).

**Interfaces:**
- Consumes unchanged `Screen::write_lines(&mut self, rect: Rect, lines: &[Vec<Segment>])` and `Screen::write_runs<'t, L: AsRef<[TextRun<'t>]>>(&mut self, rect: Rect, lines: &[L])`.
- Produces the same signatures and exact cells, with ordinary-metadata rows taking `write_simple_lines`/direct runs rather than a temporary Frame. Private eligibility helpers are optional; use the existing public recognised-region decoder as the single authority, not copied region-format logic.

- [ ] **Step 1: Diagnostic activation.** Read `/workspace/work/noctiluca-continuation-perf-probe/report.md` and the root activation ruling in this plan's progress ledger. Do not implement unless current bound temporary-Frame insertion is established as material; report a mismatch before editing.
- [ ] **Step 2: Add regression proof before implementation.** Extend the existing private Frame parity harness: ordinary metadata alone and with links/visual styles; mixed ordinary+valid semantic region metadata; malformed/negative/unknown region values; metadata on ASCII, CJK, combining fragments across Segments and joined emoji. Check full exported styles, cell text/width/continuations and overwrite clearing at nonzero origins and narrow edges. Cover suppressed control Segments, control characters in text and embedded newlines. For ordinary no-newline cases assert the direct writer accepts them; that assertion must fail on the current source while Frame-result parity holds. For actual region/newline cases assert fallback remains required. Repeat semantically relevant cases through public `write_runs` using interned styles.
- [ ] **Step 3: Run the targeted regression and record RED.** `cargo +1.91.0 test -p rs-rich-ext --lib metadata` with task-specific test names/filter as needed. Failure must be the intended rejected ordinary direct path, not a compile/setup error.
- [ ] **Step 4: Narrow direct eligibility in Screen.** Permit ordinary metadata; use the existing recognised-region interpretation for the fallback decision. Keep full ordinary Style interning and all write-row/control/newline semantics. Apply the same rule to `write_runs`. No image-specific key checks, metadata stripping, new cache, Frame refactor or unrelated cleanup.
- [ ] **Step 5: Verify.** Run focused regression, `cargo +1.91.0 test -p rs-rich-ext`, and existing intuiTUIve `image_clipping`/`image_compositing` targets with their advertised art feature. Run fmt and modified-crate strict clippy; if a pre-existing mirror lint blocks it, preserve exact failure then allow only the proven baseline category for the scoped check. Report commands, totals and limits. Do not rerun the whole workspace without a new concern.
- [ ] **Step 6: Public consumer and cost comparison.** Create an isolated external Cargo consumer using public rich-ext APIs with declared features. Verify ordinary metadata+link survives `write_lines`, `write_runs`, blit and export, including two successive overwrites. Reuse the diagnostic's frozen bound 41x14 rows for a single declared reference-Frame vs direct component replay with output equality and allocation/time measurements. Preserve raw logs, no application gate claims, no trial fishing. Coordinate root quiet grant before timing.
- [ ] **Step 7: Self-review and commit.** Commit focused source/tests, write task report with exact BASE/HEAD, paths to RED/GREEN/consumer/diagnostic results, allocation/timer caveats and remaining application blocker. Root dispatches the independent task review. Do not implement Screen placement discovery or image source recovery in this task.
