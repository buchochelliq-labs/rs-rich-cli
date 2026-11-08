# Source Layout Integrity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve visible source content and grouping while excluding proven source-clipped content.

**Architecture:** Repair Noctiluca's private DOM extraction and immutable geometry bookkeeping. Continue using the existing shared rendering/layout/clip primitives; keep public model construction and source actions compatible.

**Tech Stack:** Rust, DOMSnapshot/AX, existing PageBuilder tests and public layout projection; saved real-page public consumer for attribution.

**Spec:** `docs/superpowers/specs/2026-11-08-source-layout-integrity-design.md`.

## Global Constraints

- No site-specific selectors/rules, inferred cards/controls, access bypass, new renderer or edits to faithful crates/rich.
- Keep public Page/Block/Grid/Projection and other complete struct literals compatible; private extraction/geometry state may be additive.
- Preserve source node/revision/destination/action binding, hints, image-text association, anchors, semantic twins/colors and popup ownership.
- Existing public shared clipping/adaptive-columns/wrapping/scroll/layer APIs remain the rendering facilities. Browser visibility/source-group interpretation stays in the adapter.
- Use immutable source membership and consistent nested transform coordinates. Exclude only proven fully clipped content; partial/uncertain positioned content is distinct.
- One source/Cargo implementer at a time, no worker helpers/reviewers; quiet timing lease separate; no push/merge/publish.

## Review Focus

- A boxless source link wrapping visible media must keep the original action and image, while hidden no-layout content stays absent (Task1).
- A boxed child must remain one source grid item even when it contains many laid-out descendants (Task3).
- A partial clip or fixed/absolute escape must not be hidden by a generic ancestor predicate (Task2).
- Repeated scroll offsets and nested clips must not be applied twice or clamped to scroll-content width (Task2).
- Parent stacking followed by child reflow must keep the same linked heading/summary/media together despite glyph overhang and reverse visual/DOM order (Task3).

---

### Task 1: Retain visible linked content and valid image sources

**Files:** Modify/test `crates/noctiluca/src/model.rs`; neutral fixtures under existing model tests; use `testing.rs` only for missing fixture support.

**Interfaces:** Existing public `model::build(&PageSnapshot, &mut Revisions) -> Page` and model shapes stay unchanged. Add private Dom traversal/visible footprint helpers as needed. Source link controls retain their actual node/revision/destination; image source selection filters empty selected URLs before falling back to a nonempty declared source. Grid extraction belongs to Task3.

- [x] Add neutral no-own-box link containing laid-out image/text; assert retained media and visible spans bind original source link, full target/name/revision survive, and hidden/no-descendant-layout variant emits none. Add a loading image with a present-but-empty selected source and nonempty relative src: expect the resolved image URL, never the document URL. With both sources empty, preserve absent/pending behavior. A later nonempty selected responsive source wins. Record intended RED on current omissions/URL selection.
- [x] Implement minimal private traversal and source-link handling. Use actual visible descendants for footprint; do not assign new target IDs, infer roles or alter unrelated non-link control rendering. Prefer nonempty selected image source, then nonempty declared source; do not classify or replace valid 1x1 sources.
- [x] Run focused regressions, then Noctiluca model/layout suites. Run unchanged external saved-source consumer to a NEW output directory and verify BBC image364/link361 now retained and original link identity retained; source loading does not manufacture page-URL media. IDs are evidence only. MDN grid extraction is Task3.
- [x] Fmt/scoped clippy, document exact baseline blockers, self-review and commit; report BASE/HEAD, RED/GREEN, public outputs and limits. Root task review before Task2.

### Task 2: Exclude proven overflow-clipped descendants

**Files:** Modify/test `crates/noctiluca/src/model.rs`; existing neutral snapshot builder/fixtures as needed.

**Interfaces:** Consumes existing private DOM parent/layout/client/scroll/style data and Task1 traversal. Produces unchanged public model with conservative source visibility filtering, no new required fields or snapshot-style migration.

- [ ] Add ordinary static overflow-hidden strips with wholly outside linked text/media and intersecting siblings. Assert fully outside content has no visible projected text/hints/regions; partial intersection remains admitted. Cover x-only/y-only clips, nested clips, boxless intermediates and already-scrolled coordinates. Add absolute/fixed escape case and root/body viewport-overflow case that must remain visible. Record intended RED.
- [ ] Implement conservative ancestor-clip rejection using clipped axis and present source bounds. Do not reject across uncertain positioned chains, guess transforms/containment, use scrollWidth as clip edge, or globally clip to page viewport. Preserve semantic popup/twin behavior.
- [ ] Run model/layout/popup regressions. Rerun unchanged saved-source consumer to new directory; Giphy wholly hidden labels484–492 must no longer create the prior right-edge spans, and partly intersecting483 must remain a separate admitted case. Source actions for retained items unchanged.
- [ ] Fmt/scoped lint, self-review, commit and report with proof/limits; root independent task review before Task3.

### Task 3: Preserve immutable grid membership through nested transforms

**Files:** Modify/test `crates/noctiluca/src/layout.rs` and source-grid extraction in `crates/noctiluca/src/model.rs`; model-built neutral fixtures under existing tests, no public Grid/Block field additions.

**Interfaces:** Public `project`/`project_with` and Projection stay unchanged. Private Item/source geometry and grid transform bookkeeping must keep original enclosing-frame membership distinct from current projected geometry; retain source/source-column provenance for image containment.

- [ ] Add boxless/nested-boxless grid wrappers and a normally boxed child; assert first generated layout boxes/source order survive without flattening boxed children. Add primary header/body and complementary sidebar with unequal row spans: admitting a grid alone is insufficient; preserve association and avoid additional empty runs versus the reference. Add two nested declared grids, two asymmetric items in DOM order opposite visual x order, heading glyph top1px above its enclosing frame, related image/summary/metadata and an independent neighboring box. At40/80/120columns assert all pieces of each source item remain associated after side-by-side/stacked resize, original action IDs/hints survive, independent neighbor stays independent and reader anchor is stable on unchanged-geometry decode. Include a single-grid control and adaptation-off reference. Record intended RED from split membership/transformed-coordinate behavior.
- [ ] Retain actual grid layout boxes through boxless wrappers, then repair membership based on original source containment/frame before transforms and compose parent/child movement in consistent coordinates. Source grids with unequal row spans must not be blindly packed as equal-height repeated cards; preserve source relationships or conservatively bypass an unsafe snap. Do not force MDN outer-grid admission at the cost of header/body association. Do not increase a positional tolerance to paper over glyph overhang, regroup by title/proximity or rewrite general CSS layout. If existing rectangles cannot safely establish membership, report the exact ambiguous neutral case before introducing a private/additive source plan.
- [ ] Run layout/product/media/popup/anchor regressions. Saved-source consumer must keep BBC heading372 and its related summary/metadata in their declared item after adaptation (resolve current control by source node/destination; ordinal12 from the old snapshot is not stable after retained links); retain fresh separate outputs and compare MDN nested behavior without attributing all gaps to Task1.
- [ ] Build current Noctiluca for root native BBC/Giphy/MDN screenshots; record source-version/config and dynamic content limits. Fmt/scoped lint, self-review, commit and report. Root task review and later broad continuation review remain required; no application gate claims from source fixtures.
