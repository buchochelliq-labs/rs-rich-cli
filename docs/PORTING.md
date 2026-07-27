# Porting map & parity status

This is the module-by-module map from upstream Python `rich` to this Rust port,
plus the porting status of each. It is the lookup table used by both the
`port-module` and `sync-upstream` skills, and the single source of truth for
"what's done".

**Status:** ⬜ not started · 🟡 partial (subset ported) · 🟢 complete
**Parity:** ✅ golden-tested against real `rich` · — none yet

Mirrored upstream: `rich` **15.0.0** (see [`UPSTREAM.toml`](../UPSTREAM.toml)).

## Rendering core

| upstream `rich/…`                     | rust `crates/rich/src/…` | status | parity |
|---------------------------------------|--------------------------|:------:|:------:|
| `color.py`, `color_triplet.py`, `_palettes.py`, `palette.py` | `color.rs` | 🟡 | ✅ |
| `style.py`                            | `style.rs`               | 🟡 | ✅ |
| `cells.py`, `_cell_widths.py`         | `cells.rs`               | 🟡 | — |
| `segment.py`                          | `segment.rs`             | 🟡 | — |
| `markup.py`                           | `markup.rs`              | 🟡 | ✅ |
| `text.py` (+ justify)                 | `text.rs`                | 🟡 | ✅ |
| `_wrap.py`                            | `wrap.rs`                | 🟡 | ✅ |
| `theme.py`, `themes.py`, `default_styles.py` | `theme.rs` | 🟡 | — |
| `terminal_theme.py` | `terminal_theme.rs` | 🟡 | ✅ |
| `console.py` (+ `ConsoleOptions`, `render_lines`) | `console.rs`  | 🟡 | ✅ |
| `protocol.py`, `abc.py`, `_extension.py` | `protocol.rs`         | 🟡 | — |
| `measure.py` (+ `Renderable::measure`, fit) | `measure.rs`       | 🟡 | ✅ |
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
| `table.py` | `table.rs` | 🟡 | ✅ |
| `columns.py` | `columns.rs` | 🟡 | ✅ |
| `tree.py` | `tree.rs` | 🟡 | ✅ |
| `layout.py` | `layout.rs` | 🟡 | ✅ |
| `styled.py` | `styled.rs` | 🟢 | ✅ |
| `screen.py` | `screen.rs` | 🟡 | — |
| `progress_bar.py` | `progress_bar.rs` | 🟡 |
| `bar.py` | `bar.rs` | 🟡 | — |

## Live & progress

| upstream `rich/…` | rust file | status |
|-------------------|-----------|:------:|
| `progress.py` | `progress.rs` | 🟡 |
| `spinner.py`, `_spinners.py` (full table) | `spinner.rs` | 🟡 |
| `status.py` | `status.rs` | 🟡 |
| `live_render.py` | `live_render.rs` | 🟡 |
| `live.py` (manual refresh) | `live.rs` | 🟡 |

## Content renderers

| upstream `rich/…` | rust file | status | notes |
|-------------------|-----------|:------:|-------|
| `syntax.py` | `syntax.rs` | 🟡 | functional via `syntect` (non-parity, DIVERGENCES #18) |
| `markdown.py` | `markdown.rs` | 🟡 | paragraphs/headings/inline via `pulldown-cmark` |
| `json.py` | `json.rs` | 🟡 | ✅ |
| `pretty.py` | `pretty.rs` | 🟡 | Rust-native (`Debug` + repr highlight, #19) |
| `repr.py`, `_inspect.py` | resp. | ⬜ | need Rust reflection — see #19 |
| `traceback.py` | `traceback.rs` | 🟡 | Rust-native (error `source()` chain, #19) |
| `_log_render.py` | `log_render.rs` | 🟡 | Rust-native formatter (#19) |
| `logging.py` (log::Log handler) | `rich-ext` | ⬜ | needs the `log`/`tracing` crate |

## Utilities & platform

| upstream `rich/…` | rust file | status |
|-------------------|-----------|:------:|
| `emoji.py`, `_emoji_codes.py`, `_emoji_replace.py` | `emoji.rs` | 🟡 |
| `filesize.py` | `filesize.rs` | 🟡 |
| `prompt.py`, `pager.py` | resp. | ⬜ |
| `_unicode_data/` | `unicode_data/` | ⬜ |
| `box.substitute` (legacy/ASCII fallback) | `box.rs` | 🟡 | ✅ |
| `_windows.py`, `_win32_console.py`, `_windows_renderer.py` | `windows/` | ⬜ |
| `jupyter.py`, `file_proxy.py`, `diagnose.py`, `_fileno.py`, `_null_file.py` | resp. | ⬜ |
| `_ratio.py` (`ratio_resolve`) | `ratio.rs` | 🟡 | — |
| `_loop.py`, `_pick.py`, `_stack.py`, `_timer.py` | internal helpers | ⬜ |
| `_export_format.py`, `Console.export_html` | `export.rs` | 🟡 | ✅ |

## `rich-cli` (tool, versions separately — 1.8.1)

| upstream feature | rust `crates/rich-cli/src/…` | status |
|------------------|------------------------------|:------:|
| arg parsing, plain-file print, capability demo | `main.rs` | 🟡 |
| `--print` / `--markdown` / `--json` / `--rule`, width + justify, stdin, extension auto-detect | `main.rs` | 🟡 |
| `syntax` / `csv`/`tsv` / `panel` / `padding` / `ipynb` / URL fetch / HTML export | (tbd) | ⬜ |

---

*When you change a module's status, keep this table and the relevant roadmap
issue in sync.*
