# CLI workflow recipes

These recipes target the 0.0.8 source preparation. Build the current CLI with
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
  --continue-on-error --jobs 4 --width 88 --report json \
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

`--continue-on-error` attempts later files after a render failure. By default,
a failure stops scheduling new work; workers already running finish. Planning
errors still stop before rendering.
The aggregate JSON report goes to stderr and the rendered text to stdout.
A failing item still causes a nonzero batch exit status.

`--jobs N` bounds concurrent subprocess workers for file exports. Their output
is spooled to temporary files to bound parent buffering and replayed in input
order. Terminal-only batches stay serial. Startup and I/O costs can outweigh
parallelism on small inputs; increasing jobs does not guarantee a speedup.

Preview the input/destination plan without writing exports:

```bash
rich markdown --no-config --batch 'docs/tutorial/*.md' \
  --export-html rendered/document.html --collision suffix --dry-run
```

Dry-run reports planned inputs, destinations and planning errors without creating
parent directories, export files or worker spools. It does not render or validate
each document's contents. Missing destination directories are reported as planning
errors; create them separately before an actual export.

## Reuse compact, CI, and documentation profiles

Save this as `rich.toml`. Table order does not affect precedence:

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

The config file uses full TOML syntax with a strict schema. Unknown keys,
wrong types and invalid values are errors, including in unselected profiles.
Defaults are merged first, then the selected profile, then explicit CLI values.
Profile `false` values cancel inherited `true` values. Inverse CLI flags such as
`--no-pager`, `--no-watch`, `--no-overwrite`, `--no-batch`,
`--no-continue-on-error`, `--no-sanitize` and `--color` override configured
booleans. Arguments after `--` remain operands.

Inspect or validate configuration without rendering an input:

```bash
rich config validate --config rich.toml --profile ci
rich config show --config rich.toml --profile compact --width 64
rich json status.json --config rich.toml --profile compact --no-pager
```

Both config commands return JSON. The `settings` object contains configured
settings after profile and explicit CLI overrides; it does not expand every
built-in CLI default. An empty object does not mean the renderer has no defaults.

Supported settings include the earlier mode, width, export, batch and decoration
options, plus `height`, `watch`, `watch_cache`, `watch_interval`, `sanitize`,
`auto_pager`, `image_fit`, `image_anchor` and `image_background`. Use a still-image
command explicitly when configuring image geometry:

```toml
[profiles.thumbnail]
width = 40
height = 12
image_fit = "cover"
image_anchor = "top-left"
image_background = "#202830"
```

## Page tall terminal output automatically

```bash
rich markdown README.md --no-config --auto-pager
rich markdown README.md --no-config --auto-pager > readme.txt
rich markdown README.md --config rich.toml --no-pager
```

Automatic paging is opt-in and applies only when stdout is a terminal and the
rendered output exceeds its viewport height. Redirected stdout is never paged.
`--no-pager` disables configured explicit or automatic paging; `--no-auto-pager`
disables only automatic paging. Existing explicit `--pager` remains available.

## Keep a subject near an image edge

```bash
rich image photo.png --no-config --image-mode blocks --width 40 --height 12 \
  --image-fit cover --image-anchor top-left --image-background '#202830'
```

Choose `center`, `top`, `bottom`, `left`, `right`, `top-left`, `top-right`,
`bottom-left` or `bottom-right`. Center remains the default. The CLI requires
`--image-fit cover` when an anchor is supplied; `contain` always centers the
whole image with padding. In the library, `ImageArt::anchor(ImageAnchor::TopLeft)`
is ignored for contain or unfitted images.

See [Using the CLI](cli.md) for individual options and the
[0.0.8 preparation notes](releases/0.0.8.md) for release status.
