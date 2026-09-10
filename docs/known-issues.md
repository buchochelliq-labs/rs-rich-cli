# Known issues and limitations

What does not work yet, what was never meant to, and what works differently on
purpose. Three different things, kept apart — a deliberate trade-off listed as a
bug makes a considered decision look like neglect.

**Applies to** the `rich 0.0.3` / `rs-rich 0.0.3` source snapshot, reviewed
2026-09-10 against Python
`rich` 15.0.0. Each entry links to its issue so you can check the status without
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

The 0.0.4 development build supports `--gif-mode blocks`; see the
[capability matrix](cli.md#gif-half-block-rendering-004-development).
ASCII remains the default and the redirected/colorless fallback. Normal playback
restores the cursor; Ctrl-C can leave it hidden, as before. GIF export and Sixel
playback are unsupported.

### CSV output still retains source rows

Undecorated CSV output streams styled rows, reducing output-buffer overhead.
The 0.0.4 development build removes duplicate parsed-cell storage and trims row
capacity, reducing the measured 100k-row peak RSS by about 46%. Source rows remain
in memory for measurement, so memory still scales with input.
Decorated, aligned, paged and exported CSV output still buffers. The 0.0.4
work in [#74](https://github.com/buchochelliq-labs/rs-rich-cli/issues/74) reduces
retained memory; it does not provide bounded-memory processing.

### Long-line rendering still allocates memory

The 0.0.4 development build removes repeated UTF-8 prefix scans during Text
wrapping. On the recorded Linux benchmark, the 5 MiB Markdown paragraph now
finishes in 531 ms; it previously timed out after 10 seconds. Source and rendered
lines still occupy memory. See [measured results](benchmarks.md#004-text-wrapping-results)
for input-path distinctions, output verification and reproducible samples.

### Headerless text encodings must be selected explicitly

The development build supports `--encoding utf-16`, `utf-16le`, `utf-16be` and
`utf-8`. Default files retain UTF-8 replacement decoding; stdin and URLs remain
strict UTF-8. A recognized UTF-16 BOM gets an actionable hint. Headerless UTF-16
is not guessed: select its byte order explicitly or convert to UTF-8. See
[encoding troubleshooting](troubleshooting.md#text-encoding).

### Syntax highlighting is the slowest path

Measured at roughly **4.3 ms per KB** of source, which is what drags the CLI's
advantage over Python `rich-cli` from about 27× down to about 3.8× on
syntax-heavy input. Everything else is far faster; see
[Benchmarks](benchmarks.md) for the method and the numbers.

Tracked as [#45](https://github.com/buchochelliq-labs/rs-rich-cli/issues/45).

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

### Raw `ESC` in input reaches the terminal

An escape character in a rendered file is passed through, exactly as upstream
passes it through. It is listed here because it surprises people, not because it
is a defect: matching upstream is the project's whole purpose.

If you render untrusted input and want it neutralised, that needs an opt-in
sanitiser — [#64](https://github.com/buchochelliq-labs/rs-rich-cli/issues/64).
`BEL`, backspace, vertical tab and form feed **are** stripped, as upstream
strips them.

### Hyperlinks have no random `id=`

Upstream tags OSC 8 hyperlinks with a random id derived from Python's RNG.
Reproducing the byte sequence would mean reproducing the RNG, so ours omits it.
Terminals do not depend on it. Divergence **#20**.

---

## Fixed recently

Kept here so anyone on an older build still finds the symptom. Full detail in
[the changelog](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/CHANGELOG.md).

| Symptom | Fixed in |
|---------|----------|
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

### SVG export can squeeze CJK glyphs

SVG textLength uses character counts for some runs; wide CJK glyphs may overlap
in the exported image. The same case reproduces in pinned Python Rich 15.0.0.
Decoded text and plain terminal output retain the original characters. This is
an export-layout limitation, separate from encoding support.
