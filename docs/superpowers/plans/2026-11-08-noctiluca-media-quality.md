# Noctiluca Media Quality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make image scrolling, gauges and simultaneous video/chat stable and readable, while delivering reusable framework improvements.

**Architecture:** Noctiluca adapts browser semantics through public intuiTUIve widgets. Terminal metrics/capabilities and graphics lifecycle belong in their existing shared layers; page extraction and browser action identity remain in Noctiluca. Improve existing APIs before adding new widgets.

**Tech Stack:** Noctiluca Rust 1.90 minimum; Rust 1.91 for whole-workspace checks because `rich-lumis` declares its own higher floor; existing Cargo workspace; Chrome/Chromium with sandbox enabled; intuiTUIve Driver, recorded snapshots, local HTML fixtures, and rs-rich-record.

**Spec:** `docs/superpowers/specs/2026-11-08-noctiluca-media-quality-design.md`

## Global Constraints

- Generic components must not import browser/page/node types.
- Preserve the faithful `rs-rich` mirror and one-way dependencies.
- Every selected shared improvement is exercised by a framework-only consumer using public APIs and advertised Cargo features.
- Prefer additive APIs/default trait methods; do not add a required field to public `ImageView`.
- Preserve stale-target checks, drafts, conflict handling, disabled/read-only state, password exclusion and action results.
- Do not bypass access checks, sign in or send external chat messages as part of automated validation.
- Keep new API/component work aligned with post-1.0 issues #866/#867; do not change 1.0 release gates or publish implicitly.
- Do not claim a native-terminal pass from emulator tests or emitted escape bytes.

## Review Focus

- Terminal zoom changes pixel geometry without changing columns/rows: encoding caches must not reuse the old geometry.
- An older decoded/encoded frame completes after navigation, resize or view change: it must not resurrect a removed placement.
- Chat updates while the user is scrolled back or editing: preserve anchor, focus and draft.
- A lazy picture changes the source box while scrolling: preserve item association and avoid stale old pixels.
- A compatible protocol name lacks required features or multiplexer forwarding: use a declared fallback rather than emitting incompatible output.

## Work status

- [x] Inspect current source and relevant issues; record uncertainty rather than claim a reproduced root cause.
- [x] Create isolated checkout on `codex/noctiluca-media-quality` from `7fcdbf7`.
- [x] Start official Rust 1.90 setup and the locked baseline Noctiluca build.
- [x] Complete the Noctiluca baseline: locked build plus 108 focused tests on Rust 1.90. Live Chrome is blocked by the installed sandbox helper; whole-workspace checks use Rust 1.91 and are recorded separately.
- [ ] Complete the tasks below; update this checklist and a handoff note after each reviewed milestone.

### Task 1: Reproduction corpus and measured baseline

**Files:** Extend `crates/noctiluca/tests/fixtures/{stream-with-log,gauge-canvas,gauge-svg,product-grid,lazy}.html`; add focused lifecycle fixtures where needed. Extend `tests/{pictures,layout,live_logs,video_app}.rs`; put a combined scenario in `tests/media_quality.rs`. Extend `examples/capture.rs` only if existing instrumentation cannot report the required stages.

**Interfaces:** Consume existing `testing::FakeEngine`, `PageBuilder`, `Tab`, `Driver` and fixture server helpers. Produce deterministic fixtures and independent expected text/state/placement outcomes; a baseline report with exact build/host/protocol configuration. Test instrumentation belongs in test helpers, not new public production counters unless generally useful.

- [ ] Run `cargo build -p noctiluca --locked`; record toolchain and source SHA. Run existing focused suites before edits and distinguish skipped live-browser tests from passes.
- [ ] Add the spec's A/B/C fixture sequences, retaining keyed source identity and independently specified expected content. Include delayed intrinsic-size images, repeated emotes and overlays.
- [ ] Add tests named `media_lifecycle_matches_clean_render`, `chat_scrollback_survives_media_updates`, and `late_media_does_not_resurrect_removed_region`; assertions cover the five Review Focus conditions relevant here.
- [ ] Run `cargo test -p noctiluca --test media_quality -- --nocapture`. Record each genuine failing invariant and classify projection/layout, graphics lifecycle, update cost, or unsupported capability before changing production code. Passing scenarios become baseline coverage, not invented regressions.
- [ ] Record layout calls, encode work, bytes and frame latency for immutable image scrolling, lazy loading, video, chat, and combined work. Use release builds for timing and named configurations; preserve raw logs outside committed docs.
- [ ] Write a concise baseline/decision note selecting the minimal production fixes supported by this evidence, then commit the milestone after required checks.

### Task 2: Shared terminal metrics and capability correctness

**Files:** `crates/rich-interact/src/{session,headless}.rs`; `crates/rich-art/src/{picture,graphics,sixel}.rs`; `crates/rich-ext/src/capabilities.rs`; `crates/rich-intuituive/src/{image,app}.rs`; `crates/rich-ext/tests/capabilities.rs`; appropriate unit tests beside existing encoding tests. Update terminal guidance in `docs/guide/micro/terminals.md` and Noctiluca media guidance.

**Interfaces:** Add an optional default backend method `fn cell_pixels(&self) -> Option<(u16, u16)>` without an art dependency. Add a provided `PlacementSource::placements_with_metrics(&self, frame: &Frame, cell_pixels: Option<(u16, u16)>) -> Vec<Placement>` that defaults to `self.placements(frame)`, preserving existing implementors. Reuse `rich_art::graphics::CellPixels` after crossing into the art layer. Existing picture APIs keep default behavior; introduce additive metric-aware entry points/options rather than changing public `ImageView` or `PictureTag` construction. Metrics must reach prepared full-image, cropped-image and widget caches, including fit/anchor invalidation and changes independent of mouse reporting.

- [ ] Add failing encoding/cache tests with 8×16, 10×20 and 12×24 cells; change metrics at fixed cell dimensions; assert full/cropped output dimensions and invalidation. Add capability tests for Ghostty, WezTerm, explicit overrides and unknown multiplexer transport.
- [ ] Run the relevant tests before production edits and record expected failures.
- [ ] Thread optional measured cell pixels from session/backend into graphics fitting/encoding and its complete cache identity. Preserve default APIs and non-graphics/headless behavior.
- [ ] Correct Ghostty selection and incompatible forced-mode guidance. Reuse existing diagnostics; do not implement a speculative universal terminal probing system. If transport/features remain unproven, keep the fallback explicit.
- [ ] Run `cargo test -p rs-rich-art --features picture`, `cargo test -p rs-rich-ext --test capabilities`, `cargo test -p rs-rich-interact --lib`, and `cargo test -p rs-rich-intuituive --features art --lib`. Verify downstream source compatibility with existing public struct literals and existing `PlacementSource`/`Backend` implementors.
- [ ] Add a framework-only example showing metric change, image clipping and fallback using public APIs; commit after checks.

### Task 3: Image lifecycle and scrolling correctness

**Files:** `crates/rich-ext/src/frame/graphics.rs`; `crates/rich-intuituive/src/{image,app}.rs`; `crates/noctiluca/src/{pictures,video,pixel_view,app}.rs`; tests from Task 1. Change only layers implicated by reproduced failures.

**Interfaces:** Retain `Picture` id/version, `PlacementSource`, `GraphicsPlan` and frame assembly. Any added geometry/document generation token must invalidate obsolete asynchronous output without leaking browser types into shared components. Keep old frame/placement behavior until tests justify a different movement strategy.

- [ ] Reduce each Task 1 lifecycle failure to a small shared planner/image test or a Noctiluca rectangle/binding test. Cover scroll down/up, partial clipping, modal open/close, resize, navigation and view switch.
- [ ] Watch the reduced tests fail. Implement the smallest correction at the owning layer; do not treat missing paint sorting alone as proof of overlap.
- [ ] Compare each transitioned scene with a clean render at its final state and assert old placement cleanup and current hit targets.
- [ ] Measure moved-picture work across protocols. Permit a scroll optimization only when the planner accounts for moved placements, overlays, clipping and subsequent cleanup; otherwise optimise encoding/damage without unsafe terminal scrolling.
- [ ] Run the focused shared tests plus Noctiluca media/image suites; retain manual native pixel verification as an outstanding gate until actually run. Commit the reviewed fix.

### Task 4: Separate media/log content updates from unrelated layout

**Files:** `crates/noctiluca/src/{app,tab,pictures,pixel_view}.rs`; add `src/presentation.rs` only if a small internal projection/update unit makes responsibilities clearer; `tests/{media_quality,live_logs,video_app}.rs`.

**Interfaces:** Keep `Page` authoritative. Define a private presentation/layout dependency key for geometry/text/control meaning, separate from playback time, decoded pixels and log content that does not change outer geometry. No new public browser model API is required. Existing `Tab::Update` semantics and guarded actions remain compatible.

- [ ] Add failing tests: playback-only update yields zero unrelated layout work; unchanged-rectangle decode yields zero unrelated layout work; changed rectangle produces correct reflow; chat append in a fixed-height region preserves unrelated rows and deliberate scrollback.
- [ ] Add a slow inline-resource fixture proving useful text can be published before optional emotes finish, and stale completion cannot affect a replaced document.
- [ ] Implement dependency separation/coalescing and asynchronous optional-resource work without losing updates, controls or drafts. Maintain bounded newest-frame work; do not coalesce semantic actions as if they were video frames.
- [ ] In pixel view, test that a pacing-rejected frame does not perform avoidable full conversion, while accepted frame accounting remains correct.
- [ ] Run Noctiluca model/app/media/live-log suites, then repeat the exact baseline configurations. Require deterministic work reductions and correct final state; report latency distributions separately from CI correctness gates. Commit after checks.

### Task 5: Adaptive layout and public component adoption

**Files:** existing `rich-intuituive` layout, `split`, `scroll_virtual`, transport and control modules only where the public audit proves a gap; framework examples/docs; Noctiluca `layout.rs`, `widgets.rs`, `app.rs` adapters and A/B/C tests.

**Interfaces:** Reuse stock grid/wrap/split/anchored-scroll/transport APIs. A new generic API must be specified with concrete consumers before code is written; do not create an Amazon card, browser log type or second player state owner. This task consumes the geometry/update guarantees from Tasks 2–4.

- [ ] Record a small existing-API/real-gap matrix and select the minimum shared enhancements required by A/B/C.
- [ ] Add failing layout/interaction tests at 80×24, 120×40, 160×50 and a 40-column split: grouping, long/Unicode labels, independent scrolling, keyboard access, focus return, disabled states and themes.
- [ ] Extend selected public components with good defaults and measured sizing. Integrate them in Noctiluca with semantic grouping and existing adaptation toggles; suppress redundant presentation text only with a route to its original meaning.
- [ ] Build a consumer outside workspace feature unification and demonstrate the selected capabilities without Noctiluca. Add a concise recorded example and public customization guidance.
- [ ] Verify browser actions, drafts, identity and hit targets still reach the same page controls after reflow. Commit after checks.

### Task 6: Cross-terminal acceptance and completion

**Files:** concise results in `docs/design/noctiluca-media-quality-status.md`; updated relevant guides/gallery; native recordings as normal project artifacts with provenance. Avoid committing bulk timing logs.

**Interfaces:** Consume A/B/C and the fixed scripted interaction sequence. Each matrix row records terminal/OS/version, transport, protocol, metrics, source SHA and pass/fail/unverified/unsupported; results must not infer one terminal's behavior from another.

- [ ] Run direct/SSH/tmux coverage for Kitty, Ghostty, WezTerm and iTerm2 on hosts where available; include Sixel and half blocks. Exercise projection and supported pixel view, font/metric changes, reverse scroll and repeated overlays while video/chat update.
- [ ] Check reachable real speedtest.net, Amazon and video/chat pages; record walls or unavailable hosts as unverified. Send chat only to the local fixture, never a live service.
- [ ] Record p50/p95/p99 presentation latency, displayed frame gaps, bytes, encoding/layout work and bounded resource behavior against the same baseline. Review numerical acceptance thresholds set in Task 1.
- [ ] Run `cargo +1.91.0 fmt --all -- --check`, `cargo +1.91.0 clippy --all-targets -- -D warnings`, and `cargo +1.91.0 test --locked`; run the relevant graphics/media feature checks and external consumer. List failures/skips by name, and retain a separate Rust 1.90 Noctiluca check.
- [ ] Perform a fresh whole-change review, resolve material findings, update API/release notes and handoff state. Report only actually verified combinations; do not publish or merge implicitly.
