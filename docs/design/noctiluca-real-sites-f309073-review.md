# Noctiluca real-site review — f309073

**Build:** `f3090737be9d569fd29b0260ba18148b65caa57b`, CLI SHA-256 `047dbbb26204b43c9a6fc0b30192d73b0dd7c5323f3fd9923c338cfeac11ede4`

**Terminal:** Kitty 0.41.1, 120×40 cells. Eight public sites, separate anonymous browser profiles, five captures each: initial, scroll down, scroll back, F1 actions, Escape dismissal. All 40 images were visually inspected. Cleanup passed with no live owned process, X socket, or temporary browser root.
**Evidence:** the raw captures and their manifest were not kept in the repository. The nine contact sheets #878 committed (an overview and one per site, each with the initial, scroll down, scroll back, F1 actions and Escape dismissal states) were removed from the tree for size and third-party-content reasons: they showed live third-party pages, with those sites' branding, images and text. They remain in git history at `da011270` under `docs/design/evidence/noctiluca-real-sites-f309073/`. What each showed:

- **Overview:** the initial capture of all eight sites side by side, as a 4×2 grid.
- **BBC News:** a lead image and headline in a narrow left column beside a separate story column, empty space between them; scrolled down, a readable multi-column list of image stories; F1 opens the actions list over the page and Escape restores the first view.
- **Wikipedia (Coral reef):** the opening view is mostly a gray image placeholder and empty space around the title, contents and language links; scrolled down, the article body reads as dense linked text.
- **YouTube:** every state shows Chromium's "This site can't be reached" page with `ERR_TUNNEL_CONNECTION_FAILED`; the actions overlay opens over it.
- **Amazon:** every state shows the logo and a one-line "we do not support your session or client" message, under the page title `500 - Amazon.com`.
- **Twitch:** stream thumbnails with `LIVE` labels and viewer counts, whose Play/Mute/Settings buttons overlap neighbouring content; the status bar reads `media PAUSED 0:00`; scrolled down, category cover images with their captions.
- **Giphy:** three GIF tiles load in the first row while the rest stay gray placeholders, and captions at the right edge are clipped; after scrolling back, every tile is a placeholder.
- **Speedtest:** the privacy consent dialog, its long text and its three buttons (more options, Reject All, I Accept) fill the viewport, with site controls clipped at both sides.
- **MDN (`<video>` reference):** the opening navigation and table of contents are crowded, and the demo's code line is clipped at the right edge; scrolled down, the attribute reference reads cleanly.

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
