# Noctiluca media quality: handoff

Updated: 2026-11-08. Branch: `codex/noctiluca-media-quality`. The design baseline is `7fcdbf7263f81d89c2975a47ee4761b4a0e1c7cc`; Tasks 1–5 have been implemented and reviewed. Task 6 remains under review and its performance and full-workspace acceptance are open.

The shared `ImagePresentation` path supports independent fitted/versioned Kitty resources, feature-aware animation and supported-format pacing. Existing low-level picture IDs remain available. Manual Driver loops must acknowledge successful frame writes and use `finish_output()` to retain final cleanup resources; see the [image guide](../guide/intuituive/plugins.md#images-art). Source frames are scoped to the document and applied viewport; stale buffered frames are discarded before navigation is published.

## Native observations

The following bounded captures use immutable Noctiluca source `b6ed966ebb0fb2835610b9c6e5632bec0e087187`, binary SHA-256 `6163021047d97639283482366221cc367a59dbf7863dacc7778f8cc44457dbf6`, Debian 13, Xvfb 21.1.16 and sandboxed Chromium 156.0.8078.79. They predate the later legacy-ID wrap fix at `872504a` and are not relabeled as that build. They are qualitative native-terminal observations, not latency or frame-rate measurements.

| Terminal and transport | Observed | Limits |
|---|---|---|
| Kitty 0.41.1 direct, Kitty graphics, 80×24 at 8×16 cells | Video and readable chat, product images and Actions overlay; initial product, scroll return and overlay dismissal screenshots match byte for byte. [Video/chat](../media/noctiluca-media-quality/video-chat-initial.png), [product](../media/noctiluca-media-quality/product-grid-initial.png), [overlay](../media/noctiluca-media-quality/product-grid-overlay.png). | The right side of the gauge scene is clipped at this width. |
| Kitty direct, 120×40 at 10×20 and 160×50 at 12×24 | Video/chat, gauges and product images visible; product initial/up/dismiss screenshots match at each size. | At 160 columns the product page fits, so PageDown did not test scrolling. |
| WezTerm 20240203 direct, automatic iTerm-compatible graphics, 120×40 | Product images and headings visible; Actions dismissal returns to the initial screenshot. | WezTerm is not the iTerm2 application; product scrolling was not exercised. |
| XTerm 398 direct, Sixel, 120×40 | Stream video, readable chat and message/Send controls visible. | Dynamic capture does not establish frame rate. |
| Kitty through local tmux 3.5a, auto Cells, 120×39 | Cell-rendered video, readable chat and Actions overlay observed. | Earlier source `03d83e7`; passthrough disabled, SSH not exercised. |

The full corpus, lazy reveal, navigation away/back, Ghostty, actual macOS iTerm2 and SSH remain unverified. A local real-site model read on `b6ed966` reached Speedtest, received an Amazon page titled `500 - Amazon.com` with an unsupported-client/session message, and reached a Twitch shell without proven video/chat playback. These reads do not establish native terminal usability or HTTP status.

## Remaining gates

Matched release-profile timings and their numeric acceptance, remaining full-workspace checks, Task 6 review and whole-branch review are open. Fresh Rust 1.91 workspace clippy failed on existing faithful-rich `nonminimal_bool` diagnostics. A full workspace test stopped at an adaptive test whose headless Kitty capability assumption has since been corrected; the unrun remainder is not a pass. Keep unavailable matrix rows and performance limitations explicit, and do not merge or publish implicitly.
