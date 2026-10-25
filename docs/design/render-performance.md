# Rendering performance: viewports, infinite scroll and paint strategies

**Status:** proposed on 2026-10-25, for 0.0.23. Written from three
surveys: Noctiluca and rs-rich-web's render paths, intuiTUIve's pipeline,
and how other projects do it (sources at the end). Nothing here is built
yet. The maintainer asked for three things:

1. **Forever scroll:** pages and lists that keep loading as you scroll,
   with only what is on screen rendered. Possibly a core optimisation.
2. **Faster frames under heavy damage.**
3. **A configurable paint strategy**, with a `best` option that picks per
   frame from the damage, chosen by benchmarks rather than guesses.

## Where things stand

### intuiTUIve's pipeline

- **Damage is per node, as rectangles.** Only dirty or moved nodes draw
  (`node.rs:3105`). The `Screen` is kept between frames, so a clean node's
  cells stay. A non-retained widget clears and redraws its whole rectangle.
- **The painter compares cells inside the damage only**
  (`rich-ext/src/surface/screen.rs:906`, `changed` at :997). There are no
  row hashes and no full-buffer compare.
- **The output is plain.**
  - Every move is an absolute CUP (:799).
  - Every style change is a full reset plus the new attributes:
    `\x1b[0;…m` (:1090).
  - There is no EL, ECH or REP. There is no scroll region (DECSTBM) and no
    SU or SD.
  - Nothing switches to a full repaint when most of the screen changed.
- **What we already do well:**
  - frames go out in synchronized output (DEC 2026) where the terminal
    answers for it;
  - styles skip their SGR when unchanged;
  - links open and close only on change;
  - the style table compacts.
- **Scrolling rewrites the pane.**
  - `Canvas::scroll_up` shifts the buffer, but damages the whole
    rectangle (`widget.rs:1228`), so every shifted cell is sent again.
  - That includes `LogView`, which already renders only its new lines.
- **The scroll view is not virtual.**
  - `scroll()` lays its child out at full height, capped at
    `MAX_SCROLL_ROWS = 4000` (`builtin.rs:28`). Rows past it cannot be
    reached.
  - It draws offscreen and copies out only the rows in view, so a scroll
    renders nothing new. A change, though, draws the whole content.
- **Virtual lists exist**, but only as list and table widgets:
  `list_lazy`, `virtual_list`, `virtual_table`, `tree_lazy`. They draw a
  screenful whatever their length.
- **No "near the end" hook exists** anywhere.

### Measured against ratatui

At 80x24, in release, on the benchmark gate (`tests/versus_ratatui.rs`):

| Scene | ratatui | intuiTUIve |
|---|---:|---:|
| Status tick | 95 µs, 37 B | 15 µs, 18 B |
| Selection move | 97 µs, 184 B | 39 µs, 118 B |
| Log append | 94 µs, 443 B | 47 µs, 428 B |
| Churn, every cell | 326 µs, 62 kB | 3,042 µs, 66 kB |
| Chart redrawn | 81 µs, 4.2 kB | 463 µs, 3.1 kB |

Small updates are already cheaper than ratatui's. Heavy damage is where we
lose: churn is about 9 times slower and the chart about 6 times. The log
append sends nearly a whole pane for one new line.

### Noctiluca

- **Chrome never scrolls.**
  - The design note says Chrome's scroll follows ours, "so lazy loading
    and infinite scroll still fire" (`browser.md:358`), but nothing sends
    the scroll command (`Command::Scroll` exists in `tab.rs:76` and is
    never used).
  - Feeds never load more, and `loading=lazy` images below the fold never
    load.
- **Every read is the whole document.**
  - `DOMSnapshot.captureSnapshot` has no clip, viewport or incremental
    mode, so this is a protocol limit, not ours.
  - `build()` walks every node.
  - `projection()` lays out every block into a full grid.
- **Scrolling is cheap, but some keys are not.** The scroll view copies
  rows out. But `scroll.down`, `page.down`, `bottom` and find all call
  `view_lines()`, which lays the whole page out again on every keypress.
  With find active, that happens once per text piece on each draw.
- **Hard limits that fail silently:**
  - the 4,000-row scroll cap;
  - `MAX_BLOCKS = 20,000`, whose `truncated` flag nothing reads.
- **Images:** a Sixel or iTerm2 picture shows as half blocks unless its
  whole height is on screen (`image.rs:611`). Kitty works row by row.
- **Wasted work:** rs-rich-embed starts Chrome's screencast and decodes
  every JPEG frame even under Noctiluca, which never shows them. The
  design note measured this starving snapshot calls.

### rs-rich-web

- **The DOM renderer sends only the rows that changed**, but finds them by
  scanning every cell each frame. It ignores the damage list.
- **It rebuilds the accessibility tree's JSON each frame.**
- **There is no scrollback.** The page holds exactly the screen's rows, and
  the wheel goes to the app.

## How others do it

- **Textual:**
  - Finds what is visible through a spatial map of 100x20 tiles, so
    scrolling 1,000 widgets costs about as much as 8.
  - Merges dirty regions into per-line spans.
  - Long content uses a line API (`render_line(y)` over a `virtual_size`),
    which renders only the rows asked for.
- **Bubble Tea v2 on ultraviolet** is the closest to our cell screen, and
  the best model:
  - **Scrolling:** it hashes rows, finds runs that moved by one shift, and
    scrolls them in the terminal (whole-screen SU/SD, then DECSTBM margins,
    then IL/DL) before diffing the rest.
  - **Shorter runs:** ECH and REP only where they are shorter than the run,
    and EL where it is cheaper than blanks.
  - **Cursor moves:** it picks the shortest of several encodings.
  - **Safety:** it turns scrolling off on Windows.
- **notcurses** counts what it elided and emitted per frame (cells,
  colours, bytes, raster and write time). Our stats should do the same
  before we tune anything.
- **Carbonyl** is fast because Chromium paints only damaged pixels, and
  only the viewport. A DevTools client cannot get that. Its output side is
  a plain cell diff with one write per frame.
- **Browsh:**
  - Parses only a window of about six screens either side.
  - Watches the DOM with a MutationObserver, debounced at 100 ms on the
    leading edge.
  - Turns `fixed` and `sticky` elements into `absolute`, so they do not
    stutter.
- **Virtual lists on the web:**
  - **react-window and TanStack Virtual:** overscan, estimated then
    measured heights, and stable keys.
  - **Anchoring:** keep the top visible item still when content is
    inserted above.
  - **Infinite loaders:** a threshold of rows before the end, a minimum
    batch, and one load in flight as back-pressure.
- **Terminal caveats:**
  - Scrolled-in lines may take the current background, so reset SGR
    before scrolling and repaint the exposed rows.
  - DECSTBM homes the cursor.
  - tmux redraws scroll regions itself, so the saving is smaller there.
  - Kitty has no background-colour erase.
  - Generic xterm, screen and the Linux console have no REP.

## Proposal

Three pieces, in order. The core pieces are generic. Noctiluca uses them
but owns nothing they need.

### 1. A virtual scroll view in intuiTUIve core

`scroll_virtual(len, height_of, row)`: a scroll view over content
measured in rows that lays out and draws only what is in view plus
overscan.

- **Items, not a child tree:** `len` items, each with an estimated height
  that is corrected once drawn.
- **Builds only what is visible:** the items in view plus an overscan of
  one screen above and below. They are kept by key while they stay near
  the view and dropped when they leave it.
- **No row cap:** the offset is a `u64` of rows, and the scrollbar is
  computed from the estimated heights.
- **Anchoring:** when items above the view change height or are inserted,
  the top visible item keeps its row.
- **`on_near_end(threshold, f)`:** called once when the view comes within
  `threshold` rows of the end, and not again until `len` grows. That is
  the forever-scroll hook, with one load in flight.
- **The existing widgets keep their behaviour:**
  - `scroll()` stays as it is, for small content;
  - `list_lazy` and `virtual_table` move onto the same viewport code;
  - `MAX_SCROLL_ROWS` stays only for `scroll()`, documented, and warns in
    debug builds when it cuts content.

Second users besides Noctiluca: log viewers, chat, file lists, search
results, `rich pager` on huge files.

### 2. Paint strategies in the painter, with `best`

`App::paint_strategy(PaintStrategy)` (also `RICH_PAINT_STRATEGY`). The
choice sits at `Driver::paint` and inside `Painter::paint`, the one place
a frame becomes bytes.

- **`Cells`:** today's per-cell diff inside the damage. The default until
  `best` has its numbers.
- **`Rows`:** compare whole rows by hash, and rewrite changed rows from
  their first change to their last.
- **`Full`:** repaint everything: home, then each row, then clear the rest.
- **`Scroll`:** find runs of rows that moved by one shift, scroll them in
  the terminal, then diff what is left.
  - It tries whole-screen SU/SD first, then DECSTBM margins.
  - It resets SGR first, repaints the exposed rows, and treats the cursor
    as lost afterwards.
  - It is off on Windows consoles, and it is skipped when a Sixel or
    iTerm2 image is in the moved region.
- **`Best`:** per frame, encode the candidates that apply and send the
  shortest. It is cheap because every candidate works from the same damage
  and row hashes:
  - always `Cells`;
  - `Scroll` when the row hashes show a shift;
  - `Full` when more than a measured share of the screen changed.

  It is bounded so it never costs more than it saves: past a cell count it
  stops encoding and picks by the damaged share alone. Its thresholds come
  from the benchmarks below.

Every strategy also gets these encoder improvements, each checked against
the terminal's capabilities:

- **Style changes:** send the change from the current style, not a full
  reset, where that is shorter.
- **Cursor moves:** the cheapest of CUP, a relative move, CR plus a
  relative move, or writing through one or two unchanged cells.
- **Shorter runs:**
  - ECH or EL for a run of blanks, only with a reset background (kitty
    has no background-colour erase);
  - REP for an ASCII run, only on terminals known to have it.
- **One write per frame**, built in one buffer. It already goes out in one
  synchronized-output block.

The work stays generic: everything is in rs-rich-ext's painter and
intuiTUIve's driver. rs-rich-web's DOM renderer moves to the damage list
too, sending the rows the damage touched instead of scanning every cell.

### 3. Noctiluca on both

- **Scroll Chrome along with us.**
  - When our offset moves, send a debounced `Input.dispatchMouseEvent`
    (`mouseWheel`), so the page's own IntersectionObserver sentinels fire.
  - Use `window.scrollTo` for jumps such as `bottom` and find.
  - Read `Page.getLayoutMetrics` back for the content height.
- **Near the end, ask Chrome to scroll on.** When our view comes within a
  screen of the projection's end, scroll Chrome to its bottom too. Re-read
  on the page's change ping, with one read in flight.
- **Project only what is near the view.**
  - Index blocks by their y.
  - Lay out only the blocks within the view plus overscan (two screens),
    with prefetch beyond that.
  - Keep the page's own coordinates for the rest, so the scrollbar and
    jumps stay right.
- **Anchor by node.** On a re-read, keep the top visible block (by
  `backendNodeId`) on the same row, and reconcile with Chrome's own scroll
  anchoring.
- **Cache the layout.** `view_lines()` stops laying out the whole page per
  keypress. The memo is shared, and find works from a text index.
- **Say when the page was cut.** Show `truncated` in the status line.
- **Stop the unused screencast.** rs-rich-embed's screencast runs only for
  a raster view; the design note's "screencast on demand".
- **Images:** draw a Sixel or iTerm2 picture once it is partly in view,
  clipped to the rows shown. That needs a per-region still cut to the
  visible part.

A full snapshot per read stays, since the protocol has no other kind.
Prototyping a JavaScript walker that serialises only elements near the
viewport is listed below, to be measured on a long feed before it
replaces anything.

## Benchmarks

A benchmark comes first and decides the defaults. Each scene runs at
80x24 and 200x60, under every strategy. Each reports bytes and time per
frame, plus counts like notcurses': cells compared, cells written, cells
elided, and bytes by kind (moves, styles, text). Every scene's output is
replayed through vt100 and must reproduce the screen exactly, so a
strategy can never be faster by being wrong.

| Scene | What it stresses |
|---|---|
| One cell blinks | The smallest change; cost of the strategy choice itself |
| Typing in a field | A short run, cursor moves |
| A status line and a clock | Two small rectangles far apart |
| Log append | One new line at the bottom of a full pane: the scroll case |
| A list scrolled a page | Every row moves |
| A list scrolled one row | Every row moves by one: the best scroll case |
| A table re-sorted | Every row changes, styles stay |
| Churn, every cell | The worst case: random colours every frame |
| A chart redrawn | Dense braille, few styles |
| Noctiluca scrolling a long page | The browser's case |

There are two harnesses:

- **In process:** the gate's `Feed` backend, extended to every strategy.
  It is fast enough for CI.
- **Through a PTY:** to count the real write calls and the time to the
  terminal, using `portable-pty` and `vt100` as `tests/backends.rs` does.

The results table goes in this note, and `best`'s thresholds are set from
it. CI then fails if `best` is ever more than 5% worse, in bytes or time,
than the best fixed strategy on any scene. The existing ratatui gate keeps
its assertions.

## Order of work

1. **The benchmark harness and stats:** the scenes, the per-frame counts,
   and the vt100 check, with today's painter as the baseline.
2. **The encoder improvements:** shorter style changes and cursor moves,
   ECH, EL and REP, and one buffer.
3. **The strategies:** `Rows`, `Full` and `Scroll`, then `Best` with
   thresholds from step 1's numbers.
4. **The virtual scroll view, with `on_near_end`,** and `list_lazy` and
   `virtual_table` moved onto its viewport code.
5. **Noctiluca:**
   - Chrome's scroll following ours;
   - near-end loading;
   - projection near the view only;
   - anchoring;
   - the cached layout;
   - the truncated notice;
   - no screencast.
6. **rs-rich-web's DOM renderer** on the damage list.

Steps 1 to 3 are independent of 4 to 6, and can run alongside them.

## Open questions

- **Should `best` be the default** once it beats `cells` on every scene,
  or stay opt-in for a release? The proposal: default in the release
  after its numbers land.
- **Terminals with unknown capabilities:**
  - scrolling stays off under tmux unless the benchmark shows a gain
    there;
  - REP stays off unless the terminal is known to have it.
- **The JavaScript viewport walker:** prototype and measure on a long
  feed page before deciding.

## Sources

- **Textual:**
  - [the 1.0 post](https://raw.githubusercontent.com/Textualize/textual/main/docs/blog/posts/release1.0.0.md)
  - [`_compositor.py`](https://raw.githubusercontent.com/Textualize/textual/main/src/textual/_compositor.py)
- **Bubble Tea and ultraviolet:**
  - [the v2 renderer](https://raw.githubusercontent.com/charmbracelet/bubbletea/main/cursed_renderer.go)
  - [ultraviolet's `terminal_renderer.go`](https://raw.githubusercontent.com/charmbracelet/ultraviolet/main/terminal_renderer.go)
  - [ultraviolet's `terminal_renderer_hardscroll.go`](https://raw.githubusercontent.com/charmbracelet/ultraviolet/main/terminal_renderer_hardscroll.go)
- **ratatui:**
  [`buffer.rs`](https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui-core/src/buffer/buffer.rs)
- **notcurses:**
  - [render](https://raw.githubusercontent.com/dankamongmen/notcurses/master/doc/man/man3/notcurses_render.3.md)
  - [stats](https://raw.githubusercontent.com/dankamongmen/notcurses/master/doc/man/man3/notcurses_stats.3.md)
  - [`TERMINALS.md`](https://raw.githubusercontent.com/dankamongmen/notcurses/master/TERMINALS.md)
- **Synchronized output:**
  [the specification](https://github.com/contour-terminal/vt-extensions/blob/master/synchronized-output.md)
- **Carbonyl:**
  - [the project](https://github.com/fathyb/carbonyl)
  - [its renderer](https://raw.githubusercontent.com/fathyb/carbonyl/main/src/output/renderer.rs)
- **Browsh:**
  - [`dimensions.js`](https://raw.githubusercontent.com/browsh-org/browsh/master/webext/src/dom/dimensions.js)
  - [`manager.js`](https://raw.githubusercontent.com/browsh-org/browsh/master/webext/src/dom/manager.js)
- **Chrome DevTools protocol:**
  - [DOMSnapshot](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/DOMSnapshot.pdl)
  - [Input](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Input.pdl)
  - [Page](https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Page.pdl)
- **Virtual lists and infinite loading:**
  - [TanStack Virtual](https://raw.githubusercontent.com/TanStack/virtual/main/docs/api/virtualizer.md)
  - [react-window-infinite-loader](https://raw.githubusercontent.com/bvaughn/react-window-infinite-loader/master/README.md)
- **The web platform:**
  - [IntersectionObserver](https://raw.githubusercontent.com/w3c/IntersectionObserver/main/index.bs)
  - [CSS scroll anchoring](https://raw.githubusercontent.com/w3c/csswg-drafts/main/css-scroll-anchoring-1/Overview.bs)

Unverified:

- xterm's support for synchronized output;
- how each terminal moves Sixel images under DECSTBM;
- `captureSnapshot`'s cost on large pages, since nothing is published.
