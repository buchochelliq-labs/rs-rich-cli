# Known issues and limitations

What does not work yet, what was never meant to, and what works differently on
purpose. Three different things, kept apart — a deliberate trade-off listed as a
bug makes a considered decision look like neglect.

**Applies to** `rich 0.0.1` / `rs-rich 0.0.1`, verified 2026-08-11 against Python
`rich` 15.0.0. Each entry links to its issue so you can check the status without
waiting for this page to be updated.

---

## Bugs

Things that should work and do not.

### `--json` can emit invalid JSON when a line is cropped

**Symptom.** Piping `rich --json` through a parser fails with something like
`Invalid control character at: line 3 column 41`.

**Scope.** Only when a value is wider than the render width, so the line is
cropped mid-escape. Reproduces at any narrow width:

```bash
rich --json wide.json --width 40 | jq .
```

**Workaround.** Render at a width that fits the longest value, or drop `--width`
and let it use the terminal's:

```bash
rich --json wide.json --width 200
```

**Status.** Open —
[#67](https://github.com/buchochelliq-labs/rs-rich-cli/issues/67).
`rich --json` is for reading, not for piping into a parser; use `jq` on the raw
file when you need machine-readable output.

### Markdown images: five smaller divergences remain

Images carry upstream's marker and are hoisted above their paragraph, but an
image **inside a table cell** is not hoisted, two images in one container split
across rows, and consecutive hoisted images gain a blank row.

**Scope.** Documents whose images sit in table cells — a README badge table is
the common case.

**Status.** Open —
[#86 follow-ups](https://github.com/buchochelliq-labs/rs-rich-cli/issues).

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
| `--csv` printed a made-up one-column table and exited `0` on unreadable input | `0.0.1` (round 9) |
| Markdown link destinations vanished from piped output | `0.0.1` (round 9) |
| `--syntax` deleted every blank line in the file | `0.0.1` (round 8) |
| Long lines in Markdown code blocks were cropped and their tail lost | `0.0.1` (round 8) |
| Emoji and Indic text broke table and panel borders | `0.0.1` (rounds 8–9) |
| Deeply nested Markdown crashed the process | `0.0.1` (round 6) |

---

## Reporting something not listed here

Please [open an issue](https://github.com/buchochelliq-labs/rs-rich-cli/issues)
with the exact command, the input if you can share it, and what you expected. If
it is a *parity* difference from Python `rich`, [Reporting a parity
bug](parity.md#reporting-a-parity-bug) explains what makes those reports useful.
