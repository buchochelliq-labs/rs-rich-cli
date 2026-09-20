# CLI workflow recipes

These recipes target the 0.0.7 source workstream. Build the current CLI with
`cargo build -p rs-rich-cli` and put the resulting `rich` binary on your PATH.
Commands below use a POSIX shell; run them from the repository root unless
using your own input paths.

## Watch JSON while editing

Create `status.json` with valid JSON, then run:

```bash
rich json status.json --no-config --watch --watch-interval 0.5 --width 72
```

Edit and save the file in another window. In a terminal, watch mode polls every
half second and clears/repaints the terminal viewport for changed content.
Each poll reads the local file using a fixed 64 KiB buffer; choose a longer
interval for large files to reduce I/O. Invalid JSON and temporary missing
files (including atomic-save gaps) produce recoverable frames; fixing the file
allows the next poll to render it again. Press Ctrl+C to stop.

Redirecting stdout deliberately renders one snapshot and exits:

```bash
rich json status.json --no-config --watch --width 72 > status.txt
```

Watch takes a local file or URL, not stdin or literal markup. URL polling
requires the `fetch` feature; add `--watch-cache` to suppress unchanged URL
responses. Watch intervals must be positive finite seconds.

## Export Markdown files to HTML in a batch

Create the output directory before rendering. The quoted glob is expanded by
`rich`, using `*` and `?` in the final path segment:

```bash
mkdir -p rendered
rich markdown --no-config --batch 'docs/tutorial/*.md' \
  --export-html rendered/document.html --collision suffix \
  --continue-on-error --jobs 1 --width 88 --report json \
  > rendered/terminal.txt 2> rendered/report.json
```

For multiple inputs, the export path supplies the directory and extension:
`01-hello.md` becomes `rendered/01-hello.html`. For exactly one input, the
literal destination `rendered/document.html` is used. Input directories are
walked recursively; expanded files are sorted and deduplicated. Batch accepts
local inputs only, and excludes GIF playback and image diff mode.

`--collision suffix` chooses a free numbered destination for name collisions
and existing files. The default policy is `error`; `--overwrite` permits
replacing existing files, while `--collision overwrite` also permits planned
items to share a destination. Choose these only when replacement is intended.

`--continue-on-error` attempts later files after a render failure; the default
stops at the first failure. Planning errors still stop before rendering.
The aggregate JSON report goes to stderr and the rendered text to stdout.
A failing item still causes a nonzero batch exit status.

**Batch execution is serial in 0.0.7.** `--jobs N` accepts a positive integer
reserved as a future concurrency limit; increasing it does not create parallel
workers or improve throughput in this implementation.

## Reuse compact, CI, and documentation profiles

Save this as `rich.toml`. Keep `[defaults]` before the named profiles:

```toml
[defaults]
width = 80

[profiles.compact]
width = 48
panel = "rounded"
padding = "0,1"

[profiles.ci]
width = 100
no_color = true

[profiles.docs]
width = 88
mode = "markdown"
```

Select each profile explicitly:

```bash
rich json status.json --config rich.toml --profile compact
rich json status.json --config rich.toml --profile ci --report json
rich README.md --config rich.toml --profile docs --export-html rendered/readme.html
```

Explicit CLI values override config defaults, including short aliases and
subcommand render modes:

```bash
rich json status.json --config rich.toml --profile compact -w 64
rich json status.json --config rich.toml --profile docs
```

Without `--config`, discovery checks `./rich.toml`, then
`$HOME/.config/rich/config.toml` (`USERPROFILE` is used when `HOME` is absent).
`--no-config` disables discovery and explicit config/profile options. Named
sections may use `[profiles.NAME]` or `[profile.NAME]`; the default selected
name is `default`.

The parser supports scalar settings for `mode`, `width`, `pager`, `no_color`,
`export_html`, `export_svg`, `batch`, `continue_on_error`, `overwrite`, `jobs`,
`collision`, `panel`, and `padding`. Keep watch, machine-report, and image
options on the command line. This is a supported TOML-style subset, not a
validated versioned schema or a general TOML parser. Boolean `false` does not
cancel a `true` setting already enabled by defaults; put opt-in booleans in the
specific profiles that need them.

See [Using the CLI](cli.md) for individual options and the
[0.0.7 preparation notes](releases/0.0.7.md) for release status.
