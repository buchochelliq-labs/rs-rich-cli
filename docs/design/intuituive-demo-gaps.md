# intuiTUIve: what the demos are missing

Status: working list, updated after each round.

We rebuild well-known ratatui apps on intuiTUIve to learn what the
framework lacks. Each clone is built only from intuiTUIve's own
components, from its core or its plugins: no framework changes during the
round, and no custom widget in the example to stand in for a missing one.
What the clone needs and cannot get goes on this list. Gaps chosen from it
are added to the framework, the clones are rebuilt on them, and we repeat
until each clone uses intuiTUIve components only. Then we pick more apps.

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

## Round 2

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
