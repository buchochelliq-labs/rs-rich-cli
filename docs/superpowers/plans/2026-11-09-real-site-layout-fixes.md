# Real-Site Layout Fixes Implementation Plan

> **For agentic workers:** Execute inline from the isolated `codex/noctiluca-media-quality` branch. Each task is independently tested and committed.

**Goal:** Improve generic Noctiluca page layouts for real multi-column content and recognize accessible live-chat panes beside video.

**Architecture:** Keep web semantics in Noctiluca. Use existing public intuiTUIve adaptive grid and scroll primitives; change only Noctiluca's semantic extraction and allocation thresholds. Do not add site selectors or new browser-specific APIs to intuiTUIve.

**Tech Stack:** Rust, Noctiluca model/source projection tests, existing `adaptive_columns` and `StackLayout`.

**Spec:** `docs/superpowers/specs/2026-11-08-noctiluca-media-quality-design.md`, with real-site evidence in `/workspace/work/noctiluca-real-sites-review.md`.

## Global Constraints

- No site-specific selectors, domain names, guessed controls, access bypass, or new renderer.
- Preserve public Page/Block/Grid/Projection contracts, browser action identities, and source order.
- Keep web-page semantics in Noctiluca; shared intuiTUIve APIs remain generic and unchanged.
- Use failing regression tests before production changes.
- Verify Noctiluca package tests, formatting, and scoped Clippy; report native/performance gates separately.

## Review Focus

- A named section with an `aria-live` log beside a video must qualify as a generic video/log pair, but plain status text must not (Task 1).
- Equal source cards must keep readable terminal widths after rearrangement at 40, 80, and 120 columns (Task 2).
- Unequal side-by-side tracks may stack only when a source track is narrower than the documented minimum; sufficiently wide unequal layouts preserve their source arrangement (Task 2).
- Card captions, media, text, and actions remain inside the same source item and preserve reading order after wrapping (Task 2).
- Existing `role=log`, legacy source-less grids, and faithful `--no-adapt` output retain their behavior (Tasks 1–2).

---

### Task 1: Recognize accessible live logs in video/log pairs

**Files:** Modify `crates/noctiluca/src/model.rs`; test `crates/noctiluca/src/model/source_projection_tests.rs`.

**Interfaces:** No public type or API changes. Reuse `Walker::log_kind` as the semantic classifier for descendant chat/log containers.

- [x] Add a neutral flex-container regression with named video and chat sections; the chat descendant uses `aria-live="polite"` with element entries. Assert source grid recognition and correct source items; include a non-log status control case.
- [x] Run the focused regression and confirm it fails because the current classifier only accepts a literal `role="log"` descendant.
- [x] Change paired-section detection to call the existing live-log classifier, retaining the exactly-two-named-pane and exclusive video/log conditions.
- [x] Re-run the focused case plus existing live-log and model tests; commit the task.

### Task 2: Reflow narrow cards and unequal columns

**Files:** Modify `crates/noctiluca/src/adapt.rs` and `crates/noctiluca/src/layout/source.rs`; test `crates/noctiluca/src/model/source_projection_tests.rs` and `crates/noctiluca/tests/adapt.rs`.

**Interfaces:** Keep `Rule::SnapGrids`, `adaptive_columns`, `project_source`, and all public geometry unchanged. Set the generic Noctiluca minimum card width to 32 cells. Preserve unequal source grids when every track meets that width; stack an unequal grid as ordered source units when a track is narrower.

- [x] Add four equal cards in one source row at 120 columns. Assert that three fit the first row and the fourth moves below; assert every card label is present once and in source order. Confirm the current 16-cell minimum fails this oracle.
- [x] Add an unequal 30/70-cell source row at 100 columns. Assert the 30-cell track triggers ordered stacking; add a 40/60-cell control that remains side-by-side.
- [x] Implement the 32-cell minimum and narrow-track condition without changing the paired video/log minimum or unsupported/positioned-grid behavior.
- [x] Run focused grid, nested figure, source ordering, popup, and model/layout tests; inspect the diff and commit.
- [x] Update the existing 72-column adaptive-grid integration test to assert readable-width stacking in source order; the old test encoded the former 16-cell threshold.

### Task 3: Verify and prepare review

**Files:** update `docs/design/noctiluca-media-quality-status.md` and the local PR description/evidence manifest.

**Interfaces:** Consume Tasks 1–2 without altering their behavior.

- [ ] Run the complete Noctiluca package suite, scoped Clippy, fmt and `git diff --check`.
- [ ] Build the current CLI if disk permits; rerun the eight-site Kitty capture set and inspect before/after images for layout movement and cleanup.
- [ ] Record the unchanged iTerm-compatible performance gate as open if it cannot be run safely; do not claim release qualification.
- [ ] Perform a fresh whole-branch review, resolve material findings, then push the feature branch and open a PR for user review. Do not merge.
