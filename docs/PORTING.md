# Porting map & parity status

This is the module-by-module map from upstream Python `rich` to this Rust port,
plus the porting status of each. It is the lookup table used by both the
`port-module` and `sync-upstream` skills, and the single source of truth for
"what's done".

**Status:** ⬜ not started · 🟡 partial (subset ported) · 🟢 complete
**Parity:** ✅ golden-tested against real `rich` · — none yet

**Last verified:** 2026-09-24, against Python `rich` 15.0.0, at the 0.0.11
release test (core 0.0.7, prepared). The status column is what the golden
fixtures and differential sweeps measured, not an estimate — see
[Parity](parity.md) for the figures.

Mirrored upstream: `rich` **15.0.0** (see [`UPSTREAM.toml`](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/UPSTREAM.toml)).

## Rendering core

| upstream `rich/…`                     | rust `crates/rich/src/…` | status | parity |
|---------------------------------------|--------------------------|:------:|:------:|
| `color.py`, `color_triplet.py`, `_palettes.py`, `palette.py` | `color.rs` | 🟡 | ✅ truecolor + 8-bit + standard |
| `style.py`                            | `style.rs`               | 🟡 | ✅ |
| `cells.py`, `_cell_widths.py`         | `cells.rs`               | 🟢 | ✅ 0 / 127,754 codepoints |
| `segment.py`                          | `segment.rs`             | 🟡 | — |
| `markup.py`                           | `markup.rs`              | 🟡 | ✅ |
| `text.py` (+ justify, overflow)       | `text.rs`                | 🟡 | ✅ |
| `_wrap.py`                            | `wrap.rs`                | 🟢 | ✅ 0 / 30,680 wrap cases |
| `theme.py`, `themes.py`, `default_styles.py` | `theme.rs` | 🟢 | ✅ (theme stack and theme files since core 0.0.6) |
| `terminal_theme.py` | `terminal_theme.rs` | 🟡 | ✅ |
| `console.py` (+ `ConsoleOptions`, `render_lines`) | `console.rs`  | 🟡 | ✅ (+ `no_color.tsv`: colour removal, exports) |
| `protocol.py`, `abc.py`, `_extension.py` | `protocol.rs`         | 🟡 | — |
| `measure.py` (+ `Renderable::measure`, fit, `Measurement.get`) | `measure.rs`       | 🟡 | ✅ `Syntax`/`JSON` measurement golden (`measure.tsv`); container `__rich_measure__` (`measure_renderables.tsv`) |
| `errors.py`                           | `errors.rs`              | 🟡 | — |
| `control.py`                          | `control.rs`             | 🟢 | ✅ |
| `ansi.py`                             | `ansi.rs`                | 🟡 | ✅ |
| `highlighter.py` (Regex/Repr/ISO8601) | `highlighter.rs`         | 🟡 | ✅ |
| `scope.py`, `region.py`, `containers.py` | (tbd)                 | ⬜ | — |

## Widgets & layout

| upstream `rich/…` | rust file | status | parity |
|-------------------|-----------|:------:|:------:|
| `box.py` (all boxes + substitute) | `box.rs` | 🟢 | ✅ |
| `rule.py` | `rule.rs` | 🟡 | ✅ |
| `padding.py` | `padding.rs` | 🟡 | ✅ |
| `panel.py` | `panel.rs` | 🟡 | ✅ |
| `align.py` | `align.rs` | 🟡 | ✅ |
| `constrain.py` | `constrain.rs` | 🟡 | ✅ |
| `table.py` | `table.rs` | 🟡 | ✅ (renderable cells via `Cell`, `ColumnOptions`, markup `str` cells, `__rich_measure__`) |
| `columns.py` | `columns.rs` | 🟡 | ✅ (markup/`Text`/renderable items) |
| `tree.py` | `tree.rs` | 🟡 | ✅ (markup labels, `__rich_measure__`) |
| `layout.py` | `layout.rs` | 🟡 | ✅ |
| `styled.py` | `styled.rs` | 🟢 | ✅ |
| `screen.py` | `screen.rs` | 🟡 | — |
| `progress_bar.py` | `progress_bar.rs` | 🟢 | ✅ `bar_*`, `progress_three`, `progress_bar.tsv` (pulse, ASCII, no-colour) |
| `bar.py` | `bar.rs` | 🟡 | ✅ |

## Live & progress

| upstream `rich/…` | rust file | status | parity |
|-------------------|-----------|:------:|--------|
| `progress.py` | `progress.rs` + `pyformat.rs` | 🟡 | ✅ `progress_time.tsv` step programs (columns incl. `TextColumn`/`RenderableColumn`, fields, pulse, task API, clock, expand, table-column options, `bar_width=None`), `progress_live.tsv` (live stream, transient, disable, non-terminal) |
| `spinner.py`, `_spinners.py` (full table) | `spinner.rs` | 🟡 | ✅ `live_status.tsv` (start at first render, `update`, markup text, console clock, measure) |
| `status.py` | `status.rs` | 🟡 | ✅ `live_status.tsv` (frames, `update`, console clock) |
| `live_render.py` | `live_render.rs` | 🟡 | ✅ `live_status.tsv` (`position_cursor`/`restore_cursor`, style, wrap) |
| `live.py` | `live.rs` | 🟡 | ✅ `progress_live.tsv` (start/refresh/stop stream); auto-refresh timing by unit tests |

## Content renderers

| upstream `rich/…` | rust file | status | notes |
|-------------------|-----------|:------:|-------|
| `syntax.py` | `syntax.rs` | 🟡 | functional via `syntect` (non-parity, DIVERGENCES #18); `__rich_measure__` is parity-tested |
| `markdown.py` | `markdown.rs` | 🟡 | paragraphs/headings/inline/lists/quotes/code/links (both `hyperlinks` modes) + images (including table-cell hoisting and adjacency) + **GFM tables** via `pulldown-cmark`, with inline styling inside cells (golden `markdown_table_inline`); constructor options `justify`/`style` (golden `markdown_options`), `code_theme`/`inline_code_lexer`/`inline_code_theme` (syntect) |
| `json.py` | `json.rs` | 🟡 | ✅ default layout, arbitrary integers, Python float `repr` and overflowing exponents; optional escape-safe layout is off by default (DIVERGENCES §22) |
| `pretty.py` | `pretty.rs` | 🟡 | Rust-native (`Debug` + repr highlight, #19) |
| `repr.py`, `_inspect.py` | resp. | ⬜ | need Rust reflection — see #19 |
| `traceback.py` | `traceback.rs` | 🟡 | Rust-native (error `source()` chain, #19) |
| `_log_render.py` | `log_render.rs` | 🟡 | ✅ `log_render.tsv`; takes a pre-formatted time (DIVERGENCES §19) |
| `logging.py` (log::Log handler) | `rich-ext` `log_handler.rs` | 🟡 | `RichHandler` over the `log`/`tracing` adapters; UTC default time, no rich tracebacks (DIVERGENCES §19) |

## Utilities & platform

| upstream `rich/…` | rust file | status |
|-------------------|-----------|:------:|
| `emoji.py`, `_emoji_codes.py`, `_emoji_replace.py` | `emoji.rs` | 🟡 |
| `filesize.py` | `filesize.rs` | 🟡 |
| `pager.py` | `pager.rs` | 🟡 |
| `prompt.py` | `prompt.rs` | 🟡 |
| `_unicode_data/` (21 version tables, `UNICODE_VERSION`) | `cell_widths.rs` | 🟢 |
| `box.substitute` (legacy/ASCII fallback) | `box.rs` | 🟡 | ✅ |
| `_windows.py`, `_win32_console.py`, `_windows_renderer.py` | `windows/` | ⬜ |
| `jupyter.py`, `file_proxy.py`, `diagnose.py`, `_fileno.py`, `_null_file.py` | resp. | ⬜ |
| `_ratio.py` (`ratio_resolve`) | `ratio.rs` | 🟡 | ✅ |
| `_loop.py`, `_pick.py`, `_stack.py`, `_timer.py` | internal helpers | ⬜ |
| `_export_format.py`, `Console.export_html` | `export.rs` | 🟡 | ✅ |

## `rich-cli` (tool — tracks upstream 1.8.1, see UPSTREAM.toml)

| upstream feature | rust `crates/rich-cli/src/…` | status |
|------------------|------------------------------|:------:|
| arg parsing, plain-file print, capability demo | `main.rs` | 🟡 |
| `--print` / `--markdown` / `--json` / `--syntax` / `--csv` / `--rule`, width + justify, stdin, extension auto-detect | `main.rs` | 🟡 |
| `csv`/`tsv` table render (blue border, numeric-column bold-green, quoted-field parse, `csv.Sniffer`) | `main.rs` | 🟡 sniffer agrees with CPython's on 42/42 samples |
| HTML export (`--export-html`) + SVG export (`--export-svg`) | `main.rs` | 🟡 both done |
| `--panel`/`--padding` decorators (+ title/caption/style), `--ipynb`, URL fetch (`fetch` feature) | `main.rs` | 🟡 done |
| paging (`--pager`) | `pager.rs` + `main.rs` | 🟡 done |
| preferred subcommands (`print`, `markdown`, `syntax`, `json`, `csv`/`tsv`, `ipynb`, `jsonl`, `log`, `gif`, `diff`, `image`, `rule`) while preserving flat flags | `main.rs` | 🟡 done; `image` is a local rich-art convenience, and the tool commands (`inspect`, `ansi explain`, `view`, …) are listed under the conveniences below |
| stable exit-code classes and `--report json` / `--machine-json` result/error envelopes | `main.rs` | 🟡 done |
| JSONL / NDJSON and structured-log streaming from files/stdin | `main.rs` | 🟡 done |
| 0.0.7 binary-boundary `--watch` polling for files and fetch-enabled URLs | `main.rs` | 🟡 done; deliberate CLI convenience |

### Binary-boundary conveniences (not upstream `rich-cli`)

Per [AGENTS.md](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md)
these are recorded because they live in the `rich`
binary rather than in `rich-ext`. Each is command routing, planning or defaulting
only: they compose public `rich` / `rich-ext` APIs and add no renderer, so the
core mirror is untouched and a sync does not have to reconcile them.

| convenience | rust `crates/rich-cli/src/…` | rationale |
|-------------|------------------------------|-----------|
| `--demo` and `--demo-delay` | `demo.rs` + `main.rs` | bounded, offline tour composes existing public renderers and CLI workflows; uses temporary examples and restores terminal state on interruption |
| batch planning and `--dry-run`, with `--jobs` concurrency for file exports | `batch.rs` + `main.rs` | subprocess workers reuse the single-resource renderer; disk-spooled output is replayed in input order; terminal-only batches remain serial |
| strict TOML profiles, inverse booleans, `config show` / `config validate` | `config.rs` + `main.rs` | validated defaults/profile/CLI precedence and JSON inspection compose existing options without changing core; a working-directory `rich.toml` cannot turn colour back on against `NO_COLOR` (the user's config, `--config` and `--color` can) |
| `--auto-pager` and `--no-pager` | `main.rs` | CLI destination/height policy composes public pager APIs; redirected stdout is never paged |
| `--image-anchor` for still-image cover fitting | `main.rs` | routes to public `rich-art::ImageArt::anchor`; crop implementation and `ImageAnchor` remain in art |
| multi-file `--watch` with `--watch-debounce`, `--watch-poll`, `--watch-exit-on-error` (0.0.10, #139) | `watch.rs` + `main.rs` + `config.rs` | `notify` file events on each parent directory, polling fallback; several files repaint as public `rich-ext` `LiveCoordinator` regions; no core change |
| `rich view`, `hex`, `unicode`, `env`, `capture` (0.0.11 WS11) | `viewers.rs` + `main.rs` | compose `rich_ext::{source_view, hex, unicode_inspect, env_inspect}` and core's `AnsiDecoder`; `view` routes to the existing modes by extension and content; no core change |
| `rich capture --redact` and `--redact-pattern` (0.0.11 WS10, #224) | `viewers.rs` + `main.rs` | masks the capture with the public `rich_ext::redact::Redactor` before it is shown, exported or recorded; no core change |
| terminal-control hygiene and input limits for the 0.0.11 commands: `view` and text `diff` sanitize by default (`--no-sanitize` opts out), `capture --sanitize`, inert capture titles and error-message paths, bounded `view`/`hex`/`unicode`/`inspect`/`capture` reads, the capture exit grace, regular-file 1 MiB theme files, and a working-directory `rich.toml` that cannot set `theme_file`, `export_html`, `export_svg` or `sanitize = false` | `controls.rs` + `viewers.rs` + `tools.rs` + `config.rs` + `main.rs` | composes the public `rich_ext::sanitize_terminal_controls`; none of these commands exists upstream, and the upstream modes keep their ESC-preserving default; no core change |
| `rich inspect` / `--inspect` and `--format` (0.0.11 WS4) | `inspect.rs` + `main.rs` | composes `rich_ext::data` parsers, `Explorer`, `select`, `Redaction` and document diff; `--format auto` routes piped or extensionless input to the existing modes; no core change |
| text and patch `rich diff`, with `--side-by-side`, `--context`, `--language` and `--threshold` for text (0.0.11 WS8) | `main.rs` + `tools.rs` | composes `rich_ext::diff` (engine, `DiffView`, patch view); image diffs keep their existing path; exit 5 above the threshold as for images; no core change |
| `rich ansi explain` / `--ansi-explain` (0.0.11 WS9) | `tools.rs` + `main.rs` | composes `rich_ext::ansi_explain` and its `ExplanationView`; no core change |
| `rich bench compare` (0.0.11 WS8) | `tools.rs` + `main.rs` | composes `rich_ext::qa::bench` comparison and its table; exit 5 on a regression; no core change |
| rich-rendered `--help`, `rich <command> --help`, `rich completions`, `rich docs markdown\|man\|config`, `rich config explain\|reference` (0.0.11 WS6) | `cli_spec.rs` + `authoring.rs` + `config.rs` | one `rich_ext::cli_doc::CommandSpec` feeds help, completion scripts, Markdown/man pages and the config reference; a unit test keeps it in step with the hand-written parser; upstream prints click's help, and core is untouched |

The 0.0.8 additions are published; workflow and registry-consumer evidence is
recorded in [release notes](releases/0.0.8.md). Dry-run does not write exports
or parent directories. Parallel fail-fast stops scheduling after observed failures
but lets in-flight workers finish. Config inspection includes configured settings
and explicit overrides, not a materialized list of built-in defaults.


### Image, theme and doctor boundaries (CLI 0.0.9–0.0.11)

| Convenience | Owner | Boundary |
|---|---|---|
| Named TOML themes, `--theme`, `--theme-style` | CLI config + main | Validated data builds public `rich::Theme`; resolved bindings pass to workers; no core theme-stack change |
| Batch export filesystem hardening (#196) | CLI `batch_output.rs` + `batch.rs` | Parent retains directory handles; workers render to private staging; no-follow descendant traversal, exclusive creation and entry replacement; see CLI contract for directory-object authority and metadata semantics |
| Batch progress and Ctrl+C | CLI batch + main | Human-report stderr TTY only; kill/wait workers and exit 130; no core Live/progress behavior changes |
| `--demo-list`, `--demo-section` | CLI demo + main | Routes stable groups of existing renderers; preserves cleanup and finite pipes |
| `rich doctor` | CLI doctor + main | Read-only selected diagnostics; JSON stdout; no terminal probes, network fetch or pager execution |
| `--image-color`, `--image-dither` | CLI routing; art implementation | Public `ImageColorMode`, `Dither`, `ImageArt::color_mode`/`dither`; ASCII/blocks preprocessing in 0.0.9, then quadrants (0.0.10) and Sixel and GIF frames (0.0.11, row below) |
| `--image-color ansi16\|grayscale` (#125) | CLI routing; art `image_color.rs` | `ImageColorMode::Ansi16` (rich `STANDARD_PALETTE`) and `Grayscale` (luma over 16/232–255/231); every dither |
| `--image-mode quadrants` (#199) | CLI routing; art `quadrant.rs` | `ImageMode::Quadrants`, `QuadrantArt`; cheapest of eight two-colour 2×2 partitions; also draws `--diff` heatmaps |
| `--image-fit stretch`, `--image-max-width/height`, `--image-brightness/contrast/gamma` (#126) | CLI routing; art `image_art.rs`, `transform.rs` | `ImageFit::Stretch`, `ImageArt::max_width`/`max_height`, `ImageTransforms` brightness/contrast/gamma in a fixed order |
| `--image-dither atkinson`, `--image-color-distance`, Sixel and `--gif` colour modes (#498) | CLI routing; art `image_color.rs`, `sixel.rs`, `gif.rs` | `Dither::Atkinson`, `ColorDistance::{Rgb, Oklab}`, a fixed-palette indexed Sixel encoder, `AnimatedArt::color_mode`/`dither`/`color_distance`; Braille keeps a documented rejection |
| `--image-background default\|checkerboard` (#126) | CLI routing; art `image_art.rs` and the text backends | `ImageBackground::{Color, TerminalDefault, Checkerboard}`; alpha kept through fitting; unpainted cells for pixels under half opacity |
| `--theme-file`, `theme_file` (#499) | CLI main + config | Reads upstream theme files with the public `rich::Theme::from_file`; layered under config themes and `--theme-style`; no core theme-stack change |

Art owns palette quantisation (ANSI256, ANSI16, grayscale) and the
Floyd–Steinberg, Bayer and Atkinson dithers on the final sampled image, with RGB
or OKLab colour distance. Truecolor/no-dither defaults remain unchanged and
`ImageOptions` remains source-compatible. The CLI only routes flags to these
public APIs; none of them changes core. The rows were validated by the 0.0.9,
0.0.10 and 0.0.11 release tests; see the
[0.0.11 release notes](releases/0.0.11.md).

---

*When you change a module's status, keep this table and the relevant roadmap
issue in sync.*

### 0.0.3 title and notebook follow-up

Panel string labels parse markup/emoji, flatten newlines, expand tabs and measure
visible cells. Rule labels use console markup/emoji handling; Table title/caption
markup retains wrapped lines. Rich 15 goldens cover styled, wide, tiny, multiline
and truncated labels. Text span offsets remain valid when Unicode truncation
replaces a character with padding or an ellipsis. CLI notebooks compose the
upstream cell/output group before applying the shared decorators and alignment.
Windows paging selects `more.com` and has a required native CI launch test.

Expanded CLI 0.0.9 adds opt-in `--log-presentation rich` at the binary boundary.
JSONL conversion composes rich-ext StructuredEvent; the default log formatter and
core LogRender remain unchanged. Optional rich-ext `log`/`tracing` adapters never
install global state. Their external facade dependencies are disabled by default.

`rich-ext::live` owns extension region coordination; faithful core Live remains
unchanged. The coordinator uses core Control encoders, reserves an insertion row
and guard column, rejects supplied control content, and closes a failed session.
The virtual-screen regressions and `scripts/test_live_regions_pty.py` exercise
actual terminal writes, log retention, resize suspension and cursor restoration.

Batch directory preservation and leaf templates are CLI planning conveniences.
They reuse collision checks, worker bounds, ordered replay and cancellation.
Legacy flat export naming and dry-run parent requirements remain unchanged.

The expanded 0.0.9 still-image flags compose public `rich-art` transform and dither
builders at the CLI boundary. Transform order is rotation, H/V flips, optional
composite/grayscale, fit/anchor, sampling, palette processing and glyph selection.
They are not upstream behavior and do not change core renderers or goldens.
