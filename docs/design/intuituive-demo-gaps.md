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
