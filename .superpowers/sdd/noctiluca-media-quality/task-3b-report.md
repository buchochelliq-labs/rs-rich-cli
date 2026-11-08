# Task3b — bounded rectangular pictures, retained uploads and alpha underlay

Implementation status: DONE_WITH_CONCERNS, ready for controller review. No push,
merge or publication. Production source checkpoint is
`d13c45a675e2c25738e2920bf97f96564daf9afc`; the final report commit only adds this
report and a test-helper type alias. Baseline is
`b712137e9c1a73093d56c221e1c789fd7a9d02ab`.

Milestones: `fc1c852` implemented initial geometry/alpha/reuse; its performance
was explicitly rejected. `c51e62f` fixed measured metadata collision work,
restored Kitty row tagging, and corrected recorder invalidation. `d13c45a`
invalidates retained proof on release and preserves the public compositing tests.

## Result and public surface

- Stock `Picture::lines`, `picture_of`, `image_graphics`, Surface and Driver use
  the shared improvement automatically. No browser types or Noctiluca widget
  copies, mandatory Ratatui dependency, faithful-rich edits, or changed required
  `ImageView`/`PictureTag` fields were introduced. Job/video/script literals and
  existing terminal-scroll guards were untouched.
- Additive APIs: `PictureTag::{style_at,column_of,background_of}`,
  `Picture::encode_tag_rect_with_metrics`, and provided
  `Graphic::can_reuse_data` (default true). `Placement::same` is unchanged.
- Native overlay cell styles preserve exact source X and documented background.
  Kitty keeps its original one-segment-per-row representation: existing Unicode
  placeholder diacritics already identify source coordinates. Cells mode does
  not do native tagging. Legacy tags without X retain conservative full-width
  behavior.
- Fit/anchor the complete source first, then crop in physical pixels. Placement
  origin/size are the surviving rectangle, and every crop offset/extent, full
  dimensions, metrics, fit/anchor, version, protocol and overlay background enter
  cache/graphic identity. Kitty keeps one full-view identity across fragments.
- Adjacent matching spans merge only across consecutive screen AND source rows
  with the same inferred instance origin. Dialog holes are never filled.
- Accepted Sixel/iTerm draws erase only their encoded rectangle to the requested
  `ImageView.background` or terminal default before alpha blending. This happens
  after encoding acceptance and inside the existing graphics transaction. The
  clear prefix restores cursor and attributes BEFORE the existing encoder's
  save/restore pair, so it does not nest the single DEC save slot. All clear bytes
  are counted by the existing Picture byte counter. Rejected fragments retain
  the original fallback. This is not arbitrary prior-layer reconstruction.
- In-cell data reuse uses matching key/frame from the previous presented frame
  or an upload already scheduled in this plan; no persistent seen-ID cache.
  Both old/new graphics must permit reuse. Observed same-id/different-fit Kitty
  variants conservatively disable reuse. `release` invalidates that proof even
  for held/evicted graphics; absence followed by reappearance uploads again.
- The controller authorized the narrowly implicated recorder correction after
  native and immutable-base RED: real writes/erases permanently invalidate the
  touched cell-bound overlay pieces, even when the cell VALUE is unchanged.
  Kitty placements retain their separate lifetime. Existing recorder suites pass.

## Bounds and limitations

- At most 32 overlay rectangles per image instance and 128 overlay rectangles
  per frame. Exceeding the per-instance budget retains that instance's cells;
  exceeding the frame budget retains overlay fallbacks for the frame while Kitty
  uploads remain available. Candidate overflow is resolved before encoding.
- At most 128 encoding-cache entries and 16 MiB of retained escape data. Cache
  eviction does not delete retained Kitty ownership. Tests exercise both entry
  and byte eviction, dead versions, held evicted owners, release and reappearance.
- Full raster dimensions must be 1..4096 cells per axis and at most 16 Mi pixels;
  metrics normalize through `CellPixels::new`. Empty/reversed/out-of-range input
  is rejected; range ends clamp without overflowing. Existing `MAX_CELLS` is
  unchanged. Kitty fragments are cell placeholders, not per-cell native encodes;
  their geometry is bounded by the composed frame rather than overlay caps.
- Cold native fragments currently repeat full fitting. The measured 32-fragment
  8x8 workload costs about 6.2–6.6 ms cold and 18–19 µs warm; larger full rasters
  can cost more. This remains a bounded quality/correctness tradeoff, not an
  iTerm CPU optimization or a general responsiveness acceptance.
- Sixel uses binary alpha and quantization. Cropped fragments can choose a
  different palette than a whole-image encode. The native exposed crop improves
  visibly but is not pixel-identical to the initial whole-image quantization.
- Simultaneous different fits of the same Kitty Picture still share one terminal
  upload id: this pre-existing alias defect is NOT fixed. Reuse is disabled for
  observed conflicts, including transition back to one view. Distinct Pictures
  remain necessary for simultaneous independently fitted Kitty views.
- Foreign writes/terminal resource eviction outside the managed transaction are
  not newly detected. No terminal-scroll optimization through graphics enabled.

## Genuine RED / GREEN evidence

All logs below are retained in `/workspace/work/rs-rich-cli/work/task3b/`.
All functional commands source `/workspace/work/toolchain/env.sh`, unset
`NO_COLOR`, and use `TERM=xterm-256color`, except the explicitly withdrawn
misconfigured run below. Rust 1.90.0 was used; no full-workspace grammar linking
or faithful-rich lint repairs were attempted.

1. `cargo test -p rs-rich-intuituive --no-default-features --features art
   --test image_clipping -- --nocapture`: baseline public picture/overlay RED,
   `missing Iterm at 0,1` (false vs true), and moved Kitty still scheduled an
   upload. `red-clipping.log`: 0 passed, 2 failed. Initial corresponding GREEN:
   `green-clipping.log`: 2 passed. Final expanded suite: 11 passed.
2. Same public test target, `kitty_competing_fits_reupload_when_returning_to_one_view`:
   RED 0 uploads vs 1 required after a second fit overwrote the shared resource
   (`red-alias.log`). GREEN after conservative eligibility; unchanged conflicting
   variants also redraw. This does not make simultaneous aliasing correct.
3. `... --test image_clipping releasing_kitty -- --nocapture`: RED 0 vs 1 after
   `release(&[])` while old placements remained held (`red-release.log`). Final
   independent/repository suites GREEN after invalidating typed owner flags.
4. `cargo test -p rs-rich-ext --no-default-features --lib
   distinct_metadata_does_not -- --nocapture`: RED
   `metadata collisions require 1024 equality probes per insertion`
   (`red-metadata-work.log`), GREEN (`green-metadata-work.log`). Hash public Meta,
   with equality still confirmed. Tests cover all MetaValue forms, NaN/-0,
   insertion-order semantics, replacement in place and empty-vs-absent metadata.
5. Corrected alpha test, exact same source file in independent baseline/current
   consumers (SHA-256 `3142160061ba8997001945c54394924e87d75778c6cdea17c7542e9dd0a964ad`):
   `CARGO_TARGET_DIR=/workspace/work/task3b-base-target cargo test
   --manifest-path /workspace/work/task3b-base-consumer/Cargo.toml
   --test alpha_underlay transparent_native -- --nocapture`.
   Immutable b712 RED (`red-alpha-immutable.log`): decoded PNG corner alpha 0 and
   edge alpha 100 were preserved, but actual underlay was a colored `▀` versus
   clean blank over RGB(17,35,61); 21 raster pixels differed. Both scenes hide
   the cursor. Current GREEN: default and explicit backgrounds, fully and
   partly transparent images, exact actual decoded PNG, and actual cursor/style
   preservation through both overlay protocols (`green-public.log`, final
   `check-compositing-final.log`).
   **Withdrawn:** original `red-alpha.log`/early debug comparisons did not match
   cursor visibility. Their 21-pixel number was confounded and is NOT the proof.
   Corrected immutable RED and current GREEN replace them.
6. Recorder control:
   `CARGO_TARGET_DIR=/workspace/work/task3b-base-target cargo test
   --manifest-path /workspace/work/task3b-base-consumer/Cargo.toml
   --test recorder_limit -- --nocapture`: immutable baseline keeps Sixel AND
   iTerm tiles after same-value ECH (`red-recorder-immutable-both.log`, 2 failures).
   `cargo test -p rs-rich-record --test overlay_erasure -- --nocapture`:
   RED 4 cells vs 3 after one-cell erasure, and partial EL/ED failure
   (`red-recorder.log`, 2 failures); GREEN (`green-recorder.log`, 2 passed).
   Tests include literal blank writes, ECH, partial line/display erase, surviving
   fragments, and no later resurrection. Native control independently confirmed
   ECH2 and two literal spaces erase a red two-cell image on both Xterm and WezTerm.
7. Incremental recorder scenes initially failed for Sixel scroll transition 2
   because erased blank underlays compared equal and old tiles reappeared.
   `scene-debug.log` preserves that reduction. After actual-write invalidation,
   public Driver initial→dialog→scroll→resize→dismiss→remove→re-add agrees exactly
   with fresh final rendering for Cells/Kitty/Sixel/iTerm. Resize is only used at
   the designated resize transitions; it does not replace incremental checks.

The adjacent same-view regression passed immediately and is characterization,
not a claimed RED. No emitted-byte result is called a native compositing pass.

## Affected verification ledger

- `cargo test -p rs-rich-art --no-default-features --features picture,frames`:
  `check-art-terminal.log`: 158 unit passed, 1 ignored; 56 integration/doc passes.
  `check-art.log` is an earlier invalid-environment run: TERM=dumb caused three
  existing image_art width/fit tests to fail (155 passed). Re-running with the
  specified terminal environment passed them; no image_art behavior was changed.
- `cargo test -p rs-rich-ext --no-default-features --lib`:
  `check-ext.log`: 448 passed.
- `cargo test -p rs-rich-intuituive --no-default-features --features art --lib
  --test image_clipping --test image_compositing --test graphics_lifecycle
  --test metric_compat -- --nocapture`: `check-intuituive.log`: 149 library,
  9 then-current clipping, 3 lifecycle, 3 compositing, 1 compatibility passed.
  After the release/byte-budget follow-up, `--lib image::tests` yielded 26 passed
  (`check-final-images.log`); its integration targets were FILTERED OUT and are
  not counted as passes. The separate unfiltered final integration run
  (`check-public-repo-final.log`) passed 11 clipping, 3 compositing, 3 lifecycle,
  1 literal/trait compatibility. A final test-only type alias was revalidated
  with `--test image_compositing` (3 passed).
- `cargo test -p rs-rich-record`: `check-record.log`: 95 unit and 22
  integration/doc passes, including recording programs, keyboard flags and
  existing raw Kitty tests.
- `cargo test -p rs-rich-micro --test graphics --test rows --test pty`:
  `check-micro.log`: 17 + 18 + 7 passed.
- `CARGO_TARGET_DIR=/workspace/work/rs-rich-cli/target cargo test
  --manifest-path /workspace/work/public-widget-consumer/Cargo.toml --features art`:
  `check-public-final.log`: all 20 tests passed (alpha 2, composed lifecycle 1,
  old literal compatibility 1, old graphics lifecycle 4, clipping 11, task4 1).
  An earlier full run caught the genuine release RED (10/11 clipping passed);
  the final log replaces it. Manifest advertises intuiTUIve art and rich-art
  picture with defaults disabled; recorder is test-only. No Noctiluca dependency
  or workspace feature-unification assumption. The core new tests are also
  retained in the repository, not only in the external consumer.
- `cargo build -p noctiluca --locked`: passed (`build-noctiluca.log`). Preserved
  `/workspace/work/bin/noctiluca-task3b-d13c45a`, SHA-256
  `e2ca7c8c7ef18242e75bac3e4554d3f2446d53b3e4fffb5dd1848130be72b1f5`.
- `cargo fmt --all`; `git diff --check`: passed.

### Clippy limits, not hidden

Strict affected all-target `-D warnings` does NOT pass the pre-existing tree:
`rich-art/src/image_art.rs:811` and `rich-intuituive/src/select.rs:393`
`nonminimal_bool`; `rich-ext/examples/guide_markdown_export.rs:32`
`let_and_return`. Broad no-default all-target traversal also reaches the existing
`markdown_viewer` test without its `syntax` feature (unresolved imports).
Logs: `check-clippy*.log`. No unrelated production files were edited to hide them.
My deliberate reversed-range test literal and new test helper type-complexity
warning were fixed.

Focused unchanged-baseline-aware checks all passed (logs
`clippy-ext-focused.log`, `clippy-record-focused.log`,
`check-clippy-art-remaining.log`, `clippy-intuituive-final.log`):
`cargo clippy -p rs-rich-ext --no-default-features --lib --no-deps -- -D warnings`;
`cargo clippy -p rs-rich-record --lib --test overlay_erasure --no-deps -- -D warnings`;
`cargo clippy -p rs-rich-art --no-default-features --features picture,frames
--lib --tests --no-deps -- -D warnings -A clippy::nonminimal_bool`;
and intuiTUIve's lib, exact changed integration targets, and metric_pictures
example with art/no-default features, `--no-deps -- -D warnings
-A clippy::nonminimal_bool`. These exceptions are explicit diagnostics of the
remaining checks, not a claim that strict workspace clippy passes. Existing
full-workspace faithful-rich lint/link/disk limitations remain in the controller
ledger; no huge full-workspace retry was made.

## Measured work and provenance

Same actual public Driver workload: white `Picture::from_rgba(4,4,...)`, fitted
4x3, measured 8x16, move offsets 1,2,1,0 after initial placement.

| Protocol | b712 move uploads / graphic bytes | d13 move uploads / graphic bytes |
|---|---:|---:|
| Cells | 0 / 0 | 0 / 0 |
| Kitty | 4 / 24,820 | 0 / 0 |
| Sixel | 4 / 380 | 4 / 492 |
| iTerm | 4 / 1,736 | 4 / 1,848 |

Logs `movement-base.log`, `green-public.log`. The added 112 bytes for four overlay
moves are the accepted underlay clears, counted through Picture. The historical
33,012-byte Task3 sample used a different source construction and is NOT this
paired baseline. No input latency/FPS/native timing claim follows from these bytes.

The first implementation's debug benchmark was unacceptable: 80x24 white Iterm
Frame construction rose from 1.63 ms to 617.61 ms, and Kitty unnecessarily gained
per-cell segments. Metadata omitted from the style hash caused quadratic chains.
Those results (`cost-base.txt`, `cost-after.txt`, original source/hashes) are kept
and explicitly rejected; the deterministic chain test and final optimized samples
show the correction. Initial shared-target cross-worktree preparation produced
stale Cargo artifacts and was discarded; it is not timing/RED evidence.

Authoritative final benchmark uses two independent external manifests with the
same advertised production dependencies/defaults disabled, byte-identical source
`work/task3b/tag_cost.rs`, Rust 1.90.0 `cargo build --release`, separate targets:
`/workspace/work/task3b-base-target` and `/workspace/work/task3b-after-target`.
Baseline worktree stays b712; current production was frozen at d13 before rebuilding.
`TASK3B_SOURCE` embeds the source label. Runtime self-check proves baseline lacks
source-column metadata/native clear and current has both. SHA-256:

- source: `ac58cd3a9a3f8f233967e2ff56d5c18e75f8c343c0b7fc681ca8a219cde6b1b2`
- baseline binary: `d0e2491cddb6f5dc4ed97d0cf1ee9b0d28324471775b65c636be24e6a23d8424`
- current binary: `084b968c494bc8d787052ffb6aa5e63bef421a5c50bb2a9e6d4d85d2f26acdd3`

Built with `TASK3B_SOURCE=<source> CARGO_TARGET_DIR=<separate-target> cargo build
--release --manifest-path /workspace/work/task3b-cost-{base,after}/Cargo.toml`.
Executed saved binaries in base→after→after→base order with NO_COLOR unset and
TERM=xterm-256color, only after all native capture/build/lint jobs were idle.
Each workload discards 5 warmups; 1000/300/30 samples for 4x3/16x8/80x24, 20 for
cold/warm fragment cases. Raw forward and reverse samples follow. They measure
stage costs, not actual terminal presentation or an overall responsiveness gate.

First-pair p50 sums: Kitty 80x24 approximately 0.556 ms before/after; Cells remains
within run noise. Iterm 16x8 rises approximately 80→195 µs, 80x24 0.889→2.548 ms.
This is a measured provenance/quality cost. 32 accepted fragments cost ~6.5 ms
cold and ~19 µs warm in this small fixture; a 36-fragment shape rejects before
encoding, while the unchanged accepted scene schedules zero draws. No claim
of an iTerm CPU win, universal latency threshold or full-fit reuse is made.

## Native evidence supplied by the controller worker

Read `/workspace/work/task3b-native-after-report.md` and the native capture
provenance. Actual native control on Xterm398 Sixel and WezTerm20240203 iTerm
confirms ECH2/literal two spaces erase images (`native-overlay-erase-control.md`).

Matched Xterm product-grid b712→d13: uncovered image is smooth instead of blocky;
exposed x962..1161/y172..281 difference from initial reduces 21,906/22,000 to
7,502/22,000 pixels, mean channel difference 1.835, max26, only 2 pixels >20.
This is improved quality, NOT exact whole-image palette preservation. Dialog
interior stays clear. Dismissal and down/up return the complete initial PNG
byte-identically (hash `a6d903d59512919e6d237f7aaee29ecca89bdfbc1e7bb90e9efe26e76f3d4f3f`).
Matched WezTerm auto ITERM before/after shows rounded emotes/badges against black
without the prior rectangular colored corners. This is native evidence for these
fixtures and this terminal, not macOS iTerm2 or every alpha composition scenario.
Captures: `/workspace/work/native-terminals/task3b-{before,after}-{xterm,wezterm}/`.
No native Kitty movement capture was produced in this subtask; byte/recorder
proof is not substituted for that remaining controller acceptance gate. General
release/input/frame-pacing performance remains Task6.

### Optimized raw sample base-1 (nanoseconds)

```text
selfcheck source=b712137e9c1a73093d56c221e1c789fd7a9d02ab source_column=false native_clear=false
4x3 Cells segments=12 lines_p50p95_ns=(19410, 23588) frame_p50p95_ns=(874, 1221) placement_p50p95_ns=(124, 216)
4x3 Kitty segments=3 lines_p50p95_ns=(20822, 23895) frame_p50p95_ns=(1548, 1718) placement_p50p95_ns=(301, 360)
4x3 Iterm segments=12 lines_p50p95_ns=(21860, 25239) frame_p50p95_ns=(1885, 2313) placement_p50p95_ns=(451, 514)
16x8 Cells segments=128 lines_p50p95_ns=(42306, 61624) frame_p50p95_ns=(4692, 5167) placement_p50p95_ns=(657, 755)
16x8 Kitty segments=8 lines_p50p95_ns=(49674, 71506) frame_p50p95_ns=(9067, 10013) placement_p50p95_ns=(589, 808)
16x8 Iterm segments=128 lines_p50p95_ns=(65424, 89582) frame_p50p95_ns=(12335, 13153) placement_p50p95_ns=(2488, 2747)
80x24 Cells segments=1920 lines_p50p95_ns=(512279, 539001) frame_p50p95_ns=(60369, 78964) placement_p50p95_ns=(9048, 13328)
80x24 Kitty segments=24 lines_p50p95_ns=(454345, 577120) frame_p50p95_ns=(95217, 111066) placement_p50p95_ns=(1872, 2262)
80x24 Iterm segments=1920 lines_p50p95_ns=(717875, 967702) frame_p50p95_ns=(143037, 178212) placement_p50p95_ns=(28486, 58085)
fragments Iterm grid=8x8 accepted=0 cold_p50p95_ns=(570, 611) warm_p50p95_ns=(575, 606) initial_bytes=0 unchanged_draws=0
fragments Iterm grid=8x9 accepted=0 cold_p50p95_ns=(634, 708) warm_p50p95_ns=(633, 661) initial_bytes=0 unchanged_draws=0
fragments Sixel grid=8x8 accepted=0 cold_p50p95_ns=(565, 590) warm_p50p95_ns=(565, 600) initial_bytes=0 unchanged_draws=0
fragments Sixel grid=8x9 accepted=0 cold_p50p95_ns=(632, 646) warm_p50p95_ns=(631, 682) initial_bytes=0 unchanged_draws=0
```

### Optimized raw sample after-1 (nanoseconds)

```text
selfcheck source=d13c45a source_column=true native_clear=true
4x3 Cells segments=12 lines_p50p95_ns=(19570, 45821) frame_p50p95_ns=(864, 1422) placement_p50p95_ns=(138, 254)
4x3 Kitty segments=3 lines_p50p95_ns=(20840, 31909) frame_p50p95_ns=(1572, 2298) placement_p50p95_ns=(1179, 1620)
4x3 Iterm segments=12 lines_p50p95_ns=(28965, 36289) frame_p50p95_ns=(4989, 5859) placement_p50p95_ns=(1595, 1996)
16x8 Cells segments=128 lines_p50p95_ns=(42226, 85013) frame_p50p95_ns=(4661, 5246) placement_p50p95_ns=(616, 782)
16x8 Kitty segments=8 lines_p50p95_ns=(50237, 114482) frame_p50p95_ns=(8126, 9398) placement_p50p95_ns=(2980, 3600)
16x8 Iterm segments=128 lines_p50p95_ns=(140790, 189974) frame_p50p95_ns=(47182, 76516) placement_p50p95_ns=(7263, 7939)
80x24 Cells segments=1920 lines_p50p95_ns=(540516, 972261) frame_p50p95_ns=(58363, 194228) placement_p50p95_ns=(6083, 6587)
80x24 Kitty segments=24 lines_p50p95_ns=(459152, 517861) frame_p50p95_ns=(83117, 99550) placement_p50p95_ns=(14121, 14754)
80x24 Iterm segments=1920 lines_p50p95_ns=(1782241, 2231009) frame_p50p95_ns=(687856, 918134) placement_p50p95_ns=(77588, 126522)
fragments Iterm grid=8x8 accepted=32 cold_p50p95_ns=(6552767, 8061080) warm_p50p95_ns=(18904, 20899) initial_bytes=9664 unchanged_draws=0
fragments Iterm grid=8x9 accepted=0 cold_p50p95_ns=(15579, 16631) warm_p50p95_ns=(15706, 32627) initial_bytes=0 unchanged_draws=0
fragments Sixel grid=8x8 accepted=32 cold_p50p95_ns=(6353765, 8695060) warm_p50p95_ns=(19032, 21899) initial_bytes=2176 unchanged_draws=0
fragments Sixel grid=8x9 accepted=0 cold_p50p95_ns=(15661, 16424) warm_p50p95_ns=(15833, 34802) initial_bytes=0 unchanged_draws=0
```

### Optimized raw sample after-2 (nanoseconds)

```text
selfcheck source=d13c45a source_column=true native_clear=true
4x3 Cells segments=12 lines_p50p95_ns=(19604, 21901) frame_p50p95_ns=(890, 1130) placement_p50p95_ns=(138, 166)
4x3 Kitty segments=3 lines_p50p95_ns=(20953, 24691) frame_p50p95_ns=(1561, 2217) placement_p50p95_ns=(1172, 1380)
4x3 Iterm segments=12 lines_p50p95_ns=(29087, 36957) frame_p50p95_ns=(4984, 5850) placement_p50p95_ns=(1611, 2018)
16x8 Cells segments=128 lines_p50p95_ns=(42451, 77587) frame_p50p95_ns=(4688, 5482) placement_p50p95_ns=(638, 757)
16x8 Kitty segments=8 lines_p50p95_ns=(50585, 102212) frame_p50p95_ns=(8123, 10766) placement_p50p95_ns=(3028, 3531)
16x8 Iterm segments=128 lines_p50p95_ns=(143059, 190055) frame_p50p95_ns=(47542, 66277) placement_p50p95_ns=(7205, 7796)
80x24 Cells segments=1920 lines_p50p95_ns=(504631, 652473) frame_p50p95_ns=(59224, 88706) placement_p50p95_ns=(5917, 6070)
80x24 Kitty segments=24 lines_p50p95_ns=(455598, 571839) frame_p50p95_ns=(83530, 225439) placement_p50p95_ns=(14233, 15724)
80x24 Iterm segments=1920 lines_p50p95_ns=(1755586, 2011038) frame_p50p95_ns=(684319, 788757) placement_p50p95_ns=(76351, 103502)
fragments Iterm grid=8x8 accepted=32 cold_p50p95_ns=(6223206, 6815114) warm_p50p95_ns=(18141, 19996) initial_bytes=9664 unchanged_draws=0
fragments Iterm grid=8x9 accepted=0 cold_p50p95_ns=(15840, 38607) warm_p50p95_ns=(15891, 16621) initial_bytes=0 unchanged_draws=0
fragments Sixel grid=8x8 accepted=32 cold_p50p95_ns=(6303878, 9003248) warm_p50p95_ns=(18120, 22104) initial_bytes=2176 unchanged_draws=0
fragments Sixel grid=8x9 accepted=0 cold_p50p95_ns=(15694, 16111) warm_p50p95_ns=(15589, 16024) initial_bytes=0 unchanged_draws=0
```

### Optimized raw sample base-2 (nanoseconds)

```text
selfcheck source=b712137e9c1a73093d56c221e1c789fd7a9d02ab source_column=false native_clear=false
4x3 Cells segments=12 lines_p50p95_ns=(19548, 24147) frame_p50p95_ns=(890, 1258) placement_p50p95_ns=(124, 219)
4x3 Kitty segments=3 lines_p50p95_ns=(20933, 47358) frame_p50p95_ns=(1553, 2210) placement_p50p95_ns=(301, 478)
4x3 Iterm segments=12 lines_p50p95_ns=(22053, 26337) frame_p50p95_ns=(1889, 2453) placement_p50p95_ns=(469, 614)
16x8 Cells segments=128 lines_p50p95_ns=(43474, 81288) frame_p50p95_ns=(4681, 5212) placement_p50p95_ns=(658, 782)
16x8 Kitty segments=8 lines_p50p95_ns=(50680, 69579) frame_p50p95_ns=(9088, 9771) placement_p50p95_ns=(583, 771)
16x8 Iterm segments=128 lines_p50p95_ns=(66748, 96915) frame_p50p95_ns=(12319, 13438) placement_p50p95_ns=(2508, 2774)
80x24 Cells segments=1920 lines_p50p95_ns=(570095, 734518) frame_p50p95_ns=(58921, 141356) placement_p50p95_ns=(9163, 9338)
80x24 Kitty segments=24 lines_p50p95_ns=(474453, 630173) frame_p50p95_ns=(96670, 201619) placement_p50p95_ns=(2055, 2544)
80x24 Iterm segments=1920 lines_p50p95_ns=(812042, 952919) frame_p50p95_ns=(144497, 332188) placement_p50p95_ns=(28726, 55679)
fragments Iterm grid=8x8 accepted=0 cold_p50p95_ns=(593, 617) warm_p50p95_ns=(590, 620) initial_bytes=0 unchanged_draws=0
fragments Iterm grid=8x9 accepted=0 cold_p50p95_ns=(631, 650) warm_p50p95_ns=(632, 661) initial_bytes=0 unchanged_draws=0
fragments Sixel grid=8x8 accepted=0 cold_p50p95_ns=(574, 582) warm_p50p95_ns=(566, 593) initial_bytes=0 unchanged_draws=0
fragments Sixel grid=8x9 accepted=0 cold_p50p95_ns=(631, 660) warm_p50p95_ns=(629, 651) initial_bytes=0 unchanged_draws=0
```

## Final rebuild provenance

A final `cargo build -p noctiluca --locked` after the test-helper lint cleanup
passed (`build-noctiluca-final.log`). It relinked the CLI with SHA-256
`ca623b26735dd4d808d2bc086fa5030afd694fa0e89462a672ef93d9c8597b7d`, preserved
separately at `/workspace/work/bin/noctiluca-task3b-8f06af9`. Native captures above
remain tied to the earlier immutable `e2ca7c...` binary; they are not relabeled
as captures of this second build. `git diff d13c45a..8f06af9` contains only this
report and the test-helper type alias, with no production changes. The distinct
linked hashes are retained explicitly; no byte-identical build claim is made.
