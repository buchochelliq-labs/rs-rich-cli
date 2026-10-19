# intuiTUIve: what the demos are missing

Status: working list, updated after each round.

We rebuild well-known ratatui apps on intuiTUIve to learn what the
framework lacks. Each clone is built only from intuiTUIve's own
components, from its core or its plugins: no framework changes during the
round, and no custom widget in the example to stand in for a missing one.
What the clone needs and cannot get goes on this list. Gaps chosen from it
are added to the framework, the clones are rebuilt on them, and we repeat
until each clone uses intuiTUIve components only. Then we pick more apps.

Each round also looks beyond what the clone strictly needs. Whoever
builds a clone reports, under **Components that would help**, the
components that would make it shorter or more faithful: the ones the
original app leans on, and the ones comparable libraries (ratatui and its
crates, Bubble Tea and Bubbles, Textual) offer. Each entry gives what it
would replace, an API sketch, core or plugin, and how much it would help.

Where a gap belongs:

- **core**: rs-rich-intuituive itself (layout, widgets, keys, popups);
- **plugin**: a crate or feature beside it (rs-rich-ext's charts,
  rs-rich-ratatui, or a new one).

## Round 1: Yazi and scope-tui

The clones are [`examples/files.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/files.rs)
(Yazi, the file manager) and
[`examples/scope.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/scope.rs)
(scope-tui 0.3.5, the oscilloscope). How each compares with its original
is in [the porting guide](../guide/intuituive/porting.md).

### Keys and input

| Gap | Needed for | API sketch | Where |
|---|---|---|---|
| Key sequences, with a which-key pop-up | Yazi's `gg`, `,s`, `cc` chords; today each prefix needs a popup of its own, a key that doesn't match is ignored instead of closing it, and the palette cannot list chords | `node.bind("g g", desc, f)`; which-key turned on by the app | core |
| Catch any key | Closing which-key on any other key | `node.on_any_key(\|cx, key\| -> bool)` | core |
| Keys a plain container sees before its children | Yazi's root keymap beats a focused list's own j/k/g; the example makes the root focusable instead | `column(..).previews_keys()` | core |
| Placing the caret, selecting part of an input | Rename with the caret before the extension | `input(sig).caret(Caret::End \| Caret::At(n))`, `.select(range)` | core |

### Lists and tables

| Gap | Needed for | API sketch | Where |
|---|---|---|---|
| Row controls on `list` | Yazi's right-aligned line mode, a one-cell marker column, a selected style per row; two synced lists stand in today | `list(..).trailing(\|i\| markup)`, `.marker(\|i\| Option<style>)`, `.selected_style_for(\|i\| style)` | core |
| A table without a header | Name and line-mode columns in one widget | `TableOptions::header(false)` | core |
| `virtual_list` options | Huge directories with scrolloff and row colours; `list` rebuilds every row on each draw | `scroll_padding`, `row_style`, `offset` on `virtual_list` | core |
| A table row with ratatui's cell rules | scope-tui's header: cells cut rather than wrapped, column spacing, percentage widths | a `row` cell option, or a one-row `table` | core |

### Layout, popups and chrome

| Gap | Needed for | API sketch | Where |
|---|---|---|---|
| Widths solved as ratatui solves them | scope-tui's header: ratatui takes percentages of the whole width, intuiTUIve of the width after gaps, so cells sit 1–3 columns off and get cut at 80–100 columns | `row(..).gap(1).percent_of(Basis::Whole)`, or a `Solver::Ratatui` mode | core |
| The screen rect of a node or a list row | Yazi's rename prompt below the hovered row; the example re-derives `hsplit`'s maths | `Node::rect_handle()`, `list(..).row_rect(i)` | core |
| A popup anchored to the screen | Yazi's input (top centre) and which-key (bottom, with margins, clamped) | `cx.popup_at(Origin::TopCenter, Offset { x, y, w, h }, ..)` | core |
| A divider joined to the panel border | The help box's `├──┤` line | `column(..).divider()`, or `rule()` with border merging | core |
| Toast placement and levels | Yazi's notifications: top right, info/warn/error titles | `cx.notify(Level, title, body)` and a position setting | core |
| A confirm dialog in the framework's look | Trash, delete and overwrite; rs-rich-interact has one as a component, but not as an intuiTUIve modal | `cx.confirm(title, body, items, on_yes)` | core |
| Palette and theme details | The palette shows no key for `esc` bindings; `Theme` has no title style for popups | | core |

### Charts

| Gap | Needed for | API sketch | Where |
|---|---|---|---|
| Drop or clip lines that leave the bounds | Ratatui drops a line with an end outside the bounds, rs-rich-ext clips it; the example turns such points into NaN | `Chart::out_of_bounds(OutOfBounds::Drop \| Clip)` | plugin (rs-rich-ext) |
| A dataset drawn over the ones before it | scope-tui's reference lines break where the trace crosses them; rs-rich-ext merges the dots | `Chart::layering(Layering::PerDataset \| Merge)` | plugin (rs-rich-ext) |
| A legend style of its own | The legend box borrows the x axis's style | `Chart::legend_style("…")` | plugin (rs-rich-ext) |
| Leave the plot visible under the legend | Ratatui leaves the dots inside the legend box | `Chart::legend_clear(false)` | plugin (rs-rich-ext) |

### Data, sources and the outside world

| Gap | Needed for | API sketch | Where |
|---|---|---|---|
| The newest value from a thread | scope-tui's reader: the example hand-rolls a thread, a mutex slot and `every()` polling | `latest(\|tx\| loop { tx.send(read()) }) -> Signal<Option<T>>` | core |
| Handing the terminal to another program | Yazi's Enter/`o` opens a file; `Flow::Handoff` cannot do this inside an app | `cx.handoff(cmd)` | core |
| Ratatui colour names | scope-tui's `--palette-color DarkGray`; about 30 hand-written lines | `style_from_color_name("DarkGray") -> Option<String>` | plugin (rs-rich-ratatui) |
| FFT and spectrum | scope-tui's spectroscope; the example carries an FFT and a Hann window | `dsp::fft`, `dsp::hann`, `dsp::spectrum(samples, rate)` | plugin (new) |
| Audio capture | scope-tui's `audio` and `pulse` sources, missing entirely | `audio::input(device, rate, channels, buffer) -> Signal<Matrix>`, `audio::devices()` | plugin (new) |
| Watching a directory | Live updates in Yazi without polling | `watch_path(path, \|event, cx\| ..)` on `notify` | plugin |
| Image previews | Yazi's image preview | an `image(path)` node over rs-rich-art (half blocks, kitty, sixel) | plugin (rs-rich-art) |
| File-type icons and MIME types | Yazi's icons and type colours | `icons::for_path(&Path) -> (glyph, colour)`, a Nerd Font check | plugin |
| Local time and user names | Yazi's mtime/btime and owner line modes (UTC and uid:gid today) | | plugin, or a rs-rich-ext utility |
| Warming up syntax highlighting | A first Markdown preview takes over 2 s in a debug build | `Syntax::preload(langs)` on a worker | plugin (rs-rich-ext) |

### Bugs found

| Bug | Repro | Workaround in the example |
|---|---|---|
| A prefilled input puts the caret at the start | `input(signal("alpha".into()))`, type `x`: `xalpha` (`TextArea::new` and `Buffer::set_text` place it at 0) | Press End first |
| A `list` sized `Size::Auto` measures as the full width | `row([list(a).flex(1), list(b).auto()])`: the second list takes everything | A `switch` on the measured width with `.fixed(w)` |
| `.borders(Sides::BOTTOM)` draws nothing on a node without a panel | | A one-row `leaf` that draws a line |
| A popup's bindings work only on its outermost node | Bindings on a node inside the popup's `.panel()` never fire | Bind after `.panel()` |

## Round 2: scope-tui

The round-1 gaps were added (PR #752) and the clones rebuilt on them, with
no framework changes during the round.

### scope

[`examples/scope.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/scope.rs)
now uses no workaround from round 1. Checked side by side with scope-tui
0.3.5 (built with `app,file`) on the same PCM file at 80x24, 100x32 and
160x45: the header's cells start in the same columns and are cut the same
way at every size, the vectorscope's reference lines break where the trace
crosses them as ratatui's do, and the legend box matches. What still
differs is deliberate and listed in the porting guide.

#### Round-1 gaps now closed

| Gap | Used where |
|---|---|
| `Solver::Ratatui` | `header()`: `row([...]).solver(Solver::Ratatui).gap(1)`; the cells sit where ratatui's `Table` puts them at every width (a test checks against ratatui's `Layout` at 60 to 200 columns), where round 1 was 1–3 columns off |
| `widgets::cell` | `header()`: every header cell, cut with no `…` |
| `Chart::out_of_bounds(OutOfBounds::Drop)` | `chart()`: replaces turning points outside the bounds into NaN |
| `Chart::layering(Layering::PerDataset)` | `chart()`: the reference lines break under the trace |
| `Chart::legend_style(..)` | `chart()`: `legend_style("")`, the chart's own style as ratatui's; the axis-style hack is gone and the axes keep their default style |
| `Chart::legend_clear(false)` | `chart()` |
| `rich_ext::dsp` | `spectroscope()`: `dsp::hann`, `dsp::fft`, `dsp::magnitude`; about 100 lines of FFT, Bluestein and Hann removed |
| `rich_ratatui::style_from_color_name` | `parse()`: the ~30-line colour parser removed. rs-rich-ratatui is a dev-dependency of rs-rich-intuituive, so the example can use it |
| `latest(..)` | `play()`: a file or a pipe is read on a `latest` thread; the `Source`/`Pull` trait, the `Threaded` reader with its mutex and the `every()` polling are gone |
| `audio::input`, `audio::devices` | `scope_app()` and `list_devices()`: scope-tui's `audio [DEVICE] [--list]` and `pulse [DEVICE]` sources, behind `--features audio`; a default build says how to get them |

Also from the round-1 work: the hand-written `?` modal is replaced by the
framework's help (`cx.help()`), fed by `.bind(keys, description, ..)`.

#### New gaps and bugs

| Gap or bug | Needed for | API sketch | Where |
|---|---|---|---|
| A watch that runs on every write, not only on a change | Counting frames: a `watch` compares values, so two equal buffers (the still test signal, a file's trailing silence, digital silence from a sound card) are one. The example numbers a thread's buffers (`Frame { number, data }`) to get round it; `audio::input` cannot be numbered, so silence counts no fps there | `on_write(signal, \|value, cx\| ..)`, or `latest`/`audio::input` signals that carry a sequence number | core |
| `latest` threads and the headless clock | A `latest` thread runs on the wall clock while a headless `Script::wait` advances a clock of its own, so a thread-fed app cannot be tested frame-exactly. The test signal moved to `every(period)` (the app's clock) for that reason, and the file test no longer checks fps | a `Headless`/`Driver` hook that lets `latest` threads catch up (`driver.settle_threads()`), or `latest_every(period, \|\| value)`, a generator on the app's clock | core |
| An app builder cannot fail | `audio::input` must be called inside `App::new`, so a missing device cannot fail before the terminal is taken. The example carries the error to the first turn with `watch(\|\| (), move \|_, cx\| cx.fail(..))` | `App::try_new(\|\| -> Result<Node, E>)`, or `audio::open(..) -> Result<Capture>` before the app and `capture.signal()` inside | core (or the `audio` plugin) |
| Audio device names are descriptions | `audio::devices()` names the ALSA `null` device "Discard all samples (playback) or generate zero samples (capture)", not `null`; `input` matches names, so ALSA PCM names such as `pulse` may not be reachable | `Device { id, name, default }`; `input` takes the id | plugin (`audio`) |
| PulseAudio and PipeWire sources | scope-tui's `pulse [DEVICE] --server-buffer N`, whose main use is a sink's monitor; through cpal only ALSA's `pulse` device (the default source) is reachable, and `--server-buffer` has no meaning | `audio::input_with(AudioOptions { host: Host::Pulse, device, latency })`, or a `pulse` feature on libpulse-simple | plugin (`audio`) |
| Device configurations | `audio --list` prints each input's channels, rate range, buffer range and sample format in scope-tui; `devices()` gives only names | `Device::configs() -> Vec<DeviceConfig>` | plugin (`audio`) |
| A stalled stream | `audio --timeout S`: scope-tui ends when no buffer comes in time. Watching the signal cannot tell a stall from silence (see the first row), so the option is read but not enforced | an `AudioStatus` signal (`Running`, `Stalled(since)`, `Failed(error)`) beside the samples | plugin (`audio`) |
| ALSA writes to stderr | Without a sound card, ALSA's `ALSA lib conf.c: ...` lines are printed before the app; printed while it runs, they would land on the screen | install `snd_lib_error_set_handler` while capturing, and pass the messages to the app's log | plugin (`audio`) |
| Help sections | The help lists one node's bindings as one group; scope-tui's keys are per mode, so the descriptions carry "oscilloscope: …" | `.bind_in("oscilloscope", keys, description, f)`, or a help group per binding | core |
| Modifier variants of a binding | Shift ×10, Ctrl ×5 and Alt ×⅕ on six keys are 24 bindings; to keep the help short only the plain ones are described | `.bind_scaled("up", &[("", 1.0), ("shift", 10.0), ..], "scale up", \|cx, m\| ..)`, shown as one row | core |

No bugs were found in the round-1 additions themselves.

#### Friction with the new APIs

- **Two shapes for "the newest value from elsewhere".** `latest` gives
  `Signal<Option<T>>` (`None` before the first), `audio::input` a
  `Signal<Matrix>` that starts as empty rows. The example needs a match per
  source to take a buffer. One shape (`Option`) for both would let a source
  be one value.
- **`dsp::spectrum` stops at the Nyquist frequency.** scope-tui plots every
  bin, the mirrored half too, so the example uses `fft` and `magnitude`
  instead. Fine as it is; a `spectrum_full` would only save three lines.
- **`legend_style("")`** reads as "no style" only once you know it; ratatui's
  legend takes the chart's own style. A `Chart::style(..)` that the legend
  takes by default would say it directly.
- **`style_from_color_name` lives in rs-rich-ratatui**, which brings
  `ratatui-core`. Fine for the example (a dev-dependency); an app that only
  wants to read ratatui's colour names on its command line pays for the
  conversions too. It could sit in rs-rich-ext with rs-rich-ratatui
  re-exporting it.
- **Solver::Ratatui and `cell` were drop-in.** No friction: the header is
  shorter than round 1's and exact.
- **Frames per second count frames drawn.** `latest` drops what the app had
  no time for, as intended; with a debug build at 80x24 the example shows
  19–20 fps where scope-tui shows 21–22, because scope-tui counts every
  buffer it reads.

#### Components that would help

Patterns written by hand from several built-ins, components from other
ecosystems an audio or visualisation app would use, and plugins that would
make the clone more capable. Value is for this kind of app.

| Component | What in scope.rs it replaces or enables | API sketch | Where | Value |
|---|---|---|---|---|
| A live plot node | `renderable(move \|\| chart(..))` reading a frame signal, with no cap on how often it draws; a node that draws from a signal at most N times a second, dropping the rest | `live(fps_cap, move \|\| chart(..))` | core | medium |
| A row of percentage cells | `header()`'s seven `cell(..).percent(..)` with the solver, the gap and the style: ratatui's one-row `Table` as one call | `cells([(35, a), (25, b), ..]).spacing(1)` | core | medium |
| A frame-rate meter | the `frames`/`fps` signals and the one-second `every` | `rate(signal) -> Signal<u32>`, updates a second | core | medium |
| A mode switcher | `Mode::next`, `name` and the `tab` binding | `cycle(signal, &[modes], "tab")` | core | low |
| Generated key help with sections | the help's descriptions, prefixed by hand (see "Help sections") | `.bind_in(section, ..)` | core | medium |
| A PCM reader | `Pcm` and `deinterleave`: interleaved s16le into rows | `audio::pcm(reader, Format::S16Le, channels, buffer) -> impl Iterator<Item = Matrix>` | plugin (`audio`) | medium |
| WAV files in and out | reading WAV as well as raw PCM; recording what is shown | `audio::wav(path)`, `audio::record(signal, path)` | plugin (`audio`, hound) | medium |
| A signal generator | `Demo`: tones, partials and filtered noise | `dsp::tone(hz, rate)`, `dsp::noise(seed)`, `dsp::mix(..)` | plugin (`dsp`) | medium |
| Trigger detection | `triggered` and the offset search | `dsp::trigger(samples, level, depth, Edge::Rising) -> Option<usize>` | plugin (`dsp`) | low |
| Musical notes | `tune`'s table of octave-0 pitches | `dsp::note_hz("A4") -> Option<f64>` | plugin (`dsp`) | low |
| Axis grid lines | the spectroscope's reference lines, drawn as 29 datasets of two points | `Axis::grid([20.0, 30.0, ..])` in the axis style, under the datasets | plugin (rs-rich-ext chart) | medium |
| A spectrogram (waterfall) | a fourth mode: spectrum against time, the newest at the top, as in cava, spek and Audacity | `Spectrogram::new(rows).push(spectrum)`, a heatmap over time | plugin (rs-rich-ext chart) | high |
| A level meter with peak hold | a VU or peak meter per channel beside the scope (Bubbles' progress, ratatui's `Gauge`/`LineGauge`, and every DAW) | `meter(level, peak).hold(Duration)` | plugin (rs-rich-ext) | high |
| A sparkline of a level | the level history in the header | ratatui's `Sparkline`, Textual's `Sparkline`: `sparkline(signal)` | plugin (rs-rich-ext) | low |
| Big text | a large clock or level readout (tui-big-text) | `big_text(markup)` over rs-rich-art's FIGlet | plugin (rs-rich-art) | low |
| A device picker | choosing among `audio::devices()` inside the app, as Bubble Tea's list does in audio TUIs | `audio::picker(on_pick)`, a list over the devices | plugin (`audio`) | medium |

## Round 2: files (Yazi)

`examples/files.rs` rebuilt on what round 1's gaps added (PR #752). It
uses intuiTUIve's own components and plugins only, with no framework
change in the round. Built with default features it follows the disk only
through what it does itself and describes images; the crate's `watch` and
`art` features (which CI's tapes job now builds the examples with) add the
watcher and image previews. Local times come from rs-rich-ext's
`local-time` feature, turned on for the examples and tests through a
dev-dependency, so `files` needs no `required-features`.

### Round-1 gaps now closed, and where files.rs uses them

| Round-1 gap or workaround | Now | Where in files.rs |
|---|---|---|
| Per-prefix chord pop-ups (`which()`) | `bind_sequence` on the root for all 31 sequences; `App::which_key(true)` draws the pop-up; the help and the palette list them | `SEQUENCES`, `files_app_with` |
| The root made focusable to own the keymap | `.previews_keys()` on the root column; the current list is focused (`autofocus`) | `files_app_with`, `current_column` |
| Two synced lists for the line mode, the switch on the measured width | one `list_lazy` with `.trailing(..)` (Yazi's padding cell included) | `current_column`, `linemode_cell` |
| Markers as a lead cell of the markup | `.marker(\|i\| style)` | `current_column`, `parent_column` |
| The selected style per row | `row_style` + the theme's `reverse` was already right; `.selected_style_for` gives the preview's underline | `preview_column` |
| `list` rebuilding every row | `list_lazy` in all three columns | the three columns |
| Re-derived `hsplit` maths (`split_at`, `columns()`, `body()`, `top_center`, `hovered_rect`) | `popup_at(Origin::TopCenter, Offset::new(0, 2, 80, 3))`; `RowRects` for the hovered row (`Origin::At`); `rect_handle()` for the preview's width and the page size | `Prompt`, `Act::Rename`, `open`, `preview_column`, `Fm::page` |
| The End-key caret workaround | `.caret(Caret::At(stem))`, Yazi's `--cursor=before_ext` | `Act::Rename` |
| The hand-made confirm box | `cx.confirm(title, body, items, on_yes)` | `Act::Paste`, `Act::Remove` |
| Toasts for errors | `cx.notify(Level, title, body)` + `App::toasts_at(Corner::TopRight)`, theme `notify.*` in Yazi's colours | `work`, the prompts |
| The one-row `leaf` line | `rule()`, joined to the help's border | `help` |
| Polling the directory every 2 s | `watch_path` on the directory and its parent, moved with it (`watch` feature) | `follow_disk` |
| UTC times, numeric `uid:gid` | `rich_ext::fs::short_local_time`, `local_time`, `user_name`, `group_name` | `short_time`, `owner`, `spot` |
| Hand-copied icons and type colours | `rich_ext::filetype::icon_with` (hovered flag), `style_for`, `mime_for_path`, `mime_from_bytes`, `mime_for` | `entity`, `row_style`, `load_preview`, `spot` |
| Images described | `image_of(path, ImageView)` (`art` feature) | `image_pane` |
| Opening files left to the shell | Yazi's `[open]` rules: `cx.handoff_then($EDITOR)` for text and folders, the system opener (`$OPENER`, `xdg-open`, `open`) in a `spawn`; `O` is Yazi's "Open with:" pick below the row | `openers_for`, `open`, `open_with` |
| A first Markdown preview stalling | `rich_ext::syntax::preload` at start-up | `main` |
| `Signal`-polled preview width | the pane's `rect_handle()` in the preview `watch` | `preview_column` |

Kept on purpose: the empty-list end of visual mode, the trash never moved
into itself, and tab clicks going through `switch_tab` (all tested).

### New gaps and bugs

| Gap or bug | Yazi feature that needs it | API sketch | Where |
|---|---|---|---|
| A list's highlight is drawn over the item's markup, so no cell of a selected row can escape it | Yazi's rounded indicator ends (U+E0B6, U+E0B4) on the hovered row with a Nerd Font: its spans patch over the line style, ratatui-fashion | `.highlight_layer(Layer::Under)`, so markup (`[not reverse blue]`) wins; or `.selected_caps(open, close)` | core |
| No left-cut overflow | The header's path loses its start (`ui.truncate(.., rtl = true)`); the example cuts the string from `viewport()` width minus the count's | `Overflow::EllipsisStart`, or `text(..).truncate_start()` | core (rich's `Overflow` is a mirror, so an intuiTUIve text option) |
| Which-key has no description style and no gap between its columns | Yazi's which: keys `bright_cyan`, `bright_black` separator, descriptions `bright_magenta`, columns padded; ours run into the next column's key (`(rever a Sort…`) | theme `which_key.desc`, `which_key.sep`; a cell of gap per column; right-aligned keys as Yazi | core (bug: the missing gap) |
| `cx.confirm` has a fixed 50-wide box, and an empty body still takes a row and a rule | Yazi's trash/delete confirm is 70×20 with no body; overwrite 50×15 | `cx.confirm_with(ConfirmOptions { size, .. })`; an empty body draws nothing | core (bug: the empty body) |
| `Signal::set` needs `PartialEq`; `RectHandle` and `PathWatch` are not | Keeping the watchers to stop them, keeping the column's handle in app state | derive `PartialEq` on both (compare by identity), or `Signal::replace` without the bound | core |
| A `RectHandle` exists only after its node is made; `RowRects` can be made first | App state wants the column's rect before the view is built: the example stores `Signal<Option<RectHandle>>` | `RectHandle::new()` + `node.rect_into(&handle)` | core |
| Image decoding is not a task `wait_for_tasks` waits for | Testing the image preview needs a polling `Driver` loop | count `image`'s decode as a task | core (`art`) |
| The headless driver records only a handoff's program | Asserting that `$EDITOR` got the hovered file | `Record.handoffs: Vec<(program, args)>` | plugin (rs-rich-interact) |
| A "keyed" resource | The preview: re-fetch when the hovered path or width changes, drop stale results, clear after a delay; the example keeps a generation counter and two `spawn`s | `resource_for(move \|\| key(), \|key\| fetch(key)).stale_after(150ms)` | core |
| MIME by content in the background | Yazi's type colours come from `file`-style sniffing done by its fetchers; the listing uses the name only | `filetype::mime_batch(paths) -> Vec<&str>` run in a `spawn`, merged into the listing | plugin (rs-rich-ext) |
| No archive extraction or system trash | Yazi's `extract` opener and its trash | see components below | plugin |
| Kitty/Sixel images | Yazi's pixel-exact previews | screen pass-through for graphics escapes | core + rs-rich-art |

### Friction with the new APIs

- **`bind` vs `bind_sequence`**: in `bind("k up", ..)` a space separates
  alternatives, in `bind_sequence("g g", ..)` keys pressed in turn. Easy to
  mix up when moving a binding between the two tables; a sequence with
  one key silently becomes a `bind`. A `"g>g"` or `["g", "g"]` form would
  read unambiguously.
- **The comma**: `"\",\" s"` for `,s` is hard to read; `"comma s"` would
  be kinder.
- **Pop-up title styles**: `popup.title` styles every pop-up's title, but
  Yazi's confirm titles are blue and its input titles plain. Prompts set
  `.title_style("default")` by hand; `cx.confirm` cannot take a style.
- **`selected_style_for` was not needed** for Yazi's coloured bar:
  `row_style` plus a `reverse` highlight already gives it. Its doc example
  suggests it for that case; it earns its keep only for a different look
  per row (the preview's underline here could equally be `theme_styles`).
- **`rule()` joins the border only with no padding** between it and the
  panel, so the help pads its field and list one by one instead of the
  column; a `rule().inset(..)` or padding that rules ignore would help.
- **`popup_at` with `Origin::At(rect)`** puts the box below the rect and
  then clamps it on screen; at 80 columns an 80-wide rename box slides to
  the left edge, away from the row (Yazi does the same, but a
  `w: Size::Percent` or "shrink before moving" option would keep it
  under the row).
- **`notify` has no width or wrap control**: the 48-cell box wraps a
  one-line error at 80 columns.
- **`list_lazy`'s closures** re-read the same memos for `len`, `item`,
  `trailing`, `marker` and `row_style`; a `list_of(memo, |entry, i| Row
  { text, trailing, marker, style })` would read the data once per row.

### Components that would help

| Component | What it is | In files.rs it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|---|
| Miller columns | parent, current and preview panes over one cursor, with rails, remembered positions and click-to-navigate | `parent_column`, `current_column`, the preview's dir list, the `remember` map, three `on_mouse` handlers (~300 lines) | `miller(source: impl Tree, cursor).ratios(1, 4, 3).preview(\|node\| ..)` | core (or rs-rich-ext) | high |
| File list | a lazy list of `Entry`s with icon, type colour, marker, line mode and find highlights built in | `entity`, `row_style`, `marker_style`, `linemode_cell` | `file_list(entries, cursor).linemode(Linemode::Size).icons(true).markers(\|p\| ..)` | plugin (rs-rich-ext `filetype` + intuituive) | high |
| Status bar with segments | left/right segments with powerline caps between them, Yazi's and lualine's | `status`, the `cap` closures | `status_bar().left(seg(" NOR ", main)).right(..).caps(Caps::Rounded)` | core | medium |
| Breadcrumb / path header | a path that cuts from the left, with `~`, and flags after it | `header`'s cwd text | `path_header(path).home_tilde().flags(..)` | core | medium |
| Preview pane by type | picks a renderer from the MIME type: code, Markdown, image, directory, hex, archive listing; worker-loaded, stale-safe | `load_preview`, `Preview`, `preview_column`'s switch, the generation counter | `preview(path_signal).with(Previewer::code()).with(Previewer::image())` | plugin | high |
| Tab strip that hides below two tabs | `tabs` that collapse with one tab, number titles, and report clicks as a change event | `tab_strip`'s `switch`, `clicked` signal and two watches | `tabs(titles, active).auto_hide().on_change(\|to, cx\| ..)` | core | medium |
| Key-hint footer | the keys that work now, from the bindings (Bubbles' `help`, Textual's `Footer`) | the help modal for the common case | `key_hints().from_focus()` | core | medium |
| Input prompt preset | a titled one-line input at an origin, with change/submit/cancel (Yazi's `input`, Bubbles' `textinput` in a box) | the `Prompt` builder (~80 lines) | `cx.prompt(title, Origin, Offset).text(..).caret(..).on_change(..).open(\|text, cx\| ..)` | core | high |
| Pick menu at a point | a short list in a box at an origin, Enter picks (Yazi's `pick`) | `open`'s `popup_at` + `list` | `cx.pick(title, Origin, items, \|i, cx\| ..)` | core | medium |
| Openers | Yazi's `[opener]`/`[open]` rules: MIME globs to commands, blocking (handoff) or in the background | `Opener`, `openers_for`, `open_with`, `system_opener` | `rich_ext::open::rules().matching(mime) -> Vec<Opener>`; `opener.run(cx, paths)` | plugin (rs-rich-ext) | medium |
| Directory model | a watched, sorted, filtered listing with natural sort and stat caching (Yazi's `Folder`) | `read_dir`, `sort_entries`, `natural`, `follow_disk`, the listing memos | `dir_model(path).sort(..).hidden(false).watch()` -> `Memo<Vec<Entry>>` | plugin | high |
| File operations | copy/move/trash/delete with progress, a free-name rule and "not into itself" checks (Yazi's scheduler) | `copy_all`, `move_to`, `paste`, `unique`, `work` | `fs_ops::paste(yank, dir, Conflict::Rename).spawn(progress)` | plugin (rs-rich-ext `fs`) | medium |
| FreeDesktop trash | the XDG trash spec (and macOS/Windows bins), restore | the `.trash` folder stand-in | `rich_ext::trash::put(path)`, `list()`, `restore(item)` | plugin | medium |
| Archive listing | list tar/zip/7z contents as a preview, extract (Yazi's `archive` previewer, `extract` opener) | the "binary data" note for archives | `archive::entries(path)`, `archive::extract(path, dir)` | plugin (new) | medium |
| Git status per file | the `M`/`A`/`?` signs Yazi's git plugin shows | a line-mode column | `git::status(dir) -> HashMap<PathBuf, Status>` | plugin (new) | medium |
| Fuzzy finder over the tree | fzf/`fd` as a component that streams results (Yazi's `z`, Television, Bubbles' `list` filter) | `jump`'s blocking 20,000-entry walk | `fuzzy_find(walk(root).hidden(false)).stream()` | plugin (rs-rich-interact `Jump` + a walker) | medium |
| Task manager | running file jobs with progress (Yazi's `w`) | errors-only notifications; enables progress | `tasks_panel(scheduler)` | core | low |
| Hex view | a hex dump previewer (ratatui's `hexyl`-style widgets) | the binary note | `hex_view(bytes)` | plugin (rs-rich-ext has `hex`) | low |

## Round 2's gaps in the core: widgets, pop-ups and the clock

The core items of round 2 above, and those the plugin work (files, audio,
charts) asked for, were added; the clones are rebuilt on them in the next
round.

| Gap | Now |
|---|---|
| Highlight over the markup | `.highlight_layer(HighlightLayer::Under)` |
| No left-cut overflow | `Node::truncate_start()`; `breadcrumb(path)` |
| `cx.confirm` 50 wide, empty body | `cx.confirm_with(.., ConfirmOptions::new().size(w, h).title_style(..), ..)`; no row or rule for an empty body |
| Pop-up title styles | a pop-up's panel's `.title_style` already won over `popup.title`; `ConfirmOptions`, `PromptOptions` and `PickOptions` take one |
| `rule()` and padding | a rule runs across the padding it touches to the border |
| `popup_at` sliding away | `Origin::At` shrinks the box to the room under the rectangle before moving it |
| `notify` 48 cells | `App::notify_width(size)` |
| `list_lazy`'s five closures | `list_rows(len, \|i\| ListRow, selected)` + `.marker_column()` |
| `selected_style_for`'s docs | corrected |
| Input prompt, pick menu | `cx.prompt` / `prompt_with`, `cx.pick` / `pick_with` |
| Status bar, breadcrumb, key-hint footer, live plot, percentage cells | `status_bar` + `seg` + `Caps`, `breadcrumb`, `key_hints`, `live`, `cells` |
| Tab strip hiding, click routing | `tabs(..).auto_hide().on_select(..)` |
| One-shot timer, stepping the clock | `after` / `cx.after`, `Driver::advance`; `stale_after` on the app's clock; `DrawCx::now()` |
| Sequences in sections, cycling back | `bind_sequence_in`, `bind_cycle_back` |
| An app dropped unfinished | dropping an `App` closes its runtime |
| A non-`Send` continuation, a stream as a task | `spawn_then`, `spawn_stream` |
| Cleanup when a node goes | `Node::on_unmount` |
| `render_with` needing a quit | it settles after its keys and reads the screen |
| The wheel over a list | `.wheel_step(n)`, `0` passing it on |
| Accessible text for a widget | `Widget::access_text` |
| Kitty/Sixel pass-through | `App::graphics(PlacementSource)` after each frame's cells; the `image` node still draws cells (below) |

### New gaps and bugs

| Gap or bug | Found where | API sketch | Where |
|---|---|---|---|
| The `image` node places no graphics | `App::graphics` draws what a `PlacementSource` finds in the cells, but `image` writes half blocks with nothing to find. Wanted: with the terminal's support detected, `image` writes Kitty placeholder cells (`rich_art::kitty::placeholder_row`, the image sent once with `transmit`) or reserves its rect for a Sixel overlay, and an app-level source maps them to placements | `ImageView::graphics(Auto)`; a `GraphicsRegistry` the image node registers with, given to `App::graphics` by the `art` feature | core (`art`) + rs-rich-art |
| `switch` keeps every page it built | `on_unmount` never runs for a page switched away from, and a switch keyed by a path grows without end | `switch(..).forget_hidden()`, or a `switch_fresh` | core |
| `after` cannot be cancelled | a debounce restarts its wait; today the handler checks a generation | `after(..) -> Timeout` with `cancel()` | core |
| `truncate_start`'s `…` is unstyled | a cyan path gets a plain `…` | the `…` in the first kept cell's style | core |
| A `rule` in a row is one cell wide | `rule()` is `Size::Fixed(1)`, meant for columns | size a rule `Flex(1)` across a row | core (bug) |
| `key_hints` leaves out the app's keys | the palette and help keys (`App::help_key`) and sequences are not listed | include them, or `key_hints().with(..)` | core |
| `live` measures without a cap | sized `Auto`, each layout renders it | measure from the last draw | core |

### Components that would help

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| A debounced signal | the find-as-you-type and preview reloads built from `after` and a generation | `debounced(signal, 150ms) -> Signal<T>` | core | medium |
| A mode line | `status_bar` segments that follow `bind_cycle`'s mode, its colour per mode | `mode_segment(mode, &[(Mode::Normal, "NOR", "blue"), ..])` | core | low |
| A footer that pages | `key_hints` showing more on `?` (Bubbles' short and full help) | `key_hints().expand_on("?")` | core | low |

## Round 3: scope-tui

[`examples/scope.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/scope.rs)
rebuilt on what round 2's gaps added (the audio plugin rounded out,
rs-rich-ext's `dsp` generators and chart options, and the core's clock,
key and bar components), with no framework change in the round and no
hand-made stand-in left in the example. Its hand-rolled test signal, PCM
reader, `Frame` numbering, trigger search, note table, 29 reference
datasets, polled fps counter and the 24 modifier bindings are gone; it is
1,323 lines where round 2's was 1,340, while doing more of what scope-tui
does (`audio --timeout`, `audio --list`'s configurations, PulseAudio's
sources and `--server-buffer`). `tests/scope.rs` counts frames exactly on
the app's clock and checks the new behaviour (24 tests; 25 with `audio`).

### Round-2 gaps now closed, and where scope.rs uses them

| Round-2 gap or workaround | Now | Where in scope.rs |
|---|---|---|
| A watch that runs on every write (`Frame { number, data }` numbered by hand) | `on_write(samples, ..)` takes every buffer, silence included; the example's `Frame` is gone | `scope_app`, the `fps` match |
| Frames per second from a counter and a one-second `every` | `rate(samples)` | `scope_app` |
| `latest` threads and the headless clock | `latest_every(period, ..)` makes the test signal; `Latest::sleep` keeps a file's `-l` beat and the silence after it; tests run with `wait_for_tasks(true)` and count frames exactly (but see the first bug below) | `Input::Every`, `play`, `tests/scope.rs` |
| An app builder cannot fail (a `watch` that fails on the first turn) | `audio::open(AudioOptions)` before the app: a missing device fails before the terminal is taken, so `App::try_new` is not even needed | `input`, `capture_options` |
| Two shapes for "the newest value" | `audio::input`/`samples()` are `Signal<Option<Frame>>`, as `latest`'s; one `on_write` arm per source kind remains (see friction) | `scope_app` |
| Device names are descriptions | `Device { id, .. }`: `--list` prints ids, as scope-tui (cpal 0.15's names are ALSA ids), and `audio DEVICE` opens by id | `list_devices`, `input` |
| Device configurations | `Device::configs()`, printed in scope-tui's `  + 2ch 8000-192000hz 32-65536buf (i16)` form | `list_devices`, `main` |
| A stalled stream (`--timeout` read, not enforced) | `AudioOptions::stall_after` + `capture.status()`: `Stalled` ends the app with scope-tui's "data source returned null", printed after the terminal is back as `[!] Error executing app: …` | `scope_app`, `STALLED`, `main` |
| PulseAudio and PipeWire sources | the `pulse` feature's `Host::Pulse` (every source, sink monitors among them); `--server-buffer N` as `latency(N × buffer)`, scope-tui's `maxlength` | `pulse_options`, `input` |
| ALSA writes to stderr | kept off the screen inside the plugin; nothing in the example | — |
| Help sections | `.bind_in("oscilloscope" \| "spectroscope", ..)` | `scope_app` |
| Modifier variants of a binding (24 bindings) | `.bind_scaled(key, &MAGNITUDES, ..)`, six bindings, one help row each | `scope_app` |
| A mode switcher | `.bind_cycle("tab", .., mode, &Mode::ALL)`; `Mode::next` gone | `scope_app` |
| A row of percentage cells | `cells([(35, ..), ..])` | `header` |
| A PCM reader (`Pcm`, `deinterleave`) | `audio::pcm(reader, Format::S16Le, channels, buffer)` | `input` |
| A signal generator (`Demo`'s sines and xorshift) | `dsp::partials`, `tone(..).phase(..)`, `noise(..).low_pass(..)`, `mix` | `Demo::new` |
| Trigger detection (`triggered` and its search) | `dsp::trigger(.., Edge)` | `Oscilloscope::trigger_at` |
| Musical notes (`tune`'s table) | `dsp::note_hz` (see friction: one note in ten is a sample off scope-tui) | `tune` |
| Axis grid lines (29 two-point datasets) | `Axis::grid([..])` with `Axis::style(axis colour)` | `chart`, `Plot::x_grid`/`y_grid` |
| `legend_style("")` | `Chart::style("none")`, which the legend takes | `chart` |
| `dsp::spectrum_full` | used for the points; `fft` stays for the phase difference | `spectroscope` |
| `style_from_color_name` in rs-rich-ratatui | `rich_ext::palette::style_from_color_name`; the example no longer touches rs-rich-ratatui | `parse` |

Not used, on purpose: `live` (scope-tui draws every buffer it reads, with
no cap), `audio::wav`/`record`, `audio::picker`, `meter`, `sparkline`,
`big_text`, `Spectrogram`, `status_bar` and `key_hints`: scope-tui has
none of them, and extras are added only where scope-tui has them.

### New gaps and bugs

| Gap or bug | Needed for | Repro or API sketch | Where |
|---|---|---|---|
| **Bug: values from other threads are picked up at most every 50 ms.** The run loop waits on the terminal for `Driver::timeout` (50 ms at most) and a `latest` send or a capture's buffer does not wake that wait, so a source faster than 20 a second is capped at 20 in a real terminal; the headless clock does the same (`Driver::advance` and `Script::wait` step to the next timer or 50 ms, never to a `Latest::sleep` due) | scope-tui's frame rate: its default 2048-sample buffer at 48 kHz is 23 fps, `-b 1024` 47 fps; the clone shows 20 for both (paused, so drawing is not the cost; `-b 4096` shows its true 12). Tests step the driver by hand every 10 ms (`step` in `tests/scope.rs`) | `scope --still -b 1024`, Space, wait 3 s: `20fps`. Headless: `latest_every(10 ms, ..)` counted by `rate` under `wait_for_tasks(true)`, `driver.advance(2.1 s)`: 20, not 100. Fix: a proxy send wakes the backend's read (a self-pipe or the backend's waker), and `timeout` takes the earliest due of the threads asleep on the app's clock | core (generic) |
| **Bug: grid lines share one layer.** Under `Layering::PerDataset` an x and a y grid line that cross merge their dots (`⣸`); scope-tui's two reference datasets draw the second over the first (`⢸`). Pinned by `reference_lines_are_drawn_as_scope_tuis_datasets_are` | The vectorscope's cross, faithful cell for cell | `Chart` with `x_axis(..grid([0]))`, `y_axis(..grid([0]))`, `Layering::PerDataset`, no trace at the centre. Fix: a layer per grid line (y lines first, as scope-tui orders them) | plugin (rs-rich-ext chart) |
| **Bug: a grid with no datasets draws in `Marker::Dot`.** The grid takes the first dataset's marker, so before the first buffer the zero line is dots, then Braille | The first frame's zero line | `Chart::new(Vec::new()).y_axis(Axis::default().bounds(-1., 1.).grid([0.]))`: `•` cells. Fix: `Axis::grid_marker(..)` or `Chart::marker(..)` as the default | plugin (rs-rich-ext chart) |
| Bindings active only in a state | scope-tui's per-mode keys: `t`, `e`, `=`, `w`, `l` do nothing outside their scope but are bound (and listed) always; `p` means peaks in one scope and phase difference in another, so it is one binding with one description | `.bind_when(\|\| mode.get() == Osc, keys, desc, f)` (listed in the help only while active; two bindings may share a key under different conditions); or a `switch` whose shown page's bindings apply and are listed in its section | core (generic) |
| Sections for every kind of binding | PgUp/PgDn are the oscilloscope's and the spectroscope's, but `bind_scaled` (and `bind_cycle`) take no section | a `.section("oscilloscope", \|node\| node.bind_scaled(..))` scope, or `_in` variants of each | core (generic) |
| Named punctuation keys | `=`, `-`, `+`, `_` are written `"\"=\""` (round 2 added `comma` only) | `equals`, `minus`, `plus`, `underscore`, `slash`, … wherever keys are parsed | core (generic) |
| Testing a capture's status | `--timeout` (a `Stalled` capture) cannot be tested without a sound card that stops; the test covers only the open error | `audio::Capture::simulated(frames, period)` with `stall()` / `fail(why)`, built on `latest` | plugin (`audio`) |
| A thread feed with a state | a file's end, a pipe closed, a stall: the example's file source cannot say it ended (it pads with silence, as scope-tui), and only a capture has a status | core: `latest_with_state(..) -> (Signal<Option<T>>, Signal<FeedState>)` with `Running`, `Stalled(since)` after `stall_after`, `Ended`, `Failed(why)`, for any thread (a sensor, a log tail, a socket); plugin: `audio::Capture::status` built on it | core (generic) + plugin (`audio`) |
| `audio::pcm` moving behind a feature | Another round-3 change gates `audio::pcm`/`Format`/`deinterleave` behind a light `pcm` feature; the example and `tests/scope.rs` read files with it in a default build | the example then needs `required-features = ["pcm"]` (or `audio` implying `pcm` and the file source behind `cfg`), and CI's tapes job the feature | plugin (`audio`) |

### Friction with the new APIs

- **One `on_write` arm per kind of source.** `latest_every` and `latest`
  give `Signal<Option<Matrix>>`, a capture `Signal<Option<Frame>>`, so the
  example writes the same `on_write` + `rate` three times. With `on_write`
  seeing every write, `Frame::number` is no longer needed to count; a
  generic `latest_iter(iter, pace)` (below) and an audio source enum would
  make a source one value again.
- **`on_write` clones each value** (here a 2×2048 matrix per buffer, which
  `take` then clones again into the history). A borrowing
  `on_write_ref(signal, |&T, cx| ..)` would save both.
- **`dsp::note_hz` is not scope-tui's table.** It is equal temperament
  with octave 4 by default; scope-tui doubles octave-0 pitches rounded to
  the hundredth and defaults to octave 0. The example adds the `0`; about
  one note in ten (E2, F1, B1, … at 48 kHz) gives a buffer one sample
  off. More correct, less faithful; a test pins E2.
- **`Axis::grid` styles with `Axis::style`**, the axis line's style too.
  scope-tui's axes have no lines, so it fits here; an app with a styled
  axis and fainter grid lines would want `Axis::grid_style`.
- **`Chart::style("none")`** reads well once known; `"none"` as a magic
  string is easy to miss next to `""`.
- **`bind_scaled`'s help prints `alt ×0.2`**, where scope-tui's docs say
  ⅕; fine, but a label per variant (`("alt", 0.2, "⅕")`) would let an app
  say it its way.
- **`audio::open` made `App::try_new` unnecessary.** Opening before the
  app is the cleaner shape; `try_new` stays for builders that only fail
  inside.
- **`Driver::advance`'s 50 ms step** is documented, but nothing says a
  `latest` thread is not a "thing due" (see the first bug); the
  `latest_every` doc example steps `driver.update` by hand, which hides it.
  Round 2's "19–20 fps where scope-tui shows 21–22" was put down to a debug
  build; it is this cap.
- **The help cuts long rows.** `bind_scaled` appends `(shift ×10, ctrl ×5,
  alt ×0.2)`, 30 cells, and the help box (75 wide at 110 columns) cuts a
  row rather than wrapping it, so PgUp's description had to shrink to
  "threshold or avg up".

### Components that would help

Patterns written by hand from several built-ins, what scope-tui leans on,
and what comparable audio and visualisation TUIs and libraries offer
(cava, ratatui and its crates, Bubble Tea and Bubbles, Textual). Core
entries are generic: another, unrelated component could use them.

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| Mode-scoped views | the `in_mode` checks in eight handlers, the shared `p`, and per-mode help: a page per mode whose bindings apply, and are listed under its name, only while it shows (Textual's screens' `BINDINGS`, Bubble Tea's per-model `Update`) | `modes(mode, &[(Mode::Osc, "oscilloscope", \|\| node.bind(..)), ..])` | core (generic) | high |
| A feed with a state | the file source's end, the capture's status, `--timeout` for any source | `latest_with_state` (see the gaps), and `audio::Capture` built on it | core (generic) + plugin (`audio`) | high |
| An iterator played at a pace | `play`: frames sent as they come or a period apart, then something after (silence) | core: `latest_iter(iter, Pace::Every(d) \| Pace::Free).then(after)`; plugin: `audio::play(pcm, rate)` on it | core (generic) + plugin (`audio`) | medium |
| A history of writes | `take`'s per-channel `VecDeque`s for the spectroscope's averaging; also an oscilloscope's persistence | core: `history(signal, n) -> Memo<VecDeque<T>>`, `n` a signal; plugin: `chart::Persistence` (older frames fading, as an analogue scope's phosphor, xoscope, glscopeclient) on it | core (generic) + plugin (rs-rich-ext chart) | medium |
| Bindings that toggle or step a field | ten closures of `signal.update(\|g\| g.x = !g.x)` and the clamped steps of `nudge`/`samples`/`page` | `.bind_toggle(keys, desc, signal, \|s\| &mut s.flag)`, `.bind_step(keys, desc, signal, \|s\| &mut s.n, step, range)` | core (generic) | medium |
| A bar spectrum, cava's | a fourth view, the one most audio TUIs show: log-spaced bands, gravity fall-off, smoothing, mirrored stereo | plugin: `dsp::bands(spectrum, n, lo_hz, hi_hz)`, `dsp::Falloff::gravity(g)`; `chart::BarSpectrum::new(bands).mirror(true)`; core underneath: a `smoothed(signal, attack, release)` numeric signal (Bubbles' harmonica springs), usable for any meter or progress | core (generic) + plugin (rs-rich-ext dsp/chart) | high |
| A tuner readout | the note nearest the loudest peak, with cents, beside the header (guitar tuners, `tuner` TUIs) | plugin: `dsp::peak_hz(spectrum)` + `nearest_note`; shown with `big_text` | plugin (rs-rich-ext dsp) | low |
| Big digits | a level or frequency readout (Textual's `Digits`, tui-big-text) | `big_text` exists behind `art`; a `digits(text)` in box-drawing glyphs with no FIGlet would suit every build | core (generic) | low |
| A fake capture | `--timeout` and device failures under test (see the gaps) | `audio::Capture::simulated(..)` | plugin (`audio`) | medium |
| Already there, not in scope-tui | `Spectrogram` (spek's, Audacity's waterfall), `meter`/`PeakHold` (every DAW), `sparkline` (ratatui's, Textual's), `audio::picker` (Bubbles' list), `audio::record` | an extended scope, not this clone | plugin | — |
