# Noctiluca on real sites, second pass: gauges, shops, search and video

**Status:** a discovery pass and a proposal, 2026-11-03, on `main` at
`7ae5afc1` (Noctiluca 0.0.4). Three small fixes landed with it (see
[Fixed with this note](#fixed-with-this-note)); everything else here is a
plan. The first pass is [Noctiluca on real sites](../guide/noctiluca/real-sites.md).

The maintainer asked (verbatim):

> Fix Noctiluca in certain areas. For example get certain sites to look
> and present better in the cli. Google / ookla speed test / amazon /
> YouTube / twitch etc. look at what other native intuiTUIve controls could
> be built to make it much better. For example ookla's speed gauge didn't
> render at all. What can we do there.

The answer, in short: the gauge does not render because **a `<canvas>` is
never captured** and **an inline `<svg>` is captured once and never
again**, so a dial drawn either way shows a grey box or its first frame.
Both are fixable inside Noctiluca with the still and screencast machinery
it already has (a *live region*). On top of that, values a page states in
ARIA (`<meter>`, `<progress>`, `role=progressbar|meter|slider`, "4.5 out
of 5 stars") can be drawn as native intuiTUIve controls, fed from the page
on every reading. The other big losses on these sites are generic too: no
fixed or sticky layer, text over pictures pushed under them, and
icon-only buttons with no face.

## How it was checked

| | What | How |
|---|---|---|
| **Live** | YouTube (home, search, a channel's videos, a watch page, an embedded player), Twitch (directory, a category, a live channel) | `noctiluca` 0.0.4 (release build of `main`) in `rich record` tapes at 120×40, `--images kitty`; `noctiluca --dump json`; a scratch example that saves Chrome's snapshot and prints the model and the projection |
| **Fixtures** | A speed gauge drawn as SVG and on canvas, meters, a search results page, a shop's product grid, an embedded-player root | New pages in `crates/noctiluca/tests/fixtures/`, opened through real Chrome (`file://`) the same way |
| **Not reachable** | Google, speedtest.net, Amazon | Refused by this machine's network policy (the proxy answers 403). Nothing below about them is from the live site; it is from their general shape, reduced to fixtures |

Chrome for Testing 1243 (Playwright's build), headless, run as root
through a wrapper that adds `--no-sandbox` (Noctiluca itself never does;
see [Privacy and security](../guide/noctiluca/privacy.md)).

The captures are not committed (live pages, other people's content).
They were written to the session's scratch directory, `noctaudit/`:
`yt/` (YouTube), `tw/` (Twitch and the embedded player), `fx/` and `fx2/`
(the fixtures), each as PNG, SVG and text screenshots, plus
`yt-*.json` and `fx-*.json` dumps and Chrome's own screenshot of the
embedded player (`yt-embed-chrome.png`). To record them again, the tapes
are in `noctaudit/tapes/`; they need a `chromium` on the tape's `PATH`
(`rich record --bin-dir DIR` with `noctiluca` and `chromium` in `DIR`).

## What was seen

### YouTube (live)

- **Search results** read well: the filter chips as buttons, each result's
  thumbnail in Kitty graphics where the page puts it, the title, channel,
  views and age beside it. Wrong: every thumbnail has the caption
  **"Image: true"** (YouTube binds a boolean to the `<img>`'s `alt`;
  [fixed](#fixed-with-this-note)); two text runs set side by side collide
  into one ("16" views and "13y ago" as **"1613y ago"**); a narrow
  column breaks "7.3K" into "7.3" over "K".
- **A channel's videos**: two thumbnails per row, but only the first row
  has pictures; the rest are blank rows with no placeholder until
  scrolled. Each thumbnail's duration badge (HTML over the picture) is
  pushed **under** the picture as a separate link, and every card has a
  hover-only **[More actions]** button in full. The channel's own header
  (banner, name, subscribe) is not in view at row 1.
- **The watch page** could not be seen: from this machine YouTube redirects
  every `/watch` to Google's `/sorry` bot check, which the network policy
  refuses, so Noctiluca shows Chrome's *This site can't be reached* for
  `www.google.com`, with the address bar reading `chrome-error://chromewebdata/`
  rather than the address that was asked for. The 27 October recording
  shows the same page behind the "not a bot" wall.
- **The embedded player** (`/embed/ID`) showed **an empty page**, where
  Chrome shows "Video player configuration error, Error 153" with two
  buttons. Cause: `html { overflow: hidden }` over an absolutely placed
  body gives the root a 0-pixel box, which `conceals`
  (`crates/noctiluca/src/model.rs:884`) took for a visually hidden box,
  hiding every descendant. [Fixed](#fixed-with-this-note); after the fix
  the text and both buttons show, but the player's own hidden error panel
  and the interstitial over it both show (no paint order).
- **Home** for a fresh profile is only YouTube's "Try searching to get
  started": not a Noctiluca problem.

### Twitch (live)

- **A live channel plays**: the stream's frames in the player's region,
  `PLAYING 0:09 / live` in the status bar. Around it:
- **Fixed layers drawn in the flow.** The cookie banner, fixed to the
  bottom of the screen in a browser, is drawn at the top of the page over
  the navigation; the "Join the Twitch community!" sign-up bar, also fixed
  to the bottom, lands in the middle of the category page.
- **The player is drawn twice as tall.** A large "?" picture the page puts
  over the player (its offline/loading art) takes the player's rows, and
  the stream's frames come under it.
- **The top bar is cut at the right edge**: `[Prime off`, `[Lo`, `[Sig`,
  `[Use` (icon buttons with long accessible names, clipped by the screen,
  with no `…`). A notification count ("30") floats on its own row.
- **Icon-only buttons have no face**: `[  ]` on the category page (an SVG
  icon, no text, no name in the model).
- **Chat** shows "Welcome to astronomiemuseum's chat room!" and "Animated
  Emotes can be disabled in Settings" (a tooltip card with [Close] and
  [Show me]); the radar channel had no chat lines while recorded, so
  following new messages was not exercised.
- **Stream info** (LIVE, channel name, category links, title, viewer
  count with a red "4", uptime 6:10:46, Follow) reads in order under the
  player, and "About" with followers follows. The channel's avatar's
  caption wraps one letter-group a row ("astronom" / "iemuseum").

### The speed gauge (fixtures `gauge-svg.html`, `gauge-canvas.html`)

speedtest.net could not be opened from here, so this is the pattern, not
the site: a dial with tick labels, an arc that grows, a needle, the
number inside the dial, and usually the same number again in HTML.

- **SVG dial**: drawn, from Chrome's pixels of its box, but **frozen at its
  first frame**: at the end of the test the dial still says `0.00` with
  the needle at rest, while the HTML readout beside it says `287.40`.
  Screenshots `fx/gauge-svg-early.png` and `fx/gauge-svg.png`.
- **Canvas dial**: three **grey boxes**; the one with `role=meter` and
  `aria-label="Upload"` shows the word "Upload" in its box. Nothing of the
  dial or its number, at any time (`fx/gauge-canvas.png`).
- The HTML number beside a dial is read and follows the page.

So "the gauge didn't render at all" fits a canvas dial exactly, and an
SVG dial would look broken (stuck at zero) rather than absent.

### Meters (fixture `meters.html`)

Of seven ways a page says "a value in a range", one survives:

| Page | Shown |
|---|---|
| `<label for=disk>Disk</label> <meter value=72>` | **nothing, not even "Disk"** |
| `<label for=upload>Upload</label> <progress value=10 max=100>` (moving) | **nothing** |
| `<progress>` with no value (busy) | **nothing** |
| `role=progressbar aria-valuenow=25` on a styled bar | "Download" with no bar and no value |
| `role=meter aria-valuetext="3 of 5 bars"` | its glyphs "▂▄▆", the text value lost |
| `role=slider aria-valuenow=40` | `Volume ◀ 40 ▶` (works: a control) |
| `role=img aria-label="4.5 out of 5 stars"` on a styled `<i>` | **nothing** (no picture, no text) |

### Search results (fixture `search-results.html`)

- The results themselves read well: site and breadcrumb, the title link,
  the snippet with its bold words, the side panel beside them, the
  "People also ask" questions as buttons.
- **The sticky header lands in the middle of the page**: the logo and the
  search box are drawn below the first result, pushing its snippet down
  by four rows. Cause below ([sticky and fixed boxes](#root-causes)).
- The breadcrumb's site name wraps a word a row ("Harbour" / "Office")
  because the favicon and name are squeezed into Chrome's 43-pixel box.

### A shop's product grid (fixture `product-grid.html`)

- The three product cards sit side by side with their pictures, titles,
  and the star bar drawn (Chrome's pixels of the inline SVG); filters as
  check boxes; the department select and the search field in the header.
- **Prices read as "$2999"**, "$2449 $34.99", "$4900": shops show a price
  in parts for looks (`$`, `29`, superscript `99`, `aria-hidden`) and put
  the whole price, `$29.99`, in visually hidden text for screen readers.
  Noctiluca drops the visually hidden text and keeps the parts.
- **The star rating's words are lost**: the SVG's name, "4.5 out of 5
  stars", is neither drawn nor said; only an orange bar shows.
- **The "Customers also viewed" shelf became a live log**: six items that
  scroll sideways were drawn as a vertical list with tiny pictures, because
  the parser appending them looked like a page appending chat entries.
  [Fixed](#fixed-with-this-note).
- The cookie banner, fixed to the bottom, is drawn in the page's flow.
- Sponsored labels and alt text captions under every product picture add
  two or three rows a card ("Image: Tidewell 1.7 litre kettle, …" repeats
  the title under it).

## Root causes

| # | Problem | Where | Why |
|---|---|---|---|
| R1 | A canvas is never drawn | `crates/noctiluca/src/tab.rs:673` | `capture_stills` takes stills of `Image`, `Video` and `Svg` regions only; a `Canvas` keeps `still: None` and the projection draws its `░` placeholder (`layout.rs:1200`) |
| R2 | An SVG drawing never updates | `tab.rs:1166` (`still_key`), `tab.rs:658` | The still is keyed by node, source (empty for inline SVG) and box. A dial's subtree changes with its box unchanged, so the cached still is reused at every reading |
| R3 | A canvas's changes are never noticed | `cdp.rs:194` and the ping script | Drawing on a canvas changes no DOM, so no mutation pings; only a re-read for another reason could notice, and R1 stops it anyway |
| R4 | Text inside an SVG is lost | `model.rs:1368` | `<svg>` returns as a media region before its children are walked; the dial's `<text>0.00</text>` never reaches the model, the reader or `--dump` |
| R5 | `<meter>` and `<progress>` vanish with their label | `model.rs:1495`, `model.rs:188` | A `<label for=…>` is made silent on the assumption that it names a control that will show the name; `meter`, `progressIndicator` and `progressbar` are not roles Noctiluca knows (`Role::from_ax`), so neither the label nor the value is drawn |
| R6 | `role=img` on a non-media element is lost | `model.rs` media classification by tag only | Only `IMG`, `SVG`, `CANVAS`, … become media; an `<i role=img aria-label=…>` with a CSS background is neither text nor media |
| R7 | Sticky and fixed boxes land mid-page | `app.rs:2195` (`load_more`), `tab.rs:986`; no use of `position` in `layout.rs` | A page shorter than two screens is "near its end" at once, so Chrome is scrolled to its bottom. The snapshot then reports a sticky or fixed box where it is *on screen now*, and the projection places it at that page offset. Even unscrolled, fixed boxes are drawn in the flow: there is no pinned layer (a known limit since 0.0.22) |
| R8 | Text over a picture goes under it | `layout.rs` placement (the planned "a text box painted above an opaque one is drawn over the region", `design/browser.md`) | Not built: the skyline pushes the badge below the picture |
| R9 | A11y-split prices read wrong | `model.rs:884` (`conceals`) and the hidden-text rules | Visually hidden text is dropped (right for "skip to content" hints) even when its visible twin is `aria-hidden` and reads worse |
| R10 | Icon-only buttons have an empty face | `layout.rs` faces (`face`, `compact_face`) | A button whose only content is an SVG icon and whose name is empty in the model gets `[  ]`; `title`, an inner `<title>` or `aria-labelledby` text is not tried |
| R11 | Controls at the right edge are cut | projection growth "into free cells beside it" | A right-aligned run of icon buttons with long names has no free cells to the right; they are clipped by the screen, not shortened or moved left |
| R12 | A whole page empty (embedded player) | `model.rs:884` | See [Fixed](#fixed-with-this-note) |
| R13 | `alt="true"` captions | `model.rs` `media` | See [Fixed](#fixed-with-this-note) |
| R14 | A carousel read as a chat | `cdp.rs:194` (`sort`) | See [Fixed](#fixed-with-this-note) |
| R15 | Two layers drawn at once | no paint order (known limit) | The embedded player's hidden error panel and the interstitial above it both show; Twitch's offline art and the stream both take rows |
| R16 | Address bar shows `chrome-error://chromewebdata/` | navigation error path | A failed navigation shows Chrome's internal address, not the one asked for |

## Fixed with this note

Each with a test; the commit is on `claude/next-priority-items-3c5p8e-wip-noctaudit`.

1. **A root that hides its overflow** no longer hides the page
   (`conceals`, `model.rs:884`): `<html>` and `<body>` never count as
   visually hidden by their overflow, whose clip is the viewport's (CSS
   Overflow 3). Test: `tests/model.rs`
   `a_root_that_hides_its_overflow_over_a_placed_body_still_shows_the_page`;
   fixture `root-overflow.html`. The YouTube embed now reads its error
   text and both buttons.
2. **A script's value is not alt text** (`script_value`, `model.rs:860`):
   `true`, `false`, `null`, `undefined`, `NaN` and `[object Object]` count
   as no text alternative. Test: `a_script_value_for_alt_text_is_no_alt_text`.
3. **A sideways shelf is not a live log** (`cdp.rs:194`): appends while
   the document is still loading (the parser's) and boxes that overflow
   only sideways no longer make a log. Test: `tests/live.rs`
   `a_shelf_that_scrolls_sideways_is_not_a_live_log` (real Chrome);
   fixture `product-grid.html`. The live-log tests still pass.

## The fixes, ranked

Impact is how much of what people see on these sites it changes; effort
is S (a day or less), M (a few days), L (a week or more).

| Rank | Fix | Sites | Impact | Effort | Lands in |
|---|---|---|---|---|---|
| 1 | **Live regions**: canvas captured, SVG and canvas re-captured when they change, within the byte budget ([design](#live-regions-canvas-and-svg)) | the gauge, charts, maps, players' art, any canvas UI | high | M | Noctiluca (`tab.rs`, `app.rs`), rs-rich-embed (nothing new) |
| 2 | **Sticky and fixed layer**: read `position`, pin top bars, dock bottom banners, keep fixed boxes out of the flow ([design](#sticky-and-fixed-layers)) | all five; search's header, Twitch's banners, shop's header | high | M | Noctiluca (`model.rs`, `layout.rs`, `app.rs`) |
| 3 | **Indicators as native controls**: `<meter>`, `<progress>`, `role=progressbar|meter`, value text, fed live ([design](#indicators-as-native-controls)) | speed tests, uploads, players' buffers, dashboards | high | M | Noctiluca model + intuiTUIve `dial` (new, core) and `progress_bar` (exists) |
| 4 | **Text over pictures**: a short text box painted over a picture is drawn inside the picture's cells (badges: duration, LIVE, viewers, prices on images) | YouTube, Twitch, shops | medium-high | M | Noctiluca (`layout.rs`, `pictures.rs`) |
| 5 | **Labels of things that are not controls**: a `<label for>` whose target is not a control stays text (R5) | forms, meters | medium | S | Noctiluca (`model.rs`) |
| 6 | **SVG text and names**: an `<svg>`'s own `<text>` (and `<title>`) read into the model as its text, shown in reader mode and `--dump`, and used as alt text when it has none (R4) | the gauge's number, chart labels, logos | medium | S | Noctiluca (`model.rs`) |
| 7 | **Screen-reader twins**: when a visually hidden run and an `aria-hidden` run are siblings saying the same thing, keep the hidden one (`$29.99`) — a named adaptation, `a` shows the faithful form (R9) | shops, prices, dates ("2 days ago" / exact date) | medium | S | Noctiluca (`model.rs`, `adapt.rs`) |
| 8 | **Ratings**: `role=img` (any element) with a name of the form "N out of M stars" draws ★★★★½ and says the number (R6) | shops, app stores, reviews | medium | S | Noctiluca mapping + intuiTUIve `rating` (new, core, tiny) |
| 9 | **Icon-only button faces**: name from `title`, an inner `<title>`, `aria-labelledby`; else a glyph by the icon's name (`⋯` for "More", `✕` for "Close", `🔍` for "Search") and the name in the hint and `F1` (R10) | Twitch, YouTube, every app-like site | medium | S | Noctiluca (`layout.rs`) |
| 10 | **Right-edge controls**: a run of controls that does not fit grows to the left or takes compact faces with `…`, never clipped (R11) | Twitch's and YouTube's top bars | medium | S | Noctiluca (`layout.rs`) |
| 11 | **Carousels and shelves as a native strip**: a box that scrolls sideways shows the items in view, `‹ 1–3 of 12 ›`, and pages with keys and the wheel (Chrome's scroll follows) ([design](#carousels-and-shelves)) | YouTube shelves, shops, Twitch rows | medium | M | Noctiluca mapping + intuiTUIve `carousel` (new, core) |
| 12 | **Paint order and occlusion**: a box covered by an opaque box above it (by the snapshot's `paintOrders`, which `cdp.rs:628` already asks for and the model does not read) is not drawn (R15) | players, modals, interstitials | medium | M | Noctiluca (`model.rs`) |
| 13 | **Hover-only controls**: controls whose ancestor is `opacity: 0` until hover ("More actions") fold into the card's `F1`/`Ctrl+K` list instead of drawing | YouTube, shops | low-medium | S | Noctiluca adaptation |
| 14 | **Inline runs that collide** ("1613y ago"): two adjacent text boxes snapped to touching cells get a space between them | YouTube metadata | low-medium | S | Noctiluca (`layout.rs`) |
| 15 | **Navigation errors keep the asked address** in the address bar, with the error page's text (R16) | any blocked site | low | S | Noctiluca (`tab.rs`) |
| 16 | **Lazy thumbnails get a placeholder** with their alt text or title while they load, instead of nothing | YouTube channel pages | low | S | Noctiluca (`model.rs`: an `<img>` with no source yet but a box) |

## Designs

### Live regions: canvas and SVG

The pieces exist: stills from `Page.captureScreenshot` with a clip
(`picture_bytes`, `tab.rs:718`), the screencast cropped per region for
video (`video.rs`), the byte budget the pictures and video share, and the
tiles of the pixel view (`pixel_view.rs`, which sends only the 8×16
tiles that changed). What is missing is deciding *when* a canvas or SVG
needs new pixels.

- **SVG** changes the DOM, so a reading sees it. Give each media region a
  `revision`: for an `<svg>`, a hash of its subtree's attributes and text
  as the snapshot already reports them (no new CDP call). `still_key`
  takes the revision, so a changed drawing is captured again at the next
  reading, nearest the view first, eight a reading as now. A dial moving
  three times a second costs one clipped screenshot each time, a few kB.
- **Canvas** changes nothing in the DOM. While a canvas is on screen, the
  tab polls it: a clipped screenshot at a low rate (start at 2 a second,
  up to the region's share of the byte budget), hashed, dropped when
  unchanged. A canvas that has not changed for 10 polls drops to one
  poll every 2 s; one that changes every poll for 2 s and is larger than
  a quarter of the viewport is handed to the screencast path, as a playing
  video is. Off screen, no polls. `F4` (freeze) stops them as it stops
  GIFs. WebGL canvases are the same.
- **Drawing** is what pictures do today: Kitty, iTerm2 or Sixel when the
  terminal has one, else half blocks (Braille is worse for a dial than
  half blocks: two colours per cell). Kitty tiles: a changed still is
  diffed against the last in the 8×16 tiles the pixel view uses, and only
  changed tiles are sent, so a needle moving costs a few tiles.
- **Text**: the drawing's own `<text>` (R4) goes into the model as the
  region's text, so reader mode, `--dump` and screen readers get "287.40
  Mbps" even where pictures are off. A canvas has none; see the next
  section for when the page says it in ARIA.
- **Vector to cells** (rendering the SVG ourselves as Braille or box
  lines) was weighed and left out: Chrome already rasterises every SVG
  feature (filters, text, gradients, CSS); a converter of our own would
  cover a fraction and be a second renderer to keep. The one case where
  cells beat pixels — a terminal with no graphics at all and a small dial
  — is covered better by the native dial below when the page gives a
  value, and by half blocks otherwise.

Tests: `gauge-svg.html` (the still changes between two readings; its text
is "287.40" at the end), `gauge-canvas.html` (a canvas gets a picture; it
changes while the script draws and stops when it stops), on the fake
engine for the keying and on Chrome for the capture.

### Indicators as native controls

A new model kind, generic web semantics only:

```rust
/// A value in a range the page states (not a control: it is not
/// changed by the user).
pub struct Indicator {
    pub node: NodeId,
    pub kind: IndicatorKind,   // Meter, Progress, Busy (progress with no value)
    pub label: String,         // its accessible name, or its <label for>
    pub value: Option<f64>,    // aria-valuenow / value
    pub min: f64,
    pub max: f64,
    pub text: Option<String>,  // aria-valuetext, or the element's text
    pub rect: Rect,
    /// low / high / optimum of a <meter>: its colour band.
    pub bands: Option<(f64, f64, f64)>,
}
// BlockKind::Indicator(usize)
```

Read from: `<meter>`, `<progress>`, and any element whose role is
`progressbar` or `meter` (DOM attribute or AX role `progressIndicator`,
`meter`), including a `<canvas>` or `<svg>` given that role (then the
region is still captured, and the indicator is drawn beside it or instead
of it, by an adaptation). The value is re-read at every reading; a
progress bar moving changes attributes, so it pings.

Drawn by its box's shape:

- **Wider than tall** (most bars): intuiTUIve's `progress_bar` (core,
  `parity.rs`, exists: a bar, the percentage, a pulse for `Busy`), with
  the label before it and `aria-valuetext` after.
- **Roughly square and at least 10×5 cells** (a dial): a new intuiTUIve
  `dial` (below), with `text` or the value in its centre.
- **One row tall and narrow** (a signal meter): a `meter`-like run of
  eighths blocks; no new widget.

The `<meter>`'s `low`/`high`/`optimum` choose green, yellow or red, as
browsers draw it.

### Sticky and fixed layers

Read `position` (already in `SNAPSHOT_STYLES`, index 14) into each block
and media region. Then, in `layout::project`:

- **Fixed at the top** (a box whose top is within a row of the viewport's
  top while the page is scrolled): pinned. Its lines are drawn once, above
  the scrolling projection, on a pinned strip of at most a quarter of the
  screen; past that, it scrolls with the page from the top.
- **Fixed at the bottom** (cookie banners, sign-up bars, players'
  mini-bars, chat inputs): docked over the bottom rows, above the status
  bar, with `Esc`-like "hide this bar" in `F1`; never drawn in the flow.
  A fixed box with `role=dialog` or `aria-modal` is a modal: drawn over
  the page with the page dimmed, as the design note always planned.
- **Sticky**: placed at its *static* position (not where it is stuck now).
  The snapshot reports where it is on screen, so its static position is
  recovered from its next sibling's top minus its own height (or from
  the offset rects the snapshot already carries, `includeDOMRects` at
  `cdp.rs:628`); and the top bar of a page that sets
  `position: sticky; top: 0` on its header is pinned like a fixed one.
- **Scroll the page to its top for the reading** used for layout when it
  is short (the cause of R7): `load_more` (`app.rs:2195`) should not fire
  while the whole page fits in two screens.

### Text over pictures

When a text box lies inside a media region's box (by Chrome's geometry)
and is short (one line, up to a third of the picture's width), it is an
overlay: drawn in the picture's cells, at its place in the box, with its
own background colour, after the picture. Kitty draws a picture under
text when it is placed with a negative `z` (the protocol has it;
rs-rich-art's Kitty placement does not ask for one yet, a small addition
there); Sixel and iTerm2 overwrite cells, so for them the overlay is
drawn after the picture and the picture's cells under it are not
redrawn. Half blocks just take the text's cells. The duration on a
YouTube thumbnail, LIVE and the viewer count on Twitch's, a price tag on a
product image all stay on their picture, and the rows they took under it
go.

### Carousels and shelves

A box whose content overflows sideways only (now no longer a log, see
fix 3) and whose children are alike (the same tag, similar boxes) is a
shelf. Drawn as a new intuiTUIve `carousel` over the items Chrome lays
out in view, with `‹ ›` at its ends and `1–4 of 12`; `[` and `]` (or the
wheel with Shift) page it, which scrolls the page's box sideways (a
`scrollLeft` set through the existing scroll action) so lazy items load,
and the page is read again. Its previous and next buttons, when the page
has them (`aria-label` "Previous"/"Next", or `aria-controls` the shelf),
are folded into the `‹ ›` instead of drawn apart.

### Video pages, live streams, search and shop pages

Generic, no site code:

- **Video watch pages**: what plays already plays. What needs work is the
  furniture: overlays on the player (fix 4 and paint order, 12), the
  hover-only controls (13), chapters as a list of time links (a list of
  links whose text starts with a time is drawn as a table of time and
  title; clicking seeks — a `timestamp link` adaptation), and comments as
  they read now. An embedded player page now reads (fix 1).
- **Live streams with chat**: the chat as a live log works where the page
  marks it (`role=log`) or appends to a scroll box. Twitch's chat is a
  scroll box appended to: it should follow; the radar channel was silent,
  so this was not seen live this time. The tooltip card in the chat panel
  (dismissable, `role=dialog` or an `aria-live` region) would go to the
  docked layer.
- **Search pages**: the results are the page; the sticky header is the
  main loss (fix 2). The search box should be reachable in one key: the
  page's first `role=search` landmark's text box becomes the target of
  the `o`-like "search this site" key (`s`), and the result list (headings
  that are links, in a `main`) is what `f` labels first. "People also
  ask" buttons with `aria-expanded` already open and close.
- **Shop grids**: prices (fix 7), ratings (fix 8), the sticky header (2),
  sideways shelves (11), the cookie banner (2). The `Image:` caption
  under a product picture repeats the card's title: the existing
  `Captions` adaptation should skip a caption whose words are mostly in
  the next heading or link of the same card.

### Site adapters: no

Per-site "readers" (a YouTube adapter, an Amazon adapter) would read
each of these sites better sooner, and break when the site changes; the
maintainer's rule of 2026-10-25 (no host names, no per-site selectors;
[No site-specific code](browser.md#no-site-specific-code)) rules them
out, and every fix above is generic and helps sites nobody listed. What a
user can already do per site — keys, adaptations, `--reader` by default
in `[site."…"]` — stays the user's.

## Native controls

What exists, and is reused:

| Widget | Where | Used for |
|---|---|---|
| `progress_bar` | intuiTUIve core (`parity.rs`) | progress, meters as bars, busy pulses |
| `image_bytes_of`, `image_graphics` | intuiTUIve `art` | canvas and SVG stills, as pictures are drawn now |
| `pixels` / `pixels_fit` | intuiTUIve core | a dial in half blocks when no graphics (the `dial` draws on it) |
| `sparkline` | intuiTUIve `viz` | a value's history beside an indicator (opt-in) |
| `meter`, `tuner` | intuiTUIve `viz` | audio-domain; not used (a `tuner` could later sit on `dial`) |
| `slider` | intuiTUIve core | already draws `role=slider` |
| `collapsible` | intuiTUIve core | `aria-expanded` disclosures could use it (not needed now) |
| `seg_gauge` | intuiTUIve core | status-bar segment; not for page content |

New, generic, in intuiTUIve's core (always compiled, so each must be
reusable outside Noctiluca — they are: a value in a range, a score, a
paged strip):

| Widget | Rough API | Effort |
|---|---|---|
| `dial` | `dial(value: impl Fn() -> f64, range: RangeInclusive<f64>) -> Node`, `.label(..)`, `.readout(impl Fn() -> String)`, `.bands(low, high, optimum)`, `.sweep(degrees)`: a 270° arc in half blocks (on `pixels_fit`) or eighths, the value in `digits` when there is room, else in text | M |
| `rating` | `rating(value: impl Fn() -> f64, out_of: u32) -> Node`: `★★★★½☆` with the number, one row | S |
| `carousel` | `carousel(len: impl Fn() -> usize, first: Signal<usize>, item: impl Fn(usize) -> Node) -> Node`, `.per_page(n)`, `.on_page(impl Fn(usize))`: items side by side, `‹ ›` and `1–4 of 12`, paged by keys, wheel and clicks | M |

Noctiluca-specific (the mapping from the page, never in intuiTUIve): the
`Indicator` model kind and its reading; the shape rule choosing bar or
dial; live-region polling and revision keys; the pinned and docked
layers; overlay placement; the screen-reader-twin and rating rules; the
shelf detection and its scroll binding.

## Workstreams

Parallel agents, each on its own branch from `main`, each committing at
milestones (WIP-marked) as `AGENTS.md` asks. The file split keeps them
from colliding: one owner each for `tab.rs`, `layout.rs`'s placement and
`model.rs`'s reading, with small, named touch points.

| Stream | Fixes | Owns | Touches | Tests |
|---|---|---|---|---|
| **A. Live regions** | 1, 6 | `tab.rs` (stills, polling, keys), `pictures.rs` | `model.rs` (a `revision` and `text` on `Media`; agree with C) | fake engine: keys and polling; live: `gauge-svg.html`, `gauge-canvas.html` |
| **B. Layers and placement** | 2, 4, 10, 14, 12 | `layout.rs` (`project`), `app.rs` (pinned and docked strips, `load_more` gate) | `model.rs` (`position`, a covered flag from paint orders; agree with C), rs-rich-art (a Kitty placement under text) | `search-results.html`, `product-grid.html` (header pinned, banner docked), a Twitch-like overlay fixture |
| **C. Semantics** | 3, 5, 7, 8, 9, 13, 16 | `model.rs` (`Indicator`, labels, twins, ratings, names), `adapt.rs` | `layout.rs` (drawing an indicator block: a `Piece` for B to place) | `meters.html`, `product-grid.html`; model tests on `PageBuilder` |
| **D. intuiTUIve widgets** | `dial`, `rating`, `carousel` | `crates/rich-intuituive` (new modules, widget index, docs) | none in Noctiluca | doc tests and driver tests in intuiTUIve; screenshots for the widget index |
| **E. Shelves** (after C and D) | 11 | Noctiluca's shelf mapping | `layout.rs`, `tab.rs` (sideways scroll action) | `product-grid.html` |
| **F. Small UX** | 15, and the real-sites page | `tab.rs` error path, `docs/guide/noctiluca/` | — | a fake-engine navigation error |

Order: A, B, C and D start together; E after C and D; a recording of the
live tapes after A and B land, with real-sites.md updated.

## Risks

- **Byte budget.** Polling canvases adds screenshots; on SSH it must start
  at half blocks and a low rate, and stop off screen. The budget already
  shared by GIFs and video is the guard; a page with many animated canvases
  (ads) needs a cap per page (four live canvases on screen, nearest the
  view).
- **Sticky recovery is a heuristic.** Recovering a sticky box's static
  place from its neighbours can be wrong on unusual layouts; pinning only
  what touches the top or bottom of the viewport keeps the damage small,
  and the faithful projection (`a`) stays available.
- **Screen-reader twins** must not bring back the hints the hidden-text
  rule removes ("Use the Right Arrow Key…"): the rule applies only when an
  `aria-hidden` sibling shows the same characters (digits for prices).
- **Ratings by text** ("out of") are English-shaped; a number pair in the
  name ("4.5 / 5") is the fallback, and nothing is drawn when neither
  parses.
- **Occlusion** by paint order and boxes can hide too much (a
  translucent overlay, a box with `pointer-events: none`): only boxes
  with an opaque background or a picture count as covering.
- **The sandbox.** These checks ran Chrome without its sandbox, as root,
  through a wrapper; that is a property of this container, not of
  Noctiluca, which never turns the sandbox off.

## What this pass could not verify

- **Google, speedtest.net and Amazon**: not reachable from here. The
  gauge, search and shop findings are from reduced fixtures of their
  general shape. Whether speedtest.net's dial is a canvas or an SVG, and
  whether it states its value in ARIA, is unknown; both paths are covered.
- **A YouTube watch page playing**: YouTube's bot check blocks this
  machine (as on 27 October).
- **Twitch chat following new messages**: the channel recorded had no
  chat lines.
- **Terminals other than the recorder's emulator** (Kitty graphics as
  `rich record` draws them): no real Kitty, WezTerm, Sixel or SSH run.
