# Browser Image Source Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recover failed still decodes and upgrade temporary screenshots to later-available animated originals.

**Architecture:** Preserve shared Picture decoding and ImagePresentation; add explicit completion and bounded private browser-source attempt state in the existing tab worker. Keep browser capture/retry ownership in the adapter, with no new widgets or rendering path.

**Tech Stack:** Rust, existing Engine fake, worker mpsc decoder, public rich-art Picture; injected Instant values for deterministic retry tests.

**Spec:** `docs/superpowers/specs/2026-11-08-image-source-recovery-design.md`.

## Global Constraints

- Every reusable widget/render improvement belongs in public shared crates; browser resource/readiness/document/capture policy stays in Noctiluca's adapter using existing shared Picture/ImagePresentation APIs.
- No site selectors, forced lazy fetch, request rewrites, consent/sign-in/chat/purchase or access-check bypass.
- No new required public Page/Media/Frame/Job fields; private attempt state may be additive internally. Preserve existing inline-resource behavior and faithful crates/rich.
- One pending still decode per current document/key; both success and failure complete admission; stale/superseded results cannot publish.
- Retry transient network-original reads after 500ms, 2s, 8s, then 30s capped, within existing STILLS_PER_READ=8 batches. Retain successful picture during replacement; no immediate failure loop.
- State is pruned to live media keys and cleared on navigation. No new thread per retry or unbounded URL registry.
- One source/Cargo implementer at a time; no worker subagents, external integration or acceptance timing without root coordination.

## Review Focus

- A valid format/dimension header with an incomplete body must finish as failure and recover through screenshot (Task 1 completion test).
- A same-key retry completing after a newer attempt or document must not restore old pixels (Task 1 ownership tests).
- A failing screenshot decoder must back off and leave the last good picture visible (Task 1 failure/backoff tests).
- Repeated passive page reads must not reset backoff and hammer an unsupported or unavailable resource (Task 1 clock/read tests).
- Many failures and offscreen/not-ready images must not starve later ready media or grow state after removal (Task 1 admission/pruning tests).

---

### Task 1: Explicit still completion and bounded source recovery

**Files:**
- Modify/test: `crates/noctiluca/src/tab.rs` (private decoder result, Worker capture/take/read/tick state and tests).
- Modify if needed for neutral source sequences: `crates/noctiluca/src/testing.rs` (test Engine only, no production behavior).
- Test fixture if needed: `crates/noctiluca/tests/fixtures/image-source-recovery.html` with local generated media; no network dependency.
- Documentation: `crates/noctiluca/README.md` image fallback/retry behavior.

**Interfaces:**
- Consumes unchanged public `Engine::resource`, `Engine::image_ready`, `Engine::capture_model`, `Picture::decode_animated`, Media/StillKey identities and existing shared presentation.
- Produces unchanged public interfaces and private explicit still-completion state. `Worker::capture_stills()` remains the production entry; it may delegate to private `capture_stills_at(now: Instant)` for deterministic tests. Decoder completions must include owning document/key/attempt and success-or-failure; choose private type names to fit surrounding code, without changing the public generic codec.

- [ ] **Step 1: Reproduce both failures.** Add a corrupt PNG header/body fixture that `decodable()` accepts but `Picture::decode_animated` rejects; verify actual still decoder yields a completion and a scripted worker obtains/decode-publishes valid screenshot. Add a scripted original-resource first failure, valid screenshot, then same-key two-frame animated original; advancing injected time to just before/at 500ms must assert no request/one request and eventual animation frame count. Record intentional RED on current behavior, not setup/compiler failures.
- [ ] **Step 2: Implement explicit completion.** Preserve asynchronous decode; send every still result with doc/key/attempt. Track pending attempts privately and consume failures. On original decode failure schedule guarded screenshot fallback; on failure retain last good picture. Do not change legacy inline-channel success-only contract unless adapting all its callers explicitly and preserving existing tests.
- [ ] **Step 3: Implement source-aware retries.** Distinguish transient unavailable network original from unsupported/no-original/data-url sources and screenshot results. Schedule the exact backoff sequence, admit no more than existing batch size and one pending attempt per key, integrate due work in worker tick behind current-model/dirty guards. Never run original decode repeatedly on an unsupported format; never re-fetch successful originals unnecessarily.
- [ ] **Step 4: Add ownership/fairness regressions.** Drive injected time through all four backoff intervals; verify repeated reads retain due time; image_ready=false makes no fetch/capture; failed fallback backs off; unsupported format uses stable fallback without original polling. Complete old/new same-key attempts in reverse order, navigate, change source/size/revision and remove media; assert only current picture publishes, state prunes, previous picture survives same-key replacement, and already-accepted SVG continuity across revision-only changes preserves the existing no-gap test without accepting old completions. Document/node/kind/source/size changes retire that fallback. Unchanged geometry/actions/layout key remain equal. More than 8 failing/ready sources must eventually admit later ready entries without a busy loop. Use messages/barriers and clock injection, no sleeps.
- [ ] **Step 5: Verify and document.** `cargo +1.91.0 test -p noctiluca --lib tab::tests`, then `cargo +1.91.0 test -p noctiluca` (Chrome tests using real sandboxed binary must be explicitly distinguished from self-skipped). Run fmt and strict scoped clippy with exact known baseline lint evidence if blocked. Document bounded fallback/retry behavior and limitations.
- [ ] **Step 6: Native proof and commit.** Coordinate root native-capture lease after Cargo completion: actual Noctiluca plus local neutral fixture, capture fallback and animation progression in an available native Kitty/WezTerm, record source/image versions and geometry. Do not claim Giphy attribution from fixture. Commit source/tests/docs at milestones; write task-1-report.md with RED/GREEN, commands, source hashes, screenshots, state bounds, self-review and concerns. Root owns task review.
