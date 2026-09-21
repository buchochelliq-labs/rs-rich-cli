# Using the CLI

`rich` renders files that are painful to read in a terminal — Markdown, JSON,
CSV, source code, notebooks — and can compare two images. This page is organised
by what you are trying to do. For the complete list of options, see the
[CLI reference](cli-reference.md).

**Assumes** you can run commands in a terminal. Examples use real CLI output;
0.0.9 source-preparation workflows are documented below. See the
[release notes](releases/0.0.9.md) for validation and publication status.

---

## Take the guided tour

```bash
rich --demo-list              # list core, workflows, art
rich --demo --demo-section workflows  # play one group
rich --demo                   # one pass, 3 seconds between sections
rich --demo --demo-delay 5    # a slower tour
rich --demo --demo-delay 0    # skip section pauses
rich --demo --no-color > tour.txt  # finite, colour-free transcript
```

The tour walks through markup, tables, panels, layouts, Markdown, syntax,
progress, notebooks, JSON Lines, logs, config profiles, batch planning and
parallel HTML/SVG exports, watch updates, and rich-art's banners, Braille,
half-blocks, ASCII, crop/background controls, image diffs and GIF playback.
It shows commands alongside the CLI examples. URL fetching, external paging
and terminal-specific Sixel support are explained without opening a browser,
fetching a URL or launching a pager.

It runs once and exits. **Ctrl+C stops the tour**, restores the cursor and
cleans up temporary examples. Config files are ignored so the tour works
without setup; its profile example uses an isolated bundled configuration.
It never writes into your current directory. `--demo-delay` accepts 0–60
seconds and only affects section pauses on a terminal; watch/GIF examples
have their own short playback. Redirected output has no pauses or animation.
A build without the `art` feature explains that the art sections are unavailable.
`--demo-list` lists stable section names without playback. Use
`--demo --demo-section core|workflows|art` to select a group (supply one name);
unknown names fail before playback. Use `--demo` on its own for the full tour,
optionally with `--demo-section`, `--demo-delay`, `--no-color` or
`--no-config`; other rendering options and resources are rejected.

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
rich markdown CHANGELOG
rich syntax --width 100 script
```

Read from standard input with `-` (including `-p -` for markup):

```bash
cat data.csv | rich --csv -
cat data.csv | rich csv -
```

Input modes without a resource also read stdin until EOF. Interactive input
prints a hint: finish with Ctrl-D on Unix, or Ctrl-Z then Enter on Windows.
With no mode and no resource, `rich` runs its capability demo. The demo accepts
`--no-color`; layout, style, paging, hyperlink and export options require a
resource or render mode and are rejected for the demo.

Repeated scalar options use the last value, including `--width` and export paths.

## Watch a resource

Watch a local file while editing it:

```bash
rich --watch --watch-interval 0.5 report.md
```

The watcher checks local file contents at the polling interval using a bounded
buffer, so same-size edits with preserved timestamps are detected. Atomic saves, temporary
disappearance, malformed intermediate content, and later recovery are handled
as successive frames; a failed frame is reported and the watcher keeps
running. `Ctrl-C` terminates the interactive watch. URLs can be watched when
the default `fetch` feature is enabled:

```bash
rich --watch --watch-cache --watch-interval 5 https://example.com/data.json
```

`--watch-cache` hashes each fetched response and only renders changed bodies.
Without it, URLs are fetched and rendered every interval. When stdout is
redirected or piped, `--watch` renders exactly one snapshot and exits instead
of entering an interactive loop. A binary built with `--no-default-features`
does not support URL fetching, including URL watches. Watch cannot be combined
with batch or explicit/automatic paging.

!!! tip "Filenames that begin with a dash"

    Everything after a bare `--` is treated as the resource, however much it
    looks like an option: `rich -- -weird-name.md`.

## Use preferred subcommands

0.0.6 adds task-oriented subcommands while keeping every existing flat flag
working without warnings:

```bash
rich markdown README.md
rich json data.json
rich csv data.csv
rich ipynb notebook.ipynb
rich diff before.png after.png --threshold 2
rich image photo.png --width 60
```

The subcommands route through the same renderers as `--markdown`, `--json`,
`--csv`, `--ipynb`, `--diff` and `--image`. Common options such as `--width`,
`--no-color`, `--sanitize`, `--panel`, `--padding`, exports and alignment keep
their existing behavior where the mode supports them.

## Neutralize terminal controls in input

By default, `rich` keeps ESC bytes in input, matching upstream `rich` behavior.
Use `--sanitize` when displaying content you do not trust:

```bash
rich --sanitize suspicious.txt
```

The option replaces terminal controls with visible inert text before rendering,
so `ESC[2J` becomes `␛[2J` instead of clearing the screen. LF and TAB are
preserved for layout and CSV/TSV structure. The sanitizer covers decoded file,
stdin and URL text; literal `--print` input; JSON and notebook string values
decoded from escapes such as `\u001b`; panel/table titles and captions; rule
titles; and SVG document titles. It does not strip styling generated by `rich`
itself, and defaults remain unchanged when the option is absent.

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

## Stream JSONL and logs

Use `jsonl` / `--jsonl` for newline-delimited JSON. Each line is parsed and
rendered independently, so the command can consume long streams without holding
the complete input in memory:

```bash
tail -f app.jsonl | rich jsonl -
rich --jsonl events.ndjson
```

Malformed records fail fast by default and report the line number. Use `log` /
`--log` when records follow common structured-log shapes:

```bash
tail -f app.jsonl | rich log -
```

Objects with `timestamp`, `time` or `@timestamp`, `level` or `severity`, and
`message` or `msg` are flattened into a readable log line; remaining fields are
printed as compact JSON. Other values fall back to compact JSON.

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

Reports the regions that changed, and exits `5` when more than `2%` of the image
differs — which makes it usable as a CI gate. See
[Comparing images](image-diff.md) for the modes and how the comparison works.

## Render a still image

```bash
rich --image photo.png --width 60
rich image photo.png --image-mode blocks --height 20
```

Renders a single picture instead of a comparison: `--diff` needs exactly two
images, `--image` needs exactly one. It shares the same `--image-mode`
(auto/sixel/blocks/braille/ascii) and capability auto-detection as `--diff`, plus a new
`--height N` to bound the rendered rows independently of `--width`. `none` is
rejected for `--image`, because it means "draw nothing" and there is no
comparison report to fall back on. See
[Comparing images](image-diff.md) for how the renderer picks a mode.

### Fit, crop and transparent backgrounds

```bash
rich image photo.png --image-mode blocks --width 44 --height 12 --image-fit contain
rich image photo.png --image-mode blocks --width 44 --height 12 --image-fit cover --image-anchor top
rich image logo.png --image-fit contain --width 44 --height 12 --image-background '#542080'
```

`contain` centres the whole image in a padded rectangle; `cover` fills the
rectangle and crops excess edges around `--image-anchor` (default `center`).
Anchors are `center`, `top`, `bottom`, `left`, `right`, `top-left`, `top-right`,
`bottom-left` and `bottom-right`. An explicit anchor requires cover fitting;
contain always keeps the whole image centered. Both preserve aspect ratio assuming
terminal cells are twice as tall as they are wide. Fitting requires `--height`;
width defaults to the terminal width and is capped by available columns.

`--image-background '#RRGGBB'` composites transparent pixels before resizing and
colours contain padding. Fit padding defaults to black. Without either option,
existing renderer behaviour is preserved. Fitting rejects empty/zero dimensions
and rasters above 16 megapixels, including cover's intermediate resize; extreme
aspect ratios may therefore require a smaller size. These options apply to
`image`, not GIF playback or image comparisons.

Library equivalent:

```rust
use rich_art::{ImageAnchor, ImageArt, ImageFit, ImageMode};
let art = ImageArt::from_path("logo.png")?
    .mode(ImageMode::Blocks)
    .width(44).height(12)
    .fit(ImageFit::Cover)
    .anchor(ImageAnchor::Top)
    .background([84, 32, 128]);
```

[See the actual renderings](demos.md) and [workflow recipes](recipes.md).

### ANSI256 colour and dithering

```bash
rich image photo.png --image-mode blocks --width 60 --image-color ansi256
rich image photo.png --image-mode ascii --width 60 --image-color ansi256 --image-dither floyd-steinberg
```

Truecolor and no dithering remain the defaults (`--image-color truecolor`,
`--image-dither none`). Opt-in ANSI256 preprocessing supports ASCII and half-block
still images; Floyd–Steinberg requires ANSI256. Unsupported combinations are
rejected rather than ignored. Auto mode is allowed when it resolves to ASCII
or blocks; select a supported mode explicitly for predictable behavior. These
controls do not apply to Braille, Sixel, GIF playback or image comparisons.

Preprocessing runs on the final sampled raster after fitting and background
compositing, before glyph selection. It uses fixed ANSI256 entries 16–255,
excluding the first 16 terminal-theme-dependent colours. Nearest colour uses
squared distance in encoded RGB, with ties choosing the lowest palette index.
Floyd–Steinberg visits left-to-right, top-to-bottom and discards diffusion error
at image boundaries. This is a deterministic bounded palette policy, not a
perceptual colour-distance model.

```rust
use rich_art::{Dither, ImageArt, ImageColorMode, ImageMode};
let art = ImageArt::from_path("photo.png")?
    .mode(ImageMode::Blocks)
    .width(60)
    .color_mode(ImageColorMode::Ansi256)
    .dither(Dither::FloydSteinberg);
```

The reusable builders live in art; `ImageOptions` remains source-compatible.

## Watch a changing file

```bash
rich json status.json --no-config --watch --watch-interval 0.5
```

Interactive watch clears and repaints the terminal viewport for each changed
frame. It leaves the cursor visible and uses the existing console controls.
Invalid input or a missing file is recoverable. Local regular files are read
with a fixed 64 KiB buffer on every poll so same-size edits and atomic saves
are detected even when timestamps are preserved. For large files, choose a
longer interval to reduce disk I/O. Redirected stdout renders once and exits
without terminal clear codes. See [watch recipes](recipes.md#watch-json-while-editing).

## Use it in a script or CI

`rich` writes rendered output to stdout and diagnostics to stderr, so the two
can be separated:

```bash
rich --csv data.csv > table.txt 2> errors.txt
```

Exit codes are stable by failure class:

| Code | Meaning |
|---|---|
| `0` | Success. With `--diff --threshold`, the change is within the threshold. |
| `2` | Usage/config error, such as an invalid flag or unsupported combination. |
| `3` | Input/read/write error, such as a missing file or failed output write. |
| `4` | Parse/render data error, such as malformed JSON or JSONL. |
| `5` | Threshold/gate failure, such as `--diff --threshold` exceeded. |
| `130` | Batch interrupted with Ctrl+C. Started workers are stopped and reaped. |

Check them:

```bash
if rich --csv "$f" > /dev/null 2>&1; then
  echo "readable"
else
  echo "could not render $f" >&2
fi
```

For automation, `--report json` emits a result/error envelope on stderr while
leaving stdout for rendered content:

```bash
rich --report json jsonl events.ndjson > rendered.txt 2> report.json
```

Successful reports include `ok`, `code`, `exit_code` and a `result` object.
Failures include the same status fields plus `message` and an `error` object.
The top-level `message` is retained for simple shell consumers; structured
consumers can read `error.message`. Informational exits such as `--help` and
`--version` print their normal text and do not emit a report envelope.

`--machine-json` is an alias for `--report json`. Doctor is an informational
exception: its JSON diagnostics are the stdout document; see below.

Colour is disabled automatically when output is not a terminal, and by
a non-empty `NO_COLOR` or `--no-color` when it is. `FORCE_COLOR` is unsupported;
setting it does not add escape sequences to redirected stdout. `COLUMNS` sets
the console width (80 when neither terminal width nor the variable is available).

`--pager` tries a non-empty `MANPAGER`, then `PAGER`, then `less` on Unix or
`more.com` on Windows. `--auto-pager` opts into paging only when stdout is a
terminal and output exceeds its viewport height; redirected stdout is never
paged. `--no-pager` disables configured explicit and automatic paging, while
`--no-auto-pager` disables only automatic paging.

GIF playback repeats once by default; `--loop 0` repeats
forever in a terminal. Pipes receive the first frame once, even with `--loop 0`.

---

The system pager also requires terminal stdin; piped input and `TERM=dumb`
fall back to printing directly. `LINES` sets the viewport height when provided.

## Convert many files at once

`--batch` takes files, directories (walked recursively) and globs, and runs each
one through the same render and export path a single-resource invocation uses:

```bash
rich --batch --markdown --export-html out.html --jobs 4 docs/
rich --batch --json 'reports/*.json' --continue-on-error
```

The plan is computed before anything is written, and it is deterministic: inputs
are sorted and de-duplicated, and each output path is decided up front. Symlinked
directories are not followed, so a link pointing at its own parent cannot make
the walk run forever. Globs apply `*` and `?` to the final path segment only.

Nothing is overwritten silently. If a planned output already exists the run stops
with exit code 3 and tells you to pass `--overwrite`. `--collision suffix` writes
`out-2.html`, `out-3.html`, … instead, stepping past both in-plan duplicates and
files already on disk; `--collision overwrite` (or `--overwrite`) opts in
explicitly.

`--jobs N` bounds concurrent subprocess workers for file exports. Their output
is spooled to temporary files to bound parent buffering, then replayed in input
order. Terminal-only batches stay serial. Default fail-fast stops scheduling new
work after an observed failure; in-flight workers finish. `--continue-on-error`
allows later scheduling. Worker startup and disk I/O mean more jobs do not
guarantee a speedup.

Batch progress shows completed, failed and total counts on stderr only when
stderr is a terminal and the report format is human. `--no-progress` disables
it; `--progress` enables the preference but still respects these destination
and report gates. Redirected stderr and `--report json` never receive progress.
Ctrl+C stops scheduling, kills and waits for started workers, and exits 130.
Machine reporting emits one interrupted envelope. Cancellation is not rollback:
exports already completed may remain; inputs are preserved.

Batch cannot be combined with `--diff`, `--gif`, `--watch`, `--pager` or
`--auto-pager`. Use `--no-pager` to disable configured paging.

Add `--dry-run` to report the plan without creating directories, export files or
worker spools:

```bash
rich --batch --markdown --export-html out.html --dry-run 'docs/tutorial/*.md'
```

Dry-run reports planning errors, including missing destination directories, but
does not render or validate each document's contents.

With `--report json` a batch emits exactly one envelope, and its `code` /
`exit_code` are the most severe class any item reached — so a data error stays 4
rather than collapsing into a generic input failure:

```json
{"ok": false, "code": "data", "exit_code": 4,
 "result": {"planned": 2, "attempted": 2, "completed": 1, "failed": 1, "skipped": 0,
            "failures": [{"resource": "b.json", "code": "data", "exit_code": 4,
                          "message": "invalid JSON: …"}]}}
```

`attempted` counts items the run actually reached and `skipped` those it never
got to after a fail-fast stop, so the numbers stay honest.

## Config profiles

Defaults can live in a `rich.toml` discovered in the working directory or the
platform config directory, or named explicitly with `--config PATH`:

```toml
[defaults]
mode = "markdown"
width = 100

[profiles.ci]
no_color = true
pager = false
collision = "suffix"
```

Select a named profile with `--profile ci`, and ignore every config file with
`--no-config`. Discovery checks `./rich.toml`, then
`$HOME/.config/rich/config.toml` (falling back to `USERPROFILE` when needed).
Both `[profiles.NAME]` and `[profile.NAME]` are accepted; the default selected
name is `default`.

Full TOML syntax is parsed against a strict schema: unknown keys, invalid types
and invalid values fail validation even in inactive profiles. Missing requested
profiles also fail. Defaults are merged first, then the selected profile,
regardless of table order. Explicit CLI values win, and an explicit subcommand
(`rich json file.json`) outranks configured `mode`. Profile `false` values cancel
inherited `true`. Inverse CLI flags such as `--no-watch`, `--no-overwrite`,
`--no-batch`, `--no-continue-on-error`, `--no-sanitize` and `--color` override
configured booleans. Option-looking values and operands after `--` remain data.
Parse errors identify the config file. A quoted `#` remains part of the value.

Inspect and validate without rendering:

```bash
rich config validate --config rich.toml --profile ci
rich config show --config rich.toml --profile ci --width 64
```

Both return JSON. `settings` includes configured values after profile and CLI
overrides, not all built-in CLI defaults. The schema supports image fit/anchor/
background, watch, sanitization, paging and batch options; see the
[workflow recipes](recipes.md) for complete examples. Machine reporting remains
a CLI option (`--report json`), not a config key. Release and validation status
are recorded in the [0.0.9 preparation notes](releases/0.0.9.md).

---

## Reuse named themes

Theme tables map style names to Rich style strings:

```toml
[defaults]
theme = "night"

[themes.night]
notice = "bold cyan"
warning = "bold yellow"
"markdown.h1" = "bold magenta"
```

```bash
rich --config rich.toml --theme night --print '[notice]Ready[/]'
rich --config rich.toml --theme night --theme-style 'notice=bold green' --print '[notice]Ready[/]'
```

Defaults, the selected profile's `theme`, then `--theme NAME` determine the
selected theme. Repeated `--theme-style NAME=STYLE` bindings override configured
bindings. Theme and style names start with an ASCII letter, digit or underscore;
subsequent characters may also be dots or hyphens. All definitions and references
are validated, including inactive profiles and themes. `rich config show` exposes
the selected theme. Batch workers receive the resolved bindings so parallel
exports use the same theme. These are CLI mappings onto the public `rich::Theme`
API; they add no core theme-stack behavior. `--no-color` and `NO_COLOR` still apply.

## Inspect your environment

```bash
rich doctor
rich doctor --report json > doctor.json
rich doctor --config rich.toml --profile ci
```

Doctor reports package/build features, stdout terminal status, dimensions and
colour policy, inferred Sixel support and selected image mode, selected
config/profile and pager choice. It distinguishes detection from inference;
Sixel inference does not prove terminal support. It performs no terminal probes,
URL fetches or pager launches and does not dump the environment. It validates
configuration, so malformed config produces an actionable usage error.

Successful `doctor --report json` writes the diagnostics document to stdout,
not the rendered-content/report split used by rendering commands. Errors retain
the existing usage/error reporting contract. `--no-config` helps diagnose an
invalid local configuration independently.

## Where to go next

- [CLI reference](cli-reference.md) — every option, generated from `--help`
- [Comparing images](image-diff.md) — the `--diff` workflow in depth
- [Troubleshooting](troubleshooting.md) — error messages and what to do about them
- [Parity with Python rich](parity.md) — how close the output is, and where it differs

<a id="reading-utf-16-text-004-development"></a>

## Reading UTF-16 text (0.0.4)

Use `rich notes.txt --encoding utf-16` for a BOM-marked file, or explicitly
select `utf-16le` / `utf-16be` for headerless input. The same option works on
stdin and URLs. See [text encoding](troubleshooting.md#text-encoding) for strict
error handling and unchanged default decoding.

<a id="gif-half-block-rendering-004-development"></a>

## GIF half-block rendering (0.0.4)

```bash
rich --gif animation.gif --gif-mode blocks --width 40 --loop 2
rich animation.gif --gif-mode ascii
rich --gif first.gif second.gif --gif-mode blocks --loop 0
```

`--gif-mode` selects the GIF renderer independently of the image-diff-only
`--image-mode` flag (used by still images and diffs). Existing invocations default to ASCII. Blocks pack two
pixel rows into each terminal cell. GIF decoding retains the existing full-canvas
transparency/disposal handling, and animations keep their individual clocks.

| Destination | Explicit blocks behavior |
|---|---|
| Truecolor terminal | Full-color half-block frames |
| 256-color terminal | Half-blocks with quantized colors |
| 16-color terminal | Half-blocks with reduced color fidelity |
| `NO_COLOR`, `--no-color`, ASCII-only console, or no color capability | ASCII fallback |
| Redirected stdout | One ASCII frame; no animation controls or waiting |

The default loop count is one; `--loop 2` plays twice and `--loop 0` repeats
until interrupted. Normal completion restores the cursor. Ctrl-C terminates
playback promptly but, as with existing ASCII playback, may leave the cursor
hidden; restore it with `printf '\033[?25h'` in a Unix shell. Sixel GIF output
and GIF HTML/SVG export are not supported. Captures below are from real CLI PTY
output, not GIF export support.

Library callers select `.blocks(true).color(true)` on `AnimatedArt`.
`render_frame(index)` honors capabilities; the original `frame(index)` API still
returns ASCII art. Block height is an aspect-preserving cap; ramp/inversion
settings apply to ASCII fallback. Mixed-renderer stages retain per-frame widths.

Sequential frames captured from actual `--gif-mode blocks` CLI playback:

![CLI GIF frame 1](assets/releases/0.0.4-gif-frame0.jpg)

![CLI GIF frame 5](assets/releases/0.0.4-gif-frame4.jpg)

[Playback recordings and reproduction commands](https://github.com/buchochelliq-labs/rs-rich-cli/tree/main/.github/evidence/v0.0.4-gif).

## Optional syntax cache

For repetitive source files, build the CLI with
`cargo build -p rs-rich-cli --release --features syntax-cache`. This feature is
off by default and changes no CLI flags. It reuses parsing work within one
render; varied source files may see no speedup. See the
[measurements](benchmarks.md#004-repeated-source-syntax-results).

Disabling configured watch with `watch = false` or `--no-watch` also suppresses
inherited `watch_interval` and `watch_cache`. Explicitly passing those watch
options without enabling watch remains a usage error.

### Destination capabilities

The CLI snapshots its selected console's capabilities for nested image rendering.
Image HTML/SVG exports render the decoded image for a noninteractive destination;
terminal raster controls are excluded. `doctor --report json` includes
`terminal.provenance` with configured, detected and inferred capability origins.
Library applications can supply an explicit `rich_ext::target::RenderTarget`
without consulting the process environment.

### Rich log presentation

`rich log events.jsonl --log-presentation rich` renders typed fields and themed
severity labels. The default `plain` presentation preserves existing output.
Configuration uses `log_presentation = "rich"`; an explicit flag overrides it.
Messages remain literal, and machine report envelopes are unchanged.

The library's coordinated Live example (`cargo run -p rs-rich-ext --example
live_regions`) demonstrates ordinary messages between independently updated
regions. Resize dimensions are supplied explicitly. Empty viewports suspend
drawing and restore the cursor; growing establishes a fresh bounded area.

### Batch directories and filename templates

```sh
rich --batch --batch-preserve-dirs --batch-input-root input \
  --batch-name-template '{index}-{stem}.{output_ext}' \
  --export-html output --dry-run input
```

With directory preservation or a name template enabled, each export path names
an **output directory**. `{stem}`, `{input_ext}`, `{output_ext}` and one-based
`{index}` supply the complete filename; no extra extension is appended. Double
braces (`{{`/`}}`) escape literal braces. Templates cannot introduce paths.
`--batch-preserve-dirs` requires local inputs under `--batch-input-root`.
Missing directories are listed by dry-run and created once before execution;
dry-run never creates them. Template-only mode requires existing parents.
Existing error/suffix/overwrite policies apply after expansion. Alias checks are
repeated before worker startup; they do not guarantee protection against another
process swapping symlinks during a write. Cancellation retains created directories
and completed exports. Config keys are `batch_preserve_dirs`, `batch_input_root`
and `batch_name_template`; explicit flags override configuration.

### Still-image transforms and exports (0.0.9)

```sh
rich image photo.png --image-mode blocks --image-rotate 90 \
  --image-flip-horizontal --image-grayscale --image-color ansi256 \
  --image-dither bayer4x4 --export-html photo.html --export-svg photo.svg
```

Rotation accepts 0, 90, 180 or 270 clockwise degrees. Flips follow rotation;
`--image-flip-vertical` is also available. Grayscale composites alpha before
conversion and includes contain padding. Transform flags require still-image
mode. Bayer requires ANSI256 ASCII or blocks. Defaults remain unchanged.

Config keys are `image_rotate` (integer), `image_flip_horizontal`,
`image_flip_vertical`, `image_grayscale` (booleans), and `image_dither` (string).
CLI flags override config, including `--no-image-flip-horizontal`,
`--no-image-flip-vertical` and `--no-image-grayscale`. Batch workers inherit the
resolved options. HTML/SVG exports render against an explicit noninteractive
text target; Auto never selects Sixel for an export, and explicit Sixel fails.

### Directory-preserving batch names

```sh
rich --batch input/ --batch-preserve-dirs --batch-input-root input/ \
  --batch-name-template '{index}-{stem}.{output_ext}' --export-html rendered/ --dry-run
rich --batch input/ --batch-preserve-dirs --batch-input-root input/ \
  --batch-name-template '{index}-{stem}.{output_ext}' --export-html rendered/ --jobs 4
rich --batch input/*.json --batch-name-template '{stem}.{output_ext}' --export-svg rendered/
```

Shells expand unquoted globs; directory traversal is performed by the CLI. In
these new naming modes export arguments identify directories. Templates produce
one complete leaf using `{stem}`, `{input_ext}`, `{output_ext}`, and one-based
`{index}`; `{{` and `}}` emit braces. Parent traversal, separators and invalid
platform names fail before rendering. Relative subdirectories come from the
canonical input root. Dry runs report missing directories without creating them.
Collision policies remain `error`, `suffix`, `overwrite`. Destinations cannot alias
an input or escape through symlink parents. Workers revalidate planned paths before
writing. Concurrent hostile filesystem mutation is outside the CLI's guarantees.
The CLI currently requires UTF-8 output paths; it rejects unsupported paths rather
than silently replacing bytes. Legacy flat naming remains unchanged.

### Typed log presentation

`rich log events.jsonl --log-presentation rich` renders typed JSON fields using
`rich-ext::StructuredEvent`. The default `plain` presentation is unchanged.
Library users can attach caller-supplied diagnostics and opt into `log`/`tracing`
adapters without installing a global logger automatically.

![Actual same-source image exports](media/cli-v9-image-transforms.png)

![Actual structured diagnostic and layout export](media/expanded-v9/cli-v9-diagnostics.png)

These are real renderer outputs; reproduce them with
`python scripts/capture_expanded_v9.py --binary target/debug/rich` after building.
Raw HTML/SVG exports, source fixture and provenance accompany the previews.
Braille uses fixed luminance thresholding with 2×4 dot cells; half-block uses top
foreground/bottom background pairs. Tests enumerate all eight Braille positions,
partial transparent cells and odd block heights. Quadrants remain optional future
work; GIF block playback continues to consume the existing block renderer.
