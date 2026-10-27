# What works today and what's next

Noctiluca grows in phases, each unlocked by what people use a browser in a
terminal for. The phases and their tests are in
[the design note](../../design/browser.md#phases). This page says what the
0.0.22 and 0.0.23 releases have, and what comes next.

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
- **No fixed or sticky layer** and no paint order in the projection.
- **An inline SVG drawing, a canvas and a frame** show as their alt text.
- **The byte budget starts as a guess** (4 MB a second, 256 kB over
  SSH), lowered to the rate your terminal takes output at once measured,
  unless `[media] bandwidth` sets it. A site's `fps` is read but not
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
- **An inline SVG drawing, a canvas and a frame** show as their alt text.
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
| Sound | With `--audio`, played where Chrome runs; play, pause, mute and volume for each video and audio |
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
- **Layout.** Sticky headers that stay
  pinned, a raster view from Chrome's screencast, a canvas as a region
  inside projected text.
- **Tabs**, once rs-rich-embed drives several pages.
- **Automation.** Scripts that drive a page through the same actions a
  person runs, viewable live while they run.

## Never

DRM playback, extensions, getting past bot checks, CAPTCHAs or sign-in
walls, changing browser policy, or sending messages for you.
