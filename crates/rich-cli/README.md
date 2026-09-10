# rs-rich-cli

The `rs-rich-cli` package is a Rust port of the
[`rich-cli`](https://github.com/Textualize/rich-cli) terminal toolbox — rich
output for files, data and URLs, from the command line. Install it from
crates.io with Cargo; the executable it installs is named `rich`:

```bash
cargo install rs-rich-cli
rich --help
```

```bash
rich README.md              # auto-detected and rendered as Markdown
rich data.csv               # rendered as a table
rich notebook.ipynb         # Jupyter notebook, cells and outputs
rich https://example.com    # fetched and syntax-highlighted
rich -p "[bold red]hi[/]"   # console markup
```

The Rust package is currently version **`0.0.4`** and follows independent
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
