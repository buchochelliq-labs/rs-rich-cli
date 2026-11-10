# Source layout integrity

Continuation of the [approved media-quality design](2026-11-08-noctiluca-media-quality-design.md). Matched sandboxed source captures and the external public model/projection consumer establish four generic defects: boxless links drop visible descendants; fully clipped static descendants reach the terminal edge; boxless wrappers erase declared grid items; and grid membership splits text whose glyph bounds overhang its enclosing frame before nested transforms mix coordinate spaces.

The concrete source/model evidence, capture configuration and neutral assertions were recorded in a worker brief and source captures that were not kept in the repository. Source visits are BBC, Giphy, MDN and Wikipedia at960x630CSS. Backend IDs are diagnostic correlations only, never implementation selectors. Source PNGs and native terminal runs are separate observations.

## Contract

- An empty selected-image URL must not resolve to the page URL. Fall back to a nonempty declared image source while loading, or keep it absent/pending; never manufacture the document as an image resource. Actual later selected-source changes remain authoritative. The matched live observer confirms this separate early-loading defect; it also confirms later 1x1 GIF changes were actual browser DOM state, not a reason to override the page.

- A source-declared link with no own layout box retains its actually laid-out visible descendants and original source action identity. Derive a visible footprint from those descendants when needed; do not invent a new target or discard its destination/revision. A hidden subtree with no visible layout remains absent.
- Source grid items traverse boxless wrappers to first actual layout boxes, in source order. Stop at normally boxed children; do not flatten all descendants or infer a card from proximity. Keep established public Grid/Page/Block shapes.
- Fully outside ordinary descendants of a proven overflow clip produce no visible text, hints or graphics. Use the clipped axis only, current source coordinates and conservative outer bounds; do not double-apply scroll offsets or mistake scrollWidth for client width. Preserve partial intersections. Do not apply page-viewport clipping as a blanket content filter, since terminal adaptation may legitimately rearrange content outside the source viewport.
- For uncertain absolute/fixed positioning or insufficient containing-block facts, decline new clipping rejection. Existing source popup extraction and foreground retention must continue to work. The current captured styles omit transforms/containing-block identity: no general CSS clipping/compositor claim is permitted.
- Determine source-grid membership from immutable source geometry and its enclosing frame before any transform. Text glyph bounds may overhang that frame by1px or more; membership must not depend on increasing an arbitrary tolerance. Apply nested transforms in a consistent coordinate space and preserve parent movement/child association across stacking and resize.
- Preserve source meaning, control node/revision/destination/hints, image association, readable ordering and anchored item/relative line. Independent neighboring boxes must remain independent. Adaptation-off remains a reference; do not hide the defects by changing global adaptation defaults or removing image reservations.
- These are browser-source extraction/projection repairs. Existing shared clipping, adaptive columns, wrapping, scrolling, layers and modal components already provide the generic rendering facilities; no private replacement widget, new required public field or browser type in shared core is needed. Faithful rich and optional-art boundaries remain intact.

## Acceptance and limits

Use neutral PageBuilder/raw snapshot fixtures, including no-box links, nested boxless grid wrappers, static x/y clip strips, partial intersections, scrolled ancestors, escaped positioned popup and two nested grids with DOM order differing from visual x order. Require full binding/metadata and item association, not merely smaller page height. Test40/80/120columns and existing source-popup/product/media regressions.

Rerun the unchanged public capture consumer against saved DOM/AX after each owning change and preserve new outputs separately. Then capture actual Noctiluca BBC/Giphy/MDN at source-attributed builds, compare ordinary content without bypassing access checks and show screenshots. Public content may change between runs; distinguish semantic correspondence from a frozen-DOM comparison.

Coral's broader float/text-fragment reflow remains a separately identified gap until a safe generic constraint or public primitive is selected; these fixes do not claim to solve it. Runtime image decode/retry and narrow-label disclosure are separate scopes. No application performance acceptance, push, merge or publish belongs to these source-layout tasks.
