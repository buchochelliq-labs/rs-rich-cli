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

**Core components are generic.** A core component must be reusable by
other components: no domain types, names or parameters (no paths, files,
samples, sound cards, or one app's or one data source's shapes). Anything
only one domain could use is a plugin: an off-by-default feature module
(`files`, `pcm`/`audio`, `watch`, `art`) or another crate. Where a domain
component has a generic shape underneath, the generic part goes in core
and the plugin builds on it: `files::miller` is the core's `miller_of`
over a directory tree, `files::tasks` the core's `jobs` running file
operations, `files::finder` the core's `finder_of` over a walk,
`files::preview` the core's `preview_of` keyed by a path, and
`files::path_crumb` the core's `breadcrumb` over a path's parts.

This is a gate, not a preference. A proposal for core that is not generic
is not accepted into core: it is reshaped as a generic core piece plus a
plugin built on it, or kept as a plugin. So every gap report and every
**Components that would help** entry that proposes a domain component
must also propose the generic core shape under it (or say why there is
none, and mark it plugin). The tables below predate the rule where they
name a domain shape as core; such an entry is read as its generic shape.

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
| Breadcrumb / path header | a path that cuts from the left, with `~`, and flags after it | `header`'s cwd text | `path_header(path).home_tilde().flags(..)`; landed as core `breadcrumb(parts)` + `files::path_crumb(path)` | core + plugin (`files`) | medium |
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

## Round 3: files (Yazi)

`examples/files.rs` rebuilt on round 3's additions: the `files` plugin
(`intuituive::files`, rs-rich-ext's `listing`, `open`, `file_ops`, `trash`,
`git`, `archive`) and the core's bars, tabs and dialogs. No framework
change in the round. It went from 2,756 lines to 1,450: what is left is
Yazi's keymap, what each key does, tabs, the selection and visual mode,
the help and the spotter. It now needs the crate's `files` feature
(`required-features = ["files"]`, so a default build and `--all-targets`
skip it); `archive`, `watch` and `art` add to it, and CI's tapes job
builds it with all four.

A rule from this round on: a **core** entry below names a component that
another, unrelated component could reuse, with nothing about files, paths
or Yazi in its name or parameters. What is about files is a **plugin**
entry (the `files` feature); where a file component has a generic shape
underneath, both are proposed: the generic core piece and the plugin
built on it.

### Round-2 gaps now closed, and where files.rs uses them

| Round-2 gap or component | Now | Where in files.rs |
|---|---|---|
| Miller columns, file list, directory model, preview by type (~700 lines of round 2) | `files::miller(start).icons(..)`, `view_with(MillerOptions::new().markers(..).git(..))`; its `cwd`, `cursor`, `hovered`, `current`, `hidden`, `sort`, `linemode`, `find`, `preview_top` signals and its `arrow`/`page`/`enter`/`leave`/`cd`/`refresh` | `Fm`, `columns`, `run` |
| MIME by content | `DirModel::sniff`, inside `miller`; `m.current.mime(&entry)` | the rows' colours, `spot` |
| The keyed preview with a generation counter | `files::preview` inside `miller` (code, images with `art`, archives with `archive`, directories, hex) | gone from the example |
| Openers (`Opener`, `openers_for`, `system_opener`, `editor`) | `files::opener_rules()` (`Rules::matching`, `shared`), `files::open_with` | `open`, `launch` |
| File operations, a free name, "not into itself" | `files::tasks()` + `Op::{Copy, Move, Delete, Trash}` with `Conflict::{Rename, Overwrite}` | `Fm::job`, `paste`, `remove` |
| No system trash (a `.trash` folder stood in) | `Op::Trash`: the FreeDesktop trash | `remove`; tested with `XDG_DATA_HOME` |
| Archives described as binary | the `archive` preview; `Launch::Extract` through `Tasks::extract` | `launch`; `archives_are_listed_in_the_preview_and_extracted_by_enter` |
| Git signs | `files::git_status(m.current)` into `MillerOptions::git` | `columns`; `git_signs_show_beside_the_line_mode` |
| `jump`'s blocking 20,000-entry walk | `files::finder(walk(cwd).hidden(..))`, streaming | `jump`; `z_finds_a_file_below_here_and_goes_to_it` |
| Task manager | `files::tasks_panel(jobs)` in a modal on `w`; `Tasks::running()` for Yazi's quit confirm | `task_manager`, `quit`, the status line's progress |
| The `Prompt` builder (~80 lines) | `cx.prompt_with(.., PromptOptions::new().title_style("default").at(..).caret(..).on_change(..).on_cancel(..), ..)`, Yazi's 50 wide | every prompt in `run`; `prompts_are_yazis_50_wide_at_the_top_centre` |
| "Open with:" as `popup_at` + `list` | `cx.pick_with(title, Origin::At(row), items, PickOptions::new().offset(..), ..)` | `open` |
| `cx.confirm` 50 wide, an empty body taking rows | `cx.confirm_with(.., ConfirmOptions::new().size(..), ..)`: Yazi's 70 x 20 trash, 50 x 15 overwrite, the quit confirm | `remove`, `paste`, `quit`; `the_trash_confirm_is_yazis_70_by_20` |
| The status line's hand-made caps | `status_bar(left, right)` of `seg(..).style_with(..)`, `.caps(Caps::Rounded)` with icons | `status` |
| The header's left cut from `viewport()` | `breadcrumb(..).home_tilde().crumb_suffix(..)`, cyan through the theme's `breadcrumb*` styles | `header` |
| `tab_strip`'s `switch`, `clicked` signal and two watches | `tabs(..).auto_hide().on_select(\|to, _\| fm.switch_tab(to))` | `tab_strip` |
| Which-key columns running together, no description style | the fixed columns; theme `which_key.sep`, `which_key.desc` in Yazi's colours | `theme` |
| `"\",\" s"` | `"comma s"` | `SEQUENCES` |
| A rule in a padded column not joining the panel | fixed: the help pads its column, not each child | `help` |
| `RectHandle` in a signal, `Signal<Option<RectHandle>>` | not needed: Miller keeps its column's rect (`m.row_rects()`, `column_rect()`) | `open`, `Act::Rename` |
| The headless record kept only the program | `Record::handoff_commands` | `enter_hands_the_terminal_to_the_editor_as_yazi_does` asserts the file |

Newly faithful, and tested: the system's trash; file jobs in the
background and a task manager; quitting asks while tasks run; git signs;
type colours sniffed from content; archive listings and extraction; the
finder's streaming walk; 50-wide prompts; Yazi's confirm sizes; a click in
the parent column goes into that directory (Yazi's `cd`; round 2 went to
the parent, hovering it, and its test now says so). One assertion changed
for the worse: a binary file previews as a hex dump, not Yazi's `file`
classification (below).

Not used, on purpose: `key_hints` (Yazi has no footer), `notify_width`
(the default fits Yazi's errors at 80 columns), `Miller::navigation` (see
friction).

### New gaps and bugs

| Gap or bug | Needed for | API sketch | Where |
|---|---|---|---|
| **Bug:** work that a layout starts (a watch on a `rect_handle`) is not settled before the next input event. `files::preview` loads only once its pane has a width, so an app's first frame shows no preview and an event sent right away (a click on a directory's preview) finds nothing. Repro: `run(files_app(root).wait_for_tasks(true), Script::new().keys("q"), 80, 10)` in tests/files.rs: the preview column of the hovered `alpha` is empty; `Script::new().wait(50ms).keys("q")` shows `a.txt`. The tests wait first (`first_frame()`) | a correct first frame; clicks and keys at start-up | a turn that lays out runs the watches its new rects woke, and their tasks under `wait_for_tasks`, before the next event; or `preview` loads at a provisional width | core (generic: any watch on a rect) |
| **Bug:** list options on a node that wraps a list do nothing, silently. `file_list` returns a `switch` (for its "No items"), so `.highlight_layer(..)`, `.wheel_step(..)` or `.selected_style_for(..)` on it are ignored | Yazi's rounded ends on the hovered row (round 2's gap, still open for the clone); any wrapped list | a wrapper forwards list options to the list it holds, or warns once in debug builds that an option met no list | core (generic) |
| The hovered row's rounded ends with a Nerd Font | Yazi's hovered row | `FileListOptions::hovered_caps(open, close)` and `MillerOptions::hovered_caps`, drawing the ends in the row's colour on `highlight_layer(Under)` | plugin (on core's `highlight_layer`) |
| A binary file previews as a hex dump | Yazi's `file` previewer: `file -bL`'s classification ("data", "ELF 64-bit…") | a built-in `Previewer::file_type()` (the `file` command, else a description of the MIME type), chosen by `PreviewOptions::binary(BinaryPreview::FileType \| Hex)` | plugin |
| One cwd: tabs, each with a back/forward history, are app code (`Tab`, `history`, `switch_tab`, `close_tab`, `swap_tab`, a watch on `m.cwd`: ~140 lines) | Yazi's tabs, H and L | core: `history(signal) -> History` (back/forward over any signal's past values: `back()`, `forward()`, `can_back()`), and `tab_set(make: Fn() -> T) -> TabSet<T>` (states with an active index: `open`, `close`, `swap`, `switch`, `titles`) for `tabs(..)`; plugin: `files::tabs(miller)` giving each tab its cwd and history | core + plugin |
| Selection and visual mode are app code (`selected`, `visual`, `end_visual`, `targets`, `marker`: ~100 lines) | Space, Ctrl+A, Ctrl+R, v, V, and what y/x/d act on | core: `multi_select(keys) -> MultiSelect<K>` over a list's items by key, with `toggle`, `select_all`, `invert`, a range anchor that follows a cursor (`visual(unset)`), `targets(hovered)`; plugin: `MillerOptions::selection(sel)` for the markers and `Miller::targets()` | core + plugin |
| Yank state and paste are app code | y, x, p, P, the copied and cut markers | `files::Yank` (paths, cut) with `markers` and `paste(into, conflict) -> Op` | plugin |
| Find next/previous over the list is app code (`find_arrow`, ~25 lines); `Miller::find` only highlights | /, ?, n, N | core: `list_search(query, items, cursor)` with `next()`, `previous()`, `place() -> (n, of)` (smart case); plugin: `Miller::find_next(previous)` | core + plugin |
| The hovered entry is kept by hand across a re-sort, filter or hidden toggle (`keep`) | `,s`, `f`, `.` | core: a list cursor kept by key (`cursor_by(\|item\| key)`): when the items change, the selection follows the same key; plugin: Miller's cursor by name | core + plugin |
| A right-click menu on a row needs `cx.pointer()` and `row_rects` maths: Miller's lists are not reachable for `on_mouse` | the menu on a row | core: `list.on_context(\|index, cx\| ..)` (a right press or the menu key on an item); plugin: `MillerOptions::on_context(\|entry, cx\| ..)` | core + plugin |
| `open_with` extracts with a plain `spawn`; to show it as a task the example matches `Launch::Extract` itself and calls `Tasks::extract` | Yazi's extract as a task | `open_with_tasks(cx, opener, paths, jobs, done)`, or `Tasks` optional in `open_with` | plugin |
| No progress segment for the status line; the example prints the share and count | Yazi's progress gauge | core: `seg_gauge(fraction, label)`, a `status_bar` segment filled to a share; plugin: `Tasks::summary() -> (fraction, running, failed)` | core + plugin |
| `Miller` keeps the directories visited private (`remembered(dir)` only) | Z (zoxide) over what was visited; the example keeps its own set | `Miller::visited() -> Vec<PathBuf>`, and a frecency store (`files::frecency(path)`) for Z across sessions | plugin |
| The finder walks a tree only; Z uses rs-rich-interact's `Jump`, so two finders look different | one look for z and Z | core: a streaming fuzzy list over any item source (`fuzzy_list(source)`); plugin: `files::finder(walk)` on it (the refactor under way) | core + plugin |
| `finder` has no Esc, title or border; the example adds them | z in a modal | `finder(..)` closes its modal on Esc (or `.on_cancel`) | plugin (on the core finder) |
| The trash on macOS and Windows fails ("Unsupported") | `d` there | `trash::put` through the Finder (AppleScript or `NSFileManager`) and the shell's recycle bin (`SHFileOperation`) | plugin (rs-rich-ext) |
| Tab titles are cut to the width by hand, and the unselected style is markup in each title | Yazi's strip | core: `tabs(..).fit()` (titles cut with `…` to share the width) and a theme style `tabs.unselected` | core |

### Friction with the new APIs

- **`Miller::navigation`'s descriptions are its own** ("Move up", "Go to
  the top"), not Yazi's ("Previous file"), and its key set is fixed (no
  `shift+pageup`). The help and the which-key pop-up show descriptions,
  so the example binds the moves itself with `m.arrow`, `m.page`, …. A
  table form (`navigation_with(&[(Move::Up, "k up", "Previous file")])`)
  would let a clone keep its wording.
- **Two ways to reach a column**: Miller's lists are built inside
  `view_with`, so anything not in `MillerOptions` (an `on_mouse`, list
  options, a name) has to go on the whole view. Every option a clone
  needs has to be added to `MillerOptions` by hand; a
  `MillerOptions::current(|list| list.option(..))` hook would end that.
- **Which signals are public is uneven**: `cwd`, `cursor`, `find`,
  `preview_top` are fields, `current.filter` is a field of a field,
  `row_rects()` and `column_rect()` are methods, `remembered` a lookup,
  the preview cursor private. A clone reads all of them; one style (all
  methods returning signals) would be easier to find.
- **Tasks report twice**: a failed job stays in `tasks_panel` and the app
  notifies too, so the title shows twice ("Move src to src: src: can't go
  inside itself"). An option for the panel's line without the title, or
  `Tasks::on_fail(|job, cx| ..)` app-wide, would replace the `done`
  closure each call repeats.
- **`files::preview` does not use the core's `resource_for` /
  `stale_after`** that round 3 added for it: it keeps its own generation
  counter and a `spawn`ed sleep. The first-frame bug above would be one
  fix in one place if it did.
- **`required-features` for an example that a test includes by path**:
  `tests/files.rs` needs `#![cfg(feature = "files")]` by hand; without it a
  default `cargo test` fails to compile the test, not skips it.
### Components that would help

| Component | What it would replace or enable in files.rs | API sketch | Where | Value |
|---|---|---|---|---|
| History (back/forward) | `Tab::back`/`forward`, `history`, the watch that records them | `history(signal) -> History` with `back()`, `forward()`; a browser's, a pager's, a REPL's | core | high |
| Tab set | `tabs`/`active` signals, `new_tab`, `switch_tab`, `close_tab`, `swap_tab`, `tab_strip`'s titles (Bubble Tea apps and Textual's `TabbedContent` keep a model like it) | `tab_set(\|\| state) -> TabSet<T>`; `tabs_for(&set, \|t\| title)` | core | high |
| Multi-select with a visual range | `selected`, `visual`, `end_visual`, `targets`, `marker` (ratatui's `tui-widget-list`, Textual's `SelectionList`, vim's visual mode) | `multi_select::<K>()` with `toggle`, `range_from(cursor)`, `targets(hovered)` | core | high |
| Files on top of those | the rest of the selection code, yank, paste, tabs per directory | `files::selection(miller)`, `files::Yank`, `files::tabs(miller)` | plugin | high |
| List search | `find_arrow` and the find prompt's handlers (Bubbles' `list` filter, Textual's `OptionList` search) | `list_search(query, items, cursor).next()` | core | medium |
| Cursor kept by key | `keep` around every sort, filter and hidden toggle | `list(..).cursor_by(\|item\| key)` | core | medium |
| Context menu on list items | the columns' `on_mouse` with `cx.pointer()` and `row_rects` (Textual's `on_click` per option) | `.on_context(\|i, cx\| items)` | core | medium |
| Help screen options | the hand-built `help` modal (~40 lines) when the core help can take a title, the key notation (`<C-u>`) and Yazi's filter-first layout (Bubbles' `help`, Textual's `HelpPanel`) | `cx.help_with(HelpOptions::new().title("mgr.help").key_format(Vim))` | core | medium |
| Description list | the spotter's `[blue]key[/][yellow]value` lines (Textual's `DataTable` two-column use, ratatui's `Table` of facts) | `facts([("Name", value), ..]).key_style(..)` | core | low |
| Spotter | `spot`: name, path, MIME, size, mode, owner, times, link (Yazi's `spot`) | `files::spot(path)` on `facts` | plugin | low |
| Gauge segment | the status line's progress text | `seg_gauge(fraction, label)` | core | low |
| Frecency store | the session-only `visited` set behind Z (zoxide) | `files::frecency().visit(dir)`, `.ranked()` | plugin | medium |
| `file` classification previewer | the hex dump for binaries | `Previewer::file_type()` | plugin | low |
| Bulk rename | Yazi's `r` on a selection: the names in `$EDITOR`, renamed on save | `files::bulk_rename(cx, paths)` over a handoff | plugin | medium |
| Shell command prompt | Yazi's `;` and `:` (run a command on the selection, blocking or not) | `cx.prompt` + `open::Launch::Shell` as `files::shell(cx, paths)` | plugin | medium |
| Content search | Yazi's `s`/`S` (fd and ripgrep into a virtual listing) | core: a streaming list of results (`fuzzy_list`'s source); plugin: `files::search(root, query, Grep)` showing a listing of matches in Miller | core + plugin | medium |

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

## Round 4: files (Yazi)

`examples/files.rs` rebuilt on round 4's additions: the runtime's
settled layouts, the core's `tab_set`, `history`, `multi_select`,
`list_search`, `cursor_by`, `on_context`, `seg_gauge`, `facts`,
`fuzzy_list`, `tabs(..).fit()` and `help_with`, and the `files` plugin's
`tabs`, `selection`, `Yank`, `find_next`, `hovered_caps`,
`BinaryPreview::FileType`, `open_with_tasks`, `Tasks::on_fail` and
`summary`, `frecency`, `finder_with`, `spot`, `bulk_rename`, `shell` and
`search`. No framework change in the round. It went from 1,449 lines to
1,198 while gaining Yazi's search (`s`, `S`, Ctrl+S), shell (`;`, `:`),
bulk rename (`r` on a selection), a frecency `Z` kept between sessions,
per-tab history and a progress gauge: what is left is Yazi's keymap as a
table, what each key does, the create and rename prompts, the opener
choice, the confirms, the header and the status line's segments. Its
tests went from 766 lines to 992 (36 tests to 47). The example still
needs `files` only (`required-features = ["files"]`); the tape's keys did
not change, so it was not re-recorded.

### Round-3 gaps now closed, and where files.rs uses them

| Round-3 gap or component | Now | Where in files.rs |
|---|---|---|
| **Bug:** a layout's work not settled before the next event (no preview in the first frame) | fixed in the runtime; `files::preview` is a `resource_for` keyed by its width | the tests' `first_frame()` wait is gone; `clicks_hover_and_the_parent_column_goes_there` clicks the preview at once |
| **Bug:** list options on a wrapper (`file_list`'s `switch`) dropped | fixed: options reach the list a wrapper builds | `columns` (`hovered_caps` puts the highlight under the row) |
| The hovered row's rounded ends | `MillerOptions::hovered_caps` | `columns`, with icons on; `with_icons_the_hovered_row_has_rounded_ends` |
| A binary file as a hex dump | `MillerOptions::binary(BinaryPreview::FileType)` | `columns`; `moving_previews_text_highlighted_and_binary_as_file_classifies_it` (`data`) |
| Tabs and their history (~140 lines: `Tab`, `history`, `new_tab`, `switch_tab`, `close_tab`, `swap_tab`, the watch on `m.cwd`) | `files::tabs(m)` on the core's `tab_set` and `history` | `Fm::new`, `run` (`Back`, `Forward`, `Tab*`, `Close`); `tabs_keep_their_own_history_and_fit_their_titles` |
| Selection and visual mode (~100 lines: `selected`, `visual`, `end_visual`, `targets`, `marker`) | `files::selection(m)` (the core's `multi_select`) and `Miller::targets()` | `Fm::targets`, `run` (`Toggle`, `SelectAll`, `Invert`, `Visual`, `Escape`), `status_line`, `header`; `select_all_invert_and_unset_mode` |
| Yank state and paste | `files::Yank`: `copy`, `cut`, `markers`, `paste(into, conflict)` | `run` (`Yank`), `paste`, `columns`; `a_selection_is_what_y_acts_on_and_is_marked_while_yanked` |
| `find_arrow` (~25 lines) | `Miller::find_next(previous)` on `list_search` | `run` (`Find`, `FindArrow`); `find_previous_goes_backwards_from_the_cursor` |
| `keep` round every sort, filter and hidden toggle | the cursor kept by path (`cursor_by`) | gone; `the_cursor_stays_on_its_entry_through_a_sort_and_hidden_files` |
| A right-click menu from `cx.pointer()` and `row_rects` maths | `MillerOptions::on_context` | `columns`, `menu` |
| `launch` matching `Launch::Extract` to call `Tasks::extract` | `open_with_tasks` | `launch` (one line) |
| The status line's progress as text | `seg_gauge` over `Tasks::summary()` | `status_line` |
| Z over a session-only `visited` set, through rs-rich-interact's `Jump` | `files::frecency().follow(m)`, zoxide's ranks in the state directory | `Fm::new`, `zoxide`; `z_and_big_z_look_the_same_and_big_z_jumps_to_a_directory_visited` |
| z and Z looking different | `fuzzy_list_with` and `files::finder_with`, one `FuzzyOptions` | `picker_look`, `jump`, `zoxide` |
| `finder` with no Esc, title or border | `finder_with(.., FuzzyOptions)`; Esc closes its modal | `jump` (no `on_key("esc")` of its own) |
| Tab titles cut by hand, the unselected style as markup | `tabs(..).fit()`, the theme's `tabs.unselected` | `tab_strip`, `theme` |
| The trash on macOS and Windows | `trash::send` (the Finder, the Recycle Bin) | `remove` (tested on Linux only) |
| Tasks reporting a failure twice | `Tasks::on_fail` once; `tasks_panel_with(.., failed_title(false))` | `Fm::new`, `task_manager`; no `done` closure per job (`Fm::job`) |
| The hand-built help modal (~45 lines) | `cx.help_with(HelpOptions::new().title("mgr.help").key_format(KeyFormat::Vim).filter_first(true))` | `run` (`Help`); the keymap table lost its Vim-notation column |
| The spotter's markup lines | `files::spot(path)` on `facts`, `.key_style("blue")` | `spotter`; `tab_spots_the_hovered_file` |
| Bulk rename, the shell prompt, content search | `files::bulk_rename`, `files::shell` with `ShellOptions`, `files::search` with `Miller::show_search` | `rename`, `run` (`Shell`, `Search`, `SearchCancel`, `Escape`); `r_on_a_selection_renames_it_in_the_editor`, `semicolon_runs_a_shell_command_on_the_selection`, `s_searches_below_here_and_esc_ends_it` |
| `readable_path` and `home()` | `files::path_parts`, `files::home_dir` | `readable_path` (one line) |

Newly faithful, and tested: per-tab history; `file`'s classification of
binaries; the cursor kept through a sort or a filter; find as you type
staying on a hovered match; Ctrl+A, Ctrl+R and `V`; a selection yanked;
`s`/`S` search listings that Esc ends; `;` and `:`; bulk rename;
frecency-ranked `Z` across sessions; Yazi's help in Vim key notation;
fitted tab titles; a progress gauge shown while tasks run, red once one
fails.

Not used, on purpose: `bind_when` and `when` (Yazi's manager keymap is
the same in every state: visual mode, a search and one tab all keep it;
a binding hidden while it does nothing would leave Yazi's help); the
`MillerOptions::current`/`parent`/`preview` hooks (nothing needed past
`MillerOptions`); `Miller::visited()` (`frecency().follow(m)` already
sees every directory, and lasts the session where there is no state
directory); `Miller::navigation` and the core's `navigation_with` (see
the gaps: unreachable from `files::Miller`).

### New gaps and bugs

| Gap or bug | Needed for | API sketch | Where |
|---|---|---|---|
| **Bug:** `.autofocus()` on a `switch` focuses the switch, not the node it builds. `file_list` is a `switch` (for its "No items"), so Miller's current column never has the keys: its own Down/PgDn do nothing, and the menu key and Shift+F10 never reach `MillerOptions::on_context` (only the right button does). Repro: `App::new(|| miller(dir).view_with(MillerOptions::new().on_context(..)))`, press `menu` or `down`: no call, the cursor stays; `driver.focused_name()` is `Some("current")`. The clone binds every key on its root, so only the menu key is lost; its keyboard branch in `menu` is written but unreachable, and untested | the menu key on a file; any focused wrapped list | a wrapper hands focus (and so key events) to the focusable node it built, as round 4 hands it list options; or `autofocus` is a list option `file_list` forwards | core (generic: any `switch`, panel or padding round a focusable node) |
| `files::Miller` does not forward the core's `navigation_with`, `go` or `Move` (its `columns` are private): Yazi's wording for the moves ("Previous file", "Move cursor up half page") needs them bound by hand, so the clone's own `Act::Arrow`/`Page`/`Top`/… stay | j/k, h/l, Ctrl+U/D, PgUp/PgDn with Yazi's descriptions | `files::Miller::navigation_with(node, &[(Move, keys, desc)])` and `go(Move)`, or `Miller::columns() -> miller::Miller<_>` | plugin (the core has it) |
| Selection and visual mode are one for all tabs. Yazi keeps a mode and a selection per tab: `v` in tab 2 then `1` shows tab 1 in normal mode, and tab 2 still selecting. The clone cancels visual mode when the active tab changes (a `watch` in `Fm::new`) | per-tab selection and mode | core: `tab_set`'s state holding any value per tab already works; what is missing is a `MultiSelect` that can be swapped under the columns (`multi_select().scoped(key)`: one selection per key, here the tab); plugin: `FileTab::selection`, with `files::selection` following the active tab | core + plugin |
| A status segment cannot hide: `seg_gauge` always draws its cells, but Yazi's progress shows only while tasks run, so `status` is a `switch` between two status bars | Yazi's progress, any optional segment | `StatusSegment::when(\|\| bool)`, a segment (and its caps) left out while false | core |
| `tabs_for` and `FileTabs::strip()` give a title with no index: Yazi numbers its tabs (` 1 src `), so `tab_strip` builds the titles from `set.tabs()` itself | Yazi's strip | `tabs_for_indexed(&set, \|i, t\| title)`, or `tabs(..).numbered()` | core |
| `find_next` starts after the cursor. Yazi's find as you type stays on the hovered entry when it matches; the clone tests the hovered name with `smart_contains` before calling it | `/` and `?` | `ListSearch::next_from(here: bool)`; `Miller::find_next_from(previous, here)` | core + plugin |
| A search's listing is not a level: `h` in it goes up from the directory searched, where Yazi (whose search is a virtual folder) goes back to it; the same would serve bookmarks and "recent" listings | `h` and `l` in a search, a bookmarks listing | core: a level overlay on `miller_of` (`m.push_level(name, items)`, popped by `leave`, its items opening as their own keys); plugin: `show_search` as such a level | core + plugin |
| The core help filters as you type; Yazi's moves with j/k and filters after `/` | Yazi's help keys | `HelpOptions::filter_key("/")`: until it, letters move (j/k, gg, G) | core |
| `facts` styles its keys only: Yazi's spotter has yellow values | the spotter's look | `facts(..).value_style(..)` (theme `facts.value`) | core |
| Yazi's links: `-` and `_` symlink the yanked files (absolute, relative), Ctrl+- hardlinks them; there is no operation for it | three Yazi keys missing from the clone | `Op::Link { from, into, kind: Symbolic { relative } \| Hard }` in rs-rich-ext's `file_ops`, `Yank::link(into, kind) -> Op` | plugin (rs-rich-ext + `files`) |
| Two sessions' frecency stores clobber each other: each app writes its own copy whole, so the last to visit wins (the tests, which run side by side, show it) | Z across several Yazi windows | merge on write: read, add this session's visits, write (zoxide's approach); see the state store below | plugin, on a core state store |
| `Tasks::summary()` is read three times a frame by the gauge (share, label, style), each a pass over the jobs | the gauge | a `Memo` from `summary_memo()`, or `seg_gauge_of(memo, …)` | plugin |
| `Tasks::on_fail`'s job carries its errors in `JobState::Failed(errors)`, so every handler matches on it | the notification | `Job::errors() -> &[String]` | core (`jobs`) |

### Friction with the new APIs

- **Prompts repeat their look.** Yazi's inputs have plain titles; each
  `prompt_with` takes `input_look()`, and `ShellOptions::prompt` has to
  be given it again. An app-wide default (`App::prompt_defaults(PromptOptions)`)
  would let a clone set it once. Core.
- **The menu from the keyboard needs `row_rects`.** `on_context` says
  "then below the hovered row" but leaves it to the app: `menu` matches
  `cx.pointer()` and `row_rects().get(cursor)` to choose between
  `context_menu` and `open_menu`. A `context_menu_at(cx, fallback_rect)`,
  or an `on_context` that passes the item's rect, would do it.
- **`selection` and `Yank` mark in two ways.** The selection is drawn by
  the columns because `selection(m)` stores itself in the `Miller`
  (made before `view_with`, or it is not marked); the yank is a
  `MillerOptions::markers` closure. Either way works, but the order
  dependence is silent: `selection` after `view_with` marks nothing.
- **Visual mode before an action.** `Miller::targets()` counts a visual
  range but does not end it, so every action calls `end_visual()` first
  (`Fm::targets`), as Yazi ends visual mode on `y`, `x`, `d`.
- **The parent column's marks** are re-derived by path inside
  `files::Miller` because `selection_markers` reads the visual range by
  index of the current column; the files agent's "marks by key" (a
  `MultiSelect::mark(&key)` marker any list showing the same keys can
  use, no cursor) would make that one call.
- **`files::Miller`'s state is still half fields, half methods**
  (`m.cwd`, `m.find`, `m.current.filter` but `m.searching()`,
  `m.visited()`, `m.row_rects()`); the core `Miller` moved to methods in
  round 4, the plugin did not.
- **`tests/files.rs` still needs `#![cfg(feature = "files")]`** by hand
  for an example it includes by path.

### Components that would help

| Component | What it would replace or enable in files.rs | API sketch | Where | Value |
|---|---|---|---|---|
| Edit text in `$EDITOR` | `bulk_rename`'s temporary file, handoff and read-back; also "edit the command" in `:` and a commit-message box anywhere (Bubble Tea's `tea.ExecProcess`, Textual's `App.suspend`) | `cx.edit_text(initial, \|edited: Result<String>, cx\| ..)`; `files::bulk_rename` on it | core (generic) | high |
| App state files | frecency's private `state_dir` and atomic write; tab sessions, prompt history, a last directory (Yazi keeps its state in `~/.local/state/yazi`) | `state_dir(app_name)`, `write_state(name, bytes)` (temporary file renamed over), `read_state(name)`; merge-on-write with a closure | core (generic) | high |
| A generic frecency store | `files::frecency` keyed by paths; a command palette's recent commands, a launcher's apps | `frecency::<K: Serialize>(name)` with `visit`, `ranked`, `remove`, aged as zoxide; `files::frecency` on it | core + plugin | medium |
| A level overlay on Miller | `h` leaving a search to the parent; bookmarks and "recent" as listings (Yazi's search is a virtual `search://` folder) | `miller.push_level(title, items)` popped by `leave`; `files::Miller::show_search` and a `files::bookmarks` on it | core + plugin | medium |
| Jobs running a process | `files::shell`'s background command and extraction each spawn and watch a child by hand; Yazi's task manager shows a shell task's output (Enter) and kills it | `jobs.run_process(title, Command, done)`: cancel kills it, its output kept for an inspect view | core (generic) + plugin | medium |
| Kitty, Sixel and iTerm images | half blocks only (ratatui-image's protocols; Yazi's main draw) | the paint step placing `Surface::graphics`; `image(path)` choosing the protocol the terminal answers | core paint + plugin (rs-rich-art) | high |
| Input completion | Yazi's `cmp` pop-up under `g Space` (paths as you type); Textual's `Suggester`, Bubbles' `textinput.SetSuggestions` | core: `PromptOptions::complete(\|text\| Vec<String>)` with a pop-up list; plugin: `files::path_completer(cwd)` | core + plugin | medium |
| Prompt history | Up/Down in Yazi's inputs (find, filter, shell); Bubbles' and Textual's inputs keep one | `PromptOptions::history(key)`, kept in the state store | core | medium |
| Per-tab state, generically | the selection per tab (above); any app with tabs of documents | `tab_set` with `scoped` values (`tabs.scoped(\|\| multi_select())`), switched with the active tab | core | medium |
| Links | `-`, `_`, Ctrl+- | `Op::Link` and `Yank::link` | plugin | medium |
| Optional status segments | the `switch` between two status bars | `StatusSegment::when(..)` | core | low |
| A file picker | not this clone; any app that asks for a file (Bubbles' `filepicker`, Textual's `DirectoryTree`) | `files::pick_file(cx, start, filter, \|path, cx\| ..)` on `miller_of` or the core `tree` | plugin | medium |
| PDF and video previews | Yazi's previewers through `pdftoppm` and `ffmpegthumbnailer` | `Previewer::command(mime_glob, argv)` producing an image or text | plugin | low |

## Round 4: scope-tui

[`examples/scope.rs`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-intuituive/examples/scope.rs)
rebuilt on round 4's runtime, widgets and sound pieces, with no framework
change in the round and no hand-made stand-in left in the example. Its
hand-checked `in_mode` handlers, the `page` dispatcher, ten flag closures,
the clamped debounce steps, the quoted punctuation keys, the hand-paced
file thread, the per-channel `VecDeque` history and the equal-temperament
`tune` are gone; it is 1,274 lines where round 3's was 1,323, and more
faithful: each scope's keys apply, and are listed, only while it shows
(`p` is peaks in one and the phase difference in another), `--tune` picks
scope-tui's buffer sizes sample for sample, and the frame rate is the
buffers' rate, as scope-tui's (`-b 2048` 23 fps, `-b 1024` 47, `-b 4096`
12, on the app's clock and in a terminal). `tests/scope.rs` (853 lines,
was 741) runs on the app's clock with no hand stepping and checks the new
behaviour (29 tests; 30 with `audio`).

### Round-3 gaps now closed, and where scope.rs uses them

| Round-3 gap or workaround | Now | Where in scope.rs |
|---|---|---|
| **Bug: values from other threads picked up at most every 50 ms** (20 fps whatever the buffer; tests stepped the driver by hand every 10 ms) | the run loop is woken by a `Proxy` send, and the headless `Driver::timeout` steps to the next `latest` thread asleep on the app's clock. The tests' `step` is gone: `driver.advance(2.1 s)` reads 23, 47 and 12 fps at `-b 2048`, `1024` and `4096`. In a pty, a debug build of the example shows 24, 47, 12 and 89 (`-b 512`), where scope-tui's arithmetic gives 23.4, 46.9, 11.7 and 93.8 | `the_frame_rate_is_a_buffers_worth_of_time_as_scope_tuis`, every timed test |
| **Bug: grid lines shared one layer** (`⣸` where scope-tui draws `⢸`) | a layer a grid line, the y axis's first | `chart`; pinned by `reference_lines_are_drawn_as_scope_tuis_datasets_are` |
| **Bug: a grid with no datasets was drawn in dots** | `Chart::marker(marker)`: the zero line is Braille (or dots with `--no-braille`) before the first buffer as after | `chart` |
| Bindings active only in a state (`t`, `e`, `=`, `w`, `l` bound and listed in every scope; one `p` with one description) | `.modes(mode, &[(Mode::Oscilloscope, "oscilloscope", &oscilloscope_keys), (Mode::Spectroscope, "spectroscope", &spectroscope_keys)])`: two `p`s, two `pageup`s, two `esc`s; the general `esc` is `bind_when(mode != Oscilloscope)`, so each scope's help lists one | `scope_app` |
| Sections for every kind of binding | `in_mode`'s section (under `modes`) lists the modes' `bind_scaled` and `bind_step` bindings under the scope's name | `oscilloscope_keys`, `spectroscope_keys` |
| Named punctuation keys | `equals`, `minus`, `plus`, `underscore`; the help shows `=`, `-`, `+`, `_` | `oscilloscope_keys` |
| Testing a capture's status | `Capture::simulated(frames, period)` with `stall()` and `fail(why)`: `--timeout` and a failure are tested; the test signal is itself a simulated capture, so it and a sound card are one `Input::Capture` | `demo`, `a_capture_that_stalls_ends_the_app_as_scope_tuis_timeout`, `a_capture_that_fails_ends_the_app_saying_why` |
| A thread feed with a state | `latest_with_state` under `Capture::status()`, a `FeedState`; the file source has none (see the gaps: `audio::play` gives no state, and scope-tui needs none) | `scope_app` |
| `audio::pcm` behind `pcm` | `required-features = ["pcm"]`, unchanged; `Capture` is now `pcm`'s, so `Input` has no `cfg(feature = "audio")` arm | `Input` |
| One `on_write` arm per kind of source (three) | two: a capture's `Frame` and a file's `Matrix` | `scope_app` |
| `on_write` cloned each buffer | `on_write_ref` lends it; one copy is made, into the buffer shown (but see `recent` below) | `take` |
| `dsp::note_hz` was not scope-tui's table (E2 a sample off) | `dsp::NoteTable::ScopeTui.hz(note)`, octave 0 by default: E2 at 48 kHz is 583 samples, as scope-tui's; the hand-added octave is gone | `tune`, `tune_reads_notes_as_scope_tui_does_on_rich_exts_pitches` |
| `Axis::grid` styled with `Axis::style` | `Axis::grid_style(axis colour)` for the reference lines; `Axis::style` stays, as scope-tui styles its axes | `chart` |
| `Chart::style("none")` for the legend | `Chart::legend_unstyled()` | `chart` |
| `bind_scaled`'s help said `alt ×0.2` | labelled variants, `("alt", 0.2, "⅕")`: `scale up (shift ×10, ctrl ×5, alt ⅕)`, as scope-tui's docs | `MAGNITUDES` |
| The help cut long rows ("threshold or avg up") | it wraps; with the keys per scope the descriptions say one thing each ("threshold up", "average more") | `oscilloscope_keys`, `spectroscope_keys` |
| Ten `signal.update(\|g\| g.x = !g.x)` closures, the clamped steps of the debounce and the averaging | `bind_toggle(keys, desc, signal, \|s\| &mut s.flag)` (ten flags), `bind_step(keys, desc, signal, field, step, 1..=65535)` (the debounce's four keys, the averaging's) | `scope_app` |
| A history of writes (`take`'s per-channel `VecDeque`s) | `recent(frame, averaged)`, `averaged` a memo of the spectroscope's `average`; `spectroscope` takes the buffers, oldest first | `scope_app`, `spectroscope` |
| An iterator played at a pace (`play`'s thread) | `audio::play(pcm, rate)` with `-l` (`latest_iter` at `Pace::Every`, then silence); a pipe is `latest_iter(pcm, Pace::Free)` | `play` |

Not used, on purpose, as in round 3: `live`, `audio::record`/`wav`,
`audio::picker`, the `viz` plugin's `meter`, `sparkline`, `hold`,
`spectrogram` and `bar_spectrum`, `chart::Persistence`, `digits`,
`dsp::peak_hz`/`nearest_note`, `smoothed`, `status_bar` and `key_hints`:
scope-tui has none of them. Not reproduced, also on purpose: scope-tui
0.3.5's `file` source reads `buffer × channels` bytes a buffer, half a
buffer of s16 frames a channel (its trace fills half the width), and
paces `-l` by whole milliseconds of that; and its spectroscope keeps its
averaging history only while it shows. The clone reads a whole buffer a
channel at its time, and averages the last buffers shown in any scope.

### New gaps and bugs

| Gap or bug | Needed for | Repro or API sketch | Where |
|---|---|---|---|
| The help cuts a long key list instead of wrapping it | a binding with several keys: `bind_step("pageup shift+pageup ctrl+pageup alt+pageup", ..)` is listed `pgup/shift+pgup/ctrl+pgu` | the key column wraps as the description does, or is as wide as its widest entry up to a share of the box | core (generic) |
| A binding whose modifiers do not matter | scope-tui's spectroscope steps the averaging by one on PgUp with any modifier; the clone binds `pageup` and, with an empty description (hidden), `shift+pageup ctrl+pageup alt+pageup` | `.bind_step(..).any_modifiers()`, or `shown` keys for every kind of binding, as `bind_scaled` has | core (generic) |
| A step scaled by the modifier | ← →, ↑ ↓ and PgUp/PgDn step a clamped field by a base step times shift ×10, ctrl ×5, alt ⅕; `bind_scaled` gives the factor but not the field, so `samples` and `threshold` are hand-written clamps (and ← → truncate the step to whole samples) | `bind_step_scaled(keys, &MAGNITUDES, desc, signal, \|s\| &mut s.n, base_step, range)`; an integer field rounds (or truncates, as scope-tui) the step | core (generic) |
| A played feed with a state | an app that shows "end of file" (scope-tui does not: it pads with silence for ever, so the clone needs none) | `latest_iter(..).with_state() -> (Signal<Option<T>>, Signal<FeedState>)`, `Ended` when the values run out (before `then`'s), and `audio::play` returning it | core (generic) + plugin (`audio`) |
| A tail at its own pace | a pipe read as fast as it comes, then silence a buffer's time apart: `then` keeps `Pace::Free`, a flood, so the clone writes `then_with(\|tx\| while tx.send(silence.clone()) && tx.sleep(period) {})` | `.then_at(Pace::Every(period), iter)` | core (generic) |
| `Pcm` drops read errors | scope-tui ends on a read error with "data source returned null"; `audio::pcm` treats an error as the end, so the clone pads with silence instead | `Pcm::error() -> Option<&io::Error>`, or a `Result` item; `audio::play` with a state then `Failed(why)` | plugin (`audio`) |
| A capture that never stalls | scope-tui's `pulse` source blocks and has no timeout; the clone's ends after the default second without a buffer | `AudioOptions::stall_after(impl Into<Option<Duration>>)` and `Capture::stall_after`, as `latest_with_state` takes | plugin (`audio`) |
| `recent` clones every write | the spectroscope's history copies each buffer a second time (a 2×2048 matrix a frame), on top of the copy into the buffer shown | `recent` on `on_write_ref`, cloning once, or a `recent_of(source, n, \|&T\| -> U)` that keeps a projection | core (generic) |

### Friction with the new APIs

- **A field picker kept in a variable does not compile.** `let average =
  |s: &mut Spectroscope| &mut s.average;` passed to two `bind_step`s fails
  with "lifetime may not live long enough": a closure's return does not
  take its argument's lifetime. The clone writes `fn depth` and `fn
  average`. A `field!(Spectroscope, average)` macro, or the docs saying
  "use a `fn`", would save the detour.
- **A name for a memo and a field picker clash quietly.** `average` the
  memo and `average` the `fn` gave a type error naming `Memo<usize>`; not
  the API's fault, but `recent(signal, n)` taking a closure (`|| spec.with(..)`)
  as a `Count` would make the memo unnecessary.
- **Empty descriptions hide bindings.** `""` is still the way to keep a
  binding out of the help, as `"none"` was for the legend's style;
  Textual's `Binding(show=False)` names it: `.hidden()`.
- **`modes` wants `&dyn Fn(Node) -> Node`.** The pages are closures kept
  in variables (`oscilloscope_keys`), which reads well; the type
  (`ModePage`) only shows when inference fails.
- **One key, two scopes, one handler each.** scope-tui hands a key to the
  app and then to the scope showing (Esc resets the view and stops the
  trigger); a binding's key runs one handler, so the oscilloscope's Esc
  calls the shared `reset` itself and the general Esc is
  `bind_when(mode != Oscilloscope)`. Fine, but a "fall through" binding
  (`.and_then_parent()`, Bubble Tea's message passed on) would say it
  directly.
- **Headings in the help have no hook for tests.** A test finds a section
  by `│name ` at the box's edge, since a description may hold the same
  word ("oscilloscope, vectorscope, spectroscope").
- **`Capture::simulated` for a demo signal.** It fits (a source with a
  status, the same arm as a sound card), but its name says tests; a
  `Capture::generated(iter, period)` alias, or the one source type below,
  would read better.

### Components that would help

What scope-tui leans on, what the sound and chart work left open, and
what comparable audio and visualisation TUIs and libraries offer (cava,
ratatui and its crates, Bubble Tea and Bubbles, Textual). Core entries
are generic: another, unrelated component could use them.

| Component | What it would replace or enable | API sketch | Where | Value |
|---|---|---|---|---|
| One audio source type | the two `on_write_ref` arms and the `Input` enum: a capture, a file, a pipe and a generator as one value with samples and a status | core: a `Feed<T>` value (`samples()`, `status()`) that `latest_with_state` and `latest_iter(..).with_state()` return; plugin: `audio::Capture::from_pcm(pcm, rate)`, `Capture::generated(iter, period)`, `Capture::from_feed(feed)`, all a `Capture` | core (generic) + plugin (`audio`) | high |
| A wait the test clock counts as asleep | a feed thread blocked on a condition (a device's condvar, a socket) is busy to the headless clock, so a sound card's feed polls every 100 ms and only `simulated`'s `sleep(period)` is stepped exactly; `wait_for` would let any feed be tested on the app's clock | `Latest::wait_for(\|\| cond, timeout) -> bool`, notified by `Latest::notify()`, on the app's clock under `wait_for_tasks` | core (generic) | medium |
| Scaled steps | `samples`, `threshold` and `nudge`: a clamped field stepped by the modifier's factor (ratatui apps' and scope-tui's shift ×10) | `bind_step_scaled(keys, variants, desc, signal, field, step, range)` | core (generic) | medium |
| Falling values | a peak or a level that falls with gravity after it rises (cava's bars, a meter's peak, a notification badge's decay); `dsp::Falloff` is a plain struct in rich-ext | `falling(signal, gravity) -> Memo<f64>`, on the app's clock, beside `smoothed` (Bubbles' harmonica springs) | core (generic) | medium |
| Ages in `recent` | persistence that fades by time, not by count (a phosphor's decay, a toast list, a log's recent lines) | `recent_with_ages(signal, n) -> Memo<VecDeque<(Duration, T)>>`, or `recent(..).aged()` | core (generic) | low |
| A persistence or oscilloscope view | the oscilloscope's plot builders (trigger, peaks, zero line) for any scope app, and an analogue scope's phosphor (xoscope, glscopeclient) | plugin: `viz::oscilloscope(\|\| frame).trigger(..).persistence(n)` over `chart::Persistence`; generic core under it: `recent` (with ages) | plugin (`viz`) | medium |
| A tuner readout | the note nearest the loudest peak, with cents (guitar tuners, `tuner` TUIs) | plugin: `viz::tuner(\|\| spectrum)` on `dsp::peak_hz` and `nearest_note`, drawn with the core's `digits` | plugin (`viz`) | low |
| A help that wraps keys | the hidden modifier bindings above, and any binding with several keys | the help's key column wrapping (Bubbles' `help` full view, Textual's key panel) | core (generic) | low |
| Bindings that fall through | scope-tui's Esc, the app's and then the scope's | `.bind_through(keys, desc, f)`: runs, then passes the key on (Bubble Tea's `Update` returning the message to a child) | core (generic) | low |
| Hidden bindings by name | `""` descriptions | `.bind_hidden(keys, f)`, or `.hidden()` after a binding (Textual's `show=False`) | core (generic) | low |
| Already there, not in scope-tui | `viz::spectrogram`, `viz::bar_spectrum` (cava's bars, `dsp::bands`, `Falloff::gravity`), `meter`/`hold`, `sparkline`, `digits`, `smoothed`, `audio::record` | an extended scope, not this clone | plugin | — |

### Plugins

The user's decision for this round: visualisation and recording are
plugins, not core. The `viz` feature is the home of the views of sound and
signal data: `meter`, `sparkline`, the peak hold (`PeakHold`, `hold`), the
spectrogram and the bar spectrum, and later persistence and an
oscilloscope view. The `wav` feature is the home of recording
(`audio::record`, `wav_writer`). The core keeps only what any app could
use under them: `recent`, `smoothed`, `latest_with_state`, `latest_iter`,
`digits`, `rate`.

## Round 5: nine new apps

Round 5 cloned apps unlike a file manager and an audio scope, each built on
intuiTUIve as round 4 left it (no framework change during the round), each
with its own report in [`docs/design/gaps/`](gaps/):

| App | Clone | Lines | Tests | Report |
|---|---|---|---|---|
| gitui (git client) | `examples/git.rs` | 2,219 | 21 | [git](gaps/git.md) |
| bottom (system monitor) | `examples/top.rs` | 2,706 | 25 | [top](gaps/top.md) |
| slumber (HTTP client) | `examples/http.rs` | 2,088 | 20 | [http](gaps/http.md) |
| rata-tetris (game) | `examples/blocks.rs` | 919 | 15 | [blocks](gaps/blocks.md) |
| csvlens (CSV viewer) | `examples/csv.rs` | 1,534 | 14 | [csv](gaps/csv.md) |
| diskonaut (disk treemap) | `examples/disk.rs` | 1,395 | 21 | [disk](gaps/disk.md) |
| termusic (music player) | `examples/music.rs` | 1,556 | 17 | [music](gaps/music.md) |
| taskwarrior-tui (tasks) | `examples/tasks.rs` | 2,582 | 27 | [tasks](gaps/tasks.md) |
| a raycaster (Doom-style) | `examples/raycast/` | 2,924 | 26 | [raycast](gaps/raycast.md) |

None draws a widget of its own except the game's playfield and the
raycaster's 3D view. The round-5 core and plugin work (below the round-4
sections' backlog) landed alongside; the clones do not use it yet.

### Gaps shared across apps

What several clones hit independently leads round 6. Each is generic core
unless marked.

| Gap | Hit by | Shape |
|---|---|---|
| Panel titles and border styles that follow state | git, top, http, music | `panel_with(\|\| markup)`, a focused border kind |
| Show or hide a node by a signal; sizes that follow state | git, top, http | `.shown_when(\|\| ..)` (today's private `shown_if`), `.size_with(\|\| ..)` |
| Focus a page a `switch` has not built yet; read where the focus is | git, top, http, tasks | `cx.focus_within(id)`, `focus_scope`, `on_focus` / `focus_within` signal |
| A node rebuilt from state that drops its old child (`switch` keeps every page) | disk, csv, tasks | `keyed(key, build)` |
| Tables: reactive options and columns, marker column, keyed cursor, offset, widths in percent, row heights | csv, tasks, music, top | signal-taking `TableOptions`, `marks_by`/`selection_markers` on tables, `cursor_by`, `offset_x`, `Column::max_percent`, `row_height_for` |
| A tree keyed by item, with reveal and a prefix | git, music | `tree_keyed`, `reveal(key)`, `TreeItem::prefix` |
| Range selection on a list | git | `list(..).range(anchor)` |
| Directional (spatial) focus and selection | top, disk | move to the neighbouring rectangle by direction |
| Input completion pop-up, ghost text and history; a command line | tasks, http | `input(..).complete(..)`, `history(..)`, `command_line(..)` |
| A query language over records | tasks, top, csv | `query::parse` over a `Record` trait; syntaxes as plugins |
| Timers with a handle (pause, rate, cancel), catch-up; key repeat control; key releases for bindings | blocks, raycast | `every(..) -> Timer`, fixed step with catch-up, DAS/ARR, `bind_release` |
| `key_hints` formats; whole-app help | git, music | `key_hints_with(..)`, `HelpOptions::all_panes()` |
| Treemap layout | disk | `layout::squarify(rect, weights, ..)` |
| Natural dates and local time | tasks | date parsing ("tomorrow", "eow"), a local offset that follows DST |
| `App::min_size` guard | disk, blocks | a too-small screen by default |
| A process as a job, with progress and cancel | git, http | round 5's `Jobs::run_process` (landed) |
| App state files | blocks, tasks | round 5's `state` module (landed) |
| Plugins: git model, HTTP client, audio player and decoders, tags, disk usage, system metrics, data sources, Taskwarrior | one each | plugin |

**Bugs found:** keys typed while the terminal ignores the start-up query are
lost for up to 3 s (blocks); tables and lists treat Shift+↑/↓ as ↑/↓ (top);
an `.auto()` switch ignores its page's `.fixed()`/`.min_size()` (top);
`Driver::event` does not settle as `press` does (top); a full-size layer
paints over a `fill` beneath it (blocks); upstream's `Text.markup` is not
ported (csv); the app loop handles one input event per frame, so queued
keys spread out over slow frames: drain pending input before drawing
(raycast); rs-rich-ext's `Screen` style table and the painter's code cache
only grow, so a full-screen truecolour framebuffer leaks styles every frame,
and interning hashes with SipHash (33 of 41 ms a frame at 160x50): compact
styles between frames, a fast hash and a `put_rgb` path (sprites, raycast).

Sprites landed during the round (`Sprite`, `stage`, `pixels`/`PixelBuf`,
sprite sheets behind `art`, micro assets behind `micro`); blocks and raycast
use them from round 6.

### Real Doom (rs-rich-doom)

[rs-rich-doom](https://github.com/buchochelliq-labs/rs-rich-doom) (GPL-2.0,
a separate repo: the engine is GPL) runs doomgeneric on an intuiTUIve
`latest` thread and draws its 320x200 frames in half blocks, Braille, Kitty,
Sixel or iTerm2 at 33–35 fps (release, 80x24 and 160x50), with sound effects
through cpal. What it needed from rs-rich, beyond the raycaster's list:

| Gap | Shape | Where |
|---|---|---|
| `Painter::paint` clones the whole style table every frame and it never shrinks: averaged pixel colours made a frame 50 ms at 80x24 (nearest-neighbour sampling, keeping Doom's 256 colours, brought it to 4 ms) | drop unused styles between frames, or pixel cells that skip interning | rs-rich-ext surface (core) |
| `DrawCx` has no public screen rect, so a widget emitting graphics guesses where it is | `DrawCx::rect()` (as `EventCx` has) | core |
| Graphics output for a pixel buffer is hand-rolled (`PlacementSource`, shared state) | the `pixels` node drawing Kitty/Sixel/iTerm2 itself | `art` plugin |
| Key state: modifiers alone are dropped; whether releases are reported is unknown to the app | kitty flag 8 opt-in, `held_keys`, `releases_reported()` | rs-rich-interact (core) |
| Frame pacing and stats: no target-fps mode that skips frames; `App::stats` unreachable after `run()` | `every(..).target_fps(n)`, frames drawn in `FrameStats`, stats returned by `run` | core |
| Encoders: indexed-palette Sixel is crate-private; Kitty has PNG only and no direct placement id | public palette Sixel, Kitty raw RGB (`f=24/32`, zlib), placement ids | rs-rich-art |
| A git dependency on rs-rich-cli prints an "invalid character" error from `templates/intuituive-app/Cargo.toml` (`{{project-name}}`) | exclude the template from cargo's package discovery (rename the manifest, or a workspace `exclude`) | repo |
| Music | MUS→MIDI and an OPL/MIDI synth | plugin |

## Round 7: the nine apps rebuilt on round 6

Each clone was rebuilt on round 6's components, without library edits. Each
report in [`gaps/`](gaps/) has a "Round 2" section with the details.

| Clone | Example lines | Tests | What replaced hand-built code |
|---|---|---|---|
| git (gitui) | 2,219 → 2,204 | 21 → 23 | `tree_keyed` with status prefixes, `list_rows(..).range`, `shown_when`/`size_with` layout, `on_focus`, `key_hints_with`, `keyed` |
| tasks (taskwarrior-tui) | 2,582 → 2,412 | 27 | `query::Grammar` filters (adds `or`, `not`, brackets), `dates::parse_with`, `Zone`, `command_line_with(two_rows)` with completion and history, one reactive report table, `year_view_with` |
| http (Slumber) | 2,088 → 2,026 | 20 → 22 | one tree with `shown_when`/`size_with`, `focus_within`, `focus_border`, panel titles from state, `tree_keyed`/`tree_reveal`, `Timer` pause/resume, input history |
| disk (diskonaut) | 1,395 → 1,192 | 21 | `layout::squarify`, `focus::neighbour`, `fit_text`/`truncate_middle`, `keyed`, `App::min_size` |
| top (bottom) | 2,706 → 2,415 | 25 | focus scopes with directional focus, `shown_when` instead of a second copy of each widget, `columns_with` with `priority`, `query::Grammar` search, `title_with` |
| csv (csvlens) | 1,534 → 1,375 | 14 | `TableSource` with `view_of` (keep, find, sort), a reactive table with frozen columns and `multiline_when`, `command_line` with shared history, `Text::markup()` |
| blocks (rata-tetris) | 919 → 959 | 15 → 18 | `fixed_step` with catch-up, DAS/ARR `.repeat`, `App::min_size`, one backdrop `fill` layer, `state_file`, sprites on a stage |
| raycast | 2,924 → 2,891 | 26 → 28 | `fixed_step`, `on_key_up` and the release report, `all_keys`, `pixels_fit`, `DrawCx::rect()`, `keyed` with `on_unmount`; the Kitty-mode and held-key bugs are fixed |
| music (termusic) | 1,556 → 1,512 | 17 → 20 | a `transport` play head, `panel_with`, `tree_keyed`/`tree_reveal`, `symbol_for`, `keyed`/`shown_when`, `HelpOptions::all_panes` |

blocks grew by 40 lines. Its model is unchanged; the extra lines are the
repeat rates and the sprite builder.

### Bugs found

| Bug | Found by | Workaround in the clone |
|---|---|---|
| Closing a modal gives the focus back to the node focused before a `cx.focus(x)` made just before it opened | tasks | focuses the report after the modal closes |
| A styled `layers` node does not cover the gaps its inset children leave | blocks | a `fill` as the first layer |
| `layout::squarify` gives the largest item no tile when the rest's tile is under the minimum width | disk | 12 lines |
| A `previews_keys` container takes the letters typed into a `command_line` inside it | csv | the line sits outside the container |

Each has a minimal failing test in its report.

### Performance

`view_of` made csv's million-row filter slower: 3.3 s → 15.4 s in the
unoptimised test build. About 8 s goes on reading each row through
`TableSource::row(i)`, and about 7 s on building a `Text` per row for the
regex. Needed:

- a bulk read for passes;
- a compiled matcher on `&str` in rs-rich-ext.

### Gaps shared across apps

| Gap | Apps | Kind |
|---|---|---|
| Panel titles flush with the corner (ratatui's look), and plain-text titles | git, music, http | core: a title option |
| A tree table (a table whose rows nest) | top, tasks | core |
| A treemap that joins its tiles' borders and keeps a selection | disk | core |
| Held keys that fall back on a latch where releases are not reported | raycast | core: `held_keys_with` |
| `App::min_size` with a node of its own, and shorter forms of the message | blocks, disk | core |
| A timer that runs while a condition holds (`running_when`) | blocks | core |
| A stopped state on `Transport`; `follow_time` for a time-keyed line | music | core |
| Command-line reads (`is_typing`, `is_asking`), a closed-bar look, mode titles from state | tasks, csv | core |
| Keys that submit a command line (`<n>G`) | csv | core |
| Directional-focus binding descriptions, tie-breaking in `neighbour`, scope cycling past hidden panes | top, disk, http | core |
| A glyph checks column on tables, `column_highlight` from state | http, csv | core |
| A scroll offset kept across a resize | http | core |
| Real pixels on the `pixels` node; `PixelBuf::copy_rgb` | raycast | plugin (`art`) |
| Unit words in a grammar (`1 gib`), `TextMatch` on comparisons, `Zone` re-exported | top, tasks | core (query, dates) |
