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

Tools that upstream `rich-cli` does not have:

```bash
rich inspect deploy.yaml --select '$.servers[*].name'   # JSON/YAML/TOML/XML/INI/dotenv as a tree
rich mermaid flow.mmd                                   # a Mermaid flowchart, drawn as text
rich diff old.rs new.rs --side-by-side                  # text diff; `git diff | rich diff -` for patches
rich view src/main.rs --search todo                     # any file, rendered or highlighted, paged
rich hex firmware.bin --offset 0x200 --length 64        # hex dump (alias: hexdump)
rich unicode notes.txt                                  # graphemes, code points, widths
rich env PATH                                           # environment, secrets masked; PATH checked
rich capture --export-svg run.svg -- cargo test         # run a command and show/export its output
rich ansi explain capture.ans                           # decode every escape sequence
rich doctor                                             # what rich detected about this terminal
rich bench compare base.json new.json --threshold 10    # gate on benchmark regressions
rich completions bash                                   # shell completions; `rich docs man` for man pages
rich config explain width                               # where a setting comes from
```

This source is **`0.0.12`**, published to crates.io on 2026-09-26. It follows independent SemVer; its version does
not mirror Python `rich-cli`. The tracked upstream release is **`rich-cli`
1.8.1**, recorded in [`UPSTREAM.toml`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/UPSTREAM.toml).

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
| `--image` | a still image as ASCII, Braille, half-blocks, quadrants, or Sixel |
| `--diff` | two images, two text files, or one patch |
| `--inspect` | structured data as a tree (`rich inspect`) |
| `--ansi-explain` | the escape sequences in a capture (`rich ansi explain`) |
| `--rule` | a horizontal rule |

With no flag the mode is picked from the file extension; a bare `-` reads stdin.
`--format auto` (the default) detects piped or extensionless input, and a named
format (`json`, `yaml`, `toml`, `xml`, `ini`, `env`) overrides the extension.
Preferred subcommands such as `rich json`, `rich markdown`, `rich csv`,
`rich jsonl` and `rich log` are aliases over the same renderers. Mermaid has
only a subcommand, `rich mermaid` (alias `mmd`); `.mmd` and `.mermaid` files
are detected. Existing flat
flags remain supported.

## Options

Layout
: `-w/--width`, `--left`/`--center`/`--right`, `--panel BOX` with
  `--title`/`--caption`/`--style`, `--padding`, `--pager`, `--sanitize`,
  `--report json`, `--no-color`. `--auto-pager` pages terminal output only when
  it exceeds the viewport height; `--no-pager` disables configured paging.
  Redirected stdout is never paged.

Export
: `--export-html PATH` and `--export-svg PATH` also write a document, while the
  output still goes to the terminal — any render mode can be captured this way.
  The HTML is self-contained; the SVG loads its font from a CDN.

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
  Workers inherit resolved bindings for consistent exports. `--theme-file PATH`
  (or `theme_file`) loads the `[styles]` section of an upstream rich theme file.
  `rich config explain [KEY]` shows which layer sets each value, and
  `rich config reference` lists every key.

Still-image crop
: `--image-fit contain|cover` fits the image into an explicit height and bounded
  width. `--image-fit native` needs no height: it draws the image at its own
  pixel size, never enlarged, capped by the width and the terminal. `--image-background '#RRGGBB'` composites transparency; `default`
  leaves it to the terminal's background and `checkerboard` shows it on a gray
  checkerboard. With cover,
  `--image-anchor` selects center (default), top, bottom, left, right or a corner
  such as `top-left`. Contain stays centered.

Code highlighting
: `--highlighter syntect` (the default) or `lumis` (a build with the `lumis`
  feature) picks the highlighter for source, Markdown code, `view` and `diff`.
  `--code-theme NAME` picks one of its themes; every highlighter has
  `ansi_dark` and `ansi_light`, which use the terminal's own 16 colours.
  `rich doctor --report json` lists the highlighters and themes.

Mermaid
: `rich mermaid FILE` and ` ```mermaid ` fences in Markdown draw flowcharts as
  text. `--mermaid-backend mmdc` (a build with the `mmdc` feature) renders every
  diagram type through Mermaid's own CLI, which needs Node and a headless
  browser; `off` leaves fences as code.

Filter and highlight
: `--filter PATTERN` keeps only what matches, and `--highlight PATTERN` marks
  matches in reverse video. For text, `--print` and `--syntax` the pattern is a
  regular expression over lines; with `--inspect` it is a JSONPath.

Image palette
: `--image-color ansi256|ansi16|grayscale` opts ASCII, half-block, quadrant and
  Sixel still images, and GIF frames, into a fixed palette. Add
  `--image-dither floyd-steinberg`, `bayer4x4` or `atkinson` for dithering, and
  `--image-color-distance oklab` to match colours perceptually.
  Truecolor/no-dither remains the default. Unsupported combinations are
  rejected: Braille is monochrome, and image diffs ignore these options.

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

Viewers and capture
: `rich view` sanitises terminal controls by default, as text `rich diff` does
  (`--no-sanitize` opts out). `view`, `hex`, `unicode`, `inspect` and `capture`
  read a bounded amount (see [Limits](https://buchochelliq-labs.github.io/rs-rich-cli/cli/#limits)). `rich capture` exits
  with the command's status; `--cast FILE` records an asciicast, and the
  experimental `--redact` / `--redact-pattern` mask secrets on a best-effort
  basis. `rich COMMAND --help` shows one command's options.

See the [workflow recipes](https://buchochelliq-labs.github.io/rs-rich-cli/recipes/), the
[CLI reference](https://buchochelliq-labs.github.io/rs-rich-cli/cli-reference/) and the
[0.0.12 release notes](https://buchochelliq-labs.github.io/rs-rich-cli/releases/0.0.12/). Source versions do not imply
publication.

## Features

These three are on by default and can be dropped for a smaller binary:

- **`fetch`** — URL support (`rich <url>`), via `ureq` with bundled TLS roots.
- **`art`** — `--gif` playback and `--diff`/`--image` picture rendering, via [`rs-rich-art`](https://crates.io/crates/rs-rich-art).
- **`mermaid`** — `rich mermaid` and Mermaid fences in Markdown, drawn as text, via [`rs-rich-mermaid`](https://crates.io/crates/rs-rich-mermaid).

Off by default:

- **`lumis`** — the tree-sitter highlighter (`--highlighter lumis`), via [`rs-rich-lumis`](https://crates.io/crates/rs-rich-lumis). It compiles many grammars, so the binary is much larger, and it needs Rust 1.91.
- **`mmdc`** — the `mmdc` Mermaid backend. It starts Mermaid's CLI, which must be installed separately.

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

For faster highlighting of any source file, build with `--features onig`. That
uses the Oniguruma regex engine (C, compiled from bundled source, so a C compiler
is needed): 2–4× faster, with the same output. It is off by default.

Since 0.0.9 the CLI also has `--log-presentation rich`, still-image
rotation/flips/grayscale and `--image-dither bayer4x4`, HTML/SVG still-image exports,
and batch `--batch-preserve-dirs`, `--batch-input-root`, `--batch-name-template`.
Run `rich --help` for accepted values; flags are opt-in. New batch naming modes
treat export paths as directories, while legacy flat export naming is unchanged.
