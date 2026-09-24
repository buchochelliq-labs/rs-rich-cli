# Troubleshooting

Error messages exactly as `rich` prints them, with what causes each and what to
do. All diagnostics go to **stderr** and every failure exits non-zero, so a
failed render never looks like a successful one to a script. The exit code says
what kind of failure it was:

| Code | Meaning |
|---|---|
| 0 | success |
| 2 | usage or config error (a bad flag, value or config file) |
| 3 | input, read or write error (a missing file, an unreadable image) |
| 4 | parse or render error in the data (bad markup, invalid JSON) |
| 5 | a `--threshold` gate failed |
| 130 | interrupted with Ctrl+C |

`rich capture` is the exception: it exits with the captured command's own
status (128 + the signal number when a signal ended it).

**Applies to** `rich 0.0.11` (`rs-rich-cli` 0.0.11, prepared but not yet
published). If your version differs, check
[the changelog](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/CHANGELOG.md).

---

## `rich: cannot read <path>: No such file or directory (os error 2)`

The resource does not exist at that path. This is the Linux and macOS wording;
Windows says `The system cannot find the file specified. (os error 2)`. Either
way the exit code is 3.

Check the path, and remember that a leading `-` makes `rich` read the argument
as an option. Put it after `--`:

```bash
rich -- -leading-dash.md
```

## `rich: cannot read <path>: is a directory, not a file`

You passed a directory. `rich` renders one resource at a time; point it at a
file, or convert many files at once with `--batch`, which takes files,
directories and globs:

```bash
rich --batch --markdown --export-html out.html docs/
```

See [Convert many files at once](cli.md#convert-many-files-at-once).

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

## `rich: only one render mode (--print/--markdown/--json/--syntax/--csv/--ipynb/--rule/--gif/--diff/--image/--jsonl/--log/--inspect/--ansi-explain) may be given (try --help)`

Two mode flags were passed. Pick one — they are alternatives, not layers.

## `rich: unknown option "--<name>" (try --help)`

No such flag. `rich --help` lists all of them, and the
[CLI reference](cli-reference.md) is the same content in a searchable page.

## `rich: --<flag> requires a value (try --help)` / `rich: invalid width '<value>' (try --help)`

The flag needs a value and either got none or got something that is not a
number. `--width 80`, not `--width` or `--width wide`.

## `rich: --panel-style only has an effect with --panel (try --help)`

The flag was passed but nothing would use it. `rich` refuses rather than
silently ignoring it, so a typo in a script surfaces instead of quietly doing
nothing.

Add the flag it depends on, or drop it:

```bash
rich --panel rounded --panel-style dim notes.md
```

## `rich: --diff needs two images or files to compare, or one patch: --diff before after (try --help)`

`--diff` (or `rich diff`) takes two images, two text files, or one patch such
as `git diff` output. Anything else is a usage error (exit 2). See
[Comparing images](image-diff.md) and
[Compare text, source and patches](cli.md#compare-text-source-and-patches).

## `rich: <file>: no file changes found in it; give two files to compare`

A single input to `rich diff` is read as a patch, and this one has no file
changes in it. Give two files to compare them, or pipe a real patch:
`git diff | rich diff -`. The exit code is 4.

## `rich: cannot read <file>: The image format <Name> is not supported`

`--image` (or `rich image`) recognised the file as an image in a format `rich`
cannot decode. Convert it to PNG, JPEG, GIF, WebP or BMP first. A truncated or
corrupt image fails the same way, with the decoder's message (such as
`unexpected end of file`). The exit code is 3.

## `rich: --theme-file <path>: line <N>: <problem> (try --help)`

The theme file is not a valid upstream rich theme file: every line in its
`[styles]` section must be `name = style`. The message names the line. A file
that cannot be read (`No such file or directory`), is not a regular file, or is
over 1 MiB fails the same way. All of these are usage errors (exit 2). See
[Load an upstream theme file](cli.md#load-an-upstream-theme-file).

## A notice that input was cut short

`view`, `hex`, `unicode`, `inspect` and `capture` read a bounded amount, so an
endless input such as `/dev/zero` ends instead of exhausting memory. Beyond the
limit, `view`, `hex` and `unicode` show the first part and print a notice on
stderr; `capture` stops the command; `inspect` fails with an input error
(exit 3). The table in [Limits](cli.md#limits) lists each cap. For a large
binary file, `rich hex --offset N --length N` reads any window.

---

## Output problems that are not errors

### The output has no colour

Colour is disabled when stdout is **not a terminal** — piping or redirecting is
enough. It is also disabled by a non-empty `NO_COLOR` environment variable or by
`--no-color`. In a terminal, those remove colour only: bold, italic, underline
and the other attributes stay, as in upstream rich.

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

### `rich` shows a demo when I give it nothing

With no resource and no mode flag, `rich` shows its capability demo. Use
`rich --help` for options. Demo layout/style/paging/export flags are rejected
with a diagnostic; add a resource or render mode to use those options.

### A mode is waiting for input

Input modes without a resource, or with `-`, read stdin until EOF. In a terminal,
`rich` prints an input hint; finish with Ctrl-D on Unix or Ctrl-Z then Enter on
Windows. Piped input keeps working without a hint.

---


## Text encoding

In 0.0.4, select a known encoding explicitly:

```bash
rich notes.txt --encoding utf-16
rich notes.txt --encoding utf-16le
cat notes.txt | rich - --encoding utf-16be
rich https://example.com/notes.txt --encoding utf-16
```

`utf-16` requires a byte-order mark (BOM). `utf-16le` and `utf-16be` also accept
headerless input; use the byte order specified by the file's producer. Decoding
is strict: odd byte counts, unpaired surrogates and contradictory BOMs fail with
nonzero status before rendering. Generic `utf-16` rejects UTF-32 BOM signatures;
the little-endian signature is also possible for UTF-16 BOM followed by NUL, so
select `utf-16le` explicitly for that known case. UTF-32 is not auto-detected.
Only one BOM is consumed. File/stdin newlines are normalized as before.

Without this option, files keep upstream-compatible UTF-8 replacement decoding.
A recognized UTF-16 BOM prints a hint, without silently changing decoding or the
exit status. Headerless encodings are not guessed. Stdin and fetched bodies
remain strict UTF-8; `--encoding utf-8` additionally makes file decoding strict.
URL download limits and TLS checks are unchanged. Encoding does not apply to
literal `--print` text, rules, GIF playback, image diffs or the capability demo.

An unreadable image passed as text now produces one actionable `rich --diff`
hint. A valid text file with an image suffix still renders as text.

## Reporting a bug

Include:

1. The exact command, with the file if you can share it.
2. What you saw and what you expected.
3. `rich --version`, your OS, and your terminal. `rich doctor --report json`
   records these and what `rich` detected; attach its output.
4. Whether it also happens with `--no-color` and at a fixed `--width`, which
   separates rendering problems from terminal ones.

Issues: <https://github.com/buchochelliq-labs/rs-rich-cli/issues>

For anything security-relevant, please **do not** open a public issue — see the
project's security policy in the repository.
