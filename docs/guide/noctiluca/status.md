# What works today and what's next

Noctiluca grows in phases, each unlocked by what people use a browser in a
terminal for. The phases and their tests are in
[the design note](../../design/browser.md#phases). This page says what the
0.0.22 to 0.0.25 releases have, what 0.0.26 adds, and what comes next.

## Coming in 0.0.26 (Noctiluca 0.0.5): steady video, sound on

Measured in [Noctiluca's video and readings](../../design/noctiluca-youtube-perf.md).

| Works | Notes |
|---|---|
| Video on a page that keeps changing | About 24 frames a second where a ticking clock or progress bar held it to about one: readings the page asks for wait, frames go on meanwhile, and the screencast covers only the playing region. Done |
| iTerm2 video | Passes #883's qualification gate 3 of 3 runs, with Kitty and ANSI. Done |
| Sound | On by default when Noctiluca runs on your machine; off over SSH unless `--audio`; `--mute` turns it off. Done |
| One trace | `NOCTILUCA_TRACE=FILE` writes the pipeline's timings as JSON lines; nothing goes to stderr. Done |
| Hints while the viewport settles | An action waits up to a second for a reading Chrome refuses for a moment. Done |

### Known limits in 0.0.26

From the [real-site check](../../releases/0.0.26.md#noctiluca-on-real-sites)
of 2026-11-10; each was also seen on 0.0.25 or not checked there:

- **Media keys** can pick a page's `<video>` with no source after a pause
  ([#889](https://github.com/buchochelliq-labs/rs-rich-cli/issues/889)).
- **A busy live stream** can stop drawing after 12 to 25 s while the status
  line says it plays
  ([#890](https://github.com/buchochelliq-labs/rs-rich-cli/issues/890)).
- **A live chat log** can stop taking new lines
  ([#891](https://github.com/buchochelliq-labs/rs-rich-cli/issues/891)),
  and `{` blanks it instead of holding it
  ([#892](https://github.com/buchochelliq-labs/rs-rich-cli/issues/892)).
- **Sixel and iTerm2** still redraw pictures that only moved on scroll
  ([#884](https://github.com/buchochelliq-labs/rs-rich-cli/issues/884));
  Kitty does not.
- **Also seen:** CodeMirror examples read as one line; in-page links do
  not scroll; chat badges are drawn as alt text; stacked toasts overlap.

## In 0.0.25 (Noctiluca 0.0.4): real sites, second pass

Planned in
[Noctiluca on real sites, second pass](../../design/noctiluca-real-sites-2.md)
for search, shop, speed-test, video and streaming pages; what each change
does for which site is on [Noctiluca on real sites](real-sites.md#the-second-pass-0025).

| Works | Notes |
|---|---|
| Live drawings | A `<canvas>` is drawn and captured again while it changes; an inline SVG is captured again when it changes, and its `<text>` is read as text. Done |
| Failed navigations | The address bar keeps the address asked for, with a line saying what failed, the error code and the reload key. Done |
| Meters, progress and ratings as native controls | Drawn with intuiTUIve's bar, dial and rating, read again at every reading. Done |
| Pinned and docked layers, badges over pictures, shelves as carousels | Top bars pinned, bottom bars docked, paint order, badges in their picture's cells; a sideways shelf pages with `<` and `>`. Done |
| Prices, icon buttons, hover-only controls, placeholders | Screen-reader twins, icon glyphs, right-edge controls shortened, lazy thumbnails' placeholders. Done |
| Windows | The control socket over a named pipe. Done |

### Known limits in 0.0.25

- **Checked on fixtures, not the live sites,** for Google, speedtest.net
  and Amazon, which could not be reached from the checking machine;
  YouTube and Twitch were checked live before the changes, and the
  recorded pictures on [real sites](real-sites.md) are still 0.0.23's.
- **Live canvases cost bytes:** at most four are captured at once,
  nearest the view, within the byte budget; off screen or frozen (`F4`),
  none is.
- **A failed navigation from a link** (rather than an address you typed
  or opened) shows the address Chrome's history keeps for it, which after
  a redirect is where it was sent, not the link's own.
- **The reload key in the error line** is named as the default keymap's
  (`R`), even when it is rebound.
- **A shelf pages with `<` and `>` only:** not yet with Shift+wheel or
  clicks on its `‹ ›`, and a page's own Previous and Next buttons are not
  folded into the strip (0.1.0 or later).

## In 0.0.24 (Noctiluca 0.0.3)

Published on 2026-11-03 (with the rest of 0.0.24's items in
[the plan](../../plans/0.0.24.md)):

| Works | Notes |
|---|---|
| Scripts and coding agents | `--control` and `--read-only`; `noctiluca ls` and `noctiluca action` (open, snapshot with control ids, click, type, key, scroll, find, back, forward); `Ctrl+G` sends a picked element to the clipboard or an agent's pane. On Windows too, over a named pipe (0.0.25). See [Scripts and coding agents](agents.md) |
| The pixel view | `Alt+X` or `--pixels`: Chrome's pixels in Kitty graphics, sent as tiles only where they change, within the byte budget; clicks, the wheel and keys go to the page. See [the pixel view](media.md#the-pixel-view) |

Known limits: `click`
takes an id or an accessible name, not a CSS selector; nothing records an
agent's actions beyond the status bar; and the pixel view draws through
Kitty only, at the usual 8 by 16 pixel cell (a terminal with larger cells
scales the tiles up), and scrolling in it costs nearly a whole frame.

## In 0.0.23 (Noctiluca 0.0.2): pictures, video and live logs

| Works | Notes |
|---|---|
| Pictures in the projection | From the page's own files, where the page puts them, in Kitty, Sixel, iTerm2 or half blocks; small pictures in their line of text; an `Image:` caption under one with none |
| Animated GIFs | Moving within a byte budget, stepping down when it is short; Kitty plays them itself; `F4` freezes them |
| Video | Its poster with a play mark, then its frames in its region while it plays on screen; play, pause, seek and mute; the media state in the status bar |
| Live logs | A chat or log follows its newest entries, read without reading the page again, and holds still when scrolled up |
| Terminal adaptations | Named generic rules, each switched off with `[adapt]` or `--adapt`; `a` for the faithful projection, `?` for the about page |
| Long pages | Only the rows near the view are built; lazy images and endless feeds load as you scroll |

### Known limits in 0.0.23

- **One tab.**
- **No fixed or sticky layer** and no paint order in the projection
  (planned for 0.0.25).
- **A frame** shows as its alt text. (A canvas and an inline SVG drawing
  are drawn, and stay live, from 0.0.25.)
- **The byte budget starts as a guess** (4 MB a second, 256 kB over
  SSH), then follows the rate your terminal takes output at once measured
  (lowered to it, and on this machine raised to it, up to 32 MB a
  second), unless `[media] bandwidth` sets it. A site's `fps` is read but not
  applied.

### Checking on real sites

The tests run on the crate's own pages. Before a release, someone also
opens five kinds of real site by hand, in Kitty or WezTerm and in a
terminal with half blocks only, and notes what they see. Each row says
what to look at; the limits below the table are expected, not bugs.
[Noctiluca on real sites](real-sites.md) shows one such check, recorded.

| Site | What to look at |
|---|---|
| A news site's front page and one article | Pictures where the page puts them, captions, headings and reading order; reader mode (`r`) on the article; scrolling down a long front page loads more |
| A video site's watch page | The poster with its play mark, then frames while it plays; play, pause, seek and mute; the media state in the status bar; the comments below |
| A live stream with chat | The video, and the chat followed as a live log: new lines appear, small badges and emotes in their line, and it holds still when scrolled up (`{`, `}`) |
| A docs site | Code blocks, tables and nested lists; the sidebar's links through hints (`f`); find in page (`/`); back and forward |
| A forum thread and its reply form | Quotes and avatars; the reply box takes a draft and asks before it sends; a search field and a select; nothing is sent by the check itself |

Expected limits on real sites:

- **Logged-out walls.** "Confirm you're not a bot", a sign-in wall or a
  cookie wall can stop a page. Noctiluca never tries to get past them;
  note the wall and move on.
- **DRM never plays.** A protected video shows its poster and nothing
  else, and always will.
- **A frame** shows as its alt text.
- **Logs and chats** that a page fills in with a canvas, or in a frame,
  are not followed.

## In 0.0.22 (Noctiluca 0.0.1): the reader phase

| Works | Notes |
|---|---|
| Reading pages | Headings, paragraphs, nested lists, quotes, code and tables, in reading order, with hidden text left out |
| Two views | The projection (Chrome's layout in cells, the default) and reader mode (`r`); a raster view for debugging |
| Going places | The address bar, keywords, search, link hints, back and forward, reload, bookmarks, find in page |
| First forms | Fields with a draft and a guarded commit, password fields never read, check boxes, radios, switches, selects, sliders, buttons |
| Keys | Two presets, rebinding by action id, per-site keys bound from inside the app, the live F1 overlay |
| Still images | As pictures in **reader mode only** (Kitty, Sixel, iTerm2, or half blocks); the projection shows a placeholder with the alt text |
| Sound | On by default on your own machine, off over SSH unless `--audio`; played where Chrome runs; play, pause, mute and volume for each video and audio |
| Scripts and CI | `--dump text`, `ax` or `json`, `--no-js` |
| Safety | Chrome's sandbox always on, page text cleaned, downloads refused, a temporary profile, removed on the way out, unless you keep one |

Each item is tested: the model, layout, config and app tests run on
Chrome's saved snapshots of the crate's test pages, and the live tests run
the same pages through a real Chrome in CI.

### Known limits

- **One tab.** The tab actions are listed and say "One tab for now".
- **The projection is rough:** no fixed or sticky layer, no paint order.
- **No pictures in the projection**, no animated GIFs (a still frame in
  reader mode), and no video frames (a video's sound plays). All three
  came in 0.0.23.
- **`--fps` and `fps` are read but change nothing yet** (they cap moving
  pictures since 0.0.23).
- **Downloads are refused**, with no switch to allow them.
- **A binding cannot start from a link hint** or the focused element; the
  `Ctrl+K` picker lists the same elements.

## Planned

**Later, without a date yet:**

- **Forms and tables.** Every widget through the guarded commit, with
  conflicts caught; wide and long tables that scroll with their headers
  kept; signing in to a form.
- **Layout.** A raster view from Chrome's screencast. (Pinned headers
  and a canvas as a live region inside projected text are in 0.0.25.)
- **Tabs**, once rs-rich-embed drives several pages.
- **Automation, further.** Recorded scripts that replay a session's
  agent actions, CSS selectors as targets, and a log of what an agent
  did.

## Never

DRM playback, extensions, getting past bot checks, CAPTCHAs or sign-in
walls, changing browser policy, or sending messages for you.
