# Source Relationships Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve real source relationships so adaptive projection can keep content and actions together without geometry guesses.

**Architecture:** Add an optional immutable SourcePage companion and atomically deliver it with Page. Reuse source relationships in projection while legacy APIs retain their fallback. Existing shared layout/graphics components provide composition.

**Tech Stack:** Rust, Noctiluca model/tab/delivery/presentation; public intuiTUIve rectangular layout, text and image APIs.

**Spec:** `docs/superpowers/specs/2026-11-08-source-relations-design.md`.

## Global Constraints

- No new required Page/Block/Media/Grid/Projection/FrameMeta/Layer/Job/Config fields or public Update variants. Existing constructors/callbacks remain usable.
- No ownership hidden in strings/repurposed fields/global registries, source actions inferred from appearance, site selectors, reader-mode substitution or CSS engine.
- SourcePage is the exact immutable Page Arc plus private immutable relations; source-aware delivery and current-page state are atomic, not separate latest slots.
- Reuse stable relation identity/value across image/playback/live-only publications, preserving zero unrelated layout and existing source/document/viewport barriers.
- Browser facts remain in Noctiluca; generic layout/graphics/control capability stays public in shared crates. No faithful-rich or Cargo dependency changes.
- Preserve words, source identity/actions/hints, semantic twins/colors, anchors, clipping and source-owned popup/foreground behavior.
- One source/Cargo implementer, no worker helpers/reviewers or external actions; root coordinates native/quiet leases; no push/merge/publish.

## Review Focus

- Equal Page values with changed source ownership still require a new paired publication and projection dependency (Task1).
- Decode/playback/live-only updates must not reuse stale Page ownership or recreate an unrelated graph (Task1).
- Mixed runs and multiple pieces from one node need exact memberships, not a node-only shortcut (Task1/2).
- Nested figures/irregular grids must preserve each child once and independent source groups (Task2).
- Linked-image targets must not cover explicit foreground/caption controls, and canonical aliases must not cross layers (Task2).

---

### Task 1: Retain and atomically publish paired source relationships

**Files:** Modify `crates/noctiluca/src/{model,tab,delivery,app,presentation}.rs`; create focused private `model/source.rs` if appropriate; corresponding model/tab/delivery/presentation tests and public Noct library guide.

**Interfaces:** Public opaque Clone/Debug `model::SourcePage` with `page(&self)->&Arc<Page>`; `build_source(&PageSnapshot,&mut Revisions)->SourcePage`; `build_source_with(&PageSnapshot,&mut Revisions,&HashSet<NodeId>)->SourcePage`; `Tab::spawn_with_source(Box<dyn Engine>, impl Fn(Update,Option<SourcePage>)+Send+'static)->Tab`. Legacy signatures unchanged. Private SourceRelations/build-parts/publication helpers and envelope follow existing code patterns, not a second worker.

- [ ] Add intentional RED for exact Page Arc/source pairing and same-document relationship changes with equal Page, all five publication paths, old constructor/literal consumer compilation, navigation/stale pairing, source-control ownership and nested figure/grid memberships. Test multiple block/run pieces from one node, whitespace cleaning/tidying and Unicode provenance.
- [ ] Implement optional extraction and opaque pair. Finalize memberships after flush/stack; never expose a mutable or unrelated pair constructor. Preserve old model field semantics and legacy build behavior. Store source facts once per snapshot, no pixel/frame copies or global registry.
- [ ] Centralize paired publication for read/live/playback/still/decode; source-only changes publish, unchanged relations reuse, membership-affecting changes rebuild. Preserve live-control full-read behavior. Add one private delivery envelope and atomically apply the pair in current app state; clear on legacy updates. Extend existing Presentation dependencies using relation identity/value, never wrapper address.
- [ ] Verify old API consumer plus new feature-isolated public consumer, focused and complete Noct suite, fmt/scoped strict lint with proven baseline exceptions, and existing unchanged-geometry media/live tests. No reflow is enabled in this task. Record exact RED/GREEN/limits, commit milestones and self-review/report; root independent task gate.

### Task 2: Consume exact relationships for grids, figures and source actions

**Files:** Modify `crates/noctiluca/src/{layout,adapt,app}.rs`, private projection helper if needed; source/model tests, saved-source public consumer and existing Noct guides.

**Interfaces:** Add public `layout::project_source(&SourcePage,u16,(f64,f64),&Marks)->Projection` and private equivalents of existing logs/part paths. Add appended `Rule::FigureFlow`, name `figure-flow`, default enabled for source-aware adaptation. Consume Task1's SourcePage; public Projection and old project APIs remain unchanged. This task completes old source-layout Task3 rather than executing it twice.

- [ ] Add neutral RED for two nested grids with DOM/visual order reversed and1px glyph overhang, boxless wrappers/unequal spans, exact original actions and anchors. Add source-declared figure/caption/prose unit at40/80/120cols with nested/boxless figures, Unicode/long words/distinct links/unrelated neighbors: all intended words once, no stale source-y gaps, caption stays with image. Add offscreen canonical twin (-10000) and mixed runs with visible/clipped counterparts; linked-image guarded control hits before/after decode and explicit overlaid/caption controls.
- [ ] Resolve memberships before transformations and compose child allocation in parent coordinates; preserve immutable source provenance and bypass unsupported irregular snapping. Retain only real source group ownership, never infer cards/controls from geometry. Repair source-layout Task3's original cases through this companion path.
- [ ] Implement measured ordered figure-unit stacking inside proven ordinary flow containers using existing shared split/column/track/adaptive APIs and existing Noct span wrapping. Preserve surrounding units and image reservations. FigureFlow/faithful/source-underlay precedence must be tested. Do not claim true floats or per-line exclusions.
- [ ] Apply canonical ranges' existing Shown aliases within their layer, plus actual media-owner Control to eligible placeholder spans. Preserve explicit hit priority and source-coordinate hotspots. Adopt source-aware paths in the app; keep legacy/manual Page routes functional and compatible.
- [ ] Verify focused/full Noct suite, source dialogs/popup-underlay/partial graphics, zero unrelated media/live relayout, config and public literal consumer. Run unchanged legacy saved-source replay plus separate source-aware output to new directories. Framework-only advertised-feature consumer demonstrates the reused rectangular composition. Fmt/scoped lint, self-review and milestone commits.
- [ ] Under root native lease capture Wikipedia/BBC/MDN/Giphy/current source and40-column neutral cases. Record source/config/hashes and dynamic-site limits; no external consent/chat/login/purchase. Report exact BASE/HEAD, RED/GREEN, source/UI/action proof, limits. Root task/broad review and original fixed application gates remain open.
