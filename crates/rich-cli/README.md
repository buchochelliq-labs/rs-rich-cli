# rich-cli

A Rust port of the [`rich-cli`](https://github.com/Textualize/rich-cli) terminal
toolbox — rich output for files, data and URLs, from the command line.

```bash
rich README.md              # auto-detected and rendered as Markdown
rich data.csv               # rendered as a table
rich notebook.ipynb         # Jupyter notebook, cells and outputs
rich https://example.com    # fetched and syntax-highlighted
rich -p "[bold red]hi[/]"   # console markup
```

Its **version mirrors upstream `rich-cli`** (currently `1.8.1`) — a *different*
project from the `rich` library, which versions separately.

## Render modes

| Flag | Renders |
| --- | --- |
| `-p`, `--print` | the argument as console markup |
| `-m`, `--markdown` | Markdown (headings, lists, quotes, code, links, tables) |
| `-j`, `--json` | pretty-printed, highlighted JSON |
| `-x`, `--syntax` | syntax-highlighted source |
| `--csv` | a CSV/TSV table, with numeric columns right-aligned |
| `--ipynb` | a Jupyter notebook |
| `--gif` | animated GIFs, several at once |
| `--rule` | a horizontal rule |

With no flag the mode is picked from the file extension; a bare `-` reads stdin.

## Options

Layout
: `-w/--width`, `--left`/`--center`/`--right`, `--panel BOX` with
  `--title`/`--caption`/`--style`, `--padding`, `--pager`, `--no-color`.

Export
: `--export-html` and `--export-svg` emit a self-contained document instead of
  writing to the terminal — any render mode can be captured this way.

## Features

Both are on by default and can be dropped for a smaller binary:

- **`fetch`** — URL support (`rich <url>`), via `ureq` with bundled TLS roots.
- **`art`** — `--gif` playback, via [`rich-art`](../rich-art).

```bash
cargo install rich-cli --no-default-features   # no network, no image decoders
```

## Licence

MIT.
