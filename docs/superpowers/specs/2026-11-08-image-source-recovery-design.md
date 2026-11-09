# Browser image source recovery

Continuation of the [approved media-quality design](2026-11-08-noctiluca-media-quality-design.md). The user has authorised continued implementation; this scope closes concrete browser-adapter lifecycle gaps found while reviewing Giphy-style media. It does not claim that those gaps explain any particular public-site tile until matched evidence demonstrates it.

## Problem and resulting behaviour

The tab caches recognised-format bytes before asynchronous decoding. The decoder reports only success, so a recognised PNG header with a corrupt body can permanently leave `Some(still)` and no picture. A transient original-resource error can instead cache Chrome's PNG fallback forever for an unchanged source key, losing a later-available animated original.

Each still decode attempt must produce an explicit completion. Failure must reach the owning tab, which can obtain a guarded browser screenshot rather than leave an unfinishable decode. A successful screenshot remains visible while a temporarily unavailable original is retried. An eventual animated original replaces it without changing reserved geometry, source actions, focus or unrelated layout.

## Ownership and generic boundaries

The public shared `Picture::decode_animated` already reports `Result` and supports animated WebP. This work must preserve that reusable codec contract. Browser resource readiness, guarded screenshot capture, document ownership and tab cache policy belong to Noctiluca's adapter; do not move browser types into generic widgets or invent a second renderer/decoder. Use the existing public Picture/ImagePresentation path for presentation. Any newly discovered reusable control or presentation requirement needs its own shared primitive, not a private Noctiluca widget.

## Private lifecycle contract

- One pending still decode per current `(document, StillKey)`; completions also identify their attempt so a same-key retry cannot accept an older attempt.
- Success and failure both release pending admission. Stale document, removed node, changed source/size/revision and superseded-attempt completions cannot modify the current page.
- A supported-format original that fails actual decoding takes the guarded PNG screenshot route once for that attempt; header sniffing is not successful decoding. Do not decode synchronously on the UI/tab thread.
- Track original vs screenshot provenance privately. Retry a network original after a transient resource-read failure with delays 500ms, 2s, 8s, then 30s capped. No repeated immediate retry; one attempt per due live key, admitted within existing `STILLS_PER_READ = 8` batches. Data URLs, drawings with no original URL and formats the build does not support do not poll for an original.
- A failed fallback capture or decode also waits on bounded backoff; no tight screenshot/decode loop. Keep the last successfully decoded picture while replacement is pending or fails. An already accepted inline SVG drawing may remain presented across a content-revision-only transition in the same document/node/kind/source/rounded size, with its old provenance retained; this preserves live-gauge continuity. It is not acceptance of an obsolete completion or success for the new key. Do not transfer that fallback across document, node, kind, source or size changes; keep at most one per live SVG node.
- Retry scheduling runs from the existing worker tick without a new DOM snapshot, resource request rewrite or forced lazy fetch. Honor `image_ready`, current source-model capture guard and existing dirty/navigation checks.
- Prune pending/retry/provenance state to live media keys on page reads and clear it on document change. Retained state is bounded by current media and pending completions; no ever-growing failed-URL registry.
- Existing inline-resource admission and legacy inline completion semantics remain compatible. No new required public Page/Media/Frame/Job fields and no browser dependency in shared core.

## Acceptance

Use a neutral fake engine with explicit byte/failure sequences and injected monotonic time; no wall-clock sleeps. A corrupt PNG body that passes format detection must publish a failure completion, then a valid guarded screenshot picture. A transient original miss must show a screenshot and later upgrade the same key to a known two-frame animation, with exact attempt counts before/at each due time. Cover fallback failure, unsupported format, not-ready source, navigation, source/size/revision changes and reversed completion order. Assert geometry, actions and unrelated layout keys remain stable across image-only publication. Existing document/revision fallback and inline admission tests must pass.

Capture a native-terminal neutral fixture showing placeholder/fallback recovery and two visible animation states; then revisit matched reachable Giphy images. A lazy source placeholder or inaccessible resource remains explicitly unverified, rather than being counted as a codec failure. Public-site access checks and consent are not bypassed.
