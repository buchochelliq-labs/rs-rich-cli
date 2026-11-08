# Noctiluca media, scrolling and adaptive presentation

Date: 2026-11-08. Source baseline: `7fcdbf7263f81d89c2975a47ee4761b4a0e1c7cc`.
Status: specification for implementation; no artifact reported below is claimed fixed.

## Intent and decisions

Make Noctiluca comfortable to read and operate while images, video and live chat are present. The user reports artifacts on Ookla speedtest and Amazon, judder when scrolling images, and generally difficult reading, probably on Noctiluca 0.0.4. They are unsure whether projection or pixel view was active, so both paths are in scope. Video/chat is the first priority; gauges and product grids are required regression scenarios.

The user explicitly allows rearranging pages for usability. Every reusable widget, layout primitive or interaction improvement must become part of the public framework widget collection, usable by other applications. Noctiluca must adopt those public components rather than owning a parallel widget suite.

“Core widgets” here means intuiTUIve's reusable components and their existing optional integrations, not the faithful Python-rich mirror in `crates/rich`. Generic components must not import browser/page/node types. Optional art/media integrations retain their existing feature boundaries.

The user has authorized finalising the plan and starting a build. This specification records the agreed direction; exact internal implementation choices listed below remain engineering decisions.

## Success and scope

1. Video and chat remain usable together: playback controls respond, new messages appear, intentional scrollback holds, drafts and focus survive passive updates.
2. Images stay inside their visible regions. Scrolling, resize, overlays, navigation and view changes leave no stale graphics or duplicated pictures.
3. Adaptive projection keeps related text, images and controls together at narrow and normal sizes. It preserves source meaning, reading order and authoritative browser actions.
4. Reusable improvements ship with public APIs, defaults, documentation, a framework-only example and tests. A Noctiluca-only fix is insufficient when the capability is generally useful.
5. Improvement claims come from measurements of actual presentation and native-terminal evidence, not only model snapshots, offered frame rates or emitted escape bytes.

The first implementation milestone covers reproducible scenarios, terminal capability/cell geometry correctness, media lifecycle regressions, and unnecessary layout work during media updates. Adaptive grouping and shared control adoption follow on the same foundations. Cross-terminal validation remains required before declaring the overall work complete.

## Evidence and uncertainty

The current implementation already has virtual scrolling, layout caching, image clipping, overlay cleanup, off-thread frame encoding, live log reads, canvas/SVG capture, pinned/docked bars, and stock dial/rating/carousel components. These are foundations to extend, not missing features to recreate.

Source-supported investigation targets:

- Playback, decoded-picture and live-log updates publish a `Page` that invalidates the whole layout memo (`noctiluca/src/app.rs`, `tab.rs`). This does not mean every video frame rebuilds the page.
- Image placements inhibit terminal scroll optimization; moved Sixel/iTerm images repaint their old cells and redraw. Cropped output may be re-encoded as visible rows change (`rich-intuituive/src/image.rs`, `app.rs`; `rich-ext/src/frame/graphics.rs`).
- Picture fitting/cropping uses an 8×16 cell assumption although actual terminal cell dimensions vary (`rich-art/src/picture.rs`).
- Automatic graphics detection omits Ghostty; a protocol name does not establish support for Unicode placeholders, animation or multiplexer forwarding (`rich-ext/src/capabilities.rs`).
- Pixel scrolling changes most tiles. Some frame conversion occurs before pacing rejects a frame (`noctiluca/src/pixel_view.rs`).
- The prior real-sites pass could not access live speedtest.net or Amazon. Fixed-size reduced fixtures did not establish their real-site rendering quality (`docs/design/noctiluca-real-sites-2.md`).

None of these observations establishes the root cause of the user's particular artifact. Each fix must start from a failing invariant or measured cost. Fully contained overlays and general partial browser compositing are different cases.

## Approaches

**Selected direction: adaptive Noctiluca over shared framework components, delivered in stages.** It addresses readability and makes fixes available to other apps. First stabilise media and update handling; then integrate stronger reflow/grouping and interaction polish.

**Considered: preserve page geometry and restrict changes to rendering bugs.** Smaller behavioural scope, but does not meet the user's request for usable rearrangement where browser geometry is unsuitable for terminal cells.

**Considered: prioritise a whole-page pixel presentation.** Useful as an inspection/comparison view, but costs substantially more while scrolling and does not provide the native text/control experience that defines Noctiluca. Pixel view remains supported; it is not the new default.

## Architecture and ownership

| Layer | Responsibilities |
|---|---|
| `rich-interact` | Terminal/session measurements, backend capability facts, input and synchronized frame boundaries; no dependency on browser or art types. |
| `rich-art` | Cell-aware image fitting/cropping/encoding, protocol-specific output and identity/version caches. |
| `rich-ext` | Capability selection and reusable graphics placement/damage planning. |
| `rich-intuituive` | Reusable image widgets, layout/scroll/transport components, clipping, focus, themes, accessibility and retained updates. |
| `noctiluca` | HTML/ARIA interpretation, page grouping, browser identities/revisions, media source rectangles, guarded browser actions and adapters. |

Start from existing `picture_of`, `tiles_of`, `image_graphics`, `grid`, `Node::wrap`, `split`, `follow_end_with`, `scroll_virtual_with`, and the generic `Transport` primitives. `Transport` is a local clock aid, not an authoritative HTMLMediaElement state owner: buffering, failed/denied commands and externally changed playback must reconcile from the browser. Audit the existing contract before adding an API. Extend existing examples/gallery rather than introducing a competing rendering loop.

## Required behaviour

### M1. Media geometry and lifecycle

- Measure terminal cell pixels where available; use the documented 8×16 fallback only when unknown. Physical cell pixels and Chrome CSS pixels are separate coordinate systems.
- Carry metrics through image fitting, partial-row cropping, protocol encoding and cache keys. A changed metric invalidates affected encodings even when cell columns/rows are unchanged. Cache identity must also retain all relevant fit, anchor and presentation parameters; do not fix only the last encoder cache while leaving an earlier widget cache stale.
- Measure raster cell dimensions independently of whether pixel mouse reporting is enabled. Refresh at startup and resize, and support deterministic changes of metrics without changing rows/columns in the headless/Driver path. Do not replace Noctiluca's Chrome CSS calibration with physical terminal cell dimensions without an explicit coordinate conversion.
- Retain picture identity while frames change; replace the shown frame rather than accumulating placements. Reject asynchronous work from an invalidated document, region or geometry generation.
- Clip images to their scroll viewport and composed visible area. Covered graphics must not paint over dialogs, pinned bars or unrelated content. Restore the correct underlying scene after dismissal.
- On removal, navigation, resize, view switch and quit, remove/repaint obsolete placements according to the selected protocol. A fallback must not leave both old and new representations visible.
- Preserve existing synchronized-output detection/overrides. Frame assembly and terminal presentation must not introduce separate uncontrolled text/image writes.
- Do not enable terminal scrolling through graphics until the placement planner can account for that movement and regression evidence proves it correct.

### M2. Capabilities and fallbacks

- Distinguish terminal identity from supported graphics features and transport. Explicit configuration remains available; it must not silently claim unavailable features.
- Recognise Ghostty. Gate Kitty placeholder/animation use by evidence of the required features; do not assume that every Kitty-protocol implementation supports the same subset.
- Keep WezTerm's working iTerm-compatible route unless its required Kitty features are established. Correct documentation that recommends incompatible forced modes.
- Treat direct, SSH, and tmux/screen as separate transport conditions. Document supported passthrough/native paths, limits and fallbacks. A terminal behind an unknown multiplexer must not be advertised as verified.
- Unknown capabilities fall back to a supported representation with a concise explanation through existing diagnostics. Do not scatter host-name checks through widgets.

### P1. Video, chat and image update cost

- A playback-only change or decoded image with unchanged geometry must not recompute unrelated page layout.
- A fixed-height log's new entry must update its content/tail without rebuilding unrelated page regions. Geometry changes may require downstream reflow.
- Publish useful text without waiting for optional inline-emote/image resources. Preserve text alternatives and stable reserved geometry while resources arrive.
- Keep replaceable frame work bounded: one newest waiting frame/encode per region, plus work already executing. Coalesce superseded media state without dropping semantic actions or required log changes.
- Keep actual browser state authoritative. Preserve stale-target checks, drafts, conflict handling, disabled/read-only state, password exclusion and action results.
- Apply pacing before avoidable expensive conversion where feasible. Pixel-view budgets and changed-tile output remain separate from projected video-region optimisation.
- Instrument input-to-presented-frame latency, model reads, layout calls, graphics planning, encoding, bytes, queue depth and retained image bytes. Historical timings are not the baseline for new claims.

### L1. Adaptive presentation and shared widgets

- Use available container width/height, including narrow splits, to choose side-by-side or stacked presentation. Keep video/chat independently scrollable where their source semantics support it.
- Preserve visual association of image, title, price/value, rating and relevant action. Avoid adding a repeated image caption when equivalent adjacent text already provides the same context; retain a route to the original alternative text.
- Reserve image space before decode where source geometry exists. If a source box changes, preserve the reader's anchored item and relative line when still valid.
- Keep live logs at their tail only while following; deliberate scrollback, selection and draft editing must not be stolen by incoming updates.
- Shared controls support keyboard/mouse, visible focus, long labels, Unicode widths, themes, disabled states, overlay dismissal/focus restoration and reduced motion.
- Do not infer a form, product card or interactive control solely from visual resemblance. Group from generic source structure/relationships, not site selectors.
- Preserve the existing adaptation controls (`a`, `[adapt]`, `--no-adapt`) and view/navigation contracts. No new default mode or arbitrary configuration language is required.

### R1. Public compatibility and reuse

- Prefer additive APIs/default trait methods. `ImageView` has public fields: adding a required field would break external struct construction and is not the default design.
- Existing callers keep their documented default behaviour. Any necessary breaking change is isolated and called out for release review.
- Every selected shared improvement is exercised by a small framework-only consumer using public APIs and advertised Cargo features; it must not rely on workspace feature unification or private types.
- Do not add a mandatory Ratatui dependency, a new HTML engine, or browser types to generic widgets. Preserve the faithful `rs-rich` mirror and one-way dependencies.

## Acceptance corpus and procedure

Extend the current fixtures with three deterministic, locally served scenarios:

| ID | Scenario | Required sequence and assertions |
|---|---|---|
| A | Video with busy chat | Play known local video; append/edit/remove/reorder keyed messages with repeated/unique emotes; type a draft; scroll back; freeze/resume; open/close dialog. Video advances, messages remain correct, drafts/focus survive, scrollback holds, no graphic escapes its region. |
| B | Changing speed gauge | Canvas and SVG variants with independently changing visible text and semantic meter values; surrounding panels and a fixed overlay. Values and drawing update, labels remain readable, no duplicate/stale needle or overlay pixels. |
| C | Product grid | Eager images, delayed images with reserved size, delayed intrinsic-size images, long/Unicode labels, split prices, ratings, badges and a horizontal shelf. Scroll down/up, resize, reveal lazy images, navigate away/back. Item association, anchor and graphic containment remain correct. |

Run at 80×24, 120×40 and 160×50 cells, plus a narrow split of 40 columns. Exercise known 8×16, 10×20 and 12×24 cell pixels and a metric change without a column/row change. These are test inputs, not a declaration of supported terminal font sizes.

For each scenario compare projection and pixel view where that view's required graphics features are supported. Use reader mode as a semantic-content cross-check, not as a substitute for fixing projection.

Capture direct, SSH and tmux runs for Kitty, Ghostty, WezTerm and iTerm2, recording terminal/multiplexer version, OS, selected protocol, cell metrics, configuration and limitations. Include a Sixel-capable terminal and ANSI/half-block baseline. Unsupported combinations are marked explicitly; emulator-only success is not native verification. iTerm2 requires an actual supported host, not a claimed Linux emulation.

Automated gates: no stale placement after each lifecycle transition; reconstructed final scene agrees with a clean render; identity/action/draft invariants hold; playback-only and unchanged-geometry decode updates cause zero unrelated layout computations; frame work is bounded; unchanged scenes send no repeated image upload; the standalone public consumer builds.

Performance runs record p50/p95/p99 input and update latency, displayed frames, frame gaps/drops, bytes, CPU stage timings and resource peaks on the same machine/configuration. A suspected optimisation is retained only with improved relevant cost and no regression in correctness or responsiveness. Set numerical release thresholds from the recorded baseline before marking the optimisation task complete; do not invent hardware-independent FPS promises or use shared-runner wall-clock noise as the only CI gate.

Real-site checks of speedtest.net, Amazon and video/live-chat pages supplement the corpus on reachable ordinary sessions. A blocked site is recorded as unverified. Do not bypass access checks, sign in or send external chat messages as part of automated validation.

## Delivery, scheduling and non-goals

1. Reproduction corpus, baseline measurements and build setup.
2. Shared terminal metrics/capability corrections and lifecycle regressions.
3. Video/chat update separation and measured scrolling improvements.
4. Adaptive grouping/shared-control adoption and public examples.
5. Native-terminal evidence and release documentation.

Keep new API/component work aligned with post-1.0 issues #866/#867; #868 remains a separate optional mappings spike. Do not change 1.0 release gates or publish a release implicitly. Existing rendering bug fixes may be separated for appropriate maintenance review.

Not included: a general CSS compositor rewrite, site-specific adapters, DRM/access bypass, a new agent/MCP stack, Markdown/teletext modes, arbitrary user-defined widget mappings, or a new component suite duplicating current widgets.

## Engineering decisions left to implementation

Choose the smallest additive metrics/context API after tracing the current backend-to-placement path; choose internal signal/projection revisions that preserve authoritative `Page` state; select a scroll fast path only after the reduced movement test identifies its benefit and correctness; reuse/combine current controls before naming new ones. Record those decisions, the failing tests and actual terminal evidence with the implementation.

## References

- [Repository contract](../../../AGENTS.md)
- [Browser design](../../design/browser.md)
- [Second real-sites pass and its validation limits](../../design/noctiluca-real-sites-2.md)
- [Existing rendering performance work](../../design/render-performance.md)
- [Shared component audit #866](https://github.com/buchochelliq-labs/rs-rich-cli/issues/866)
- [Browser translation #867](https://github.com/buchochelliq-labs/rs-rich-cli/issues/867)
- [Performance evaluation approach #861](https://github.com/buchochelliq-labs/rs-rich-cli/issues/861)
