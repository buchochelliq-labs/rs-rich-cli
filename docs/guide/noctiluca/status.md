# What works today and what's next

Noctiluca grows in phases, each unlocked by what people use a browser in a
terminal for. The phases and their tests are in
[the design note](../../design/browser.md#phases). This page says what the
0.0.22 release has, and what comes next.

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
  reader mode), and no video frames (a video's sound plays).
- **`--fps` and `fps` are read but change nothing yet.**
- **Downloads are refused**, with no switch to allow them.
- **A binding cannot start from a link hint** or the focused element; the
  `Ctrl+K` picker lists the same elements.

## Planned

**0.0.23: pictures and scrolling.**

- Pictures in the projection.
- Animated GIFs.
- Done: Chrome's scroll follows Noctiluca's, so lazy images and infinite
  feeds load as you scroll, and a picture partly in view shows its part.

**Also in 0.0.23: video.** Play, pause, seek and mute a video in its own region,
its frames under a budget, stepping down to character art on a slow
terminal.

**Later, without a date yet:**

- **Forms and tables.** Every widget through the guarded commit, with
  conflicts caught; wide and long tables that scroll with their headers
  kept; signing in to a form.
- **Layout.** Grid and columns side by side, sticky headers that stay
  pinned, a raster view from Chrome's screencast, a canvas as a region
  inside projected text.
- **Tabs**, once rs-rich-embed drives several pages.
- **Automation.** Scripts that drive a page through the same actions a
  person runs, viewable live while they run.

## Never

DRM playback, extensions, getting past bot checks, CAPTCHAs or sign-in
walls, changing browser policy, or sending messages for you.
