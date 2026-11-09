# Noctiluca media quality: handoff

Updated: 2026-11-09. Branch: `codex/noctiluca-media-quality`. The aggregate final review was measured at `cbc7d930b81acdde5800755b69eece2b218eeccb`; follow-up source-grid and video/chat semantic fixes are now at `f3090737be9d569fd29b0260ba18148b65caa57b`. **Release acceptance remains blocked:** the previous final iTerm-compatible runs failed at least one original performance gate, and the current source has not been requalified against that gate. The eight-site visual pass below is qualitative and does not measure performance.

## Behavior and compatibility

Source dialogs use generic intuiTUIve layers with owner-controlled focus, pointer confinement and pane-local maximize scopes. Absolute/static declared dialogs and open native dialogs form independent surfaces; document surfaces scroll and fixed surfaces retain their source placement. Hidden/offscreen surfaces cannot trap focus or intercept source Escape. Popup underlays retain configured PageColors and accessible text twins, including canonical split-price alternatives, while geometric adaptations are suppressed. Faithful mode and explicit rule disables retain their behavior. Unsupported color syntax cannot establish opacity.

Waiting encodes and prepared results are bounded to current document/view/region incarnations. A current executing encode can finish while newer offers coalesce. Chrome guards model-owned fallback/canvas captures before and after document/applied-viewport changes; standalone capture remains compatible. A playing region partly outside the viewport uses the existing whole-region fallback instead of waiting indefinitely for a quiet screencast. Strict incomplete-crop rejection, admission, codec/quality and pacing limits are retained.

Bound picture provenance uses a checked compact carrier with exact ID/version, geometry, source column, fit/anchor, protocol and background/binding flags; unsupported values retain legacy metadata. Public unbound lines and `style()`/`style_at()` remain compatible. Generic Kitty span planning accepts adjacent copies restarting at an equal/backward nonzero source column while rejecting malformed and forward-jump spans. Manual Driver loops acknowledge successful writes with `output_written()` and retain cleanup resources with `finish_output()`; see the [image guide](../guide/intuituive/plugins.md#images-art).

The sole required model migration is `Page.overlays`: complete Noctiluca `Page { ... }` literals add `overlays: Vec::new()` or use `..Page::default()`. `Page::dialogs()` exposes declared source dialogs. No required fields were added to public `ImageView`, `PictureTag`, `FrameMeta`, Layer/Stack/Projection/Job or faithful mirror types. Generic core code imports no browser types. See the [layout guide](../guide/intuituive/layout.md#owner-controlled-overlays).

## Verification and immutable builds

The full workspace at shared-source milestone `60037d9` passed **6,965 tests, 21 ignored** across 449 suites. The later production changes are confined to leaf Noctiluca; its complete final-source package passed **365 tests, 3 ignored** across 36 suites. This is not a full-workspace rerun claim at `cbc7d93`. The independent feature-isolated framework consumer, with no Noctiluca dependency, passed 54 public-API tests, including complete struct literals and graphics clipping/compositing. Modified-crate scoped all-target lint passes with the verified baseline-only `nonminimal_bool` allowance; strict workspace lint still has the three pre-existing faithful Rich diagnostics. Formatting and whitespace pass.

The follow-up source `f309073` passes the complete Noctiluca package suite (**433 passed, 3 ignored** across 37 test binaries), scoped all-target Clippy with the verified baseline allowance, formatting, and whitespace checks. New RED/GREEN regressions cover accessible `aria-live` chat pairing beside video, a 32-cell minimum grid track, and ordered stacking of narrow unequal tracks. These checks do not replace a current-source release performance run.

Actual sandboxed Chromium regressions separately passed current-document/applied-viewport capture guards and changing pixels for the partially visible video: before the correction 58 distinct casts produced zero delivered video frames; afterward 52 distinct casts produced 23 distinct video pixel buffers. Normal optional-live package tests do not replace these explicitly executed integrations.

Both final artifacts are Rust 1.90 release builds, mode `0555` (read/execute, write protected):

- CLI: `/workspace/work/final-wave-immutable-binaries/noctiluca-cbc7d93-release-rust190`, SHA-256 `8ec19f52ad9d627007331bdc4a73b347ea4e518ffeed6c2312e8621e1ca97ed9`.
- Harness: `/workspace/work/final-wave-immutable-binaries/cbc7d93-harness-99f214ce-tests`, SHA-256 `3cc28dafe30ae1525c86b30cb840306517cc911d1e2987974cd3c689819aa396`.

All earlier immutable binaries, build logs, raw 33 observations, oracles and screenshots remain preserved. No faithful `crates/rich` source changed, no new release thresholds were set, and nothing was pushed, merged or published.

## Native observations

These are bounded qualitative observations on the explicitly named source, not FPS, physical-display latency or all-corpus conformance. Full local evidence is `/workspace/work/final-wave-native-review.md`.

| Source / terminal | Observed | Limits |
|---|---|---|
| `cbc7d93`, Kitty 0.41.1 direct, 40×24 / 400×456 TTY pixels | Local stream movie content changes in a video-only ROI: 32,162/41,310 pixels differ between two samples. Chat #7–#24 and Message/Send remain readable after scrolling. | No message sent. This corrects the frozen movie ROI observed on `be730eb`; no native frame-rate or response-time study. |
| `be730eb`, light Xterm 398, Sixel, 120×40 | Brand/Price/Delivery are readable; canonical `$29.99`, `$24.49`, `$49.00` appear with the source cookie popup visible. F1/Escape returns the initial PNG exactly. | Opaque fixture images do not prove alpha composition. PageUp returns row1 rather than initial row5, so scrolling screenshots are not byte-identical. |
| `be730eb`, WezTerm 20240203, auto iTerm-compatible graphics, 120×40 | Product images/canonical prices visible; F1/Escape returns the initial PNG exactly. | No product scroll test; this is not actual macOS iTerm2. |
| `be730eb`, Kitty, actual Speedtest at 40×24 and 120×40 | Source privacy popup visible. At 40 columns, one Tab then three PageDowns reveals controls and `f` shows their hints; 120-column F1/Escape restores the initial PNG. | Some narrow button labels truncate; no consent or speed test selected. |
| `be730eb`, Kitty, public Twitch/Giphy | Twitch video changes after focused play; a Giphy cat tile changes in pixel view. Hacker News story list is readable. | Giphy projection still has gaps/clipped links. These samples do not prove smoothness or all-media support. |

### Eight-site follow-up — `f309073`

A fresh anonymous Kitty 0.41.1 run captured all five states at 120×40 for BBC News, Wikipedia, YouTube, Amazon, Twitch, Giphy, Speedtest, and MDN. F1/Escape restored the page on all sites and runner cleanup passed. The screenshots confirm that the new generic grid regressions are useful but do not establish that every real page is polished: BBC's opening columns remain awkward, Wikipedia retains a large placeholder, Twitch still shows overlapping controls and `PAUSED 0:00`, Giphy retains blank tiles and right-edge clipping, Speedtest's privacy dialog dominates, and MDN's navigation/code remain crowded. YouTube still fails with `ERR_TUNNEL_CONNECTION_FAILED`; Amazon shows an unsupported-client page. No consent, login, chat, playback, purchase, or speed-test interaction occurred. This qualitative review does not measure scrolling judder or frame rate; see the [fresh contact sheet](/workspace/work/native-terminals/noctiluca-real-sites-f309073/review-overview.jpg) and [full site-by-site notes](/workspace/work/noctiluca-real-sites-f309073-review.md).

Public Wikipedia shows opening paragraphs with a placeholder/right-edge truncation; BBC and MDN have large gaps or fragmented/clipped text. Their causes remain unresolved. YouTube failed with `ERR_TUNNEL_CONNECTION_FAILED`; Amazon returned an unsupported-client page titled `500 - Amazon.com`. These are failures/unknowns, not site passes. Ghostty, actual macOS iTerm2 and SSH remain unverified. Earlier source-specific captures remain preserved and are not relabeled as the current build.

## Final quantitative attempt — failed

The sole declared final sequence ran exactly three current repeats each for iTerm-compatible output, Kitty and ANSI after the native content check and in a quiet window. Source, saved setup/body, release toolchain, quality/codec, byte budget, 60 frame offers, 20 semantic offers, 8 keys, 18 static images and 120×40 workload were unchanged. Frozen baseline binary SHA begins `23b7a280`; the original iTerm gate file remains byte-identical. No replacement runs or new baseline were used.

Original limits: iTerm p95 ≤13.670ms, p99 ≤15.734ms, input maximum ≤5.417ms, child CPU ≤0.806s; Kitty 46.442/50.885/16.178ms; ANSI 14.857/15.494/13.310ms. Every run passed independent emitted-color/identity decoding and population checks: all 20 semantic updates, all 8 keys observed, zero unrelated layouts, and emitted+censored=60. Emission/censoring identities are retained, rather than filtered to match older counts.

| Run | Frame p95/p99 ms | Input max ms | Child CPU s | Emitted/censored | Failed original gates |
|---|---:|---:|---:|---:|---|
| iTerm 1 | 13.358 / 13.692 | 5.769 | 0.802852 | 59 / 1 | input |
| iTerm 2 | 14.761 / 20.507 | 4.311 | 0.853713 | 60 / 0 | p95, p99, CPU |
| iTerm 3 | 13.585 / 13.667 | 5.022 | 0.861565 | 59 / 1 | CPU |
| Kitty 1 | 12.154 / 14.249 | 2.048 | 0.406366 | 25 / 35 | none |
| Kitty 2 | 12.963 / 13.715 | 1.503 | 0.381869 | 25 / 35 | none |
| Kitty 3 | 11.756 / 13.110 | 2.322 | 0.345280 | 25 / 35 | none |
| ANSI 1 | 3.806 / 5.315 | 2.514 | 0.268646 | 60 / 0 | none |
| ANSI 2 | 3.678 / 3.904 | 2.828 | 0.235291 | 60 / 0 | none |
| ANSI 3 | 3.730 / 4.012 | 2.248 | 0.230993 | 60 / 0 | none |

Raw evidence: `/workspace/work/final-wave-release-measurements/` (all nine recordings/logs/decodes), pre-run `final-wave-release-declaration.json`, complete statistics and identities `final-wave-release-comparison.json`, and file hashes `final-wave-release-raw-manifest.json`. The full aggregate report is `.superpowers/sdd/noctiluca-media-quality/final-wave-report.md`. Component allocation reductions do not override these application failures. This branch is being raised for review; merge and release acceptance remain separate, and the iTerm-compatible performance gate needs a passing current-source run.
