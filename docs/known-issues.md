# Known issues and limitations

What does not work yet, what was never meant to, and what works differently on
purpose. Three different things, kept apart — a deliberate trade-off listed as a
bug makes a considered decision look like neglect.

**Applies to** the 0.0.20 release (`rs-rich-cli` 0.0.20, `rs-rich` 0.0.10),
reviewed against Python `rich` 15.0.0. Older entries name the release they
were found in. Each entry links to its issue or report so you can check the
status without waiting for this page to be updated.

---

## Bugs

Things that should work and do not.

### Narrow `--json` output is display output, not machine-readable JSON

**Scope.** Default builds match upstream: narrow output may wrap or crop inside
an escape, and `--width` crops overlong JSON lines. The optional, off-by-default
`json-escape-safe` Cargo feature avoids partial escapes when cropping and keeps
escapes together when folding at widths that can fit them. See
[DIVERGENCES §22](DIVERGENCES.md#22-escape-safe-json-presentation-json-escape-safe).
Neither mode promises machine-readable output; use the original JSON with `jq`
when every value must survive.

### The recorder keeps fewer frames on macOS

`rich record` keeps every screen update the program finished, up to the
tape's frame rate. On Linux a game recorded at `Set Framerate 35` keeps
about 30 frames a second; on CI's macOS runners the same recording kept 7
frames over 0.8 s, so its video plays in jumps. Whether macOS's PTY reads
or the recorder's settling rule merge the updates is not known yet; the
frame-rate test keeps its lower bound on Linux only until it is. Record
games on Linux for now. Planned for [0.0.21](plans/0.0.21.md#1-round-7s-bugs-and-regressions-intuituive-record).

### Four intuiTUIve bugs the demo clones found

Round 7 of the [demo loop](design/intuituive-demo-gaps.md#round-7-the-nine-apps-rebuilt-on-round-6)
found these; each has a minimal failing test in its clone's report and a
workaround in the clone, and all four are planned for 0.0.21:

- a `cx.focus(x)` made just before `cx.modal(..)` opens is not where the
  focus returns when the modal closes: it goes back to the node focused
  before ([tasks](design/gaps/tasks.md)); focus `x` again after the modal
  closes;
- a styled `layers` node does not cover the gaps its inset children leave
  ([blocks](design/gaps/blocks.md)); put a `fill` as the first layer;
- `layout::squarify` gives the largest item no tile when the rest's tile is
  under the minimum width ([disk](design/gaps/disk.md));
- a `previews_keys` container takes the letters typed into a
  `command_line` inside it ([csv](design/gaps/csv.md)); put the command
  line outside the container.

Filtering a million rows through `view_of` is also slower than the clone's
own loop was (15.4 s against 3.3 s in an unoptimised build): each row is
read through `TableSource::row` and turned into a `Text` for the regex.

---

## Limitations

Things that are not implemented, and are not claimed to be.

### Windows legacy console is not supported

`rich` assumes a VT-capable terminal. On Windows it enables virtual terminal
processing; on a console that cannot do VT (very old `cmd.exe`), output will
contain raw escape sequences.

Upstream falls back to the Win32 console API for these. The port will not:
every supported Windows release has a VT-capable console, so this is a
permanent divergence ([DIVERGENCES §38](DIVERGENCES.md#38-the-windows-legacy-console-is-not-supported),
[#12](https://github.com/buchochelliq-labs/rs-rich-cli/issues/12)).
**Workaround:** use Windows Terminal, or `--no-color`.

### A tape without `Output` also writes an APNG

Since 0.0.20 `rich record` writes every format a tape does not narrow, and
the formats now include an animated PNG beside the GIF. An APNG is in full
colour, so it can be several megabytes for a long recording, and encoding
it adds time. Name what you want with an `Output` line (`Output gif png`)
or `--format gif,png,svg`, or skip all video with `--no-video`. Whether the
APNG stays a default is an open question for
[0.0.21](plans/0.0.21.md#1-round-7s-bugs-and-regressions-intuituive-record).

### The local time zone on Windows follows `TZ` when it is set

`rs-rich-interact`'s `Zone::Local` (and so `DateTime::now_local`, natural
dates and intuiTUIve's calendars) asks Windows for the system's zone,
unless `TZ` is set: then `TZ` wins, as it does for the C runtime. Windows
has no zoneinfo database, so `TZ` must be a POSIX rule
(`CET-1CEST,M3.5.0,M10.5.0/3`) or `TZDIR` must name a zoneinfo directory;
a zone name alone, such as `Europe/Berlin`, is read as UTC. See
[Troubleshooting](troubleshooting.md#the-local-time-is-wrong-on-windows).

### Chrome's DevTools port in rs-rich-embed

The Chrome engine of `rs-rich-embed`'s web view talks to Chrome over
DevTools on a random port of `127.0.0.1`, which other programs and users
on the machine can reach while the view is open, and DevTools has no
authentication. Use the Chrome engine on a machine you do not share. A
pipe in place of the port is
[planned for 0.0.21](plans/0.0.21.md#8-devtools-over-a-pipe-on-unix-embed)
on Linux and macOS; Windows keeps the port until a maintained crate can
pass Chrome the pipe's two handles.

### No Jupyter integration, and no Python-object inspection

`jupyter.py` and `inspect`/`repr` have no equivalent: they render live Python
objects, which has no meaning in a Rust port. `pretty` and `traceback` are
reimplemented Rust-natively instead — see
[Divergences](DIVERGENCES.md).

### SVG export is not self-contained offline

`--export-svg` references its font from a CDN. The HTML from `--export-html`
*is* self-contained. If you need an offline SVG, embed the font yourself after
export.

### GIF interruption and export limitations

`--gif-mode` is `ascii` (the default, and the redirected/colorless fallback) or
`blocks`; see the [capability matrix](cli.md#gif-half-block-rendering-004).
Since 0.0.11, `--image-color` and `--image-dither` also apply to GIF frames.
Normal playback restores the cursor; Ctrl-C can leave it hidden. GIF export and
Sixel playback are unsupported.

### Redaction is experimental and best effort

`rich_ext::redact`, `rich capture --redact` and `--redact-pattern`, and
`rich inspect --redact` mask what their detectors recognise: secret-named keys,
bearer tokens, common token prefixes, AWS key ids, JWTs and URL passwords. They
can miss a secret in a format they do not know. Read the output, and any SVG,
HTML or cast file written from it, before sharing it, and
[report](https://github.com/buchochelliq-labs/rs-rich-cli/issues/new?template=bug_report.yml)
anything that gets through. `rich env` masking is best effort in the same way.

### Viewers read a bounded amount

`view`, `hex`, `unicode`, `inspect`, `capture` and `--theme-file` stop at a
fixed size (for example 8 MiB or 20,000 lines for source in `view`, 64 MiB per
`inspect` document, 1 MiB of `capture` output), so an endless input ends instead
of exhausting memory. Larger inputs are shown in part or refused; the caps are
not configurable. See [Limits](cli.md#limits).

### `capture` has no PNG export

`rich capture` shows the output in a panel, exports SVG or HTML, and records an
asciicast with `--cast`. There is no PNG export; convert the SVG yourself.

### Mermaid subgraphs are drawn without frames; DOT's limits

`rs-rich-diagram` 0.0.2 frames DOT `cluster…` subgraphs and honours
`rank=same` where a layered drawing can (a group an edge runs within is
dropped, with a note); other `rank` values are accepted with a note. Mermaid
flowcharts do not pass their subgraphs on as clusters yet, so those are
still drawn without frames, with a note. ER diagrams attach relationship
edges to an entity's box, not to the column's row. Node ports, HTML-like
labels, `record` shapes and several graphs in one file are refused with the
construct and its line (`rich dot` exits 4). For Graphviz's full layout, use
`--dot-backend graphviz --export-svg` with Graphviz installed. See
[Diagrams](guide/diagram/index.md#dot-graphviz).

### SVG export can squeeze CJK glyphs

SVG textLength uses character counts for some runs; wide CJK glyphs may overlap
in the exported image. The same case reproduces in pinned Python Rich 15.0.0.
Decoded text and plain terminal output retain the original characters. This is
an export-layout limitation, separate from encoding support.

### CSV output still retains source rows

Undecorated CSV output streams styled rows, reducing output-buffer overhead.
0.0.4 removes duplicate parsed-cell storage and trims row
capacity, reducing the measured 100k-row peak RSS by about 46%. Source rows remain
in memory for measurement, so memory still scales with input.
Decorated, aligned, paged and exported CSV output still buffers. The 0.0.4
work in [#74](https://github.com/buchochelliq-labs/rs-rich-cli/issues/74) reduces
retained memory; it does not provide bounded-memory processing.

### Long-line rendering still allocates memory

0.0.4 removes repeated UTF-8 prefix scans during Text
wrapping. On the recorded Linux benchmark, the 5 MiB Markdown paragraph now
finishes in 531 ms; it previously timed out after 10 seconds. Source and rendered
lines still occupy memory. See [measured results](benchmarks.md#004-text-wrapping-results)
for input-path distinctions, output verification and reproducible samples.

### Headerless text encodings must be selected explicitly

0.0.4 supports `--encoding utf-16`, `utf-16le`, `utf-16be` and
`utf-8`. Default files retain UTF-8 replacement decoding; stdin and URLs remain
strict UTF-8. A recognized UTF-16 BOM gets an actionable hint. Headerless UTF-16
is not guessed: select its byte order explicitly or convert to UTF-8. See
[encoding troubleshooting](troubleshooting.md#text-encoding).

### Syntax parsing remains costly for varied source

The off-by-default 0.0.4 `syntax-cache` Cargo feature helps repeated boilerplate
whose parser state stays unchanged. The off-by-default `onig` feature (0.0.11)
makes all highlighting 2–4× faster by using Oniguruma, at the cost of a C build
dependency. Default builds use uncached Syntect. Measured real-source files showed essentially unchanged runtime;
loading and parsing new syntax still costs more than plain text. See the
[workload-specific results](benchmarks.md#004-repeated-source-syntax-results).
Historical 0.0.2 Windows/Python comparisons are retained separately in the
benchmark page; they are not current 0.0.4 measurements. The implemented scope
of [#45](https://github.com/buchochelliq-labs/rs-rich-cli/issues/45) is repeated-line
parsing reuse, not a blanket syntax speedup.

---

## Deliberate divergences

Things that work differently on purpose. The full list, each with its reasoning,
is in [Divergences](DIVERGENCES.md); these are the ones you are most likely to
notice.

### Highlighted code does not match Python byte-for-byte

Upstream uses Pygments; this port uses `syntect`, which ships different grammars
and themes. Colours therefore differ. Everything *around* the code — width,
padding, wrapping, the background block — is parity-tested.

Replacing `syntect` would mean shipping a Pygments-equivalent lexer set, which is
out of scope. This is divergence **#18**.

### Raw `ESC` in input reaches the terminal in the upstream modes

In the modes ported from upstream (`rich FILE`, `--print`, `--markdown`,
`--json`, …), an escape character in a rendered file is passed through, exactly
as upstream passes it through. It is listed here because it surprises people,
not because it is a defect: matching upstream is the project's whole purpose.
`BEL`, backspace, vertical tab and form feed **are** stripped, as upstream
strips them.

For untrusted input, pass `--sanitize` (since 0.0.5,
[#64](https://github.com/buchochelliq-labs/rs-rich-cli/issues/64)), which shows
terminal controls as inert symbols. `rich view` and text `rich diff`, which have
no upstream behaviour to keep, sanitise by default; `--no-sanitize` turns that
off. See [Neutralize terminal controls](cli.md#neutralize-terminal-controls-in-input).

### Hyperlinks have no random `id=`

Upstream tags OSC 8 hyperlinks with a random id derived from Python's RNG.
Reproducing the byte sequence would mean reproducing the RNG, so ours omits it.
Terminals do not depend on it. Divergence **#20**.

---

## Fixed recently

Kept here so anyone on an older build still finds the symptom. Problems found
and fixed in features that were never published (such as the 0.0.11 release-test
fixes to `view`, stack traces and hyperlinks) are in the changelog instead. Full detail in
[the changelog](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/CHANGELOG.md).

| Symptom | Fixed in |
|---------|----------|
| `rich mermaid` and `rich_mermaid::Mermaid` printed a blank line after the diagram | mermaid `0.0.4`, CLI `0.0.15` |
| `rich micro preview` labelled a still image with a frame time ("100 ms") | CLI `0.0.15` |
| `NO_COLOR` also dropped bold and underline in a terminal | core `0.0.7` |
| A project `rich.toml` with `no_color = false` undid `NO_COLOR` | CLI `0.0.11` |
| Markup in plain-string table cells and tree labels printed literally | core `0.0.7` |
| `Columns` and `Tree` overflowed narrow widths | core `0.0.7` |
| `--export-svg` titled literal print text and rules with a fragment of the markup | CLI `0.0.11` |
| A multi-file `--watch --watch-exit-on-error` promised a retry it never made | CLI `0.0.10` |
| Live `print` dropped ordinary writes at widths 0 and 1; diagnostic snippets kept CRLF carriage returns | ext `0.0.7` |
| `--watch` missed same-size edits and atomic saves | CLI `0.0.7` |
| Diff exports lost graphical content when stdout was redirected | `0.0.3` |
| Notebook layout flags were ignored; title markup printed literally | `0.0.3` |
| Windows default pager failed to launch `more.com` | `0.0.3` |
| Redirected GIFs emitted cursor controls or looped forever | `0.0.3` |
| JSON rejected deep/non-finite input or rounded large integers | `0.0.3` |
| Early-closing CSV consumers produced an error | `0.0.3` |
| `--csv` printed a made-up one-column table and exited `0` on unreadable input | `0.0.2` (round 9) |
| Markdown link destinations vanished from piped output | `0.0.2` (round 9) |
| `--syntax` deleted every blank line in the file | `0.0.2` (round 8) |
| Long lines in Markdown code blocks were cropped and their tail lost | `0.0.2` (round 8) |
| Emoji and Indic text broke table and panel borders | `0.0.2` (rounds 8–9) |
| Deeply nested Markdown crashed the process | `0.0.2` (round 6) |

---

## Reporting something not listed here

Please [open an issue](https://github.com/buchochelliq-labs/rs-rich-cli/issues)
with the exact command, the input if you can share it, and what you expected. If
it is a *parity* difference from Python `rich`, [Reporting a parity
bug](parity.md#reporting-a-parity-bug) explains what makes those reports useful.
