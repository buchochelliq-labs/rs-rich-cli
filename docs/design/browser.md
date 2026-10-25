# Noctiluca: a terminal web browser on intuiTUIve

Status: proposal, with a first spike merged on branch `022-browser`
(`crates/noctiluca`). Written 25 October 2026.

Noctiluca is a full web browser for the terminal. Chrome loads and runs the
page. Noctiluca reads what Chrome laid out and draws it in cells: text as
terminal text, controls as intuiTUIve controls, and images, GIFs and video
as media regions. It is a crate of its own, `noctiluca` (binary
`noctiluca`), built on intuiTUIve and rs-rich-embed. intuiTUIve's core
gets only generic pieces from this work (see
[the core proposals](#the-pocs-core-proposals-c1-to-c5)).

## Where things stand

- **Merged on `022-browser`** (pushed to
  `claude/next-priority-items-3c5p8e-wip-browser`): `DevTools::call` and
  a history fix in rs-rich-embed; the `noctiluca` spike; this note.
- **In flight**: nothing.
- **Next**: the decisions at [the end](#what-a-human-must-decide), then
  rs-rich-embed's items 2 to 4 (events, the faster loop, screencast on
  demand), then phase 0.1.

This note started from a proof of concept the user supplied: a Node and
Playwright browser (`src/`), a metadata-driven widget experiment
(`native-web/`) and an uncompiled intuiTUIve frontend (`native-web/rust/`).
Where this design differs from the PoC, it says so and says why.

## Goal

- **Text stays text.** Headings, paragraphs, links, lists and tables are
  terminal cells the user can select and copy. Text is never rasterized,
  except in the raster debug view.
- **Media stays media.** Images, GIFs, video, canvas and WebGL show as
  media regions: pixels through Kitty, iTerm2 or Sixel, or character art
  where the terminal has no graphics. Chrome decodes everything.
- **Sound plays.** Chrome plays page audio on the machine it runs on.
- **Controls come from page semantics.** Buttons, fields, check boxes,
  selects and sliders come from Chrome's accessibility tree, the same way
  on every site. There are no per-site layouts.
- **The F1 action overlay stays live.** It lists what can be done now, with
  the reason when an action is unavailable, and it updates while open.

Never: DRM playback, extensions, getting past bot checks or sign-in walls,
or changing browser policy.

## Use cases, and how they drive the build

Each phase is built for named use cases, against fixtures written for
them, and is done when those use cases work on the fixtures.

1. **Reading docs from the terminal.** Rust docs, MDN, Python, manuals.
   Code blocks stay real text that can be copied.
2. **The developer's web chores.** Pull requests and issues, CI logs,
   cloud consoles, dashboards. These are mostly forms, tables and links;
   charts show as media regions.
3. **Remote and SSH work.** Internal dashboards, router admin pages,
   Jupyter. The browser runs where the network is, and only text crosses
   the link.
4. **News and long reading.** Articles in reader mode, with their images
   inline.
5. **Media in a corner.** A video or a livestream beside your work, its
   chat read-only.
6. **Accessibility.** Pages arrive as their real accessibility tree: as
   structure, not pixels.
7. **Automation and agents.** A script or an AI agent drives real pages
   through Noctiluca's page model, while a human watches the same session.
8. **Testing and CI.** A page snapshot as text for diffs, accessibility
   checks, and tapes of web flows.

### Phases by the use cases they unlock

| Phase | Unlocks | Fixtures and acceptance |
|---|---|---|
| 0.1, reader | 1, 4, 6 | A saved docs page, an article and a form page. Text can be selected, links followed by hint, the AX tree is complete, and no escape sequence leaks |
| 0.2, forms and tables | 2, 3 | A login-style form, a table-heavy dashboard and a settings page. Widgets commit, conflicts are caught, tables scroll |
| 0.3, layout projection | 2, 4 | Two-column and grid layouts stay side by side |
| 0.4, media | 5 | The PoC's GIF and video pages. Frames stay within budget; play, pause and seek work; sound plays on the host |
| 0.5, automation API | 7, 8 | A scripted flow on a fixture site, the same session viewable live, and a text-snapshot diff |

[Phases](#phases) below holds each phase's full acceptance list. The
reader phase is built against fixtures for use cases 1, 4 and 6 first.

### How we stay honest

- **Dogfood.** Use Noctiluca for this repo's own docs, pull requests and
  CI. Each annoyance becomes an issue. Generic fixes go to intuiTUIve
  core.
- **A local site corpus.** Saved pages of real types, checked into the
  fixtures, are the regression suite, the way the eleven clones serve
  intuiTUIve. Check the licence of anything saved. Prefer pages written
  for the corpus, or permissively licensed docs.
- **A live-site checklist,** run on a networked machine at each phase:
  BBC, YouTube search, Twitch, GitHub and MDN. Screenshots are filed as
  evidence.
- **One stated non-goal per phase,** in its section below.

### The first public release

Use cases 1 and 3: docs and dashboards from the terminal, over SSH too.
They need only the reader and the forms and tables phases, and they are
the clearest win over other terminal browsers. Media and automation come
after.

## What was verified on this machine

All of this ran in this session, on Linux with 4 CPUs, against Chrome for
Testing 153.0.8010.12 (`/opt/pw-browsers/chromium-1243`), with local pages
only.

| Check | Result |
|---|---|
| The PoC's Rust frontend, copied to a scratch directory, against published rs-rich-intuituive 0.0.6 | Builds unchanged with no warnings; clippy clean; its 5 tests pass. There is no `build.rs`. The Rust half of the PoC was never broken, only never built |
| The same, against this branch's intuiTUIve by path | Builds; 5 of 5 pass. This branch's `crates/rich-intuituive` is identical to the `rs-rich-intuituive-v0.0.6` tag |
| rs-rich-embed's tests with the `chrome` feature, as root | 50 pass; the live browser tests skip |
| The same as an unprivileged user (`runuser -u nobody`), with `RICH_EMBED_CHROME` set | All pass, live ones included: a screencast frame over the pipe and over the port, no listening port while a view is open, no sandbox switch reaching the browser |
| Chrome as root | Refuses to start: "Running as root without --no-sandbox is not supported". The engine never turns the sandbox off, so browser tests must run as a normal user |
| `DevTools::call` (new, see below) | Live test passes: evaluates a page's heading, a refused method fails only that call |
| The spike: `noctiluca page.html` | Live test passes in 0.5 s: headings and paragraphs in order, hidden text left out, an ESC byte stripped, script-written text present |
| A three-page read, 12 runs | 12 of 12 after fixing an engine bug the spike found (below). 1 run in 4 failed before |
| Codecs, from `canPlayType` | H.264, AAC, VP9, AV1 and Opus: "probably"; HEVC: no. True of both Chrome for Testing and its headless shell |
| Key systems, from `requestMediaKeySystemAccess` on a `file://` page | Full Chrome in `--headless=new`: Widevine and Clear Key access granted (decryption not tried). Headless shell: Widevine not supported, Clear Key granted |

Not run: the PoC's `npm` scripts and tests (untrusted, per the brief), any
live website (the network blocks them), audio output, native terminals.

### Snapshot cost on the local pages

`cargo run --release -p noctiluca --example snapshot_cost -- PAGES`,
median of 5 calls, 100 x 40 cells at 10 x 20 pixels:

| Page | Snapshot ms | DOM nodes | Layout boxes | Snapshot KiB | AX tree ms | AX nodes | AX KiB |
|---|---:|---:|---:|---:|---:|---:|---:|
| `native-web/audio-demo.html` | 4.0 | 31 | 15 | 3 | 8.4 | 92 | 34 |
| `demo/css.html` (an animated GIF) | 2.8 | 82 | 53 | 10 | 19.1 | 87 | 29 |
| `native-web/demo.html` | 7.5 | 120 | 70 | 13 | 14.9 | 138 | 59 |
| `demo/giphy.html` (its GIF is remote, so blocked) | 16.7 | 60 | 31 | 7 | 18.5 | 46 | 18 |
| `demo/index.html` | 16.0 | 103 | 62 | 12 | 20.7 | 86 | 32 |
| `demo/video.html` | 10.3 | 33 | 15 | 3 | 17.6 | 55 | 16 |
| `demo/watch.html` | 7.7 | 73 | 48 | 11 | 19.8 | 101 | 30 |
| `crates/noctiluca/tests/fixtures/page.html` | 16.8 | 51 | 23 | 4 | 17.7 | 31 | 11 |

Three things this shows:

- **The pages are small.** A news site has 50 to 100 times these nodes.
  Whether a snapshot stays cheap there can only be measured with a network
  (see [the end](#what-only-a-networked-machine-can-verify)).
- **Many calls cost about 16 ms whatever the page.** The engine's thread
  reads DevTools with a 15 ms poll and looks at the app's calls only
  between reads, so a call can wait a whole poll first. That is a floor
  set by the engine, not by Chrome.
- **A screencast can starve calls.** In a debug build, `css.html` took
  660 ms a snapshot and every other page about 18 ms. Its animated GIF
  keeps screencast frames coming, and the engine decodes each JPEG on the
  same thread that answers calls. A release build hides it (2.8 ms). It
  will come back with video or several GIFs.

Both are fixed in rs-rich-embed (see [what it gains](#rs-rich-embed-cdp-generic)).

### A bug the spike found and fixed

Reading three local pages in a row ended the engine about one run in four
with "Not attached to an active page". A `file://` page loads in a new
renderer process, swapped in for `about:blank`'s. A
`Page.getNavigationHistory` sent during the swap is refused, and the engine
treated a refused history request as fatal. Any cross-site navigation in a
`web_view` could hit it too. It is now ignored, since the next navigation
event asks again. Fixed in `crates/rich-embed/src/chrome.rs`, with a
changelog entry.

## Architecture

Everything is Rust. There is no Node and no Playwright. Chrome is a
separate process, started by rs-rich-embed with its sandbox on, a
temporary profile and DevTools over a pipe.

```
noctiluca (the browser crate)
  UI: tabs, address bar, reader mode, link hints, find, F1 overlay, downloads
  model: blocks, controls, media regions, actions (one registry)
  layout: projection of Chrome's geometry into cells; reader reflow
  media: frame scheduler, budget, encoders through rs-rich-art
  Engine trait ──▶ CDP backend, over rs-rich-embed's ChromeEngine + DevTools
        │
rs-rich-embed: launch, sandbox, pipe, profile, screencast, input,
               history, DevTools::call, events, region capture, downloads
rs-rich-art:   Kitty, iTerm2, Sixel, half blocks; fast frame encoding
rs-rich-intuituive: retained nodes, signals, controls, modals, tabs,
                    jobs, finder, image nodes and graphics placement
```

### The engine trait

The browser talks to an `Engine` trait, not to Chrome. Its first backend
is CDP over rs-rich-embed. A WebDriver BiDi backend (Firefox) or a
JavaScript-free engine for fast reading (blitz: stylo with taffy) could
plug in later. CDP comes first because a DOM snapshot with layout and a
screencast have no BiDi equivalents yet.

```rust
pub trait Engine {
    fn open(&mut self, tab: TabId, url: &str) -> io::Result<()>;
    fn new_tab(&mut self, url: &str) -> io::Result<TabId>;
    fn close_tab(&mut self, tab: TabId) -> io::Result<()>;
    fn history(&mut self, tab: TabId, step: HistoryStep) -> io::Result<()>;
    /// The page as laid out: nodes, layout boxes, text boxes, styles,
    /// paint order, and the accessibility tree.
    fn snapshot(&mut self, tab: TabId) -> io::Result<PageSnapshot>;
    /// Pixels of a region of the page, in CSS pixels.
    fn capture(&mut self, tab: TabId, region: Rect, format: Format) -> io::Result<Frame>;
    /// A real input event at a node or a point.
    fn act(&mut self, tab: TabId, action: PageAction) -> io::Result<()>;
    fn events(&mut self) -> Vec<EngineEvent>; // changed, navigated, download, dialog, …
}
```

**Where the trait lives is a decision.** The lead's direction says the
trait is "implemented by rs-rich-embed's CDP backend". But if the trait is
in `noctiluca`, rs-rich-embed cannot implement it without depending on the
browser, which turns the dependency graph around. Two ways out:

1. **The trait in `noctiluca`**, with the CDP backend as an adapter in
   `noctiluca` over rs-rich-embed's generic CDP calls. rs-rich-embed stays
   a general library. This note recommends it.
2. **The trait in rs-rich-embed**, as a generic "page engine" next to
   `WebEngine`. Other apps could use it. It puts more of a browser's shape
   into embed.

### Reading the page: CDP, not an injected collector

The PoC's `native-web/dom.js` runs in the page's own JavaScript world. It
keeps its state on `window.__nativeWebPocV1`, which the page can read,
replace or break. It hooks every element's events, samples every control's
properties on each pass (O(n) by design), and computes accessible names
only partly (`aria-labelledby`, `aria-label`, labels, then attributes).

Noctiluca reads the page with two DevTools calls instead:

- **`DOMSnapshot.captureSnapshot`**, with computed styles, layout
  rectangles, inline text boxes and paint order. It is one call, and the
  page cannot observe it. The spike uses it already: a text node with no
  layout box is not shown, which handles `display: none` with no rules of
  our own.
- **`Accessibility.getFullAXTree`** for the real roles, names, states and
  values, as Chrome computes them for screen readers. The cross-site
  semantic controls come straight from it. A node's `backendDOMNodeId`
  joins it to the snapshot.

To know when to read again, one small script runs in an **isolated
world** (`Page.createIsolatedWorld`), not the page's world: a single
`MutationObserver` and media event listeners, which call a binding
(`Runtime.addBinding` with `executionContextName`) as a "something
changed" ping. Noctiluca debounces the pings (about 100 ms, and at once
after an action of ours) and takes a new snapshot. The page cannot see an
isolated world's globals or the binding. It can still notice extra work
through timing; that is accepted.

The lead's direction asked for `Runtime.addBinding` with a
MutationObserver. Doing that in the page's main world would bring back the
PoC's exposure, since the binding would be a global the page can call or
fake. The isolated world keeps the ping private. This needs checking on
real sites: some frameworks replace whole subtrees on every frame, and the
debounce must not turn that into a snapshot per frame.

What carries over from the PoC: its stale-target rules (an action is bound
to a node, its document and its meaning, and fails if the meaning
changed), keyed select options, the password rule (a password field's
value is never read), the extraction bounds and its control character
cleaning. They move into Rust, against snapshot and AX data.

### The model

One model per tab, rebuilt from each snapshot and diffed against the last
by `backendDOMNodeId`:

- **Blocks**: headings, paragraphs, list items, table cells, quotes, code.
  Each holds runs of text with style and the layout rectangles of its text
  boxes.
- **Controls**: an AX node with an interactive role (button, link,
  text box, check box, radio, combo box, slider, switch, tab), with name,
  value, state, availability, and a stable key. Per-option keys for
  selects.
- **Media regions**: the opaque boxes (below), each with its rectangle,
  kind, and for media elements their playback state.
- **Actions**: one registry. Each action has an id, a label, keys, an
  availability rule and a reason when unavailable. The keys, the link
  hints, the mouse and the F1 overlay all run actions from this registry,
  and each re-checks availability when it runs.

### Layout: Chrome's geometry, snapped to cells

The PoC's hybrid view reflows text by its own rules, so flex, grid,
columns and tables collapse into one column. Noctiluca lays out by
Chrome's own geometry instead:

1. **The viewport is the terminal.** Chrome's viewport is set to
   `columns × cell width` by `rows × cell height` CSS pixels
   (`Emulation.setDeviceMetricsOverride`, which rs-rich-embed already
   sends). The cell size comes from the terminal when it reports one, else
   10 x 20.
2. **Every layout box has a kind**: text (it has inline text boxes),
   control (its AX role is interactive), or opaque. Opaque covers images,
   GIFs, video, canvas, WebGL, SVG with more than a few elements, and
   cross-origin iframes. Hidden boxes are dropped (`visibility`,
   `opacity: 0`, clipped to nothing, or covered by a higher box in paint
   order).
3. **Each box is snapped to cells**: `x0 = floor(left / cw)`,
   `x1 = ceil(right / cw)`, `y0 = round(top / ch)`.
4. **Text is placed by its line boxes.** Each inline text box is one line
   as Chrome broke it. Its text goes at `(x0, y0)` in terminal text with
   its mapped style (colour, background, bold, italic, decorations, as the
   PoC's `css-layout.js` maps them).

The hard part is that terminal glyphs are not Chrome's glyphs. A line
Chrome set in a proportional 16 px font can need more or fewer cells than
its box has. Rows can also collide, since two of Chrome's lines can round
to one row. The algorithm:

- **Inside a box, rewrap.** If a text box's line does not fit in
  `x1 - x0` cells, the block's runs are rewrapped to that width, by
  Unicode cell width (`rich::cells`), so its height can grow by rows.
- **Collisions push down, by column band.** Boxes are placed in reading
  order (top, then left). A skyline holds, for each column, the first free
  row. A box goes at `max(y0 + shift, the skyline's highest row over its
  columns)`. When a box grows by `d` rows, `d` is added to the shift of
  every later box whose top is below the grown box's original bottom and
  whose columns overlap it. A column that grew pushes what is under it,
  not what is beside it. That keeps a two-column layout two columns.
- **Text never overlaps text.** If two text boxes still claim a cell, the
  later in paint order wins and the other moves down a row.
- **Overflow is clipped and marked.** Text wider than the viewport, and
  `pre` or `nowrap` text, is clipped with a `…` cell. Horizontal scroll is
  a per-box action, not a page mode.
- **Fixed and sticky boxes** go on an overlay layer pinned to the screen,
  drawn after the page. A modal dialog above the page (by paint order and
  `aria-modal`) dims what is under it.
- **Opaque boxes** keep their snapped rectangle. A text box painted above
  an opaque one (a caption over a hero image) is drawn over the region's
  cells with a background.
- **The whole document** is projected, not only the viewport, since the
  snapshot covers every layout box. Scrolling is ours, in rows; Chrome's
  scroll position follows, so lazy loading and infinite scroll still
  fire.

A test can check the invariants on every fixture page: no two text runs
share a cell, reading order is kept within a column band, and every
control's cell rectangle lies within its snapped box.

A second mode keeps the PoC's idea: **reader mode**, a semantic reflow of
the blocks into one readable column, from the AX tree and the snapshot's
text. It is the more accessible view and the fallback for pages the
projection reads badly.

### Fall back per region, not per page

The PoC's F2 switches the whole page to Chrome's raster. Noctiluca falls
back one region at a time. An opaque box becomes a media region, a tile of
Chrome's own pixels for that rectangle, drawn with the graphics backends.
The rest of the page stays text. The whole-page raster becomes a **raster
view** for debugging. rs-rich-embed's `web_view` over `ChromeEngine`
already is one: a screencast drawn in half blocks.

### Media regions

Chrome decodes all media. Noctiluca asks for frames only for media regions
that changed, within a frame budget.

Three ways to get frames:

| | `Page.startScreencast` | `Page.captureScreenshot` with `clip` | `HeadlessExperimental.beginFrame` |
|---|---|---|---|
| What | Chrome pushes viewport frames when it paints | One region, when we ask | A frame when we ask, with Chrome's clock stopped between frames |
| Good for | Video, many changing regions | Static images, slow GIFs, a few regions | Deterministic CI frames |
| Cost | The whole viewport is encoded and decoded even when one small region changes; each frame needs an ack | A round trip per region per frame | Needs the headless shell with `--enable-begin-frame-control`; not in `--headless=new` (not verified here) |
| Plan | Crop per region; only while video plays | Default for everything else | CI goldens only, if the headless shell is used |

Two more paths skip Chrome's raster altogether:

- **Original bytes.** A static `<img>` can be fetched as its original
  bytes (`Page.getResourceContent`) and fitted by rs-rich-art. That gives
  better quality and works off-screen.
- **Native animation.** An animated GIF can be decoded once by rs-rich-art
  (`decode_animation`) and handed to Kitty as an animation
  (`kitty::transmit_animation`). The terminal then plays it with no
  further bytes. It runs on the terminal's clock, not the page's, which is
  fine for a GIF and wrong for a canvas.

**Terminal bandwidth is the real limit,** above all Sixel over SSH. A
640 x 360 Sixel frame is hundreds of kilobytes. The scheduler therefore:

- measures how fast the terminal takes output, and gives each region a
  frame rate from a total byte budget (the PoC's default of 8 fps is a
  ceiling, not a target);
- updates only regions on screen, newest change first;
- steps down per region when over budget: Kitty, then iTerm2, then Sixel,
  then quadrant or half-block character art. Over SSH (`SSH_CONNECTION`
  set) video starts at character art unless the user asks for more;
- uses Kitty's shared memory (`t=s`) or temporary file (`t=t`) transfer
  when the terminal is on the same machine. rs-rich-art sends only direct
  data (`t=d`) today; this is a gap for rs-rich-art.

intuiTUIve already draws pictures in cells: `image_bytes_of` (a node over
reactive bytes) with `image_graphics` (Kitty placeholders, Sixel and
iTerm2 placement). For video it needs a path that takes decoded RGBA
frames without encoding PNG in between. That is a proposal for rs-rich-art
and intuiTUIve's `art` feature: fast frame encoding.

**Codecs and DRM.** On this machine Chrome for Testing reports H.264 and
AAC as playable (`canPlayType`: "probably"). That contradicts the lead's
note that Chromium lacks them: plain open-source Chromium builds usually
do lack them, but Chrome for Testing has them. So Noctiluca prefers a
system Chrome, then Chrome for Testing, then Chromium, and shows the
codec table in its about page. Full Chrome in `--headless=new` granted
Widevine access in a test here; its headless shell did not. Either way, DRM
playback is out of scope by policy: Noctiluca never tries a licence
request.

### Audio

Chrome plays page audio on the machine Chrome runs on. When Noctiluca runs
over SSH, the sound plays on the server, not at the user's terminal. No
terminal protocol carries audio. A remote audio transport would be a
separate, optional project.

rs-rich-embed starts Chrome with `--mute-audio` every time. Noctiluca
needs an opt-in to leave it off (`--audio`), and stays muted by default.
Volume, mute, play and pause are actions on the media element, read back
from Chrome. The PoC showed this works: it recorded Chrome's ALSA output to
a file sink and measured playing, muted, 30% and paused levels. That test
is worth porting (Linux CI only).

### Input and focus

- **Keys and pastes** go to the focused field as real input events
  (`Input.dispatchKeyEvent`, `Input.insertText`), as rs-rich-embed sends
  them now.
- **Clicks** are real mouse events at the middle of the target's content
  quad (`DOM.getContentQuads`), so the page sees trusted events. A
  terminal click maps to the control or link under that cell in the
  projection.
- **Focus**: Tab and Shift+Tab move through the controls in reading
  order; `DOM.focus` moves Chrome's focus along.
- **Selects and ranges**: choosing an option sets it through the isolated
  world and fires `input` and `change`, keyed by option, as the PoC does.
- **Every action re-checks** its target before running: same document,
  node still there, same role and name. Otherwise it fails with a reason
  and nothing is sent.

### Tabs, history, bookmarks, downloads, find

- **Tabs**: one DevTools target per tab (`Target.createTarget`).
  rs-rich-embed drives one target today. Pop-ups open as tabs.
  intuiTUIve's `tab_set` and `tabs_for` draw the tab bar.
- **History**: per tab from `Page.getNavigationHistory`, already in
  rs-rich-embed. A searchable list of past pages uses intuiTUIve's
  `finder_of`.
- **Bookmarks**: a local file in the user's config directory, written only
  when the user adds one.
- **Downloads**: always ask first. Chrome downloads into a private
  quarantine directory (`Browser.setDownloadBehavior` with
  `allowAndName`); on `Browser.downloadWillBegin` Noctiluca asks with the
  file name and source. No cancels it (`Browser.cancelDownload`); yes moves
  the finished file to the downloads directory under a cleaned name, never
  executable, never opened. Progress shows in intuiTUIve's `jobs_panel`.
  rs-rich-embed denies all downloads today; that stays its default.
- **Find in page** searches Noctiluca's own projected text, so it needs no
  DevTools call. It scrolls to each match and highlights it.
- **Link hints**: press `f` and every visible link and control gets a
  short label; type it to act, Shift to open in a new tab. Hints come from
  the action registry, so they follow the same availability rules.
- **The mouse** works throughout: click, wheel, and drag-select in text.

### Security

- Chrome runs as a separate process with its sandbox on. rs-rich-embed
  drops every switch that would turn any of it off, so Noctiluca cannot
  run as root. This is deliberate.
- DevTools runs over a pipe on Linux and macOS: no port. On Windows it is
  a `127.0.0.1` port, which other local users can reach (rs-rich-embed
  documents this).
- The profile is temporary and private (mode 700) unless the user opts in
  to a kept profile.
- No page JavaScript runs in our process. Page data is parsed as JSON from
  DevTools and treated as untrusted.
- Our one script lives in an isolated world. It reports only that
  something changed.
- Everything from the page is cleaned before it reaches the terminal:
  text, titles, URLs, link targets, alt text, option labels, download
  names. C0 and C1 control characters, DEL, and bidirectional overrides
  and isolates are stripped, so no escape sequence can pass. The spike
  does this (`noctiluca::clean`), and its fixture has an ESC byte.
- URLs in the address bar and in link hints show their real host, with
  punycode for confusable names.
- Downloads always ask first and never run.
- Password fields: their value is never read and shows as a fixed mask.

## What is reused and what is new

| From | Reused |
|---|---|
| rs-rich-intuituive | The app, signals, retained and keyed nodes, `button`, `input`, `text_area`, `checkbox`, `select_field`, `slider`, modals and dialogs, `tab_set`, `finder_of`, `jobs_panel`, help and palette, toasts, `image_bytes_of` and `image_graphics`, `pixels`, the accessibility tree and `Driver` for tests |
| rs-rich-embed | `ChromeEngine`: finding the browser, launching it sandboxed, the pipe, the private profile, the screencast, input events, history; `web_view` as the raster view; `DevTools::call` (new) |
| rs-rich-art | Kitty (with placeholders and animation), iTerm2 and Sixel encoders, `fit_to_cells`, `decode_animation`, `FrameBudget`, half blocks, quadrants and Braille |
| rs-rich-record | Its VT emulator, for end-to-end tests of the binary on a PTY, and tapes for the docs |
| rs-rich | `Text`, styles, cell widths, markup escaping |
| The PoC | Its rules, not its code: stale targets, keyed options, the password rule, the CSS-to-cell style mapping, live/DVR caution for media, the audio output test, and its local demo pages as fixtures |

New:

- **`noctiluca`**: the engine trait and the CDP adapter, the model, the
  projection layout, reader mode, the media scheduler, the action
  registry, link hints, find, downloads policy, bookmarks and the UI.
- **rs-rich-embed** (generic CDP capabilities, below).
- **rs-rich-art**: fast frame encoding from RGBA, and Kitty's `t=s` and
  `t=t` transfers.
- **intuiTUIve core**: only C1 to C4, below.

### rs-rich-embed: CDP, generic

Each of these is useful to any app that drives Chrome, not only to a
browser:

1. **`DevTools::call`** (done on this branch): send any command to the
   page, get its result. Cloneable and `Send`.
2. **Events**: a subscription to DevTools events by method name
   (`Page.downloadWillBegin`, `Runtime.bindingCalled`, dialogs).
3. **A faster loop**: wake the engine's thread when a call arrives instead
   of after the 15 ms read poll (one channel for both), and decode
   screencast frames off that thread, dropping stale ones. Both were
   measured above.
4. **Screencast on demand**: start and stop it, since a reader does not
   need it.
5. **Region capture**: `capture(rect, format)` over
   `Page.captureScreenshot` with `clip`.
6. **Several pages**: one browser, many targets.
7. **Isolated worlds**: create one and add a binding to it.
8. **Audio opt-in**: leave out `--mute-audio` when asked.
9. **Downloads policy**: deny (the default), or quarantine with events.

## The PoC's core proposals, C1 to C5

The PoC reviewed rs-rich-intuituive 0.0.6 at commit `29084d4b`. The brief
said this branch is newer than 0.0.6. For intuiTUIve it is not:
`crates/rich-intuituive` on this branch is identical to the
`rs-rich-intuituive-v0.0.6` tag, which is that same commit. The branch is
three commits past it, all plan documents. So each verdict below is from
the current source, and it matches what the PoC saw.

### C1. Reactive labels on base controls

**Still a gap.** `ButtonWidget` holds `label: String`, and `button` takes
`impl Into<String>`. The face and the accessible name are fixed when the
node is built. `checkbox`, `switch_toggle` and `Node::label` are the same.

**Generic: yes.** Play and Pause, counts, and job or device states all
need it.

**API:**

```rust
pub fn button_of(label: impl Fn() -> String + 'static, on_press: impl FnMut(&mut Ctx) + 'static) -> Node;
pub fn checkbox_of(label: impl Fn() -> String + 'static, on: Signal<CheckState>) -> Node; // with C2
```

Reading the label inside `draw` tracks it, as `text(|| ..)` does. The
accessible name follows the label. `button(label, f)` stays as
`button_of(move || label.clone(), f)`. Acceptance: the label changes while
focused and while pressed, with the same node, focus and pressed state.

### C2. A three-state check box

**Still a gap.** `Toggle` holds `Signal<bool>`. The accessibility state is
`checked: Option<bool>`, which cannot say "mixed" either, so this is also
a gap in `a11y`.

**Generic: yes.** A parent of partly chosen children, permissions, and
file trees all need it.

**API:**

```rust
pub enum CheckState { Off, On, Mixed }
pub fn checkbox_tri(label: impl Into<String>, state: Signal<CheckState>) -> Node;
// Mixed is drawn [-]; Space from Mixed goes to On by default:
impl Node { pub fn on_mixed(self, next: CheckState) -> Node; }
// a11y: a `mixed: bool` beside `checked`, described as "mixed"
// (`aria-checked="mixed"` on the web renderer).
```

`checkbox(label, Signal<bool>)` is unchanged. Acceptance: all three states
drawn and described; keys and clicks move between them; a change from
outside while focused redraws; a disabled Mixed box stays Mixed.

### C3. Keyed select options, with per-option availability

**Still a gap.** `select_field` takes `impl Fn() -> Vec<String>` and
`Signal<usize>`. An option is an index: it cannot keep its identity
through a reorder, and it has no enabled flag or reason. `radio` has the
same shape and the same gap.

**Generic: yes.** Devices, branches, accounts and environments change
under an open list.

**API:**

```rust
pub struct SelectOption<K> { pub key: K, pub label: String, pub enabled: bool, pub reason: Option<String> }
pub fn select_field_keyed<K: Clone + PartialEq + 'static>(
    options: impl Fn() -> Vec<SelectOption<K>> + 'static,
    selected: Signal<Option<K>>,
) -> Node;
pub fn radio_keyed<K: Clone + PartialEq + 'static>(/* the same */) -> Node;
```

If the selected key disappears, `selected` becomes `None` and the field
says so. Disabled options are shown, cannot be chosen by key, click or
typeahead, and show their reason. Acceptance: a reorder keeps the
selection; a removal follows that policy; a disabled option cannot be
chosen in any way; a label changes without losing the key.

### C4. A live action list

**Still a gap,** and a sharper one than the PoC said. `open_help` and
`open_palette` (`app.rs`) collect the bindings once, when they open. A
binding whose `bind_when` condition changes while the overlay is open is
not updated. And from reading `run_binding_with`, a command chosen in the
palette runs its handler without checking that condition again, so a
command that stopped applying after the palette opened would still run.
That last point is from the source and has not been tested.

**Generic: yes.** Any app whose commands depend on changing state needs a
live list: jobs, devices, players.

**API:** make the existing overlays live rather than add a new one, and
add reasons:

```rust
impl Node {
    /// A binding shown even when unavailable, greyed, with why.
    pub fn bind_unless(self, unavailable: impl Fn() -> Option<String> + 'static,
                       keys: &str, description: &str, handler: impl FnMut(&mut Ctx) + 'static) -> Node;
}
// Help and palette re-read the bindings each frame while open, keep the
// selection by binding id across changes, and check availability again
// when a command runs.
```

For actions that are not key bindings (a page's actions in Noctiluca), a
generic source:

```rust
pub struct Action { pub id: String, pub label: String, pub keys: Vec<Key>, pub unavailable: Option<String> }
pub fn action_overlay(actions: impl Fn() -> Vec<Action> + 'static,
                      run: impl FnMut(&str, &mut Ctx) + 'static) -> Node;
```

Acceptance: actions appear, disappear and change label while open; the
selection stays on the same id; an action that became unavailable does not
run; focus returns where it was; no key reaches the editor beneath.

### C5. A draft field with a commit lifecycle

**Still a gap,** but not for core. Nothing in intuiTUIve tracks a
confirmed value, a local draft, a pending commit and a conflict. `input`
binds one signal both ways, which is enough for local state.

**Generic: in principle.** A remote setting or device would use it. But
only the browser needs it today, and the PoC proved it in JavaScript
only. It stays in `noctiluca` until a second user appears; it could then
move to rs-rich-ext or an intuiTUIve plugin. The lead's direction agrees:
core gets C1 to C4 only.

```rust
pub enum DraftState { Clean, Dirty, Pending, Conflict { remote: String }, Failed(String) }
pub struct DraftField<T> { /* confirmed, draft, revision, state */ }
```

## Phases

Every phase's acceptance tests run in CI on local pages only: the PoC's
`demo/*.html` and `native-web/*.html` (copied into
`crates/noctiluca/tests/fixtures` with their licence checked) and new
fixtures. Browser tests follow rs-rich-embed's rule: they run when
`RICH_EMBED_CHROME` names a browser and skip otherwise, and CI runs them
as a normal user, never as root.

### 0.0.1: the spike (done)

`noctiluca FILE` prints a local page's headings and paragraphs. One live
test, four unit tests, and the `snapshot_cost` example.

### 0.1: reader

Use cases 1, 4 and 6. Text, links, first forms, images, history, in
reader mode, in an intuiTUIve app. Fixtures: a saved docs page, an
article and a form page.

- Headings, paragraphs, lists, tables, quotes and code read in order;
  hidden text is left out.
- A link hint follows a link to a second local page; back and forward
  return, with the address and title right.
- A form fixture: type into a field, tick a check box, choose a select
  option by key (with the PoC's duplicate-value options), submit to a page
  that echoes the query, and read the echo.
- A password field's value never appears in the model or on screen.
- A static image shows as a media region: Kitty placeholder bytes in the
  PTY output, and character art in the `Driver` screen.
- A fixture full of escape sequences, bidi overrides and control
  characters (in text, title, URL and alt text) puts none of them in the
  output.
- The F1 overlay lists the registry's actions, with reasons for
  unavailable ones, and updates while open (needs C4).
- The binary on a PTY, read through rs-rich-record's emulator: start,
  read, quit; the terminal's modes are restored.
- A snapshot-cost budget on the fixtures, reported (not failed) in CI.

Non-goal: a faithful layout. The first projection may be rough; reader
mode is one key away.

### 0.2: forms and tables

Use cases 2 and 3. Fixtures: a login-style form, a table-heavy dashboard
and a settings page.

- Every web widget commits through the guarded commit, and a value the
  page changed under a draft is caught as a conflict, never overwritten.
- Wide and long tables scroll in both directions with their headers kept.
- A login-style form signs in to a fixture that checks the posted fields.

Non-goal: custom widgets with poor accessibility. They read as what their
AX tree says, and fall back to their region.

### 0.3: layout projection and GIFs

Use cases 2 and 4.

- Flex, grid, columns and table fixtures project with columns side by
  side, checked as cell positions.
- The layout invariants hold on every fixture: no text shares a cell,
  reading order holds within a column band, controls stay in their boxes.
- A sticky header stays pinned while the page scrolls.
- `demo/css.html`'s local GIF shows at least two different frames within
  one second, within the frame budget; freezing stops the bytes.
- The raster view shows the page from the screencast.
- Per-region fallback: a canvas fixture shows as a region inside
  projected text.

Non-goal: pixel-exact layout. Cells are not Chrome's glyphs.

### 0.4: media: video and audio

Use case 5.

- The PoC's local `motion.webm`: play, pause, seek and mute through the
  actions; `currentTime` moves; frames change; the region's frame rate
  stays under its cap (counted output bytes over two seconds).
- Over a slow simulated terminal, video steps down to character art.
- Audio with `--audio`, Linux CI only: the PoC's ALSA file-sink test.
  Playing is non-zero, muted is zero, 30% is quieter than 50%, paused is
  zero.
- The about page shows the codec table, and the test checks it matches
  `canPlayType`.

Non-goal: DRM playback, ever.

### 0.5: automation API

Use cases 7 and 8.

- A script drives a flow on a fixture site through the page model and
  the action registry, the same actions a person runs.
- The same session is viewable live while the script runs.
- A page snapshot as text, stable enough to diff in CI.

Non-goal: getting past bot checks, CAPTCHAs or sign-in walls for a
script.

### Never

DRM playback, extensions, getting past bot checks, CAPTCHAs or sign-in
walls, changing browser policy, sending chat messages for the user.

## Risks

- **Site breakage.** Chrome runs the page, so most sites work. The risk is
  in our reading of it: custom controls with poor accessibility, canvas
  apps, shadow DOM and cross-origin iframes. Mitigations: per-region
  fallback, reader mode, the raster view. AX trees vary in quality from
  site to site.
- **The projection on real pages.** Proportional fonts against cells,
  overlapping layers, carousels and transforms will produce some ugly
  layouts. Reader mode is the safety net. The invariant tests keep it from
  getting worse.
- **Policy and DRM.** Sites may block headless Chrome or show bot checks.
  Noctiluca stops there and says so. DRM sites do not play. The PoC hit
  YouTube's "confirm you're not a bot" and stopped; so will we.
- **Running as root.** Chrome will not start sandboxed as root, and
  rs-rich-embed will not turn the sandbox off. Containers and CI must run
  Noctiluca as a normal user, with user namespaces allowed (Ubuntu 24.04
  restricts them through AppArmor, which can stop Chrome's sandbox).
- **Performance.** Snapshot cost grows with the page. A busy page can
  change every frame. The debounce, region-only frames and a byte budget
  bound it, but news sites must be measured.
- **Page JavaScript.** It runs in Chrome's sandbox, not in our process.
  Our isolated-world script is small and only pings. The DevTools pipe is
  private on Unix, and a local port on Windows.
- **Downloads.** They are quarantined and ask first. They are never run
  or opened, and their names are cleaned.
- **Terminal injection.** Every string from the page is cleaned before it
  is drawn.

## Name and mascot

**Noctiluca** is the "sea sparkle" plankton that makes waves glow at
night. The logo is a bloom of tiny terminal windows. Each glowing cell is
a small terminal, linked to its neighbours, like light spreading through
dark water. The mascot is a lanternfish whose lamp is the terminal itself:
a small glowing terminal window held out on its lure.

Both can be drawn with what the repo has:

- **As intuiTUIve's mascots are.** Memo the elephant
  (`docs/assets/intuituive/mascot/`) is a set of SVGs for the docs plus a
  32 x 32 pixel grid. In a terminal the grid becomes a `pixels` buffer or
  a `sprite_sheet_bytes` sprite on a `stage`. The lanternfish would follow
  the same pattern: an SVG set, a pixel grid in the mark's colours, and the
  lure's terminal window as the one lit cell.
- **The bloom with rs-rich-diagram.** Each cell is a node (a small
  rounded box) and each link is an edge. `Diagram` draws it in box
  drawing, at any width, for the README, the help screen and `--version`.
- **A splash and a loading animation with micro assets.** A micro asset
  can be animated (`status/loading` and `fun/campfire` are). The bloom's
  cells light up one after another while a page loads; the lanternfish's
  lure blinks while idle. `rich micro create` packs both, at 4 x 2 cells
  for the fish, like Memo's.

This is identity, not architecture. It needs no new code.

## What only a networked machine can verify

- Real sites: news pages, search, video sites, live streams, chat pages.
  Their snapshot cost, their AX quality, and how the projection reads them.
- Consent dialogs, sign-in walls and bot checks: that Noctiluca stops at
  them and says so.
- H.264 and AAC streams over MSE and HLS from real CDNs, and adaptive
  bitrate.
- Webfonts from CDNs and their effect on the projection.
- Cross-origin iframes and embedded players from real sites.
- Downloads from real servers, including redirects and
  `Content-Disposition` names.
- TLS through a corporate proxy.
- Physical audio devices, and the SSH case where the sound stays on the
  server.
- Native terminals: Kitty, Ghostty, WezTerm, iTerm2, foot, and Sixel over
  real SSH links.

## Decisions

Answered by the user on 2026-10-25:

1. **The engine trait lives in `noctiluca`**, with an adapter over
   rs-rich-embed's `ChromeEngine`.
2. **intuiTUIve core takes C1 to C4**, with the APIs above, in 0.0.22
   (intuiTUIve 0.0.7). C5 stays in `noctiluca`.
3. **rs-rich-embed gains switches** for audio and for allowing downloads,
   both off by default.
4. **A kept profile is opt-in only**; the default profile is temporary.
   Bookmarks live in a file of Noctiluca's own.
5. **The system Chrome is preferred** when one is installed, for codecs.
6. **CI installs a browser** and runs the live tests as a normal user.
7. **The PoC is the user's own**, so its fixtures and its `dom.js` logic may
   be copied, as long as what is copied fits intuiTUIve's architecture:
   generic pieces go to core or embed, the web adapters compose stock
   widgets, and nothing domain-specific enters intuiTUIve core.
8. **`noctiluca` is reserved soon**: it joins `RELEASES.toml` and the release
   tooling, and its 0.0.1 can be published on its own crate tag once it is
   on `main`.
9. **The projection opens by default**; reader mode is one key away (not
   answered; the recommendation stands until the user says otherwise).
