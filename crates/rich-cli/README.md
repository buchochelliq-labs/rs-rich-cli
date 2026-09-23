# rs-rich-cli

The `rs-rich-cli` package is a Rust port of the
[`rich-cli`](https://github.com/Textualize/rich-cli) terminal toolbox — rich
output for files, data and URLs, from the command line. Install it from
crates.io with Cargo; the executable it installs is named `rich`:

```bash
cargo install rs-rich-cli
rich --help
rich --demo  # guided suite tour; Ctrl+C stops
```

```bash
rich README.md              # auto-detected and rendered as Markdown
rich data.csv               # rendered as a table
rich notebook.ipynb         # Jupyter notebook, cells and outputs
rich https://example.com    # fetched and syntax-highlighted
rich -p "[bold red]hi[/]"   # console markup
rich json data.json         # preferred subcommand form
rich jsonl events.ndjson    # streaming JSON Lines / NDJSON
rich log app.jsonl          # structured-log JSONL
```

The source package targets **`0.0.9`** (source preparation; validation pending) and follows independent
SemVer; its version does not mirror Python `rich-cli`. The tracked upstream
release is **`rich-cli` 1.8.1**, recorded in
[`../../UPSTREAM.toml`](../../UPSTREAM.toml).

## Render modes

| Flag | Renders |
| --- | --- |
| `-p`, `--print` | the argument as console markup |
| `-m`, `--markdown` | Markdown (headings, lists, quotes, code, links, tables) |
| `-j`, `--json` | pretty-printed, highlighted JSON |
| `-x`, `--syntax` | syntax-highlighted source |
| `--csv` | a CSV/TSV table, with numeric columns right-aligned |
| `--ipynb` | a Jupyter notebook |
| `--jsonl` | streaming JSON Lines / NDJSON |
| `--log` | streaming structured-log JSONL |
| `--gif` | animated GIFs, several at once |
| `--image` | a still image as ASCII, Braille, half-blocks, or Sixel |
| `--rule` | a horizontal rule |

With no flag the mode is picked from the file extension; a bare `-` reads stdin.
Preferred subcommands such as `rich json`, `rich markdown`, `rich csv`,
`rich jsonl` and `rich log` are aliases over the same renderers. Existing flat
flags remain supported.

## Options

Layout
: `-w/--width`, `--left`/`--center`/`--right`, `--panel BOX` with
  `--title`/`--caption`/`--style`, `--padding`, `--pager`, `--sanitize`,
  `--report json`, `--no-color`. `--auto-pager` pages terminal output only when
  it exceeds the viewport height; `--no-pager` disables configured paging.
  Redirected stdout is never paged.

Export
: `--export-html` and `--export-svg` emit a self-contained document instead of
  writing to the terminal — any render mode can be captured this way.

Watch
: `--watch` re-renders changed local files (several may be given, each in its
  own live region) or one URL. Files use OS file events debounced by
  `--watch-debounce SEC` (default 0.1); `--watch-poll` polls every
  `--watch-interval SEC` instead, and URLs are always polled.
  `--watch-exit-on-error` stops on the first failed render. `--watch-cache`
  avoids re-rendering unchanged URL responses. Watch mode is intentionally
  finite when stdout is redirected: it renders one snapshot and exits, making
  pipelines deterministic. Builds without the `fetch` feature reject URL
  watches with the same stable URL-support error as one-shot URL input.
  Watch cannot be combined with batch or explicit/automatic paging.

Batch
: `--batch` plans local files, directories and globs. `--dry-run` reports the
  plan and planning errors without writing outputs. `--jobs N` bounds subprocess
  workers for file exports; disk-spooled output is replayed in input order.
  Terminal-only batches remain serial. Fail-fast stops new scheduling while
  in-flight workers finish; `--continue-on-error` permits later scheduling.
  Human-report stderr terminals show completed/failed/total progress;
  `--no-progress` disables it. JSON reports and redirected stderr never contain
  progress. Ctrl+C stops scheduling, kills and reaps started workers and exits
  130; completed exports may remain.

Configuration
: `--config PATH` and `--profile NAME` select strict TOML defaults and profiles.
  Precedence is defaults, selected profile, then explicit CLI settings; profile
  `false` and inverse flags can disable inherited booleans. Unknown keys and
  invalid values fail even in inactive profiles. `rich config show` and
  `rich config validate` return JSON; `settings` includes configured values and
  explicit overrides, not every built-in default. `--no-config` disables config.
  `[themes.NAME]` maps style names to styles. Select with defaults/profile `theme`
  or `--theme NAME`; `--theme-style NAME=STYLE` overrides individual bindings.
  Workers inherit resolved bindings for consistent exports.

Still-image crop
: `--image-fit contain|cover` fits the image into an explicit height and bounded
  width. `--image-background '#RRGGBB'` composites transparency. With cover,
  `--image-anchor` selects center (default), top, bottom, left, right or a corner
  such as `top-left`. Contain stays centered.

Image palette
: `--image-color ansi256|ansi16|grayscale` opts ASCII, half-block and quadrant
  still images into a fixed palette. Add `--image-dither floyd-steinberg` or
  `bayer4x4` for dithering. Truecolor/no-dither remains the default. Unsupported
  combinations are rejected; GIF, diff, Braille and Sixel preprocessing are
  outside this feature.

Quadrants and adjustments
: `--image-mode quadrants` draws 2×2 pixels per cell. `--image-fit stretch`
  ignores the aspect ratio; `--image-max-width`/`--image-max-height` cap the size;
  `--image-brightness`, `--image-contrast` and `--image-gamma` (1.0 = unchanged)
  adjust tone before quantization.

Discovery and diagnostics
: `--demo-list` lists `core`, `workflows`, `art`;
  `--demo --demo-section workflows` plays one group. `rich doctor` inspects build,
  terminal, selected config and pager settings without terminal probes, network
  requests or pager execution. Its successful `--report json` document goes to
  stdout. Sixel capability is inferred, not tested.

See the [workflow recipes](https://buchochelliq-labs.github.io/rs-rich-cli/recipes/)
and [0.0.9 preparation notes](https://buchochelliq-labs.github.io/rs-rich-cli/releases/0.0.9/)
for examples and pending release gates. Source versions do not imply publication.

## Features

Both are on by default and can be dropped for a smaller binary:

- **`fetch`** — URL support (`rich <url>`), via `ureq` with bundled TLS roots.
- **`art`** — `--gif` playback and `--diff`/`--image` picture rendering, via [`rich-art`](../rich-art).

```bash
cargo install rs-rich-cli --no-default-features   # installs `rich`; no network or image decoders
```

## Licence

MIT.

## Optional syntax cache

For repetitive source files, build the CLI with
`cargo build -p rs-rich-cli --release --features syntax-cache`. This feature is
off by default and changes no CLI flags. It reuses parsing work within one
render; varied source files may see no speedup. See the
[measurements](https://buchochelliq-labs.github.io/rs-rich-cli/benchmarks/#004-repeated-source-syntax-results).

The expanded 0.0.9 preparation adds `--log-presentation rich`, still-image
rotation/flips/grayscale and `--image-dither bayer4x4`, HTML/SVG still-image exports,
and batch `--batch-preserve-dirs`, `--batch-input-root`, `--batch-name-template`.
Run `rich --help` for accepted values; flags are opt-in. New batch naming modes
treat export paths as directories, while legacy flat export naming is unchanged.
