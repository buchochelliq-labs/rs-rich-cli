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
| `theme.py`, `themes.py`, `default_styles.py`, `terminal_theme.py` | `theme.rs` | 🟡 | — |
| `console.py` (+ `ConsoleOptions`, `render_lines`) | `console.rs`  | 🟡 | ✅ |
| `protocol.py`, `abc.py`, `_extension.py` | `protocol.rs`         | 🟡 | — |
| `measure.py` (+ `Renderable::measure`, fit) | `measure.rs`       | 🟡 | ✅ |
| `errors.py`                           | `errors.rs`              | 🟡 | — |
| `control.py`                          | `control.rs`             | ⬜ | — |
| `ansi.py`                             | `ansi.rs`                | ⬜ | — |
| `highlighter.py` (ReprHighlighter)    | `highlighter.rs`         | 🟡 | ✅ |
| `scope.py`, `region.py`, `containers.py` | (tbd)                 | ⬜ | — |

## Widgets & layout

| upstream `rich/…` | rust file | status | parity |
|-------------------|-----------|:------:|:------:|
| `box.py` | `box.rs` | 🟡 | — |
| `rule.py` | `rule.rs` | 🟡 | ✅ |
| `padding.py` | `padding.rs` | 🟡 | ✅ |
| `panel.py` | `panel.rs` | 🟡 | ✅ |
| `align.py` | `align.rs` | 🟡 | ✅ |
| `constrain.py` | `constrain.rs` | 🟡 | ✅ |
| `table.py` | `table.rs` | 🟡 | ✅ |
| `columns.py` | `columns.rs` | 🟡 | ✅ |
| `tree.py` | `tree.rs` | 🟡 | ✅ |
| `layout.py` | `layout.rs` | ⬜ | — |
| `progress_bar.py` | `progress_bar.rs` | 🟡 |
| `bar.py` | `bar.rs` | 🟡 | — |

## Live & progress

| upstream `rich/…` | rust file | status |
|-------------------|-----------|:------:|
| `progress.py` | `progress.rs` | ⬜ |
| `spinner.py`, `_spinners.py` (full table) | `spinner.rs` | 🟡 |
| `status.py` | `status.rs` | ⬜ |
| `live.py`, `live_render.py` | resp. | ⬜ |

## Content renderers

| upstream `rich/…` | rust file | status | notes |
|-------------------|-----------|:------:|-------|
| `syntax.py` | `syntax.rs` | ⬜ | evaluate `syntect` for Pygments-equivalent |
| `markdown.py` | `markdown.rs` | ⬜ | needs a CommonMark parser |
| `json.py` | `json.rs` | 🟡 | ✅ |
| `pretty.py`, `repr.py`, `_inspect.py` | resp. | ⬜ | |
| `traceback.py` | `traceback.rs` | ⬜ | |
| `logging.py`, `_log_render.py` | resp. | ⬜ | |

## Utilities & platform

| upstream `rich/…` | rust file | status |
|-------------------|-----------|:------:|
| `emoji.py`, `_emoji_codes.py`, `_emoji_replace.py` | `emoji.rs` | 🟡 |
| `filesize.py` | `filesize.rs` | 🟡 |
| `prompt.py`, `pager.py`, `screen.py` | resp. | ⬜ |
| `_unicode_data/` | `unicode_data/` | ⬜ |
| `_windows.py`, `_win32_console.py`, `_windows_renderer.py` | `windows/` | ⬜ |
| `jupyter.py`, `file_proxy.py`, `diagnose.py`, `_fileno.py`, `_null_file.py` | resp. | ⬜ |
| `_loop.py`, `_pick.py`, `_ratio.py`, `_stack.py`, `_timer.py` | internal helpers | ⬜ |
| `_export_format.py` (HTML/SVG export) | `export.rs` | ⬜ |

## `rich-cli` (tool, versions separately — 1.8.1)

| upstream feature | rust `crates/rich-cli/src/…` | status |
|------------------|------------------------------|:------:|
| arg parsing, plain-file print, capability demo | `main.rs` | 🟡 |
| `markdown` / `syntax` / `json` / `csv`/`tsv` / `rule` / `panel` / `ipynb` / URL fetch / HTML export | (tbd) | ⬜ |

---

*When you change a module's status, keep this table and the relevant roadmap
issue in sync.*
