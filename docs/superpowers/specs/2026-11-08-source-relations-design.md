# Source relationships and readable flow

This continues the [approved media-quality design](2026-11-08-noctiluca-media-quality-design.md) and completes the source-grid/nested work in [source layout integrity](2026-11-08-source-layout-integrity-design.md). Matched Wikipedia evidence now also justifies a conservative figure/caption flow improvement. The user permits rearrangement and requires generic reusable UI capabilities; browser interpretation stays in Noctiluca, using existing public intuiTUIve layout/graphics components.

## Problem and selected approach

The flat Page model preserves individual boxes but discards actual source ownership. Rectangles cannot faithfully establish nested grid membership, figure/caption ownership, linked-image targets or the placement of a hidden canonical twin. Repurposing Block.frame, region, Grid, names or node IDs would corrupt their contracts. Required new fields would break external complete literals.

Add an optional opaque `SourcePage` pairing the exact `Arc<Page>` with immutable private `SourceRelations`. Keep old build/project/Tab callback APIs. The source-aware app carries the pair atomically and reuses relations during image/playback/live-only updates. There is no global source registry, unrelated Page/context pairing API, new renderer, CSS engine or browser type in shared core.

New UI primitives are unnecessary for this scope: public column/split/track/adaptive-column, text wrapping, scroll, layers and image graphics already supply rectangular composition. The separately specified stock-button wrapping remains the selected new reusable control capability.

## Interfaces and lifetime

Expose `model::SourcePage` with private fields, Clone/Debug and `page(&self) -> &Arc<Page>`. Add `build_source(&PageSnapshot, &mut Revisions) -> SourcePage`, and `build_source_with(&PageSnapshot, &mut Revisions, &HashSet<NodeId>) -> SourcePage`. Existing build/build_with return Page with unchanged contracts and no required-field migration.

Add `Tab::spawn_with_source(engine: Box<dyn Engine>, update: impl Fn(Update, Option<SourcePage>) + Send + 'static) -> Tab`. Some accompanies exactly Update::Page and points to the same Page Arc; all other updates have None. Old spawn discards the companion through the common implementation. Do not add a public Update variant.

Retain actual source ancestor/order edges, grid item roots through boxless wrappers, exact generated block/run memberships, figure/caption subtrees, actual Control ownership of media descendants, original rectangles/nesting, and existing Spoken→Shown matches. A source node can generate several pieces; node-only or geometry-only lookup is insufficient. Finalize after model flushing/stack classification. Relations hold no Page, preventing cycles, and contain no copied image/frame arrays.

Every publication path—snapshot, live log, playback, still and decoder completion—must attach the correct current relations. Source-only relationship changes publish even when Page compares equal. Unchanged relations reuse their allocation or exact structural value; SourcePage wrapper identity is not a layout key. Navigation, current-document barriers and applied-viewport/source capture guards remain unchanged. Old snapshots and manually constructed Pages retain the source-less fallback.

## Source-aware projection

Add `layout::project_source(&SourcePage, columns: u16, cell: (f64,f64), marks: &Marks) -> Projection`, plus private log/part equivalents required by the app. Public Projection remains unchanged. Delivery queues the pair in one private envelope, never in separately updated latest slots. Existing Presentation dependency checks include relations identity/value without invalidating unchanged-geometry media updates.

- Resolve grid membership against immutable source item roots before transformation. Carry parent allocation into nested children using immutable local offsets. Preserve original source actions, image association, reading order and anchors. Unequal row spans and boxless primary header/body plus complementary sidebar must not become arbitrary equal cards; bypass an unsupported snap rather than destroy source relationships.
- A genuine source FIGURE and its caption subtree form one ordered unit. Within a proven ordinary flow container, stack source units and prose by measured terminal height at the final width. Preserve each intended word once, captions with their figure, distinct source actions, legitimate image reservations and source surroundings. Do not retain stale source-y gaps inside the reflowed group. Nested figures remain nested without duplicate children.
- Add `Rule::FigureFlow` / `figure-flow` for this rectangular stacking, enabled for source-aware adaptive projection. Faithful/no-adapt and source-preserving popup underlay disable it. Do not overload SnapGrids/CollapseGaps or claim per-line float/exclusion support. Document the public Rule variant for exhaustive-match consumers.
- Use the existing canonical twin match for placement: selected Spoken ranges follow their known Shown counterpart while keeping canonical text/style/control provenance. Mixed paragraphs must not move wholesale; wholly clipped shown pairs remain absent. This repairs the proven offscreen canonical placement case, not a new matching heuristic.
- Bind eligible linked-image placeholder spans to the actual retained source Control. Preserve distinct caption/foreground controls and guarded activation; decode must retain hit identities. Video/canvas hotspots and ambiguous ownership keep the existing source-coordinate route. No control ownership inferred from overlapping rectangles.

Only ordinary flow participates in grid/figure rearrangement. Dialogs, pinned/docked/positioned foreground and popup-open source-positioned underlay retain their placement paths. Alias selection stays within its existing layer. Graphics still use shared composed-cell clipping and exact native provenance.

## Acceptance

Prove complete old public literals/constructors still compile, legacy outputs/contracts remain compatible, and source-aware pairs cannot mix snapshots. Test all publication paths, same-document relationship-only changes, navigation barriers and zero unrelated layout for image/playback/live-only changes.

At40/80/120columns test nested/reversed-source-order grids with glyph overhang, boxless wrappers and unequal spans; genuine nested figures/captions with Unicode/long words/links and unrelated neighbors; full text once with no obsolete y gaps; offscreen canonical twins including mixed runs and wholly clipped pairs; guarded image targets before/after decode; resize/scroll anchors and source dialogs/foreground clipping. Keep unchanged legacy saved-source replay and a separate source-aware consumer. Demonstrate rectangular composition in a feature-isolated framework-only consumer.

Source evidence: `/workspace/work/noctiluca-figure-flow-audit.md`, `/workspace/work/noctiluca-source-relations-design-proposal.md`, saved source/projection captures, SourceTask2 offscreen-twin baseline proof. These proposals are not passing results. Native screenshots, original fixed application gates and broad continuation review remain required; no merge/push/publish.
