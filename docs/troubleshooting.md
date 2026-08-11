# Troubleshooting

Error messages exactly as `rich` prints them, with what causes each and what to
do. All diagnostics go to **stderr** and every failure exits **1**, so a failed
render never looks like a successful one to a script.

**Applies to** `rich 0.0.2`. If your version differs, check
[the changelog](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/CHANGELOG.md).

---

## `rich: cannot read <path>: The system cannot find the file specified. (os error 2)`

The resource does not exist at that path.

Check the path, and remember that a leading `-` makes `rich` read the argument
as an option. Put it after `--`:

```bash
rich -- -leading-dash.md
```

## `rich: cannot read <path>: is a directory, not a file`

You passed a directory. `rich` renders one resource at a time; point it at a
file, or loop:

```bash
for f in docs/*.md; do rich "$f"; done
```

## `rich: Could not determine delimiter`

`--csv` could not work out how the file is separated, and the extension is not
`.csv` or `.tsv` so there is no fallback to use.

This is usually a file that is not tabular at all. If it *is* tabular, give it a
`.csv` or `.tsv` extension — those fall back to comma and tab respectively
without sniffing.

`rich` deliberately fails here rather than rendering a one-column table, so that
`rich --csv "$f" && publish` does not proceed on unreadable input.

## `rich: invalid JSON: json parse error: <detail> at line L column C`

The document is not valid JSON. The line and column point at the first problem.

`rich` accepts `NaN`, `Infinity` and `-Infinity` (as Python's `json` module
emits them) and nests arbitrarily deep, so those are not the cause.

## `rich: only one render mode (--print/--markdown/--json/--syntax/--csv/--ipynb/--rule) may be given`

Two mode flags were passed. Pick one — they are alternatives, not layers.

## `rich: unknown option "--<name>"`

No such flag. `rich --help` lists all of them, and the
[CLI reference](cli-reference.md) is the same content in a searchable page.

## `rich: --<flag> requires a number` / `rich: invalid width '<value>'`

The flag needs a value and either got none or got something that is not a
number. `--width 80`, not `--width` or `--width wide`.

## `rich: --panel-style only has an effect with --panel`

The flag was passed but nothing would use it. `rich` refuses rather than
silently ignoring it, so a typo in a script surfaces instead of quietly doing
nothing.

Add the flag it depends on, or drop it:

```bash
rich --panel rounded --panel-style dim notes.md
```

## `rich: --diff needs exactly two images: --diff before.png after.png`

`--diff` compares exactly two images. See [Comparing images](image-diff.md).

---

## Output problems that are not errors

### The output has no colour

Colour is disabled when stdout is **not a terminal** — piping or redirecting is
enough. It is also disabled by a non-empty `NO_COLOR` environment variable or by
`--no-color`.

To keep colour through a pager, use `--pager`, which `rich` sets up itself.

### The URL is missing from a Markdown link

You are running with `-y/--hyperlinks`, which emits OSC 8 hyperlinks — the
terminal shows the label and hides the target. Drop the flag to get
`label (url)`, which survives a pipe.

### The table is wider than my terminal, or its borders look broken

Check that the terminal's own idea of its width matches reality (`COLUMNS`), and
that the font renders the box-drawing characters. If a line containing emoji
overflows, please
[report it](https://github.com/buchochelliq-labs/rs-rich-cli/issues/new) with
the exact input — width handling is measured against Python `rich` and that
class of defect is treated as a bug.

### `rich` prints its help and exits 0 when I give it nothing

That is intended, and matches upstream `rich-cli`: with no resource and no mode
flag there is nothing to render.

---

## Reporting a bug

Include:

1. The exact command, with the file if you can share it.
2. What you saw and what you expected.
3. `rich --version`, your OS, and your terminal.
4. Whether it also happens with `--no-color` and at a fixed `--width`, which
   separates rendering problems from terminal ones.

Issues: <https://github.com/buchochelliq-labs/rs-rich-cli/issues>

For anything security-relevant, please **do not** open a public issue — see the
project's security policy in the repository.
