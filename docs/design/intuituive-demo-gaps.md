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
