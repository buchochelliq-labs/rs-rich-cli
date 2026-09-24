# CLI workflow recipes

These recipes target the 0.0.11 source (prepared, not yet published; 0.0.10
is the latest published CLI). Build the current CLI with
`cargo build -p rs-rich-cli` and put the resulting `rich` binary on your PATH.
The sections from [Gate CI on a text diff](#gate-ci-on-a-text-diff) onwards
need 0.0.11.
Commands below use a POSIX shell; run them from the repository root unless
using your own input paths.

## Watch JSON while editing

Create `status.json` with valid JSON, then run:

```bash
rich json status.json --no-config --watch --watch-interval 0.5 --width 72
```

Edit and save the file in another window. In a terminal, watch mode reacts to
file events (debounced by `--watch-debounce`, default 0.1 s) and
clears/repaints the terminal viewport for changed content. `--watch-interval`
only matters with `--watch-poll` (use it on network filesystems) or for URLs.
Invalid JSON and temporary missing files (including atomic-save gaps) produce
recoverable frames; fixing the file renders it again. Add
`--watch-exit-on-error` to stop with a non-zero exit instead. Press Ctrl+C to
stop.

Watch several files side by side, each in its own region:

```bash
rich --no-config --watch status.json notes.md
```

Redirecting stdout deliberately renders one snapshot and exits:

```bash
rich json status.json --no-config --watch --width 72 > status.txt
```

Watch takes local files or one URL, not stdin or literal markup. URL polling
requires the `fetch` feature; add `--watch-cache` to suppress unchanged URL
responses. Watch intervals must be positive finite seconds; the debounce may
be 0 to 3600 seconds.

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
options, plus `height`, `watch`, `watch_cache`, `watch_interval`, `watch_debounce`,
`watch_poll`, `watch_exit_on_error`, `sanitize`,
`auto_pager`, `format`, `theme_file`, `log_presentation`, `image_fit`,
`image_anchor`, `image_background`, `image_max_width`, `image_max_height`,
`image_color`, `image_dither`, `image_color_distance`, `image_brightness`,
`image_contrast`, `image_gamma`, `image_rotate`, `image_flip_horizontal`,
`image_flip_vertical` and `image_grayscale`. `rich config reference` lists every
key with its type, default and flag. Use a still-image
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
[0.0.11 release notes](releases/0.0.11.md) for release status.


## Style a team notice and its export

```bash
cat > team-theme.toml <<'TOML'
[defaults]
theme = "team"

[themes.team]
notice = "bold cyan"
warning = "bold yellow"
"markdown.h1" = "bold magenta"

[profiles.review]
theme = "team"
width = 88
TOML
rich config validate --config team-theme.toml --profile review
rich --config team-theme.toml --profile review --print '[notice]Ready for review[/]'
rich --config team-theme.toml --theme team --theme-style 'notice=bold green' \
  --print '[notice]Approved[/]' --export-html notice.html
```

CLI bindings override the selected theme's bindings. Named theme selection uses
default/profile/CLI precedence. Batch exports inherit a resolved snapshot of
those bindings. Invalid definitions fail even when their theme is not selected.

## Monitor and cancel a batch

```bash
mkdir -p rendered
rich markdown --no-config --batch 'docs/tutorial/*.md' --jobs 4 \
  --export-html rendered/document.html --collision suffix
```

On an interactive stderr terminal, human reports include completed/failed/total
progress. Press Ctrl+C during a long batch to stop scheduling and terminate and
reap started workers; the command exits 130. Inputs remain intact; completed
exports may remain. To suppress status updates, add `--no-progress`.

A machine report stays exactly one JSON document on stderr, with no progress
mixed in, including when interrupted:

```bash
rich markdown --no-config --batch 'docs/tutorial/*.md' --jobs 4 \
  --export-html rendered/document.html --collision suffix --report json \
  > rendered/content.txt 2> rendered/report.json
```

## Diagnose a terminal or play one tour section

```bash
rich doctor --no-config
rich doctor --no-config --report json > doctor.json
rich --demo-list
rich --demo --demo-section core --demo-delay 0
rich --demo --demo-section workflows --no-color > workflows.txt
rich --demo --demo-section art
```

Doctor's successful JSON document is on stdout. It reports selected capabilities,
configuration and pager choice without probing the terminal, fetching URLs or
launching the pager. Sixel support is inferred, not tested. The selected tour
runs once; redirected output stays finite and Ctrl+C restores terminal state.
Lean builds explain the unavailable art section.

## Compare palette reduction with dithering

Use any local `photo.png` with gradients. Explicit ASCII/blocks mode keeps the
comparison independent of terminal graphics detection:

```bash
rich image photo.png --no-config --image-mode blocks --width 60 --image-color truecolor
rich image photo.png --no-config --image-mode blocks --width 60 --image-color ansi256
rich image photo.png --no-config --image-mode blocks --width 60 \
  --image-color ansi256 --image-dither floyd-steinberg
rich image photo.png --no-config --image-mode ascii --width 60 \
  --image-color ansi256 --image-dither floyd-steinberg --export-html dither.html
```

Dithering needs a quantized `--image-color`: `ansi256`, `ansi16` or
`grayscale`. It works with ASCII, half-block, quadrant and Sixel still images and
with GIF frames; Braille is monochrome and rejects a reduced palette, and image
diffs ignore these controls. The dithers are `floyd-steinberg`, `bayer4x4` and
`atkinson`. The nearest colour is measured in encoded RGB by default;
`--image-color-distance oklab` measures perceptually instead, which keeps hues
truer on the ANSI16 palette:

```bash
rich image photo.png --no-config --image-mode quadrants --width 60 \
  --image-color ansi16 --image-dither atkinson --image-color-distance oklab
```

`--image-dither none` restores the default no-diffusion path; truecolor/no-dither
preserves the existing rendering policy. These controls require the `art` feature.

## Gate CI on a text diff

```bash
rich diff expected.txt actual.txt --threshold 5 --no-pager
```

The diff is printed either way. When more than 5% of the lines changed, `rich`
prints `FAIL` and exits `5`, so the CI step fails. Text diffs show terminal
controls in the files as inert symbols by default.

## Gate CI on a benchmark regression

```bash
rich bench compare baseline.json candidate.json --threshold 10
rich bench compare target/criterion-main target/criterion
```

Any benchmark slower by more than 10% (and outside the noise) exits `5`. The
inputs are runs saved by `rich_ext::qa::bench` or criterion output directories.

## Capture a command's output in CI

```bash
rich capture --export-svg test-run.svg --cast test-run.cast -- cargo test
```

`capture` runs the command with colour forced, shows its output in a panel, and
exits with the command's own status, so a failing `cargo test` still fails the
step while the SVG and asciicast are written. Add `--redact` to mask secrets
before anything is shown or written; it is experimental and best effort, so
read the output before publishing it:

```bash
rich capture --redact --export-html deploy.html -- ./deploy.sh
```

## Check PATH and secrets in the environment

```bash
rich env PATH           # one row per entry: ok, missing, duplicate or empty
rich env AWS 'DB_*'     # filter names by substring or glob; secrets are masked
```

## Read any file, and compare two configs

```bash
rich view src/main.rs --search todo
rich inspect deploy.yaml --compare deploy.prod.yaml
rich inspect settings.json --select '$.servers[*].name' --redact
```

`view` picks the right renderer (or a hex dump for binary) and pages tall
output. `inspect --compare` lists added, removed and changed values.

## Use an upstream rich theme file

```bash
rich --theme-file night.ini --print '[notice]Ready[/] in 42 ms'
```

The file's `[styles]` section is read as upstream rich reads it. Put
`theme_file = "night.ini"` in your own config to make it the default; a
working-directory `rich.toml` cannot set it.

## Install shell completions

```bash
rich completions bash > ~/.local/share/bash-completion/completions/rich
rich completions zsh  > "${fpath[1]}/_rich"
rich completions fish > ~/.config/fish/completions/rich.fish
```
