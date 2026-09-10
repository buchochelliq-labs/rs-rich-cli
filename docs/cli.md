# Using the CLI

`rich` renders files that are painful to read in a terminal — Markdown, JSON,
CSV, source code, notebooks — and can compare two images. This page is organised
by what you are trying to do. For the complete list of options, see the
[CLI reference](cli-reference.md).

**Assumes** you can run commands in a terminal. Every example below was run
against `rich 0.0.2` and shows its real output, with colour removed for print.

---

## Read a file

Point `rich` at a file and it picks a renderer from the extension: `.md`,
`.json`, `.csv`, `.tsv` and `.ipynb` get their own; **anything else with an
extension is syntax-highlighted**.

```bash
rich README.md
rich data.json
rich main.rs
```

Force a renderer when the extension is missing or misleading:

```bash
rich --markdown CHANGELOG
rich --syntax --width 100 script
```

Read from standard input with `-` (including `-p -` for markup):

```bash
cat data.csv | rich --csv -
```

Input modes without a resource also read stdin until EOF. Interactive input
prints a hint: finish with Ctrl-D on Unix, or Ctrl-Z then Enter on Windows.
With no mode and no resource, `rich` runs its capability demo. The demo accepts
`--no-color`; layout, style, paging, hyperlink and export options require a
resource or render mode and are rejected for the demo.

Repeated scalar options use the last value, including `--width` and export paths.

!!! tip "Filenames that begin with a dash"

    Everything after a bare `--` is treated as the resource, however much it
    looks like an option: `rich -- -weird-name.md`.

## Render a CSV as a table

```bash
rich --csv team.csv --title "Team"
```

```text
             Team
┏━━━━━━━┳━━━━━━━━━━┳━━━━━━━━━┓
┃ name  ┃ role     ┃ commits ┃
┡━━━━━━━╇━━━━━━━━━━╇━━━━━━━━━┩
│ Ada   │ author   │     120 │
│ Grace │ reviewer │      98 │
└───────┴──────────┴─────────┘
```

The delimiter and whether row 1 is a header are **detected**, not assumed, so
semicolon- and tab-separated exports work without a flag. Numeric columns are
right-aligned automatically.

If the delimiter cannot be determined and the file is not `.csv`/`.tsv`, `rich`
reports it and **exits non-zero** rather than inventing a one-column table:

```bash
rich --csv notes.txt
```

```text
rich: Could not determine delimiter
```

## Render Markdown, and keep the links readable

```bash
rich notes.md
```

```text
                           Notes

See the docs (https://example.com/docs) for detail.
```

Link destinations are printed after the label, so a piped or redirected render
keeps them. Pass `-y/--hyperlinks` to emit real clickable
[OSC 8](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda)
hyperlinks instead — useful in a terminal, lossy in a pipe:

```bash
rich --hyperlinks notes.md
```

## Frame and position the output

```bash
rich --panel rounded --panel-style dim --print "Ready"
```

```text
╭───────╮
│ Ready │
╰───────╯
```

A panel **shrinks to its content**. Use `-e/--expand` to fill the width instead.

- `--style` styles the content; `--panel-style` styles the border. They are
  different flags because they do different things.
- `--width N` bounds the *rendered block*, not the console, so `--center` still
  positions it within your real terminal width.
- `--title` and `--caption` interpret Rich markup; on a CSV they also become
  the table's title and caption. Rules interpret markup in their resource title.
- Notebooks use the same layout chain, so padding, panel, style, width and
  alignment apply to the complete notebook, including its outputs.

## Export what you rendered

```bash
rich report.md --export-html report.html
rich report.md --export-svg report.svg
```

The HTML is self-contained. The SVG references its font from a CDN, so it is
**not** self-contained offline. Both exports may be requested together, and
stdout is still printed once. Diff reports choose color blocks for HTML/SVG
independently of redirected stdout, which stays readable ASCII. Explicit
`--image-mode ascii`, `--image-mode none` and `--no-color` are respected; Sixel
requests use blocks in exported documents.

## Compare two images

```bash
rich --diff before.png after.png --threshold 2
```

Reports the regions that changed, and exits `1` when more than `2%` of the image
differs — which makes it usable as a CI gate. See
[Comparing images](image-diff.md) for the modes and how the comparison works.

## Use it in a script or CI

`rich` writes rendered output to stdout and diagnostics to stderr, so the two
can be separated:

```bash
rich --csv data.csv > table.txt 2> errors.txt
```

Exit codes are `0` for success and `1` for failure — including a resource that
cannot be read or parsed. Check them:

```bash
if rich --csv "$f" > /dev/null 2>&1; then
  echo "readable"
else
  echo "could not render $f" >&2
fi
```

Colour is disabled automatically when output is not a terminal, and by
a non-empty `NO_COLOR` or `--no-color` when it is. `FORCE_COLOR` is unsupported;
setting it does not add escape sequences to redirected stdout. `COLUMNS` sets
the console width (80 when neither terminal width nor the variable is available).

`--pager` tries a non-empty `MANPAGER`, then `PAGER`, then `less` on Unix or
`more.com` on Windows. GIF playback repeats once by default; `--loop 0` repeats
forever in a terminal. Pipes receive the first frame once, even with `--loop 0`.

---

## Where to go next

- [CLI reference](cli-reference.md) — every option, generated from `--help`
- [Comparing images](image-diff.md) — the `--diff` workflow in depth
- [Troubleshooting](troubleshooting.md) — error messages and what to do about them
- [Parity with Python rich](parity.md) — how close the output is, and where it differs

## Reading UTF-16 text (0.0.4 development)

Use `rich notes.txt --encoding utf-16` for a BOM-marked file, or explicitly
select `utf-16le` / `utf-16be` for headerless input. The same option works on
stdin and URLs. See [text encoding](troubleshooting.md#text-encoding) for strict
error handling and unchanged default decoding.
