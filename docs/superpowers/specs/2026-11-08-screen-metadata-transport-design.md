# Screen metadata transport continuation

Status: diagnostic-gated continuation of the approved Noctiluca media-quality design. Start: `44c78a3e1a255663aef315b4238fe270a7e4b752` (production `cbc7d93`).

The user has asked to continue the remaining work. The [parent design](2026-11-08-noctiluca-media-quality-design.md) remains binding. This phase targets redundant temporary Frame construction for ordinary metadata in the public shared Screen APIs. It does not repeat the completed media-quality implementation or replace its original performance baseline.

## Contract

- `Screen::write_lines` and `Screen::write_runs` preserve exactly the existing Frame-backed cells: text, widths, continuation cells, full ordinary metadata, links and visual styles. Writing a shorter row clears the remainder and clipping clears split wide graphemes as before.
- Ordinary metadata is eligible for direct insertion. Recognised semantic region tags still take the existing Frame route that strips their region transport tag. Unknown or malformed metadata follows the existing Frame result, without interpreting image-specific keys in Screen.
- Embedded line breaks continue to use Frame. Control Segments remain suppressed; controls embedded in ordinary text become visible symbols. Zero-width and combining text retain the existing cross-segment rules.
- The change is internal to `rich-ext`; no new required fields, public API/default changes, feature or dependency changes, or edits to faithful `crates/rich`.
- Every application using these public Screen methods benefits without a browser-specific path. Noctiluca uses the existing shared API automatically.
- Keep the change only after a bounded current **bound** image diagnostic establishes material temporary-Frame transport cost and a frozen component comparison demonstrates lower relevant work with identical cells. Instrumented results are diagnostics, not release acceptance.
- Preserve all historical release samples, quality, offers, identities and gates. The iTerm-compatible workload remains release-blocked until the original application gates pass; component or allocation improvement alone does not clear that blocker.

## Evidence and boundaries

Read-only diagnosis: `/workspace/work/noctiluca-continuation-perf-diagnosis.md`. Current Screen rejects every style with metadata, so tagged image rows deep-copy styles into temporary Frames before interning into Screen. Only recognised semantic region tags require special Frame interpretation. Diagnostic ownership is recorded separately in `/workspace/work/noctiluca-continuation-perf-probe/`.

The separate Screen-to-Frame graphics discovery path, image decode failure/retry state, narrow labels and page-group projection remain follow-up scopes. Do not introduce a typed attachment plane, browser group model or new generic widget as part of this change.

## Acceptance

1. Direct ordinary-metadata writes are proven against the explicit old Frame path, including mixed metadata and Unicode/controls/clipping, for both public entry points.
2. Recognised region tags and embedded newlines demonstrably retain their old fallback behavior. Malformed tags preserve the old cells.
3. Existing rich-ext surface/frame tests and shared image clipping/compositing tests pass. Strict modified-crate linting reports any baseline exception explicitly.
4. A public-API-only consumer with isolated advertised features checks metadata survives direct writes and ordinary rendering; no workspace feature unification.
5. One declared frozen bound component comparison retains raw results and reports output equality and allocation/time changes. No claim of native-terminal or application latency improvement from this component result alone.

New application acceptance runs happen only after the continuation's relevant implementation is frozen, in a declared quiet window, against the original baseline and exact original gates. No publish, merge or push is part of this authorization.
