# Noctiluca 0.0.27: video jitter and flicker, Amazon, "the browser has gone"

**Status:** findings and proposals, 2026-11-11, on
`claude/next-priority-items-3c5p8e-wip-noct027` at `69dad1ff` (0.0.27
opened; the same Noctiluca as the 0.0.26 release). No code changed with
this note. It feeds the 0.0.27 plan with the three problems the
maintainer reported after 0.0.26:

1. Video playback jitters a little and flickers (Kitty and iTerm2 at
   least).
2. Amazon is still not perfect.
3. "The browser is reported as lost" at times.

## How it was measured

- **Binary:** a debug build of `noctiluca` (rs-rich-art is optimised in
  dev builds, `aca95eac`). The disk was too small for a release build.
  Debug costs show in Noctiluca's CPU (108 to 160% of a core against 37
  to 80% in the [perf note](noctiluca-youtube-perf.md)'s release runs),
  in a screencast take of 25 ms (decoding the JPEG on the tab's thread)
  and in readings of 150 to 230 ms (155 ms in release). **The patterns
  below (when frames stop, what is written in what order) do not depend
  on the build; the absolute numbers do.** Re-measure them on a release
  build before quoting them as release figures.
- **Host:** a 4-core Linux container, headless Chromium 153 (Playwright's
  build) behind a no-sandbox wrapper. Load average 0.5 to 0.8, except in
  the stress run (below).
- **Harness:** the perf note's PTY harness (outside the repository), at
  120×40 with 8×16-pixel cells. It answers the terminal queries, records
  every read with its time, and keeps `NOCTILUCA_TRACE`'s JSON lines.
  Changes for this pass:
  - it answers the device-attributes query **last**, as terminals do.
    Answered first, the session's start-up query stops at it and finds
    no synchronized output and no kitty keyboard (`rich-interact` `session.rs:850`). So
    every earlier harness run drew without `CSI ? 2026`;
  - it keeps each read's time and length, so a byte offset in the
    capture maps to a time;
  - it has a step that kills Chrome's browser process or its renderers.
- **Analysis:** a script over the raw capture finds, with their times:
  - Kitty uploads (`a=T`), deletes (`a=d`) and placeholder cells, with
    the image id in each placeholder's colour;
  - iTerm2 (`1337;File`) and Sixel (DCS `q`) images, and the erases
    before them;
  - synchronized-update brackets.

  The tab's `frame`, `read`, `cast`, `capture` and `area` events and the
  app's `frame_done` and `encode` events come from the trace. Intervals
  are measured from 10 s to 33.5 s of each run, while the video plays
  and before the harness stops reading.
- **Page:** the perf note's watch-page fixture. It has a 30 fps H.264
  test card in a player whose clock and progress bar a script updates on
  `timeupdate`, a title, comments and related videos.
- **pyte:** it does not implement `CSI S` / `CSI T` (scroll up and down).
  Noctiluca scrolls with `CSI t;b r` and `CSI n S`, so any pyte screen
  taken after a scroll shows stale rows. Screens taken after a scroll in
  this pass and in earlier harness runs (the perf note's, 0.0.26's
  real-site checks) are not evidence of a Noctiluca fault unless the raw
  bytes say so.

## 1. Video: jitter and flicker

### What was measured

Steady playback on the fixture, 23.5 s windows. "Tab" is a frame handed
to the app (trace `frame`). "Shown" is a frame on the terminal: a Kitty
upload of the video's geometry, or an iTerm2 or Sixel image of over
20 kB, from the raw capture. Synchronized output is on in every row but
the first.

| Run | Interval p50 / p95 / max (ms) | stdev (ms) | ≥ 60 ms | ≥ 200 ms |
|---|---|---|---|---|
| Kitty, no sync (shown) | 34.1 / 120.6 / 2515 | 143 | 47 of 353 | 12 |
| Kitty (tab) | 34.2 / 82.7 / 239 | 36.6 | 56 of 533 | 19 |
| Kitty (shown) | 34.4 / 86.3 / 265 | 38.1 | 64 of 518 | 19 |
| iTerm2 (tab) | 33.8 / 82.2 / 250 | 35.7 | 57 of 540 | 16 |
| iTerm2 (shown) | 56.6 / 150.7 / 275 | 34.2 | 133 of 352 | 9 |
| Sixel (tab) | 33.9 / 79.7 / 237 | 35.8 | 58 of 543 | 17 |
| Sixel (shown) | 65.3 / 168.9 / 257 | 34.3 | 224 of 317 | 8 |
| Half blocks (tab) | 33.4 / 67.3 / 241 | 34.4 | 41 of 560 | 16 |
| Half blocks (app `frame_done`) | 33.4 / 81.8 / 243 | 35.7 | 53 of 560 | 17 |
| Kitty, 3 busy cores + a reload every ~3 s (shown) | 48.8 / 282 / 1336 | 144 | 327 of 919 | 86 |

| Scroll case (Kitty) | Result |
|---|---|
| One `j`, then `k` | The screencast area was dropped. Frames came by `Page.captureScreenshot` (55 to 90 ms each, on the tab's thread) at **4 to 5 a second** for 5 s. |
| `j j j k k k` | The same, for **20 s**. Gaps of 165 to 465 ms, p50 183 ms. |

How often a frame's region is blanked or deleted before its next frame
is drawn:

- **Kitty, every frame (737 of 738).** Each frame is a new image id. The
  painter writes the region's placeholder cells under the new id first,
  then uploads the new image, then deletes the old id
  (`P336 T336 D335`).
  - Without synchronized output, the cells name an image the terminal
    does not have yet for the whole upload. The upload spanned 25 PTY
    reads (p50), about 100 kB compressed.
  - With synchronized output, the upload is inside the same `2026` block
    (0 of 738 outside), so the terminal shows it whole.
- **iTerm2 and Sixel, every frame (502 of 502 and 451 of 451).** Each
  frame writes the region's half-block cells (about 55×14 cells), then
  erases them (`ESC 7`, then `ESC[55X` on each row, then `ESC 8`), then
  draws the image. That is three states of the same cells in one frame.
- **Layout:** the region's top left stayed at (4, 3) in all 738 Kitty
  frames and all 502 iTerm2 frames. No layout shift moved it while the
  video played unscrolled.
- **Screencast restarts:** one at the start of each run, then none while
  it plays unscrolled. There is one per reload (24 in the stress run),
  and the scroll cases above drop the area.

### Causes of jitter

1. **Readings stop the frames, about once a second.** Every interval of
   200 ms or more in the steady runs had a reading inside it:
   - Kitty 25 of 25, iTerm2 24, Sixel 24, half blocks 26;
   - 35 readings in 35 s, 150 to 230 ms each in this build (155 ms in
     release, as the perf note measured);
   - the interval after a reading is long too: 34 of the 42 non-reading
     intervals over 55 ms held the `page_apply` that follows a reading.

   The model read runs on the tab's thread (`tab.rs:2137`). While a ping
   waits, `ping_read_gap` (`tab.rs:93`) holds the next one for at least
   a second while the screencast runs. The result is a regular stutter
   of about a fifth of a second every second. This is the perf note's
   "What is left" item 1.
2. **Frames bunch after a stall.** `stamp_every` (`video.rs:222`) keeps
   a region's frames on fixed slots and accepts a frame up to a quarter
   of a slot early. After a reading, the next frames come 20 to 30 ms
   apart: 96 of the 533 Kitty intervals fell in 20 to 29 ms against 322
   in 30 to 39. The frame after a freeze is shown briefly, then the
   cadence resumes.
3. **The tab samples on its own clock, not the video's.** Slots are
   `MIN_INTERVAL` = 1000/30 = 33 ms apart (`video.rs:52`). The fixture
   is 30 fps (33.3 ms): the phase slides about one source frame every
   100 frames, so one frame in about 3.3 s is held or skipped. A 24 or
   25 fps video (most of YouTube) sampled at 30 Hz is held on every
   fourth or fifth slot: a 1-1-1-2 cadence, visible as judder on a pan.
   The screencast's own frame `timestamp` (`FrameMeta`, `chrome.rs:163`)
   is not used to pace.
4. **iTerm2 and Sixel drop frames unevenly.** The tab hands 540 and 543
   frames to the app; the terminal gets 352 and 317, p50 57 and 65 ms
   apart. 133 and 224 intervals are 60 ms or more (Kitty: 64). The frames
   go out as an irregular mix of one-slot and two-slot holds. Two drops
   happen on the way:
   - the app's mailbox and the `Pacer`: 540 offered, 440 applied;
   - the encoder's queue, which keeps the newest: 440 applied, 353
     encoded.

   `whole_rate` (`app.rs:1074`) rounds the pacer's interval to a whole
   frame rate, but the tab still cut 30 frames a second. In this build
   the app's thread is the bottleneck. The same pattern appears whenever
   the pacer or the encoder runs below the source rate: over SSH, on a
   slow terminal, or for a big region.
5. **One row of scrolling turns the cast off for seconds.** On `j`:
   - Noctiluca follows with a wheel of 90 CSS px (`follow_scroll`,
     `app.rs:400`), which puts Chrome's viewport top at y = 90;
   - the video's box starts at y = 72, so `screencast_area`
     (`video.rs:258`) finds it partly outside Chrome's viewport and casts
     the whole viewport;
   - `Frames::cut` cannot cut the whole region from that frame, so every
     frame is taken with `capture_model` (`tab.rs:1647`): a screenshot of
     55 to 90 ms, at 4 to 5 frames a second;
   - this holds until Chrome's scroll puts the box back inside (5 s and
     20 s in the two runs).

   The video is wholly visible in Noctiluca's view all along: the two
   views differ by the projection's rows.
6. **Load and navigation:** with three cores busy and a reload every
   ~3 s, p95 rose to 282 ms. Each reload restarts the screencast (area
   to `None` and back), and readings ran 77 to 318 ms.

### Causes of flicker

1. **Kitty without synchronized output (certain, from the bytes):**
   - each frame's placeholder cells switch to the new image id before
     its upload. `draw_graphics` (`rich-intuituive/src/app.rs:7369`) is
     appended after the frame's cells (`app.rs:9280`);
   - a terminal that draws between the cells and the end of the upload
     shows the region empty: Kitty draws nothing for a placeholder whose
     image it does not hold;
   - with `CSI ? 2026` the frame is atomic and this does not show;
   - synchronized output is off whenever the start-up query is not
     answered within `ANSWER_WAIT` (1 s, `session.rs:665`): a slow SSH
     link, a multiplexer that does not answer DECRQM 2026, or
     `RICH_SYNC_OUTPUT=0`.

   Whether the maintainer's Kitty took this path is **not verified**; no
   real Kitty was available.
2. **iTerm2 and Sixel, every frame (certain, from the bytes):**
   - the region's half blocks are repainted with the new frame's colours
     (the picture's tagged cells change every frame);
   - then they are erased (`ESC[nX`, `image.rs:1000`: "clearing before
     native alpha blending", for transparent PNG and Sixel pixels);
   - then the image is drawn;
   - without synchronized output this shows half blocks, then the
     background, then the picture: a flash every frame;
   - with it, it depends on how the terminal applies an inline image
     inside an update. iTerm2 decodes inline images off its main thread,
     so cells erased in the update can show empty until the decode
     lands. This is **not verified** on a real iTerm2;
   - it also costs the half-block bytes of every frame (for a picture
     the terminal will cover) and the painter's CPU for them.
3. **Moving to and from capture mode (scrolling, cause 5 above):** frames
   come from a different capture path, as JPEG screenshots of the region
   at 4 to 5 fps. It reads as stutter rather than a blank.
4. **Not seen:** a picture cleared by a reading (`page_apply` did not
   rewrite the region in any run). Kitty placements are not deleted
   before the new one is uploaded with sync on (the old id is deleted
   after). No layout shift moved the region by a cell while it played.

### Proposed fixes

| # | Fix | Size | How a test shows it |
|---|---|---|---|
| V1 | **Upload in-cell graphics before the cells that name them.** In `draw_graphics`, Kitty in-cell graphics (`Graphic::in_cells`) are written before the frame's cells, without a cursor move (a virtual placement does not move the cursor); deletes stay after. Generic intuiTUIve, no Noctiluca change | S | intuiTUIve driver test, sync off: `picture_of` with a changing frame; in each frame's write, the new id's `a=T` comes before the first placeholder cell coloured with that id, and the old id's `a=d` comes after it. It fails today (`P` before `T`) |
| V2 | **No erase and no half blocks under an opaque live picture in iTerm2 and Sixel.** For an opaque picture (a video frame has no alpha), write the escape over the cells without `ESC[nX`, and keep its cells' content constant across versions (the cells are covered), so the painter does not rewrite them every frame. The erase stays for pictures with alpha | M | Driver test: a live opaque picture under iTerm2 and under Sixel, second and later frames: no `ESC[` *n* `X` and no `▀` in the region's rows, one image escape. A tape or harness run shows the bytes per frame fall by the half-block share |
| V3 | **Read off the tab's thread** (the perf note's item 1): take the snapshot's `ScreencastScope` on the reader and check and adopt it on the tab | L | Harness or ignored test: on the watch fixture, no interval of 200 ms or more lines up with a reading, and p95 is under 50 ms with readings every second |
| V4 | **Keep a playing region inside Chrome's viewport when following our scroll.** In `follow_scroll`, clamp Chrome's target so that the on-screen playing regions (or the one casting) stay inside its viewport, which is taller than ours; or let `screencast_area` cast an area beyond the layout viewport, once checked against Chrome | S to M | Tab test with the fake engine: one `j` on the watch fixture keeps `area` set and makes no `capture`. Live: no `capture` events and no fall in frame rate across `j` / `k` |
| V5 | **Pace from the source.** Take a frame when Chrome's screencast has a new one (on-demand already signals it) and present by its `timestamp`, held to the pacer's cap and rounded to a divisor of the source rate (30, 15, 10 for 30 fps; 24, 12 for 24 fps), instead of fixed 33 ms slots. Accept no frame more than a slot early after a stall (cause 2) | M | `video.rs` unit tests: a 24 fps source gives even 41.7 ms intervals (stdev under 2 ms); a 30 fps source with a 200 ms stall gives no interval under 30 ms after it. Live: stdev of shown intervals |
| V6 | **One drop point, decided at the tab.** When the pacer or encoder keeps fewer frames than the source sends, tell the tab the even rate it can take (V5's divisor) so that the mailbox and the encoder never need to drop | S (after V5) | Harness: iTerm2 and Sixel shown intervals cluster at one value (66.7 ms at 15 fps), not 33/66 mixed; the encoder's coalesced count is near zero |

## 2. Amazon

`curl -sI https://www.amazon.com` from this machine answers
`HTTP/1.1 403 Forbidden` (`text/plain`, 72 bytes), which is the network
policy's refusal, as in the earlier passes. Nothing below is from the
live site. It comes from the shop fixture
(`crates/noctiluca/tests/fixtures/product-grid.html`) opened in Kitty
and half-block modes at 120×40, from the earlier passes'
notes ([second pass](noctiluca-real-sites-2.md),
[f309073 review](noctiluca-real-sites-f309073-review.md)), and from the
code.

### What remains wrong, and why

1. **Amazon may not serve Noctiluca's browser a normal page at all.**
   - The f309073 review's live Amazon visit got "an unsupported-session
     / client page" (`500 - Amazon.com`), not a search or a product
     page.
   - Chrome's user agent in Noctiluca is `Mozilla/5.0 (X11; Linux
     x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/153.0.0.0
     Safari/537.36` (read from `navigator.userAgent` in this build's
     Chrome).
   - `launch_args` (`chrome.rs:1173`) starts `--headless=new` and sets
     no `--user-agent`, and nothing calls `Emulation.setUserAgentOverride`.
   - Shops commonly answer a `HeadlessChrome` user agent with a robot
     check or a reduced page. That would explain "not perfect" better
     than any layout fault, but it was **not verified** against Amazon
     from here.
   - `navigator.webdriver` is `false` in this Chrome.
2. **Star ratings show as the SVG's pixels, without their value.**
   - On the fixture, each card's stars are drawn as a picture: Kitty
     placeholders, half blocks in ANSI. The model has "4.5 out of 5
     stars" as text and as a rating indicator (`--dump text` lists it).
   - The native ★★★★½ that `indicator::instead` (`indicator.rs:114`)
     should put in its place did not show at 120×40, with the
     `indicators` rule on by default.
   - Cause **not found** in this pass; see `layout.rs:1677`, where an
     indicator with a media region is drawn only under the rule.
3. **Blank rows under a pinned header:** four empty rows between the
   shop's header and its first content at the top of the page (rows 5
   to 8 at 120×40, both modes). Not traced further.
4. **Earlier fixture findings that still hold** (the second pass's
   list):
   - the cookie banner takes three rows of the docked strip;
   - "Sponsored" labels add a row a card;
   - the "Customers also viewed" shelf's `[Previous items]` button takes
     its own row above the strip.
5. **Fixed since the second pass, and seen fixed here:**
   - prices read whole (`$29.99`, `$24.49`, `$49.00`; the
     screen-reader twins rule);
   - the shelf pages as a carousel;
   - the `Image:` captions no longer repeat titles.

None of these needs code about a site. The user-agent question is a
policy choice, not a layout fix.

### Proposed fixes

| # | Fix | Size | How a test shows it |
|---|---|---|---|
| A1 | **A user agent without `Headless`**: derive it from the browser's own (`Browser.getVersion`'s `userAgent` with `HeadlessChrome` → `Chrome`) and set it with `Emulation.setUserAgentOverride`, with matching `userAgentMetadata` brands. On by default or behind a setting, as the maintainer decides: it changes what every site sees | S | rs-rich-embed live test (`RICH_EMBED_CHROME`): `navigator.userAgent` has no `Headless` and `navigator.userAgentData.brands` names no `HeadlessChrome` on an `https`-like secure context (`http://127.0.0.1`) |
| A2 | **Find why the rating indicator does not replace the stars' picture** at 120×40, then fix it in the projection | S | `tests/layout.rs` on `product-grid.html`: the card's row has `★★★★½` and `4.5`, and no picture covers it |
| A3 | **A live Amazon pass from a network that reaches it**: a search results page and a product page, Kitty and half blocks, before and after A1 | S | The checklist in the [guide](../guide/noctiluca/real-sites.md); screenshots in the release notes |

## 3. "The browser has gone"

### Every path that produces it

All four sites are in `crates/rich-embed/src/chrome.rs`, and all mean the
same thing: **the engine's thread has ended**, so its channels are
closed.

| Line | Where | When |
|---|---|---|
| 401 | `DevTools::call`, sending | The engine's thread has ended (the receiver of `calls` is gone) |
| 409–410 | `DevTools::call`, waiting | The call's answer sender was dropped unanswered. It lives in `Session.pending` until answered, so this too means the thread ended with the call pending |
| 817 | `ChromeEngine::devtools` | The engine has no `calls` sender: the browser never started, or the engine was dropped |
| 926 | `ChromeEngine::send` | The engine's thread has ended (navigation, resize, input, screencast and frame acks) |

The thread ends (`run`, `chrome.rs:1974`) and its error goes to
`PageState.error` as `"Chrome: …"` (`chrome.rs:910`) on any of:

1. **The DevTools pipe closes:** "the browser closed: *last stderr
   line*" (`chrome.rs:1541`). The browser process exited: crashed,
   killed, or ended by the OOM killer.
2. **A detach event:** `Inspector.detached` or
   `Target.detachedFromTarget` (`chrome.rs:1963`) gives "the page was
   closed", though **the browser is still alive**. Browser-level events
   have no `sessionId`, so the `ours` check (`chrome.rs:1856`) lets any
   `Target.detachedFromTarget` through without comparing its
   `params.sessionId` or `targetId` with the page's.
3. **No page within 30 s of the start:** "the browser did not open a
   page in time" (`START_TIMEOUT`, `chrome.rs:2040`), though **the
   browser is still running**. It is left running until the engine is
   dropped.
4. **An error answer to `Target.createTarget` or `Target.attachToTarget`**
   (`chrome.rs:1784`). Start-up only.
5. **A write error on the pipe** (`Transport::send`), which happens only
   once the browser has closed its end. On Windows, a WebSocket close or
   error (`chrome.rs:1528`).
6. **A panic on the engine's thread.** None was found on these paths:
   `history` indexing is guarded, frame decoding returns `Option`.

A DevTools call that times out is **not** this message. It is
`TimedOut`, "*method*: no answer in time", after Noctiluca's `CALL` of
30 s (`noctiluca/src/cdp.rs:35`).

### What was reproduced

| Case | What Chrome sent / did | What Noctiluca did |
|---|---|---|
| Browser process killed (SIGKILL) at 12 s, then `R` | Pipe closed | A red toast **"the browser has gone"** on `R`, and again on every later command. The page stays on screen as last read; `media PLAYING` leaves the status line. **No restart.** The cause ("Chrome: the browser closed: …") was set in `PageState.error` and never shown |
| All renderers killed at 12 s, then `R` and `j` | `Inspector.targetCrashed` on the page's session; the browser lives; calls to the page get **no answer** until a reload (`Page.reload` answers and brings the page back, `Inspector.targetReloadedAfterCrash`) | **No message.** The reading that started at the kill never finished: the tab's thread waits in a call with no answer, up to 30 s for each call of the reading. The screen froze with `media PLAYING 0:11`; `R` was queued behind it. To the user the browser is lost, though Chrome is alive and a reload would recover it |
| `Page.crash` (DevTools, raw probe) | `Inspector.targetCrashed`, then no answers until a reload | Not run through Noctiluca; same path as above |
| A page calling `window.close()` | Refused by Chrome (a page Noctiluca opened); no event | Nothing; the page stays |
| 3 busy cores, video page, 30 reloads over 100 s | — | **Not reproduced:** no "gone", no stop |

Raw probe output for the renderer kill (browser socket, flattened
session):

```text
('Inspector.targetCrashed', 'A9A561AC0ACA7628729A5ECC25EE43B5', '{}')
browser alive: True
evaluate after: null            # no answer within 10 s
reload after: {"id": 6, "result": {}, "sessionId": "A9A5…"}
events after reload: [..., 'Inspector.targetReloadedAfterCrash', 'Page.frameStartedNavigating', ...]
```

### Root causes

- **R1. Noctiluca never shows why the engine ended.**
  - `PageState.error` is read only by `--dump` (`dump.rs:37`).
  - Commands fail with `Refusal::Failed` (`tab.rs:1705`), which the app
    toasts (`app.rs:2207`). So the user reads the message of the
    *command* that found the engine gone, not the cause.
- **R2. Nothing restarts the browser or reopens the page.**
  - `ChromeEngine::send` starts a browser only when it has none
    (`commands.is_none()`). After the thread ends, `commands` stays
    `Some(dead sender)`, so every call fails at once.
  - `Tab` and `ChromeBackend` hold the dead `DevTools` too.
- **R3. A renderer crash is not noticed.**
  - rs-rich-embed ignores `Inspector.targetCrashed`.
  - Noctiluca's calls to the crashed page wait the full 30 s each,
    blocking the tab's thread (readings, frames, commands).
  - This is the likeliest "lost while Chrome is alive" on long video
    sessions and busy pages, where renderers are the processes that
    grow and get killed. It is not this message, but it looks the same
    to the user, or worse.
- **R4. Two exits end a live browser's session with no way back:**
  - a detach event, not filtered to the page's own session (cause 2);
  - the start timeout on a loaded machine (cause 3), which also leaves
    that browser running until the engine is dropped.

  Neither was reproduced in this pass.

### Proposed fixes

| # | Fix | Size | How a test shows it |
|---|---|---|---|
| B1 | **Say why:** when `PageState.error` becomes set, show it once in the status line or a notice, cleaned (`clean::clean`), with the key that restarts; commands after it say "the browser stopped: *cause*" once, not a toast each | S | `tests/app.rs` with the fake engine: an engine whose state gets an error shows it once; a second command shows no second toast |
| B2 | **Restart on demand:** an engine whose thread has ended starts a new browser on the next navigation (`ChromeEngine` drops the dead channels and the child when the thread exits; `start` then runs again), and Noctiluca's reload key reopens the last address in it, re-running `ChromeBackend::start`'s setup (binding, script, world) | M | rs-rich-embed live test: kill the browser's process, then `open(url)` succeeds and a frame arrives. Noctiluca: fake engine test that a dead engine is replaced on `R` |
| B3 | **Handle `Inspector.targetCrashed`:** mark the page crashed in `PageState` (a new field, or `error`), answer pending page calls at once with an error instead of letting them wait 30 s, and let Noctiluca offer a reload (which Chrome answers) | M | rs-rich-embed live test: `Page.crash` through `DevTools::call` makes the next `Runtime.evaluate` fail within 1 s and sets the state; after `reload` the page answers again. Noctiluca: a crashed state shows a notice and `R` recovers |
| B4 | **Filter detach events** to the page's own `sessionId` and `targetId` (`params.sessionId` / `params.targetId` against `self.session` / `self.target`) | S | Unit test of `Session::message` with a `Target.detachedFromTarget` for another session id: the engine keeps running. One for its own: it ends, as now |
| B5 | **Start timeout:** end the browser that did not open a page in time (it is left running today), and make the 30 s depend on the machine's load or retry once | S | rs-rich-embed test with a stub binary that never answers: after the timeout no child process remains |
| B6 | **Shorter waits for page calls that a frame or a reading can do without** (snapshot calls on a crashed or hung page): a per-call timeout well under `CALL` for the tab's periodic work | S | Fake engine test: a call that never answers delays the next frame by no more than the shorter timeout |

## What could not be verified, and why

- **Real terminals.** None was available: Kitty, iTerm2, a Sixel
  terminal, Ghostty or WezTerm. Flicker is inferred from the bytes and
  the order they come in; whether a given terminal shows it depends on
  its synchronized-output support and image handling. Whether the
  maintainer's Kitty and iTerm2 sessions had synchronized output
  detected is unknown. Check with `RICH_SYNC_OUTPUT=0` and `=1`.
- **Release numbers.** Every number above is from a debug build (disk).
  The perf note's release runs agree on reading gaps of 0.20 to 0.23 s.
- **Real video sites.** YouTube is behind a bot wall from this network
  and Twitch was not run; the fixture is 30 fps. The 24 fps judder in
  cause 3 is arithmetic, not a measurement.
- **Amazon.** Not reachable (403). The user-agent explanation is
  inferred from the earlier live visit's unsupported-client page and the
  user agent Chrome sends.
- **"The browser has gone" in the wild.** Not reproduced with Chrome
  alive. The browser-process-killed path and the renderer-crash freeze
  were reproduced by killing processes. Which one the maintainer hit is
  unknown: B1's message would tell the next time.
- **The stars' picture over the rating indicator** (Amazon 2): cause not
  found.
