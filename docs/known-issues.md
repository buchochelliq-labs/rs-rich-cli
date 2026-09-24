# Known issues and limitations

What does not work yet, what was never meant to, and what works differently on
purpose. Three different things, kept apart — a deliberate trade-off listed as a
bug makes a considered decision look like neglect.

**Applies to** the 0.0.11 cohort (`rs-rich-cli` 0.0.11, `rs-rich` 0.0.7,
`rs-rich-ext` 0.0.9, `rs-rich-art` 0.0.9, `rs-rich-macros` 0.0.1), prepared but
not yet published, reviewed 2026-09-24 against Python `rich` 15.0.0. The latest
published cohort is 0.0.10. Each entry links to its issue so you can check the status without
waiting for this page to be updated.

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

---

## Limitations

Things that are not implemented, and are not claimed to be.

### Windows legacy console is not supported

`rich` assumes a VT-capable terminal. On Windows it enables virtual terminal
processing; on a console that cannot do VT (very old `cmd.exe`), output will
contain raw escape sequences.

Upstream falls back to the Win32 console API for these. That port has not been
done — [#12](https://github.com/buchochelliq-labs/rs-rich-cli/issues/12).
**Workaround:** use Windows Terminal, or `--no-color`.

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

### `rs-rich-macros` is not on crates.io yet

`rs-rich-macros` 0.0.1 is new in the 0.0.11 cohort. Until the cohort is
published, `rs-rich-ext`'s `macros` feature (`richf!`, `#[derive(Rich)]`) works
only from a checkout of this repository.

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
whose parser state stays unchanged. Default builds use uncached Syntect. Measured real-source files showed essentially unchanged runtime;
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
| `NO_COLOR` also dropped bold and underline in a terminal | core `0.0.7` (prepared) |
| A project `rich.toml` with `no_color = false` undid `NO_COLOR` | CLI `0.0.11` (prepared) |
| Markup in plain-string table cells and tree labels printed literally | core `0.0.7` (prepared) |
| `Columns` and `Tree` overflowed narrow widths | core `0.0.7` (prepared) |
| `--export-svg` titled literal print text and rules with a fragment of the markup | CLI `0.0.11` (prepared) |
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
