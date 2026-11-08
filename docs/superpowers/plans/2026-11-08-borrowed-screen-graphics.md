# Borrowed Screen Graphics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Avoid reconstructing an owned Frame solely to discover graphics in the already-composed Screen.

**Architecture:** Add a provided Screen entry point to PlacementSource, with the exact old conversion as default. Driver uses it; the stock optional image source shares its existing rectangle builder between Frame and borrowed Screen row adapters.

**Tech Stack:** Rust, rich-ext PlacementSource/Screen, optional intuiTUIve art integration, existing image clipping/compositing tests and public consumer.

**Spec:** `docs/superpowers/specs/2026-11-08-borrowed-screen-graphics-design.md`.

## Global Constraints

- Additive provided trait method only; existing PlacementSource implementors, Frame callers, public literals, feature boundaries and defaults remain compatible.
- Generic core contains no browser/image-specific metadata parser. Stock optional image integration owns image discovery; no edits to faithful crates/rich or private tag wire format.
- Discover surviving composed cells and exact provenance; preserve legacy run grouping, source columns/rows, adjacent copies, holes, fragment limits, crop geometry and resource retirement order.
- Borrow Screen styles only within the call; no persistent StyleId cache, whole retained-style-table clone or new provenance plane.
- Original application gates, quality, offers and historical evidence stay fixed; component diagnostics are not release acceptance. Quiet timing requires root lease.
- One source/Cargo implementer at a time; no worker helpers/reviewers, push, merge or publication.

## Review Focus

- Legacy tags without source-column metadata require full equal-style run grouping; fragmented legacy rows fall back exactly as before (Task 1 parity).
- Two adjacent or translated copies with identical IDs must not merge into one false source rectangle (Task 1 copied-scene parity).
- Covered/wide/cleared cells and changed metrics must not preserve stale crops or placements (Task 1 composed-scene mutation tests).
- Third-party sources overriding only Frame or metrics methods must still receive the same geometry and measured pixels (Task 1 external consumer).
- A borrowed scan must not retain StyleIds across Screen compaction/blits or release IDs before ordered successful output (Task 1 lifecycle/consumer proof).

---

### Task 1: Provided Screen placement entry point and image override

**Files:**
- Modify/test: `crates/rich-ext/src/frame/graphics.rs` (provided trait method/default parity).
- Modify/test: `crates/rich-intuituive/src/image.rs` (borrowed adapter/shared rectangle construction).
- Modify: `crates/rich-intuituive/src/app.rs` (`draw_graphics` calls new entry point).
- Extend: existing `crates/rich-intuituive/tests/image_clipping.rs`, `image_compositing.rs` and external public consumer as appropriate for actual target names.
- Document: `docs/guide/intuituive-images.md` if present, else nearest existing image guide section; no new duplicate guide.

**Interfaces:**
- Consumes unchanged `PlacementSource::placements(&Frame)` and `placements_with_metrics(&Frame, Option<(u16,u16)>)`.
- Produces provided `fn placements_from_screen(&self, screen: &crate::surface::Screen, cell_pixels: Option<(u16,u16)>) -> Vec<Placement>` on the public trait. Its default uses the precise previous Driver conversion and calls `placements_with_metrics`.
- Driver calls that method with the composed Screen/measured cell pixels; the stock ImageGraphics override borrows rows and shares one private rectangle/placement algorithm with the Frame route. Private helper names/types are implementation choices.

- [ ] **Step 1: RED public dispatch contract.** Write a minimal custom source whose Frame method records Frame geometry/metrics and whose new Screen method deliberately records borrowed invocation. A Driver paint must invoke the new method, not reconstruct Frame. Record intended RED; the initial missing API compile error alone is not the behavioral proof. Verify default-only legacy implementor parity separately after the method exists.
- [ ] **Step 2: Add provided method and route Driver.** Move exact old Screen export/Frame build into the provided fallback and call it from Driver. Preserve height filtering, painter changes, diff/repaint, reconcile/output-written behavior. Confirm default-only external source gets the same Frame rows and metrics, with no implementation migration.
- [ ] **Step 3: Implement the image override with parity.** Use a borrowed Screen row-run adapter that groups equal styles with the same cell lengths as the Frame export, including continuations. Feed a single shared rectangle builder; do not duplicate merging/fragment-limit logic. Preserve every source-column distinction and full legacy equal-style span. Existing public Frame routes remain reference. No bundled tag decoder or long-lived cache in this scope.
- [ ] **Step 4: Prove composed-scene equivalence.** Compare placements and decoded drawing pixels/keys/crops from both routes on fixed bound and legacy scenes: partial left/right/top/bottom, a center overlay/hole, two adjacent copies, translated copied crop, newline/control/wide metadata cells, missing provenance and fragment overflow. Mutate/clear/blit/compact the same Screen and change known/unknown cell metrics before subsequent scans; no stale placement may survive. Exercise Kitty/iTerm/Sixel with existing public APIs and validate retirement only after successful output. Extend existing tests rather than duplicate their whole corpus.
- [ ] **Step 5: Verify public reuse and checks.** Extend isolated advertised-feature consumer with direct borrowed invocation and a source implementing only existing required methods. Run focused rich-ext trait tests, intui art image clipping/compositing tests and modified-crate suites/lints appropriate to change. Record baseline clippy exceptions explicitly; fmt and diff-check. No repeated full-workspace run unless new concerns justify it.
- [ ] **Step 6: Frozen diagnostic and commit.** Under root quiet grant, one identical bound 41x14 composed-Screen component comparison of old export/build/placements vs new method; verify output equality, preserve raw allocation/time evidence, label diagnostics only. Commit focused source/tests/docs, self-review and report exact BASE/HEAD, checks, compatibility and limitations. Root dispatches independent task review, then schedules original application acceptance after remaining relevant implementation freezes.
