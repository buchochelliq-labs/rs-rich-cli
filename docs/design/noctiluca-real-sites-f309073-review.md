# Noctiluca real-site review — f309073

**Build:** `f3090737be9d569fd29b0260ba18148b65caa57b`, CLI SHA-256 `047dbbb26204b43c9a6fc0b30192d73b0dd7c5323f3fd9923c338cfeac11ede4`

**Terminal:** Kitty 0.41.1, 120×40 cells. Eight public sites, separate anonymous browser profiles, five captures each: initial, scroll down, scroll back, F1 actions, Escape dismissal. All 40 images were visually inspected. Cleanup passed with no live owned process, X socket, or temporary browser root.
**Evidence:** the raw captures and their manifest were not kept in the repository. Committed contact sheets: [overview](evidence/noctiluca-real-sites-f309073/review-overview.jpg), [BBC](evidence/noctiluca-real-sites-f309073/bbc-review.jpg), [Wikipedia](evidence/noctiluca-real-sites-f309073/wikipedia-review.jpg), [YouTube](evidence/noctiluca-real-sites-f309073/youtube-review.jpg), [Amazon](evidence/noctiluca-real-sites-f309073/amazon-review.jpg), [Twitch](evidence/noctiluca-real-sites-f309073/twitch-review.jpg), [Giphy](evidence/noctiluca-real-sites-f309073/giphy-review.jpg), [Speedtest](evidence/noctiluca-real-sites-f309073/speedtest-review.jpg), [MDN](evidence/noctiluca-real-sites-f309073/mdn-review.jpg).

| Site | Current observation |
|---|---|
| BBC News | Lead image and stories load. Opening content remains sparse and split across an awkward narrow left column and right-side story column; scrolling reveals a more readable multi-column story list. |
| Wikipedia | Article body is readable after scrolling. Opening viewport has a large gray image placeholder and substantial empty space around the title/contents. |
| YouTube | Chromium reports `ERR_TUNNEL_CONNECTION_FAILED`; the site itself could not be evaluated. |
| Amazon | Amazon displays an unsupported-session/client page. This is not evidence of an HTTP 500 response or a product-page test. |
| Twitch | Video controls and labels overlap nearby content; the player reports `PAUSED 0:00`, and chat/playback were not activated. Stream cards and chat-like content are visible, but live video/chat behavior remains unverified. |
| Giphy | Several media tiles remain gray placeholders. Captions are mostly tied to their tiles, but long content at the right edge is clipped and the four-column results remain hard to scan. |
| Speedtest | The privacy dialog dominates the viewport; its text and buttons are visible, with some underlying content clipped at the sides. No choice was made and no test was started. |
| MDN | Article content becomes readable after scrolling, while the opening navigation is crowded and a code line clips at the right edge. |

F1 opens the Noctiluca actions overlay and Escape restores the page on all eight sites. The capture run checks layout and cleanup only: it does not measure judder, frame rate, latency, or video smoothness. It did not interact with site consent, login, purchases, chat, playback, or speed-test controls. The live results demonstrate that the branch's generic regression tests are not a claim that every site issue is fixed; BBC/Giphy/Twitch/Speedtest/MDN still need follow-up work, and YouTube/Amazon remain blocked by the observed environment/client behavior.

The supplied initial, scroll, overlay, and dismissal screenshots are qualitative snapshots from separate live loads, so changing page content prevents a strict before/after pixel comparison with the previous capture run.
