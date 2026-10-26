# rs-rich-doom as a test of rich: review handoff

Status: a review handoff, prepared on 2026-10-20 for the agent that continues
it. Saved for later; nothing in it has been acted on yet.

## Status on main when this was filed (2026-10-20)

The review read rich at `866b897`, the revision rs-rich-doom pins. Two of its
three rich findings may already be addressed on main by work that merged
after that revision; check them first:

| Finding | On main now | Next step |
|---|---|---|
| Image widget integration | `bbf0b22` (round 5 plugins, in 0.0.20): intuiTUIve's `art` images from bytes, with Kitty, Sixel and iTerm2 pixels | Check whether that covers a live, in-memory frame source with fitting, placement and cleanup; if not, what is left |
| Custom-palette Sixel | Still open: `rich-art`'s `encode_indexed` is `pub(crate)` and maps through the fixed ANSI palette | Design a public indices + palette + size encoder |
| Style-table lifetime | `8a3f243` (round 6, in 0.0.20): "styles that do not pile up" (style table compaction) | Measure a sustained run on the new crates to confirm it settles |

Moving rs-rich-doom onto the 0.0.20 crates (pixel cells, held keys,
`fixed_step`, stats) is already in the [0.0.21 plan](../plans/0.0.21.md),
workstream 1; this review's adapter findings fit alongside it.

---

Prepared on 20 October 2026 for another agent continuing the rs-rich-doom review.

**Purpose:** use Doom as a technology demo and a repeatable workload for finding gaps in the rs-rich ecosystem. The assessment is positive: the demo exercises substantial library functionality and reveals actionable API and resource-management questions. The next value comes from validating those findings and feeding reusable improvements back into the appropriate rich crates.

## User intent and scope

The user originally asked: “Review this repo and give me feedback on what you think of it? Is it good?”

The user then clarified: **“It was a test of rich to see where the gaps are like a tech demo.”**

Preserve that clarification. Assess the project by what it exercises, what it exposes, and how repeatably it can test rich. Installation polish and general game-release readiness are secondary to this purpose. A security audit is outside the requested review.

The latest instruction was to put the review into a note for another agent. This session reviewed source and created this handoff; it made no repository changes, opened no issues or pull requests, and did not merge, tag or release anything. Follow-up implementation scope should come from the next task.

## Reviewed versions and evidence

- Demo repository: [buchochelliq-labs/rs-rich-doom](https://github.com/buchochelliq-labs/rs-rich-doom).
- Reviewed main commit: `4edc2a78cc0eecccfbeb1a359b2259b3bbcab481`, merged on 19 October 2026.
- Rich dependency repository: [buchochelliq-labs/rs-rich-cli](https://github.com/buchochelliq-labs/rs-rich-cli).
- Pinned rich revision: `866b897a91e8ce05b1325685abffca15cbf612b1`.
- All five direct rich dependencies use that revision. See [Cargo.toml](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/Cargo.toml).

The review covered the README, Rust application modules, C shim and relevant engine argument handling, build and CI configuration, tests, benchmark and recording tapes. The saved half-block ANSI screenshot was rendered and inspected. Relevant upstream rich source was checked at the pinned revision.

**Verification limit:** the game was not compiled or played in a real terminal during this review. The environment had no Cargo/rustc and ordinary shell networking failed; authenticated GitHub file access worked. Source-level findings and rectangle calculations are identified below. No new FPS, memory, audio-device or terminal-compatibility measurements were produced.

These findings describe the reviewed revisions. Check current branches and existing issues before treating them as still outstanding; later rich changes may already address them.

## Overall assessment

The demo is a strong choice for testing rich. One recognisable application creates sustained image updates, frequent colour changes, sensitive keyboard interaction, graphics-placement work and a recording workload. These expose behaviour that is difficult to assess from a static screen alone.

The implementation has clear responsibilities:

| Component | Responsibility |
| - | - |
| `src/engine.rs` | Embedded Doom engine, platform callbacks, frame delivery and virtual clock |
| `src/app.rs` | intuiTUIve application, custom view and newest-frame updates |
| `src/keys.rs` | Terminal-key translation and held-key/release policy |
| `src/render.rs` | Aspect fitting, sampling and terminal-cell conversion |
| `src/graphics.rs` | Image protocol encoding and placement |
| `src/sound.rs` | Sound-effect mixing and music output |
| `src/data.rs` | IWAD lookup and Freedoom download |

Useful design choices include an engine thread separate from the app, `latest` delivery so a slow renderer discards obsolete frames, suppression of identical consecutive engine frames, cached resampling tables, palette-preserving sampling, and merging adjacent cells with identical styles.

Sources: [application](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/app.rs), [engine](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/engine.rs), [rendering](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/render.rs), [module structure](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/lib.rs).

## What the workload exercises

| Workload | Rich behaviour to assess |
| - | - |
| Continuously changing coloured frames | Rendering cost, allocations, style reuse and output volume |
| Half blocks and Braille | Cell conversion, colour fidelity and cell-painting performance |
| Kitty, Sixel and iTerm2 paths | Graphics encoding, placement, fallback and cleanup |
| Held and simultaneous keys | Input-event handling and the ergonomics of continuous interaction |
| Independent frame production | Responsiveness and frame coalescing when rendering falls behind |
| Changing terminal dimensions | Interaction between node layout, aspect fitting and graphic placement |
| PTY recordings | Whether rs-rich-record handles active graphical applications |

The repo includes deterministic Freedoom recording tapes and a headless drawing benchmark. The README reports roughly 33–35 application frames per second in release PTY runs for the tested sizes and modes; those are repository-reported results, not measurements from this review.

Sources: [README measurements](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/README.md#measured), [drawing benchmark](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/examples/draw_bench.rs), [half-block tape](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/docs/tapes/doom-half.tape), [Kitty tape](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/docs/tapes/doom-kitty.tape).

## Main rich findings

### Image widget integration

**Status: confirmed API integration gap at the pinned rich revision.**

The standard intuiTUIve image widget is oriented around image files and changing file paths. Its documentation and implementation say that Sixel and Auto use half blocks in that widget. Separately, rich provides the graphics-placement primitives, and the Doom demo connects them using its own `DoomView`, `Pictures` and `Picture`.

The conclusion is specific: the existing standard image node does not join the available graphics machinery into a convenient live-image interface. It would be incorrect to conclude that intuiTUIve or rich has no graphics support.

A useful upstream improvement is a reusable image/framebuffer widget that accepts in-memory frames or a reactive frame source and handles:

- Protocol selection and a cell fallback.
- Fitting the image into its node rectangle.
- Image identity and updates.
- Layout changes and placement.
- Cleanup when the widget disappears.

Keep Doom engine behaviour and game-specific display policy in the adapter. Move generic image delivery into rich where its API is useful to other applications.

A future verification should feed changing in-memory frames through the standard node, resize it, remove it, and check both the cell path and supported graphics paths for correct placement and cleanup.

Sources: [standard image widget](https://github.com/buchochelliq-labs/rs-rich-cli/blob/866b897a91e8ce05b1325685abffca15cbf612b1/crates/rich-intuituive/src/image.rs), [graphics contracts](https://github.com/buchochelliq-labs/rs-rich-cli/blob/866b897a91e8ce05b1325685abffca15cbf612b1/crates/rich-ext/src/frame/graphics.rs), [Doom application integration](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/app.rs), [Doom graphics integration](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/graphics.rs).

### Custom palette Sixel API

**Status: confirmed missing public API at the pinned rich revision; since
built (0.0.23): `sixel::encode_palette` and `index_exact` (doom gap 2), and
`sixel::encode_rgba`, which `SixelArt` now uses, keeping the exact colours
of a picture of 256 colours or fewer.**

Doom supplies a local `sixel_indexed` encoder to preserve the frame's existing colours. This was checked against upstream rather than assuming an equivalent public API did not exist:

- `SixelArt::encode()` resizes and, in its default truecolour path, uses the adaptive encoder.
- `SixelArt::encode_cells()` also uses the adaptive encoder.
- Upstream contains an indexed encoder, but it is `pub(crate)`.
- That internal encoder maps indices through the fixed ANSI palette; it does not accept an arbitrary caller-supplied palette.

A public API accepting indices, a palette and dimensions would let applications encode prepared indexed rasters and let rich own the reusable Sixel implementation. This is relevant to game frames, pixel art and emulator output.

Do not claim that the current adaptive encoder always changes colours or that the custom implementation is faster. Those outcomes require a decoded-output comparison and a benchmark. Sixel's own colour precision must also be reflected in expected results.

A future regression should supply a small palette with non-ANSI colours, encode and decode it, and check dimensions and colour fidelity within protocol precision. Performance comparisons should use the same prepared input.

Sources: [public encoding paths](https://github.com/buchochelliq-labs/rs-rich-cli/blob/866b897a91e8ce05b1325685abffca15cbf612b1/crates/rich-art/src/sixel.rs#L162-L217), [internal fixed-palette encoder](https://github.com/buchochelliq-labs/rs-rich-cli/blob/866b897a91e8ce05b1325685abffca15cbf612b1/crates/rich-art/src/sixel.rs#L221-L245), [Doom custom encoder](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/graphics.rs#L55-L146).

### Style table lifetime under changing colours

**Status: confirmed accumulating data structure and documented workaround; memory impact requires measurement.**

The Doom resampler explains why terminal cells use nearest-neighbour sampling: averaging can generate many new colours and therefore many new styles. Upstream `StyleTable::intern()` appends previously unseen styles to its vector and index. The surface keeps its screen between ordinary draws.

This makes a long-running animation with continuously changing colours a useful resource-management test. It raises a concrete question: does memory usage settle, or continue increasing with newly encountered colour combinations?

The review did not measure a memory leak, out-of-memory event or growth rate. Do not present any of those as observed. Nearest-neighbour sampling constrains colour variety; it is not proof that every long run has stable resource usage. A 256-colour source palette also does not imply only 256 styles: foreground/background combinations produce distinct styles, and the source palette can change over time.

Recommended investigation:

1. Measure process memory and retained style count during a sustained run.
2. Compare a palette-constrained workload with a synthetic workload that continually introduces new foreground/background combinations.
3. Track retained styles against currently live screen styles.
4. If growth is confirmed, consider style recycling or compaction while preserving the painter's style-ID and previous-frame assumptions.

Keep a small upstream regression for resource lifetime, with Doom as the broader integration workload.

Sources: [Doom's sampling rationale](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/render.rs#L104-L110), [StyleTable intern and storage](https://github.com/buchochelliq-labs/rs-rich-cli/blob/866b897a91e8ce05b1325685abffca15cbf612b1/crates/rich-ext/src/frame.rs#L54-L120), [persistent surface](https://github.com/buchochelliq-labs/rs-rich-cli/blob/866b897a91e8ce05b1325685abffca15cbf612b1/crates/rich-ext/src/surface.rs#L107-L124), [screen and painter](https://github.com/buchochelliq-labs/rs-rich-cli/blob/866b897a91e8ce05b1325685abffca15cbf612b1/crates/rich-ext/src/surface/screen.rs).

## Findings in the Doom adapter

These are useful source-level observations, but they should not be reported as confirmed defects in rich.

### Capped Sixel centring

**Status: confirmed rectangle-calculation issue; actual terminal display was not exercised.**

The app first calculates a centred rectangle. `Pictures::covered()` then reduces its width and height to respect the Sixel pixel cap, retaining the original `x` and `y`. The smaller image therefore retains the position of the larger one.

For a 160×50 terminal, 8×16-pixel cells and the default 640-pixel cap:

- The Sixel path reserves the last row, leaving 49 rows.
- The initial fitted rectangle is approximately 131×49 cells at `x=14, y=0`.
- The cap reduces the footprint to approximately 80×30 cells.
- The current calculation retains `x=14, y=0`; centring that final footprint in the available area would place it at approximately `x=40, y=9`.

Calculate final image dimensions and placement together, then centre the final footprint. Upstream already has centring and fitting primitives; this finding is attributed to the adapter's calculation. A combined geometry helper is an ergonomics opportunity, not proof that rich cannot centre graphics.

Sources: [view fitting](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/app.rs#L159-L182), [capped footprint and placement](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/graphics.rs#L185-L220), [existing rectangle centring](https://github.com/buchochelliq-labs/rs-rich-cli/blob/866b897a91e8ce05b1325685abffca15cbf612b1/crates/rich-ext/src/surface/screen.rs#L89-L106).

### Relative engine argument paths

**Status: confirmed launcher path-handling issue; secondary to the rich experiment.**

`play()` canonicalises the IWAD, changes the process working directory to the application data directory, then forwards the remaining Doom arguments unchanged. Relative paths supplied through arguments such as `-- -file mods/example.wad` consequently resolve from a different working directory than the caller may expect.

Preserve the caller's path semantics while directing saves/configuration explicitly, or resolve supported path-bearing arguments before changing directories. Do not blindly rewrite every argument: built-in demo names and non-path arguments need their original meaning.

Sources: [launcher](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/main.rs#L92-L115), [Doom WAD argument handling](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/doomgeneric/w_main.c).

### Multiple keys bound to one action

**Status: confirmed state-machine behaviour from source; a narrower gameplay edge case.**

The adapter maps `x`, `z` and Ctrl-derived fire to the same Doom FIRE key, while held state is tracked by the resulting Doom key. On a terminal reporting releases, holding `x`, pressing `z`, then releasing `x` can emit FIRE-up while `z` remains physically held. Repeats may later restore it, but the intervening state is wrong.

If addressed, track physical/source identities and aggregate them into a game action. Keep this attributed to the adapter's mapping policy unless a separate upstream defect is demonstrated.

Source: [key translation and held state](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/keys.rs#L108-L240).

### Engine lifecycle

The embedded C engine is intentionally once-per-process. Its exit callback parks the engine thread and the process ends elsewhere. That is documented and reasonable for the current standalone demo; it becomes relevant if future work needs repeated sessions or embedding. It is not a reason for a broad rewrite of this tech demo.

Source: [engine lifecycle](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/src/engine.rs#L148-L220).

## Existing tests and remaining validation

The headless integration test runs 400 engine ticks using a virtual clock, checks changing 320×200 frame content, converts a frame to terminal cells, and—with sound enabled—checks synthesised music without requiring an audio device. Unit tests cover key state, fitting and sampling, cell conversion, Sixel encoding, CLI/data handling and audio mixing.

The integration test currently returns successfully when `RS_RICH_DOOM_IWAD` is absent, after printing a skip message to captured stderr. A successful ordinary `cargo test` summary therefore does not establish that the actual engine test executed. CI sets the variable. The no-default-features CI step uses `--lib`, which leaves the binary and engine integration path outside that step.

Sources: [engine integration test](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/tests/engine.rs), [CI workflow](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/.github/workflows/ci.yml).

The most useful further checks for the tech-demo purpose are:

1. **Sustained rendering:** memory, retained styles, frame-time distribution, output bytes and responsiveness.
2. **Controlled comparisons:** same rich revision, game data, demo sequence, build profile, terminal size and renderer mode.
3. **Actual terminals:** resize during updates, hold simultaneous keys, release keys in different orders, close the view, exit and check cleanup.
4. **Minimal upstream regressions:** reproduce each library issue in the owning crate as well as keeping Doom as an integration workload.
5. **A findings record:** observation, workaround, owning crate, fix and resulting measurement.

The README says the graphics-mode escape output was exercised in a PTY, but the image modes had not yet been visually checked in real Kitty or Sixel terminals. Treat terminal support entries as expected compatibility until there is evidence for a tested terminal/version combination.

Source: [terminal support and validation notes](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/README.md#terminal-support).

### Starting commands for a suitable environment

These commands are starting points for another agent; they were not executed in this review. Use an absolute IWAD path and a terminal for interactive runs.

```sh
cargo test --locked

RS_RICH_DOOM_IWAD=/absolute/path/freedoom1.wad \
  cargo test --locked --test engine -- --nocapture

RS_RICH_DOOM_IWAD=/absolute/path/freedoom1.wad \
  cargo test --locked --no-default-features --test engine -- --nocapture

cargo run --locked --release --example draw_bench -- \
  /absolute/path/freedoom1.wad 80 24 half

cargo run --locked --release --example draw_bench -- \
  /absolute/path/freedoom1.wad 160 50 sixel
```

For a soak test, ensure the workload actually continues for the intended duration. A single `-playdemo` invocation may finish on its own; an upper time limit does not guarantee the game ran for that entire interval.

## CI context from the review

As of the review, the latest main run was [39305754297](https://github.com/buchochelliq-labs/rs-rich-doom/actions/runs/39305754297), created on 19 October 2026 for the reviewed commit. It concluded failure.

The macOS and Ubuntu jobs both had empty step lists, runner ID 0 and an empty runner name. Accessible job logs were unavailable. The best-supported interpretation is that the jobs did not reach runner execution; this run supplies no evidence that compilation, formatting, Clippy or tests were attempted. The precise pre-run cause was not established. Do not attribute it to code, dependencies, billing or an outage without new evidence.

The workflow defines Linux/macOS checks and downloads checksum-verified Freedoom for its engine test. No Windows job is present. Recheck current CI before relying on this dated snapshot.

Sources: [run](https://github.com/buchochelliq-labs/rs-rich-doom/actions/runs/39305754297), [jobs response](https://api.github.com/repos/buchochelliq-labs/rs-rich-doom/actions/runs/39305754297/jobs), [workflow](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/.github/workflows/ci.yml).

Source-install prerequisites, release binaries and broader distribution work were considered early in the review but are secondary after the user's tech-demo clarification. The source requires Rust 1.90 and a C compiler, with Linux ALSA development headers for the default sound feature. Those facts matter when arranging a reproducible test environment, rather than as a reason to mark down the experiment.

Sources: [manifest](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/Cargo.toml), [installation instructions](https://github.com/buchochelliq-labs/rs-rich-doom/blob/4edc2a78cc0eecccfbeb1a359b2259b3bbcab481/README.md#install).

## Suggested continuation

Start by checking current demo and rich revisions and whether these items already have issues or fixes. Keep evidence tied to the version actually tested.

Prioritise the two confirmed public-API opportunities—standard image-node integration and custom-palette Sixel encoding—and measure the style-table lifetime concern. Keep adapter fixes separately attributed. When an issue is validated, identify the smallest useful library change and a regression case that checks the behaviour independently of Doom.

A useful follow-up result would include:

- Confirmed current findings and any that are already resolved.
- The owning crate or adapter module for each finding.
- Reproduction steps and measurements where relevant.
- Focused implementation and test proposals.
- A short record of what Doom taught rich.

The overall judgement remains positive: this is a well-chosen demo that already supplies concrete feedback about rich's API integration, palette control and resource lifetime. Preserve its focused role as a demanding, repeatable workload.
