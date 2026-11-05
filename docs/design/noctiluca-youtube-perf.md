# Noctiluca on YouTube: a measured audit of speed, video, sound and layout

**Status:** an audit, a ranked plan and the first three fixes,
2026-11-05, on `main` at `b0bfdc4c` (Noctiluca 0.0.4, the 0.0.25 cohort).
The fixes are in [What was fixed](#what-was-fixed), with before and after
numbers; everything under [The plan](#the-plan) is still to do.

The maintainer's report, verbatim: "a terrible experience", "the layout
is still terrible", "videos are not processing well at all", "extremely
slow frame rate", "no sound is playing either", "ideally we need to make
it much more responsive", and "is it because we're using Chrome?"

The answers, in short:

- **Frame rate.** Video drew 12 frames a second in Kitty at 120x40 and
  9.5 at 200x60, for three reasons in our pipeline, not Chrome's: every
  frame went to the terminal as uncompressed base64 RGB (390 kB a frame,
  15 MB a second), the page was read again three to four times a second
  while a player's clock ticked (each reading holds up the frames, as they
  share the tab's thread), and pictures were sent again on every scroll.
  Fixed: 24 frames a second at both sizes, a quarter of the bytes a frame.
- **Is it Chrome?** Partly, and only at the top. Chrome decodes the video
  at full rate (2 to 13 dropped frames in 390) and paints its screencast at
  29 frames a second; YouTube's bot wall is the network's, not headless
  Chrome's (headful Chrome in Xvfb got the same wall). What Chrome does
  cost is the screencast itself: a JPEG of the whole viewport for each
  frame, 60 to 117% of a core, which caps us near 25 frames a second and
  grows with the window. That is the next item, not a reason to leave
  Chrome.
- **Sound.** Off by default, by design (`--mute-audio` unless `--audio`),
  and the status bar says `sound off (--audio)`. With `--audio`, headless
  Chrome opens the system's audio device itself (measured: it opened ALSA's
  `default`); whether that reaches the speakers is the maintainer's
  machine's question (below). Nothing is lost in our pipeline: there is no
  audio path in Noctiluca to lose it.
- **Layout.** Eleven concrete defects on the search and watch pages, each
  mapped to a generic rule, in [Layout](#layout).
- **Responsiveness.** Keys answer in 3 to 30 ms on the app's thread; what
  made scrolling heavy was the 1.1 MB of pictures each row of scroll sent
  to Kitty (now 27 kB). A reading of YouTube's search page costs 250 to
  350 ms on the tab's thread (DOMSnapshot 150 to 180, the AX tree 110 to
  130, our model 20 to 30), which now waits three times its cost between
  pinged readings.

## How it was measured

- **The binary:** a release build (`cargo build --release -p noctiluca`)
  on a 4-core Linux container, Chromium 153 headless (`--headless=new`),
  driven by a Python PTY harness: it sets the window size, answers the
  terminal's queries, sends keys, records every byte with its time, counts
  the bytes inside graphics escapes, samples the CPU of Noctiluca and of
  Chrome's processes from `/proc`, and renders the final screen with
  `pyte`. Key latency is the time from a key to the last text-carrying
  output before a 300 ms quiet spell.
- **The trace:** `NOCTILUCA_TRACE=FILE` (added with this note, kept)
  writes JSON lines: each reading (snapshot, AX tree and build times, node
  and box counts, why it ran), each projection, each video frame (screencast
  or capture, its size), each encode, and once a second the painter's
  counters (frames, bytes, draw, paint and write time).
- **Raw CDP:** a probe that opens a page over DevTools and measures the
  same calls without Noctiluca (DOMSnapshot and AX sizes and times, the
  page's mutation rate, `Performance.getMetrics`, the screencast's rate,
  media state and the `Media` domain's player properties).
- **Pages:** YouTube's home, a search (`big buck bunny`) and a watch page
  (live); a **watch-page fixture** for video, since YouTube's player shows
  "Sign in to confirm you're not a bot" from this machine: a local page
  with a 1280x720, 30 fps H.264 test card and a tone, a player whose clock
  and progress bar a script updates on `timeupdate` (as a site's player
  does), a title, comments and a column of related videos; and **Twitch**'s
  always-on live channel (`astronomiemuseum`, 640x360 H.264), a real
  site's player playing.
- **Sizes and protocols:** 120x40 and 200x60 cells; Kitty, Sixel and half
  blocks, forced with `--images`. iTerm2 was not measured (no emulator
  here; it sends a PNG per frame, as Kitty's stills do).

Caveat: the harness reads the PTY as fast as Python can. A real terminal
parses the base64 and decodes the pixels, so it takes bytes more slowly:
every byte figure here costs more on the maintainer's screen than here.

## The measurements

### Reading a page (CDP capture and model)

| Page | DOM nodes | Boxes | DOMSnapshot | AX tree | Model build | Projection (app thread) |
|---|---|---|---|---|---|---|
| YouTube home (logged out: "Try searching to get started") | 3,272 | 340 | 200–320 ms, 1.24 MB | 22–36 ms, 89 kB | — | 0.2 ms |
| YouTube search | 16,760 | 3,530 | 150–180 ms (330–450 ms with Python's parse), 2.0 MB | 110–140 ms, 0.82 MB | 20–30 ms | 8 ms (20 ms max) |
| YouTube search at 200x60 | 17,900 | — | 337 ms | (in that) | 27 ms | 10 ms (25 ms max) |
| YouTube watch page (bot wall) | 8,844 | 1,397 | 346 ms, 2.1 MB | 75 ms, 0.31 MB | — | 2.6 ms |
| Twitch channel | 1,040 | 580 | 64–247 ms | 39–500 ms | — | — |
| Watch-page fixture | 256 | — | 36 ms | (in that) | 1 ms | 2.3 ms |

The first content of the search page was read 3.1 s after start; the
readings while it loaded took up to 0.8 s (1.5 s on the watch page).

How often the page pinged a change (MutationObserver, 5 s): YouTube's
search and home, 3 records (quiet); Twitch, 418 records in 275 batches
(rspack chunks, class flips, the viewer count); the fixture's player, its
clock and bar at about 4 Hz. Every ping became a full reading 100 ms later.

### Scrolling (YouTube search, after the page settled)

| Case | Bytes a row (`j`/`k`) | Bytes a page (`PgDn`) | Settle | CPU idle (Noctiluca / Chrome) |
|---|---|---|---|---|
| Kitty 120x40, before | 1,096 kB | 996 kB | ≤ 27 ms | 7% / 7% |
| Kitty 120x40, after | **27 kB** | 769 kB | ≤ 18 ms | 5% / 2% |
| Kitty 200x60, before | 1,206 kB | 549 kB | ≤ 21 ms | 13% / 49% |
| Kitty 200x60, after | **47 kB** | 700 kB | ≤ 6 ms | 5% / 3% |
| Sixel 120x40 (unchanged) | 435 kB | 413 kB | ≤ 29 ms | 5% / 4% |
| Half blocks 120x40 | 7 kB | 54 kB | ≤ 8 ms | 5% / 4% |

Before, each scroll of one row sent 24 thumbnails to Kitty again as PNG
(60 transmissions of 24 images in one run, up to 10 of one image). A page
down still sends the pictures newly in view, which is what it should send.
Sixel and iTerm2 draw over the cells, so a picture that moves must be
drawn again; that is item 6 of the plan.

### Video (15 s of playback, after 10 s of warm-up)

Frames: screencast frames handed to the app; drawn: frames encoded and
painted (the byte budget's pacing drops the rest). Reads: readings of the
page a second, their cost, and the share of the tab's thread they took.

| Case | Frames/s | Drawn/s | Encode | MB/s to the terminal | Reads/s | Read | Tab reading | Paint ms/s | Write ms/s | CPU Noctiluca / Chrome |
|---|---|---|---|---|---|---|---|---|---|---|
| Fixture, Kitty 120x40, before | 15.1 | 12.3 | 8.6 ms | 15.2 | 3.7 | 97 ms | 36% | 22 | 367 | 35% / 60% |
| Fixture, Kitty 120x40, **after** | 24.7 | **23.9** | 6.9 ms | **7.9** | 0.9 | 87 ms | 8% | 16 | 185 | 54% / 67% |
| Fixture, Kitty 200x60, before | 12.5 | 9.5 | 22.9 ms | 11.9 | 3.8 | 108 ms | 41% | 21 | 282 | 52% / 99% |
| Fixture, Kitty 200x60, **after** | 24.3 | **22.6** | 18.1 ms | **5.2** | 0.9 | 91 ms | 8% | 20 | 119 | 104% / 117% |
| Fixture, Sixel 120x40, before | 15.6 | 12.8 | 18.3 ms | 1.3 | 3.7 | 104 ms | 39% | 283 | 65 | 59% / 63% |
| Fixture, Sixel 120x40, **after** | 24.9 | **22.9** | 11.2 ms | 2.4 | 0.9 | 86 ms | 8% | 327 | 58 | 95% / 66% |
| Fixture, half blocks 120x40, before | 18.5 | 18.5 | — | 0.3 | 3.8 | 90 ms | 34% | 19 | 24 | 17% / 43% |
| Fixture, half blocks 120x40, **after** | 25.8 | **25.8** | — | 0.4 | 0.9 | 83 ms | 8% | 19 | 16 | 18% / 45% |
| Twitch, Kitty 120x40, before | 20.0 | 18.7 | 7.9 ms | 8.0 | 1.0 | 224 ms | 22% | 13 | 191 | 43% / 65% |
| Twitch, Kitty 120x40, **after** | 21.0 | **20.6** | 9.2 ms | **5.7** | 0.9 | 215 ms | 19% | 11 | 127 | 50% / 70% |
| Twitch, half blocks 120x40, before | 16.1 | 16.1 | — | 0.18 | 1.6 | 241 ms | 39% | 11 | 4 | 15% / 65% |

Bytes a Kitty frame: 391 kB before (raw RGB in base64, the region's
440x224 pixels), 100 kB after on the test card and 276 kB on Twitch's
picture (zlib at its fastest: a test card shrinks fourfold, a camera's
picture by half).

The same fixture with its player's clock script off (no pings, no
readings) gave 21.5 frames and 15.2 drawn a second before the fixes, against
15.1 and 12.3 with it: the readings alone cost a third of the frames.

### Chrome's side

| Measure | Value |
|---|---|
| Video decoding (fixture 720p, Twitch 360p) | FFmpeg software decoders; 2 dropped of 391 frames (fixture), 13 of 319 (Twitch) |
| Screencast, as fast as acknowledged (raw CDP, 960x608) | 29.4 frames a second, JPEG quality 70, 78 kB a frame |
| Screencast as Noctiluca takes it (on demand, one frame ahead) | median 40 ms between frames (p90 63 ms) at full viewport; 33 ms at a quarter scale (half blocks) |
| Taking a frame (JPEG decode of the whole viewport in rs-rich-embed) | 3.8–7 ms at 960x648, 12–16.5 ms at 1600x1008 |
| Chrome's CPU while a video plays | 43–70% at 120x40, 99–117% at 200x60 (the screencast's encode grows with the viewport) |
| Headless vs headful | Headful Chrome in Xvfb (no "HeadlessChrome" in its user agent) got the same bot wall on the watch page: the wall is the network's |
| GPU | None here (software compositing; Vulkan unavailable) |

## Root causes, by symptom

### "Extremely slow frame rate", "videos are not processing well"

1. **Uncompressed frames to Kitty.** A video frame went to Kitty as raw
   RGB, base64'd: 390 kB a frame, 12 to 15 MB a second at 12 frames. The
   painter's write took 37% of the app's thread here, and a real terminal
   parses slower than this harness, so the measured output rate, and with
   it the byte budget, falls, and the pacing drops frames. **Fixed** (o=z).
2. **Readings stall frames.** A player's clock and progress bar change the
   DOM about four times a second; each change was read 100 ms later with a
   full DOMSnapshot and AX tree, on the same thread that takes frames from
   the screencast. 36 to 41% of that thread went to reading during
   playback, a third of the frames lost. On YouTube's page (a reading is
   250 to 350 ms) it would be worse. **Fixed** (a pinged reading waits three
   times its cost, and a second while a video plays).
3. **The screencast is the whole viewport.** Chrome encodes and we decode
   a JPEG of the entire viewport for every frame, to cut a 440x224 region
   from it. At 200x60 that is 1600x1008 a frame: Chrome at a full core and
   12 to 16 ms of decode a frame. This is the ceiling now (24 to 25 frames
   a second). **Planned** (item 1).
4. **Sixel's painter.** Sixel frames cost 14 ms each on the app's thread in
   the painter (327 ms a second at 23 frames), which competes with keys.
   **Planned** (item 5).

### "No sound is playing"

- Noctiluca starts Chrome with `--mute-audio` unless `--audio` (or
  `[media] audio = true`); the status bar says `sound off (--audio)`.
  This is the documented default ([media](../guide/noctiluca/media.md#sound)).
- With `--audio`, headless Chrome (`--headless=new`) plays through the
  machine's audio output itself: on this machine, with no sound card, it
  opened ALSA's `default` PCM and failed (`PcmOpen: default, No such file
  or directory`), after decoding the AAC track. On a desktop, Chrome's
  PulseAudio/PipeWire or CoreAudio output is the same path a normal Chrome
  uses. **Needs the maintainer's machine** to confirm sound plays there.
- Over SSH (`--ssh`, or Noctiluca itself on a remote host) the sound plays
  on the machine Chrome runs on, never the terminal's.
- YouTube itself mutes autoplay without a gesture; `p` plays as the user's
  gesture, and `Alt+M` unmutes the element.

### "The layout is still terrible"

See [Layout](#layout): eleven defects on the search and watch pages, all
generic. None is a speed problem; each is a projection rule.

### "Make it much more responsive"

- Keys are handled on the app's thread in 3 to 30 ms; the projection of
  YouTube's search page is 8 ms (25 ms at worst).
- **Scrolling sent every picture again** to Kitty: 1.1 MB a row, which a
  real terminal takes visibly long to parse, and a held key queues.
  **Fixed** (27 kB a row).
- Readings cost 250 to 350 ms on YouTube (16,800 nodes): an action of ours
  (click, type) still reads at once and waits behind a reading in
  progress; pinged readings now wait three times their cost.
- Sixel and iTerm2 redraw every picture on screen at each scroll step
  (435 kB a row with Sixel). **Planned** (item 6).

## Is it Chrome?

No, not for what the maintainer saw; and Chrome is the right engine to
keep. The evidence:

- Chrome decodes the video at its full rate with almost no dropped frames,
  and its screencast runs at 29 frames a second when taken as fast as it
  comes. Our pipeline drew 12.
- The three causes fixed here are ours, and fixing them doubled the frame
  rate without touching Chrome.
- The bot wall on YouTube's watch page is the network's: headful Chrome
  gets it too.
- What Chrome does cost is the screencast's full-viewport JPEG (60 to
  117% of a core) and the size of a DOMSnapshot of a large page (150 to
  180 ms for 16,800 nodes). Both are reduced by asking Chrome for less
  (items 1, 3 and 4 below), not by another engine: another engine would
  need the same layout, the same codecs (YouTube needs H.264/VP9/AV1 and
  MSE) and the same sandbox.

## What was fixed

Each with a test that fails without it, and measured again with the same
harness (the tables above: "after").

1. **Kitty pictures are not sent again when they move** (rs-rich-ext's
   `frame::plan`): a graphic that lives in its cells (Kitty's Unicode
   placeholders) and only moved is shown by its cells, which the cell diff
   writes; only a new picture or a new frame is sent. Scrolling YouTube's
   search: 1,096 kB a row to 27 kB (120x40), 1,206 kB to 47 kB (200x60).
   Test: `graphics_in_cells_that_move_are_not_sent_again`.
2. **A page that keeps changing is read at most a quarter of the time**
   (Noctiluca's tab): a reading the page pinged for waits three times the
   last reading's cost, and a second while a video plays on screen.
   Readings for an action, a navigation or a load never wait. The
   fixture's readings went from 3.7 to 0.9 a second, the tab's time spent
   reading from 36% to 8%. Tests:
   `a_page_that_keeps_changing_is_read_at_most_a_quarter_of_the_time`
   (an engine whose snapshot takes 120 ms and that pings at every look),
   `pinged_readings_wait_for_their_cost_and_for_playing_video`.
3. **Kitty video frames are zlib-compressed** (rs-rich-art's `Picture`,
   `o=z` at its fastest level): 391 kB a frame to 100 kB (test card) and
   276 kB (Twitch). Test: `encodes_each_protocol_and_rows_shown` in rs-rich-art's
   picture tests checks `o=z` and that a flat 64x64 frame takes under a tenth of its
   raw size.

Together: Kitty at 120x40 draws 23.9 frames a second (12.3 before), at
200x60 22.6 (9.5), Sixel 22.9 (12.8), half blocks 25.8 (18.5), Twitch in
Kitty 20.6 (18.7), with half the bytes a second or less.

And the trace (`NOCTILUCA_TRACE`), kept for the next round.

## Layout

Compared at the same viewport (120x40 cells, 960x648 CSS pixels): Chrome's
`Page.captureScreenshot` against Noctiluca's projection. Each defect is
stated as the generic rule that would fix it; none needs to know YouTube.

| # | Where | What Chrome shows | What Noctiluca draws | Generic rule |
|---|---|---|---|---|
| L1 | Header | A skip link above the page (`top: -1000px`), shown only when focused | `[Skip navigation]` at the top left | A control wholly above or left of the page is not drawn (as text already is), unless focused |
| L2 | Header | Logo, search box, voice search, settings and sign-in on one 56 px row | Spread over three rows: buttons on the first, the search box on the second, the logo on the third | Items of a fixed or sticky bar of at most four rows that share a vertical band are placed on its middle row |
| L3 | Header | A text box with a clear button | `Search ‹big buck bunny ▾›`, drawn as a select | A `combobox` that is an `<input>` is drawn as a text field with a list, not a select |
| L4 | Header | A microphone icon (voice search) | `[🔍]`, the search glyph, beside the search button's `[🔍]` | An icon button's glyph comes from the first word of its name that names an icon ("voice" → 🎤), not any word ("Search") |
| L5 | Mini guide | Icons with 10 px labels (Home, Shorts, Subscriptions, You) | `Subscriptio` / `ns`: a word broken mid-word in a 9-cell column | Text too wide for its box in cells is cut with `…` (or allowed into empty cells beside it), never broken inside a word |
| L6 | Results | `23M views • 11y ago` on one line | `23M` and `11y ago` on two lines; `9M 18y` / `ago` | Inline runs of one line in Chrome stay on one row when they fit the box's cells; a separator (`•`) kept |
| L7 | Results | The channel avatar beside the channel name | The avatar over the second half of the date (`▀▀ ago`) | A small picture in a line takes cells of its own, before the next run |
| L8 | Results | A `⋮` menu at each result's right edge | `[Action menu` cut by the scroll bar, and `[Actio…` on another | Right-edge controls are shortened to fit (0.0.25's rule) in every row, with the scroll bar's column counted |
| L9 | Watch page | `20M` on the like button | `[like this video along with 20,128,674 othe…]` | A button with visible text is faced with that text; its `aria-label` is its accessible name, read by the overlay and `--dump ax` |
| L10 | Watch page | `28K views` under a related video | `28` and `K` on two rows | As L5: a number and its unit are not split |
| L11 | Search, watch | Thumbnails 360x202 CSS pixels | 45x11 cells each: faithful, and 43% of a 120-column screen | A reader preference, not a defect: a `[media] thumbnails` size cap for result lists (rows of alike items with a picture and text), off by default |

## The plan

Ranked by expected gain for the cost. Each is generic.

| # | Change | Symptom | Expected gain | Cost |
|---|---|---|---|---|
| 1 | **Screencast only what plays.** While a video plays, set Chrome's device metrics so its viewport is the region's box (or clip by scrolling the player into the top of a viewport sized to it), or capture the `<video>` in-page: draw it into an `OffscreenCanvas` at the region's pixel size and send the RGB over a binding, or `VideoFrame` + `copyTo` (WebCodecs). The screencast's JPEG and our decode shrink with the region: from 1600x1008 to 440x224 a frame | Frame rate at large sizes, Chrome's CPU | Chrome's screencast cost ÷ 3 to 15; 30 frames a second at 200x60; frees a core | Medium. The in-page path needs the page's world (the isolated world can read the video element; cross-origin video taints the canvas, so the screencast stays the fallback) |
| 2 | **Kitty frames through shared memory or a temporary file** (`t=s`, `t=t`) when the terminal is on this machine (not over SSH) and answers the query (`a=q,t=s`): about 100 bytes a frame on the PTY instead of 100 to 280 kB | Frame rate, write time on the app's thread | Write time ÷ 1000 locally; full frame rate in any Kitty-protocol terminal on the machine | Medium (rs-rich-art: a query, the shm object's lifetime; Kitty and Ghostty support `t=s`, WezTerm `t=f`) |
| 3 | **Read off the frame thread.** Split the tab's engine use: readings (DOMSnapshot, AX) on one thread, frames and captures on another, over the same DevTools connection (it is already shared by a reader thread). A reading then never delays a frame | Frame rate during page changes, responsiveness of actions | The remaining 8% of frames lost to readings; actions no longer wait behind a reading | Medium-high (the `Engine` trait is `&mut`; split into a reading handle and a media handle) |
| 4 | **Cheaper readings of large pages.** Fetch the AX tree only for the nodes the model asks names of (`Accessibility.queryAXTree`/`getPartialAXTree`), or reuse the last one when the DOMSnapshot's strings and node count are unchanged; cache the model's per-node work by the node's revision | Responsiveness on YouTube (250–350 ms a reading) | AX is 40% of a reading on YouTube's search page: about 120 ms of 300 | Medium |
| 5 | **Sixel's painter off the app's thread.** The Sixel frame's escape is made on the encoder's thread, but the painter still spends 14 ms a frame on it (327 ms a second at 23 frames): find what it does (the placement scan over the frame, or copying the escape) and move it out | Responsiveness while a video plays in Sixel | About a third of the app's thread back | Low-medium (profile first) |
| 6 | **Sixel and iTerm2 pictures while scrolling.** Drawing a picture over cells means every move sends it again (435 kB a row). While the view moves, show the pictures' cells (half blocks) and draw the pixels once the view has rested 150 ms | Responsiveness of scrolling in Sixel/iTerm2 | Scrolling as cheap as half blocks; a picture appears a moment after the view rests | Low |
| 7 | **Layout L1, L5, L10** (off-page controls, words split, numbers split) | Layout | Visible on every YouTube page; L5 on any narrow column | Low each, in the projection |
| 8 | **Layout L2, L6, L7, L8** (header bands, inline runs on one row, small pictures in lines, right-edge controls) | Layout | The header and every result row | Medium |
| 9 | **Layout L3, L4, L9** (combobox input, icon glyphs, button faces) | Layout | Header and action buttons | Low |
| 10 | **Sound on by default on this machine.** Today sound is off unless `--audio`. A run on this machine (not over SSH) could start unmuted and keep pages quiet until the user plays something (Chrome's autoplay policy already blocks unmuted autoplay without a gesture), with `Alt+M` the switch. A product decision: the maintainer's | Sound | Sound where people expect it | Low in code; needs the decision |
| 11 | **Sound to the terminal's machine.** Over SSH, or with Chrome elsewhere, capture the tab's audio in the page (`captureStream()` into an `AudioWorklet`, PCM over a binding) and play it with intuiTUIve's `audio`/`pcm` features, timed to the frames' clock | Sound over SSH | Sound wherever the terminal is | High (latency and sync; DRM'd media never captures) |
| 12 | **Thumbnails' size cap** (L11) | Layout | Two or three results a screen instead of one and a half | Low, an opt-in |

## What needs the maintainer's machine

- **Sound with `--audio`:** whether headless Chrome's output reaches the
  speakers on the maintainer's OS (PulseAudio or PipeWire on Linux,
  CoreAudio on macOS). Here there is no sound card: Chrome tried ALSA's
  `default` and failed, which is the path a desktop routes to the sound
  server.
- **The terminal:** which emulator and protocol (`PROJECTION + KITTY`,
  `SIXEL`, `CELLS` in the header), and how fast it takes output. Run with
  `NOCTILUCA_TRACE=/tmp/n.jsonl`, play a video for 20 seconds, and the
  trace's `paint` lines give its frames, bytes and write time a second.
- **YouTube's player:** this machine gets the bot wall, so YouTube's own
  player (MSE, its clock, its ads) was replaced by the fixture and Twitch.
  On the maintainer's network a watch page plays; the trace there says
  how often it is read and what each frame costs.
