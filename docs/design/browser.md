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

- **Done on `022-browser`** (pushed to
  `claude/next-priority-items-3c5p8e-wip-browser`): the reader phase,
  0.1, as `noctiluca` 0.0.1, and rs-rich-embed's events and switches.
  Item by item, with what is short, in [0.1: reader](#01-reader).
- **The web widgets run on intuiTUIve's C1 to C4** (merged from ws2):
  `button_of`, `checkbox_of` and `switch_of` with live labels,
  `select_field_keyed` and `radio_keyed` keyed by the options' own nodes,
  and F1 is intuiTUIve's live `action_overlay`.
- **Sound works** (`--audio`): verified leaving Chrome through an ALSA
  sink, not on a speaker. See [Audio](#audio).
- **The first projection** is the default view: boxes at Chrome's
  positions, rewrapped, pushed down by a skyline per column, grown to the
  right into free cells. Two-column and flex layouts stay side by side.
  No fixed or sticky layer, no paint order, media as framed alt text:
  0.3's work.
- **Scrolling (0.0.23, branch `023-scroll`)**: both text views run on
  intuiTUIve's `scroll_virtual`, so only rows near the view are built and
  there is no row cap. Chrome's scroll follows ours, near the end Chrome
  is scrolled to its bottom so feeds load more, the row in view keeps its
  place across readings (anchored by node), the layout is cached, a cut
  page says so, and the screencast is off. See
  [rendering performance](render-performance.md#what-landed).
- **Next**: 0.2, forms and tables (a login form, a dashboard and a
  settings page as fixtures); rs-rich-embed's faster loop and several
  pages (tabs).

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

### No site-specific code

A rule of the maintainer's (2026-10-25). Nothing in Noctiluca checks for a
site: no host names, no per-site selectors, no "this is a chat" case.
Everything comes from generic web semantics, so any page built from the
same constructs reads the same way:

- **pictures** from `<img>`, `<picture>` and CSS background images;
- **video and audio** from `<video>` and `<audio>`, with their poster,
  controls and play state;
- **side by side** from grids and flex rows, by Chrome's geometry;
- **live updates** from DOM mutations plus ARIA `role=log` and
  `aria-live`: any live log follows its new entries and sticks to its
  bottom;
- **inputs and buttons** from their roles and names.

The user's own site keys (`[site."…"]` in the config) are the user's
settings, not code about a site. Fixtures are test pages named by what
they exercise (`news.html`, `media-grid.html`, `stream-with-log.html`),
in ordinary HTML.

Every cell and pixel comes from our own stack: the model and projection,
drawn by intuiTUIve's nodes and painter, pictures encoded by rs-rich-art
and intuiTUIve's `art` feature. Chrome supplies the DOM, the layout,
resource bytes and, while a video plays, cropped screencast frames. No
external converter (the PoC's chafa) and no captured page stands in for
rendering.

### Terminal adaptations

Optimising for the terminal is welcome when it is generic: a named rule
about constructs and cells, applied to every page alike, with a test on a
fixture.

Each rule has a name (`crate::adapt`), and each can be switched off on
its own: `[adapt]` in the config (`collapse-nav = false`), `--adapt` on
the command line (`--adapt=-decorative-images,-collapse-nav`,
`+text-over-pictures`), and `--no-adapt` for all of them. A key (`a`)
toggles between the adapted and the **faithful** projection, and the
about page (`?`) lists the rules and which are on. A rule is on by
default unless it changes what the page means. The projection reads one
`Adapt` set through `Marks::adapt`; the rules applied where pictures are
drawn and by the media scheduler read the same set.

| Rule (name) | What it does | Default | Applied by | Test |
|---|---|---|---|---|
| Decorative pictures drop (`decorative-images`) | A spacer or tracking pixel (under 8 x 8 CSS pixels) always drops; a picture marked `alt=""`, `aria-hidden` or `role=presentation` drops only when it is icon-sized (at most 16 CSS pixels tall, under a row). An empty-alt picture of real size (a card's thumbnail, a comment's avatar, titled by the text beside it) stays, drawn without a caption | on | layout | `adapt.rs`: `decorative_pictures_drop_and_real_ones_stay` |
| Small pictures stay inline (`inline-pictures`) | An image at most two rows tall inside a line of text (an emote, a badge, an icon) is drawn in its line, at its cells. A live log's entries carry theirs as `Run::picture` | on | pictures (A) | (in progress) |
| Picture heights are capped (`cap-pictures`) | A picture is at most `MAX_PICTURE_ROWS` (14) rows, narrowed to keep its shape; what is under it in its column moves up by the rows saved | on | layout, pictures | `adapt.rs`: `tall_pictures_are_capped_and_what_is_under_moves_up` |
| A caption under each picture (`captions`) | A picture with alt text, at least three rows tall, and no `<figcaption>` gets an `Image: …` line under it | on | layout (A) | A's |
| Buttons as `[ label ]` (`icon-buttons`) | A button reads as its accessible name in brackets, an icon-only one too; a face wider than its box pushes the controls after it on its row. Off, an icon-only button is a shaded box, as its picture would be | on | layout | `adapt.rs`: `icon_buttons_read_as_their_name` |
| Grids fit the terminal (`snap-grids`) | A grid's (or a wrapping flex row's) items get equal cell widths that tile it, and when a column would be under `MIN_GRID_CELLS` (16) cells, fewer columns, the items flowing into more rows; what is under the grid moves by the rows it grew | on | layout | `adapt.rs`: `grids_snap_their_columns_to_the_cells` |
| Navigation collapses on narrow terminals (`collapse-nav`) | Under `NARROW_COLUMNS` (100), a navigation or complementary landmark with no field and no live log in it becomes one row: its name, then its links (each still a hint target). A sidebar gives its columns to what is beside it | on | layout | `adapt.rs`: `navigation_collapses_to_a_row_on_a_narrow_terminal` |
| Text over pictures of text (`text-over-pictures`) | A picture with alt text inside a heading, a link or a button, or a banner (four times wider than tall, at most three rows), is drawn as its alt text in one row | off: it changes meaning | layout | `adapt.rs`: `a_picture_of_text_reads_as_its_text_when_asked` |
| A live log shows its tail (`live-tail`) | A `role=log`, `aria-live` or appended scroll box shows its newest entries that fit, and holds still while the user has scrolled it up (`[`, `]`); off, it shows from the entry Chrome shows first | on | layout | `logs.rs`, `live_logs.rs`, `app.rs` |
| Page colours, readable (`page-colors`) | Every run keeps the page's colour as CSS. A saturated colour (a red "LIVE", an author's name) is drawn, mixed toward white on a dark terminal or toward black on a light one until it reads; a grey, or a colour near black or white (chroma under 0.25), takes the terminal's own foreground. The background is dark unless `COLORFGBG` says light. Links keep their link style. Off, no page colour is drawn | on | layout | `adapt.rs` unit tests; `logs.rs`: `authors_keep_their_colour` |
| Frame rates and tiers follow bandwidth (`bandwidth`) | Each moving region's frame rate comes from a byte budget, at most 8 a second, and it steps down Kitty, iTerm2, Sixel, then cells when short | on | media scheduler (B) | B's |

The faithful projection (`--no-adapt`, or `a`) is checked against the
adapted one on the fixture: `adapt.rs`,
`no_adapt_gives_the_faithful_projection`.

### Live logs

Built in 0.0.23. Any live log, by generic semantics only: a box with
`role=log` (or the AX role `log`), an `aria-live` box (`polite` or
`assertive`) whose content is element entries (a live box of plain text,
a status line, is ordinary text), or a scroll box the page appends
element children to. A log's entries are the element children of its
entry box: the log itself, or the one box it wraps them in (a scroller
inside a panel). A log built from `<ul aria-live>` and one from
`<div role=log>` read and project alike (`log-ul.html`, `log-div.html`).

- **The live ping.** The isolated world's `MutationObserver` sorts each
  batch of changes. When every change is inside a log the tab has read,
  it pings `'live'` instead of `''`, and moves the revision of each entry
  that changed. A picture loading in a log is a live change too. A scroll
  box the page appends to becomes a log at the next full reading.
- **Incremental reads.** On a live ping the tab reads only its logs
  (`Engine::read_live`): in the same isolated world, a function returns
  the entries' keys in order and the runs of only those whose revision
  the tab has not seen. Entries are keyed by node, never by text. Each
  run carries its colour when it differs from its entry's (an author's
  name), and each picture its source, alt text and size. A field's value
  inside an entry is never read. No snapshot is taken: measured on
  `stream-with-log.html`, a new entry shows in well under 600 ms with no
  full reading (`live_logs.rs`).
- **The view.** The projection gives a log its own box, at its rows, and
  shows its tail (the `live-tail` rule). `[` scrolls it up, by half its
  rows in entries, and it holds still on the entry at its bottom while
  new ones come; `]` scrolls down, and at the end it follows again.

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

To know when to read again (built: `cdp.rs`'s `PING_SCRIPT`), one small
script runs in an **isolated world** (`Page.createIsolatedWorld`), not the page's world: a single
`MutationObserver` and media event listeners, which call a binding
(`Runtime.addBinding` with `executionContextName`) as a "something
changed" ping. Noctiluca debounces the pings (about 100 ms, and at once
after an action of ours) and takes a new snapshot. The page cannot see an
isolated world's globals or the binding. It can still notice extra work
through timing; that is accepted. Measured: a binding's calls are
reported only while the `Runtime` domain is enabled, so Noctiluca enables
it. Some bot checks look for an enabled `Runtime`; that is accepted (decided
in 0.0.22), and the README says so.
The script also samples the form fields' values every 500 ms (passwords
left out), as the PoC did, so a value a page script sets with no event
is still seen.

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
   sends). Built: 8 x 18 CSS pixels a cell, narrower than rs-rich-embed's
   10 x 20, so a proportional 16 px font's average glyph is about one
   cell. Reading the terminal's own cell size is still to do.
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
  snapshot covers every layout box. Scrolling is ours, in rows. Chrome's
  scroll position follows once the view rests (120 ms): a move of up to
  three screens as a turn of the wheel (`Input.dispatchMouseEvent`,
  `mouseWheel`), a longer one as `window.scrollTo`, each row placed at
  its box's top on the page. `Page.getLayoutMetrics` is read after each
  scroll. So lazy loading and infinite scroll fire, and near the end of
  the projection Chrome is scrolled to its bottom and the page read again
  on its change ping.

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

**The picture API (0.0.23).** Picture handling is rs-rich-art's
(`rich_art::picture`, its `picture` feature): decoding, fitting, every
protocol's encoding, a picture's identity, byte accounting and the step
down. intuiTUIve's `art` feature holds only the node and painter glue.
Noctiluca only decides what shows where.

```rust
// rs-rich-art, `picture` feature
pub enum Pixels { Cells, Kitty, Sixel, Iterm }      // detect(), name(), lower()
pub struct ImageView { mode, fit, anchor, background, pixels } // builder
pub struct Picture;                                  // Clone, Eq by (id, version)
impl Picture {
    pub fn new(image: DynamicImage) -> Picture;      // a new id
    pub fn decode(bytes: &[u8]) -> Result<Picture, String>;
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Picture>;
    pub fn animated(frames: Vec<AnimationFrame>) -> Option<Picture>; // Kitty plays it
    pub fn next(&self, image: DynamicImage) -> Picture;              // same id, version + 1
    pub fn next_rgba(&self, width: u32, height: u32, rgba: Vec<u8>) -> Option<Picture>;
    pub fn still(&self) -> Picture;                  // an animation stopped
    pub fn find(id: u32, version: u32) -> Option<Picture>;
    pub fn id(&self) -> u32;
    pub fn version(&self) -> u32;
    pub fn image(&self) -> &Arc<DynamicImage>;
    pub fn is_animated(&self) -> bool;
    pub fn frames(&self) -> Option<&[AnimationFrame]>;
    /// Cells for rows of one's own; tagged for the painter when in pixels.
    pub fn lines(&self, console: &Console, view: ImageView, width: u16, height: u16)
        -> Vec<Vec<Segment>>;
    pub fn encode(&self, pixels: Pixels, cols: usize, rows: usize) -> Option<String>;
    pub fn encode_rows(&self, pixels: Pixels, cols: usize, rows: usize,
                       first: usize, shown: usize) -> Option<String>;
    pub fn release(id: u32) -> String;
}
pub struct PictureTag { id, version, cols, rows, row, pixels } // of(&Style), style(), picture()
pub fn picture_bytes(id: u32) -> u64;               // escapes sent for a picture so far
pub fn count_picture_bytes(id: u32, bytes: u64);    // called by the painter's glue
pub struct ByteBudget { bytes_per_second, max_fps: 8.0, min_fps: 2.0 } // new(), guess()
pub struct Pacer;                                    // one moving picture's rate and tier
impl Pacer {
    pub fn new(pixels: Pixels, cols: usize, rows: usize) -> Pacer;
    pub fn pixels(&self) -> Pixels;
    pub fn resize(&mut self, cols: usize, rows: usize);
    pub fn estimate(pixels: Pixels, cols: usize, rows: usize) -> u64;
    pub fn fps(&self, budget: &ByteBudget, share: f64) -> f64;
    pub fn due(&mut self, now: Instant, budget: &ByteBudget, share: f64) -> bool;
    pub fn sent(&mut self, now: Instant, bytes: Option<u64>);
    pub fn settle(&mut self, budget: &ByteBudget, share: f64) -> bool; // stepped down?
    pub fn recent_bytes(&self) -> u64;               // the last two seconds
}
pub fn over_ssh() -> bool;
// rs-rich-art, `kitty`: raw pixels, no PNG (Doom's gap 3)
pub fn transmit_rgba(id: u32, rgba: &[u8], width: u32, height: u32,
                     cols: usize, rows: usize) -> String;

// rs-rich-intuituive, `art`: the glue
pub use rich_art::picture::{picture_bytes, ImageView, Picture, Pixels};
pub fn picture_of(picture: impl Fn() -> Option<Picture> + 'static, view: ImageView) -> Node;
pub fn image_graphics() -> Arc<dyn PlacementSource>; // finds tags anywhere, counts bytes
```

A live frame keeps its picture's id, so Kitty replaces the image it holds
(one `a=d` when the app ends), and Sixel and iTerm2 draw the new frame
over its cells. `picture_of` draws a frame at once, with no worker. A
moving picture's `Pacer` takes its share of the `ByteBudget`; where the
painter's own frame budget lands (branches 023-paint and 023-core), the
budget's `bytes_per_second` comes from the painter's measured output
rate instead of `ByteBudget::guess`.

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

**Sound plays on the machine Chrome runs on, never over SSH.** Over SSH
it plays on the server. Noctiluca says so in its README, its help and the
player ("sound is off: start with `--audio`" when it is off).

Built in 0.0.22:

- `--audio`, or `[media] audio = true`, starts Chrome without
  `--mute-audio` (rs-rich-embed's `ChromeEngine::audio`). Without it the
  browser is muted, as before.
- Each video and audio element gets controls in reader mode, from the
  element's real state, read with a call on the element in an isolated
  world: Play or Pause and Mute or Unmute (stock `button_of`, live labels,
  C1), a volume slider (the stock slider), and its time. While something
  plays, its state is read again every 500 ms.
- The registry has `media.play_pause` (`p`), `media.mute` (`Alt+M`),
  `media.volume_up` (`+`) and `media.volume_down` (`-`), in ten-point
  steps that never unmute. F1 lists them saying what they would do now
  ("Pause the media", "Unmute the media").
- `[site."…"] audio = false` keeps that site's media muted. Sound is a
  switch of the browser's launch, so a site cannot turn it on for a run
  that started without it.
- Play runs as the user's gesture (`userGesture` on the call), so a play
  the user asked for is not refused as autoplay. No autoplay policy is
  changed.

**Verified on this machine** (`tests/audio.rs`, run as a normal user,
Chrome for Testing 153). As the PoC did, the test points ALSA at a sink
of its own: an `ALSA_CONFIG_PATH` for its process and the browser it
starts, whose default output writes to a FIFO the test reads at a real
device's pace (48 kHz, stereo, 16-bit; unpaced, Chrome decodes as fast as
it can and the media clock runs away). On a local two-second 440 Hz tone:

| Check | Result |
|---|---|
| The element plays, not muted, its `currentTime` advancing | yes |
| RMS of the samples Chrome wrote, playing at 100% | 0.189 |
| Muted | 0.000 |
| At 30% | 0.058 (0.31 of full) |
| A Chrome started without `--audio`, playing the same tone | 0.000 |

**Not verified:** a physical device (loudspeakers, headphones, a
PulseAudio or PipeWire server, macOS or Windows audio), video sound, and
any live site. The sink proves samples leave Chrome, not that a speaker
plays them.

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

### Going places: shortcuts, switches and keys

Built in the reader phase.

**Site shortcuts (keywords).** `noctiluca gh rs-rich`, or `gh rs-rich`
typed in the address bar, expands a keyword's template:
`gh = "https://github.com/search?q={}"`. The query is percent-encoded
before it goes in. A keyword with no argument opens its home address
(`gh` alone opens `https://github.com`). A few are built in: `rs`
(docs.rs), `mdn`, `crates` (crates.io) and `gh` (GitHub). The config can
override or remove each. Bookmarks can carry a keyword too.

**The address bar decides in this order:** a URL (a scheme, a path to a
local file, or something that looks like a host: `localhost:8080`,
`example.com/x`), then a keyword, then a search. The search engine is
configurable. Its default is DuckDuckGo's HTML search, with no tracking
parameters added.

**Config.** A TOML file at `noctiluca/config.toml` in the platform config
directory (`$XDG_CONFIG_HOME`, else `~/.config`, on Linux; Application
Support on macOS; `%APPDATA%` on Windows). `noctiluca --config PATH`
reads another. Tables: `[shortcuts]`, `[search]`, `[keys]`, `[startup]`
(home page, default view), `[media]` (audio on or off, an fps cap),
`[profile]` and `[site."…"]`. An unknown key is a warning, not an error,
so an old config still opens. The crate ships a documented sample,
`crates/noctiluca/config.sample.toml`.

**Command-line switches,** in the hand-written style of `rs-rich-cli`'s
commands (`fn parse_args(&[String]) -> Result<Args, String>`; the CLI
has no clap):

```
noctiluca [URL | KEYWORD ARGS…]
  --reader | --projection | --raster    the first view
  --audio                               let pages play sound
  --profile NAME                        an opt-in kept profile
  --chrome PATH                         the browser to run
  --images ansi|kitty|sixel|iterm2|none how images reach the terminal
  --fps N                               the media frame cap
  --dump text|ax|json                   print the page's model and exit
  --no-js                               turn the page's script off
  --config PATH                         another config file
```

`--dump` is for scripts and CI (use cases 7 and 8). `--no-js` uses
`Emulation.setScriptExecutionDisabled`, which Chrome supports cleanly.

**Keys.** No new key system. Every action is a binding in a
`rich_interact::keymap::Keymap` (context `noctiluca`), rebound from
`[keys]` through that crate's `Overrides` (`Keymap::apply`), and bound on
the app's root with intuiTUIve's `Node::bind` and `Node::bind_sequence`
(for `g t`). Defaults: `o` open, `O` open in a new tab, `f` link hints,
`/` find, `H` and `L` back and forward, `g t` and `g T` tabs, `F1`
actions, `r` the reader toggle, `Ctrl+L` the address bar. `[keys] preset`
chooses `vim` (those) or `plain` (arrows, `Alt+Left` and `Alt+Right`,
`Ctrl+F`, `Ctrl+T`). The F1 overlay lists every binding from the same
registry that runs it, so a rebinding shows there at once.

**Per-site settings.** `[site."github.com"]` tables, matched by host. A
pattern (`*.example.com`) or a URL prefix (`"https://example.com/docs/"`)
also works, and the most specific match wins: a URL prefix over an exact
host over a wildcard, a longer one over a shorter. A site table can set
`keys` that apply only there, the `view`, `images` and `audio` on or off,
an `fps` cap, a viewport `width` in cells, and a `keyword` that searches
the current site. A site key binds to one of three things:

1. a built-in action from the registry: `media.play_pause`, `hints`,
   `reader`, `scroll.down`, `back`;
2. "activate the element with this accessible role and name":
   `{ role = "button", name = "Next page" }`, looked up in the AX tree
   when the key is pressed;
3. a sequence of those.

**Binding a key from the page (built).** `Ctrl+K`, "Bind a key on this
site…", from F1 or a key, lists the page's named controls (by role and
accessible name) and every action. Choose one, press the key, confirm:
Enter binds it on this host, `w` on `*.host`. A key that already does
something here (a site key or an action) is shown, and Enter replaces
it. A name that more than one element has is said ("3 elements are named
"Next"; this binds match 2"), and saved with `nth`; Esc refuses it. The
binding is written to `[site."<host>".keys]` through `toml_edit`, keeping
the file's comments and layout, atomically, and works at once. "Site
shortcuts…" (`K`) lists this site's keys: Enter tests one, `d` removes
it, `e` binds it to another key, `s` moves it between the host and
`*.host`. `noctiluca keys --site github.com` lists them. Not built yet:
starting a binding straight from a link hint or the focused element (the
picker lists the same elements).

There is no raw JavaScript and no CSS selector. A selector or a script in
a config file would be page scripting by another name: a shared config
could then read or post a page's data, and a selector breaks with every
redesign. A role and a name are what a screen reader user would say,
they survive restyling, and activating one is the same guarded action a
person runs by hint, re-checked against the page when it runs.

The design principle stays: the code has no per-site layout and no
hostname logic. Per-site settings are the user's own overrides over the
same semantic actions. Site keys show in the F1 overlay, marked with the
site table they came from. A binding whose element is not on the page is
listed as unavailable, with the reason.

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
4. **Screencast on demand** (done on `023-scroll`):
   `ChromeEngine::screencast(false)` and `set_screencast`. Noctiluca turns
   it off; a raster view would turn it on.
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
test, four unit tests, and the `snapshot_cost` example (now
`capture --cost`).

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
- [Going places](#going-places-shortcuts-switches-and-keys): keyword
  expansion with the query escaped; the address bar's order (URL,
  keyword, search); config parsing and its warnings; the switches; a key
  rebound in config showing in F1; site matching, most specific first; a
  site key overriding a global one on its site only; a role and name
  binding activating the right element on a fixture, and a missing one
  shown unavailable.

Non-goal: a faithful layout. The first projection may be rough; reader
mode is one key away.

**Status: done in 0.0.22**, each item tested. The fixtures are pages
written for the corpus (`crates/noctiluca/tests/fixtures`: `reader`,
`docs`, `article`, `form`, `echo`, `second`, `inject`, `audio`), not saved
real pages, so their licence is the repository's. Chrome's snapshots of
them are kept in `tests/snapshots` (re-made with the `capture` example),
so the model, layout, config and app tests run with no browser; the live
tests run the same pages through Chrome as a normal user (CI's `browser`
job), and the PTY test runs the binary over the snapshots
(`NOCTILUCA_FAKE_ENGINE`, read only with the test-only `fake-engine`
feature). Beyond the list: sound (see [Audio](#audio)),
`--dump text|ax|json`, `--no-js`, per-site view, images and width, and a
docs tape (`docs/tapes/noctiluca.tape`, over the snapshots, since the
tape job runs no browser). Short of the list: the image in the `Driver`
screen is checked as half blocks only by eye; a binding cannot yet start
from a hint or the focused element; `O` and `g t` say tabs need
rs-rich-embed's several pages; the PoC's own demo pages are not copied
in (ours cover the same ground).

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

**Drawn** (0.0.22), in `docs/assets/noctiluca/`: the logo
(`logo.svg`) and a favicon; the lanternfish as a hero, a tip pose, a
troubleshooting pose and 32 and 16 px silhouettes; a 32 x 32 pixel
lanternfish (`.txt`, `.svg`, `.png`); and `memo-amazed.svg`, intuiTUIve's
Memo wide-eyed at a page in her terminal. `ICONOGRAPHY.md` there has the
palette (blue and cyan, beside intuiTUIve's purple), the drawing rules and
the files. The guide's [Noctiluca page](../guide/noctiluca.md) uses them.
Not done yet: the bloom in rs-rich-diagram and the micro-asset animations
below.

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
